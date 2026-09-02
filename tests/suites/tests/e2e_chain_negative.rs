//! M0.14 -- the contract chain, negative paths.
//!
//! Each case injects one fault, asserts the exact error code, and then re-reads
//! the previously-committed good version to prove committed state survived the
//! failure. That re-read is the executable form of guard rails #3 and #4.

mod common;

use common::{InProcCloud, E2E_CHUNK};
use contracts::{DirectoryId, ErrorCode, VersionId};
use objectstore::ObjectStoreAdmin;
use space_corruptor as corrupt;
use space_generators::generate_manifest;

/// A committed 3-chunk file we can keep re-reading between fault injections.
struct GoodVersion {
    manifest: contracts::Manifest,
    bytes: Vec<u8>,
}

async fn establish_good(cloud: &InProcCloud) -> GoodVersion {
    let client = &cloud.client;
    let file = client
        .create_file(DirectoryId::new(), "good.bin", "good-key")
        .await
        .unwrap();
    let (manifest, bodies) = generate_manifest(3 * E2E_CHUNK, E2E_CHUNK, 7);
    for (chunk, body) in &bodies {
        client
            .register_and_put_chunk(&chunk.chunk_id, body)
            .await
            .unwrap();
    }
    client.put_manifest(&manifest).await.unwrap();
    let vid = VersionId::new();
    client
        .create_version(file.file_id, vid, None, manifest.manifest_id)
        .await
        .unwrap();
    client.commit_version(vid).await.unwrap();
    let bytes: Vec<u8> = bodies.iter().flat_map(|(_, b)| b.clone()).collect();
    GoodVersion { manifest, bytes }
}

async fn assert_good_still_readable(cloud: &InProcCloud, good: &GoodVersion) {
    let got = cloud
        .client
        .reassemble(&good.manifest)
        .await
        .expect("committed good version must still read back");
    assert_eq!(got, good.bytes, "a failed op damaged committed state");
}

#[tokio::test]
async fn case_1_flipped_byte_in_stored_chunk() {
    let cloud = InProcCloud::start().await;
    let good = establish_good(&cloud).await;

    let (manifest, bodies) = generate_manifest(2 * E2E_CHUNK, E2E_CHUNK, 11);
    for (c, b) in &bodies {
        cloud
            .client
            .register_and_put_chunk(&c.chunk_id, b)
            .await
            .unwrap();
    }
    cloud
        .state
        .blobs
        .corrupt_for_test(&bodies[0].0.chunk_id, 5)
        .await;
    let err = cloud.client.reassemble(&manifest).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::IntegrityHashMismatch);

    assert_good_still_readable(&cloud, &good).await;
}

#[tokio::test]
async fn case_2_truncated_stored_chunk() {
    let cloud = InProcCloud::start().await;
    let good = establish_good(&cloud).await;

    let (chunk_id, body) = space_generators::generate_chunk(E2E_CHUNK, 21);
    cloud
        .client
        .register_and_put_chunk(&chunk_id, &body)
        .await
        .unwrap();
    cloud
        .state
        .blobs
        .truncate_for_test(&chunk_id, body.len() - 100)
        .await;

    let err = cloud
        .client
        .get_chunk_range(&chunk_id, 0, body.len() as u64)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::IntegrityLengthMismatch);

    assert_good_still_readable(&cloud, &good).await;
}

#[tokio::test]
async fn case_3_missing_chunk_the_manifest_references() {
    let cloud = InProcCloud::start().await;
    let good = establish_good(&cloud).await;

    let (manifest, bodies) = generate_manifest(2 * E2E_CHUNK, E2E_CHUNK, 31);
    for (c, b) in &bodies {
        cloud
            .client
            .register_and_put_chunk(&c.chunk_id, b)
            .await
            .unwrap();
    }
    // reach in and delete one chunk body (admin-only, not on the client path)
    cloud
        .state
        .blobs
        .delete(&bodies[1].0.chunk_id)
        .await
        .unwrap();

    let err = cloud.client.reassemble(&manifest).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ChunkNotFound);

    assert_good_still_readable(&cloud, &good).await;
}

