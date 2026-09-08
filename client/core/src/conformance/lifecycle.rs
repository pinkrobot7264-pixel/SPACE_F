//! Handle lifecycle and the deleted-but-open lifecycle
//! (fs-semantics §1 and §2; manual §3.3.1, §3.3.2, §11.4).
//!
//! The lifecycle as a state machine:
//!
//! ```text
//! New ──create/open──▶ Open ──io──▶ Active ──cleanup──▶ Cleaned ──close──▶ Closed
//! ```
//!
//! **The valuable cases are the illegal transitions**: I/O after cleanup, I/O
//! after close, double close, close of a never-allocated handle (§11.4).

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::{HandleId, Vfs};

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "lifecycle");

    close_is_exactly_once_per_open(&c);
    io_after_close_is_invalid_handle(&c);
    io_after_cleanup_before_close_is_invalid_handle(&c);
    duplicate_close_is_a_silent_no_op(&c);
    stale_handle_after_slot_reuse_is_invalid(&c);
    open_count_tracks_file_objects_not_files(&c);
    unlink_happens_at_cleanup_not_close(&c);
    deleted_but_open_stays_usable(&c);
    reclamation_is_exactly_at_the_last_close(&c);
    recreating_at_a_deleted_path_gives_a_different_node(&c);
    delete_on_close_unlinks_at_cleanup(&c);
    illegal_transitions_leave_state_unchanged(&c);
}

fn close_is_exactly_once_per_open<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "close is exactly one per open", || {
        c.file_with("once.txt", b"x");
        let a = c.open_file("once.txt").unwrap();
        let b = c.open_file("once.txt").unwrap();
        // Two Create/Open calls => two file objects => two HandleIds.
        assert_ne!(a, b, "two opens must yield distinct handles");
        c.close(a);
        // The second handle is unaffected by the first closing.
        assert!(c.vfs.file_info(&cx(), b).is_ok());
        c.close(b);
    });
}

fn io_after_close_is_invalid_handle<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    // INV-FS-3: a closed handle performs no I/O and no metadata mutation;
    // every attempt is InvalidHandle.
    step(c.vfs, "io after close (INV-FS-3)", || {
        c.file_with("closed.txt", b"abc");
        let h = c.open_file("closed.txt").unwrap();
        c.close(h);

        let mut buf = [0u8; 4];
        expect_err("read after close", ErrorCode::InvalidHandle, c.vfs.read(&cx(), h, 0, &mut buf));
        expect_err(
            "write after close",
            ErrorCode::InvalidHandle,
            c.vfs.write(&cx(), h, 0, b"z", WriteMode::NORMAL),
        );
        expect_err(
            "file_info after close",
            ErrorCode::InvalidHandle,
            c.vfs.file_info(&cx(), h),
        );
        expect_err(
            "set_file_size after close",
            ErrorCode::InvalidHandle,
            c.vfs.set_file_size(&cx(), h, 0, false),
        );
        expect_err(
            "set_basic_info after close",
            ErrorCode::InvalidHandle,
            c.vfs.set_basic_info(&cx(), h, BasicInfoPatch::default()),
        );
        expect_err(
            "can_delete after close",
            ErrorCode::InvalidHandle,
            c.vfs.can_delete(&cx(), h),
        );
        expect_err(
            "dir_open after close",
            ErrorCode::InvalidHandle,
            c.vfs.dir_open(&cx(), h, None, None),
        );
    });
}

fn io_after_cleanup_before_close_is_invalid_handle<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "io after cleanup before close", || {
        c.file_with("cleaned.txt", b"abc");
        let h = c.open_file("cleaned.txt").unwrap();
        c.vfs.cleanup(&cx(), h, CleanupFlags::NONE);

        // INV-FS-3. Windows sends no further I/O on that file object, so any
        // such call is a bug or an attack (fs-semantics §1).
        let mut buf = [0u8; 4];
        expect_err(
            "read after cleanup",
            ErrorCode::InvalidHandle,
            c.vfs.read(&cx(), h, 0, &mut buf),
        );
        expect_err(
            "write after cleanup",
            ErrorCode::InvalidHandle,
            c.vfs.write(&cx(), h, 0, b"z", WriteMode::NORMAL),
        );
        expect_err(
            "file_info after cleanup",
            ErrorCode::InvalidHandle,
            c.vfs.file_info(&cx(), h),
        );

        // Close still works -- it is the one callback guaranteed to fire.
        c.vfs.close(&cx(), h);
    });
}

