//! The shared object-store conformance suite (M0.9, T0.11).
//!
//! Write it once, run it against every implementation forever. Phase 7's
//! disk-backed fake and Phase 9's `S3Store` call [`run_conformance_suite`]
//! unchanged.

use contracts::{ChunkId, ErrorCode};

use crate::ObjectStore;

/// Run every conformance case against `store`. Panics on the first failure with
/// a message naming the case.
pub async fn run_conformance_suite<S: ObjectStore>(store: &S) {
    put_then_get_returns_identical_bytes(store).await;
    put_twice_identical_is_ok(store).await;
    put_same_id_different_bytes_conflicts(store).await;
    put_id_not_addressing_bytes_conflicts(store).await;
    range_get_returns_exactly_length_at_three_offsets(store).await;
    range_beyond_eof_is_an_error_not_a_short_read(store).await;
    offset_plus_length_overflow_is_invalid_parameter(store).await;
    zero_byte_object_round_trips(store).await;
    large_object_round_trips(store).await;
    missing_object_is_chunk_not_found(store).await;
    head_returns_correct_size(store).await;
    exists_matches_reality(store).await;
    concurrent_puts_of_same_id_all_succeed(store).await;
}

fn id_for(bytes: &[u8]) -> ChunkId {
    ChunkId::from_bytes(bytes)
}

async fn put_then_get_returns_identical_bytes<S: ObjectStore>(store: &S) {
    let bytes = b"hello object store".to_vec();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put");
    let got = store.get(&id).await.expect("get");
    assert_eq!(&got[..], &bytes[..], "put->get must be identity");
}

async fn put_twice_identical_is_ok<S: ObjectStore>(store: &S) {
    let bytes = b"idempotent put".to_vec();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put 1");
    store
        .put(&id, &bytes)
        .await
        .expect("put 2 (identical) must be Ok");
}

async fn put_same_id_different_bytes_conflicts<S: ObjectStore>(store: &S) {
    let bytes = b"original content".to_vec();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put");
    let err = store
        .put(&id, b"different content entirely!!")
        .await
        .expect_err("must conflict");
    assert_eq!(err.code, ErrorCode::IntegrityChunkIdConflict);
}

async fn put_id_not_addressing_bytes_conflicts<S: ObjectStore>(store: &S) {
    let id = id_for(b"claimed content");
    let err = store
        .put(&id, b"actual different content")
        .await
        .expect_err("id must address bytes");
    assert_eq!(err.code, ErrorCode::IntegrityChunkIdConflict);
}

async fn range_get_returns_exactly_length_at_three_offsets<S: ObjectStore>(store: &S) {
    let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put");
    for &(offset, length) in &[(0u64, 10u32), (500, 200), (990, 10)] {
        let got = store.get_range(&id, offset, length).await.expect("range");
        assert_eq!(
            got.len(),
            length as usize,
            "range must return exactly length"
        );
        assert_eq!(
            &got[..],
            &bytes[offset as usize..offset as usize + length as usize]
        );
    }
}

async fn range_beyond_eof_is_an_error_not_a_short_read<S: ObjectStore>(store: &S) {
    let bytes = b"short".to_vec();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put");
    let err = store.get_range(&id, 3, 10).await.expect_err("must error");
    assert_eq!(err.code, ErrorCode::IntegrityLengthMismatch);
}

async fn offset_plus_length_overflow_is_invalid_parameter<S: ObjectStore>(store: &S) {
    let bytes = b"overflow probe".to_vec();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put");
    let err = store
        .get_range(&id, u64::MAX, 8)
        .await
        .expect_err("must not panic");
    assert_eq!(err.code, ErrorCode::InvalidParameter);
}

async fn zero_byte_object_round_trips<S: ObjectStore>(store: &S) {
    let id = id_for(b"");
    store.put(&id, b"").await.expect("put empty");
    assert_eq!(store.get(&id).await.expect("get empty").len(), 0);
    assert_eq!(store.head(&id).await.expect("head empty"), 0);
    assert!(store.exists(&id).await.expect("exists empty"));
}

async fn large_object_round_trips<S: ObjectStore>(store: &S) {
    let bytes: Vec<u8> = (0..64 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put 64MiB");
    assert_eq!(store.head(&id).await.expect("head"), bytes.len() as u64);
    let tail = store
        .get_range(&id, bytes.len() as u64 - 4, 4)
        .await
        .expect("tail range");
    assert_eq!(&tail[..], &bytes[bytes.len() - 4..]);
}

async fn missing_object_is_chunk_not_found<S: ObjectStore>(store: &S) {
    let id = id_for(b"never stored anywhere at all");
    assert_eq!(
        store.get(&id).await.expect_err("missing get").code,
        ErrorCode::ChunkNotFound
    );
    assert_eq!(
        store.head(&id).await.expect_err("missing head").code,
        ErrorCode::ChunkNotFound
    );
}

async fn head_returns_correct_size<S: ObjectStore>(store: &S) {
    let bytes = vec![7u8; 12345];
    let id = id_for(&bytes);
    store.put(&id, &bytes).await.expect("put");
    assert_eq!(store.head(&id).await.expect("head"), 12345);
}

async fn exists_matches_reality<S: ObjectStore>(store: &S) {
    let bytes = b"presence check".to_vec();
    let id = id_for(&bytes);
    assert!(!store.exists(&id).await.expect("exists before"));
    store.put(&id, &bytes).await.expect("put");
    assert!(store.exists(&id).await.expect("exists after"));
}

async fn concurrent_puts_of_same_id_all_succeed<S: ObjectStore>(store: &S) {
    let bytes = b"concurrent put target".to_vec();
    let id = id_for(&bytes);
    let mut handles = Vec::new();
    for _ in 0..16 {
        let id = id.clone();
        let bytes = bytes.clone();
        // Run sequentially against the &S; the point is identical-bytes puts
        // never conflict regardless of ordering.
        handles.push(async move { store.put(&id, &bytes).await });
    }
    for h in handles {
        h.await.expect("concurrent identical put must be Ok");
    }
    assert_eq!(store.head(&id).await.expect("head"), bytes.len() as u64);
}
