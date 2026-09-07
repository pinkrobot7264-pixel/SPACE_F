//! Fuzz arbitrary `u64` -> handle resolve / free (§14.1).
//!
//! Asserts INV-ID-3 (a stale or forged handle never resolves) and INV-ID-5
//! (no identifier derives from an address; `0` is never valid).

#![no_main]

use libfuzzer_sys::fuzz_target;
use space_client_core::vfs::ids::{GenId, HandleId};
use space_client_core::vfs::invariants::VfsDiagnostics;
use space_client_core::vfs::memvfs::MemVfs;
use space_client_core::vfs::types::*;
use space_client_core::vfs::{OpCtx, Vfs};

fuzz_target!(|raws: Vec<u64>| {
    let vfs = MemVfs::with_defaults();
    let cx = OpCtx::new(std::time::Duration::from_secs(30));

    let live = vfs
        .create(
            &cx,
            &vfs.parse_path("\\f.bin").unwrap(),
            CreateOptions {
                create_options: 0,
                granted_access: 0,
                file_attributes: 0,
                allocation_size: 0,
            },
        )
        .unwrap()
        .handle;
    vfs.write(&cx, live, 0, b"canary", WriteMode::NORMAL).unwrap();

    for raw in raws.iter().take(64) {
        let h = HandleId::from_raw(*raw);
        if h == live {
            continue;
        }

        // None of these may panic, and none may resolve.
        assert!(
            vfs.file_info(&cx, h).is_err(),
            "forged handle {raw:#x} resolved"
        );
        let mut buf = [0u8; 8];
        assert!(vfs.read(&cx, h, 0, &mut buf).is_err());
        assert!(vfs.can_delete(&cx, h).is_err());
        assert!(vfs.dir_open(&cx, h, None, None).is_err());
        // The void entry points must be no-ops, not crashes.
        vfs.cleanup(&cx, h, CleanupFlags::DELETE);
        vfs.close(&cx, h);
    }

    // The canary survived every probe: no forged handle reached live state.
    let mut buf = [0u8; 6];
    let n = vfs
        .read(&cx, live, 0, &mut buf)
        .expect("the live handle must still work");
    assert_eq!(
        &buf[..n as usize],
        b"canary",
        "a forged handle mutated live state"
    );

    vfs.check_invariants()
        .expect("invariants after fuzzed handles");
});
