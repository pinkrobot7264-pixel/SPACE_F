# ADR-0002: Chunk identity is the BLAKE3 hash of its bytes

- Status: Accepted
- Date: 2026-09-02

## Context

Chunks are immutable. We need a chunk identifier that (a) lets any party verify
a chunk's bytes without a trusted index, (b) gives free deduplication, and (c)
is cheap to compute on the read and write paths.

## Decision

A chunk's id **is** its content hash: `b3:<64 lowercase hex>` where the hash is
BLAKE3 of the exact chunk bytes (`contracts::ChunkId`). There is no separate
`hash` field anywhere in the model.

- `ChunkId::from_bytes` computes it; `ChunkId::verify` checks it.
- The object store refuses a `put` whose id does not address the bytes, and
  refuses a second `put` of different bytes under an existing id
  (`INTEGRITY_CHUNK_ID_CONFLICT`).
- The read path verifies every whole chunk it reassembles; corrupted bytes are
  never returned as valid (`INTEGRITY_HASH_MISMATCH`), satisfying guard rail #3.

BLAKE3 over SHA-256: faster, parallel/SIMD-friendly, 256-bit output, and its
tree structure leaves room for future verified streaming.

## Consequences

- `hash_algorithm` in config accepts only `"blake3"`.
- Migrating hash algorithms later means rewriting every chunk id -- treated as a
  major format change, not a runtime toggle.

## Revisit when

A verified-streaming or Merkle-proof requirement appears, or a platform without a
fast BLAKE3 implementation must be supported.
