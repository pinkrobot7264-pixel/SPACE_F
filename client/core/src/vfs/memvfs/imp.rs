//! `MemVfs` -- the `Vfs` implementation (§5.1, §6-§10).
//!
//! Every method follows the same shape:
//!
//! ```text
//! cx.check()?          // OperationTimeout (ADR-0014)
//! self.state(cx)?      // deadline-bounded lock (§3.6)
//! resolve the handle   // InvalidHandle (INV-ID-3)
//! do the work
//! ```
//!
//! The deadline-bounded lock is load-bearing: an unbounded `lock()` here would
//! make L9 a lie, because a hung operation holding the lock would block every
//! other operation forever instead of for their own deadlines.

use contracts::{ErrorCode, SpaceError};
use parking_lot::{Mutex, MutexGuard};

use crate::time::now_filetime;
use crate::vfs::ids::{CursorId, HandleId, NodeId};
use crate::vfs::limits::allocation_size_for;
use crate::vfs::path::{FoldedName, VfsPath};
use crate::vfs::types::*;
use crate::vfs::{OpCtx, Vfs, VfsConfig};

use super::node::MemNode;
use super::{
    new_cursor_table, new_handle_table, new_node_table, CursorSlot, CursorTable, HandleSlot,
    HandleTable, NodeTable,
};

pub struct MemVfsState {
    pub nodes: NodeTable,
    pub root: NodeId,
    pub handles: HandleTable,
    pub cursors: CursorTable,
    pub total_bytes: u64,
    pub next_index_number: u64,
    pub cfg: VfsConfig,
}

/// The Phase 1 filesystem. All state behind one mutex (§3.6).
pub struct MemVfs {
    inner: Mutex<MemVfsState>,
    cfg: VfsConfig,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl MemVfs {
    pub fn new(cfg: VfsConfig) -> Self {
        let mut nodes = new_node_table();
        let now = now_filetime();
        // index_number starts at 1: 0 is a plausible "unset" value and Windows
        // should never see one.
        let root = nodes
            .alloc(MemNode::new_dir("", None, 1, now))
            .expect("fresh node table cannot be exhausted");

        MemVfs {
            inner: Mutex::new(MemVfsState {
                nodes,
                root,
                handles: new_handle_table(cfg.limits.max_open_handles),
                cursors: new_cursor_table(cfg.limits.max_open_cursors),
                total_bytes: 0,
                next_index_number: 2,
                cfg,
            }),
            cfg,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(VfsConfig::default())
    }

    pub fn config(&self) -> VfsConfig {
        self.cfg
    }

    /// Deadline-bounded lock acquisition (§3.6).
    ///
    /// **This is why `parking_lot` is used rather than `std::sync::Mutex`** --
    /// `std` has no deadline-bounded acquisition, so L9 could not cover lock
    /// wait, and ADR-0009's "the deadline is the only bound that exists" would
    /// be false.
    fn state(&self, cx: &OpCtx) -> Result<MutexGuard<'_, MemVfsState>, SpaceError> {
        self.inner.try_lock_until(cx.deadline).ok_or_else(|| {
            SpaceError::new(ErrorCode::OperationTimeout, "state lock deadline exceeded")
        })
    }

    /// Unbounded lock, for the diagnostic path only (§3.6).
    ///
    /// A diagnostic that gives up on a timeout would report "healthy" for a
    /// wedged filesystem -- the worst possible answer.
    pub(crate) fn state_blocking(&self) -> MutexGuard<'_, MemVfsState> {
        self.inner.lock()
    }
}

// ---------------------------------------------------------------------------
// State helpers
// ---------------------------------------------------------------------------

impl MemVfsState {
    fn fold(&self, name: &str) -> FoldedName {
        FoldedName::new(name, self.cfg.capabilities)
    }

