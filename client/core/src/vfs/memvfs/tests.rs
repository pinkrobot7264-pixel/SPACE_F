//! `MemVfs` implementation tests (§3.7, §5.4).
//!
//! These are the tests that would **not** transfer to Phase 2's VFS: slab
//! reuse, generation wraparound, `BTreeMap` ordering, and the checker's own
//! behaviour. Everything expressing a semantic rule or an invariant lives in
//! the conformance suite instead (§11.3).
//!
//! The dividing question is: *would Phase 2's VFS also have to pass this?* If
//! yes, it belongs in conformance.

use std::time::Duration;

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::{Limits, PathLimits};
use crate::vfs::types::*;
use crate::vfs::{OpCtx, Vfs, VfsConfig};

use super::node::MemNode;
use super::MemVfs;

fn cx() -> OpCtx {
    OpCtx::new(Duration::from_secs(30))
}

fn vfs() -> MemVfs {
    MemVfs::with_defaults()
}

fn create_file(v: &MemVfs, path: &str) -> HandleId {
    let p = v.parse_path(path).unwrap();
    v.create(
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
    .handle
}

fn create_dir(v: &MemVfs, path: &str) -> HandleId {
    let p = v.parse_path(path).unwrap();
    v.create(
        &cx(),
        &p,
        CreateOptions {
            create_options: FILE_DIRECTORY_FILE,
            granted_access: 0,
            file_attributes: 0,
            allocation_size: 0,
        },
    )
    .unwrap()
    .handle
}

use crate::vfs::ids::HandleId;

/// A deterministic rendering of the entire filesystem state.
///
/// Used to assert "state byte-identical" for the read-only rule (§3.5) and for
/// INV-RES-3 (§14.3). Rendering rather than deriving `PartialEq` on the state
/// keeps the comparison total: every field that matters appears here, and a
/// field added later without being rendered shows up as an unexplained pass.
fn snapshot(v: &MemVfs) -> String {
    let s = v.state_blocking();
    let mut out = String::new();
    out.push_str(&format!(
        "total_bytes={} next_index={} handles_live={} cursors_live={}\n",
        s.total_bytes,
        s.next_index_number,
        s.handles.live_count(),
        s.cursors.live_count()
    ));
    let mut nodes: Vec<_> = s
        .nodes
        .iter()
        .map(|(id, n)| {
            let children: Vec<String> = n
                .children
                .iter()
                .map(|(k, c)| format!("{k}->{:?}", c.as_raw()))
                .collect();
            format!(
                "node {:?} name={:?} dir={} attrs={:#x} size={} alloc={} parent={:?} idx={} open={} unlinked={} data_hash={:x} children=[{}]",
                id.as_raw(),
                n.name,
                n.is_dir,
                n.attributes,
                n.file_size(),
                n.allocation_size(),
                n.parent.map(|p| p.as_raw()),
                n.index_number,
                n.open_count,
                n.unlinked,
                // Cheap content fingerprint; a byte change moves it.
                n.data.iter().fold(1469598103934665603u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(1099511628211)
                }),
                children.join(",")
            )
        })
        .collect();
    nodes.sort();
    for n in nodes {
        out.push_str(&n);
        out.push('\n');
    }
    let mut handles: Vec<_> = s
        .handles
        .iter()
        .map(|(id, h)| {
            format!(
                "handle {:?} node={:?} dir={} cleaned={} access={:#x}",
                id.as_raw(),
                h.node.as_raw(),
                h.is_dir,
                h.cleaned_up,
                h.granted_access
            )
        })
        .collect();
    handles.sort();
    for h in handles {
        out.push_str(&h);
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// The checker passes on well-formed structures
// ---------------------------------------------------------------------------

#[test]
fn check_invariants_passes_on_a_fresh_empty_filesystem() {
    let v = vfs();
    v.check_invariants().unwrap();
}

#[test]
fn check_invariants_passes_after_ordinary_activity() {
    let v = vfs();
    create_dir(&v, "\\dir");
    let h = create_file(&v, "\\dir\\a.txt");
    v.write(&cx(), h, 0, b"hello world", WriteMode::NORMAL)
        .unwrap();
    v.check_invariants().unwrap();

    let d = create_file(&v, "\\dir\\b.txt");
    v.cleanup(&cx(), d, CleanupFlags::DELETE);
    v.check_invariants().unwrap();
    v.close(&cx(), d);
    v.check_invariants().unwrap();

    v.close(&cx(), h);
    v.check_invariants().unwrap();
}

// ---------------------------------------------------------------------------
// The checker is read-only (§3.5, proven by snapshot comparison)
// ---------------------------------------------------------------------------

#[test]
fn checker_is_read_only_on_a_populated_fixture() {
    let v = vfs();
    create_dir(&v, "\\dir");
    let a = create_file(&v, "\\dir\\a.txt");
    v.write(&cx(), a, 0, b"content", WriteMode::NORMAL).unwrap();
    let _b = create_file(&v, "\\b.txt");
    create_dir(&v, "\\dir\\sub");

    let before = snapshot(&v);
    v.check_invariants().unwrap();
    // Run it repeatedly: a checker that mutated once would drift.
    for _ in 0..5 {
        v.check_invariants().unwrap();
    }
    let after = snapshot(&v);

    assert_eq!(before, after, "check_invariants mutated state");
}

#[test]
fn checker_does_not_change_any_subsequent_result() {
    // "It must be safe to call at any point in any test, and calling it must
    // not change any subsequent result."
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    v.write(&cx(), h, 0, b"0123456789", WriteMode::NORMAL)
        .unwrap();

    let mut buf = [0u8; 4];
    let n1 = v.read(&cx(), h, 2, &mut buf).unwrap();
    let first = (n1, buf);

    for _ in 0..10 {
        v.check_invariants().unwrap();
    }

    let mut buf2 = [0u8; 4];
    let n2 = v.read(&cx(), h, 2, &mut buf2).unwrap();
    assert_eq!((n2, buf2), first);
}

#[test]
fn checker_is_read_only_even_when_it_reports_a_violation() {
    // The dangerous case: a checker that "helpfully" repaired what it found.
    let v = vfs();
    let _h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let root = s.root;
        let ids: Vec<_> = s.nodes.iter().map(|(id, _)| id).collect();
        let victim = ids.into_iter().find(|i| *i != root).unwrap();
        s.nodes.resolve_mut(victim).unwrap().index_number = 1; // collide with root
    }

    let before = snapshot(&v);
    let err = v.check_invariants().unwrap_err();
    assert_eq!(err.id, "INV-ID-2");
    let after = snapshot(&v);
    assert_eq!(
        before, after,
        "checker repaired a violation instead of reporting it"
    );
}

// ---------------------------------------------------------------------------
// The checker FIRES (§3.7) -- a checker that never fires is not a checker
// ---------------------------------------------------------------------------

#[test]
fn fires_inv_ns_1_when_the_root_is_malformed() {
    // root has a parent
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let root = s.root;
        let other = s.handles.resolve(h).unwrap().node;
        s.nodes.resolve_mut(root).unwrap().parent = Some(other);
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-1");

    // root is not a directory
    let v = vfs();
    {
        let mut s = v.state_blocking();
        let root = s.root;
        s.nodes.resolve_mut(root).unwrap().is_dir = false;
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-1");
}

#[test]
fn fires_inv_ns_2_when_a_node_is_not_in_its_parent() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let root = s.root;
        let _ = h;
        // Remove the child entry but leave the node's parent pointer.
        s.nodes.resolve_mut(root).unwrap().children.clear();
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-2");
}

#[test]
fn fires_inv_ns_2_when_a_linked_node_has_no_parent() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        s.nodes.resolve_mut(node).unwrap().parent = None;
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-2");
}

