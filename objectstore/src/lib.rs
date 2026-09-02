//! Object-store abstraction and the in-memory fake (M0.9).
//!
//! The store holds immutable, content-addressed chunk bodies keyed by
//! [`ChunkId`]. Two traits, deliberately split:
//!  * [`ObjectStore`] -- what client-facing code sees. **No delete.** A
//!    committed version's chunks are never removed by normal operation.
//!  * [`ObjectStoreAdmin`] -- adds delete, for lifecycle tooling and tests.
//!
//! [`FakeObjectStore`] is the reference implementation. Every store, including
//! Phase 7's disk-backed fake and Phase 9's S3 store, must pass
//! [`conformance::run_conformance_suite`] unchanged.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use tokio::sync::RwLock;

use contracts::{ChunkId, ErrorCode, SpaceError};

pub mod conformance;

/// Read/write access to immutable chunk bodies. No delete.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Store `bytes` under `id`. Idempotent: storing identical bytes again is
    /// `Ok`. Storing *different* bytes under an existing id is
    /// `IntegrityChunkIdConflict`. The id must address the bytes.
    async fn put(&self, id: &ChunkId, bytes: &[u8]) -> Result<(), SpaceError>;

    /// Fetch the whole object.
    async fn get(&self, id: &ChunkId) -> Result<Bytes, SpaceError>;

    /// Fetch exactly `length` bytes starting at `offset`. Never returns a short
    /// read: a range past the end is an error.
    async fn get_range(&self, id: &ChunkId, offset: u64, length: u32) -> Result<Bytes, SpaceError>;

    /// Size of the stored object, in bytes.
    async fn head(&self, id: &ChunkId) -> Result<u64, SpaceError>;

    /// Whether an object exists.
    async fn exists(&self, id: &ChunkId) -> Result<bool, SpaceError>;
}

/// Adds destructive operations. Kept out of [`ObjectStore`] so client code
/// cannot delete a chunk even by accident.
#[async_trait]
pub trait ObjectStoreAdmin: ObjectStore {
    async fn delete(&self, id: &ChunkId) -> Result<(), SpaceError>;
}

/// In-memory reference implementation.
#[derive(Default)]
pub struct FakeObjectStore {
    inner: RwLock<HashMap<ChunkId, Bytes>>,
}

impl FakeObjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only: reach in and flip a byte of a stored object so the read
    /// path's verification can be exercised.
    pub async fn corrupt_for_test(&self, id: &ChunkId, at: usize) {
        let mut g = self.inner.write().await;
        if let Some(obj) = g.get_mut(id) {
            let mut v = obj.to_vec();
            if at < v.len() {
                v[at] ^= 0xFF;
                *obj = Bytes::from(v);
            }
        }
    }

    /// Test-only: truncate a stored object to `len` bytes.
    pub async fn truncate_for_test(&self, id: &ChunkId, len: usize) {
        let mut g = self.inner.write().await;
        if let Some(obj) = g.get_mut(id) {
            let mut v = obj.to_vec();
            v.truncate(len);
            *obj = Bytes::from(v);
        }
    }
}

#[async_trait]
impl ObjectStore for FakeObjectStore {
    async fn put(&self, id: &ChunkId, bytes: &[u8]) -> Result<(), SpaceError> {
        if !id.verify(bytes) {
            return Err(SpaceError::new(
                ErrorCode::IntegrityChunkIdConflict,
                "chunk_id does not address these bytes",
            ));
        }
        let mut g = self.inner.write().await;
        match g.get(id) {
            Some(existing) if existing == bytes => Ok(()),
            Some(_) => Err(SpaceError::new(
                ErrorCode::IntegrityChunkIdConflict,
                "chunk_id already stores different bytes",
            )),
            None => {
                g.insert(id.clone(), Bytes::copy_from_slice(bytes));
                Ok(())
            }
        }
    }

    async fn get(&self, id: &ChunkId) -> Result<Bytes, SpaceError> {
        let g = self.inner.read().await;
        let obj = g
            .get(id)
            .ok_or_else(|| SpaceError::new(ErrorCode::ChunkNotFound, "no such chunk"))?;
        // Read-side verification: a corrupted store must never serve wrong bytes
        // as valid.
        if !id.verify(obj) {
            return Err(SpaceError::new(
                ErrorCode::IntegrityHashMismatch,
                "stored bytes do not match chunk_id",
            ));
        }
        Ok(obj.clone())
    }

    async fn get_range(&self, id: &ChunkId, offset: u64, length: u32) -> Result<Bytes, SpaceError> {
        let g = self.inner.read().await;
        let obj = g
            .get(id)
            .ok_or_else(|| SpaceError::new(ErrorCode::ChunkNotFound, "no such chunk"))?;
        let end = offset
            .checked_add(length as u64)
            .ok_or_else(|| SpaceError::invalid_param("offset + length overflow"))?;
        // Bounds first: a truncated stored object surfaces as a length mismatch,
        // not a hash mismatch.
        if end > obj.len() as u64 {
            return Err(SpaceError::new(
                ErrorCode::IntegrityLengthMismatch,
                "requested range extends past the object",
            ));
        }
        // A full-object read can be verified against the content address; a
        // partial read cannot, and the caller verifies whole chunks itself.
        if offset == 0 && length as usize == obj.len() && !id.verify(obj) {
            return Err(SpaceError::new(
                ErrorCode::IntegrityHashMismatch,
                "stored bytes do not match chunk_id",
            ));
        }
        Ok(obj.slice(offset as usize..end as usize))
    }

    async fn head(&self, id: &ChunkId) -> Result<u64, SpaceError> {
        let g = self.inner.read().await;
        g.get(id)
            .map(|o| o.len() as u64)
            .ok_or_else(|| SpaceError::new(ErrorCode::ChunkNotFound, "no such chunk"))
    }

    async fn exists(&self, id: &ChunkId) -> Result<bool, SpaceError> {
        Ok(self.inner.read().await.contains_key(id))
    }
}

#[async_trait]
impl ObjectStoreAdmin for FakeObjectStore {
    async fn delete(&self, id: &ChunkId) -> Result<(), SpaceError> {
        self.inner.write().await.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_store_passes_the_conformance_suite() {
        let store = FakeObjectStore::new();
        conformance::run_conformance_suite(&store).await;
    }

    #[tokio::test]
    async fn corrupted_store_is_caught_on_read() {
        let store = FakeObjectStore::new();
        let bytes = b"conformance corruption case".to_vec();
        let id = ChunkId::from_bytes(&bytes);
        store.put(&id, &bytes).await.unwrap();
        store.corrupt_for_test(&id, 3).await;
        let err = store.get(&id).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::IntegrityHashMismatch);
    }

    #[tokio::test]
    async fn admin_trait_is_separate() {
        // Client code that only imports ObjectStore cannot call delete.
        fn only_reads(_s: &dyn ObjectStore) {}
        let store = FakeObjectStore::new();
        only_reads(&store);
    }
}
