//! Resource limits (Phase 1 §3.4).
//!
//! `docs/protocols/resource-limits.md` is the prose specification; this module
//! is its executable form. **Tests read values from [`Limits`] / [`PathLimits`]
//! rather than restating numbers**, so the table has exactly one source of
//! truth.

/// L1 -- maximum path length in UTF-16 code units.
pub const MAX_PATH_CHARS: usize = 32_767;
/// L2 -- maximum length of a single path component.
pub const MAX_COMPONENT_CHARS: usize = 255;
/// L3 -- maximum number of components in a path.
pub const MAX_PATH_DEPTH: usize = 512;
/// L4 -- maximum bytes in a single read or write.
pub const MAX_IO_BYTES: usize = 16 * 1024 * 1024;
/// L6 -- maximum entries in one directory.
pub const MAX_DIR_ENTRIES: usize = 65_536;
/// L7 -- maximum simultaneously open handles.
pub const MAX_OPEN_HANDLES: usize = 65_536;
/// L8 -- maximum simultaneously open cursors.
///
/// Small on purpose: a cursor lives for one `ReadDirectory` call (§3.3.8) and
/// concurrency is bounded by dispatcher threads (ADR-0010). The limit exists to
/// **catch a leak**, not to serve a workload.
pub const MAX_OPEN_CURSORS: usize = 256;
/// L5 default -- total bytes the filesystem may hold.
pub const DEFAULT_MAX_BYTES: u64 = 1 << 30;

/// The allocation unit. `allocation_size` is `file_size` rounded up to this
/// (INV-FS-1). Matches `SectorSize * SectorsPerAllocationUnit` in §4.1.
pub const ALLOCATION_UNIT: u64 = 4096;

/// Limits the conformance suite needs in order to test boundaries.
///
/// **These are not capabilities** (§11.5). A capability selects between
/// documented behaviours; a limit is a configured number. Keeping them in
/// separate types is what stops "make the limit huge" becoming a way to skip a
/// test.
///
/// The field set is fixed by the manual (§11.5) so that Phase 2's VFS is driven
/// by the identical struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// L5
    pub max_bytes: u64,
    /// L7
    pub max_open_handles: usize,
    /// L8
    pub max_open_cursors: usize,
    /// L6
    pub max_dir_entries: usize,
    /// L4
    pub max_io_bytes: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        max_bytes: DEFAULT_MAX_BYTES,
        max_open_handles: MAX_OPEN_HANDLES,
        max_open_cursors: MAX_OPEN_CURSORS,
        max_dir_entries: MAX_DIR_ENTRIES,
        max_io_bytes: MAX_IO_BYTES,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// L1 / L2 / L3 -- the naming bounds enforced by `VfsPath::parse`.
///
/// Separate from [`Limits`] because the manual fixes `Limits`' field set as part
/// of the conformance-suite signature, and because these three are structural
/// naming rules rather than resource budgets: they are cheap to hit directly in
/// a test, so no test needs them lowered to run quickly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathLimits {
    /// L1
    pub max_path_chars: usize,
    /// L2
    pub max_component_chars: usize,
    /// L3
    pub max_path_depth: usize,
}

impl PathLimits {
    pub const DEFAULT: PathLimits = PathLimits {
        max_path_chars: MAX_PATH_CHARS,
        max_component_chars: MAX_COMPONENT_CHARS,
        max_path_depth: MAX_PATH_DEPTH,
    };
}

impl Default for PathLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Round `size` up to the allocation unit (INV-FS-1).
///
/// Saturating rather than wrapping: a `file_size` near `u64::MAX` cannot be
/// reached through any Phase 1 path (L5 bounds it long before), but rounding it
/// must not be the thing that panics if one ever is.
pub fn allocation_size_for(size: u64) -> u64 {
    match size.checked_add(ALLOCATION_UNIT - 1) {
        Some(v) => (v / ALLOCATION_UNIT) * ALLOCATION_UNIT,
        None => u64::MAX - (u64::MAX % ALLOCATION_UNIT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_table() {
        // docs/protocols/resource-limits.md. If this fails, the document and
        // the code disagree and one of them is wrong.
        assert_eq!(Limits::DEFAULT.max_io_bytes, 16 * 1024 * 1024);
        assert_eq!(Limits::DEFAULT.max_dir_entries, 65_536);
        assert_eq!(Limits::DEFAULT.max_open_handles, 65_536);
        assert_eq!(Limits::DEFAULT.max_open_cursors, 256);
        assert_eq!(PathLimits::DEFAULT.max_path_chars, 32_767);
        assert_eq!(PathLimits::DEFAULT.max_component_chars, 255);
        assert_eq!(PathLimits::DEFAULT.max_path_depth, 512);
    }

    #[test]
    fn allocation_size_rounds_up_to_4096_and_never_below_file_size() {
        // Section 6.4: sizes 0, 1, 4095, 4096, 4097.
        for (size, expected) in [
            (0u64, 0u64),
            (1, 4096),
            (4095, 4096),
            (4096, 4096),
            (4097, 8192),
        ] {
            let a = allocation_size_for(size);
            assert_eq!(a, expected, "size {size}");
            assert!(a >= size, "allocation_size below file_size for {size}");
            assert_eq!(a % ALLOCATION_UNIT, 0, "not a multiple for {size}");
        }
    }

    #[test]
    fn allocation_size_does_not_panic_near_u64_max() {
        // Unreachable through any Phase 1 path (L5 bounds it), but rounding
        // must not be the thing that panics if it ever is.
        let a = allocation_size_for(u64::MAX);
        assert_eq!(a % ALLOCATION_UNIT, 0);
    }
}