    fn take_index_number(&mut self) -> u64 {
        let n = self.next_index_number;
        // Monotonic and never reused within a process lifetime (INV-ID-2).
        self.next_index_number += 1;
        n
    }

    fn node(&self, id: NodeId) -> Result<&MemNode, SpaceError> {
        self.nodes.resolve(id)
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut MemNode, SpaceError> {
        self.nodes.resolve_mut(id)
    }

    /// Resolve a handle to its slot, rejecting handles that are past `Cleanup`.
    ///
    /// I/O after `Cleanup` and before `Close` is `InvalidHandle`: Windows sends
    /// no further I/O on that file object, so any such call is a bug or an
    /// attack (INV-FS-3).
    fn handle(&self, h: HandleId) -> Result<&HandleSlot, SpaceError> {
        let slot = self.handles.resolve(h)?;
        if slot.cleaned_up {
            return Err(SpaceError::new(
                ErrorCode::InvalidHandle,
                "handle used after cleanup",
            ));
        }
        Ok(slot)
    }

    fn node_of(&self, h: HandleId) -> Result<&MemNode, SpaceError> {
        let node = self.handle(h)?.node;
        self.node(node)
    }

    fn node_id_of(&self, h: HandleId) -> Result<NodeId, SpaceError> {
        Ok(self.handle(h)?.node)
    }

    /// Walk a path to its node.
    ///
    /// Distinguishes "the file is missing" from "a directory on the way is
    /// missing": Windows tools rely on `ObjectPathNotFound` vs `FileNotFound`
    /// and report quite different things to the user (fs-semantics §3).
    fn lookup(&self, path: &VfsPath) -> Result<NodeId, SpaceError> {
        let mut cur = self.root;
        let n = path.components().len();
        for (i, comp) in path.components().iter().enumerate() {
            let node = self.node(cur)?;
            if !node.is_dir {
                // A non-directory in the middle of the path.
                return Err(SpaceError::new(
                    ErrorCode::ObjectPathNotFound,
                    "path component is not a directory",
                ));
            }
            match node.children.get(&self.fold(comp)) {
                Some(&child) => cur = child,
                None if i + 1 == n => {
                    return Err(SpaceError::new(ErrorCode::FileNotFound, "no such file"))
                }
                None => {
                    return Err(SpaceError::new(
                        ErrorCode::ObjectPathNotFound,
                        "parent directory does not exist",
                    ))
                }
            }
        }
        Ok(cur)
    }

    /// Resolve the parent directory of `path`, for create and rename.
    fn lookup_parent(&self, path: &VfsPath) -> Result<NodeId, SpaceError> {
        let parent = path.parent().ok_or_else(|| {
            SpaceError::new(ErrorCode::InvalidParameter, "the root has no parent")
        })?;
        let id = match self.lookup(&parent) {
            Ok(id) => id,
            // A missing *parent* is always ObjectPathNotFound, even when the
            // missing component is the last one of the parent path.
            Err(e) if e.code == ErrorCode::FileNotFound => {
                return Err(SpaceError::new(
                    ErrorCode::ObjectPathNotFound,
                    "parent directory does not exist",
                ))
            }
            Err(e) => return Err(e),
        };
        if !self.node(id)?.is_dir {
            return Err(SpaceError::new(
                ErrorCode::NotADirectory,
                "parent is not a directory",
            ));
        }
        Ok(id)
    }

    /// Is `ancestor` on the parent chain of `id` (or equal to it)?
    /// Used to reject renaming a directory into its own subtree (INV-NS-4).
    fn is_ancestor(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut cur = Some(id);
        let mut steps = 0usize;
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            steps += 1;
            if steps > self.cfg.path_limits.max_path_depth + 2 {
                // Defensive: a cycle would otherwise spin here. The checker
                // reports it as INV-NS-3; this just refuses to hang.
                return false;
            }
            cur = self.nodes.resolve(c).ok().and_then(|n| n.parent);
        }
        false
    }

