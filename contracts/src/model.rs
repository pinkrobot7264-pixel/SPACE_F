//! Domain model (M0.7).
//!
//! These structs are the vocabulary every later phase speaks. They are
//! deliberately small: a `File` names bytes, a `Version` is an immutable
//! snapshot, a `Manifest` maps logical byte ranges onto immutable `Chunk`s.
//!
//! Every rule a `Manifest` obeys is a *named invariant* in [`crate::validate`]
//! and in `docs/protocols/schemas.md`. No invariant lives only in code.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{ChunkId, DirectoryId, FileId, ManifestId, VersionId};

/// Whether a version is still being assembled or is authoritative.
///
/// The only legal transition is `Candidate -> Committed` (invariant 10). A
/// crash can leave at worst a `Candidate`; it can never corrupt a `Committed`
/// version.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VersionState {
    Candidate,
    Committed,
}

impl VersionState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!((self, next), (Self::Candidate, Self::Committed))
    }
}

/// A named entry in the namespace. Holds no bytes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct File {
    pub file_id: FileId,
    pub parent_id: DirectoryId,
    pub name: String,
    /// `None` until the first version commits. Must reference a `Committed`
    /// version (invariant 11).
    pub current_version_id: Option<VersionId>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

/// An immutable snapshot of a file's bytes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Version {
    pub version_id: VersionId,
    pub file_id: FileId,
    /// The version this one was derived from, if any. Must belong to the same
    /// `file_id` (invariant 12).
    pub parent_version_id: Option<VersionId>,
    pub manifest_id: ManifestId,
    pub state: VersionState,
    pub created_at: DateTime<Utc>,
}

/// Metadata for one immutable, content-addressed chunk. There is no separate
/// `hash` field: `chunk_id` *is* the hash.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub chunk_id: ChunkId,
    pub size: u64,
}

/// One contiguous logical byte range, served from a slice of one chunk.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Offset of this range within the logical file.
    pub logical_offset: u64,
    /// Length of this range, in bytes. Always `> 0` (invariant 7).
    pub length: u64,
    /// The chunk that backs this range.
    pub chunk_id: ChunkId,
    /// Offset within the backing chunk. `chunk_offset + length <= chunk.size`
    /// (invariant 8).
    pub chunk_offset: u64,
}

/// The full logical-to-physical map for one version.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_id: ManifestId,
    pub total_size: u64,
    /// Number of entries (segments), not distinct chunks (invariant 6).
    pub chunk_count: u64,
    pub entries: Vec<ManifestEntry>,
    /// Metadata for every *distinct* chunk referenced by `entries`. Within-file
    /// deduplication is legal: two entries may name the same chunk.
    pub chunks: Vec<Chunk>,
}

impl Manifest {
    /// Look up a referenced chunk's declared size.
    pub fn chunk_size(&self, id: &ChunkId) -> Option<u64> {
        self.chunks
            .iter()
            .find(|c| &c.chunk_id == id)
            .map(|c| c.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_state_machine_only_allows_candidate_to_committed() {
        assert!(VersionState::Candidate.can_transition_to(VersionState::Committed));
        assert!(!VersionState::Committed.can_transition_to(VersionState::Candidate));
        assert!(!VersionState::Committed.can_transition_to(VersionState::Committed));
        assert!(!VersionState::Candidate.can_transition_to(VersionState::Candidate));
    }
}
