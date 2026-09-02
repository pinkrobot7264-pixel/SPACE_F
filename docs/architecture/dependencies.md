# SPACE toolchain and dependency versions

Recorded at the Phase 0 exit gate. The `verify-env` script
(`scripts/verify-env.ps1`) checks the presence and minimum version of each.

## Platform (M0.0)

| Component | Version verified | Notes |
|---|---|---|
| Windows | 11 Pro 26200 (dev VM) | Windows-first project; dev happens in a Win11 guest (see runbook) |
| WinFsp | **2.1** (`2.1.25156`, SxS `20260901T102116Z`) | Core + **Developer** feature (headers, `winfsp-x64.lib`, MEMFS). Driver registers as `winfsp+<InstanceID>`; use `fsptool-x64.exe lsdrv`, not `sc query`. |
| Visual Studio 2022 Build Tools | MSVC 19.44 / v143 (`14.44.35207`) | Desktop C++ workload, Windows 11 SDK, C++ CMake tools. **No WDK** (ADR-0001). |
| CMake | 3.28+ (bundled with VS BuildTools) | |
| Ninja | bundled with VS BuildTools | CI initialises the MSVC env before invoking Ninja so `cl.exe` is the compiler. |

## Rust (M0.1)

| Component | Version | Notes |
|---|---|---|
| rustc / cargo | 1.98.0 | host triple `x86_64-pc-windows-msvc` (never `-gnu`) |
| workspace MSRV | 1.80 | `rust-version` in `Cargo.toml` |
| cargo-nextest | 0.9.x | per-test process isolation; required from Phase 5 |
| rustfmt, clippy | bundled | CI runs `cargo fmt --check` and `clippy -D warnings` |

## Key crates (`Cargo.toml [workspace.dependencies]`)

`tokio` 1 · `axum` 0.7 · `tower-http` 0.5 · `serde` 1 · `serde_json` 1 ·
`toml` 0.8 · `thiserror` 2 · `uuid` 1 (v7) · `blake3` 1 · `bytes` 1 ·
`chrono` 0.4 · `tracing` 0.1 · `tracing-subscriber` 0.3 ·
`tracing-appender` 0.2 · `rand` 0.8 · `rand_chacha` 0.3 · `reqwest` 0.12.

## Installed during Phase 0 setup, not exercised by Phase 0 code

Per the execution manual (§1.10, §1.11) these are part of the Phase 0 machine
bootstrap and are checked by `scripts/verify-env.ps1`, but no Phase 0 crate
depends on them. Install steps are in `docs/runbooks/dev-machine-setup.md`.

| Tool | Installed via | First *used* by |
|---|---|---|
| **PostgreSQL 18** | EDB installer | **Phase 8** -- backend + schema; until then the cloud metadata store is in-memory behind the `MetadataStore` trait |
| **gitleaks** | `winget install -e --id Gitleaks.Gitleaks` | Phase 0 -- pre-commit hook + CI `gitleaks detect` |
| **Python 3.12** | `winget install -e --id Python.Python.3.12` | repository helper scripts (as needed) |

On any given developer machine these may or may not be present yet; the clean-VM
bootstrap (restore snapshot -> follow the runbook -> `bootstrap.ps1`) is what
establishes and verifies them.

## Release profile

`[profile.release]` sets `panic = "abort"` and `debug = 1` (symbols kept for
crash-dump analysis). The `fault-injection` feature must never be enabled in a
release build; CI asserts this.