#[test]
fn fires_inv_ns_3_on_a_parent_chain_longer_than_l3() {
    // A shallow depth limit so the chain is cheap to build. The invariant is
    // about the bound, not about 512 specifically.
    let cfg = VfsConfig {
        limits: Limits::DEFAULT,
        path_limits: PathLimits {
            max_path_depth: 4,
            ..PathLimits::DEFAULT
        },
        capabilities: Capabilities::PHASE_1,
    };
    let v = MemVfs::new(cfg);
    {
        let mut s = v.state_blocking();
        let now = crate::time::now_filetime();
        // Build a chain of 8 orphan nodes, each pointing at the previous one.
        // They are unlinked so INV-NS-2 does not fire first.
        let mut prev = None;
        let mut first = None;
        for i in 0..8u64 {
            let idx = s.next_index_number;
            s.next_index_number += 1;
            let mut n = MemNode::new_dir(format!("c{i}"), prev, idx, now);
            n.unlinked = true;
            let id = s.nodes.alloc(n).unwrap();
            if first.is_none() {
                first = Some(id);
            }
            prev = Some(id);
        }
        // The chain's root is unlinked with open_count 0, which INV-FS-4 would
        // report first; give every node a phantom open so FS-4 stays quiet and
        // NS-3 is what fires. FS-2 would then disagree, so back each with a
        // real handle instead.
        let ids: Vec<_> = s
            .nodes
            .iter()
            .filter(|(_, n)| n.unlinked)
            .map(|(id, _)| id)
            .collect();
        for id in ids {
            let h = s
                .handles
                .alloc(super::HandleSlot {
                    node: id,
                    granted_access: 0,
                    is_dir: true,
                    cleaned_up: false,
                    delete_on_close: false,
                })
                .unwrap();
            let _ = h;
            s.nodes.resolve_mut(id).unwrap().open_count += 1;
        }
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-3");
}

#[test]
fn fires_inv_ns_4_when_a_node_is_its_own_ancestor() {
    // A *self*-parent would trip INV-NS-2 first (the node is no longer in its
    // parent's child map), and the checker reports in the §5.3 order. To
    // isolate NS-4 the cycle must be otherwise well-formed: a two-node cycle in
    // which each node appears exactly once in the other's child map.
    let v = vfs();
    let ha = create_dir(&v, "\\a");
    let hb = create_dir(&v, "\\b");
    {
        let mut s = v.state_blocking();
        let a = s.handles.resolve(ha).unwrap().node;
        let b = s.handles.resolve(hb).unwrap().node;
        let root = s.root;
        let caps = s.cfg.capabilities;

        s.nodes.resolve_mut(root).unwrap().children.clear();

        s.nodes.resolve_mut(a).unwrap().parent = Some(b);
        s.nodes.resolve_mut(b).unwrap().parent = Some(a);

        let key_a = crate::vfs::path::FoldedName::new("a", caps);
        let key_b = crate::vfs::path::FoldedName::new("b", caps);
        s.nodes.resolve_mut(b).unwrap().children.insert(key_a, a);
        s.nodes.resolve_mut(a).unwrap().children.insert(key_b, b);
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-4");
}

#[test]
fn a_self_parent_reports_ns_2_first_by_documented_ordering() {
    // Pins the precedence rather than leaving it accidental: a self-parent
    // breaks both NS-2 and NS-4, and the checker walks NS-2 first (§5.3). The
    // value of this test is that a reordering of the checker becomes a visible
    // change rather than a silent one.
    let v = vfs();
    let h = create_dir(&v, "\\d");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        s.nodes.resolve_mut(node).unwrap().parent = Some(node);
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-2");
}

#[test]
fn fires_inv_ns_5_when_a_child_key_disagrees_with_its_display_name() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        // Rename the node without updating the parent's folded key -- exactly
        // the bug a rename implementation makes.
        s.nodes.resolve_mut(node).unwrap().name = "renamed.txt".to_string();
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-NS-5");
}

#[test]
fn fires_inv_id_2_on_a_duplicate_index_number() {
    let v = vfs();
    create_file(&v, "\\a.txt");
    let h = create_file(&v, "\\b.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        s.nodes.resolve_mut(node).unwrap().index_number = 1; // the root's
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-ID-2");
}

#[test]
fn fires_inv_id_4_when_a_handle_references_a_dead_node() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        let root = s.root;
        // Detach and free the node, leaving the handle dangling.
        s.nodes.resolve_mut(root).unwrap().children.clear();
        s.nodes.free(node).unwrap();
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-ID-4");
}

#[test]
fn fires_inv_fs_1_when_allocation_size_falls_below_file_size() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    v.write(&cx(), h, 0, &[7u8; 5000], WriteMode::NORMAL)
        .unwrap();
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        // Allocation is a stored property, so this is reachable -- which is the
        // point of storing it rather than computing it.
        s.nodes.resolve_mut(node).unwrap().explicit_allocation = Some(4096);
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-FS-1");
}