    /// Link `child` into `parent` under `name`, enforcing L6.
    fn link(&mut self, parent: NodeId, name: &str, child: NodeId) -> Result<(), SpaceError> {
        let key = self.fold(name);
        let max = self.cfg.limits.max_dir_entries;
        let p = self.node_mut(parent)?;
        if p.children.contains_key(&key) {
            return Err(SpaceError::new(
                ErrorCode::FileExists,
                "name already exists in directory",
            ));
        }
        if p.children.len() >= max {
            // L6. Checked before any mutation so the failure leaves state
            // exactly as it was (INV-RES-3).
            return Err(SpaceError::new(
                ErrorCode::ResourceExhausted,
                "directory entry limit (L6) reached",
            ));
        }
        p.children.insert(key, child);
        Ok(())
    }

    fn unlink_from_parent(&mut self, id: NodeId) -> Result<(), SpaceError> {
        let (parent, name) = {
            let n = self.node(id)?;
            (n.parent, n.name.clone())
        };
        if let Some(p) = parent {
            let key = self.fold(&name);
            let pn = self.node_mut(p)?;
            pn.children.remove(&key);
        }
        Ok(())
    }

    /// Total bytes held by a node's subtree, for accounting on delete.
    fn subtree_bytes(&self, id: NodeId) -> u64 {
        let mut total = 0u64;
        let mut stack = vec![id];
        let mut steps = 0usize;
        while let Some(cur) = stack.pop() {
            steps += 1;
            if steps > 1_000_000 {
                break; // defensive; a cycle is reported by the checker
            }
            if let Ok(n) = self.nodes.resolve(cur) {
                total = total.saturating_add(n.file_size());
                stack.extend(n.children.values().copied());
            }
        }
        total
    }

    /// Reclaim an unlinked node with no remaining handles (fs-semantics §2).
    ///
    /// Deterministic: exactly at the `Close` that brings `open_count` to zero.
    /// Never earlier, never later, never on a timer.
    fn reclaim_if_due(&mut self, id: NodeId) {
        let due = match self.nodes.resolve(id) {
            Ok(n) => n.unlinked && n.open_count == 0,
            Err(_) => false,
        };
        if !due {
            return;
        }
        // Collect the subtree first; a directory deleted while open takes its
        // (necessarily empty, by CanDelete) children with it.
        let mut stack = vec![id];
        let mut doomed = Vec::new();
        while let Some(cur) = stack.pop() {
            if let Ok(n) = self.nodes.resolve(cur) {
                stack.extend(n.children.values().copied());
                doomed.push(cur);
            }
        }
        for d in doomed {
            if let Ok(n) = self.nodes.resolve(d) {
                let bytes = n.file_size();
                self.total_bytes = self.total_bytes.saturating_sub(bytes);
            }
            let _ = self.nodes.free(d);
        }
    }

    fn touch_accessed(&mut self, id: NodeId, now: u64) {
        if let Ok(n) = self.node_mut(id) {
            n.accessed = now;
        }
    }

    fn touch_written(&mut self, id: NodeId, now: u64) {
        if let Ok(n) = self.node_mut(id) {
            n.written = now;
            n.changed = now;
        }
    }
}

// ---------------------------------------------------------------------------
// The Vfs implementation
// ---------------------------------------------------------------------------

impl Vfs for MemVfs {
    fn volume_info(&self, cx: &OpCtx) -> Result<VolumeInfo, SpaceError> {
        cx.check()?;
        let s = self.state(cx)?;
        let total = s.cfg.limits.max_bytes;
        Ok(VolumeInfo {
            total_size: total,
            free_size: total.saturating_sub(s.total_bytes),
        })
    }

    fn probe(&self, cx: &OpCtx, path: &VfsPath) -> Result<Probe, SpaceError> {
        cx.check()?;
        let s = self.state(cx)?;
        let id = s.lookup(path)?;
        let n = s.node(id)?;
        Ok(Probe {
            file_attributes: n.attributes,
        })
    }

