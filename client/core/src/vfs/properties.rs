//! Property tests (Phase 1 §14.2).
//!
//! `proptest`, against the trait, **no mount required**. Every failing case
//! prints its seed (the M0.10 harness rule); `proptest` writes the failing
//! input to `proptest-regressions/` so it becomes a permanent regression case.

#![cfg(test)]

use proptest::prelude::*;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::{Limits, PathLimits};
use crate::vfs::memvfs::MemVfs;
use crate::vfs::types::*;
use crate::vfs::{OpCtx, Vfs, VfsPath};

fn cx() -> OpCtx {
    OpCtx::new(std::time::Duration::from_secs(30))
}

fn vfs() -> MemVfs {
    MemVfs::new(VfsConfig {
        limits: Limits {
            max_bytes: 16 * 1024 * 1024,
            max_open_handles: 256,
            max_open_cursors: 32,
            max_dir_entries: 512,
            max_io_bytes: crate::vfs::limits::MAX_IO_BYTES,
        },
        path_limits: PathLimits::DEFAULT,
        capabilities: Capabilities::PHASE_1,
    })
}

fn create(v: &MemVfs, path: &str) -> crate::vfs::HandleId {
    let p = v.parse_path(path).unwrap();
    v.create(
        &cx(),
        &p,
        CreateOptions {
            create_options: 0,
            granted_access: 0,
            file_attributes: 0,
            allocation_size: 0,
        },
    )
    .unwrap()
    .handle
}

fn mkdir(v: &MemVfs, path: &str) {
    let p = v.parse_path(path).unwrap();
    let h = v
        .create(
            &cx(),
            &p,
            CreateOptions {
                create_options: FILE_DIRECTORY_FILE,
                granted_access: 0,
                file_attributes: 0,
                allocation_size: 0,
            },
        )
        .unwrap()
        .handle;
    v.cleanup(&cx(), h, CleanupFlags::NONE);
    v.close(&cx(), h);
}

fn close(v: &MemVfs, h: crate::vfs::HandleId) {
    v.cleanup(&cx(), h, CleanupFlags::NONE);
    v.close(&cx(), h);
}

