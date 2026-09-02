# Fault-point registry (M0.6)

A *fault point* is a named location in real code where the crash, concurrency
and fault-injection suites (Phase 3 onward) can ask the process to misbehave.
The names are registered here **before** the code they live in exists, so later
phases have a stable vocabulary.

- Implementation: `faults` crate. `faults::fault_point(name) -> FaultAction`.
- In a release build `fault_point` is `#[inline(always)]` and always returns
  `FaultAction::None`. The armable registry, `arm` and `disarm_all` only exist
  under the `fault-injection` Cargo feature, which is **dev-only**. CI asserts
  the feature is off in release
  (`.github/workflows/ci.yml`, "Release build must not enable fault injection").
- `FaultAction`: `None` · `Panic` · `Delay(Duration)` · `Error(ErrorCode)` ·
  `CorruptBytes`.
- `faults::FAULT_POINTS` is the authoritative list; `arm` panics on an unknown
  name. Keep this table and that slice in sync.

| Name | Fires in (future phase) | Purpose |
|---|---|---|
| `post_wal_write` | Phase 5 write path | crash after the WAL record is durable, before chunk write |
| `pre_chunk_publish` | Phase 3 chunk store | crash/stall just before a staged chunk is atomically published |
| `post_chunk_publish` | Phase 3 chunk store | crash after publish, before the index records it |
| `pre_manifest_build` | Phase 6 commit | fail before a manifest is assembled |
| `pre_manifest_commit` | Phase 6 commit | crash after the manifest validates, before the version flips to Committed |
| `post_manifest_commit` | Phase 6 commit | crash after commit, before the caller is acknowledged |
| `mid_upload` | Phase 7 transfer | drop/stall the connection partway through a chunk PUT |
| `mid_download` | Phase 4 / Phase 7 transfer | truncate/stall a range GET |
| `pre_cache_write` | Phase 4 byte cache | fail before a cache entry is written |
| `post_cache_write` | Phase 4 byte cache | crash after the cache write, before the cache index update |
| `pre_db_commit` | Phase 8 backend | roll back before the metadata transaction commits |
| `post_db_commit` | Phase 8 backend | crash after DB commit, before the object store is updated |

The `faults` unit test asserts this list has 12 unique entries.
