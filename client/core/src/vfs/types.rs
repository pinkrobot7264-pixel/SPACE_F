//! Value types of the `Vfs` contract (Phase 1 §3.2).
//!
//! Nothing here names a Windows type. The `u32` flag fields carry values Windows
//! defines, but they are opaque bit sets to the VFS: it tests documented bits
//! and never interprets an NTSTATUS or a Win32 error (ADR-0015). That is what
//! lets the conformance suite compile with no Windows crate (§11.7).

use crate::vfs::limits;

// ---------------------------------------------------------------------------
// File attributes -- Windows-defined bit values, used as opaque flags.
// ---------------------------------------------------------------------------

pub const FILE_ATTRIBUTE_READONLY: u32 = 0x0000_0001;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
pub const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x0000_0020;
pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;

/// Windows' "no change" sentinel for an attributes field.
pub const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Create options -- the three Phase 1 honours (§7.1).
// ---------------------------------------------------------------------------

pub const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
pub const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
pub const FILE_DELETE_ON_CLOSE: u32 = 0x0000_1000;

// ---------------------------------------------------------------------------
// Cleanup flags -- WinFsp's FspCleanup* bits.
// ---------------------------------------------------------------------------

pub const FSP_CLEANUP_DELETE: u32 = 0x01;
pub const FSP_CLEANUP_SET_ALLOCATION_SIZE: u32 = 0x02;
pub const FSP_CLEANUP_SET_ARCHIVE_BIT: u32 = 0x10;
pub const FSP_CLEANUP_SET_LAST_ACCESS_TIME: u32 = 0x20;
pub const FSP_CLEANUP_SET_LAST_WRITE_TIME: u32 = 0x40;
pub const FSP_CLEANUP_SET_CHANGE_TIME: u32 = 0x80;

/// Flags passed to [`crate::vfs::Vfs::cleanup`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CleanupFlags(pub u32);

impl CleanupFlags {
    pub const NONE: CleanupFlags = CleanupFlags(0);
    pub const DELETE: CleanupFlags = CleanupFlags(FSP_CLEANUP_DELETE);

    /// The only signal Windows gives that a delete should take effect
    /// (fs-semantics §1, bookkeeping table).
    pub fn delete(self) -> bool {
        self.0 & FSP_CLEANUP_DELETE != 0
    }
}

// ---------------------------------------------------------------------------
// File and volume information.
// ---------------------------------------------------------------------------

/// Metadata for one node. Mirrors `space_file_info` field for field; the
/// `#[repr(C)]` twin lives at the FFI boundary so ABI concerns stay there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FileInfo {
    pub file_attributes: u32,
    /// Always 0 in Phase 1: reparse points are out of scope (§1.6).
    pub reparse_tag: u32,
    pub allocation_size: u64,
    pub file_size: u64,
    /// Windows `FILETIME`: 100ns intervals since 1601-01-01 UTC.
    pub creation_time: u64,
    pub last_access_time: u64,
    pub last_write_time: u64,
    pub change_time: u64,
    /// Windows-visible file identity. Monotonic, never reused (§3.1, INV-ID-2).
    pub index_number: u64,
}

impl FileInfo {
    pub fn is_dir(&self) -> bool {
        self.file_attributes & FILE_ATTRIBUTE_DIRECTORY != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VolumeInfo {
    pub total_size: u64,
    pub free_size: u64,
}

// ---------------------------------------------------------------------------
// Operation inputs and outputs.
// ---------------------------------------------------------------------------

/// Result of an existence + attributes probe. WinFsp calls this before
/// create/open (§6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub file_attributes: u32,
}

impl Probe {
    pub fn is_dir(&self) -> bool {
        self.file_attributes & FILE_ATTRIBUTE_DIRECTORY != 0
    }
}

/// Options for [`crate::vfs::Vfs::create`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateOptions {
    /// Raw NT `CreateOptions` bit set. Phase 1 honours `FILE_DIRECTORY_FILE`,
    /// `FILE_NON_DIRECTORY_FILE` and `FILE_DELETE_ON_CLOSE` (§7.1).
    pub create_options: u32,
    /// Recorded per handle for diagnostics only; **not enforced** (ADR-0012).
    pub granted_access: u32,
    pub file_attributes: u32,
    pub allocation_size: u64,
}

impl CreateOptions {
    pub fn wants_directory(&self) -> bool {
        self.create_options & FILE_DIRECTORY_FILE != 0
    }
    pub fn wants_non_directory(&self) -> bool {
        self.create_options & FILE_NON_DIRECTORY_FILE != 0
    }
    pub fn delete_on_close(&self) -> bool {
        self.create_options & FILE_DELETE_ON_CLOSE != 0
    }
}

/// Options for [`crate::vfs::Vfs::open`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenOptions {
    pub create_options: u32,
    /// Recorded per handle for diagnostics only; **not enforced** (ADR-0012).
    pub granted_access: u32,
}

impl OpenOptions {
    pub fn wants_directory(&self) -> bool {
        self.create_options & FILE_DIRECTORY_FILE != 0
    }
    pub fn wants_non_directory(&self) -> bool {
        self.create_options & FILE_NON_DIRECTORY_FILE != 0
    }
    pub fn delete_on_close(&self) -> bool {
        self.create_options & FILE_DELETE_ON_CLOSE != 0
    }
}

/// A successful `create` or `open`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opened {
    pub handle: crate::vfs::ids::HandleId,
    pub info: FileInfo,
}