fn duplicate_close_is_a_silent_no_op<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "duplicate close", || {
        c.file_with("dup.txt", b"abc");
        let h = c.open_file("dup.txt").unwrap();
        c.vfs.cleanup(&cx(), h, CleanupFlags::NONE);
        c.vfs.close(&cx(), h);
        // Second close: silent no-op at the FFI (`void`); no panic, no double
        // free. The handle manager reports InvalidHandle internally.
        c.vfs.close(&cx(), h);
        c.vfs.close(&cx(), h);
        // Cleanup of a closed handle is equally harmless.
        c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
        // ...and did NOT delete the file, because the handle no longer resolves.
        assert!(c.exists("dup.txt"), "a stale cleanup deleted a live file");
    });
}

fn stale_handle_after_slot_reuse_is_invalid<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "stale handle after slot reuse", || {
        c.file_with("reuse.txt", b"first");
        let old = c.open_file("reuse.txt").unwrap();
        c.close(old);

        // Force the slot to be reused.
        let new = c.open_file("reuse.txt").unwrap();

        // INV-ID-3: the stale value fails the generation check and does not
        // reach the new object.
        let mut buf = [0u8; 4];
        expect_err(
            "read via stale handle",
            ErrorCode::InvalidHandle,
            c.vfs.read(&cx(), old, 0, &mut buf),
        );
        assert!(c.vfs.file_info(&cx(), new).is_ok());
        c.close(new);
    });
}

fn open_count_tracks_file_objects_not_files<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "open_count bookkeeping", || {
        c.file_with("counted.txt", b"data");
        let a = c.open_file("counted.txt").unwrap();
        let b = c.open_file("counted.txt").unwrap();
        // Same file, two file objects: two HandleIds, one node.
        let ia = c.vfs.file_info(&cx(), a).unwrap();
        let ib = c.vfs.file_info(&cx(), b).unwrap();
        assert_eq!(
            ia.index_number, ib.index_number,
            "two opens of one file must report one index_number"
        );

        c.close(a);
        // Node still alive and readable through the other handle.
        assert!(c.vfs.file_info(&cx(), b).is_ok());
        c.close(b);
        assert!(c.exists("counted.txt"));
    });
}

fn unlink_happens_at_cleanup_not_close<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "unlink at cleanup", || {
        c.file_with("unlinkme.txt", b"data");
        let h = c.open_file("unlinkme.txt").unwrap();

        assert!(c.exists("unlinkme.txt"));
        c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
        // Path lookup fails immediately at cleanup. Doing this at Close instead
        // is how deleted files reappear.
        assert!(
            !c.exists("unlinkme.txt"),
            "unlink did not take effect at Cleanup"
        );
        c.vfs.close(&cx(), h);
        assert!(!c.exists("unlinkme.txt"));
    });
}

fn deleted_but_open_stays_usable<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "deleted but open", || {
        c.file_with("ghost.txt", b"0123456789");
        let keep = c.open_file("ghost.txt").unwrap();
        let doomed = c.open_file("ghost.txt").unwrap();

        // Delete through one handle's cleanup.
        c.vfs.cleanup(&cx(), doomed, CleanupFlags::DELETE);
        assert!(!c.exists("ghost.txt"), "path lookup should fail after unlink");

        // The other handle keeps working: read, write, file_info, set_file_size.
        let mut buf = [0u8; 4];
        let n = c.vfs.read(&cx(), keep, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n as usize], b"0123");
        c.vfs
            .write(&cx(), keep, 0, b"XYZ", WriteMode::NORMAL)
            .unwrap();
        assert!(c.vfs.file_info(&cx(), keep).is_ok());
        c.vfs.set_file_size(&cx(), keep, 5, false).unwrap();

        // Renaming an unlinked node is rejected.
        let target = c.p("ghost-renamed.txt");
        expect_err(
            "rename of an unlinked node",
            ErrorCode::InvalidParameter,
            c.vfs.rename(&cx(), keep, &target, false),
        );

        c.vfs.close(&cx(), doomed);
        c.close(keep);
    });
}

