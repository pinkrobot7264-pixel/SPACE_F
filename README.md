# SPACE

Windows-first cloud filesystem: remote/cloud storage that behaves like a local
filesystem, built on WinFsp with chunked, content-addressed object storage,
immutable versions and a crash-safe durable-state layer.

See `docs/architecture/` and the Architecture Decision Records in
`docs/decisions/`.

## Prerequisites

See `docs/runbooks/dev-machine-setup.md`. In short: Windows 11 + VS 2022 C++
build tools (MSVC v143) + Windows SDK + CMake + Ninja + WinFsp 2.x (with the
Developer feature) + Rust stable `x86_64-pc-windows-msvc` + cargo-nextest.

## Build

```powershell
.\scripts\bootstrap.ps1   # once: dirs, config.toml, git hook, verify toolchain
.\scripts\build.ps1       # Rust workspace (debug + release) + C++ WinFsp adapter
```

## Test

```powershell
.\scripts\test.ps1        # fmt, clippy -D warnings, nextest, release guard
```

## Layout

| Path | What |
|---|---|
| `contracts/` | shared vocabulary: ids, error model + NTSTATUS, domain model + 13 invariants, API DTOs, redaction, log schema |
| `client/` | `core` (cloud client + startup), `config`, `main` (`space-client` skeleton), `winfsp-adapter` (C++ link stub) |
| `cloud/` | `api` (`space-cloud` axum server), `metadata` (in-memory `MetadataStore` trait), plus Phase 8+ stubs |
| `objectstore/` | `ObjectStore` / `ObjectStoreAdmin` traits, `FakeObjectStore`, shared conformance suite |
| `faults/` | fault-point registry (compiled out of release) |
| `tests/` | `harness` (process isolation + drop-guard teardown), `generators`, `suites` (crash/concurrency/security/performance scaffolds + E2E chain) |
| `tools/` | `corruptor`, `fixturegen` |
| `schemas/` | `api.openapi.yaml` |
| `docs/` | `architecture/`, `protocols/`, `decisions/`, `runbooks/`, `evidence/` |

## Phase status

**Phase 0 — foundation. Complete.** Repository, workspace, build system, CI,
contracts, config, logging + redaction, fault-point registry, object-store
abstraction + fake, deterministic generators, client & cloud skeletons,
integration harness, and the end-to-end contract chain (happy + negative) are in
place and tested. **There is no filesystem mount yet** — that is Phase 1.
