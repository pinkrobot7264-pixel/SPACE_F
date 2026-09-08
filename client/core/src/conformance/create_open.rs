//! Open and create (fs-semantics §3; manual §3.3.3, §7.1).
//!
//! Every row of the §3.3.3 table is a test named for the row.

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::Vfs;

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "create_open");

    open_nonexistent_is_file_not_found(&c);
    open_with_missing_parent_is_object_path_not_found(&c);
    create_existing_is_file_exists(&c);
    open_directory_with_non_directory_file_is_file_is_a_directory(&c);
    open_file_with_directory_file_is_not_a_directory(&c);
    same_file_opened_twice(&c);
    create_reports_the_new_file_info(&c);
    create_directory(&c);
    create_under_a_file_is_rejected(&c);
    create_at_the_root_is_rejected(&c);
    failed_create_leaves_no_orphan(&c);
    granted_access_is_recorded_not_enforced(&c);
    overwrite_truncates_and_keeps_identity(&c);
}

fn open_nonexistent_is_file_not_found<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "open nonexistent", || {
        expect_err(
            "open nonexistent",
            ErrorCode::FileNotFound,
            c.open_file("nope.txt"),
        );
        let p = c.p("nope.txt");
        expect_err(
            "probe nonexistent",
            ErrorCode::FileNotFound,
            c.vfs.probe(&cx(), &p),
        );
    });
}

fn open_with_missing_parent_is_object_path_not_found<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "open with missing parent", || {
        // Distinct from FileNotFound: Windows tools distinguish them, and
        // reporting the wrong one sends the user looking in the wrong place.
        expect_err(
            "open under a missing directory",
            ErrorCode::ObjectPathNotFound,
            c.open_file("missing-dir\\file.txt"),
        );
        expect_err(
            "create under a missing directory",
            ErrorCode::ObjectPathNotFound,
            c.try_create_file("missing-dir\\file.txt"),
        );
        expect_err(
            "deeply missing parent",
            ErrorCode::ObjectPathNotFound,
            c.open_file("a\\b\\c\\d.txt"),
        );
    });
}

fn create_existing_is_file_exists<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "create existing", || {
        c.file_with("dup.txt", b"x");
        expect_err(
            "create over an existing file",
            ErrorCode::FileExists,
            c.try_create_file("dup.txt"),
        );
        // Case-insensitively, too (CaseSensitiveSearch = 0).
        expect_err(
            "create over an existing file, different case",
            ErrorCode::FileExists,
            c.try_create_file("DUP.TXT"),
        );
    });
}

fn open_directory_with_non_directory_file_is_file_is_a_directory<V: Vfs + VfsDiagnostics>(
    c: &Ctx<V>,
) {
    step(c.vfs, "FILE_NON_DIRECTORY_FILE on a directory", || {
        let h = c.create_dir("adir");
        c.close(h);
        let p = c.p("adir");
        expect_err(
            "FILE_NON_DIRECTORY_FILE on a directory",
            ErrorCode::FileIsADirectory,
            c.vfs.open(
                &cx(),
                &p,
                OpenOptions {
                    create_options: FILE_NON_DIRECTORY_FILE,
                    granted_access: 0,
                },
            ),
        );
    });
}

fn open_file_with_directory_file_is_not_a_directory<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "FILE_DIRECTORY_FILE on a file", || {
        c.file_with("afile.txt", b"x");
        expect_err(
            "FILE_DIRECTORY_FILE on a file",
            ErrorCode::NotADirectory,
            c.open_dir("afile.txt"),
        );
    });
}

fn same_file_opened_twice<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "same file opened twice", || {
        c.file_with("twice.txt", b"content");
        let a = c.open_file("twice.txt").unwrap();
        let b = c.open_file("twice.txt").unwrap();
        // Two HandleIds, one NodeId (observable as one index_number),
        // open_count == 2 (asserted by INV-FS-2 via step()).
        assert_ne!(a, b);
        assert_eq!(
            c.vfs.file_info(&cx(), a).unwrap().index_number,
            c.vfs.file_info(&cx(), b).unwrap().index_number
        );
        // A write through one is visible through the other: one node.
        c.vfs
            .write(&cx(), a, 0, b"CONTENT", WriteMode::NORMAL)
            .unwrap();
        let mut buf = [0u8; 7];
        c.vfs.read(&cx(), b, 0, &mut buf).unwrap();
        assert_eq!(&buf, b"CONTENT");
        c.close(a);
        c.close(b);
    });
}

