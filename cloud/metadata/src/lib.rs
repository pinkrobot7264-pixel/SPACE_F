//! Server-side metadata store (M0.8/M0.12).
//!
//! [`MetadataStore`] is the trait the API layer talks to. In Phase 0 the only
//! implementation is [`InMemoryMetadataStore`]; Phase 8 adds a PostgreSQL-backed
//! one behind the same trait (see the Phase plan, Part G).
//!
//! The store enforces the cross-object invariants (10, 11, 12) and the version
//! state machine: a version is invisible to readers until it is `Committed`,
//! and committing runs full manifest validation first (guard rails #3, #4).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;

use contracts::validate::{validate_manifest, validate_version_transition};
use contracts::{
    Chunk, ChunkId, DirectoryId, ErrorCode, File, FileId, Manifest, ManifestId, SpaceError,
    Version, VersionId, VersionState,
};

#[async_trait]
pub trait MetadataStore: Send + Sync {
    async fn create_file(
        &self,
        parent_id: DirectoryId,
        name: &str,
        idempotency_key: &str,
    ) -> Result<File, SpaceError>;
    async fn get_file(&self, file_id: FileId) -> Result<File, SpaceError>;
    async fn list_children(&self, dir_id: DirectoryId) -> Result<Vec<File>, SpaceError>;

    async fn register_chunk(&self, chunk_id: ChunkId, size: u64) -> Result<Chunk, SpaceError>;
    async fn put_manifest(&self, manifest: Manifest) -> Result<Manifest, SpaceError>;
    async fn get_manifest(&self, manifest_id: ManifestId) -> Result<Manifest, SpaceError>;

    async fn create_version(
        &self,
        file_id: FileId,
        version_id: VersionId,
        parent_version_id: Option<VersionId>,
        manifest_id: ManifestId,
    ) -> Result<Version, SpaceError>;
    async fn commit_version(&self, version_id: VersionId) -> Result<Version, SpaceError>;
    /// Only returns `Committed` versions; a `Candidate` is `VersionNotFound`.
    async fn get_current_version(&self, version_id: VersionId) -> Result<Version, SpaceError>;
}

#[derive(Default)]
struct Inner {
    files: HashMap<FileId, File>,
    file_idempotency: HashMap<String, FileId>,
    versions: HashMap<VersionId, Version>,
    manifests: HashMap<ManifestId, Manifest>,
    chunks: HashMap<ChunkId, Chunk>,
}

#[derive(Default)]
pub struct InMemoryMetadataStore {
    inner: Mutex<Inner>,
}

impl InMemoryMetadataStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn not_found(code: ErrorCode, what: &str) -> SpaceError {
    SpaceError::new(code, format!("{what} not found"))
}

#[async_trait]
impl MetadataStore for InMemoryMetadataStore {
    async fn create_file(
        &self,
        parent_id: DirectoryId,
        name: &str,
        idempotency_key: &str,
    ) -> Result<File, SpaceError> {
        if name.is_empty() || name.contains(['/', '\\', '\0']) {
            return Err(SpaceError::invalid_param("illegal file name"));
        }
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g.file_idempotency.get(idempotency_key) {
            return Ok(g.files[existing].clone());
        }
        let now = Utc::now();
        let file = File {
            file_id: FileId::new(),
            parent_id,
            name: name.to_string(),
            current_version_id: None,
            created_at: now,
            modified_at: now,
        };
        g.file_idempotency
            .insert(idempotency_key.to_string(), file.file_id);
        g.files.insert(file.file_id, file.clone());
        Ok(file)
    }

    async fn get_file(&self, file_id: FileId) -> Result<File, SpaceError> {
        self.inner
            .lock()
            .unwrap()
            .files
            .get(&file_id)
            .cloned()
            .ok_or_else(|| not_found(ErrorCode::FileNotFound, "file"))
    }

