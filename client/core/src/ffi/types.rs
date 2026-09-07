//! `#[repr(C)]` structs that cross the FFI (Phase 1 §2.2).
//!
//! Boundary rule 1: only `#[repr(C)]` structs of fixed-width scalars and
//! pointers cross. No Rust enums, slices, `String`, `Option`, or trait objects.
//!
//! Boundary rule 7: every shared struct's size and field offsets are asserted
//! on **both** sides. Layout drift otherwise surfaces as garbage file sizes and
//! takes days to trace.

use crate::vfs::types::FileInfo;

/// Mirrors `space_file_info`. **64 bytes; asserted on both sides.**
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpaceFileInfo {
    pub file_attributes: u32,
    /// Always 0 in Phase 1.
    pub reparse_tag: u32,
    pub allocation_size: u64,
    pub file_size: u64,
    /// Windows `FILETIME`, 100ns since 1601.
    pub creation_time: u64,
    pub last_access_time: u64,
    pub last_write_time: u64,
    pub change_time: u64,
    /// Windows-visible identity; see §3.1.
    pub index_number: u64,
}

impl From<FileInfo> for SpaceFileInfo {
    fn from(i: FileInfo) -> Self {
        SpaceFileInfo {
            file_attributes: i.file_attributes,
            reparse_tag: i.reparse_tag,
            allocation_size: i.allocation_size,
            file_size: i.file_size,
            creation_time: i.creation_time,
            last_access_time: i.last_access_time,
            last_write_time: i.last_write_time,
            change_time: i.change_time,
            index_number: i.index_number,
        }
    }
}

/// Mirrors `space_volume_info`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpaceVolumeInfo {
    pub total_size: u64,
    pub free_size: u64,
}

/// Mirrors `space_dir_entry`.
///
/// `name` is **borrowed**: valid until the next `space_core_dir_next` on the
/// same cursor (boundary rule 3, the one documented exception to rule 2). The
/// buffer it points at is owned by the FFI layer's per-cursor name table, not
/// by the VFS.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SpaceDirEntry {
    pub name: *const u16,
    pub info: SpaceFileInfo,
}

impl Default for SpaceDirEntry {
    fn default() -> Self {
        SpaceDirEntry {
            name: std::ptr::null(),
            info: SpaceFileInfo::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boundary rule 7. The C side carries the matching `static_assert`s in
    /// `space_core.h`; if these two ever disagree the build breaks on one side
    /// or the other, which is the entire point.
    #[test]
    fn file_info_layout_matches_c() {
        assert_eq!(std::mem::size_of::<SpaceFileInfo>(), 64);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, file_size), 16);
    }

    #[test]
    fn every_file_info_field_is_where_c_expects_it() {
        // Spelled out so a reordering is caught at the exact field rather than
        // only as a size change.
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, file_attributes), 0);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, reparse_tag), 4);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, allocation_size), 8);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, file_size), 16);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, creation_time), 24);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, last_access_time), 32);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, last_write_time), 40);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, change_time), 48);
        assert_eq!(std::mem::offset_of!(SpaceFileInfo, index_number), 56);
        assert_eq!(std::mem::align_of::<SpaceFileInfo>(), 8);
    }

    #[test]
    fn volume_info_layout_matches_c() {
        assert_eq!(std::mem::size_of::<SpaceVolumeInfo>(), 16);
        assert_eq!(std::mem::offset_of!(SpaceVolumeInfo, total_size), 0);
        assert_eq!(std::mem::offset_of!(SpaceVolumeInfo, free_size), 8);
    }

    #[test]
    fn dir_entry_layout_matches_c() {
        // pointer + 64-byte struct, 8-byte aligned.
        assert_eq!(std::mem::size_of::<SpaceDirEntry>(), 72);
        assert_eq!(std::mem::offset_of!(SpaceDirEntry, name), 0);
        assert_eq!(std::mem::offset_of!(SpaceDirEntry, info), 8);
    }

    #[test]
    fn file_info_converts_field_for_field() {
        let i = FileInfo {
            file_attributes: 0x10,
            reparse_tag: 0,
            allocation_size: 8192,
            file_size: 4097,
            creation_time: 1,
            last_access_time: 2,
            last_write_time: 3,
            change_time: 4,
            index_number: 42,
        };
        let s: SpaceFileInfo = i.into();
        assert_eq!(s.file_attributes, 0x10);
        assert_eq!(s.allocation_size, 8192);
        assert_eq!(s.file_size, 4097);
        assert_eq!(s.creation_time, 1);
        assert_eq!(s.last_access_time, 2);
        assert_eq!(s.last_write_time, 3);
        assert_eq!(s.change_time, 4);
        assert_eq!(s.index_number, 42);
        assert_eq!(s.reparse_tag, 0, "reparse_tag is always 0 in Phase 1");
    }
}