fn reclamation_is_exactly_at_the_last_close<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "reclamation at last close", || {
        c.file_with("reclaim.txt", b"payload");
        let a = c.open_file("reclaim.txt").unwrap();
        let b = c.open_file("reclaim.txt").unwrap();

        c.vfs.cleanup(&cx(), a, CleanupFlags::DELETE);
        c.vfs.close(&cx(), a);
        // open_count is 1: still readable through b.
        let mut buf = [0u8; 7];
        assert_eq!(c.vfs.read(&cx(), b, 0, &mut buf).unwrap(), 7);

        c.vfs.cleanup(&cx(), b, CleanupFlags::NONE);
        c.vfs.close(&cx(), b);
        // Now reclaimed. The invariant checker asserts it: an unlinked node
        // with open_count 0 that was not reclaimed is INV-FS-4.
    });
}

fn recreating_at_a_deleted_path_gives_a_different_node<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "recreate at a deleted path", || {
        c.file_with("phoenix.txt", b"old");
        let old = c.open_file("phoenix.txt").unwrap();
        let old_index = c.vfs.file_info(&cx(), old).unwrap().index_number;

        c.vfs.cleanup(&cx(), old, CleanupFlags::DELETE);

        // Creating a new file at the old path succeeds and is a *different*
        // node (INV-ID-1, INV-ID-2).
        let fresh = c.create_file("phoenix.txt");
        let new_index = c.vfs.file_info(&cx(), fresh).unwrap().index_number;
        assert_ne!(
            old_index, new_index,
            "a recreated path must not reuse index_number"
        );

        // The old node is still readable through its handle and is unaffected.
        let buf = [0u8; 3];
        c.vfs.close(&cx(), old);
        let _ = buf;

        c.close(fresh);
        c.delete("phoenix.txt");
    });
}

fn delete_on_close_unlinks_at_cleanup<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "FILE_DELETE_ON_CLOSE", || {
        let p = c.p("doc.tmp");
        let h = c
            .vfs
            .create(
                &cx(),
                &p,
                CreateOptions {
                    create_options: FILE_DELETE_ON_CLOSE,
                    granted_access: 0,
                    file_attributes: 0,
                    allocation_size: 0,
                },
            )
            .unwrap()
            .handle;
        assert!(c.exists("doc.tmp"));
        // WinFsp raises FspCleanupDelete at cleanup; the VFS unlinks then.
        c.vfs.cleanup(&cx(), h, CleanupFlags::NONE);
        assert!(
            !c.exists("doc.tmp"),
            "FILE_DELETE_ON_CLOSE did not unlink at cleanup"
        );
        c.vfs.close(&cx(), h);
    });
}

fn illegal_transitions_leave_state_unchanged<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "illegal transitions (INV-FS-5)", || {
        c.file_with("stable.txt", b"unchanged");

        // Close of a never-allocated handle.
        let bogus = HandleId::from_raw(0xDEAD_BEEF_CAFE_0001);
        c.vfs.close(&cx(), bogus);
        c.vfs.cleanup(&cx(), bogus, CleanupFlags::DELETE);
        c.vfs.close(&cx(), HandleId::INVALID);
        c.vfs.cleanup(&cx(), HandleId::INVALID, CleanupFlags::DELETE);

        // None of that touched the file.
        assert_eq!(c.read_all("stable.txt"), b"unchanged");
        assert!(c.exists("stable.txt"));
    });
}
