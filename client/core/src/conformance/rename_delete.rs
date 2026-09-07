//! Rename and delete (fs-semantics §5; manual §3.3.5, §10.1).

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::Vfs;

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "rename_delete");

    rename_within_and_across_directories(&c);
    rename_over_existing_without_replace_is_file_exists(&c);
    rename_over_existing_with_replace(&c);
    rename_a_directory_moves_the_whole_subtree(&c);
    rename_a_directory_into_its_own_subtree_is_rejected(&c);
    rename_an_unlinked_node_is_rejected(&c);
    open_handle_survives_rename(&c);
    case_only_rename_updates_the_display_name(&c);
    can_delete_is_a_pure_query(&c);
    delete_non_empty_directory_is_directory_not_empty(&c);
    delete_while_open(&c);
    rename_round_trip_restores_the_tree(&c);
}

fn rename_within_and_across_directories<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename within and across directories", || {
        let d1 = c.create_dir("d1");
        c.close(d1);
        let d2 = c.create_dir("d2");
        c.close(d2);
        c.file_with("d1\\a.txt", b"payload");

        // within a directory
        let h = c.open_file("d1\\a.txt").unwrap();
        c.vfs.rename(&cx(), h, &c.p("d1\\b.txt"), false).unwrap();
        c.close(h);
        assert!(!c.exists("d1\\a.txt"));
        assert_eq!(c.read_all("d1\\b.txt"), b"payload");

        // across directories
        let h = c.open_file("d1\\b.txt").unwrap();
        c.vfs.rename(&cx(), h, &c.p("d2\\c.txt"), false).unwrap();
        c.close(h);
        assert!(!c.exists("d1\\b.txt"));
        assert_eq!(c.read_all("d2\\c.txt"), b"payload");
    });
}

fn rename_over_existing_without_replace_is_file_exists<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename over existing, replace = false", || {
        c.file_with("src.txt", b"source");
        c.file_with("dst.txt", b"destination");
        let h = c.open_file("src.txt").unwrap();
        expect_err(
            "rename over existing without replace",
            ErrorCode::FileExists,
            c.vfs.rename(&cx(), h, &c.p("dst.txt"), false),
        );
        c.close(h);
        // Both survive, unchanged (INV-NS-5).
        assert_eq!(c.read_all("src.txt"), b"source");
        assert_eq!(c.read_all("dst.txt"), b"destination");
    });
}

fn rename_over_existing_with_replace<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename over existing, replace = true", || {
        c.file_with("rsrc.txt", b"source");
        c.file_with("rdst.txt", b"destination");
        let h = c.open_file("rsrc.txt").unwrap();
        c.vfs.rename(&cx(), h, &c.p("rdst.txt"), true).unwrap();
        c.close(h);
        assert!(!c.exists("rsrc.txt"));
        assert_eq!(c.read_all("rdst.txt"), b"source");

        // A directory target may not be replaced by a file.
        let d = c.create_dir("rdir");
        c.close(d);
        c.file_with("rfile.txt", b"f");
        let h = c.open_file("rfile.txt").unwrap();
        expect_err(
            "replace a directory with a file",
            ErrorCode::FileIsADirectory,
            c.vfs.rename(&cx(), h, &c.p("rdir"), true),
        );
        c.close(h);
        assert!(c.exists("rdir"));
        assert!(c.exists("rfile.txt"));
    });
}

fn rename_a_directory_moves_the_whole_subtree<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename a directory moves the subtree", || {
        let a = c.create_dir("tree");
        c.close(a);
        let b = c.create_dir("tree\\inner");
        c.close(b);
        c.file_with("tree\\inner\\leaf.txt", b"leafdata");

        let h = c.open_dir("tree").unwrap();
        c.vfs.rename(&cx(), h, &c.p("moved"), false).unwrap();
        c.close(h);

        assert!(!c.exists("tree"));
        assert!(c.exists("moved"));
        assert!(c.exists("moved\\inner"));
        assert_eq!(c.read_all("moved\\inner\\leaf.txt"), b"leafdata");
    });
}

fn rename_a_directory_into_its_own_subtree_is_rejected<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename a directory into its own subtree", || {
        let a = c.create_dir("cyc");
        c.close(a);
        let b = c.create_dir("cyc\\child");
        c.close(b);

        let h = c.open_dir("cyc").unwrap();
        // INV-NS-4: this would detach the subtree from the root.
        expect_err(
            "rename a directory into its own subtree",
            ErrorCode::InvalidParameter,
            c.vfs.rename(&cx(), h, &c.p("cyc\\child\\self"), false),
        );
        c.close(h);
        // check_invariants after this step is what proves nothing was detached.
        assert!(c.exists("cyc\\child"));
    });
}

