//! Error-model assertions (ADR-0014, ADR-0015; manual §11.7).
//!
//! Three properties:
//!
//! 1. every callback-reachable `ErrorCode` is produced by at least one
//!    conformance test;
//! 2. **no conformance test ever observes `NetworkTimeout`**;
//! 3. an expired deadline yields `OperationTimeout`.
//!
//! The fourth -- "no VFS method returns a Windows type" -- is asserted by the
//! fact that this module compiles without any Windows crate (§11.2).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use contracts::{ErrorCode, SpaceError};

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::{HandleId, OpCtx, Vfs};

use super::util::{cx, step, Ctx};

/// The codes a Phase 1 callback can reach (ADR-0014, "Filesystem / VFS" row).
///
/// `SharingViolation` and `PermissionDenied` are in the taxonomy but are raised
/// by WinFsp's FSD, not by the VFS (ADR-0012), so they are not expected from
/// this suite. `BufferOverflow` is produced by `GetSecurityByName` at the FFI
/// boundary (§6.2), which is outside the `Vfs` trait.
const VFS_REACHABLE: &[ErrorCode] = &[
    ErrorCode::FileNotFound,
    ErrorCode::ObjectPathNotFound,
    ErrorCode::FileExists,
    ErrorCode::NotADirectory,
    ErrorCode::FileIsADirectory,
    ErrorCode::DirectoryNotEmpty,
    ErrorCode::InvalidParameter,
    ErrorCode::InvalidHandle,
    ErrorCode::ObjectNameInvalid,
    ErrorCode::NameTooLong,
    ErrorCode::EndOfFile,
    ErrorCode::DiskFull,
    ErrorCode::ResourceExhausted,
    ErrorCode::OperationTimeout,
];

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "errors");

    every_reachable_code_is_produced(&c);
    an_expired_deadline_yields_operation_timeout(&c);
    the_vfs_never_produces_a_network_code(&c);
}

fn every_reachable_code_is_produced<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    let mut seen: HashSet<ErrorCode> = HashSet::new();
    let mut record = |r: Result<(), SpaceError>| {
        if let Err(e) = r {
            seen.insert(e.code);
        }
    };

    step(c.vfs, "produce every reachable error code", || {
        c.file_with("e.txt", b"content");
        let d = c.create_dir("edir");
        c.close(d);
        c.file_with("edir\\child.txt", b"x");
        let h = c.open_file("e.txt").unwrap();

        record(c.open_file("missing.txt").map(|_| ())); // FileNotFound
        record(c.open_file("no-dir\\f.txt").map(|_| ())); // ObjectPathNotFound
        record(c.try_create_file("e.txt").map(|_| ())); // FileExists
        record(c.open_dir("e.txt").map(|_| ())); // NotADirectory
        record(
            c.vfs
                .open(
                    &cx(),
                    &c.p("edir"),
                    OpenOptions {
                        create_options: FILE_NON_DIRECTORY_FILE,
                        granted_access: 0,
                    },
                )
                .map(|_| ()),
        ); // FileIsADirectory

        let dh = c.open_dir("edir").unwrap();
        record(c.vfs.can_delete(&cx(), dh)); // DirectoryNotEmpty
        c.close(dh);

        record(
            c.vfs
                .write(&cx(), h, u64::MAX, b"x", WriteMode::NORMAL)
                .map(|_| ()),
        ); // InvalidParameter
        record(c.vfs.file_info(&cx(), HandleId::INVALID).map(|_| ())); // InvalidHandle
        record(c.raw("\\CON").map(|_| ())); // ObjectNameInvalid
        record(c.raw(&format!("\\{}", "a".repeat(40_000))).map(|_| ())); // NameTooLong
        record(c.vfs.read(&cx(), h, 999, &mut [0u8; 4]).map(|_| ())); // EndOfFile
        record(
            c.vfs
                .set_file_size(&cx(), h, c.limits.max_bytes + 1, false)
                .map(|_| ()),
        ); // DiskFull

        // ResourceExhausted: exhaust the cursor table, which is the cheapest of
        // the three resource limits to reach.
        let dh = c.open_dir("edir").unwrap();
        let mut held = Vec::new();
        loop {
            match c.vfs.dir_open(&cx(), dh, None, None) {
                Ok(cur) => held.push(cur),
                Err(e) => {
                    record(Err(e));
                    break;
                }
            }
            if held.len() > c.limits.max_open_cursors + 8 {
                panic!("cursor limit never fired");
            }
        }
        for cur in held {
            c.vfs.dir_close(&cx(), cur);
        }
        c.close(dh);

        // OperationTimeout, from an already-expired deadline.
        let expired = OpCtx::with_deadline(Instant::now() - Duration::from_millis(1));
        record(c.vfs.file_info(&expired, h).map(|_| ()));

        c.close(h);
    });

    let missing: Vec<_> = VFS_REACHABLE.iter().filter(|c| !seen.contains(c)).collect();
    assert!(
        missing.is_empty(),
        "these callback-reachable codes were never produced by the suite: {missing:?}"
    );

    // Property 2, asserted over everything this module observed.
    assert!(
        !seen.contains(&ErrorCode::NetworkTimeout),
        "the VFS produced NetworkTimeout (ADR-0014 forbids it)"
    );
    assert!(!seen.contains(&ErrorCode::NetworkUnavailable));
    assert!(!seen.contains(&ErrorCode::ProtocolViolation));
    assert!(!seen.contains(&ErrorCode::AuthFailed));
}

fn an_expired_deadline_yields_operation_timeout<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "expired deadline", || {
        c.file_with("timeout.txt", b"x");
        let h = c.open_file("timeout.txt").unwrap();
        let expired = OpCtx::with_deadline(Instant::now() - Duration::from_millis(1));

        // Every fallible entry point must report the deadline, not proceed.
        let checks: Vec<(&str, ErrorCode)> = vec![
            ("file_info", c.vfs.file_info(&expired, h).unwrap_err().code),
            (
                "read",
                c.vfs.read(&expired, h, 0, &mut [0u8; 4]).unwrap_err().code,
            ),
            (
                "write",
                c.vfs
                    .write(&expired, h, 0, b"x", WriteMode::NORMAL)
                    .unwrap_err()
                    .code,
            ),
            (
                "volume_info",
                c.vfs.volume_info(&expired).unwrap_err().code,
            ),
            (
                "probe",
                c.vfs.probe(&expired, &c.p("timeout.txt")).unwrap_err().code,
            ),
            (
                "dir_open",
                c.vfs.dir_open(&expired, h, None, None).unwrap_err().code,
            ),
            (
                "can_delete",
                c.vfs.can_delete(&expired, h).unwrap_err().code,
            ),
        ];
        for (what, code) in checks {
            assert_eq!(
                code,
                ErrorCode::OperationTimeout,
                "{what} on an expired deadline gave {code:?}"
            );
        }
        c.close(h);
    });
}

fn the_vfs_never_produces_a_network_code<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "no network codes (ADR-0014)", || {
        // A local filesystem operation that is slow is OperationTimeout. A
        // remote request that is slow would be NetworkTimeout -- and the VFS
        // layer may never produce it, now or in Phase 4, because the
        // Windows-facing layer does not care why something was slow.
        assert!(
            !VFS_REACHABLE.contains(&ErrorCode::NetworkTimeout),
            "NetworkTimeout must not be in the VFS-reachable set"
        );
        assert!(ErrorCode::OperationTimeout.retryable());
        assert_eq!(
            ErrorCode::OperationTimeout.origin(),
            contracts::Origin::Client
        );
    });
}
