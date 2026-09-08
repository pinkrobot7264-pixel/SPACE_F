//! Read and write (fs-semantics §4; manual §3.3.4, §8.1).
//!
//! **These boundary tests are what Phases 3 and 4 will depend on.**
//!
//! The §8.1 matrix runs against files of size 0, 1, 4095, 4096, 4097 and 1 MiB.

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::Vfs;

use super::util::{cx, expect_err, step, Ctx};

/// The §8.1 size matrix.
const SIZES: &[usize] = &[0, 1, 4095, 4096, 4097, 1024 * 1024];

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "read_write");

    the_size_matrix(&c);
    read_at_or_past_eof_is_end_of_file(&c);
    read_crossing_eof_is_a_short_success(&c);
    read_length_zero_succeeds_with_zero(&c);
    offset_length_overflow_is_invalid_parameter(&c);
    length_above_l4_is_invalid_parameter(&c);
    write_past_eof_zero_fills_the_gap(&c);
    write_to_end_of_file_ignores_the_offset(&c);
    write_length_zero_changes_nothing(&c);
    constrained_io_never_grows_the_file(&c);
    read_after_write_returns_what_was_written(&c);
    io_on_a_directory_is_rejected(&c);
}

fn the_size_matrix<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    for &size in SIZES {
        let name = format!("m{size}.bin");
        let content: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

        step(c.vfs, &format!("size matrix {size}"), || {
            c.file_with(&name, &content);
            let h = c.open_file(&name).unwrap();

            // whole file
            let mut buf = vec![0u8; size.max(1)];
            if size == 0 {
                // A zero-length file: any non-empty read is at EOF.
                expect_err(
                    "read whole empty file",
                    ErrorCode::EndOfFile,
                    c.vfs.read(&cx(), h, 0, &mut buf),
                );
            } else {
                let n = c.vfs.read(&cx(), h, 0, &mut buf).unwrap() as usize;
                assert_eq!(n, size, "short read of a {size}-byte file");
                assert_eq!(&buf[..n], &content[..], "content mismatch at size {size}");

                // first byte
                let mut one = [0u8; 1];
                assert_eq!(c.vfs.read(&cx(), h, 0, &mut one).unwrap(), 1);
                assert_eq!(one[0], content[0]);

                // last byte
                let mut last = [0u8; 1];
                assert_eq!(c.vfs.read(&cx(), h, size as u64 - 1, &mut last).unwrap(), 1);
                assert_eq!(last[0], content[size - 1]);

                // read at file_size - 1 with a 4096 buffer => exactly 1 byte
                let mut big = [0u8; 4096];
                let n = c.vfs.read(&cx(), h, size as u64 - 1, &mut big).unwrap();
                assert_eq!(n, 1, "read crossing EOF must be a short success");
            }

            // read at file_size => EndOfFile
            let mut probe = [0u8; 16];
            expect_err(
                &format!("read at EOF, size {size}"),
                ErrorCode::EndOfFile,
                c.vfs.read(&cx(), h, size as u64, &mut probe),
            );

            // read length 0 => success, 0 transferred, at any offset
            let mut empty: [u8; 0] = [];
            assert_eq!(c.vfs.read(&cx(), h, 0, &mut empty).unwrap(), 0);
            assert_eq!(c.vfs.read(&cx(), h, size as u64, &mut empty).unwrap(), 0);

            // allocation_size is a multiple of 4096 and >= file_size (INV-FS-1)
            let info = c.vfs.file_info(&cx(), h).unwrap();
            assert_eq!(info.file_size, size as u64);
            assert_eq!(info.allocation_size % 4096, 0);
            assert!(info.allocation_size >= info.file_size);

            c.close(h);
        });
    }
}

fn read_at_or_past_eof_is_end_of_file<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "read at/past EOF", || {
        c.file_with("eof.txt", b"12345");
        let h = c.open_file("eof.txt").unwrap();
        let mut buf = [0u8; 8];
        // At EOF and past it: EndOfFile with zero transferred -- NOT
        // success-with-zero, which is how a caller loops forever.
        expect_err(
            "read at EOF",
            ErrorCode::EndOfFile,
            c.vfs.read(&cx(), h, 5, &mut buf),
        );
        expect_err(
            "read past EOF",
            ErrorCode::EndOfFile,
            c.vfs.read(&cx(), h, 6, &mut buf),
        );
        expect_err(
            "read far past EOF",
            ErrorCode::EndOfFile,
            c.vfs.read(&cx(), h, 1_000_000, &mut buf),
        );
        // read at u64::MAX: EndOfFile or InvalidParameter, never a panic.
        let r = c.vfs.read(&cx(), h, u64::MAX, &mut buf);
        let code = r.unwrap_err().code;
        assert!(
            code == ErrorCode::EndOfFile || code == ErrorCode::InvalidParameter,
            "read at u64::MAX gave {code:?}"
        );
        c.close(h);
    });
}

