//! Deterministic test-data generators (M0.10).
//!
//! Everything here is a pure function of a `u64` seed: the same seed always
//! produces the same bytes and therefore the same [`ChunkId`]s. Generated data
//! never contains a zero byte -- zero-filled data hides offset bugs.
//!
//! Build the streaming variant now: the Phase 11 500 GB fixture calls
//! [`generate_file_streaming`] and must never need the whole file in memory.

#![forbid(unsafe_code)]

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use contracts::{Chunk, ChunkId, DirectoryId, FileId, Manifest, ManifestEntry, ManifestId};

#[inline]
fn nz(b: u8) -> u8 {
    if b == 0 {
        0xAA
    } else {
        b
    }
}

const STREAM_BLOCK: usize = 8192;

/// A deterministic non-zero byte stream. Output depends only on the seed, never
/// on how callers slice their reads: the underlying RNG's `fill_bytes` is *not*
/// chunk-invariant, so we always pull from it in fixed [`STREAM_BLOCK`] units
/// and buffer the remainder ourselves.
struct ByteStream {
    rng: ChaCha8Rng,
    block: Box<[u8; STREAM_BLOCK]>,
    used: usize,
}

impl ByteStream {
    fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            block: Box::new([0; STREAM_BLOCK]),
            used: STREAM_BLOCK,
        }
    }

    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            if self.used == STREAM_BLOCK {
                self.rng.fill_bytes(self.block.as_mut_slice());
                for b in self.block.iter_mut() {
                    *b = nz(*b);
                }
                self.used = 0;
            }
            let n = (buf.len() - i).min(STREAM_BLOCK - self.used);
            buf[i..i + n].copy_from_slice(&self.block[self.used..self.used + n]);
            self.used += n;
            i += n;
        }
    }
}

/// Default chunk size (ADR-0003): 32 MiB.
pub const CHUNK: u64 = 32 * 1024 * 1024;

/// Sizes every chunk-boundary test must iterate, at the real 32 MiB chunk size.
pub const BOUNDARY_SIZES: &[u64] = &[
    0,
    1,
    4095,
    4096,
    CHUNK - 1,
    CHUNK,
    CHUNK + 1,
    3 * CHUNK,
    3 * CHUNK + 1,
];

/// The same boundary pattern relative to an arbitrary `chunk_size`. E2E tests
/// use a small chunk size so the full contract chain stays fast while still
/// crossing every boundary.
pub fn boundary_sizes_for(chunk_size: u64) -> Vec<u64> {
    vec![
        0,
        1,
        4095.min(chunk_size.saturating_sub(1)),
        4096.min(chunk_size),
        chunk_size - 1,
        chunk_size,
        chunk_size + 1,
        3 * chunk_size,
        3 * chunk_size + 1,
    ]
}

/// `size` deterministic non-zero bytes.
pub fn generate_file(size: u64, seed: u64) -> Vec<u8> {
    let mut out = vec![0u8; size as usize];
    ByteStream::new(seed).fill(&mut out);
    out
}

/// Stream `size` deterministic bytes through `sink` in `block`-sized pieces
/// without ever holding the whole file. Identical output to [`generate_file`]
/// for the same seed.
pub fn generate_file_streaming(size: u64, seed: u64, block: usize, mut sink: impl FnMut(&[u8])) {
    assert!(block > 0, "block size must be positive");
    let mut stream = ByteStream::new(seed);
    let mut remaining = size;
    let mut buf = vec![0u8; block];
    while remaining > 0 {
        let n = remaining.min(block as u64) as usize;
        stream.fill(&mut buf[..n]);
        sink(&buf[..n]);
        remaining -= n as u64;
    }
}

/// A single chunk and its content address.
pub fn generate_chunk(size: u64, seed: u64) -> (ChunkId, Vec<u8>) {
    let bytes = generate_file(size, seed);
    (ChunkId::from_bytes(&bytes), bytes)
}

/// How many entries a file of `file_size` splits into at `chunk_size`.
pub fn chunk_count(file_size: u64, chunk_size: u64) -> u64 {
    assert!(chunk_size > 0);
    file_size.div_ceil(chunk_size)
}

