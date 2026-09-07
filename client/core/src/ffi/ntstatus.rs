//! NTSTATUS translation -- the Windows boundary (ADR-0015).
//!
//! This module exists **here and not in `contracts`** because `contracts` is
//! shared with the cloud service, which has no business knowing about Windows
//! types. The error *taxonomy* is a contract concern; the *translation* is a
//! Windows concern.
//!
//! The VFS never sees an NTSTATUS or a Win32 error code. It returns
//! `SpaceError` and nothing else. This is the only place `ErrorCode` becomes a
//! number Windows understands.
//!
//! **The mapping is total, not injective** (ADR-0015). Several codes
//! legitimately share one NTSTATUS: `OperationTimeout` and `NetworkTimeout`
//! both map to `STATUS_IO_TIMEOUT`, and the four integrity codes all map to
//! `STATUS_FILE_CORRUPT_ERROR`. The exhaustiveness test asserts every code
//! *has* a mapping -- never that mappings are unique.

use contracts::ErrorCode;

/// A Windows `NTSTATUS` value. See `ntstatus.h`.
pub type NtStatus = u32;

pub const STATUS_SUCCESS: NtStatus = 0x0000_0000;

// Warning-severity status. Not a failure -- see `BufferOverflow` below.
pub const STATUS_BUFFER_OVERFLOW: NtStatus = 0x8000_0005;

pub const STATUS_END_OF_FILE: NtStatus = 0xC000_0011;
pub const STATUS_ACCESS_DENIED: NtStatus = 0xC000_0022;
pub const STATUS_OBJECT_NAME_INVALID: NtStatus = 0xC000_0033;
pub const STATUS_OBJECT_NAME_NOT_FOUND: NtStatus = 0xC000_0034;
pub const STATUS_OBJECT_NAME_COLLISION: NtStatus = 0xC000_0035;
pub const STATUS_OBJECT_PATH_NOT_FOUND: NtStatus = 0xC000_003A;
pub const STATUS_SHARING_VIOLATION: NtStatus = 0xC000_0043;
pub const STATUS_DISK_FULL: NtStatus = 0xC000_007F;
pub const STATUS_INSUFFICIENT_RESOURCES: NtStatus = 0xC000_009A;
pub const STATUS_DEVICE_NOT_READY: NtStatus = 0xC000_00A3;
pub const STATUS_IO_TIMEOUT: NtStatus = 0xC000_00B5;
pub const STATUS_FILE_IS_A_DIRECTORY: NtStatus = 0xC000_00BA;
pub const STATUS_UNEXPECTED_NETWORK_ERROR: NtStatus = 0xC000_00C4;
pub const STATUS_INTERNAL_ERROR: NtStatus = 0xC000_00E5;
pub const STATUS_DIRECTORY_NOT_EMPTY: NtStatus = 0xC000_0101;
pub const STATUS_FILE_CORRUPT_ERROR: NtStatus = 0xC000_0102;
pub const STATUS_NOT_A_DIRECTORY: NtStatus = 0xC000_0103;
pub const STATUS_NAME_TOO_LONG: NtStatus = 0xC000_0106;
pub const STATUS_CANCELLED: NtStatus = 0xC000_0120;
pub const STATUS_CANNOT_DELETE: NtStatus = 0xC000_0121;
pub const STATUS_INVALID_PARAMETER: NtStatus = 0xC000_000D;
pub const STATUS_INVALID_HANDLE: NtStatus = 0xC000_0008;
pub const STATUS_IO_DEVICE_ERROR: NtStatus = 0xC000_0185;

/// The `NTSTATUS` a code maps to, or `None` for startup-only errors.
///
/// This is M0.4's `ErrorCode::ntstatus()`, moved here by ADR-0015. Startup-only
/// codes happen before the mount exists, so they have no *meaningful* NTSTATUS;
/// [`from_error_code`] gives them a total mapping for the one entry point that
/// can still produce them.
pub fn ntstatus(code: ErrorCode) -> Option<NtStatus> {
    use ErrorCode::*;
    Some(match code {
        ConfigMissing | ConfigInvalid | ConfigUnsupportedVersion => return None,

        // --- Phase 1 filesystem codes (manual section 1.3) ---
        ObjectNameInvalid => STATUS_OBJECT_NAME_INVALID,
        ObjectPathNotFound => STATUS_OBJECT_PATH_NOT_FOUND,
        NotADirectory => STATUS_NOT_A_DIRECTORY,
        FileIsADirectory => STATUS_FILE_IS_A_DIRECTORY,
        EndOfFile => STATUS_END_OF_FILE,
        NameTooLong => STATUS_NAME_TOO_LONG,
        CannotDelete => STATUS_CANNOT_DELETE,
        BufferOverflow => STATUS_BUFFER_OVERFLOW,
        // Shares STATUS_IO_TIMEOUT with NetworkTimeout. Deliberate: the mapping
        // is total, not injective. Windows does not care why we were slow.
        OperationTimeout => STATUS_IO_TIMEOUT,

        // --- Phase 0 codes ---
        FileNotFound | VersionNotFound | ManifestNotFound | ChunkNotFound => {
            STATUS_OBJECT_NAME_NOT_FOUND
        }
        FileExists => STATUS_OBJECT_NAME_COLLISION,
        DirectoryNotEmpty => STATUS_DIRECTORY_NOT_EMPTY,
        InvalidParameter => STATUS_INVALID_PARAMETER,
        InvalidHandle => STATUS_INVALID_HANDLE,
        IntegrityHashMismatch
        | IntegrityLengthMismatch
        | IntegrityManifestInvalid
        | IntegrityChunkIdConflict => STATUS_FILE_CORRUPT_ERROR,
        NetworkTimeout => STATUS_IO_TIMEOUT,
        NetworkUnavailable => STATUS_UNEXPECTED_NETWORK_ERROR,
        ProtocolViolation | StorageError => STATUS_IO_DEVICE_ERROR,
        DiskFull => STATUS_DISK_FULL,
        ResourceExhausted => STATUS_INSUFFICIENT_RESOURCES,
        Cancelled => STATUS_CANCELLED,
        SharingViolation => STATUS_SHARING_VIOLATION,
        AuthFailed | PermissionDenied => STATUS_ACCESS_DENIED,
        InternalError => STATUS_INTERNAL_ERROR,
    })
}