fn read_crossing_eof_is_a_short_success<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "read crossing EOF", || {
        c.file_with("cross.txt", b"abcdefghij");
        let h = c.open_file("cross.txt").unwrap();
        let mut buf = [0u8; 100];
        let n = c.vfs.read(&cx(), h, 6, &mut buf).unwrap();
        // Normal, not an error.
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], b"ghij");
        c.close(h);
    });
}

fn read_length_zero_succeeds_with_zero<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "read length 0", || {
        c.file_with("zero.txt", b"data");
        let h = c.open_file("zero.txt").unwrap();
        let mut empty: [u8; 0] = [];
        // A zero-length read asks for nothing and gets nothing. Checked before
        // the EOF rule, which is about reads that cannot be satisfied.
        assert_eq!(c.vfs.read(&cx(), h, 0, &mut empty).unwrap(), 0);
        assert_eq!(c.vfs.read(&cx(), h, 4, &mut empty).unwrap(), 0);
        assert_eq!(c.vfs.read(&cx(), h, u64::MAX, &mut empty).unwrap(), 0);
        c.close(h);
    });
}

fn offset_length_overflow_is_invalid_parameter<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "offset+length overflow", || {
        c.file_with("ovf.txt", b"0123456789");
        let h = c.open_file("ovf.txt").unwrap();

        // offset = u64::MAX - 1, length 4096. checked_add is not optional:
        // this arrives from the fuzzer within the hour.
        let mut buf = [0u8; 4096];
        let code = c
            .vfs
            .read(&cx(), h, u64::MAX - 1, &mut buf)
            .unwrap_err()
            .code;
        assert!(
            code == ErrorCode::InvalidParameter || code == ErrorCode::EndOfFile,
            "overflowing read gave {code:?}"
        );

        // Writes must catch it as InvalidParameter -- there is no EOF shortcut
        // on the write path.
        expect_err(
            "overflowing write",
            ErrorCode::InvalidParameter,
            c.vfs
                .write(&cx(), h, u64::MAX - 1, &[0u8; 4096], WriteMode::NORMAL),
        );
        expect_err(
            "write at u64::MAX",
            ErrorCode::InvalidParameter,
            c.vfs.write(&cx(), h, u64::MAX, b"x", WriteMode::NORMAL),
        );

        // The file is untouched by all of that.
        c.close(h);
        assert_eq!(c.read_all("ovf.txt"), b"0123456789");
    });
}

fn length_above_l4_is_invalid_parameter<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "length above L4", || {
        c.file_with("l4.txt", b"x");
        let h = c.open_file("l4.txt").unwrap();

        // At the limit: allowed (the read is short, but not rejected).
        let mut at = vec![0u8; c.limits.max_io_bytes];
        assert!(
            c.vfs.read(&cx(), h, 0, &mut at).is_ok(),
            "a read exactly at L4 must be accepted"
        );

        // Limit + 1: rejected before any allocation or indexing.
        let mut over = vec![0u8; c.limits.max_io_bytes + 1];
        expect_err(
            "read above L4",
            ErrorCode::InvalidParameter,
            c.vfs.read(&cx(), h, 0, &mut over),
        );
        let over_w = vec![0u8; c.limits.max_io_bytes + 1];
        expect_err(
            "write above L4",
            ErrorCode::InvalidParameter,
            c.vfs.write(&cx(), h, 0, &over_w, WriteMode::NORMAL),
        );

        c.close(h);
        assert_eq!(
            c.read_all("l4.txt"),
            b"x",
            "a rejected write changed the file"
        );
    });
}

fn write_past_eof_zero_fills_the_gap<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "write past EOF zero-fills", || {
        c.file_with("gap.bin", b"AAAA");
        let h = c.open_file("gap.bin").unwrap();
        let (n, info) = c
            .vfs
            .write(&cx(), h, 10, b"BBBB", WriteMode::NORMAL)
            .unwrap();
        assert_eq!(n, 4);
        assert_eq!(info.file_size, 14);
        c.close(h);

        let all = c.read_all("gap.bin");
        assert_eq!(&all[0..4], b"AAAA");
        assert_eq!(&all[4..10], &[0u8; 6], "the gap must be zero-filled");
        assert_eq!(&all[10..14], b"BBBB");
    });
}

