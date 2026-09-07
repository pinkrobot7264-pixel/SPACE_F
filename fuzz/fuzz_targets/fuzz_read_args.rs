//! Fuzz read arguments: arbitrary handle, offset, length (§14.1).
//!
//! Asserts: no panic; INV-ID-3 (a forged handle never resolves); every error is
//! a valid `ErrorCode` and never `NetworkTimeout` (ADR-0014).

#![no_main]

use libfuzzer_sys::fuzz_target;
use space_client_core::vfs::ids::{GenId, HandleId};
use space_client_core::vfs::invariants::VfsDiagnostics;
use space_client_core::vfs::memvfs::MemVfs;
use space_client_core::vfs::types::*;
use space_client_core::vfs::{OpCtx, Vfs};

#[derive(arbitrary::Arbitrary, Debug)]
struct Args {
    handle: u64,
    offset: u64,
    len: u16,
    use_real_handle: bool,
}

fuzz_target!(|args: Args| {
    let vfs = MemVfs::with_defaults();
    let cx = OpCtx::new(std::time::Duration::from_secs(30));

    let real = vfs
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
    vfs.write(&cx, real, 0, b"0123456789", WriteMode::NORMAL)
        .unwrap();

    let h = if args.use_real_handle {
        real
    } else {
        HandleId::from_raw(args.handle)
    };
    let mut buf = vec![0u8; args.len as usize];

    // `offset` is unconstrained, so u64::MAX and overflow cases arrive here.
    match vfs.read(&cx, h, args.offset, &mut buf) {
        Ok(n) => {
            assert!(
                args.use_real_handle || h == real,
                "a forged handle resolved (INV-ID-3)"
            );
            assert!(
                n as usize <= buf.len(),
                "read reported more than the buffer holds"
            );
        }
        Err(e) => {
            assert!(
                contracts::ErrorCode::ALL.contains(&e.code),
                "read produced a code outside the taxonomy: {:?}",
                e.code
            );
            assert_ne!(e.code, contracts::ErrorCode::NetworkTimeout, "ADR-0014");
        }
    }

    vfs.check_invariants()
        .expect("invariants after a fuzzed read");
});