async fn expect_manifest_rejected(mutate: impl Fn(&mut contracts::Manifest)) {
    let cloud = InProcCloud::start().await;
    let good = establish_good(&cloud).await;

    let (mut manifest, bodies) = generate_manifest(3 * E2E_CHUNK, E2E_CHUNK, 41);
    for (c, b) in &bodies {
        cloud
            .client
            .register_and_put_chunk(&c.chunk_id, b)
            .await
            .unwrap();
    }
    mutate(&mut manifest);
    let err = cloud.client.put_manifest(&manifest).await.unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::IntegrityManifestInvalid,
        "{}",
        err.message
    );

    // and no version can be built on a manifest that was never accepted
    let file = cloud
        .client
        .create_file(DirectoryId::new(), "x", "x")
        .await
        .unwrap();
    let bad = cloud
        .client
        .create_version(file.file_id, VersionId::new(), None, manifest.manifest_id)
        .await
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::ManifestNotFound);

    assert_good_still_readable(&cloud, &good).await;
}

#[tokio::test]
async fn case_4_manifest_with_a_gap() {
    expect_manifest_rejected(|m| corrupt::introduce_gap(m, 0)).await;
}

#[tokio::test]
async fn case_5_manifest_with_an_overlap() {
    expect_manifest_rejected(|m| corrupt::introduce_overlap(m, 0)).await;
}

#[tokio::test]
async fn case_6_total_size_off_by_one() {
    expect_manifest_rejected(|m| corrupt::alter_total_size(m, 1)).await;
}

#[tokio::test]
async fn case_7_chunk_count_off_by_one() {
    expect_manifest_rejected(|m| corrupt::alter_chunk_count(m, 1)).await;
}

#[tokio::test]
async fn case_8_invalid_manifest_never_becomes_committable() {
    // Our stack validates at PUT time, so a version can never be built on an
    // unvalidated manifest -- strictly stronger than "reject at commit".
    expect_manifest_rejected(|m| corrupt::swap_entries(m, 0, 2)).await;
}

#[tokio::test]
async fn case_9_candidate_version_is_invisible_to_readers() {
    let cloud = InProcCloud::start().await;
    let good = establish_good(&cloud).await;

    let (manifest, bodies) = generate_manifest(E2E_CHUNK, E2E_CHUNK, 51);
    for (c, b) in &bodies {
        cloud
            .client
            .register_and_put_chunk(&c.chunk_id, b)
            .await
            .unwrap();
    }
    cloud.client.put_manifest(&manifest).await.unwrap();
    let file = cloud
        .client
        .create_file(DirectoryId::new(), "cand.bin", "cand")
        .await
        .unwrap();
    let vid = VersionId::new();
    cloud
        .client
        .create_version(file.file_id, vid, None, manifest.manifest_id)
        .await
        .unwrap();
    // not committed -> file has no current version
    let stat = cloud.client.stat_file(file.file_id).await.unwrap();
    assert_eq!(stat.current_version_id, None);

    assert_good_still_readable(&cloud, &good).await;
}

#[tokio::test]
async fn case_10_put_existing_chunk_id_with_different_bytes() {
    let cloud = InProcCloud::start().await;
    let good = establish_good(&cloud).await;

    let (id, body) = space_generators::generate_chunk(4096, 61);
    cloud
        .client
        .register_and_put_chunk(&id, &body)
        .await
        .unwrap();
    let mut different = body.clone();
    different[0] ^= 0xFF;
    let err = cloud
        .client
        .register_and_put_chunk(&id, &different)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::IntegrityChunkIdConflict);

    assert_good_still_readable(&cloud, &good).await;
}

#[tokio::test]
async fn case_11_commit_the_same_version_twice() {
    let cloud = InProcCloud::start().await;
    let _good = establish_good(&cloud).await;

    let (manifest, bodies) = generate_manifest(E2E_CHUNK + 1, E2E_CHUNK, 71);
    for (c, b) in &bodies {
        cloud
            .client
            .register_and_put_chunk(&c.chunk_id, b)
            .await
            .unwrap();
    }
    cloud.client.put_manifest(&manifest).await.unwrap();
    let file = cloud
        .client
        .create_file(DirectoryId::new(), "twice.bin", "twice")
        .await
        .unwrap();
    let vid = VersionId::new();
    cloud
        .client
        .create_version(file.file_id, vid, None, manifest.manifest_id)
        .await
        .unwrap();
    let a = cloud.client.commit_version(vid).await.unwrap();
    let b = cloud.client.commit_version(vid).await.unwrap();
    assert_eq!(a.version_id, b.version_id);
    assert_eq!(format!("{:?}", b.state), "Committed");
}