    fn create(&self, cx: &OpCtx, path: &VfsPath, o: CreateOptions) -> Result<Opened, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;

        if path.is_root() {
            return Err(SpaceError::new(
                ErrorCode::FileExists,
                "the root already exists",
            ));
        }

        let parent = s.lookup_parent(path)?;
        let name = path
            .file_name()
            .ok_or_else(|| SpaceError::new(ErrorCode::InvalidParameter, "path has no name"))?
            .to_string();

        // Existing name is FileExists, checked before allocating a node so a
        // failed create leaves no orphan (INV-RES-3).
        let key = s.fold(&name);
        if s.node(parent)?.children.contains_key(&key) {
            return Err(SpaceError::new(ErrorCode::FileExists, "file exists"));
        }
        if s.node(parent)?.children.len() >= s.cfg.limits.max_dir_entries {
            return Err(SpaceError::new(
                ErrorCode::ResourceExhausted,
                "directory entry limit (L6) reached",
            ));
        }

        let now = now_filetime();
        let index_number = s.take_index_number();
        let is_dir = o.wants_directory();
        let node = if is_dir {
            MemNode::new_dir(name.clone(), Some(parent), index_number, now)
        } else {
            MemNode::new_file(name.clone(), Some(parent), index_number, now, o.file_attributes)
        };

        let id = s.nodes.alloc(node)?;
        if let Err(e) = s.link(parent, &name, id) {
            // Roll back the node so a failed link leaves no orphan.
            let _ = s.nodes.free(id);
            return Err(e);
        }

        let handle = match s.handles.alloc(HandleSlot {
            node: id,
            granted_access: o.granted_access,
            is_dir,
            cleaned_up: false,
            delete_on_close: o.delete_on_close(),
        }) {
            Ok(h) => h,
            Err(e) => {
                // L7 hit: undo the whole create (INV-RES-3).
                let _ = s.unlink_from_parent(id);
                let _ = s.nodes.free(id);
                return Err(e);
            }
        };
        s.node_mut(id)?.open_count += 1;

