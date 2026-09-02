//! M0.14 -- the contract chain, happy path.
//!
//! create File -> create Version(Candidate) -> generate content -> chunk ->
//! PUT chunks -> build Manifest -> validate 13 invariants -> commit Version ->
//! read back via chunk ranges -> reassemble -> assert byte-identical and every
//! ChunkId re-derives.

mod common;

use common::{InProcCloud, E2E_CHUNK};
use contracts::{DirectoryId, VersionId};
use space_generators::{boundary_sizes_for, generate_manifest};

async fn run_chain_for_size(cloud: &InProcCloud, size: u64, seed: u64) {
    let client = &cloud.client;
    let dir = DirectoryId::new();
    let file = client
        .create_file(dir, &format!("f{size}.bin"), &format!("key-{size}-{seed}"))
        .await
        .expect("create file");

    let (manifest, bodies) = generate_manifest(size, E2E_CHUNK, seed);

    for (chunk, body) in &bodies {
        let ack = client
            .register_and_put_chunk(&chunk.chunk_id, body)
            .await
            .expect("put chunk");
        assert_eq!(ack.chunk.size, body.len() as u64);
        // idempotent re-PUT
        client
            .register_and_put_chunk(&chunk.chunk_id, body)
            .await
            .expect("re-put chunk is idempotent");
    }

    client.put_manifest(&manifest).await.expect("put manifest");

    let vid = VersionId::new();
    let version = client
        .create_version(file.file_id, vid, None, manifest.manifest_id)
        .await
        .expect("create version");
    assert_eq!(
        format!("{:?}", version.state),
        "Candidate",
        "new version starts as Candidate"
    );

    let committed = client.commit_version(vid).await.expect("commit");
    assert_eq!(format!("{:?}", committed.state), "Committed");

    let reassembled = client.reassemble(&manifest).await.expect("reassemble");
    let expected: Vec<u8> = bodies.iter().flat_map(|(_, b)| b.clone()).collect();
    assert_eq!(
        reassembled, expected,
        "size {size}: bytes must be identical"
    );
    assert_eq!(reassembled.len() as u64, size);

    let stat = client.stat_file(file.file_id).await.expect("stat");
    assert_eq!(
        stat.current_version_id,
        Some(vid),
        "size {size}: file points at the committed version"
    );
}

#[tokio::test]
async fn happy_chain_across_boundary_sizes() {
    let cloud = InProcCloud::start().await;
    for (i, size) in boundary_sizes_for(E2E_CHUNK).into_iter().enumerate() {
        run_chain_for_size(&cloud, size, 1000 + i as u64).await;
    }
}

#[tokio::test]
async fn happy_chain_over_a_real_cloud_process() {
    // The same chain, but talking to a spawned `space-cloud` binary, to prove
    // the process path (harness M0.13) end to end.
    let env = space_test_harness::TestEnv::start("e2e-happy-proc").await;
    let client = space_client_core::CloudClient::new(&env.cloud_url);
    let dir = DirectoryId::new();
    let file = client
        .create_file(dir, "proc.bin", "proc-key")
        .await
        .unwrap();
    let (manifest, bodies) = generate_manifest(3 * E2E_CHUNK + 7, E2E_CHUNK, 42);
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
    let got = client.reassemble(&manifest).await.unwrap();
    let expected: Vec<u8> = bodies.iter().flat_map(|(_, b)| b.clone()).collect();
    assert_eq!(got, expected);
}
