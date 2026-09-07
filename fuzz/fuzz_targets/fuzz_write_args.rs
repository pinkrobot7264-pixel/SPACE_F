//! Fuzz write arguments: arbitrary handle, offset, buffer, mode flags (§14.1).
//!
//! Asserts: no panic; INV-RES-2 (exceeding a limit produces the specified error
//! and never a panic, an abort, or an unbounded allocation).

#![no_main]

use libfuzzer_sys::fuzz_target;
use space_client_core::vfs::ids::HandleId;
use space_client_core::vfs::ids::GenId as _;
use space_client_core::vfs::invariants::VfsDiagnostics;
use space_client_core::vfs::memvfs::MemVfs;
use space_client_core::vfs::types::*;
use space_client_core::vfs::{OpCtx, Vfs};

#[derive(arbitrary::Arbitrary, Debug)]
struct Args {
    handle: u64,
    offset: u64,
    data: Vec<u8>,
    write_to_eof: bool,
    constrained_io: bool,
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

    let h = if args.use_real_handle {
        real
    } else {
        HandleId::from_raw(args.handle)
    };
    let mode = WriteMode {
        write_to_eof: args.write_to_eof,
        constrained_io: args.constrained_io,
    };

    let before = vfs.file_info(&cx, real).map(|i| i.file_size).unwrap_or(0);

    match vfs.write(&cx, h, args.offset, &args.data, mode) {
        Ok((n, info)) => {
            assert!(
                n as usize <= args.data.len(),
                "write reported more than was supplied"
            );
            if args.constrained_io && h == real {
                // ConstrainedIo never grows the file.
                assert!(
                    info.file_size <= before,
                    "a constrained write grew the file from {before} to {}",
                    info.file_size
                );
            }
        }
        Err(e) => {
            assert!(
                contracts::ErrorCode::ALL.contains(&e.code),
                "write produced a code outside the taxonomy: {:?}",
                e.code
            );
            assert_ne!(e.code, contracts::ErrorCode::NetworkTimeout, "ADR-0014");
        }
    }

    vfs.check_invariants()
        .expect("invariants after a fuzzed write");
});
