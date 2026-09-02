//! Manifest and version invariants (M0.7).
//!
//! One function per invariant, named with its number, so a failing test says
//! *which rule* broke rather than "invalid manifest". The canonical prose list
//! is `docs/protocols/schemas.md` -- it must stay in the same order and
//! numbering as this file.
//!
//! All arithmetic on attacker-controlled fields is checked: a corrupted or
//! hostile manifest is exactly what Phase 7 injects.

use crate::errors::{ErrorCode, SpaceError};
use crate::ids::ChunkId;
use crate::model::{File, Manifest, Version};

fn invalid(msg: impl Into<String>) -> SpaceError {
    SpaceError::new(ErrorCode::IntegrityManifestInvalid, msg)
}

/// Validate every structural invariant of a manifest (1-9, 13).
///
/// Invariants 10-12 are cross-object and live in [`validate_version_transition`],
/// [`validate_file_current_version`] and [`validate_version_parent`].
pub fn validate_manifest(m: &Manifest) -> Result<(), SpaceError> {
    inv13_empty_file_is_valid(m)?;
    inv01_entries_sorted(m)?;
    inv04_starts_at_zero(m)?;
    inv02_entries_contiguous(m)?;
    inv03_no_overlap(m)?;
    inv07_lengths_nonzero(m)?;
    inv05_lengths_sum_to_total(m)?;
    inv06_count_matches(m)?;
    inv09_chunk_ids_wellformed(m)?;
    inv08_chunk_offsets_within_chunk(m)?;
    Ok(())
}

/// Invariant 1: entries are ordered by `logical_offset`.
pub fn inv01_entries_sorted(m: &Manifest) -> Result<(), SpaceError> {
    if m.entries
        .windows(2)
        .any(|w| w[0].logical_offset > w[1].logical_offset)
    {
        return Err(invalid("inv01: entries not sorted by logical_offset"));
    }
    Ok(())
}

/// Invariant 2: entries are contiguous -- no gap between one and the next.
pub fn inv02_entries_contiguous(m: &Manifest) -> Result<(), SpaceError> {
    for w in m.entries.windows(2) {
        let end = w[0]
            .logical_offset
            .checked_add(w[0].length)
            .ok_or_else(|| invalid("inv02: logical_offset + length overflow"))?;
        if end < w[1].logical_offset {
            return Err(invalid("inv02: gap between entries"));
        }
    }
    Ok(())
}

/// Invariant 3: entries do not overlap.
pub fn inv03_no_overlap(m: &Manifest) -> Result<(), SpaceError> {
    for w in m.entries.windows(2) {
        let end = w[0]
            .logical_offset
            .checked_add(w[0].length)
            .ok_or_else(|| invalid("inv03: logical_offset + length overflow"))?;
        if end > w[1].logical_offset {
            return Err(invalid("inv03: entries overlap"));
        }
    }
    Ok(())
}

/// Invariant 4: a non-empty file's first entry starts at offset 0.
pub fn inv04_starts_at_zero(m: &Manifest) -> Result<(), SpaceError> {
    if let Some(first) = m.entries.first() {
        if first.logical_offset != 0 {
            return Err(invalid("inv04: first entry does not start at 0"));
        }
    }
    Ok(())
}

/// Invariant 5: entry lengths sum to `total_size`, in checked arithmetic.
pub fn inv05_lengths_sum_to_total(m: &Manifest) -> Result<(), SpaceError> {
    let mut sum: u64 = 0;
    for e in &m.entries {
        sum = sum
            .checked_add(e.length)
            .ok_or_else(|| invalid("inv05: length sum overflow"))?;
    }
    if sum != m.total_size {
        return Err(invalid("inv05: entry lengths do not sum to total_size"));
    }
    Ok(())
}

/// Invariant 6: `chunk_count` equals the number of entries.
pub fn inv06_count_matches(m: &Manifest) -> Result<(), SpaceError> {
    if m.chunk_count != m.entries.len() as u64 {
        return Err(invalid("inv06: chunk_count != entries.len()"));
    }
    Ok(())
}

/// Invariant 7: every entry has a non-zero length.
pub fn inv07_lengths_nonzero(m: &Manifest) -> Result<(), SpaceError> {
    if m.entries.iter().any(|e| e.length == 0) {
        return Err(invalid("inv07: zero-length entry"));
    }
    Ok(())
}

