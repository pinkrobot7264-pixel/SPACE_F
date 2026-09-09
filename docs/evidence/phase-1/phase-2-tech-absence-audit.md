# §1.6 audit — Phase-2+ technology absent from the Phase 1 filesystem path

Commit audited: `abaf369` (branch `phase/1-winfsp`).
Method: dependency-graph audit plus reachability analysis from the FFI entry
points, not a filename scan.

## 1. Prohibited dependencies — none present

`Cargo.lock`, whole workspace:

| crate family | occurrences |
|---|---:|
| `aws-sdk`, `aws-config`, `rusoto` | 0 |
| `s3` | 0 |
| `postgres`, `tokio-postgres`, `sqlx`, `diesel` | 0 |
| `etcd`, `consul`, `zookeeper` (distributed locking) | 0 |

No object store, no relational database, no distributed coordination service is
linked into the workspace at all.

## 2. Source scan of the client crates

Scanned `client/` and `contracts/` for: `aws`, `s3::`, `postgres`, `sqlx`,
`rusoto`, `reqwest`, `manifest`, `chunk_store`, `distributed`, `byte_cache`.

Two hits needed investigation rather than dismissal.

### `client/core/src/ffi/ntstatus.rs` — not a violation

Mentions `ManifestNotFound`, `ChunkNotFound`, `IntegrityManifestInvalid`. These
are **error-code identifiers**, not manifest logic. ADR-0015 requires the
NTSTATUS mapping to be **total** over every `ErrorCode` variant, including the
Phase 0 codes the shared `contracts` crate defines. Removing them would break
totality, which is asserted by
`every_error_code_has_a_mapping_unless_it_is_startup_only`.

### `client/core/src/lib.rs` — present but unreachable

Contains `CloudClient`: a `reqwest`-based HTTP client with `put_manifest()`,
`/v1/manifests/{id}` routes, and ranged chunk reads via the `RANGE` header.
This is Phase 0 (M0) code that still lives in the crate.

**Reachability result: it is not reachable from the Phase 1 filesystem.**

```
grep -rn "CloudClient::new|CloudClient {" client/core/src/ffi/ client/core/src/vfs/ client/main/
  -> no matches
```

`CloudClient` is constructed nowhere in the FFI boundary, the VFS, or
`client/main`. `space_core_start` does not touch it. Every Phase 1 filesystem
operation is served by `MemVfs`, which holds no network client.

**Recorded limitation, not claimed as clean:** `reqwest` is still a declared
dependency of `space-client-core` and is therefore linked into the client
binary, even though no Phase 1 code path calls it. That is dead weight in the
shipped artifact, not a behavioural violation. Removing it is a Phase 0 cleanup
that would touch the cloud crates, so it is recorded here rather than done
during Phase 1 certification.

## Verdict

**PASS** for the requirement as stated — no Phase-2+ technology participates in
the Phase 1 filesystem path.

**With one observation:** `CloudClient` and its `reqwest` dependency remain
compiled into `space-client-core`. Unreachable, but present.
