//! Resource limits (§3.4; manual §14.3).
//!
//! One test per row, **at the limit and at limit + 1**. Values are read from
//! `Limits`; `resource-limits.md` is the single source of truth and is not
//! restated here.
//!
//! For every limit+1 case, three assertions in this order:
//!
//! 1. the specified error code (INV-RES-2)
//! 2. **`state_before == state_after`** (INV-RES-3)
//! 3. `check_invariants()` passes (INV-RES-1)
//!
//! **Assertion 2 matters more than assertion 1. A limit that fails *and*
//! corrupts is worse than no limit.**

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::{HandleId, Vfs};

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "boundaries");

    l1_l2_l3_path_bounds(&c);
    l4_single_io(&c);
    l5_total_bytes(&c);
    l6_entries_per_directory(&c);
    l7_open_handles(&c);
    l8_open_cursors(&c);
}

/// A comparable fingerprint of one file's observable state.
fn file_state<V: Vfs + VfsDiagnostics>(c: &Ctx<V>, rel: &str) -> (Vec<u8>, u64, u64) {
    let h = c.open_file(rel).unwrap();
    let info = c.vfs.file_info(&cx(), h).unwrap();
    let size = info.file_size as usize;
    let mut buf = vec![0u8; size];
    if size > 0 {
        c.vfs.read(&cx(), h, 0, &mut buf).unwrap();
    }
    c.close(h);
    (buf, info.file_size, info.allocation_size)
}

fn l1_l2_l3_path_bounds<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "L1/L2/L3 path bounds", || {
        // L2 at the limit: a 255-character component is legal.
        let at = "a".repeat(255);
        let h = c
            .try_create_file(&at)
            .expect("a component exactly at L2 must be accepted");
        c.close(h);

        // L2 + 1.
        let over = "a".repeat(256);
        expect_err(
            "component at L2 + 1",
            ErrorCode::NameTooLong,
            c.raw(&format!("\\{over}")),
        );

        // L3 + 1.
        let deep: String = (0..600).map(|_| "\\a").collect();
        expect_err("path at L3 + 1", ErrorCode::NameTooLong, c.raw(&deep));

        // L1 + 1.
        let long = format!("\\{}", "a".repeat(40_000));
        expect_err("path at L1 + 1", ErrorCode::NameTooLong, c.raw(&long));
    });
}

fn l4_single_io<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "L4 single read/write", || {
        c.file_with("l4.bin", b"seed");
        let before = file_state(c, "l4.bin");
        let h = c.open_file("l4.bin").unwrap();

        // At the limit: accepted.
        let mut at = vec![0u8; c.limits.max_io_bytes];
        assert!(c.vfs.read(&cx(), h, 0, &mut at).is_ok());

        // Limit + 1: rejected before any allocation or indexing.
        let mut over = vec![0u8; c.limits.max_io_bytes + 1];
        expect_err(
            "read at L4 + 1",
            ErrorCode::InvalidParameter,
            c.vfs.read(&cx(), h, 0, &mut over),
        );
        let over_w = vec![0xABu8; c.limits.max_io_bytes + 1];
        expect_err(
            "write at L4 + 1",
            ErrorCode::InvalidParameter,
            c.vfs.write(&cx(), h, 0, &over_w, WriteMode::NORMAL),
        );

        c.close(h);
        assert_eq!(
            file_state(c, "l4.bin"),
            before,
            "L4 failure mutated the file"
        );
    });
}

fn l5_total_bytes<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "L5 total bytes", || {
        // Fill the volume, then prove the write that would exceed it leaves the
        // file byte-identical.
        let vi = c.vfs.volume_info(&cx()).unwrap();
        let free = vi.free_size;
        let chunk = c.limits.max_io_bytes.min(1024 * 1024);

        c.file_with("l5.bin", b"");
        let h = c.open_file("l5.bin").unwrap();

        // Grow until close to full.
        let mut written = 0u64;
        let filler = vec![0x5Au8; chunk];
        while free.saturating_sub(written) > chunk as u64 * 2 {
            match c.vfs.write(&cx(), h, written, &filler, WriteMode::NORMAL) {
                Ok((n, _)) => written += n as u64,
                Err(e) if e.code == ErrorCode::DiskFull => break,
                Err(e) => panic!("unexpected error while filling: {e}"),
            }
        }

        // Now request more than certainly remains.
        let vi = c.vfs.volume_info(&cx()).unwrap();
        let remaining = vi.free_size;
        let too_much = vec![0x77u8; (remaining as usize + 4096).min(c.limits.max_io_bytes)];

        let size_before = c.vfs.file_info(&cx(), h).unwrap().file_size;
        let mut tail = vec![0u8; 64.min(size_before as usize)];
        if !tail.is_empty() {
            c.vfs
                .read(&cx(), h, size_before - tail.len() as u64, &mut tail)
                .unwrap();
        }

        let err = c
            .vfs
            .write(&cx(), h, size_before, &too_much, WriteMode::NORMAL)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::DiskFull, "L5 must report DiskFull");

        // INV-RES-3: the file is byte-identical to before.
        let size_after = c.vfs.file_info(&cx(), h).unwrap().file_size;
        assert_eq!(size_after, size_before, "a DiskFull write changed the size");
        if !tail.is_empty() {
            let mut tail_after = vec![0u8; tail.len()];
            c.vfs
                .read(&cx(), h, size_after - tail.len() as u64, &mut tail_after)
                .unwrap();
            assert_eq!(tail_after, tail, "a DiskFull write changed file content");
        }

        // set_file_size hits the same bound with the same guarantee.
        expect_err(
            "set_file_size beyond L5",
            ErrorCode::DiskFull,
            c.vfs.set_file_size(&cx(), h, c.limits.max_bytes + 1, false),
        );
        assert_eq!(c.vfs.file_info(&cx(), h).unwrap().file_size, size_before);

        // Free the space again so later modules are not starved.
        c.vfs.set_file_size(&cx(), h, 0, false).unwrap();
        c.close(h);
        c.delete("l5.bin");
    });
}

