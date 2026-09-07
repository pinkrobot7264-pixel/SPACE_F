//! The node model (§5.1).

use std::collections::BTreeMap;

use crate::vfs::ids::NodeId;
use crate::vfs::limits::allocation_size_for;
use crate::vfs::path::FoldedName;
use crate::vfs::types::{FileInfo, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_DIRECTORY};

/// A file or directory.
#[derive(Clone, Debug)]
pub struct MemNode {
    /// Display name, **case preserved** (fs-semantics §7). The lookup key is
    /// the folded name held by the parent.
    pub name: String,
    pub is_dir: bool,
    pub attributes: u32,
    pub data: Vec<u8>,
    /// `BTreeMap` gives the total, stable enumeration order §3.3.8 requires,
    /// for free. The key is the folded name; the display name lives on the
    /// child node (INV-NS-5).
    pub children: BTreeMap<FoldedName, NodeId>,
    pub parent: Option<NodeId>,
    pub created: u64,
    pub accessed: u64,
    pub written: u64,
    pub changed: u64,
    /// Monotonic, never reused within a process lifetime (§3.1, INV-ID-2).
    /// **Not derived from the slab index**, which is reused after free.
    pub index_number: u64,
    pub open_count: u32,
    /// Names the state, not an intent (§5.1). Phase 2 reads this field.
    pub unlinked: bool,
    /// Allocation size when it has been set explicitly above the rounded file
    /// size (`set_file_size` with `set_allocation = true`). `None` means "track
    /// the file size", which is the normal case.
    pub explicit_allocation: Option<u64>,
}

impl MemNode {
    pub fn new_dir(name: impl Into<String>, parent: Option<NodeId>, index_number: u64, now: u64) -> Self {
        Self {
            name: name.into(),
            is_dir: true,
            attributes: FILE_ATTRIBUTE_DIRECTORY,
            data: Vec::new(),
            children: BTreeMap::new(),
            parent,
            created: now,
            accessed: now,
            written: now,
            changed: now,
            index_number,
            open_count: 0,
            unlinked: false,
            explicit_allocation: None,
        }
    }

    pub fn new_file(
        name: impl Into<String>,
        parent: Option<NodeId>,
        index_number: u64,
        now: u64,
        attributes: u32,
    ) -> Self {
        // A file always carries at least ARCHIVE, and never DIRECTORY.
        let attributes = (attributes & !FILE_ATTRIBUTE_DIRECTORY) | FILE_ATTRIBUTE_ARCHIVE;
        Self {
            name: name.into(),
            is_dir: false,
            attributes,
            data: Vec::new(),
            children: BTreeMap::new(),
            parent,
            created: now,
            accessed: now,
            written: now,
            changed: now,
            index_number,
            open_count: 0,
            unlinked: false,
            explicit_allocation: None,
        }
    }

    pub fn file_size(&self) -> u64 {
        self.data.len() as u64
    }

    /// `allocation_size` is a **stored property**, not a computation.
    ///
    /// When unset it tracks `file_size` rounded up to the allocation unit. When
    /// set explicitly (by `set_file_size` with `set_allocation = true`, or by
    /// `overwrite`) it is authoritative, and every mutation path is responsible
    /// for keeping it at or above `file_size` -- see [`Self::fix_allocation`].
    ///
    /// It is stored rather than derived on purpose: if it were `max(rounded,
    /// explicit)` then INV-FS-1 (`file_size <= allocation_size`) would be true
    /// by construction, the checker branch for it could never fire, and the
    /// invariant would be decoration. Making it storable makes the invariant
    /// falsifiable, which is the only way it can catch a Phase 2 bug.
    ///
    /// Reporting allocation below `file_size` causes oddities in Explorer's
    /// size columns and in some applications' EOF handling, which is why the
    /// invariant exists at all.
    pub fn allocation_size(&self) -> u64 {
        match self.explicit_allocation {
            Some(a) => a,
            None => allocation_size_for(self.file_size()),
        }
    }

    /// Re-establish INV-FS-1 after the data length changed.
    ///
    /// Called by every path that grows or shrinks `data`. An explicit
    /// allocation that no longer covers the file reverts to tracking the file
    /// size; it is never left below it.
    pub fn fix_allocation(&mut self) {
        if let Some(a) = self.explicit_allocation {
            let needed = allocation_size_for(self.file_size());
            if a < needed {
                self.explicit_allocation = None;
            }
        }
    }

    pub fn info(&self) -> FileInfo {
        FileInfo {
            file_attributes: self.attributes,
            reparse_tag: 0,
            allocation_size: self.allocation_size(),
            file_size: self.file_size(),
            creation_time: self.created,
            last_access_time: self.accessed,
            last_write_time: self.written,
            change_time: self.changed,
            index_number: self.index_number,
        }
    }
}