    async fn list_children(&self, dir_id: DirectoryId) -> Result<Vec<File>, SpaceError> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<File> = g
            .files
            .values()
            .filter(|f| f.parent_id == dir_id)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(v)
    }

    async fn register_chunk(&self, chunk_id: ChunkId, size: u64) -> Result<Chunk, SpaceError> {
        ChunkId::parse(chunk_id.as_str())?;
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g.chunks.get(&chunk_id) {
            if existing.size != size {
                return Err(SpaceError::new(
                    ErrorCode::IntegrityChunkIdConflict,
                    "chunk already registered with a different size",
                ));
            }
            return Ok(existing.clone());
        }
        let chunk = Chunk {
            chunk_id: chunk_id.clone(),
            size,
        };
        g.chunks.insert(chunk_id, chunk.clone());
        Ok(chunk)
    }

    async fn put_manifest(&self, manifest: Manifest) -> Result<Manifest, SpaceError> {
        validate_manifest(&manifest)?;
        let mut g = self.inner.lock().unwrap();
        // referential integrity against registered chunks
        for c in &manifest.chunks {
            match g.chunks.get(&c.chunk_id) {
                Some(reg) if reg.size == c.size => {}
                Some(_) => {
                    return Err(SpaceError::new(
                        ErrorCode::IntegrityManifestInvalid,
                        "manifest chunk size disagrees with registration",
                    ))
                }
                None => {
                    return Err(SpaceError::new(
                        ErrorCode::ChunkNotFound,
                        "manifest references an unregistered chunk",
                    ))
                }
            }
        }
        if let Some(existing) = g.manifests.get(&manifest.manifest_id) {
            if *existing != manifest {
                return Err(SpaceError::new(
                    ErrorCode::IntegrityManifestInvalid,
                    "manifest id already stores different content",
                ));
            }
        }
        g.manifests.insert(manifest.manifest_id, manifest.clone());
        Ok(manifest)
    }

    async fn get_manifest(&self, manifest_id: ManifestId) -> Result<Manifest, SpaceError> {
        self.inner
            .lock()
            .unwrap()
            .manifests
            .get(&manifest_id)
            .cloned()
            .ok_or_else(|| not_found(ErrorCode::ManifestNotFound, "manifest"))
    }

    async fn create_version(
        &self,
        file_id: FileId,
        version_id: VersionId,
        parent_version_id: Option<VersionId>,
        manifest_id: ManifestId,
    ) -> Result<Version, SpaceError> {
        let mut g = self.inner.lock().unwrap();
        if !g.files.contains_key(&file_id) {
            return Err(not_found(ErrorCode::FileNotFound, "file"));
        }
        if !g.manifests.contains_key(&manifest_id) {
            return Err(not_found(ErrorCode::ManifestNotFound, "manifest"));
        }
        if let Some(parent) = parent_version_id {
            match g.versions.get(&parent) {
                Some(p) if p.file_id == file_id => {}
                Some(_) => {
                    return Err(SpaceError::new(
                        ErrorCode::IntegrityManifestInvalid,
                        "parent version belongs to a different file",
                    ))
                }
                None => return Err(not_found(ErrorCode::VersionNotFound, "parent version")),
            }
        }
        if let Some(existing) = g.versions.get(&version_id) {
            // idempotent by client-generated version_id
            if existing.file_id == file_id
                && existing.parent_version_id == parent_version_id
                && existing.manifest_id == manifest_id
            {
                return Ok(existing.clone());
            }
            return Err(SpaceError::new(
                ErrorCode::IntegrityChunkIdConflict,
                "version_id already used with different content",
            ));
        }
        let version = Version {
            version_id,
            file_id,
            parent_version_id,
            manifest_id,
            state: VersionState::Candidate,
            created_at: Utc::now(),
        };
        g.versions.insert(version_id, version.clone());
        Ok(version)
    }

    async fn commit_version(&self, version_id: VersionId) -> Result<Version, SpaceError> {
        let mut g = self.inner.lock().unwrap();
        let version = g
            .versions
            .get(&version_id)
            .cloned()
            .ok_or_else(|| not_found(ErrorCode::VersionNotFound, "version"))?;
        if version.state == VersionState::Committed {
            return Ok(version); // idempotent re-commit
        }
        validate_version_transition(version.state, VersionState::Committed)?;
        let manifest = g
            .manifests
            .get(&version.manifest_id)
            .cloned()
            .ok_or_else(|| not_found(ErrorCode::ManifestNotFound, "manifest"))?;
        // Re-validate before the version becomes authoritative.
        validate_manifest(&manifest)?;

        let mut committed = version.clone();
        committed.state = VersionState::Committed;
        g.versions.insert(version_id, committed.clone());
        if let Some(file) = g.files.get_mut(&committed.file_id) {
            file.current_version_id = Some(version_id);
            file.modified_at = Utc::now();
        }
        Ok(committed)
    }

    async fn get_current_version(&self, version_id: VersionId) -> Result<Version, SpaceError> {
        let g = self.inner.lock().unwrap();
        match g.versions.get(&version_id) {
            Some(v) if v.state == VersionState::Committed => Ok(v.clone()),
            _ => Err(not_found(ErrorCode::VersionNotFound, "committed version")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn candidate_version_is_invisible_until_committed() {
        let s = InMemoryMetadataStore::new();
        let dir = DirectoryId::new();
        let file = s.create_file(dir, "a.txt", "k1").await.unwrap();

        let chunk = Chunk {
            chunk_id: ChunkId::from_bytes(b"hi"),
            size: 2,
        };
        s.register_chunk(chunk.chunk_id.clone(), 2).await.unwrap();
        let manifest = Manifest {
            manifest_id: ManifestId::new(),
            total_size: 2,
            chunk_count: 1,
            entries: vec![contracts::ManifestEntry {
                logical_offset: 0,
                length: 2,
                chunk_id: chunk.chunk_id.clone(),
                chunk_offset: 0,
            }],
            chunks: vec![chunk],
        };
        let manifest = s.put_manifest(manifest).await.unwrap();

        let vid = VersionId::new();
        s.create_version(file.file_id, vid, None, manifest.manifest_id)
            .await
            .unwrap();
        assert_eq!(
            s.get_current_version(vid).await.unwrap_err().code,
            ErrorCode::VersionNotFound
        );

        let committed = s.commit_version(vid).await.unwrap();
        assert_eq!(committed.state, VersionState::Committed);
        assert_eq!(
            s.get_file(file.file_id).await.unwrap().current_version_id,
            Some(vid)
        );
        // idempotent re-commit
        assert!(s.commit_version(vid).await.is_ok());
    }

    #[tokio::test]
    async fn create_file_is_idempotent_by_key() {
        let s = InMemoryMetadataStore::new();
        let dir = DirectoryId::new();
        let a = s.create_file(dir, "a", "same").await.unwrap();
        let b = s.create_file(dir, "a", "same").await.unwrap();
        assert_eq!(a.file_id, b.file_id);
    }
}
