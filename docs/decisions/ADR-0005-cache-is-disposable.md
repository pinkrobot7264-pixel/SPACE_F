# ADR-0005: The cache is disposable

- Status: Accepted
- Date: 2026-09-02

## Context

The client keeps a byte cache and a chunk cache to avoid re-fetching. Caches are
a frequent source of subtle corruption bugs when they are treated as a source of
truth.

## Decision

The cache is disposable and rebuildable from authoritative data. Concretely:

- A cache entry that fails verification (BLAKE3 mismatch, wrong length, stale
  index) is **deleted and reconstructed**, never repaired in place and never
  served (guard rail #2).
- Deleting the entire cache directory is always safe: it costs performance, not
  correctness. `C:\SPACE\runtime\cache\` is not in Git and can be removed at any
  time.
- The cache has hard size bounds (`config.cache.max_bytes`) and a defined
  eviction policy (`lru` | `slru`). Under disk pressure it evicts or returns a
  controlled error; it never corrupts durable state.

## Consequences

- Phase 3 and Phase 4 build the caches with verification-on-read and
  rebuild-on-failure from the start.
- Recovery tests may wipe the cache between steps and still expect correct reads.

## Revisit when

A cache-warming or offline-availability feature needs cache contents to survive
as semi-authoritative -- which would need its own durability treatment, not a
relaxation of this rule.
