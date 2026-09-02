# SPACE PHASE 0 — FINAL REPORT

Date: 2026-09-02 · Repo: `C:\SPACE\src\space` · Branch: `main` (not committed —
see §13)

## 1. Overall status

**COMPLETE** — every Phase 0 requirement from `SPACE_Phase_0_Execution_Manual`
and the Phase-by-Phase plan has an implementation and passing evidence. No
Phase 1 functionality was introduced. Two items need the user: install
`gitleaks` locally, and decide on commits/tags (§13).

## 2. Requirements completed

| Milestone | Requirement | Evidence |
|---|---|---|
| M0.0 | Platform proof (WinFsp) | `T0.0-winfsp-version.txt` (2.1), `T0.0-winfsp-driver.txt`; C++ adapter links `FspVersion` against `winfsp-x64.lib` via MSVC+Ninja |
| M0.1 | Repo, Rust workspace, build system, WinFsp link check | 18-member workspace builds debug+release; `CMakeLists` + `FindWinFsp.cmake` resolve WinFsp; `scripts/build.ps1` |
| M0.2 | CI + test framework | `.github/workflows/ci.yml` (fmt, clippy -D, build, nextest, release build, fault-injection guard, MSVC C++ adapter, gitleaks); real cargo test targets in `tests/suites` |
| M0.3 | Shared identity types | `contracts/src/ids.rs` — 6 UUIDv7 prefixed ids + `ChunkId` (BLAKE3); 6 tests inc. known BLAKE3 vectors, flipped-byte detection |
| M0.4 | Errors + NTSTATUS map | `contracts/src/errors.rs` — 26 codes, `ALL` slice, `retryable`/`origin`/`ntstatus`; `every_error_code_is_fully_classified`, `all_slice_covers_every_variant`; `docs/protocols/errors.md` |
| M0.5 | Typed config + resource bounds | `client/config` — `deny_unknown_fields`, every numeric bound, `reject_secret_keys`, `check_paths_writable`, release-only rules; 13 tests; `config.example.toml` |
| M0.6 | Logging + redaction + fault points | `contracts/src/logging.rs` (13-field JSON schema, span→line propagation), `contracts/src/redaction.rs` (`Secret<T>`, `sanitize_url`), `faults` crate (12 registered points, compiled out of release); `docs/protocols/fault-points.md`; `tests/suites/tests/logging.rs` |
| M0.7 | Domain model + 13 invariants | `contracts/src/model.rs`, `contracts/src/validate.rs` — one numbered fn per invariant, one-pass/one-fail per invariant; `docs/protocols/schemas.md` |
| M0.8 | API contracts + idempotency + OpenAPI | `contracts/src/api.rs` (DTOs, `contract_version:1`, `deny_unknown_fields`, identical error shape); `schemas/api.openapi.yaml` |
| M0.9 | ObjectStore split + fake + conformance | `objectstore` — `ObjectStore` (no delete) / `ObjectStoreAdmin`; `FakeObjectStore`; `conformance::run_conformance_suite` (14 cases) reused by the fake |
| M0.10 | Generators + corruptor | `tests/generators` — deterministic `ChaCha8`, never-zero bytes, streaming variant proven bounded, `with_seed` prints seed on failure; `tools/corruptor` — one primitive per broken invariant |
| M0.11 | Client skeleton | `client/core/src/startup.rs` + `client/main` — parse args, load+validate config, init logging, startup line, Ctrl-C, bounded shutdown. **No mount.** `--version`→0, bad config→1 with `CONFIG_*` |
| M0.12 | Cloud skeleton | `cloud/api` (`space-cloud`) — axum, `/health` exact shape, request-id middleware (mint on malformed), structured 404, graceful shutdown; `cloud/metadata` `MetadataStore` trait + in-memory impl |
| M0.13 | Integration harness | `tests/harness` — `TestEnv` spawns real `space-cloud` on a free port, isolated namespace, `Drop` teardown + evidence-on-panic; isolation and no-leftover-process tests |
| M0.14 | E2E happy + negative | `tests/suites/tests/e2e_chain_happy.rs` (chain across boundary sizes + one run vs the spawned process), `e2e_chain_negative.rs` (11 cases; each re-reads the prior good version to prove committed state survived) |
| M0.15 | Exit gate | ADR-0001…0006; `docs/protocols/*`; `docs/architecture/{overview,dependencies}.md`; runbooks updated; full gate below |

## 3. Requirements not completed

None functionally. Outstanding operational items:

- **Local `gitleaks`** is not installed, so the local pre-commit hook and a local
  `gitleaks detect` cannot run. The hook file is installed
  (`.git/hooks/pre-commit`) and CI runs gitleaks. Fix: `winget install -e --id
  Gitleaks.Gitleaks`.
- **Git commits + tags** (`M0.2`…`M0.15`, `phase/0-foundation`) not created — the
  engineering-loop rules say not to commit without explicit instruction.
- CI is not executed here (no GitHub Actions runner locally); the workflow is
  written to the documented Windows contract.

## 4. Files created

Contracts: `contracts/src/{ids,errors,model,validate,api,redaction,logging}.rs`.
Object store: `objectstore/src/{lib,conformance}.rs`.
Client: `client/core/src/{lib,startup}.rs`, `client/main/src/main.rs`,
`client/winfsp-adapter/probe.cpp`.
Cloud: `cloud/api/src/{lib,main}.rs`, `cloud/metadata/src/lib.rs`.
Tests: `tests/harness/src/lib.rs`, `tests/generators/src/lib.rs`,
`tools/corruptor/src/lib.rs`, `tools/fixturegen/src/lib.rs`,
`tests/suites/{Cargo.toml,src/lib.rs}`,
`tests/suites/tests/{common/mod,crash,concurrency,security,performance,integration,logging,e2e_chain_happy,e2e_chain_negative}.rs`.
Config/scripts: `config.example.toml`, `scripts/{verify-env,bootstrap}.ps1`,
`scripts/pre-commit.sh`.
Docs: `docs/decisions/ADR-000{1..6}-*.md`,
`docs/protocols/{errors,schemas,fault-points}.md`,
`docs/architecture/{overview,dependencies}.md`,
`docs/evidence/phase-0/{T0.0-winfsp-version,T0.0-winfsp-driver,toolchain}.txt`,
`schemas/api.openapi.yaml`.

## 5. Files modified

`Cargo.toml` (workspace deps, `[profile.release]` `panic=abort`/`debug=1`,
`rust-version`, `tests/suites` member), `Cargo.lock`, every crate `Cargo.toml`
(names aligned to the manual: `space-*`), every crate `src/lib.rs` (real
content), `client/winfsp-adapter/{CMakeLists.txt,adapter.cpp}`,
`.github/workflows/ci.yml`, `.gitignore` (`build-ninja/`),
`README.md`, `docs/runbooks/{ci,dev-machine-setup}.md`,
`scripts/{build,test}.ps1`.

Deleted: loose `tests/*/placeholder.rs` (were never compiled — replaced by real
integration-test targets in `tests/suites/tests/`), and `.gitkeep` files in dirs
that now hold real files.

## 6. Commands executed (representative)

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo build --workspace --release
cargo nextest run --workspace
cargo test --workspace --doc
cargo metadata --no-deps --format-version 1
cargo tree --workspace --edges features        # grep -c fault-injection => 0
cmake -B build -S . -G Ninja                    # under vcvars64 (MSVC)
cmake --build build
```

## 7. Tests

`cargo nextest run --workspace` → **99 passed, 1 skipped** (~9 s).
Skipped: `space-generators::streaming_full_gib` (`#[ignore]` — 1 GiB ChaCha8
pass is slow in debug; the 128 MiB `streaming_does_not_accumulate` runs by
default and a `--run-ignored`/release run covers the literal spec).
Doc-tests: 0 (no doc examples), pass.
Per-area: contracts 39, config 13, faults 2 (+2 with feature), objectstore 3,
generators 8, corruptor 3, metadata 2, cloud-api 4, client-core 3, harness 2,
suites 20 (E2E happy 2, E2E negative 11, logging 2, 5 scaffolds).

## 8. Build verification

- `cargo build --workspace` — OK (debug).
- `cargo build --workspace --release` — OK. `panic="abort"`, `debug=1`. The
  release-only config validators (`wal_fsync_policy != "never"`,
  `auth_mode != "none"`) compile in.
- C++ WinFsp adapter — `cmake -G Ninja` + `cmake --build` under `vcvars64`:
  compiler `cl.exe` (MSVC 19.44 / v143), `WinFsp found: C:/Program Files
  (x86)/WinFsp`, links `space_winfsp_adapter.lib` and `space_winfsp_probe.exe`
  against `winfsp-x64.lib`. Exit 0.
- Binaries smoke-tested: `space-client --version` → 0; `space-client --config
  missing` → 1 + `CONFIG_MISSING`; `space-cloud` → JSON startup log + `/health`
  returns the exact documented shape.

## 9. CI verification