/// Total version of [`ntstatus`], used by the FFI guard.
///
/// Every fallible entry point returns an `NTSTATUS` (boundary rule 9), so the
/// guard needs a total function. Startup-only codes can be produced by
/// `space_core_start` alone -- at that point there is no volume, and
/// `STATUS_DEVICE_NOT_READY` is the honest answer for "the filesystem never came
/// up". They are unreachable from every other entry point.
pub fn from_error_code(code: ErrorCode) -> NtStatus {
    ntstatus(code).unwrap_or(STATUS_DEVICE_NOT_READY)
}

/// Does this status indicate failure?
///
/// `BufferOverflow` is a **warning** (`0x8000_0005`), not an error: it is how
/// `GetSecurityByName` reports an undersized buffer while still returning the
/// required size. Treating it as a failure is how it collapses into
/// `InternalError` and produces an `S:` that Explorer can see but not open
/// (§6.2).
pub fn is_error(status: NtStatus) -> bool {
    // NTSTATUS severity lives in the top two bits: 0b11 == STATUS_SEVERITY_ERROR.
    (status >> 30) == 0b11
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M0.4's mapping test, moved here by ADR-0015. It iterates
    /// `ErrorCode::ALL`, so it works unchanged.
    ///
    /// It asserts **totality, not injectivity** -- every code has a mapping.
    /// Several codes legitimately share one NTSTATUS. Do not strengthen this to
    /// a bijection; see ADR-0015.
    #[test]
    fn every_error_code_has_a_mapping_unless_it_is_startup_only() {
        for &code in ErrorCode::ALL {
            assert_eq!(
                ntstatus(code).is_none(),
                code.is_startup_only(),
                "unmapped code: {code:?}"
            );
        }
    }

    #[test]
    fn from_error_code_is_total() {
        for &code in ErrorCode::ALL {
            let s = from_error_code(code);
            if code.is_startup_only() {
                assert_eq!(s, STATUS_DEVICE_NOT_READY, "{code:?}");
            } else {
                assert_eq!(s, ntstatus(code).unwrap(), "{code:?}");
            }
        }
    }

    #[test]
    fn the_mapping_is_deliberately_not_injective() {
        // Documented sharing. If either of these ever becomes distinct it is a
        // contract change, not a tidy-up.
        assert_eq!(
            ntstatus(ErrorCode::OperationTimeout),
            ntstatus(ErrorCode::NetworkTimeout)
        );
        assert_eq!(
            ntstatus(ErrorCode::IntegrityHashMismatch),
            ntstatus(ErrorCode::IntegrityChunkIdConflict)
        );
    }

    #[test]
    fn the_nine_phase_1_codes_map_to_the_manual_s_values() {
        // Phase 1 manual section 1.3, value column. Hard-coded on purpose: this
        // is the table being asserted, not a computation.
        let rows = [
            (ErrorCode::ObjectNameInvalid, 0xC000_0033u32),
            (ErrorCode::ObjectPathNotFound, 0xC000_003A),
            (ErrorCode::NotADirectory, 0xC000_0103),
            (ErrorCode::FileIsADirectory, 0xC000_00BA),
            (ErrorCode::EndOfFile, 0xC000_0011),
            (ErrorCode::NameTooLong, 0xC000_0106),
            (ErrorCode::CannotDelete, 0xC000_0121),
            (ErrorCode::BufferOverflow, 0x8000_0005),
            (ErrorCode::OperationTimeout, 0xC000_00B5),
        ];
        for (code, expected) in rows {
            assert_eq!(ntstatus(code), Some(expected), "{code:?}");
        }
    }

    #[test]
    fn buffer_overflow_is_a_warning_not_an_error() {
        // Section 1.3: "BufferOverflow is a warning status, not an error".
        assert!(!is_error(STATUS_BUFFER_OVERFLOW));
        assert!(!is_error(STATUS_SUCCESS));
        assert!(is_error(STATUS_OBJECT_NAME_NOT_FOUND));
        assert!(is_error(STATUS_INTERNAL_ERROR));
    }

    #[test]
    fn operation_timeout_maps_to_status_io_timeout() {
        // ADR-0009 / section 2.5: the only timeout Phase 1 can produce.
        assert_eq!(
            ntstatus(ErrorCode::OperationTimeout),
            Some(STATUS_IO_TIMEOUT)
        );
    }
}