fn create_reports_the_new_file_info<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "create reports file info", || {
        let p = c.p("fresh.txt");
        let o = c
            .vfs
            .create(
                &cx(),
                &p,
                CreateOptions {
                    create_options: 0,
                    granted_access: 0,
                    file_attributes: 0,
                    allocation_size: 0,
                },
            )
            .unwrap();
        assert_eq!(o.info.file_size, 0);
        assert_eq!(o.info.allocation_size, 0);
        assert_eq!(o.info.reparse_tag, 0, "reparse_tag is always 0 in Phase 1");
        assert!(!o.info.is_dir());
        assert!(o.info.index_number > 0, "index_number 0 is never valid");
        // create sets all four timestamps (fs-semantics §6).
        assert!(o.info.creation_time > 0);
        assert_eq!(o.info.creation_time, o.info.last_write_time);
        c.close(o.handle);
    });
}

fn create_directory<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "create directory", || {
        let p = c.p("newdir");
        let o = c
            .vfs
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
            .unwrap();
        assert!(
            o.info.is_dir(),
            "directory must carry FILE_ATTRIBUTE_DIRECTORY"
        );
        c.close(o.handle);

        // Nested creation works once the parent exists.
        c.file_with("newdir\\inner.txt", b"nested");
        assert_eq!(c.read_all("newdir\\inner.txt"), b"nested");
    });
}

fn create_under_a_file_is_rejected<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "create under a file", || {
        c.file_with("notadir.txt", b"x");
        // A file in the middle of a path is a path error, not a name error.
        expect_err(
            "create under a file",
            ErrorCode::ObjectPathNotFound,
            c.try_create_file("notadir.txt\\child"),
        );
    });
}

fn create_at_the_root_is_rejected<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "create at the root", || {
        let root = c.raw("\\").unwrap();
        expect_err(
            "create the root",
            ErrorCode::FileExists,
            c.vfs.create(
                &cx(),
                &root,
                CreateOptions {
                    create_options: FILE_DIRECTORY_FILE,
                    granted_access: 0,
                    file_attributes: 0,
                    allocation_size: 0,
                },
            ),
        );
    });
}

fn failed_create_leaves_no_orphan<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "failed create leaves no orphan", || {
        // INV-RES-3: a create that fails must leave no half-created node. The
        // invariant checker after this step is what actually proves it -- an
        // orphan would trip INV-NS-2 or INV-FS-4.
        c.file_with("orphan.txt", b"x");
        for _ in 0..20 {
            let _ = c.try_create_file("orphan.txt");
            let _ = c.try_create_file("missing\\deep\\orphan.txt");
        }
        assert_eq!(c.read_all("orphan.txt"), b"x");
    });
}

fn granted_access_is_recorded_not_enforced<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "granted_access is not enforced (ADR-0012)", || {
        // Share access is WinFsp-owned. The core records granted_access for
        // diagnostics and does not enforce it, so an open with zero access
        // still permits I/O at this layer.
        let p = c.p("access.txt");
        let h = c
            .vfs
            .create(
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
            .handle;
        c.vfs
            .write(&cx(), h, 0, b"written", WriteMode::NORMAL)
            .unwrap();
        let mut buf = [0u8; 7];
        assert_eq!(c.vfs.read(&cx(), h, 0, &mut buf).unwrap(), 7);
        c.close(h);
    });
}

fn overwrite_truncates_and_keeps_identity<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "overwrite", || {
        c.file_with("ow.txt", b"the original content");
        let h = c.open_file("ow.txt").unwrap();
        let before = c.vfs.file_info(&cx(), h).unwrap();

        let info = c.vfs.overwrite(&cx(), h, 0, false, 0).unwrap();
        assert_eq!(info.file_size, 0, "overwrite must truncate");
        assert_eq!(
            info.index_number, before.index_number,
            "overwrite must not change file identity"
        );
        c.close(h);
        assert_eq!(c.read_all("ow.txt"), b"");

        // Overwriting a directory is rejected.
        let d = c.create_dir("owdir");
        expect_err(
            "overwrite a directory",
            ErrorCode::FileIsADirectory,
            c.vfs.overwrite(&cx(), d, 0, false, 0),
        );
        c.close(d);
    });
}
