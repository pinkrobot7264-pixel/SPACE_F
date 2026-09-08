//! The invariant checker (Phase 1 §5.3).
//!
//! **No branch of this file mutates anything.** That is the §3.5 read-only
//! rule, and `checker_is_read_only_*` proves it by snapshot comparison.
//!
//! Implemented *with* the model rather than later, so that every test written
//! from Session 5 onward is an invariant test at no authoring cost.

use std::collections::{HashMap, HashSet};

use crate::vfs::ids::NodeId;
use crate::vfs::invariants::{viol, InvariantViolation, VfsDiagnostics};
use crate::vfs::limits::ALLOCATION_UNIT;

use super::imp::MemVfs;

impl VfsDiagnostics for MemVfs {
    fn check_invariants(&self) -> Result<(), InvariantViolation> {
        // Diagnostic path: an unbounded wait is correct here. A checker that
        // gave up on a timeout would report "healthy" for a wedged filesystem.
        let s = self.state_blocking();

        let mut index_numbers = HashSet::new();
        let mut counted: HashMap<NodeId, u32> = HashMap::new();

        // INV-NS-1 -- the root exists, is a directory, and has no parent.
        let root = s
            .nodes
            .resolve(s.root)
            .map_err(|_| viol("INV-NS-1", "root missing"))?;
        if root.parent.is_some() {
            return Err(viol("INV-NS-1", "root has a parent"));
        }
        if !root.is_dir {
            return Err(viol("INV-NS-1", "root is not a directory"));
        }
        if root.unlinked {
            return Err(viol("INV-NS-1", "root is unlinked"));
        }

        for (id, node) in s.nodes.iter() {
            // INV-ID-2 -- unique index_number, never reused.
            if !index_numbers.insert(node.index_number) {
                return Err(viol(
                    "INV-ID-2",
                    format!("duplicate index_number {}", node.index_number),
                ));
            }

            // INV-FS-1 -- file_size <= allocation_size, allocation is a
            // multiple of the allocation unit.
            let alloc = node.allocation_size();
            if node.file_size() > alloc {
                return Err(viol(
                    "INV-FS-1",
                    format!(
                        "file_size {} > allocation_size {} on {:?}",
                        node.file_size(),
                        alloc,
                        id
                    ),
                ));
            }
            if alloc % ALLOCATION_UNIT != 0 {
                return Err(viol(
                    "INV-FS-1",
                    format!("allocation_size {alloc} is not a multiple of {ALLOCATION_UNIT}"),
                ));
            }

            // INV-NS-2 -- every live LINKED non-root node has exactly one
            // parent and appears exactly once in that parent's child map.
            // Unlinked nodes are exempt by fs-semantics §2. Deliberate, not an
            // oversight.
            if id != s.root && !node.unlinked {
                let p = node
                    .parent
                    .ok_or_else(|| viol("INV-NS-2", "linked non-root node without a parent"))?;
                let parent = s
                    .nodes
                    .resolve(p)
                    .map_err(|_| viol("INV-NS-2", "dangling parent"))?;
                if parent.children.values().filter(|c| **c == id).count() != 1 {
                    return Err(viol(
                        "INV-NS-2",
                        format!("node {:?} not exactly once in parent {:?}", id, p),
                    ));
                }
            }

            // INV-NS-3 / INV-NS-4 -- walk the parent chain.
            let (mut cur, mut steps) = (node.parent, 0usize);
            while let Some(p) = cur {
                if p == id {
                    return Err(viol("INV-NS-4", format!("node {id:?} is its own ancestor")));
                }
                steps += 1;
                if steps > s.cfg.path_limits.max_path_depth {
                    return Err(viol("INV-NS-3", "parent cycle or excessive depth"));
                }
                cur = s.nodes.resolve(p).ok().and_then(|n| n.parent);
            }

            // INV-RES-1 -- L6, entries per directory.
            if node.is_dir && node.children.len() > s.cfg.limits.max_dir_entries {
                return Err(viol("INV-RES-1", "directory entry limit (L6) exceeded"));
            }

            // INV-NS-5 -- no two children of one directory have colliding
            // folded names. BTreeMap keys are unique by construction; assert
            // the map agrees with the display names it indexes, which is where
            // a rename bug would actually show up.
            if node.is_dir {
                let mut folded = HashSet::new();
                for (key, child) in node.children.iter() {
                    if !folded.insert(key.clone()) {
                        return Err(viol(
                            "INV-NS-5",
                            format!("colliding folded name {key} in {id:?}"),
                        ));
                    }
                    if let Ok(c) = s.nodes.resolve(*child) {
                        let expected =
                            crate::vfs::path::FoldedName::new(&c.name, s.cfg.capabilities);
                        if expected != *key {
                            return Err(viol(
                                "INV-NS-5",
                                format!("child key {key} disagrees with display name {:?}", c.name),
                            ));
                        }
                    }
                }
            }
        }

        // INV-ID-4 / INV-FS-2 -- handles refer to live nodes, and open_count
        // agrees with the number of live handles.
        for (_, slot) in s.handles.iter() {
            if s.nodes.resolve(slot.node).is_err() {
                return Err(viol("INV-ID-4", "handle references a dead node"));
            }
            *counted.entry(slot.node).or_default() += 1;
        }
        for (id, n) in counted.iter() {
            let node = s
                .nodes
                .resolve(*id)
                .map_err(|_| viol("INV-ID-4", "handle references a dead node"))?;
            if node.open_count != *n {
                return Err(viol(
                    "INV-FS-2",
                    format!(
                        "open_count {} disagrees with {} live handles on {:?}",
                        node.open_count, n, id
                    ),
                ));
            }
        }
        // The other direction: a node claiming opens that no handle backs.
        for (id, node) in s.nodes.iter() {
            let live = counted.get(&id).copied().unwrap_or(0);
            if node.open_count != live {
                return Err(viol(
                    "INV-FS-2",
                    format!(
                        "open_count {} disagrees with {} live handles on {:?}",
                        node.open_count, live, id
                    ),
                ));
            }
        }

        // INV-FS-4 -- an unlinked node with no handles should already be
        // reclaimed. Reclamation is deterministic: exactly at the Close that
        // brings open_count to zero.
        for (id, node) in s.nodes.iter() {
            if node.unlinked && node.open_count == 0 {
                return Err(viol(
                    "INV-FS-4",
                    format!("unlinked node {id:?} with no handles was not reclaimed"),
                ));
            }
        }

        // INV-RES-1 -- every limit holds.
        if s.total_bytes > s.cfg.limits.max_bytes {
            return Err(viol("INV-RES-1", "total_bytes exceeds max_bytes (L5)"));
        }
        if s.handles.live_count() > s.cfg.limits.max_open_handles {
            return Err(viol("INV-RES-1", "handle limit (L7) exceeded"));
        }
        if s.cursors.live_count() > s.cfg.limits.max_open_cursors {
            return Err(viol("INV-RES-1", "cursor limit (L8) exceeded"));
        }

        Ok(())
    }
}