fn rename_an_unlinked_node_is_rejected<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename an unlinked node", || {
        // Two file objects on one node. The delete happens through `doomed`;
        // `keep` is a *different* file object that has not been cleaned up, so
        // it is still a legal handle -- which is what makes this a test of the
        // unlinked-node rule and not of the after-cleanup rule.
        //
        // Renaming through the cleaned-up handle would report InvalidHandle
        // first (fs-semantics §1), and that is also correct; it just tests a
        // different rule.
        c.file_with("gone.txt", b"x");
        let keep = c.open_file("gone.txt").unwrap();
        let doomed = c.open_file("gone.txt").unwrap();

        c.vfs.cleanup(&cx(), doomed, CleanupFlags::DELETE);
        c.vfs.close(&cx(), doomed);

        // The surviving handle still works for I/O...
        let mut buf = [0u8; 1];
        assert_eq!(c.vfs.read(&cx(), keep, 0, &mut buf).unwrap(), 1);
        // ...but not for rename: an unlinked node has no parent to move within.
        expect_err(
            "rename an unlinked node",
            ErrorCode::InvalidParameter,
            c.vfs.rename(&cx(), keep, &c.p("back.txt"), false),
        );

        c.close(keep);
        assert!(!c.exists("back.txt"));
        assert!(!c.exists("gone.txt"));
    });
}

fn open_handle_survives_rename<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "open handle survives rename", || {
        // Handles reference NodeId, not path (INV-ID-1).
        c.file_with("before.txt", b"0123456789");
        let h = c.open_file("before.txt").unwrap();
        let index_before = c.vfs.file_info(&cx(), h).unwrap().index_number;

        c.vfs.rename(&cx(), h, &c.p("after.txt"), false).unwrap();

        // Reads through the handle still return the correct bytes.
        let mut buf = [0u8; 10];
        assert_eq!(c.vfs.read(&cx(), h, 0, &mut buf).unwrap(), 10);
        assert_eq!(&buf, b"0123456789");
        // ...and it is the same node.
        assert_eq!(
            c.vfs.file_info(&cx(), h).unwrap().index_number,
            index_before
        );
        // Writes too.
        c.vfs.write(&cx(), h, 0, b"XX", WriteMode::NORMAL).unwrap();
        c.close(h);
        assert_eq!(c.read_all("after.txt"), b"XX23456789");
    });
}

fn case_only_rename_updates_the_display_name<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "case-only rename", || {
        // CasePreservedNames = 1 with CaseSensitiveSearch = 0: the target
        // "exists" (it is the same node), so this must not be FileExists.
        c.file_with("casing.txt", b"x");
        let h = c.open_file("casing.txt").unwrap();
        c.vfs.rename(&cx(), h, &c.p("CASING.TXT"), false).unwrap();
        c.close(h);
        let names = c.list("");
        assert!(
            names.iter().any(|n| n == "CASING.TXT"),
            "case-only rename did not update the display name: {names:?}"
        );
        assert!(c.exists("casing.txt"), "lookup is still case-insensitive");
    });
}

fn can_delete_is_a_pure_query<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "can_delete is a pure query", || {
        c.file_with("q.txt", b"x");
        let h = c.open_file("q.txt").unwrap();
        // Asking does not delete.
        c.vfs.can_delete(&cx(), h).unwrap();
        c.vfs.can_delete(&cx(), h).unwrap();
        c.close(h);
        assert!(c.exists("q.txt"), "can_delete removed the file");
    });
}

fn delete_non_empty_directory_is_directory_not_empty<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "delete a non-empty directory", || {
        let d = c.create_dir("full");
        c.close(d);
        c.file_with("full\\child.txt", b"x");

        let h = c.open_dir("full").unwrap();
        expect_err(
            "can_delete on a non-empty directory",
            ErrorCode::DirectoryNotEmpty,
            c.vfs.can_delete(&cx(), h),
        );
        c.close(h);
        assert!(c.exists("full\\child.txt"));

        // Emptying it makes the query succeed.
        c.delete("full\\child.txt");
        let h = c.open_dir("full").unwrap();
        c.vfs.can_delete(&cx(), h).unwrap();
        c.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
        c.vfs.close(&cx(), h);
        assert!(!c.exists("full"));
    });
}

fn delete_while_open<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "delete while open", || {
        // INV-FS-4: usable through existing handles, unreachable by path,
        // reclaimed at the last close.
        c.file_with("open-delete.txt", b"still here");
        let keeper = c.open_file("open-delete.txt").unwrap();
        c.delete("open-delete.txt");

        assert!(!c.exists("open-delete.txt"));
        let mut buf = [0u8; 10];
        let n = c.vfs.read(&cx(), keeper, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n as usize], b"still here");

        // Close after delete: no error, no leak.
        c.close(keeper);
    });
}

fn rename_round_trip_restores_the_tree<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "rename A->B->A", || {
        // The deterministic core of the §14.2 property "rename A->B->A leaves
        // the tree identical" (INV-NS-2, INV-ID-1).
        c.file_with("rt.txt", b"roundtrip");
        let before = c.list("");
        let index_before = {
            let h = c.open_file("rt.txt").unwrap();
            let i = c.vfs.file_info(&cx(), h).unwrap().index_number;
            c.close(h);
            i
        };

        let h = c.open_file("rt.txt").unwrap();
        c.vfs.rename(&cx(), h, &c.p("rt-moved.txt"), false).unwrap();
        c.vfs.rename(&cx(), h, &c.p("rt.txt"), false).unwrap();
        c.close(h);

        assert_eq!(c.list(""), before, "tree differs after a rename round trip");
        let h = c.open_file("rt.txt").unwrap();
        assert_eq!(
            c.vfs.file_info(&cx(), h).unwrap().index_number,
            index_before,
            "identity changed across a rename round trip"
        );
        c.close(h);
        assert_eq!(c.read_all("rt.txt"), b"roundtrip");
    });
}
