# SPACE architecture overview

SPACE presents cloud storage to Windows as a local filesystem. The design keeps
five concerns separate so each can be tested in isolation.

```
  Windows I/O
      │  (NTSTATUS)
┌─────▼───────────────┐
│ WinFsp kernel driver│  (signed, not ours -- ADR-0001)
└─────┬───────────────┘
      │  callback table (Phase 1)
┌─────▼───────────────┐     thin C++ adapter: translate + bound + cancel
│ client/winfsp-adapter│    (Phase 0: link-check stub only)
└─────┬───────────────┘
      │  FFI
┌─────▼───────────────────────────────────────────────┐
│ Rust client core                                    │
│  vfs · metadata · cache · storage(WAL) · transfer   │
│  scheduler · sync · security · config               │
└─────┬───────────────────────────────────────────────┘
      │  HTTP contract (contracts::api, schemas/api.openapi.yaml)
┌─────▼───────────────────────────────────────────────┐
│ Rust cloud service (cloud/api)                      │
│  api · auth · metadata · manifests · versions       │
│  chunks · database                                  │
└─────┬───────────────────────┬───────────────────────┘
      │ MetadataStore trait   │ ObjectStore trait
┌─────▼─────────┐      ┌──────▼──────────────┐
│ in-memory     │      │ FakeObjectStore     │  Phase 7: disk fake
│ (Phase 8: PG) │      │ (Phase 9: S3)       │  content-addressed, immutable
└───────────────┘      └─────────────────────┘
```

## Data model

- A **File** names bytes and points at its current **Version**.
- A **Version** is immutable and references a **Manifest**.
- A **Manifest** maps logical byte ranges onto immutable, content-addressed
  **Chunks** (BLAKE3 -- ADR-0002). Manifests obey 13 named invariants
  (`docs/protocols/schemas.md`).
- A version is `Candidate` until its manifest validates and it is committed,
  then `Committed` and authoritative (ADR-0006). A crash can only ever leave a
  `Candidate` behind.

## Cross-cutting contracts

- **Errors** (`docs/protocols/errors.md`): one `SpaceError` type, 26 codes, each
  classified for retryability, origin and NTSTATUS. No path blocks forever.
- **Logging** (`contracts::logging`): one JSON object per line, fixed 13-field
  schema, `request_id` propagated client→cloud via `X-Request-Id`.
- **Redaction** (`contracts::redaction`): `Secret<T>` cannot be printed;
  `.expose()` is grep-audited out of logging code.
- **Fault points** (`docs/protocols/fault-points.md`): named injection sites,
  compiled out of release builds.
- **Cache is disposable** (ADR-0005); **durability contract** (ADR-0004).

## Guard rails

The ten non-negotiable engineering guard rails from the phase plan (immutable
chunks, invisible incomplete versions, recoverable durable state, bounded
resources, validate every boundary, fail the operation not Windows, user-mode
first, synthetic data first) are enforced structurally where possible and tested
where not.