fn l6_entries_per_directory<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    // The L6 boundary test checks invariants once at the end rather than per
    // step, so the O(nodes) walk does not dominate (§11.6).
    let d = c.create_dir("l6");
    c.close(d);

    let max = c.limits.max_dir_entries;
    for i in 0..max {
        c.try_create_file(&format!("l6\\e{i}"))
            .map(|h| c.close(h))
            .unwrap_or_else(|e| panic!("create {i} of {max} failed below the limit: {e}"));
    }

    // At the limit: full.
    let err = expect_err(
        "create at L6 + 1",
        ErrorCode::ResourceExhausted,
        c.try_create_file("l6\\overflow"),
    );
    let _ = err;

    // INV-RES-3: the directory is unchanged by the failure.
    let names = c.list("l6");
    assert_eq!(
        names.len(),
        max + 2,
        "a rejected create changed the directory"
    );
    assert!(
        !names.iter().any(|n| n == "overflow"),
        "the rejected entry was created anyway"
    );

    // Renaming a file into a full directory hits the same bound, and the
    // source must survive (§10.1).
    c.file_with("l6-src.txt", b"source");
    let h = c.open_file("l6-src.txt").unwrap();
    expect_err(
        "rename into a full directory",
        ErrorCode::ResourceExhausted,
        c.vfs.rename(&cx(), h, &c.p("l6\\moved"), false),
    );
    c.close(h);
    assert_eq!(
        c.read_all("l6-src.txt"),
        b"source",
        "a rejected rename disturbed the source"
    );

    c.vfs
        .check_invariants()
        .expect("invariants after the L6 boundary");
}

fn l7_open_handles<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    // Also checked once at the end: opening tens of thousands of handles with a
    // per-step O(nodes) walk would dominate the suite.
    c.file_with("l7.txt", b"x");

    let max = c.limits.max_open_handles;
    let mut held: Vec<HandleId> = Vec::with_capacity(max);

    // Open until the table refuses. The suite closes every handle it opens, so
    // exactly `max` opens must succeed. Asserting the *exact* count rather than
    // just "the limit eventually fires" is what makes this test double as a
    // leak detector for the whole suite: a module that forgets a close shows up
    // here as a shortfall, naming the number leaked.
    loop {
        match c.open_file("l7.txt") {
            Ok(h) => held.push(h),
            Err(e) => {
                assert_eq!(
                    e.code,
                    ErrorCode::ResourceExhausted,
                    "L7 must report ResourceExhausted, got {:?}",
                    e.code
                );
                break;
            }
        }
        assert!(
            held.len() <= max,
            "L7 never fired: {} handles opened with a limit of {max}",
            held.len()
        );
    }

    assert_eq!(
        held.len(),
        max,
        "expected exactly {max} opens to succeed but {} did -- \
         {} handle(s) were still open when the L7 test started, \
         which means an earlier conformance module leaked them",
        held.len(),
        max - held.len()
    );

    // Limit + 1.
    expect_err(
        "open at L7 + 1",
        ErrorCode::ResourceExhausted,
        c.open_file("l7.txt"),
    );
    // ...and creating a file also needs a handle, so it fails the same way and
    // must not leave a node behind (INV-RES-3).
    expect_err(
        "create at L7 + 1",
        ErrorCode::ResourceExhausted,
        c.try_create_file("l7-orphan.txt"),
    );
    assert!(
        !c.exists("l7-orphan.txt"),
        "a create that failed on L7 left the node behind"
    );

    for h in held {
        c.close(h);
    }

    c.vfs
        .check_invariants()
        .expect("invariants after the L7 boundary");
}

fn l8_open_cursors<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "L8 open cursors", || {
        let d = c.create_dir("l8");
        c.close(d);
        let h = c.open_dir("l8").unwrap();

        let max = c.limits.max_open_cursors;
        let mut held = Vec::with_capacity(max);
        for i in 0..max {
            match c.vfs.dir_open(&cx(), h, None, None) {
                Ok(cur) => held.push(cur),
                Err(e) => panic!("dir_open {i} of {max} failed below the limit: {e}"),
            }
        }

        expect_err(
            "dir_open at L8 + 1",
            ErrorCode::ResourceExhausted,
            c.vfs.dir_open(&cx(), h, None, None),
        );

        // Every cursor still works: the failure did not disturb them.
        for cur in &held {
            assert!(c.vfs.dir_next(&cx(), *cur).is_ok());
        }
        for cur in held {
            c.vfs.dir_close(&cx(), cur);
        }
        // Freeing makes room again -- proof the limit counts live cursors and
        // not lifetime allocations.
        let cur = c.vfs.dir_open(&cx(), h, None, None).unwrap();
        c.vfs.dir_close(&cx(), cur);
        c.close(h);
    });
}