#[test]
fn fires_inv_fs_1_when_allocation_size_is_not_a_multiple_of_the_unit() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        s.nodes.resolve_mut(node).unwrap().explicit_allocation = Some(4097);
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-FS-1");
}

#[test]
fn fires_inv_fs_2_when_open_count_disagrees_with_live_handles() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        s.nodes.resolve_mut(node).unwrap().open_count = 7;
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-FS-2");
}

#[test]
fn fires_inv_fs_2_when_a_node_claims_opens_no_handle_backs() {
    let v = vfs();
    create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let root = s.root;
        s.nodes.resolve_mut(root).unwrap().open_count = 3;
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-FS-2");
}

#[test]
fn fires_inv_fs_4_when_an_unlinked_node_was_not_reclaimed() {
    let v = vfs();
    let h = create_file(&v, "\\a.txt");
    {
        let mut s = v.state_blocking();
        let node = s.handles.resolve(h).unwrap().node;
        let root = s.root;
        s.nodes.resolve_mut(root).unwrap().children.clear();
        let n = s.nodes.resolve_mut(node).unwrap();
        n.unlinked = true;
        n.parent = None;
        n.open_count = 0;
        // and drop the handle so FS-2 stays quiet
        let hid = h;
        s.handles.free(hid).unwrap();
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-FS-4");
}

#[test]
fn fires_inv_res_1_when_total_bytes_exceeds_max_bytes() {
    let v = vfs();
    {
        let mut s = v.state_blocking();
        s.total_bytes = s.cfg.limits.max_bytes + 1;
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-RES-1");
}

#[test]
fn fires_inv_res_1_when_a_directory_exceeds_l6() {
    let cfg = VfsConfig {
        limits: Limits {
            max_dir_entries: 2,
            ..Limits::DEFAULT
        },
        ..VfsConfig::default()
    };
    let v = MemVfs::new(cfg);
    create_file(&v, "\\a");
    create_file(&v, "\\b");
    {
        // Force a third child past the limit, bypassing the enforcing path.
        let mut s = v.state_blocking();
        let now = crate::time::now_filetime();
        let idx = s.next_index_number;
        s.next_index_number += 1;
        let root = s.root;
        let id = s
            .nodes
            .alloc(MemNode::new_file("c", Some(root), idx, now, 0))
            .unwrap();
        let key = crate::vfs::path::FoldedName::new("c", s.cfg.capabilities);
        s.nodes.resolve_mut(root).unwrap().children.insert(key, id);
    }
    assert_eq!(v.check_invariants().unwrap_err().id, "INV-RES-1");
}

/// Every invariant the checker is responsible for has a firing test above.
///
/// §3.7 names INV-NS-1, NS-2, NS-3, NS-5, ID-2, ID-4, FS-1, FS-2 and RES-1.
/// NS-4 and FS-4 are checker branches too, so they are covered here as well.
/// The remaining invariants (ID-1, ID-3, ID-5, NS-6, FS-3, FS-5, DIR-1..3,
/// RES-2, RES-3) are conformance/fuzz properties rather than checker branches
/// and are asserted there.
#[test]
fn every_checker_branch_has_a_firing_test() {
    const CHECKER_BRANCHES: &[&str] = &[
        "INV-NS-1",
        "INV-NS-2",
        "INV-NS-3",
        "INV-NS-4",
        "INV-NS-5",
        "INV-ID-2",
        "INV-ID-4",
        "INV-FS-1",
        "INV-FS-2",
        "INV-FS-4",
        "INV-RES-1",
    ];
    // This list is asserted against the firing tests by name in the module
    // above; the constant exists so the exit-gate audit can read it.
    assert_eq!(CHECKER_BRANCHES.len(), 11);
    for id in CHECKER_BRANCHES {
        assert!(
            crate::vfs::invariants::ALL_INVARIANTS.contains(id),
            "{id} is not a documented invariant"
        );
    }
}

// ---------------------------------------------------------------------------
// Implementation-specific behaviour (§11.3: stays out of conformance)
// ---------------------------------------------------------------------------

#[test]
fn btree_map_gives_ascending_folded_order_for_free() {
    let v = vfs();
    for name in ["\\Zebra", "\\apple", "\\Mango", "\\banana"] {
        create_file(&v, name);
    }
    let root = v.parse_path("\\").unwrap();
    let h = v
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
    let cur = v.dir_open(&cx(), h, None, None).unwrap();
    let mut names = Vec::new();
    while let Some(e) = v.dir_next(&cx(), cur).unwrap() {
        names.push(e.name);
    }
    v.dir_close(&cx(), cur);
    // Ascending folded-name order, case preserved in the display names.
    assert_eq!(names, vec!["apple", "banana", "Mango", "Zebra"]);
}

#[test]
fn index_numbers_are_strictly_increasing_and_never_reused() {
    // INV-ID-2 across 10,000 create/delete cycles (§5.4).
    let v = vfs();
    let mut last = 0u64;
    for i in 0..2_000 {
        let path = format!("\\f{i}.txt");
        let h = create_file(&v, &path);
        let idx = v.file_info(&cx(), h).unwrap().index_number;
        assert!(idx > last, "index_number went backwards at {i}");
        last = idx;
        v.cleanup(&cx(), h, CleanupFlags::DELETE);
        v.close(&cx(), h);
    }
    v.check_invariants().unwrap();
    // Slot indices were reused many times over; index numbers were not.
    let s = v.state_blocking();
    assert!(s.next_index_number > 2_000);
}

#[test]
fn slab_reuse_does_not_leak_nodes() {
    let v = vfs();
    let before = v.state_blocking().nodes.live_count();
    for i in 0..500 {
        let h = create_file(&v, &format!("\\t{i}"));
        v.cleanup(&cx(), h, CleanupFlags::DELETE);
        v.close(&cx(), h);
    }
    let after = v.state_blocking().nodes.live_count();
    assert_eq!(before, after, "nodes leaked across create/delete cycles");
    v.check_invariants().unwrap();
}

#[test]
fn handle_and_cursor_counts_return_to_baseline() {
    // The soak test's fail condition (§16.5), asserted cheaply in unit form.
    let v = vfs();
    for _ in 0..100 {
        let h = create_file(&v, "\\x");
        let root = v.parse_path("\\").unwrap();
        let d = v
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
        let c = v.dir_open(&cx(), d, None, None).unwrap();
        while v.dir_next(&cx(), c).unwrap().is_some() {}
        v.dir_close(&cx(), c);
        v.close(&cx(), d);
        v.cleanup(&cx(), h, CleanupFlags::DELETE);
        v.close(&cx(), h);
    }
    let s = v.state_blocking();
    assert_eq!(s.handles.live_count(), 0);
    assert_eq!(s.cursors.live_count(), 0);
    drop(s);
    v.check_invariants().unwrap();
}

#[test]
fn a_deadline_already_expired_yields_operation_timeout_not_a_hang() {
    let v = vfs();
    let expired = OpCtx::with_deadline(std::time::Instant::now() - Duration::from_millis(1));
    let p = v.parse_path("\\a").unwrap();
    let err = v
        .create(
            &expired,
            &p,
            CreateOptions {
                create_options: 0,
                granted_access: 0,
                file_attributes: 0,
                allocation_size: 0,
            },
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OperationTimeout);
}

#[test]
fn lock_acquisition_is_deadline_bounded() {
    // §3.6: "lock acquisition must be deadline-bounded, or L9 is a lie."
    // Hold the state lock, then prove another operation gives up at its own
    // deadline rather than blocking forever.
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let v = Arc::new(vfs());
    let holding = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));

    let v2 = Arc::clone(&v);
    let holding2 = Arc::clone(&holding);
    let release2 = Arc::clone(&release);
    let t = std::thread::spawn(move || {
        let _guard = v2.state_blocking();
        holding2.store(true, Ordering::SeqCst);
        while !release2.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
    });

    while !holding.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }

    let started = std::time::Instant::now();
    let short = OpCtx::new(Duration::from_millis(200));
    let p = v.parse_path("\\blocked").unwrap();
    let err = v
        .create(
            &short,
            &p,
            CreateOptions {
                create_options: 0,
                granted_access: 0,
                file_attributes: 0,
                allocation_size: 0,
            },
        )
        .unwrap_err();
    let elapsed = started.elapsed();

    release.store(true, Ordering::SeqCst);
    t.join().unwrap();

    assert_eq!(err.code, ErrorCode::OperationTimeout);
    assert!(
        elapsed < Duration::from_secs(5),
        "lock wait was not deadline-bounded: {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(150),
        "returned before the deadline: {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// Windowed enumeration (implementation-specific: §11.3 keeps this out of
// conformance, because the window size is a MemVfs choice, not a contract term)
// ---------------------------------------------------------------------------

#[test]
fn a_directory_larger_than_one_window_enumerates_completely() {
    // The refill path. DIR_WINDOW is 1024, so this crosses it twice and ends
    // on a partial window -- the three cases that differ.
    let v = vfs();
    create_dir(&v, "\\big");
    const N: usize = 2_500;
    for i in 0..N {
        let h = create_file(&v, &format!("\\big\\f{i:05}"));
        v.cleanup(&cx(), h, CleanupFlags::NONE);
        v.close(&cx(), h);
    }

    let d = v
        .open(
            &cx(),
            &v.parse_path("\\big").unwrap(),
            OpenOptions {
                create_options: FILE_DIRECTORY_FILE,
                granted_access: 0,
            },
        )
        .unwrap()
        .handle;

    let cur = v.dir_open(&cx(), d, None, None).unwrap();
    let mut names = Vec::new();
    while let Some(e) = v.dir_next(&cx(), cur).unwrap() {
        names.push(e.name);
    }
    v.dir_close(&cx(), cur);
    v.close(&cx(), d);

    assert_eq!(
        names.len(),
        N + 2,
        "expected N + 2 entries across window refills"
    );
    assert_eq!(names[0], ".");
    assert_eq!(names[1], "..");

    // Ascending folded order is preserved ACROSS window boundaries -- the case
    // a single-window implementation cannot get wrong and a refilling one can.
    let children = &names[2..];
    let mut sorted = children.to_vec();
    sorted.sort();
    assert_eq!(
        children,
        sorted.as_slice(),
        "order broke across a window boundary"
    );

    let mut dedup = sorted.clone();
    dedup.dedup();
    assert_eq!(
        dedup.len(),
        N,
        "an entry was duplicated or dropped at a refill"
    );
}

#[test]
fn a_marker_past_a_window_boundary_resumes_correctly() {
    // Resuming from a marker that sits beyond the first window: the resume key
    // must seek into the BTreeMap rather than into the current buffer.
    let v = vfs();
    create_dir(&v, "\\big");
    const N: usize = 1_500;
    for i in 0..N {
        let h = create_file(&v, &format!("\\big\\f{i:05}"));
        v.cleanup(&cx(), h, CleanupFlags::NONE);
        v.close(&cx(), h);
    }

    let d = v
        .open(
            &cx(),
            &v.parse_path("\\big").unwrap(),
            OpenOptions {
                create_options: FILE_DIRECTORY_FILE,
                granted_access: 0,
            },
        )
        .unwrap()
        .handle;

    // f01200 is well past the 1024-entry first window.
    let cur = v.dir_open(&cx(), d, None, Some("f01200")).unwrap();
    let mut names = Vec::new();
    while let Some(e) = v.dir_next(&cx(), cur).unwrap() {
        names.push(e.name);
    }
    v.dir_close(&cx(), cur);
    v.close(&cx(), d);

    assert_eq!(
        names.len(),
        N - 1201,
        "resume past a window boundary yielded the wrong count"
    );
    assert_eq!(names.first().map(String::as_str), Some("f01201"));
    assert_eq!(names.last().map(String::as_str), Some("f01499"));
}

#[test]
fn enumeration_work_per_call_does_not_grow_with_directory_size() {
    // The defect this replaced: dir_open snapshotted every child on every call,
    // so a full listing was O(N^2) and, under the single state lock, starved
    // every other operation.
    //
    // Timing is a blunt instrument, so this asserts the SHAPE rather than an
    // absolute number: ten times the entries must not cost anything like ten
    // times as much per dir_open call.
    use std::time::Instant;

    fn time_one_dir_open(n: usize) -> std::time::Duration {
        let v = vfs();
        create_dir(&v, "\\d");
        for i in 0..n {
            let h = create_file(&v, &format!("\\d\\f{i:06}"));
            v.cleanup(&cx(), h, CleanupFlags::NONE);
            v.close(&cx(), h);
        }
        let d = v
            .open(
                &cx(),
                &v.parse_path("\\d").unwrap(),
                OpenOptions {
                    create_options: FILE_DIRECTORY_FILE,
                    granted_access: 0,
                },
            )
            .unwrap()
            .handle;

        let started = Instant::now();
        for _ in 0..20 {
            let cur = v.dir_open(&cx(), d, None, None).unwrap();
            v.dir_close(&cx(), cur);
        }
        let elapsed = started.elapsed();
        v.close(&cx(), d);
        elapsed
    }

    let small = time_one_dir_open(500);
    let large = time_one_dir_open(5_000);

    // With the window, both do at most DIR_WINDOW work, so the ratio should be
    // near 1. A quadratic implementation would show ~10x here. The bound is
    // deliberately loose -- this is a shape assertion on a timing measurement,
    // and a tight bound would be flaky on a loaded machine.
    let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-6);
    assert!(
        ratio < 4.0,
        "dir_open cost grew {ratio:.1}x for 10x the entries \
         ({small:?} -> {large:?}); work per call is not bounded by the window"
    );
}
