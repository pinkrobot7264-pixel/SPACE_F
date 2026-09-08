//! `MemVfs` -- the Phase 1 `Vfs` implementation (§5.1).
//!
//! **Phase 2 replaces this module and nothing above it.** Everything the
//! adapter, the FFI and the conformance suite depend on is the `Vfs` trait, not
//! this type.
//!
//! All state lives behind a single `parking_lot::Mutex` (§3.6). That is the
//! whole concurrency model: exactly one VFS operation executes at a time. It is
//! documented in `docs/protocols/concurrency.md` rather than discovered.

pub mod table;

use crate::vfs::ids::{CursorId, HandleId, NodeId};
use crate::vfs::types::FileInfo;
use contracts::ErrorCode;
use table::GenerationalTable;

/// One open file object (§3.3.1). Exactly one per successful `create`/`open`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleSlot {
    pub node: NodeId,
    /// Recorded for diagnostics only. Share access is WinFsp-owned (ADR-0012).
    pub granted_access: u32,
    pub is_dir: bool,
    /// Set by `cleanup`. I/O after `Cleanup` and before `Close` is
    /// `InvalidHandle`: Windows sends no further I/O on that file object, so
    /// any such call is a bug or an attack (INV-FS-3).
    pub cleaned_up: bool,
    /// `FILE_DELETE_ON_CLOSE` was requested at open time. WinFsp normally
    /// raises `FspCleanupDelete` itself; this is recorded so the VFS behaves
    /// identically when driven directly by the conformance suite, which has no
    /// WinFsp to raise it.
    pub delete_on_close: bool,
}

/// One directory enumeration, valid for a single `ReadDirectory` call (§3.3.8).
///
/// The entries are snapshotted at `dir_open`. Under §3.6 no mutation can occur
/// while a cursor is open, so the snapshot cannot go stale within its lifetime;
/// it is what makes the "each entry exactly once" guarantee (INV-DIR-2) hold for
/// any buffer size.
#[derive(Clone, Debug)]
pub struct CursorSlot {
    /// The directory being enumerated, so a window can be refilled without
    /// re-resolving the handle.
    pub dir: NodeId,
    /// The current window of entries.
    ///
    /// **Bounded, not the whole directory.** Snapshotting every child on every
    /// `dir_open` made enumeration O(N^2): WinFsp issues roughly N/33
    /// `ReadDirectory` calls for an N-entry directory, and each one cloned all
    /// N names and `FileInfo`s. At L6 (65,536 entries) that is ~130 million
    /// clones for a single listing, and because §3.6 serialises everything
    /// behind one state lock, a listing in progress starves every other
    /// operation. Observed: a 5,000-entry root took a directory listing past
    /// three minutes and dropped concurrent file creation to under two per
    /// second.
    pub entries: Vec<(String, FileInfo)>,
    pub next: usize,
    /// Exclusive lower bound for the next window: the folded name of the last
    /// child yielded. `None` means "start at the first child".
    pub resume: Option<crate::vfs::path::FoldedName>,
    /// No children remain beyond the current window.
    pub drained: bool,
}

pub type NodeTable = GenerationalTable<NodeId, MemNode>;
pub type HandleTable = GenerationalTable<HandleId, HandleSlot>;
pub type CursorTable = GenerationalTable<CursorId, CursorSlot>;

pub fn new_handle_table(max: usize) -> HandleTable {
    GenerationalTable::new(
        max,
        ErrorCode::InvalidHandle,
        "open handle limit (L7) reached",
    )
}

pub fn new_cursor_table(max: usize) -> CursorTable {
    // A bad cursor is a bad argument to dir_next, not a filesystem handle, so
    // it reports InvalidParameter (INV-DIR-3).
    GenerationalTable::new(
        max,
        ErrorCode::InvalidParameter,
        "open cursor limit (L8) reached",
    )
}

pub fn new_node_table() -> NodeTable {
    // Nodes are bounded by L5 (total bytes) and L6 (entries per directory)
    // rather than by a count of their own, so the table's own ceiling is only a
    // backstop against the u32 index space.
    GenerationalTable::new(
        u32::MAX as usize,
        ErrorCode::FileNotFound,
        "node table exhausted",
    )
}

mod check;
mod imp;
#[cfg(test)]
mod tests;
pub use imp::{MemVfs, MemVfsState};
pub use node::MemNode;

mod node;
