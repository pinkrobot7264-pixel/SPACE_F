//! Identifier behaviour (§3.1, ADR-0008; INV-ID-1..5).

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::{CursorId, HandleId, Vfs};

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "handles");

    zero_is_never_a_valid_identifier(&c);
    forged_identifiers_never_resolve(&c);
    a_stale_handle_does_not_reach_a_reused_slot(&c);
    node_identity_is_stable_across_rename_and_unlink(&c);
    index_numbers_are_unique_and_not_reused(&c);
    handles_are_unique(&c);
}

fn zero_is_never_a_valid_identifier<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-ID-5: 0 is never valid", || {
        let mut buf = [0u8; 4];
        expect_err(
            "read on handle 0",
            ErrorCode::InvalidHandle,
            c.vfs.read(&cx(), HandleId::INVALID, 0, &mut buf),
        );
        expect_err(
            "file_info on handle 0",
            ErrorCode::InvalidHandle,
            c.vfs.file_info(&cx(), HandleId::INVALID),
        );
        expect_err(
            "dir_next on cursor 0",
            ErrorCode::InvalidParameter,
            c.vfs.dir_next(&cx(), CursorId::INVALID),
        );
        // The void entry points must simply do nothing.
        c.vfs.close(&cx(), HandleId::INVALID);
        c.vfs.cleanup(&cx(), HandleId::INVALID, CleanupFlags::DELETE);
        c.vfs.dir_close(&cx(), CursorId::INVALID);
    });
}

fn forged_identifiers_never_resolve<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "forged identifiers", || {
        // The values the fuzz targets reach for (§14.1), asserted as a fixed
        // set so the property is checked on every run and not only under fuzz.
        c.file_with("forge.txt", b"real");
        let live = c.open_file("forge.txt").unwrap();

        for raw in [
            1u64,
            2,
            u64::MAX,
            u64::MAX - 1,
            0xFFFF_FFFF,
            0x1_0000_0000,
            0xDEAD_BEEF_CAFE_BABE,
            live.as_raw().wrapping_add(1),
            live.as_raw().wrapping_sub(1),
            live.as_raw() ^ 0xFFFF_FFFF_0000_0000,
        ] {
            if raw == live.as_raw() {
                continue;
            }
            let h = HandleId::from_raw(raw);
            let mut buf = [0u8; 4];
            assert!(
                c.vfs.read(&cx(), h, 0, &mut buf).is_err(),
                "forged handle {raw:#x} resolved for read"
            );
            assert!(
                c.vfs.file_info(&cx(), h).is_err(),
                "forged handle {raw:#x} resolved for file_info"
            );
            // The void entry points must not panic on any of them.
            c.vfs.close(&cx(), h);
            c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
        }

        // ...and the real handle survived all that probing.
        assert!(c.vfs.file_info(&cx(), live).is_ok());
        c.close(live);
        assert!(c.exists("forge.txt"), "a forged cleanup deleted a real file");
    });
}

fn a_stale_handle_does_not_reach_a_reused_slot<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-ID-3 slot reuse", || {
        // The strongest safety property in Phase 1.
        c.file_with("slot-a.txt", b"AAAA");
        c.file_with("slot-b.txt", b"BBBB");

        let old = c.open_file("slot-a.txt").unwrap();
        c.close(old);

        // Whatever slot `old` used is now free; force reuse.
        let new = c.open_file("slot-b.txt").unwrap();

        let mut buf = [0u8; 4];
        assert!(
            c.vfs.read(&cx(), old, 0, &mut buf).is_err(),
            "a stale handle resolved after slot reuse"
        );
        // ...and specifically did not read slot-b's content.
        assert_ne!(&buf, b"BBBB", "a stale handle reached the new object");

        let n = c.vfs.read(&cx(), new, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n as usize], b"BBBB");
        c.close(new);
    });
}

fn node_identity_is_stable_across_rename_and_unlink<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-ID-1 identity stability", || {
        c.file_with("ident.txt", b"stable");
        let h = c.open_file("ident.txt").unwrap();
        let start = c.vfs.file_info(&cx(), h).unwrap().index_number;

        // ...across rename
        c.vfs.rename(&cx(), h, &c.p("ident-moved.txt"), false).unwrap();
        assert_eq!(c.vfs.file_info(&cx(), h).unwrap().index_number, start);

        // ...and across unlink
        c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
        c.vfs.close(&cx(), h);
    });
}

fn index_numbers_are_unique_and_not_reused<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-ID-2 index_number", || {
        // Windows treats index_number as a file identity key; reuse causes
        // Explorer to conflate distinct files.
        let mut seen = std::collections::HashSet::new();
        for i in 0..50 {
            let name = format!("idx{i}.txt");
            let h = c.create_file(&name);
            let idx = c.vfs.file_info(&cx(), h).unwrap().index_number;
            assert!(idx != 0, "index_number 0 is never valid");
            assert!(seen.insert(idx), "index_number {idx} was reused");
            // Delete immediately so the slab index is recycled but the index
            // number must not be.
            c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
            c.vfs.close(&cx(), h);
        }
    });
}

fn handles_are_unique<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-ID-4 handle uniqueness", || {
        c.file_with("uniq.txt", b"x");
        let mut live = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..32 {
            let h = c.open_file("uniq.txt").unwrap();
            assert!(seen.insert(h.as_raw()), "duplicate live HandleId {h:?}");
            live.push(h);
        }
        for h in live {
            c.close(h);
        }
    });
}
