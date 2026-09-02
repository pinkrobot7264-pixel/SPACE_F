# ADR-0003: 32 MiB default chunk size, recorded per-manifest

- Status: Accepted
- Date: 2026-09-02

## Context

Chunk size trades off: larger chunks mean fewer index entries, fewer requests
and better throughput, but coarser deduplication and more wasted transfer on a
small random read. The MVP targets 500 GB logical files read in small ranges.

## Decision

- Default chunk size is **32 MiB** (`config.chunking.chunk_size_bytes`,
  `space_generators::CHUNK`).
- The size in force when a manifest was built is implied by that manifest's
  entries; a manifest is self-describing (`total_size`, per-entry `length`,
  per-entry `chunk_offset`). Nothing assumes a global constant at read time.
- This is an initial value to be benchmarked in Phase 3 and Phase 11, not a
  final one.

## Consequences

- A 500 GB file is ~16k chunks -- a tractable manifest.
- A 4 MiB random read touches one or two chunks; acceptable for the MVP, to be
  measured.
- Because size is per-manifest, a future size change does not invalidate old
  versions.

## Revisit when

Phase 3 or Phase 11 benchmarks show read amplification or manifest size is a
bottleneck.
