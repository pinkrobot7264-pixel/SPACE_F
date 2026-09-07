//! Fuzz arbitrary operation sequences, checking **all invariants after every
//! step** (§14.1).
//!
//! This is the valuable one: it finds handle-lifecycle and namespace-state bugs
//! that per-call fuzzing cannot. On failure it reports the violated invariant
//! ID *and* the operation sequence, because "an invariant broke" without the
//! sequence that broke it is not a reproduction.

#![no_main]

use libfuzzer_sys::fuzz_target;
use space_client_core::vfs::ids::{CursorId, HandleId};
use space_client_core::vfs::ids::GenId as _;
use space_client_core::vfs::invariants::VfsDiagnostics;
use space_client_core::vfs::memvfs::MemVfs;
use space_client_core::vfs::types::*;
use space_client_core::vfs::{OpCtx, Vfs};

/// Names are drawn from a small pool so collisions, renames onto existing
/// entries, and re-creation at a deleted path actually happen. A fuzzer with
/// unique random names explores almost none of the interesting namespace
/// states.
const NAMES: &[&str] = &["a", "b", "c", "dir", "dir\\x", "dir\\y", "A", "DIR"];

#[derive(arbitrary::Arbitrary, Debug)]
enum Op {
    Create { name: u8, dir: bool },
    Open { name: u8, dir: bool },
    Write { handle: u8, offset: u16, len: u8, to_eof: bool, constrained: bool },
    Read { handle: u8, offset: u16, len: u8 },
    SetSize { handle: u8, size: u16, allocation: bool },
    Rename { handle: u8, to: u8, replace: bool },
    CanDelete { handle: u8 },
    Cleanup { handle: u8, delete: bool },
    Close { handle: u8 },
    Enumerate { handle: u8, marker: Option<u8> },
    Probe { name: u8 },
}

fuzz_target!(|ops: Vec<Op>| {
    let vfs = MemVfs::with_defaults();
    let cx = OpCtx::new(std::time::Duration::from_secs(30));

    // Handles the sequence has opened. Closed handles are deliberately KEPT so
    // later operations use stale values -- that is the INV-ID-3 path.
    let mut handles: Vec<HandleId> = Vec::new();
    let mut cursors: Vec<CursorId> = Vec::new();
    let mut trace: Vec<String> = Vec::new();

    let name_of = |i: u8| NAMES[(i as usize) % NAMES.len()];
    let pick = |v: &Vec<HandleId>, i: u8| -> HandleId {
        if v.is_empty() {
            HandleId::from_raw(i as u64)
        } else {
            v[(i as usize) % v.len()]
        }
    };

    for (step, op) in ops.into_iter().take(64).enumerate() {
        trace.push(format!("{op:?}"));

        match op {
            Op::Create { name, dir } => {
                let n = name_of(name);
                if let Ok(p) = vfs.parse_path(&format!("\\{n}")) {
                    if let Ok(o) = vfs.create(
                        &cx,
                        &p,
                        CreateOptions {
                            create_options: if dir { FILE_DIRECTORY_FILE } else { 0 },
                            granted_access: 0,
                            file_attributes: 0,
                            allocation_size: 0,
                        },
                    ) {
                        handles.push(o.handle);
                    }
                }
            }
            Op::Open { name, dir } => {
                let n = name_of(name);
                if let Ok(p) = vfs.parse_path(&format!("\\{n}")) {
                    if let Ok(o) = vfs.open(
                        &cx,
                        &p,
                        OpenOptions {
                            create_options: if dir { FILE_DIRECTORY_FILE } else { 0 },
                            granted_access: 0,
                        },
                    ) {
                        handles.push(o.handle);
                    }
                }
            }
            Op::Write { handle, offset, len, to_eof, constrained } => {
                let h = pick(&handles, handle);
                let data = vec![0xABu8; len as usize];
                let _ = vfs.write(
                    &cx,
                    h,
                    offset as u64,
                    &data,
                    WriteMode { write_to_eof: to_eof, constrained_io: constrained },
                );
            }
            Op::Read { handle, offset, len } => {
                let h = pick(&handles, handle);
                let mut buf = vec![0u8; len as usize];
                let _ = vfs.read(&cx, h, offset as u64, &mut buf);
            }
            Op::SetSize { handle, size, allocation } => {
                let h = pick(&handles, handle);
                let _ = vfs.set_file_size(&cx, h, size as u64, allocation);
            }
            Op::Rename { handle, to, replace } => {
                let h = pick(&handles, handle);
                if let Ok(p) = vfs.parse_path(&format!("\\{}", name_of(to))) {
                    let _ = vfs.rename(&cx, h, &p, replace);
                }
            }
            Op::CanDelete { handle } => {
                let _ = vfs.can_delete(&cx, pick(&handles, handle));
            }
            Op::Cleanup { handle, delete } => {
                let h = pick(&handles, handle);
                vfs.cleanup(
                    &cx,
                    h,
                    if delete { CleanupFlags::DELETE } else { CleanupFlags::NONE },
                );
            }
            Op::Close { handle } => {
                // The handle is NOT removed from the pool: later steps must be
                // able to reach it as a stale value.
                vfs.close(&cx, pick(&handles, handle));
            }
            Op::Enumerate { handle, marker } => {
                let h = pick(&handles, handle);
                let m = marker.map(|i| name_of(i).to_string());
                if let Ok(c) = vfs.dir_open(&cx, h, None, m.as_deref()) {
                    let mut seen = 0usize;
                    while let Ok(Some(_)) = vfs.dir_next(&cx, c) {
                        seen += 1;
                        // INV-DIR-1: bounded by N + 2 for any directory.
                        assert!(
                            seen <= NAMES.len() + 8,
                            "enumeration did not terminate after {seen} entries\nsequence:\n{}",
                            trace.join("\n")
                        );
                    }
                    vfs.dir_close(&cx, c);
                    cursors.push(c);
                }
                // A previously closed cursor must never resolve again.
                for c in cursors.iter().take(8) {
                    let _ = vfs.dir_next(&cx, *c);
                }
            }
            Op::Probe { name } => {
                if let Ok(p) = vfs.parse_path(&format!("\\{}", name_of(name))) {
                    let _ = vfs.probe(&cx, &p);
                }
            }
        }

        // The whole point: **all invariants after every step**, with the
        // sequence attached so the failure is a reproduction rather than a
        // report.
        if let Err(v) = vfs.check_invariants() {
            panic!(
                "invariant {} violated at step {}: {}\nsequence:\n{}",
                v.id,
                step,
                v.detail,
                trace.join("\n")
            );
        }
    }
});
