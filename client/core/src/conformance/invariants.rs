//! Invariants asserted as behaviour rather than as checker branches (§3.5).
//!
//! The checker covers the structural invariants after every `step()`. This
//! module covers the ones that are properties of *behaviour* and cannot be seen
//! by walking the tree: INV-FS-3, INV-FS-5, INV-RES-2, INV-RES-3.

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::Vfs;

use super::util::{cx, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "invariants");

    the_checker_passes_on_a_populated_tree(&c);
    inv_fs_5_rejected_operations_leave_state_unchanged(&c);
    inv_res_2_limits_never_panic(&c);
    a_long_mixed_sequence_preserves_every_invariant(&c);
}

fn the_checker_passes_on_a_populated_tree<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "checker on a populated tree", || {
        let d = c.create_dir("tree");
        c.close(d);
        let d = c.create_dir("tree\\a");
        c.close(d);
        let d = c.create_dir("tree\\b");
        c.close(d);
        for i in 0..10 {
            c.file_with(&format!("tree\\a\\f{i}.txt"), format!("content {i}").as_bytes());
        }
        c.vfs.check_invariants().expect("populated tree");
    });
}

fn inv_fs_5_rejected_operations_leave_state_unchanged<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-FS-5 rejected operations", || {
        c.file_with("fs5.txt", b"original content");
        let d = c.create_dir("fs5dir");
        c.close(d);

        let h = c.open_file("fs5.txt").unwrap();

        // A pile of operations that must all be rejected.
        let _ = c.try_create_file("fs5.txt"); // FileExists
        let _ = c.open_file("fs5-missing.txt"); // FileNotFound
        let _ = c.open_dir("fs5.txt"); // NotADirectory
        let _ = c.vfs.rename(&cx(), h, &c.p("fs5dir"), false); // FileExists
        let _ = c.vfs.write(&cx(), h, u64::MAX, b"x", WriteMode::NORMAL); // overflow
        let _ = c.vfs.read(&cx(), h, u64::MAX, &mut [0u8; 8]); // overflow/EOF
        let _ = c.try_create_file("missing\\deep\\file.txt"); // ObjectPathNotFound
        let _ = c.raw("\\CON"); // ObjectNameInvalid

        c.close(h);

        // Nothing moved.
        assert_eq!(c.read_all("fs5.txt"), b"original content");
        assert!(c.exists("fs5dir"));
        assert!(!c.exists("fs5-missing.txt"));
    });
}

fn inv_res_2_limits_never_panic<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-RES-2 limits never panic", || {
        c.file_with("res2.txt", b"data");
        let h = c.open_file("res2.txt").unwrap();

        // Every limit, hit from a hostile direction. None may panic, abort, or
        // allocate without bound; every one must return a valid ErrorCode.
        let attempts: Vec<Result<(), contracts::SpaceError>> = vec![
            c.vfs
                .read(&cx(), h, u64::MAX, &mut vec![0u8; 4096])
                .map(|_| ()),
            c.vfs
                .write(&cx(), h, u64::MAX - 10, &[0u8; 4096], WriteMode::NORMAL)
                .map(|_| ()),
            c.vfs
                .set_file_size(&cx(), h, u64::MAX, false)
                .map(|_| ()),
            c.vfs
                .set_file_size(&cx(), h, u64::MAX, true)
                .map(|_| ()),
            c.raw(&format!("\\{}", "x".repeat(100_000))).map(|_| ()),
        ];
        for a in attempts {
            if let Err(e) = a {
                assert!(
                    contracts::ErrorCode::ALL.contains(&e.code),
                    "an operation returned a code outside the taxonomy: {:?}",
                    e.code
                );
                assert_ne!(
                    e.code,
                    ErrorCode::NetworkTimeout,
                    "the VFS may never produce NetworkTimeout (ADR-0014)"
                );
            }
        }

        c.close(h);
        assert_eq!(c.read_all("res2.txt"), b"data");
    });
}

fn a_long_mixed_sequence_preserves_every_invariant<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    // The deterministic ancestor of `fuzz_op_sequence` (§14.1): a scripted mix
    // of every operation, with the checker after each one.
    let d = c.create_dir("mixed");
    c.close(d);

    for round in 0..12 {
        let a = format!("mixed\\r{round}a.txt");
        let b = format!("mixed\\r{round}b.txt");
        let sub = format!("mixed\\d{round}");

        step(c.vfs, &format!("mixed round {round} create"), || {
            let h = c.create_dir(&sub);
            c.close(h);
            c.file_with(&a, format!("round {round}").as_bytes());
        });

        step(c.vfs, &format!("mixed round {round} write"), || {
            let h = c.open_file(&a).unwrap();
            c.vfs.write(&cx(), h, 100, b"far out", WriteMode::NORMAL).unwrap();
            c.vfs.write(&cx(), h, 0, b"OVER", WriteMode::NORMAL).unwrap();
            c.vfs.write(&cx(), h, 0, b"append", WriteMode::APPEND).unwrap();
            c.vfs.set_file_size(&cx(), h, 50, false).unwrap();
            c.close(h);
        });

        step(c.vfs, &format!("mixed round {round} rename"), || {
            let h = c.open_file(&a).unwrap();
            c.vfs.rename(&cx(), h, &c.p(&b), false).unwrap();
            c.close(h);
        });

        step(c.vfs, &format!("mixed round {round} enumerate"), || {
            let names = c.list("mixed");
            assert!(names.iter().any(|n| n.ends_with(&format!("r{round}b.txt"))));
        });

        step(c.vfs, &format!("mixed round {round} delete"), || {
            let keeper = c.open_file(&b).unwrap();
            c.delete(&b);
            // Still usable through the surviving handle.
            let mut buf = [0u8; 4];
            let _ = c.vfs.read(&cx(), keeper, 0, &mut buf);
            c.close(keeper);

            let h = c.open_dir(&sub).unwrap();
            c.vfs.can_delete(&cx(), h).unwrap();
            c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
            c.vfs.close(&cx(), h);
        });
    }

    c.vfs
        .check_invariants()
        .expect("invariants after the mixed sequence");
}
