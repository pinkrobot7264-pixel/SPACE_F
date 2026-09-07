//! Fuzz arbitrary `u64` -> `dir_next` / `dir_close` (§14.1).
//!
//! Asserts INV-DIR-3: a closed or never-allocated `CursorId` resolves to a
//! controlled error, **never to another directory's state**.

#![no_main]

use libfuzzer_sys::fuzz_target;
use space_client_core::vfs::ids::{CursorId, GenId};
use space_client_core::vfs::invariants::VfsDiagnostics;
use space_client_core::vfs::memvfs::MemVfs;
use space_client_core::vfs::types::*;
use space_client_core::vfs::{OpCtx, Vfs};

fuzz_target!(|raws: Vec<u64>| {
    let vfs = MemVfs::with_defaults();
    let cx = OpCtx::new(std::time::Duration::from_secs(30));

    // A real directory with a real open cursor, so a forged value has
    // something it could wrongly reach.
    let d = vfs
        .create(
            &cx,
            &vfs.parse_path("\\d").unwrap(),
            CreateOptions {
                create_options: FILE_DIRECTORY_FILE,
                granted_access: 0,
                file_attributes: 0,
                allocation_size: 0,
            },
        )
        .unwrap()
        .handle;

    for i in 0..5 {
        let p = vfs.parse_path(&format!("\\d\\f{i}")).unwrap();
        let h = vfs
            .create(
                &cx,
                &p,
                CreateOptions {
                    create_options: 0,
                    granted_access: 0,
                    file_attributes: 0,
                    allocation_size: 0,
                },
            )
            .unwrap()
            .handle;
        vfs.cleanup(&cx, h, CleanupFlags::NONE);
        vfs.close(&cx, h);
    }

    let live = vfs.dir_open(&cx, d, None, None).unwrap();

    for raw in raws.iter().take(64) {
        let c = CursorId::from_raw(*raw);
        if c == live {
            continue;
        }
        // Must be a controlled error, never another cursor's entries.
        assert!(
            vfs.dir_next(&cx, c).is_err(),
            "forged cursor {raw:#x} resolved"
        );
        vfs.dir_close(&cx, c);
    }

    // The live cursor still enumerates correctly after all that probing.
    let mut count = 0;
    while vfs.dir_next(&cx, live).unwrap().is_some() {
        count += 1;
        assert!(count <= 5 + 2, "enumeration exceeded N + 2 (INV-DIR-1)");
    }
    vfs.dir_close(&cx, live);

    vfs.check_invariants()
        .expect("invariants after fuzzed cursors");
});
