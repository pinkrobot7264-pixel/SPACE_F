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

---

# Phase 1 additions (manual §13.1)

Phase 1 registers eight points at the WinFsp boundary. They are consulted inside
`guard()` (`client/core/src/ffi/mod.rs`), which is inside `catch_unwind`, so the
`Panic` action travels the same path a real panic does — otherwise the ADR-0013
row would be testing the harness rather than the guard.

| Name | Fires in | Purpose |
|---|---|---|
| `winfsp_pre_read` | `space_core_read` | the primary hang/timeout target (§13.2) |
| `winfsp_pre_write` | `space_core_write` | resource and cancellation faults on a mutating path |
| `winfsp_pre_open` | `space_core_open` | failure before a handle exists |
| `winfsp_pre_create` | `space_core_create` | failure during namespace mutation |
| `winfsp_pre_readdir` | `space_core_dir_open` | enumeration under fault |
| `winfsp_pre_getinfo` | `space_core_get_file_info` | the cheapest callback to hang |
| `winfsp_pre_rename` | `space_core_rename` | failure mid-namespace-move |
| `winfsp_pre_cleanup` | `space_core_cleanup` | failure on the unlink path |

## The Phase 1 action set

`None` · `Fail(ErrorCode)` · `Delay(Duration)` · `Hang` · `Cancel` ·
`InvalidInput` · `StaleHandle` · `ResourceExhausted` · `Panic`

Each targets something specific, which is why they are distinct variants rather
than one generic "fail":

| Action | Target | Expected |
|---|---|---|
| `StaleHandle` | INV-ID-3 | `InvalidHandle`; no state change; `check_invariants` passes |
| `ResourceExhausted` | INV-RES-2/3 | `ResourceExhausted`; **state byte-identical**; invariants pass |
| `InvalidInput` | INV-NS-6, INV-RES-2 | a specific `ErrorCode`, never a panic |
| `Cancel` | ADR-0009 | `Cancelled` → `STATUS_CANCELLED`; no partial mutation |
| `Hang` | ADR-0009, L9 | returns `STATUS_IO_TIMEOUT` within `callback_timeout_ms` + 500ms |
| `Delay` | §13.4 | a delay *under* the deadline must still **succeed** |
| `Panic` | ADR-0013, ADR-0013a | `STATUS_INTERNAL_ERROR`, logged with `request_id`, poisons, then main-thread unmount |

## `CorruptBytes` is excluded from Phase 1

The variant exists — Phase 3 needs it — but **`arm` refuses it**, with a test.

Phase 1 has no integrity mechanism: content lives in a `Vec<u8>` in-process,
with no checksum, no authoritative second copy, and no cache to reconstruct
from. Flipping a byte would produce a test whose expected result is "the flipped
byte comes back", which asserts nothing — while sitting in the exit gate looking
like integrity coverage.

It returns in **Phase 3**, injected between chunk-store read and content
verification, expecting an integrity error.

## What can and cannot be tested in-process

`client/core/src/ffi/fault_tests.rs` covers the action semantics, the deadline
bound, and that the bound **scales with `callback_timeout_ms`** — which is what
proves the deadline is real rather than an artefact of fast operations.

The other half of §13.2 is a claim about *Windows*: Explorer stays responsive,
unrelated operations return `OperationTimeout` rather than hanging, unmount
still succeeds, and `os-safety-check.ps1` is clean. Those cannot be asserted
from inside the process and are driven through a real mount by
`scripts/fault-injection-test.ps1`.
