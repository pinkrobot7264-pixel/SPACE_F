//! Timestamps and metadata mutation (fs-semantics §6; manual §3.3.6, §10).

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::Vfs;

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "metadata");

    create_sets_all_four_timestamps(&c);
    read_sets_accessed_only(&c);
    write_sets_written_and_changed(&c);
    set_basic_info_treats_zero_as_no_change(&c);
    set_basic_info_applies_explicit_values(&c);
    set_basic_info_cannot_change_the_directory_bit(&c);
    set_file_size_truncates_and_extends(&c);
    set_file_size_allocation_only(&c);
    allocation_size_invariants_hold_across_sizes(&c);
    flush_validates_its_handle(&c);
}

/// Timestamps have coarse resolution, so tests that need a *later* value force
/// one rather than sleeping.
fn bump(v: &impl Vfs, h: crate::vfs::HandleId, t: u64) {
    v.set_basic_info(
        &cx(),
        h,
        BasicInfoPatch {
            file_attributes: None,
            creation_time: Some(t),
            last_access_time: Some(t),
            last_write_time: Some(t),
            change_time: Some(t),
        },
    )
    .unwrap();
}

fn create_sets_all_four_timestamps<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "create sets all four timestamps", || {
        let h = c.create_file("ts.txt");
        let i = c.vfs.file_info(&cx(), h).unwrap();
        assert!(i.creation_time > 0);
        assert!(i.last_access_time > 0);
        assert!(i.last_write_time > 0);
        assert!(i.change_time > 0);
        c.close(h);
    });
}

fn read_sets_accessed_only<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "read sets accessed only", || {
        c.file_with("racc.txt", b"data");
        let h = c.open_file("racc.txt").unwrap();
        bump(c.vfs, h, 1_000_000);
        let before = c.vfs.file_info(&cx(), h).unwrap();

        let mut buf = [0u8; 4];
        c.vfs.read(&cx(), h, 0, &mut buf).unwrap();
        let after = c.vfs.file_info(&cx(), h).unwrap();

        assert!(
            after.last_access_time > before.last_access_time,
            "read must update last_access_time"
        );
        assert_eq!(
            after.last_write_time, before.last_write_time,
            "read must not touch last_write_time"
        );
        assert_eq!(
            after.creation_time, before.creation_time,
            "read must not touch creation_time"
        );
        c.close(h);
    });
}

fn write_sets_written_and_changed<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "write sets written and changed", || {
        c.file_with("wts.txt", b"data");
        let h = c.open_file("wts.txt").unwrap();
        bump(c.vfs, h, 1_000_000);
        let before = c.vfs.file_info(&cx(), h).unwrap();

        c.vfs.write(&cx(), h, 0, b"more", WriteMode::NORMAL).unwrap();
        let after = c.vfs.file_info(&cx(), h).unwrap();

        assert!(after.last_write_time > before.last_write_time);
        assert!(after.change_time > before.change_time);
        assert_eq!(after.creation_time, before.creation_time);
        c.close(h);
    });
}

fn set_basic_info_treats_zero_as_no_change<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "set_basic_info: 0 means no change", || {
        c.file_with("zeros.txt", b"x");
        let h = c.open_file("zeros.txt").unwrap();
        bump(c.vfs, h, 5_000_000);
        let before = c.vfs.file_info(&cx(), h).unwrap();

        // All zeros: nothing changes except change_time, which tracks the
        // metadata mutation itself.
        let after = c
            .vfs
            .set_basic_info(&cx(), h, BasicInfoPatch::default())
            .unwrap();

        assert_eq!(after.creation_time, before.creation_time);
        assert_eq!(after.last_access_time, before.last_access_time);
        assert_eq!(after.last_write_time, before.last_write_time);
        assert_eq!(after.file_attributes, before.file_attributes);
        c.close(h);
    });
}

fn set_basic_info_applies_explicit_values<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "set_basic_info: explicit values", || {
        c.file_with("explicit.txt", b"x");
        let h = c.open_file("explicit.txt").unwrap();

        let info = c
            .vfs
            .set_basic_info(
                &cx(),
                h,
                BasicInfoPatch {
                    file_attributes: Some(FILE_ATTRIBUTE_READONLY),
                    creation_time: Some(111_000_000),
                    last_access_time: None,
                    last_write_time: Some(333_000_000),
                    change_time: Some(444_000_000),
                },
            )
            .unwrap();

        assert_eq!(info.creation_time, 111_000_000);
        assert_eq!(info.last_write_time, 333_000_000);
        assert_eq!(info.change_time, 444_000_000);
        assert_ne!(info.file_attributes & FILE_ATTRIBUTE_READONLY, 0);

        // The change survives a close/reopen.
        c.close(h);
        let h = c.open_file("explicit.txt").unwrap();
        let again = c.vfs.file_info(&cx(), h).unwrap();
        assert_eq!(again.creation_time, 111_000_000);
        c.close(h);
    });
}