/// Invariant 8: each entry's `chunk_offset + length` stays within its chunk.
pub fn inv08_chunk_offsets_within_chunk(m: &Manifest) -> Result<(), SpaceError> {
    for e in &m.entries {
        let size = m
            .chunk_size(&e.chunk_id)
            .ok_or_else(|| invalid("inv08: entry references unknown chunk"))?;
        let end = e
            .chunk_offset
            .checked_add(e.length)
            .ok_or_else(|| invalid("inv08: chunk_offset + length overflow"))?;
        if end > size {
            return Err(invalid("inv08: chunk_offset + length exceeds chunk size"));
        }
    }
    Ok(())
}

/// Invariant 9: chunk ids are well-formed and `entries` <-> `chunks` agree.
pub fn inv09_chunk_ids_wellformed(m: &Manifest) -> Result<(), SpaceError> {
    for c in &m.chunks {
        ChunkId::parse(c.chunk_id.as_str())
            .map_err(|_| invalid("inv09: malformed chunk id in chunks"))?;
    }
    // every distinct entry chunk id has metadata
    for e in &m.entries {
        ChunkId::parse(e.chunk_id.as_str())
            .map_err(|_| invalid("inv09: malformed chunk id in entry"))?;
        if m.chunk_size(&e.chunk_id).is_none() {
            return Err(invalid("inv09: entry chunk id has no chunk metadata"));
        }
    }
    // no orphan chunk metadata
    for c in &m.chunks {
        if !m.entries.iter().any(|e| e.chunk_id == c.chunk_id) {
            return Err(invalid("inv09: unreferenced chunk metadata"));
        }
    }
    Ok(())
}

/// Invariant 13: an empty file (`total_size == 0`) has no entries and
/// `chunk_count == 0`. This is *valid*, not an error.
pub fn inv13_empty_file_is_valid(m: &Manifest) -> Result<(), SpaceError> {
    if m.total_size == 0 && (!m.entries.is_empty() || m.chunk_count != 0) {
        return Err(invalid("inv13: empty file must have no entries"));
    }
    Ok(())
}

/// Invariant 10: version state transitions.
pub fn validate_version_transition(
    from: crate::model::VersionState,
    to: crate::model::VersionState,
) -> Result<(), SpaceError> {
    if !from.can_transition_to(to) {
        return Err(SpaceError::new(
            ErrorCode::IntegrityManifestInvalid,
            format!("inv10: illegal version transition {from:?} -> {to:?}"),
        ));
    }
    Ok(())
}

/// Invariant 11: a file's `current_version_id` must reference a `Committed`
/// version of that same file.
pub fn validate_file_current_version(file: &File, current: &Version) -> Result<(), SpaceError> {
    if Some(current.version_id) != file.current_version_id {
        return Err(invalid("inv11: version is not this file's current version"));
    }
    if current.file_id != file.file_id {
        return Err(invalid(
            "inv11: current version belongs to a different file",
        ));
    }
    if current.state != crate::model::VersionState::Committed {
        return Err(SpaceError::new(
            ErrorCode::VersionNotFound,
            "inv11: current_version_id points at a non-committed version",
        ));
    }
    Ok(())
}

