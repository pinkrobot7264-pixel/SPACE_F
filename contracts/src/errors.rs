//! Error model for SPACE (M0.4).
//!
//! One error type crosses every boundary: [`SpaceError`]. Every distinct failure
//! is a variant of [`ErrorCode`]. Each code is *fully classified* -- it has a
//! retryability, an [`Origin`], and (unless it is a startup-only error) an
//! `NTSTATUS` value that the WinFsp adapter will hand back to Windows.
//!
//! Rules (see `docs/protocols/errors.md`):
//!  * No code path blocks indefinitely; every wait resolves to `NetworkTimeout`,
//!    `Cancelled`, or success.
//!  * Retryability is a property of the code, read from this table by the
//!    transfer engine -- never a per-call-site judgement.

use serde::{Deserialize, Serialize};

use crate::ids::{OperationId, RequestId};

/// Windows `NTSTATUS` value. See `ntstatus.h`.
pub type NtStatus = u32;

// NTSTATUS constants used by the mapping below.
pub const STATUS_OBJECT_NAME_NOT_FOUND: NtStatus = 0xC000_0034;
pub const STATUS_OBJECT_NAME_COLLISION: NtStatus = 0xC000_0035;
pub const STATUS_DIRECTORY_NOT_EMPTY: NtStatus = 0xC000_0101;
pub const STATUS_INVALID_PARAMETER: NtStatus = 0xC000_000D;
pub const STATUS_INVALID_HANDLE: NtStatus = 0xC000_0008;
pub const STATUS_FILE_CORRUPT_ERROR: NtStatus = 0xC000_0102;
pub const STATUS_IO_TIMEOUT: NtStatus = 0xC000_00B5;
pub const STATUS_DEVICE_NOT_READY: NtStatus = 0xC000_00A3;
pub const STATUS_UNEXPECTED_NETWORK_ERROR: NtStatus = 0xC000_00C4;
pub const STATUS_IO_DEVICE_ERROR: NtStatus = 0xC000_0185;
pub const STATUS_DISK_FULL: NtStatus = 0xC000_007F;
pub const STATUS_INSUFFICIENT_RESOURCES: NtStatus = 0xC000_009A;
pub const STATUS_CANCELLED: NtStatus = 0xC000_0120;
pub const STATUS_SHARING_VIOLATION: NtStatus = 0xC000_0043;
pub const STATUS_ACCESS_DENIED: NtStatus = 0xC000_0022;
pub const STATUS_INTERNAL_ERROR: NtStatus = 0xC000_00E5;

/// Which side of the client/server boundary a code originates from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Client,
    Server,
    Either,
}

/// Every distinct failure mode in SPACE.
///
/// Serialized in `SCREAMING_SNAKE_CASE` so the wire form matches
/// `docs/protocols/errors.md` exactly.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ConfigMissing,
    ConfigInvalid,
    ConfigUnsupportedVersion,
    FileNotFound,
    FileExists,
    DirectoryNotEmpty,
    VersionNotFound,
    ManifestNotFound,
    ChunkNotFound,
    InvalidParameter,
    InvalidHandle,
    IntegrityHashMismatch,
    IntegrityLengthMismatch,
    IntegrityManifestInvalid,
    IntegrityChunkIdConflict,
    NetworkTimeout,
    NetworkUnavailable,
    ProtocolViolation,
    StorageError,
    DiskFull,
    ResourceExhausted,
    Cancelled,
    SharingViolation,
    AuthFailed,
    PermissionDenied,
    InternalError,
}