fn set_basic_info_cannot_change_the_directory_bit<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "set_basic_info cannot change node kind", || {
        let d = c.create_dir("kind");
        // Asking to clear FILE_ATTRIBUTE_DIRECTORY must not turn a directory
        // into a file: a node never changes kind.
        let info = c
            .vfs
            .set_basic_info(
                &cx(),
                d,
                BasicInfoPatch {
                    file_attributes: Some(FILE_ATTRIBUTE_NORMAL),
                    ..BasicInfoPatch::default()
                },
            )
            .unwrap();
        assert!(info.is_dir(), "the directory bit must be preserved");
        c.close(d);

        c.file_with("kindfile.txt", b"x");
        let h = c.open_file("kindfile.txt").unwrap();
        let info = c
            .vfs
            .set_basic_info(
                &cx(),
                h,
                BasicInfoPatch {
                    file_attributes: Some(FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_NORMAL),
                    ..BasicInfoPatch::default()
                },
            )
            .unwrap();
        assert!(!info.is_dir(), "a file must not gain the directory bit");
        c.close(h);
    });
}

fn set_file_size_truncates_and_extends<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "set_file_size", || {
        c.file_with("size.bin", b"0123456789");
        let h = c.open_file("size.bin").unwrap();

        // truncate to smaller: discards
        let i = c.vfs.set_file_size(&cx(), h, 4, false).unwrap();
        assert_eq!(i.file_size, 4);
        let mut buf = [0u8; 4];
        c.vfs.read(&cx(), h, 0, &mut buf).unwrap();
        assert_eq!(&buf, b"0123");

        // extend: zero-fills
        let i = c.vfs.set_file_size(&cx(), h, 10, false).unwrap();
        assert_eq!(i.file_size, 10);
        let mut buf = [0u8; 10];
        c.vfs.read(&cx(), h, 0, &mut buf).unwrap();
        assert_eq!(&buf, b"0123\0\0\0\0\0\0", "extension must zero-fill");

        // truncate to 0
        let i = c.vfs.set_file_size(&cx(), h, 0, false).unwrap();
        assert_eq!(i.file_size, 0);
        assert_eq!(i.allocation_size, 0);
        c.close(h);
    });
}

fn set_file_size_allocation_only<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "set_file_size with set_allocation", || {
        c.file_with("alloc.bin", b"12345");
        let h = c.open_file("alloc.bin").unwrap();

        // Adjusts allocation only: the file size is untouched.
        let i = c.vfs.set_file_size(&cx(), h, 100_000, true).unwrap();
        assert_eq!(i.file_size, 5, "set_allocation must not change file_size");
        assert!(i.allocation_size >= 100_000);
        assert_eq!(i.allocation_size % 4096, 0);
        assert!(i.allocation_size >= i.file_size, "INV-FS-1");
        c.close(h);
    });
}

fn allocation_size_invariants_hold_across_sizes<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "allocation_size across sizes", || {
        // §6.4: sizes 0, 1, 4095, 4096, 4097.
        for (i, size) in [0usize, 1, 4095, 4096, 4097].iter().enumerate() {
            let name = format!("a{i}.bin");
            c.file_with(&name, &vec![7u8; *size]);
            let h = c.open_file(&name).unwrap();
            let info = c.vfs.file_info(&cx(), h).unwrap();
            assert_eq!(info.file_size, *size as u64);
            assert_eq!(
                info.allocation_size % 4096,
                0,
                "allocation_size not a multiple of 4096 for size {size}"
            );
            assert!(
                info.allocation_size >= info.file_size,
                "allocation_size below file_size for size {size}"
            );
            c.close(h);
        }
    });
}

fn flush_validates_its_handle<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "flush", || {
        // Phase 1 has no durable state by design (§15.4), so flush does no
        // work -- but it still validates its handle, so a bad one is reported
        // rather than silently accepted.
        c.file_with("flush.txt", b"x");
        let h = c.open_file("flush.txt").unwrap();
        assert!(c.vfs.flush(&cx(), Some(h)).unwrap().is_some());
        // Whole-volume flush.
        assert!(c.vfs.flush(&cx(), None).unwrap().is_none());
        c.close(h);
        expect_err(
            "flush a closed handle",
            ErrorCode::InvalidHandle,
            c.vfs.flush(&cx(), Some(h)),
        );
    });
}