/// Write mode flags (fs-semantics §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WriteMode {
    /// Ignore `offset`, append at the current EOF.
    pub write_to_eof: bool,
    /// From the cache manager's write-behind path. The write is truncated to
    /// the existing file size; the file never grows. **Ignoring this produces
    /// files that grow during a copy.**
    pub constrained_io: bool,
}

impl WriteMode {
    pub const NORMAL: WriteMode = WriteMode {
        write_to_eof: false,
        constrained_io: false,
    };
    pub const APPEND: WriteMode = WriteMode {
        write_to_eof: true,
        constrained_io: false,
    };
    pub const CONSTRAINED: WriteMode = WriteMode {
        write_to_eof: false,
        constrained_io: true,
    };
}

/// A patch for [`crate::vfs::Vfs::set_basic_info`].
///
/// `None` means "do not change". The FFI maps Windows' sentinels onto this:
/// `0` for a time field, `INVALID_FILE_ATTRIBUTES` for attributes
/// (fs-semantics §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BasicInfoPatch {
    pub file_attributes: Option<u32>,
    pub creation_time: Option<u64>,
    pub last_access_time: Option<u64>,
    pub last_write_time: Option<u64>,
    pub change_time: Option<u64>,
}

impl BasicInfoPatch {
    /// Build from the raw FFI values, applying the "0 means no change" rule.
    pub fn from_raw(attrs: u32, ctime: u64, atime: u64, wtime: u64, chtime: u64) -> Self {
        let time = |v: u64| if v == 0 { None } else { Some(v) };
        Self {
            file_attributes: if attrs == INVALID_FILE_ATTRIBUTES || attrs == 0 {
                None
            } else {
                Some(attrs)
            },
            creation_time: time(ctime),
            last_access_time: time(atime),
            last_write_time: time(wtime),
            change_time: time(chtime),
        }
    }
}

/// One entry of a directory enumeration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// Display name, case preserved (fs-semantics §7). For the synthetic
    /// entries this is `.` or `..`.
    pub name: String,
    pub info: FileInfo,
}

// ---------------------------------------------------------------------------
// Capabilities (§11.5).
// ---------------------------------------------------------------------------

/// A capability **selects which documented correct behaviour is expected. It
/// never decides whether a rule is checked.**
///
/// Every capability-dependent test runs in both configurations and asserts that
/// configuration's specified behaviour. Both variants are written into
/// `fs-semantics.md` §7 -- if you cannot write both, you do not have a
/// capability, you have a gap.
///
/// Capabilities are part of the versioned contract (ADR-0011). Adding one
/// requires a version bump and an ADR; `capability_count_is_deliberate` makes
/// that hard to do by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// `false` = ASCII folding (Phase 1); `true` = Unicode simple case folding
    /// (Phase 2 target). Both behaviours specified in `fs-semantics.md` §7;
    /// both tested.
    pub unicode_case_folding: bool,
}

impl Capabilities {
    pub const PHASE_1: Capabilities = Capabilities {
        unicode_case_folding: false,
    };
    pub const PHASE_2_TARGET: Capabilities = Capabilities {
        unicode_case_folding: true,
    };
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::PHASE_1
    }
}

/// Configuration handed to a `Vfs` implementation at construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VfsConfig {
    pub limits: limits::Limits,
    pub path_limits: limits::PathLimits,
    pub capabilities: Capabilities,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adding a capability weakens contract uniformity. If this fails, bump
    /// `VFS_CONTRACT_VERSION` and write an ADR -- do not just edit the number.
    #[test]
    fn capability_count_is_deliberate() {
        assert_eq!(std::mem::size_of::<Capabilities>(), 1);
    }

    #[test]
    fn basic_info_patch_treats_zero_as_no_change() {
        // fs-semantics §6: set_basic_info treats 0 as "do not change" per field.
        let p = BasicInfoPatch::from_raw(0, 0, 0, 0, 0);
        assert_eq!(p, BasicInfoPatch::default());
        assert!(p.file_attributes.is_none());
        assert!(p.creation_time.is_none());

        let p = BasicInfoPatch::from_raw(FILE_ATTRIBUTE_READONLY, 100, 0, 300, 0);
        assert_eq!(p.file_attributes, Some(FILE_ATTRIBUTE_READONLY));
        assert_eq!(p.creation_time, Some(100));
        assert_eq!(p.last_access_time, None);
        assert_eq!(p.last_write_time, Some(300));
        assert_eq!(p.change_time, None);
    }

    #[test]
    fn invalid_file_attributes_also_means_no_change() {
        let p = BasicInfoPatch::from_raw(INVALID_FILE_ATTRIBUTES, 0, 0, 0, 0);
        assert!(p.file_attributes.is_none());
    }

    #[test]
    fn cleanup_flags_read_the_delete_bit() {
        assert!(!CleanupFlags::NONE.delete());
        assert!(CleanupFlags::DELETE.delete());
        assert!(CleanupFlags(FSP_CLEANUP_DELETE | FSP_CLEANUP_SET_ARCHIVE_BIT).delete());
        assert!(!CleanupFlags(FSP_CLEANUP_SET_ARCHIVE_BIT).delete());
    }

    #[test]
    fn create_options_read_the_three_phase_1_bits() {
        let o = CreateOptions {
            create_options: FILE_DIRECTORY_FILE | FILE_DELETE_ON_CLOSE,
            granted_access: 0,
            file_attributes: 0,
            allocation_size: 0,
        };
        assert!(o.wants_directory());
        assert!(!o.wants_non_directory());
        assert!(o.delete_on_close());
    }
}
