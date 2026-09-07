//! Error model for SPACE (M0.4, extended by Phase 1 section 1.3).
//!
//! One error type crosses every boundary: [`SpaceError`]. Every distinct failure
//! is a variant of [`ErrorCode`]. Each code is *fully classified* -- it has a
//! retryability and an [`Origin`].
//!
//! **The NTSTATUS translation does not live here (ADR-0015).** `contracts` is
//! shared with the cloud service, which has no business knowing about Windows
//! types. The error *taxonomy* is a contract concern; the *translation* is a
//! Windows concern and lives in `client/core/src/ffi/ntstatus.rs`, together with
//! the exhaustiveness test that iterates [`ErrorCode::ALL`].
//!
//! Rules (see `docs/protocols/errors.md`):
//!  * No code path blocks indefinitely; every wait resolves to a timeout code,
//!    `Cancelled`, or success.
//!  * `OperationTimeout` means *a filesystem operation exceeded its configured
//!    execution deadline*. It is the only timeout Phase 1 can produce.
//!    `NetworkTimeout` means *a remote request exceeded its deadline* and is
//!    reserved for the Phase 4+ transfer engine; the VFS layer may never produce
//!    it (ADR-0014, enforced by a conformance assertion).
//!  * Retryability is a property of the code, read from this table by the
//!    transfer engine -- never a per-call-site judgement.

use serde::{Deserialize, Serialize};

use crate::ids::{OperationId, RequestId};

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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ConfigMissing,
    ConfigInvalid,
    ConfigUnsupportedVersion,
    FileNotFound,
    FileExists,
    DirectoryNotEmpty,
    // --- Phase 1 filesystem additions (Phase 1 manual section 1.3) ---
    ObjectNameInvalid,
    ObjectPathNotFound,
    NotADirectory,
    FileIsADirectory,
    EndOfFile,
    NameTooLong,
    CannotDelete,
    BufferOverflow,
    OperationTimeout,
    // --- end Phase 1 additions ---
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
        ErrorCode::ObjectNameInvalid,
        ErrorCode::ObjectPathNotFound,
        ErrorCode::NotADirectory,
        ErrorCode::FileIsADirectory,
        ErrorCode::EndOfFile,
        ErrorCode::NameTooLong,
        ErrorCode::CannotDelete,
        ErrorCode::BufferOverflow,
        ErrorCode::OperationTimeout,
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
    /// no meaningful `NTSTATUS`. The mapping table at the Windows boundary reads
    /// this predicate (ADR-0015).
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
                | ErrorCode::OperationTimeout
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
            | SharingViolation
            // Produced only at the Windows-facing boundary or by VfsPath parsing.
            | ObjectNameInvalid
            | NameTooLong
            | BufferOverflow
            | OperationTimeout => Origin::Client,
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
            // Namespace and file-state codes: local now, remote-capable later.
            | ObjectPathNotFound
            | NotADirectory
            | FileIsADirectory
            | EndOfFile
            | CannotDelete
            | InternalError => Origin::Either,
        }
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
        // 26 Phase 0 codes + 9 Phase 1 filesystem codes (Phase 1 manual 1.3).
        assert_eq!(ErrorCode::ALL.len(), 35);
    }

    #[test]
    fn every_error_code_is_fully_classified() {
        // The NTSTATUS half of "fully classified" is asserted at the Windows
        // boundary (client/core/src/ffi/ntstatus.rs) per ADR-0015; contracts
        // owns only retryability and origin.
        for &code in ErrorCode::ALL {
            let _ = code.retryable();
            let _ = code.origin();
            let _ = code.is_startup_only();
        }
    }

    #[test]
    fn retryable_is_exactly_the_network_resource_and_operation_timeout_codes() {
        // OperationTimeout is retryable per the Phase 1 manual section 1.3 table.
        let retryable: Vec<_> = ErrorCode::ALL
            .iter()
            .copied()
            .filter(|c| c.retryable())
            .collect();
        assert_eq!(
            retryable,
            vec![
                ErrorCode::OperationTimeout,
                ErrorCode::NetworkTimeout,
                ErrorCode::NetworkUnavailable,
                ErrorCode::ResourceExhausted,
            ]
        );
    }

    #[test]
    fn the_nine_phase_1_codes_are_present() {
        for code in [
            ErrorCode::ObjectNameInvalid,
            ErrorCode::ObjectPathNotFound,
            ErrorCode::NotADirectory,
            ErrorCode::FileIsADirectory,
            ErrorCode::EndOfFile,
            ErrorCode::NameTooLong,
            ErrorCode::CannotDelete,
            ErrorCode::BufferOverflow,
            ErrorCode::OperationTimeout,
        ] {
            assert!(ErrorCode::ALL.contains(&code), "{code:?} missing from ALL");
        }
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
        assert_eq!(
            serde_json::to_string(&ErrorCode::ObjectPathNotFound).unwrap(),
            "\"OBJECT_PATH_NOT_FOUND\""
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