        let info = s.node(id)?.info();
        Ok(Opened { handle, info })
    }

    fn open(&self, cx: &OpCtx, path: &VfsPath, o: OpenOptions) -> Result<Opened, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;

        let id = s.lookup(path)?;
        let is_dir = s.node(id)?.is_dir;

        // fs-semantics §3.
        if is_dir && o.wants_non_directory() {
            return Err(SpaceError::new(
                ErrorCode::FileIsADirectory,
                "FILE_NON_DIRECTORY_FILE on a directory",
            ));
        }
        if !is_dir && o.wants_directory() {
            return Err(SpaceError::new(
                ErrorCode::NotADirectory,
                "FILE_DIRECTORY_FILE on a file",
            ));
        }

        let handle = s.handles.alloc(HandleSlot {
            node: id,
            granted_access: o.granted_access,
            is_dir,
            cleaned_up: false,
            delete_on_close: o.delete_on_close(),
        })?;
        s.node_mut(id)?.open_count += 1;

        let info = s.node(id)?.info();
        Ok(Opened { handle, info })
    }

    fn overwrite(
        &self,
        cx: &OpCtx,
        h: HandleId,
        attrs: u32,
        replace_attrs: bool,
        alloc: u64,
    ) -> Result<FileInfo, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;

        if s.node(id)?.is_dir {
            return Err(SpaceError::new(
                ErrorCode::FileIsADirectory,
                "cannot overwrite a directory",
            ));
        }

        let old_len = s.node(id)?.file_size();
        let now = now_filetime();
        let n = s.node_mut(id)?;
        n.data.clear();
        n.explicit_allocation = if alloc > 0 { Some(allocation_size_for(alloc)) } else { None };
        n.attributes = if replace_attrs {
            (attrs & !FILE_ATTRIBUTE_DIRECTORY) | FILE_ATTRIBUTE_ARCHIVE
        } else {
            n.attributes | attrs | FILE_ATTRIBUTE_ARCHIVE
        };
        n.written = now;
        n.changed = now;
        s.total_bytes = s.total_bytes.saturating_sub(old_len);

        Ok(s.node(id)?.info())
    }

    fn cleanup(&self, cx: &OpCtx, h: HandleId, flags: CleanupFlags) {
        // `cleanup` is void and cannot report failure, so every step is
        // best-effort. A deadline miss here is not an error the caller can act
        // on; the operation simply does not happen.
        let Ok(mut s) = self.state(cx) else { return };

        let Ok(slot) = s.handles.resolve(h) else { return };
        if slot.cleaned_up {
            return;
        }
        let id = slot.node;
        let wants_delete = flags.delete() || slot.delete_on_close;

        if let Ok(slot) = s.handles.resolve_mut(h) {
            slot.cleaned_up = true;
        }

        if wants_delete {
            // Unlink at Cleanup: this is the only signal Windows gives that the
            // delete should take effect (fs-semantics §1). Doing it at Close
            // instead is how deleted files reappear.
            if s.nodes.resolve(id).map(|n| !n.unlinked).unwrap_or(false) {
                let _ = s.unlink_from_parent(id);
                if let Ok(n) = s.node_mut(id) {
                    n.unlinked = true;
                    n.parent = None;
                }
            }
        }
    }

    fn close(&self, cx: &OpCtx, h: HandleId) {
        let Ok(mut s) = self.state(cx) else { return };

        // Close is the only callback guaranteed to fire for every open, so this
        // is where open_count and the handle slot are released
        // (fs-semantics §1, bookkeeping table).
        let Ok(slot) = s.handles.free(h) else { return };
        if let Ok(n) = s.node_mut(slot.node) {
            n.open_count = n.open_count.saturating_sub(1);
        }
        s.reclaim_if_due(slot.node);
    }

    fn read(
        &self,
        cx: &OpCtx,
        h: HandleId,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<u32, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;

        if s.node(id)?.is_dir {
            return Err(SpaceError::new(
                ErrorCode::FileIsADirectory,
                "read on a directory",
            ));
        }

        // L4, before touching the buffer.
        if buf.len() > s.cfg.limits.max_io_bytes {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "length exceeds max_io_bytes (L4)",
            ));
        }

        // A zero-length read asks for nothing and gets nothing -- checked
        // before the EOF rule, which is about reads that cannot be satisfied.
        if buf.is_empty() {
            let now = now_filetime();
            s.touch_accessed(id, now);
            return Ok(0);
        }

        let len = s.node(id)?.file_size();
        if offset >= len {
            return Err(SpaceError::new(ErrorCode::EndOfFile, "read at or past EOF"));
        }

        // checked_add is not optional: offset = u64::MAX, length = 4096 arrives
        // from the fuzzer within the hour.
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or_else(|| SpaceError::new(ErrorCode::InvalidParameter, "offset+length overflow"))?
            .min(len);

        let n = (end - offset) as usize;
        let now = now_filetime();
        {
            let node = s.node(id)?;
            buf[..n].copy_from_slice(&node.data[offset as usize..end as usize]);
        }
        s.touch_accessed(id, now);
        Ok(n as u32)
    }

    fn write(
        &self,
        cx: &OpCtx,
        h: HandleId,
        offset: u64,
        buf: &[u8],
        mode: WriteMode,
    ) -> Result<(u32, FileInfo), SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;

        if s.node(id)?.is_dir {
            return Err(SpaceError::new(
                ErrorCode::FileIsADirectory,
                "write on a directory",
            ));
        }
        if buf.len() > s.cfg.limits.max_io_bytes {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "length exceeds max_io_bytes (L4)",
            ));
        }

        let cur_len = s.node(id)?.file_size();

        // WriteToEndOfFile ignores the offset entirely.
        let offset = if mode.write_to_eof { cur_len } else { offset };

        if buf.is_empty() {
            return Ok((0, s.node(id)?.info()));
        }

        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or_else(|| SpaceError::new(ErrorCode::InvalidParameter, "offset+length overflow"))?;

        // ConstrainedIo: the file must not grow. From the cache manager's
        // write-behind path; ignoring it produces files that grow during a copy.
        let (end, take) = if mode.constrained_io {
            if offset >= cur_len {
                return Ok((0, s.node(id)?.info()));
            }
            let clamped = end.min(cur_len);
            (clamped, (clamped - offset) as usize)
        } else {
            (end, buf.len())
        };

        // L5, checked before any mutation so a DiskFull leaves the file
        // byte-identical (INV-RES-3).
        if end > cur_len {
            let growth = end - cur_len;
            if s.total_bytes.saturating_add(growth) > s.cfg.limits.max_bytes {
                return Err(SpaceError::new(
                    ErrorCode::DiskFull,
                    "write would exceed max_bytes (L5)",
                ));
            }
        }

        let now = now_filetime();
        {
            let node = s.node_mut(id)?;
            if end > node.data.len() as u64 {
                // Extending past EOF zero-fills the gap.
                node.data.resize(end as usize, 0);
            }
            node.data[offset as usize..(offset as usize + take)].copy_from_slice(&buf[..take]);
            node.fix_allocation();
        }
        if end > cur_len {
            s.total_bytes = s.total_bytes.saturating_add(end - cur_len);
        }
        s.touch_written(id, now);

        Ok((take as u32, s.node(id)?.info()))
    }

    fn flush(&self, cx: &OpCtx, h: Option<HandleId>) -> Result<Option<FileInfo>, SpaceError> {
        cx.check()?;
        let s = self.state(cx)?;
        // Nothing to flush: Phase 1 has no durable state by design (§15.4).
        // The callback still validates its handle, so a bad one is reported.
        match h {
            None => Ok(None),
            Some(h) => Ok(Some(s.node_of(h)?.info())),
        }
    }

    fn file_info(&self, cx: &OpCtx, h: HandleId) -> Result<FileInfo, SpaceError> {
        cx.check()?;
        let s = self.state(cx)?;
        Ok(s.node_of(h)?.info())
    }

    fn set_basic_info(
        &self,
        cx: &OpCtx,
        h: HandleId,
        patch: BasicInfoPatch,
    ) -> Result<FileInfo, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;
        let now = now_filetime();

        let n = s.node_mut(id)?;
        if let Some(a) = patch.file_attributes {
            // A node never changes kind, so the DIRECTORY bit is preserved
            // rather than taken from the caller.
            let dir_bit = n.attributes & FILE_ATTRIBUTE_DIRECTORY;
            n.attributes = (a & !FILE_ATTRIBUTE_DIRECTORY) | dir_bit;
        }
        if let Some(t) = patch.creation_time {
            n.created = t;
        }
        if let Some(t) = patch.last_access_time {
            n.accessed = t;
        }
        if let Some(t) = patch.last_write_time {
            n.written = t;
        }
        // change_time is set explicitly if given, and otherwise updated because
        // metadata changed (fs-semantics §6).
        n.changed = patch.change_time.unwrap_or(now);

        Ok(s.node(id)?.info())
    }

    fn set_file_size(
        &self,
        cx: &OpCtx,
        h: HandleId,
        new_size: u64,
        set_allocation: bool,
    ) -> Result<FileInfo, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;

        if s.node(id)?.is_dir {
            return Err(SpaceError::new(
                ErrorCode::FileIsADirectory,
                "cannot set the size of a directory",
            ));
        }

        let cur = s.node(id)?.file_size();

        if set_allocation {
            // Adjusts allocation only. Shrinking allocation below the file size
            // would break INV-FS-1, so the file is truncated to fit.
            let rounded = allocation_size_for(new_size);
            if rounded < cur {
                let freed = cur - rounded;
                s.node_mut(id)?.data.truncate(rounded as usize);
                s.total_bytes = s.total_bytes.saturating_sub(freed);
            }
            let now = now_filetime();
            let n = s.node_mut(id)?;
            n.explicit_allocation = Some(rounded);
            n.written = now;
            n.changed = now;
            return Ok(s.node(id)?.info());
        }

        if new_size > cur {
            let growth = new_size - cur;
            if s.total_bytes.saturating_add(growth) > s.cfg.limits.max_bytes {
                return Err(SpaceError::new(
                    ErrorCode::DiskFull,
                    "set_file_size would exceed max_bytes (L5)",
                ));
            }
            if new_size > s.cfg.limits.max_bytes {
                return Err(SpaceError::new(
                    ErrorCode::DiskFull,
                    "size exceeds max_bytes (L5)",
                ));
            }
            s.node_mut(id)?.data.resize(new_size as usize, 0);
            s.total_bytes = s.total_bytes.saturating_add(growth);
        } else if new_size < cur {
            s.node_mut(id)?.data.truncate(new_size as usize);
            s.total_bytes = s.total_bytes.saturating_sub(cur - new_size);
        }

        let now = now_filetime();
        let n = s.node_mut(id)?;
        n.explicit_allocation = None;
        n.written = now;
        n.changed = now;
        Ok(s.node(id)?.info())
    }

    fn can_delete(&self, cx: &OpCtx, h: HandleId) -> Result<(), SpaceError> {
        cx.check()?;
        let s = self.state(cx)?;
        let n = s.node_of(h)?;
        // A pure query (fs-semantics §5). Removal happens at Cleanup.
        if n.is_dir && !n.children.is_empty() {
            return Err(SpaceError::new(
                ErrorCode::DirectoryNotEmpty,
                "directory is not empty",
            ));
        }
        Ok(())
    }

    fn rename(
        &self,
        cx: &OpCtx,
        h: HandleId,
        to: &VfsPath,
        replace: bool,
    ) -> Result<(), SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;

        if s.node(id)?.unlinked {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "cannot rename an unlinked node",
            ));
        }
        if id == s.root {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "cannot rename the root",
            ));
        }

        let new_parent = s.lookup_parent(to)?;
        let new_name = to
            .file_name()
            .ok_or_else(|| SpaceError::new(ErrorCode::InvalidParameter, "target has no name"))?
            .to_string();

        // Renaming a directory into its own subtree would detach the subtree
        // from the root (INV-NS-4).
        if s.node(id)?.is_dir && s.is_ancestor(id, new_parent) {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "cannot rename a directory into its own subtree",
            ));
        }

        let key = s.fold(&new_name);
        let existing = s.node(new_parent)?.children.get(&key).copied();

        if let Some(target) = existing {
            if target == id {
                // Case-only rename of the same node: update the display name.
                s.node_mut(id)?.name = new_name;
                let now = now_filetime();
                s.node_mut(id)?.changed = now;
                return Ok(());
            }
            if !replace {
                return Err(SpaceError::new(
                    ErrorCode::FileExists,
                    "target exists and replace was not requested",
                ));
            }
            let target_is_dir = s.node(target)?.is_dir;
            let src_is_dir = s.node(id)?.is_dir;
            if target_is_dir && !src_is_dir {
                return Err(SpaceError::new(
                    ErrorCode::FileIsADirectory,
                    "cannot replace a directory with a file",
                ));
            }
            if target_is_dir && !s.node(target)?.children.is_empty() {
                return Err(SpaceError::new(
                    ErrorCode::DirectoryNotEmpty,
                    "cannot replace a non-empty directory",
                ));
            }
            // Remove the target. It may still be open, in which case it becomes
            // an unlinked-but-open node like any other (fs-semantics §2).
            s.unlink_from_parent(target)?;
            let open = s.node(target)?.open_count;
            if open == 0 {
                let bytes = s.subtree_bytes(target);
                s.total_bytes = s.total_bytes.saturating_sub(bytes);
                let _ = s.nodes.free(target);
            } else {
                let n = s.node_mut(target)?;
                n.unlinked = true;
                n.parent = None;
            }
        } else {
            // L6 on the destination directory, checked before mutating.
            if s.node(new_parent)?.children.len() >= s.cfg.limits.max_dir_entries {
                return Err(SpaceError::new(
                    ErrorCode::ResourceExhausted,
                    "destination directory entry limit (L6) reached",
                ));
            }
        }

        s.unlink_from_parent(id)?;
        s.node_mut(id)?.name = new_name.clone();
        s.node_mut(id)?.parent = Some(new_parent);
        s.link(new_parent, &new_name, id)?;
        let now = now_filetime();
        s.node_mut(id)?.changed = now;
        Ok(())
    }

    fn dir_open(
        &self,
        cx: &OpCtx,
        h: HandleId,
        _pattern: Option<&str>,
        marker: Option<&str>,
    ) -> Result<CursorId, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let id = s.node_id_of(h)?;

        if !s.node(id)?.is_dir {
            return Err(SpaceError::new(
                ErrorCode::NotADirectory,
                "cannot enumerate a file",
            ));
        }

        // `pattern` is accepted and ignored in Phase 1: WinFsp performs the
        // filtering (fs-semantics §8). Recorded as a dependency, not a gap.

        let is_root = id == s.root;
        let mut entries: Vec<(String, FileInfo)> = Vec::new();

        // Order: ".", then "..", then children in ascending folded-name order.
        // Both dot entries are omitted for the root.
        if !is_root {
            let self_info = s.node(id)?.info();
            entries.push((".".to_string(), self_info));
            let parent_id = s.node(id)?.parent.unwrap_or(s.root);
            let parent_info = s.node(parent_id)?.info();
            entries.push(("..".to_string(), parent_info));
        }

        // BTreeMap iteration is already in ascending folded-name order, which is
        // the total, stable order §3.3.8 requires.
        let children: Vec<NodeId> = s.node(id)?.children.values().copied().collect();
        for child in children {
            let n = s.node(child)?;
            entries.push((n.name.clone(), n.info()));
        }

        // The marker is a resume point, not a filter: yield entries strictly
        // after it in the order above. Getting this wrong is what silently
        // truncates large directories (INV-DIR-2).
        let start = match marker {
            None => 0,
            Some(m) => {
                let mkey = s.fold(m);
                let mut idx = entries.len();
                for (i, (name, _)) in entries.iter().enumerate() {
                    if s.fold(name) == mkey {
                        idx = i + 1;
                        break;
                    }
                }
                idx
            }
        };

        s.cursors.alloc(CursorSlot {
            entries,
            next: start,
        })
    }

    fn dir_next(&self, cx: &OpCtx, cursor: CursorId) -> Result<Option<DirEntry>, SpaceError> {
        cx.check()?;
        let mut s = self.state(cx)?;
        let slot = s.cursors.resolve_mut(cursor)?;
        if slot.next >= slot.entries.len() {
            return Ok(None);
        }
        let (name, info) = slot.entries[slot.next].clone();
        slot.next += 1;
        Ok(Some(DirEntry { name, info }))
    }

    fn dir_close(&self, cx: &OpCtx, cursor: CursorId) {
        let Ok(mut s) = self.state(cx) else { return };
        let _ = s.cursors.free(cursor);
    }

    fn parse_path(&self, s: &str) -> Result<VfsPath, SpaceError> {
        VfsPath::parse_with(s, &self.cfg.path_limits)
    }
}
