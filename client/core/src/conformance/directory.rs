//! Directory enumeration (fs-semantics §8; manual §3.3.8).
//!
//! This is the **semantic** contract. The WinFsp protocol that carries it
//! (buffer packing, the NULL end marker) lives in the adapter and is not
//! visible here.

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::{CursorId, Vfs};

use super::util::{cx, expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "directory");

    order_is_dot_dotdot_then_folded_ascending(&c);
    the_root_omits_both_dot_entries(&c);
    marker_yields_entries_strictly_after(&c);
    marker_chaining_yields_each_entry_exactly_once(&c);
    enumeration_terminates_within_n_plus_2(&c);
    pattern_is_accepted_and_ignored(&c);
    enumerating_a_file_is_not_a_directory(&c);
    a_stale_cursor_is_a_controlled_error(&c);
    dot_entries_carry_the_right_info(&c);
    an_empty_directory_yields_only_dot_entries(&c);
    resume_from_a_marker_that_no_longer_exists(&c);
    delete_while_enumerating_visits_every_entry(&c);
}

fn resume_from_a_marker_that_no_longer_exists<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "resume from a deleted marker", || {
        // Regression: the marker is a position in a total order, not a lookup
        // key. Resuming by membership returned "end of directory" whenever the
        // marker had been deleted between calls, which silently truncated every
        // delete-while-enumerating client.
        let d = c.create_dir("gone-marker");
        c.close(d);
        for n in ["a", "b", "c", "d", "e"] {
            c.file_with(&format!("gone-marker\\{n}"), b"x");
        }
        let h = c.open_dir("gone-marker").unwrap();

        // Delete the entry we are about to resume from.
        c.delete("gone-marker\\c");

        // Resuming after the now-absent "c" must still yield "d" and "e" --
        // the entries that sort after it -- not nothing.
        let after = c.list_handle(h, Some("c"));
        assert_eq!(
            after,
            vec!["d", "e"],
            "a deleted marker stranded the enumeration"
        );

        // The same holds for a marker that never existed at all.
        let after_ghost = c.list_handle(h, Some("bb"));
        assert_eq!(after_ghost, vec!["d", "e"]);

        // ...and for a marker sorting before everything.
        let after_low = c.list_handle(h, Some("!"));
        assert_eq!(after_low, vec!["a", "b", "d", "e"]);

        c.close(h);
    });
}

fn delete_while_enumerating_visits_every_entry<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "delete while enumerating", || {
        // The exact shape of `Remove-Item -Recurse` / `rmdir /s`: enumerate a
        // batch, delete it, resume from the last name seen -- which no longer
        // exists. This drained only ~35 of 500 files before the fix, and the
        // directory then refused to be removed as non-empty.
        let d = c.create_dir("del-while-enum");
        c.close(d);
        const N: usize = 40;
        for i in 0..N {
            c.file_with(&format!("del-while-enum\\f{i:03}"), b"x");
        }

        let h = c.open_dir("del-while-enum").unwrap();
        let mut deleted = 0usize;
        let mut marker: Option<String> = None;
        let mut rounds = 0usize;

        loop {
            rounds += 1;
            assert!(rounds < N + 10, "enumeration did not terminate");

            let cur = c.vfs.dir_open(&cx(), h, None, marker.as_deref()).unwrap();
            let mut batch = Vec::new();
            // A small buffer, so the resume path is exercised many times.
            while batch.len() < 5 {
                match c.vfs.dir_next(&cx(), cur).unwrap() {
                    Some(e) => batch.push(e.name),
                    None => break,
                }
            }
            c.vfs.dir_close(&cx(), cur);

            if batch.is_empty() {
                break;
            }
            marker = Some(batch.last().unwrap().clone());

            for name in batch {
                if name == "." || name == ".." {
                    continue;
                }
                c.delete(&format!("del-while-enum\\{name}"));
                deleted += 1;
            }
        }

        assert_eq!(
            deleted, N,
            "delete-while-enumerating visited only {deleted} of {N} entries"
        );

        c.close(h);

        // And the directory is now genuinely empty, so it can be removed --
        // which is the failure the user actually sees.
        let names = c.list("del-while-enum");
        assert_eq!(names, vec![".", ".."], "entries survived the sweep: {names:?}");
        let dh = c.open_dir("del-while-enum").unwrap();
        c.vfs
            .can_delete(&cx(), dh)
            .expect("the swept directory must be deletable");
        c.vfs.cleanup(&cx(), dh, CleanupFlags::DELETE);
        c.vfs.close(&cx(), dh);
    });
}