/// A complete, valid manifest plus every chunk body it references.
///
/// Each entry is one full chunk (the last may be shorter). Chunk offset is
/// always 0, so this exercises the common layout.
pub fn generate_manifest(
    file_size: u64,
    chunk_size: u64,
    seed: u64,
) -> (Manifest, Vec<(Chunk, Vec<u8>)>) {
    assert!(chunk_size > 0);
    let mut stream = ByteStream::new(seed);
    let n = chunk_count(file_size, chunk_size);

    let mut entries = Vec::new();
    let mut bodies: Vec<(Chunk, Vec<u8>)> = Vec::new();
    let mut chunks = Vec::new();
    let mut offset = 0u64;

    for _ in 0..n {
        let len = (file_size - offset).min(chunk_size);
        let mut body = vec![0u8; len as usize];
        stream.fill(&mut body);
        let id = ChunkId::from_bytes(&body);
        entries.push(ManifestEntry {
            logical_offset: offset,
            length: len,
            chunk_id: id.clone(),
            chunk_offset: 0,
        });
        let chunk = Chunk {
            chunk_id: id,
            size: len,
        };
        if !chunks.iter().any(|c: &Chunk| c.chunk_id == chunk.chunk_id) {
            chunks.push(chunk.clone());
        }
        bodies.push((chunk, body));
        offset += len;
    }

    let manifest = Manifest {
        manifest_id: ManifestId::new(),
        total_size: file_size,
        chunk_count: entries.len() as u64,
        entries,
        chunks,
    };
    (manifest, bodies)
}

/// A synthetic directory tree of `File` metadata.
pub fn generate_tree(depth: u32, breadth: u32, seed: u64) -> Vec<contracts::File> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let now = chrono::Utc::now();
    let mut files = Vec::new();
    let mut frontier = vec![DirectoryId::new()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for parent in &frontier {
            for i in 0..breadth {
                let file = contracts::File {
                    file_id: FileId::new(),
                    parent_id: *parent,
                    name: format!("n{}_{}", i, rng.next_u32() % 1000),
                    current_version_id: None,
                    created_at: now,
                    modified_at: now,
                };
                files.push(file);
                next.push(DirectoryId::new());
            }
        }
        frontier = next;
    }
    files
}

/// Wrap a seeded test so a failure prints the seed that reproduces it.
pub fn with_seed<F: FnOnce(u64)>(seed: u64, f: F) {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("FAILING SEED = {seed}");
        hook(info);
    }));
    f(seed);
    let _ = std::panic::take_hook();
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::validate::validate_manifest;

    #[test]
    fn same_seed_same_bytes_and_ids() {
        let a = generate_file(10_000, 42);
        let b = generate_file(10_000, 42);
        assert_eq!(a, b);
        assert_eq!(ChunkId::from_bytes(&a), ChunkId::from_bytes(&b));
        assert_ne!(generate_file(10_000, 43), a);
    }

    #[test]
    fn never_emits_a_zero_byte() {
        assert!(!generate_file(50_000, 7).contains(&0));
    }

    #[test]
    fn streaming_matches_in_memory_for_the_same_seed() {
        for &block in &[1usize, 7, 4096, 1 << 20] {
            let mut streamed = Vec::new();
            generate_file_streaming(100_003, 99, block, |b| streamed.extend_from_slice(b));
            assert_eq!(streamed, generate_file(100_003, 99), "block={block}");
        }
    }

    #[test]
    fn each_boundary_size_produces_the_expected_chunk_count() {
        for &size in BOUNDARY_SIZES {
            let expect = if size == 0 { 0 } else { size.div_ceil(CHUNK) };
            assert_eq!(chunk_count(size, CHUNK), expect, "size={size}");
        }
    }

    #[test]
    fn generated_manifests_are_valid_across_boundary_sizes() {
        // small chunk size keeps the test fast while still crossing boundaries
        let cs = 4096;
        for &size in &[0u64, 1, 4095, 4096, 4097, 3 * 4096, 3 * 4096 + 1] {
            let (m, bodies) = generate_manifest(size, cs, 1);
            validate_manifest(&m).unwrap_or_else(|e| panic!("size {size}: {e}"));
            let total: u64 = bodies.iter().map(|(c, _)| c.size).sum();
            assert_eq!(total, size);
            for (c, body) in &bodies {
                assert!(c.chunk_id.verify(body));
            }
        }
    }

    fn assert_streams_without_accumulating(size: u64) {
        let block = 8usize << 20;
        let mut max_seen = 0usize;
        let mut total = 0u64;
        generate_file_streaming(size, 5, block, |b| {
            max_seen = max_seen.max(b.len());
            total += b.len() as u64;
        });
        assert_eq!(total, size);
        assert!(max_seen <= block, "sink never sees more than one block");
    }

    #[test]
    fn streaming_does_not_accumulate() {
        // 128 MiB is enough to prove the sink is bounded and no full buffer is
        // built. The literal 1 GiB spec case is `streaming_full_gib` below,
        // #[ignore]d because a debug-build ChaCha8 pass over 1 GiB is slow.
        assert_streams_without_accumulating(128 << 20);
    }

    #[test]
    #[ignore = "slow in debug; run with --run-ignored or in release"]
    fn streaming_full_gib() {
        assert_streams_without_accumulating(1 << 30);
    }

    #[test]
    fn with_seed_restores_the_panic_hook() {
        with_seed(123, |s| assert_eq!(s, 123));
    }

    #[test]
    fn generate_tree_shapes() {
        let files = generate_tree(3, 2, 1);
        assert_eq!(files.len(), 2 + 4 + 8);
    }
}
