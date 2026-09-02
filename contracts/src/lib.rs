//! `contracts` -- the shared vocabulary of SPACE.
//!
//! Every crate on both sides of the client/server boundary depends on this one
//! and nothing depends the other way. It contains only definitions and pure
//! validation: identity types (M0.3), the error model and NTSTATUS map (M0.4),
//! the domain model and its 13 named invariants (M0.7), the API DTOs (M0.8),
//! structural redaction (M0.6) and the structured-logging schema (M0.6).
//!
//! See `docs/protocols/` for the prose specifications these modules implement.

#![forbid(unsafe_code)]

pub mod api;
pub mod errors;
pub mod ids;
pub mod logging;
pub mod model;
pub mod redaction;
pub mod validate;

pub use errors::{ErrorCode, NtStatus, Origin, SpaceError};
pub use ids::{ChunkId, DirectoryId, FileId, ManifestId, OperationId, RequestId, VersionId};
pub use model::{Chunk, File, Manifest, ManifestEntry, Version, VersionState};
pub use redaction::{sanitize_url, Password, Secret, Token};