fn order_is_dot_dotdot_then_folded_ascending<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "enumeration order", || {
        let d = c.create_dir("ord");
        c.close(d);
        // Deliberately created out of order and in mixed case.
        for n in ["Zebra", "apple", "Mango", "banana", "Cherry"] {
            c.file_with(&format!("ord\\{n}"), b"x");
        }
        let names = c.list("ord");
        assert_eq!(
            names,
            vec![".", "..", "apple", "banana", "Cherry", "Mango", "Zebra"],
            "order must be '.', '..', then children in ascending folded-name order"
        );
    });
}

fn the_root_omits_both_dot_entries<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "the root omits . and ..", || {
        let root = c.raw("\\").unwrap();
        let h = c
            .vfs
            .open(
                &cx(),
                &root,
                OpenOptions {
                    create_options: FILE_DIRECTORY_FILE,
                    granted_access: 0,
                },
            )
            .unwrap()
            .handle;
        let names = c.list_handle(h, None);
        c.close(h);
        assert!(
            !names.iter().any(|n| n == "." || n == ".."),
            "the root must omit both dot entries: {names:?}"
        );
    });
}

fn marker_yields_entries_strictly_after<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "marker is a resume point", || {
        let d = c.create_dir("mark");
        c.close(d);
        for n in ["a", "b", "c", "d", "e"] {
            c.file_with(&format!("mark\\{n}"), b"x");
        }
        let h = c.open_dir("mark").unwrap();

        let all = c.list_handle(h, None);
        assert_eq!(all, vec![".", "..", "a", "b", "c", "d", "e"]);

        // Strictly after, never including the marker itself.
        let after_c = c.list_handle(h, Some("c"));
        assert_eq!(after_c, vec!["d", "e"]);

        let after_dotdot = c.list_handle(h, Some(".."));
        assert_eq!(after_dotdot, vec!["a", "b", "c", "d", "e"]);

        let after_last = c.list_handle(h, Some("e"));
        assert!(after_last.is_empty(), "marker at the last entry must yield nothing");

        c.close(h);
    });
}

fn marker_chaining_yields_each_entry_exactly_once<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-DIR-2 marker chaining", || {
        // The executable form of the marker bug that silently truncates large
        // directories: correct at 50 entries and truncated at 5,000 is the
        // classic symptom, so this walks a directory in every buffer size from
        // 1 upward.
        let d = c.create_dir("chain");
        c.close(d);
        const N: usize = 60;
        for i in 0..N {
            c.file_with(&format!("chain\\f{i:03}"), b"x");
        }
        let h = c.open_dir("chain").unwrap();

        let full = c.list_handle(h, None);
        assert_eq!(full.len(), N + 2, "expected N + 2 entries");

        for buf_size in [1usize, 2, 3, 7, 17, N + 2, N + 100] {
            let mut collected: Vec<String> = Vec::new();
            let mut marker: Option<String> = None;
            let mut rounds = 0;

            loop {
                rounds += 1;
                assert!(rounds < N + 10, "enumeration did not terminate (buf {buf_size})");

                let cur = c.vfs.dir_open(&cx(), h, None, marker.as_deref()).unwrap();
                let mut batch = Vec::new();
                while batch.len() < buf_size {
                    match c.vfs.dir_next(&cx(), cur).unwrap() {
                        Some(e) => batch.push(e.name),
                        None => break,
                    }
                }
                // A cursor is valid for one ReadDirectory call only: closed
                // here, resumption carried by the marker.
                c.vfs.dir_close(&cx(), cur);

                if batch.is_empty() {
                    break;
                }
                marker = Some(batch.last().unwrap().clone());
                collected.extend(batch);
            }

            assert_eq!(
                collected, full,
                "marker chaining diverged at buffer size {buf_size}"
            );
            let mut sorted = collected.clone();
            sorted.sort();
            let before = sorted.len();
            sorted.dedup();
            assert_eq!(before, sorted.len(), "duplicate entries at buffer size {buf_size}");
        }

        c.close(h);
    });
}