/// Invariant 12: a version's `parent_version_id` must belong to the same file.
pub fn validate_version_parent(child: &Version, parent: &Version) -> Result<(), SpaceError> {
    if child.parent_version_id != Some(parent.version_id) {
        return Err(invalid("inv12: parent_version_id mismatch"));
    }
    if child.file_id != parent.file_id {
        return Err(invalid("inv12: parent version belongs to a different file"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ChunkId, FileId, ManifestId, VersionId};
    use crate::model::{Chunk, ManifestEntry, VersionState};

    /// A valid `chunk_count`-entry manifest, each entry a distinct 4-byte chunk.
    fn valid_manifest(chunk_count: u64) -> Manifest {
        let mut entries = Vec::new();
        let mut chunks = Vec::new();
        for i in 0..chunk_count {
            let bytes = (i as u32).to_le_bytes();
            let id = ChunkId::from_bytes(&bytes);
            entries.push(ManifestEntry {
                logical_offset: i * 4,
                length: 4,
                chunk_id: id.clone(),
                chunk_offset: 0,
            });
            chunks.push(Chunk {
                chunk_id: id,
                size: 4,
            });
        }
        Manifest {
            manifest_id: ManifestId::new(),
            total_size: chunk_count * 4,
            chunk_count,
            entries,
            chunks,
        }
    }

    #[test]
    fn valid_manifests_pass() {
        for n in [1, 2, 3, 8] {
            validate_manifest(&valid_manifest(n)).unwrap();
        }
    }

    #[test]
    fn empty_manifest_is_valid() {
        let m = Manifest {
            manifest_id: ManifestId::new(),
            total_size: 0,
            chunk_count: 0,
            entries: vec![],
            chunks: vec![],
        };
        validate_manifest(&m).unwrap();
    }

    #[test]
    fn inv01_swap_two_entries_fails() {
        let mut m = valid_manifest(3);
        m.entries.swap(0, 2);
        assert_eq!(
            validate_manifest(&m).unwrap_err().code,
            ErrorCode::IntegrityManifestInvalid
        );
    }

    #[test]
    fn inv02_gap_fails() {
        let mut m = valid_manifest(3);
        m.entries[1].logical_offset += 1;
        m.entries[2].logical_offset += 1;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv03_overlap_fails() {
        let mut m = valid_manifest(3);
        m.entries[1].logical_offset -= 1;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv04_first_offset_one_fails() {
        let mut m = valid_manifest(2);
        m.entries[0].logical_offset = 1;
        m.entries[1].logical_offset = 5;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv05_total_size_off_by_one_fails() {
        let mut m = valid_manifest(3);
        m.total_size += 1;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv05_length_overflow_does_not_panic() {
        let mut m = valid_manifest(2);
        m.entries[0].length = u64::MAX;
        m.entries[1].length = u64::MAX;
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.code, ErrorCode::IntegrityManifestInvalid);
    }

    #[test]
    fn inv06_chunk_count_off_by_one_fails() {
        let mut m = valid_manifest(3);
        m.chunk_count += 1;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv07_zero_length_entry_fails() {
        let mut m = valid_manifest(2);
        m.entries[0].length = 0;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv08_chunk_offset_plus_length_exceeds_chunk_size_fails() {
        let mut m = valid_manifest(2);
        m.entries[0].chunk_offset = 2; // 2 + 4 > 4
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn inv09_corrupt_chunk_id_string_fails() {
        let mut m = valid_manifest(2);
        m.chunks[0].chunk_id = ChunkId::from_bytes(b"unreferenced");
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn within_file_dedup_is_legal() {
        // two entries, same chunk, still valid
        let id = ChunkId::from_bytes(b"AB");
        let m = Manifest {
            manifest_id: ManifestId::new(),
            total_size: 4,
            chunk_count: 2,
            entries: vec![
                ManifestEntry {
                    logical_offset: 0,
                    length: 2,
                    chunk_id: id.clone(),
                    chunk_offset: 0,
                },
                ManifestEntry {
                    logical_offset: 2,
                    length: 2,
                    chunk_id: id.clone(),
                    chunk_offset: 0,
                },
            ],
            chunks: vec![Chunk {
                chunk_id: id,
                size: 2,
            }],
        };
        validate_manifest(&m).unwrap();
    }

    #[test]
    fn inv10_state_transitions() {
        validate_version_transition(VersionState::Candidate, VersionState::Committed).unwrap();
        assert!(
            validate_version_transition(VersionState::Committed, VersionState::Candidate).is_err()
        );
    }

    #[test]
    fn inv11_current_version_must_be_committed() {
        let file_id = FileId::new();
        let vid = VersionId::new();
        let now = chrono::Utc::now();
        let mut file = File {
            file_id,
            parent_id: crate::ids::DirectoryId::new(),
            name: "a.txt".into(),
            current_version_id: Some(vid),
            created_at: now,
            modified_at: now,
        };
        let mut version = Version {
            version_id: vid,
            file_id,
            parent_version_id: None,
            manifest_id: ManifestId::new(),
            state: VersionState::Candidate,
            created_at: now,
        };
        assert!(validate_file_current_version(&file, &version).is_err());
        version.state = VersionState::Committed;
        validate_file_current_version(&file, &version).unwrap();
        file.current_version_id = Some(VersionId::new());
        assert!(validate_file_current_version(&file, &version).is_err());
    }

    #[test]
    fn inv12_parent_must_be_same_file() {
        let now = chrono::Utc::now();
        let file_id = FileId::new();
        let parent = Version {
            version_id: VersionId::new(),
            file_id,
            parent_version_id: None,
            manifest_id: ManifestId::new(),
            state: VersionState::Committed,
            created_at: now,
        };
        let mut child = Version {
            version_id: VersionId::new(),
            file_id,
            parent_version_id: Some(parent.version_id),
            manifest_id: ManifestId::new(),
            state: VersionState::Candidate,
            created_at: now,
        };
        validate_version_parent(&child, &parent).unwrap();
        child.file_id = FileId::new();
        assert!(validate_version_parent(&child, &parent).is_err());
    }
}