impl ErrorCode {
    /// Every variant, explicitly listed. The `all_slice_covers_every_variant`
    /// test is deliberately brittle: adding a variant without adding it here
    /// breaks the build.
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::ConfigMissing,
        ErrorCode::ConfigInvalid,
        ErrorCode::ConfigUnsupportedVersion,
        ErrorCode::FileNotFound,
        ErrorCode::FileExists,
        ErrorCode::DirectoryNotEmpty,
        ErrorCode::VersionNotFound,
        ErrorCode::ManifestNotFound,
        ErrorCode::ChunkNotFound,
        ErrorCode::InvalidParameter,
        ErrorCode::InvalidHandle,
        ErrorCode::IntegrityHashMismatch,
        ErrorCode::IntegrityLengthMismatch,
        ErrorCode::IntegrityManifestInvalid,
        ErrorCode::IntegrityChunkIdConflict,
        ErrorCode::NetworkTimeout,
        ErrorCode::NetworkUnavailable,
        ErrorCode::ProtocolViolation,
        ErrorCode::StorageError,
        ErrorCode::DiskFull,
        ErrorCode::ResourceExhausted,
        ErrorCode::Cancelled,
        ErrorCode::SharingViolation,
        ErrorCode::AuthFailed,
        ErrorCode::PermissionDenied,
        ErrorCode::InternalError,
    ];

    /// Startup-only errors happen before the filesystem is mounted, so they have
    /// no meaningful `NTSTATUS`.
    pub fn is_startup_only(self) -> bool {
        matches!(
            self,
            ErrorCode::ConfigMissing
                | ErrorCode::ConfigInvalid
                | ErrorCode::ConfigUnsupportedVersion
        )
    }

    /// Whether the transfer engine may retry an operation that failed with this
    /// code. This is the single source of truth; call sites never decide.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::NetworkTimeout
                | ErrorCode::NetworkUnavailable
                | ErrorCode::ResourceExhausted
        )
    }

    /// Which side of the boundary the code can originate from.
    pub fn origin(self) -> Origin {
        use ErrorCode::*;
        match self {
            ConfigMissing
            | ConfigInvalid
            | ConfigUnsupportedVersion
            | InvalidHandle
            | NetworkTimeout
            | NetworkUnavailable
            | Cancelled
            | SharingViolation => Origin::Client,
            AuthFailed | PermissionDenied => Origin::Server,
            FileNotFound
            | FileExists
            | DirectoryNotEmpty
            | VersionNotFound
            | ManifestNotFound
            | ChunkNotFound
            | InvalidParameter
            | IntegrityHashMismatch
            | IntegrityLengthMismatch
            | IntegrityManifestInvalid
            | IntegrityChunkIdConflict
            | ProtocolViolation
            | StorageError
            | DiskFull
            | ResourceExhausted
            | InternalError => Origin::Either,
        }
    }

    /// The `NTSTATUS` this code maps to, or `None` for startup-only errors.
    pub fn ntstatus(self) -> Option<NtStatus> {
        use ErrorCode::*;
        Some(match self {
            ConfigMissing | ConfigInvalid | ConfigUnsupportedVersion => return None,
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
}

/// The error that crosses every SPACE boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Box<SpaceError>>,
}

impl SpaceError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            operation_id: None,
            request_id: None,
            source: None,
        }
    }

    /// Shorthand for the most common validation failure.
    pub fn invalid_param(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParameter, message)
    }

    pub fn with_operation(mut self, id: OperationId) -> Self {
        self.operation_id = Some(id);
        self
    }

    pub fn with_request(mut self, id: RequestId) -> Self {
        self.request_id = Some(id);
        self
    }

    /// Wrap a root cause, preserving the chain.
    pub fn caused_by(mut self, source: SpaceError) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Walk to the deepest cause.
    pub fn root_cause(&self) -> &SpaceError {
        let mut cur = self;
        while let Some(next) = &cur.source {
            cur = next;
        }
        cur
    }

    pub fn retryable(&self) -> bool {
        self.code.retryable()
    }

    pub fn ntstatus(&self) -> Option<NtStatus> {
        self.code.ntstatus()
    }
}

impl std::fmt::Display for SpaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)?;
        if let Some(src) = &self.source {
            write!(f, " (caused by {src})")?;
        }
        Ok(())
    }
}

impl std::error::Error for SpaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_slice_covers_every_variant() {
        // Guards against adding a variant and forgetting ALL.
        assert_eq!(ErrorCode::ALL.len(), 26);
    }

    #[test]
    fn every_error_code_is_fully_classified() {
        for &code in ErrorCode::ALL {
            let _ = code.retryable();
            let _ = code.origin();
            assert_eq!(
                code.ntstatus().is_none(),
                code.is_startup_only(),
                "unmapped code: {code:?}"
            );
        }
    }

    #[test]
    fn retryable_is_exactly_the_three_network_resource_codes() {
        let retryable: Vec<_> = ErrorCode::ALL
            .iter()
            .copied()
            .filter(|c| c.retryable())
            .collect();
        assert_eq!(
            retryable,
            vec![
                ErrorCode::NetworkTimeout,
                ErrorCode::NetworkUnavailable,
                ErrorCode::ResourceExhausted
            ]
        );
    }

    #[test]
    fn codes_round_trip_through_serde_with_screaming_snake_names() {
        for &code in ErrorCode::ALL {
            let json = serde_json::to_string(&code).unwrap();
            assert!(json.starts_with('"') && json.ends_with('"'));
            assert_eq!(json, json.to_uppercase(), "{code:?} is not SCREAMING_SNAKE");
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(code, back);
        }
        assert_eq!(
            serde_json::to_string(&ErrorCode::IntegrityHashMismatch).unwrap(),
            "\"INTEGRITY_HASH_MISMATCH\""
        );
    }

    #[test]
    fn error_chaining_preserves_root_cause_through_three_levels() {
        let root = SpaceError::new(ErrorCode::DiskFull, "no space on device");
        let mid = SpaceError::new(ErrorCode::StorageError, "chunk publish failed").caused_by(root);
        let top = SpaceError::new(ErrorCode::InternalError, "commit aborted").caused_by(mid);
        assert_eq!(top.root_cause().code, ErrorCode::DiskFull);
    }

    #[test]
    fn display_contains_code_and_message_but_no_secret() {
        let e = SpaceError::new(ErrorCode::AuthFailed, "token rejected");
        let s = e.to_string();
        assert!(s.contains("AuthFailed"));
        assert!(s.contains("token rejected"));
        assert!(!s.contains("wJalrXUtnFEMI"));
    }

    #[test]
    fn space_error_round_trips_through_json() {
        let e = SpaceError::new(ErrorCode::ChunkNotFound, "b3:deadbeef missing");
        let json = serde_json::to_string(&e).unwrap();
        let back: SpaceError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, ErrorCode::ChunkNotFound);
        assert_eq!(back.message, e.message);
    }
}