fn enumeration_terminates_within_n_plus_2<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-DIR-1 termination", || {
        let d = c.create_dir("term");
        c.close(d);
        const N: usize = 10;
        for i in 0..N {
            c.file_with(&format!("term\\t{i}"), b"x");
        }
        let h = c.open_dir("term").unwrap();
        let cur = c.vfs.dir_open(&cx(), h, None, None).unwrap();
        let mut count = 0;
        while c.vfs.dir_next(&cx(), cur).unwrap().is_some() {
            count += 1;
            assert!(count <= N + 2, "enumeration exceeded N + 2 entries");
        }
        assert_eq!(count, N + 2);
        // Draining past the end keeps returning None rather than restarting.
        assert!(c.vfs.dir_next(&cx(), cur).unwrap().is_none());
        assert!(c.vfs.dir_next(&cx(), cur).unwrap().is_none());
        c.vfs.dir_close(&cx(), cur);
        c.close(h);
    });
}

fn pattern_is_accepted_and_ignored<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "pattern is accepted and ignored", || {
        // Recorded as a dependency on WinFsp's filtering, not a gap: if Phase 2
        // ever takes over pattern matching, this test is the thing that has to
        // change deliberately.
        let d = c.create_dir("pat");
        c.close(d);
        for n in ["a.txt", "b.log", "c.txt"] {
            c.file_with(&format!("pat\\{n}"), b"x");
        }
        let h = c.open_dir("pat").unwrap();
        let cur = c.vfs.dir_open(&cx(), h, Some("*.txt"), None).unwrap();
        let mut names = Vec::new();
        while let Some(e) = c.vfs.dir_next(&cx(), cur).unwrap() {
            names.push(e.name);
        }
        c.vfs.dir_close(&cx(), cur);
        c.close(h);
        assert!(
            names.iter().any(|n| n == "b.log"),
            "pattern must be ignored in Phase 1, got {names:?}"
        );
    });
}

fn enumerating_a_file_is_not_a_directory<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "enumerate a file", || {
        c.file_with("notdir.txt", b"x");
        let h = c.open_file("notdir.txt").unwrap();
        expect_err(
            "dir_open on a file",
            ErrorCode::NotADirectory,
            c.vfs.dir_open(&cx(), h, None, None),
        );
        c.close(h);
    });
}

fn a_stale_cursor_is_a_controlled_error<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-DIR-3 stale cursor", || {
        let d = c.create_dir("stale");
        c.close(d);
        c.file_with("stale\\x", b"x");
        let h = c.open_dir("stale").unwrap();

        let cur = c.vfs.dir_open(&cx(), h, None, None).unwrap();
        c.vfs.dir_close(&cx(), cur);

        // A closed cursor resolves to a controlled error, never to another
        // directory's state.
        expect_err(
            "dir_next on a closed cursor",
            ErrorCode::InvalidParameter,
            c.vfs.dir_next(&cx(), cur),
        );
        // Closing twice is a silent no-op (`void`).
        c.vfs.dir_close(&cx(), cur);

        // Never-allocated and forged cursors.
        for raw in [0u64, 1, u64::MAX, 0xDEAD_BEEF, cur.as_raw() ^ 0xFFFF] {

            let bogus = CursorId::from_raw(raw);
            assert!(
                c.vfs.dir_next(&cx(), bogus).is_err(),
                "a forged cursor {raw:#x} resolved"
            );
            c.vfs.dir_close(&cx(), bogus);
        }

        c.close(h);
    });
}

fn dot_entries_carry_the_right_info<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "dot entries", || {
        let outer = c.create_dir("dots");
        let outer_index = c.vfs.file_info(&cx(), outer).unwrap().index_number;
        c.close(outer);
        let inner = c.create_dir("dots\\inner");
        let inner_index = c.vfs.file_info(&cx(), inner).unwrap().index_number;

        let cur = c.vfs.dir_open(&cx(), inner, None, None).unwrap();
        let dot = c.vfs.dir_next(&cx(), cur).unwrap().unwrap();
        let dotdot = c.vfs.dir_next(&cx(), cur).unwrap().unwrap();
        c.vfs.dir_close(&cx(), cur);
        c.close(inner);

        assert_eq!(dot.name, ".");
        assert_eq!(dot.info.index_number, inner_index, "'.' must describe the directory itself");
        assert!(dot.info.is_dir());
        assert_eq!(dotdot.name, "..");
        assert_eq!(dotdot.info.index_number, outer_index, "'..' must describe the parent");
        assert!(dotdot.info.is_dir());
    });
}

fn an_empty_directory_yields_only_dot_entries<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "empty directory", || {
        let d = c.create_dir("empty");
        c.close(d);
        assert_eq!(c.list("empty"), vec![".", ".."]);
    });
}