`.github/workflows/ci.yml` on `windows-latest`:
checkout → install+verify WinFsp dev files → Rust stable (rustfmt, clippy) →
nextest install → `fmt --check` → `clippy -D warnings` → `cargo build
--workspace` → `cargo nextest run --workspace` → `cargo build --workspace
--release` → fault-injection guard (`cargo tree --edges features` must not
mention `fault-injection`) → **`ilammy/msvc-dev-cmd@v1` (x64)** → `where cl` +
`cmake -G Ninja` + build → `gitleaks detect`.
Not run here (no runner). Rationale for the self-hosted split from Phase 1 is in
`docs/runbooks/ci.md`.

## 10. Security verification

- `config.toml`, `secrets/`, `*.pem/key/pfx`, `.env*`, `runtime/`, `*.jsonl`,
  `*.dmp`, `target/`, `build/` are git-ignored; none tracked.
- `Config::reject_secret_keys` refuses any config key ending
  `_password/_secret/_token/_key` (tested).
- `Secret<T>` cannot be `Display`/`Debug`-printed; `.expose()` is the only exit
  and appears nowhere in a logging path. `sanitize_url` strips query strings.
  Logging integration test asserts a secret handed to the logger never reaches a
  line.
- Manual pattern scan over tracked source: no AWS keys, private keys, or inline
  passwords.
- Pre-commit hook (`gitleaks protect --staged`) installed; **`gitleaks` binary
  not present locally** — CI covers it.

## 11. Known risks

- `MetadataStore` is in-memory only (by design until Phase 8 / PostgreSQL). The
  trait boundary is in place; the PG implementation is future work.
- E2E runs the contract chain at a 64 KiB test chunk size for speed; the real
  32 MiB `BOUNDARY_SIZES` are exercised by the generator's own unit tests, not
  end-to-end. One happy-path run does go through the spawned `space-cloud`
  process.
- `cargo nextest run --workspace` does **not** build workspace binaries; the
  harness needs `space-cloud`. `scripts/test.ps1` and CI build first; a bare
  `cargo nextest` without a prior build fails with a clear message.
- The C++ side is a link-check only; the real WinFsp callback table is Phase 1.
- Phase 11's 500 GB fixture will not fit the dev VM — decision recorded in the
  runbook.

## 12. Phase boundary

Confirmed — **no Phase 1+ functionality**: no WinFsp mount, no drive letter, no
VFS namespace, no path resolver, no real read/write data path, no chunking
engine, no byte/chunk cache, no WAL, no transfer/retry engine, no scheduler, no
sync, no PostgreSQL, no AWS. `client/winfsp-adapter` is an empty library plus a
link-check probe. Only foundations and contracts that Phase 0 explicitly
requires are present.

## 13. Git status

Committed on branch **`phase/0-foundation`** (off `main` @ `M0.1`), not pushed:

| Commit | Contents | Tags |
|---|---|---|
| `M0.2: CI pipeline, dev scripts, secret-scan hook, C++ link-check probe` | ci.yml, scripts, pre-commit hook, adapter probe, config.example, runbooks | `M0.2` |
| `M0.3-M0.14: contracts, subsystems, skeletons, harness, E2E chain` | the full implementation body | `M0.3` … `M0.14` |
| `M0.15: exit gate -- ADRs, protocol docs, OpenAPI, evidence` | ADRs, protocols, architecture, OpenAPI, evidence | `M0.15`, `phase/0-foundation` |

`M0.3`–`M0.14` share one commit: they were implemented in a single pass and each
intermediate state would not have built on its own (workspace manifest, crate
renames and `Cargo.lock` all move together).

- Working tree clean; only `target/` is untracked (git-ignored).
- `gitleaks detect` over all commits: no leaks. The pre-commit hook ran on every
  commit.
- Not pushed. `main` is unchanged — fast-forward it when ready.

## 14. Recommended next step

1. `winget install -e --id Gitleaks.Gitleaks`, then `.\scripts\bootstrap.ps1`
   and `.\scripts\test.ps1` from a clean shell to confirm the documented path.
2. Review the deviations (crate renames, `tests/suites` member, E2E chunk size,
   CI MSVC step) and the ADRs.
3. If satisfied, commit per milestone and tag `phase/0-foundation`; push; let CI
   run once on `windows-latest`.
4. Restore the `clean-windows` VM snapshot and do the manual's clean-machine
   rerun (§17.3) — the one check that can only be done by you.
5. Begin Phase 1 (WinFsp skeleton + safe mount) against `docs/decisions/ADR-0001`.