/// A name that always parses, so the property is about the operation and not
/// about name validation (which `naming.rs` covers exhaustively).
fn safe_name() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,20}".prop_map(|s| s)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        // The seed of a failing case is printed and persisted by proptest, so a
        // failure is reproducible rather than a story about a flake.
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource(
            "proptest-regressions"
        ))),
        ..ProptestConfig::default()
    })]

    /// write -> read returns the same bytes.
    #[test]
    fn write_then_read_returns_the_same_bytes(
        offset in 0u64..8192,
        content in prop::collection::vec(any::<u8>(), 0..4096),
    ) {
        let v = vfs();
        let h = create(&v, "\\p.bin");

        if content.is_empty() {
            let (n, _) = v.write(&cx(), h, offset, &content, WriteMode::NORMAL).unwrap();
            prop_assert_eq!(n, 0);
        } else {
            let (n, info) = v.write(&cx(), h, offset, &content, WriteMode::NORMAL).unwrap();
            prop_assert_eq!(n as usize, content.len());
            prop_assert_eq!(info.file_size, offset + content.len() as u64);

            let mut back = vec![0u8; content.len()];
            let got = v.read(&cx(), h, offset, &mut back).unwrap();
            prop_assert_eq!(got as usize, content.len());
            prop_assert_eq!(&back, &content);
        }

        close(&v, h);
        prop_assert!(v.check_invariants().is_ok());
    }

    /// write -> size equals max(old_size, offset + length) unless ConstrainedIo.
    #[test]
    fn write_extends_the_file_to_offset_plus_length(
        first in prop::collection::vec(any::<u8>(), 1..1024),
        offset in 0u64..4096,
        second in prop::collection::vec(any::<u8>(), 1..1024),
    ) {
        let v = vfs();
        let h = create(&v, "\\p.bin");

        let (_, i1) = v.write(&cx(), h, 0, &first, WriteMode::NORMAL).unwrap();
        let old_size = i1.file_size;

        let (_, i2) = v.write(&cx(), h, offset, &second, WriteMode::NORMAL).unwrap();
        let expected = old_size.max(offset + second.len() as u64);
        prop_assert_eq!(i2.file_size, expected);

        close(&v, h);
        prop_assert!(v.check_invariants().is_ok());
    }

    /// ConstrainedIo never grows the file.
    #[test]
    fn constrained_io_never_grows_the_file(
        initial in prop::collection::vec(any::<u8>(), 1..2048),
        offset in 0u64..4096,
        payload in prop::collection::vec(any::<u8>(), 1..1024),
    ) {
        let v = vfs();
        let h = create(&v, "\\p.bin");
        let (_, i1) = v.write(&cx(), h, 0, &initial, WriteMode::NORMAL).unwrap();
        let size_before = i1.file_size;

        let (n, i2) = v.write(&cx(), h, offset, &payload, WriteMode::CONSTRAINED).unwrap();
        prop_assert_eq!(i2.file_size, size_before, "ConstrainedIo grew the file");
        prop_assert!(n as u64 <= payload.len() as u64);

        close(&v, h);
        prop_assert!(v.check_invariants().is_ok());
    }

    /// rename A -> B -> A leaves the tree identical (INV-NS-2, INV-ID-1).
    #[test]
    fn rename_round_trip_leaves_the_tree_identical(
        a in safe_name(),
        b in safe_name(),
    ) {
        prop_assume!(a.to_lowercase() != b.to_lowercase());

        let v = vfs();
        let h = create(&v, &format!("\\{a}"));
        let index_before = v.file_info(&cx(), h).unwrap().index_number;

        v.rename(&cx(), h, &v.parse_path(&format!("\\{b}")).unwrap(), false).unwrap();
        v.rename(&cx(), h, &v.parse_path(&format!("\\{a}")).unwrap(), false).unwrap();

        prop_assert_eq!(v.file_info(&cx(), h).unwrap().index_number, index_before);
        close(&v, h);

        // Bound outside the macro: `prop_assert!` expands through `concat!`, so
        // an inline `{a}` capture in a nested `format!` cannot see the binding.
        let path_a = v.parse_path(&format!("\\{a}")).unwrap();
        let path_b = v.parse_path(&format!("\\{b}")).unwrap();
        prop_assert!(v.probe(&cx(), &path_a).is_ok());
        prop_assert!(v.probe(&cx(), &path_b).is_err());
        prop_assert!(v.check_invariants().is_ok());
    }

    /// INV-DIR-2: after creating N files, enumeration with **any** buffer size
    /// returns exactly N entries, no duplicates, no omissions.
    ///
    /// This is the executable form of the marker bug that silently truncates
    /// large directories.
    #[test]
    fn enumeration_is_complete_for_any_buffer_size(
        n in 0usize..40,
        buf_size in 1usize..12,
    ) {
        let v = vfs();
        mkdir(&v, "\\d");
        for i in 0..n {
            let h = create(&v, &format!("\\d\\f{i:03}"));
            close(&v, h);
        }

        let dh = v.open(&cx(), &v.parse_path("\\d").unwrap(), OpenOptions {
            create_options: FILE_DIRECTORY_FILE,
            granted_access: 0,
        }).unwrap().handle;

        // Chained enumeration in batches of buf_size, exactly as WinFsp drives
        // ReadDirectory with a fixed buffer.
        let mut seen: Vec<String> = Vec::new();
        let mut marker: Option<String> = None;
        let mut rounds = 0usize;
        loop {
            rounds += 1;
            prop_assert!(rounds < n + 10, "enumeration did not terminate");

            let c = v.dir_open(&cx(), dh, None, marker.as_deref()).unwrap();
            let mut batch = Vec::new();
            while batch.len() < buf_size {
                match v.dir_next(&cx(), c).unwrap() {
                    Some(e) => batch.push(e.name),
                    None => break,
                }
            }
            v.dir_close(&cx(), c);

            if batch.is_empty() { break; }
            marker = Some(batch.last().unwrap().clone());
            seen.extend(batch);
        }

        // "." and ".." plus the n children, each exactly once.
        prop_assert_eq!(seen.len(), n + 2, "wrong entry count at buffer size {}", buf_size);
        let mut sorted = seen.clone();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        prop_assert_eq!(before, sorted.len(), "duplicate entries");

        close(&v, dh);
        prop_assert!(v.check_invariants().is_ok());
    }

    /// INV-DIR-1: enumeration halts within N + 2 steps for any buffer size.
    #[test]
    fn enumeration_halts_within_n_plus_2(n in 0usize..30) {
        let v = vfs();
        mkdir(&v, "\\d");
        for i in 0..n {
            let h = create(&v, &format!("\\d\\f{i:03}"));
            close(&v, h);
        }
        let dh = v.open(&cx(), &v.parse_path("\\d").unwrap(), OpenOptions {
            create_options: FILE_DIRECTORY_FILE,
            granted_access: 0,
        }).unwrap().handle;

        let c = v.dir_open(&cx(), dh, None, None).unwrap();
        let mut count = 0usize;
        while v.dir_next(&cx(), c).unwrap().is_some() {
            count += 1;
            prop_assert!(count <= n + 2, "exceeded N + 2 entries");
        }
        prop_assert_eq!(count, n + 2);
        v.dir_close(&cx(), c);
        close(&v, dh);
    }

    /// No two live handles share an ID; a freed ID never resolves
    /// (INV-ID-3, INV-ID-4).
    #[test]
    fn handles_are_unique_and_freed_ids_never_resolve(n in 1usize..40) {
        use std::collections::HashSet;

        let v = vfs();
        let seed = create(&v, "\\shared");
        close(&v, seed);

        let mut live = Vec::new();
        let mut ids = HashSet::new();
        for _ in 0..n {
            let h = v.open(&cx(), &v.parse_path("\\shared").unwrap(), OpenOptions {
                create_options: 0,
                granted_access: 0,
            }).unwrap().handle;
            prop_assert!(ids.insert(h.as_raw()), "duplicate live handle");
            live.push(h);
        }

        let freed = live.pop().unwrap();
        close(&v, freed);
        prop_assert!(v.file_info(&cx(), freed).is_err(), "a freed handle resolved");

        for h in live {
            prop_assert!(v.file_info(&cx(), h).is_ok());
            close(&v, h);
        }
        prop_assert!(v.check_invariants().is_ok());
    }

    /// `parse(display(parse(p))) == parse(p)`.
    #[test]
    fn path_display_round_trips(components in prop::collection::vec(safe_name(), 1..8)) {
        let raw = format!("\\{}", components.join("\\"));
        let p = VfsPath::parse(&raw).unwrap();
        let again = VfsPath::parse(&p.to_string()).unwrap();
        prop_assert_eq!(p, again);
    }

    /// Any input at all: no panic, no invariant violation.
    ///
    /// The trait-level companion to the fuzz targets -- it runs on every `cargo
    /// test`, so the property is checked continuously and not only during a
    /// fuzzing session.
    #[test]
    fn arbitrary_input_never_panics_or_breaks_an_invariant(
        raw_path in ".{0,64}",
        raw_handle in any::<u64>(),
        offset in any::<u64>(),
        len in 0usize..256,
    ) {
        use crate::vfs::ids::{CursorId, HandleId};

        let v = vfs();
        // Path parsing must never panic on arbitrary text.
        let _ = v.parse_path(&raw_path);

        // Arbitrary handles and cursors must never resolve or panic.
        let h = HandleId::from_raw(raw_handle);
        let mut buf = vec![0u8; len];
        let _ = v.read(&cx(), h, offset, &mut buf);
        let _ = v.write(&cx(), h, offset, &buf, WriteMode::NORMAL);
        let _ = v.file_info(&cx(), h);
        let _ = v.set_file_size(&cx(), h, offset, false);
        let _ = v.can_delete(&cx(), h);
        let _ = v.dir_open(&cx(), h, None, None);
        v.cleanup(&cx(), h, CleanupFlags::DELETE);
        v.close(&cx(), h);

        let c = CursorId::from_raw(raw_handle);
        let _ = v.dir_next(&cx(), c);
        v.dir_close(&cx(), c);

        prop_assert!(v.check_invariants().is_ok());
    }
}