fn write_to_end_of_file_ignores_the_offset<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "WriteToEndOfFile", || {
        c.file_with("append.txt", b"start");
        let h = c.open_file("append.txt").unwrap();
        // A wildly wrong offset must be ignored entirely.
        c.vfs
            .write(&cx(), h, 999_999, b"-more", WriteMode::APPEND)
            .unwrap();
        c.vfs
            .write(&cx(), h, 0, b"-end", WriteMode::APPEND)
            .unwrap();
        c.close(h);
        assert_eq!(c.read_all("append.txt"), b"start-more-end");
    });
}

fn write_length_zero_changes_nothing<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "write length 0", || {
        c.file_with("noop.txt", b"unchanged");
        let h = c.open_file("noop.txt").unwrap();
        let (n, info) = c.vfs.write(&cx(), h, 0, b"", WriteMode::NORMAL).unwrap();
        assert_eq!(n, 0);
        assert_eq!(info.file_size, 9);
        let (n, _) = c.vfs.write(&cx(), h, 500, b"", WriteMode::NORMAL).unwrap();
        assert_eq!(
            n, 0,
            "a zero-length write past EOF must not extend the file"
        );
        c.close(h);
        assert_eq!(c.read_all("noop.txt"), b"unchanged");
    });
}

fn constrained_io_never_grows_the_file<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "ConstrainedIo", || {
        // From the cache manager's write-behind path. Ignoring it produces
        // files that grow during a copy.
        c.file_with("constrained.bin", b"0123456789");
        let h = c.open_file("constrained.bin").unwrap();

        // offset >= file_size: zero transferred, success, file unchanged.
        let (n, info) = c
            .vfs
            .write(&cx(), h, 10, b"XXXX", WriteMode::CONSTRAINED)
            .unwrap();
        assert_eq!(n, 0);
        assert_eq!(info.file_size, 10);
        let (n, _) = c
            .vfs
            .write(&cx(), h, 50, b"XXXX", WriteMode::CONSTRAINED)
            .unwrap();
        assert_eq!(n, 0);

        // crossing EOF: truncated to the existing size.
        let (n, info) = c
            .vfs
            .write(&cx(), h, 8, b"YYYYYYYY", WriteMode::CONSTRAINED)
            .unwrap();
        assert_eq!(n, 2, "constrained write must be truncated to the file size");
        assert_eq!(
            info.file_size, 10,
            "constrained write must not grow the file"
        );

        c.close(h);
        assert_eq!(c.read_all("constrained.bin"), b"01234567YY");
    });
}

fn read_after_write_returns_what_was_written<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "read after write", || {
        // The deterministic core of the §8.1 property test. The randomised
        // version with a recorded seed lives in the proptest suite; this is the
        // part every implementation must pass on every run.
        c.file_with("rw.bin", &[]);
        let h = c.open_file("rw.bin").unwrap();

        let mut expected = Vec::new();
        for (offset, len) in [
            (0usize, 100usize),
            (50, 200),
            (1000, 37),
            (137, 1),
            (0, 4096),
        ] {
            let content: Vec<u8> = (0..len).map(|i| ((i + offset) % 253) as u8).collect();
            c.vfs
                .write(&cx(), h, offset as u64, &content, WriteMode::NORMAL)
                .unwrap();
            if expected.len() < offset + len {
                expected.resize(offset + len, 0u8);
            }
            expected[offset..offset + len].copy_from_slice(&content);

            let mut back = vec![0u8; len];
            let n = c.vfs.read(&cx(), h, offset as u64, &mut back).unwrap();
            assert_eq!(n as usize, len);
            assert_eq!(back, content, "read-after-write mismatch at {offset}+{len}");
        }

        let mut whole = vec![0u8; expected.len()];
        c.vfs.read(&cx(), h, 0, &mut whole).unwrap();
        assert_eq!(whole, expected, "whole-file content diverged");
        c.close(h);
    });
}

fn io_on_a_directory_is_rejected<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "io on a directory", || {
        let d = c.create_dir("iodir");
        let mut buf = [0u8; 4];
        expect_err(
            "read a directory",
            ErrorCode::FileIsADirectory,
            c.vfs.read(&cx(), d, 0, &mut buf),
        );
        expect_err(
            "write a directory",
            ErrorCode::FileIsADirectory,
            c.vfs.write(&cx(), d, 0, b"x", WriteMode::NORMAL),
        );
        expect_err(
            "set_file_size on a directory",
            ErrorCode::FileIsADirectory,
            c.vfs.set_file_size(&cx(), d, 10, false),
        );
        c.close(d);
    });
}
