# SPACE CI Runbook



## Phase 0



Phase 0 CI runs on the GitHub-hosted `windows-latest` Windows runner.



Phase 0 CI covers:



- Rust formatting

- Rust Clippy with warnings denied

- Rust workspace tests through cargo-nextest

- Release-build fault-injection check

- C++ WinFsp adapter build

- Secret scanning



WinFsp Developer files are installed during CI and verified before the C++ adapter build.

### C++ toolchain in CI

The C++ adapter must be compiled by MSVC (`cl.exe`), never MinGW/GCC. Because the
build uses the Ninja generator, CI initialises the MSVC developer environment
with `ilammy/msvc-dev-cmd@v1` (arch `x64`) *before* the `cmake -B build -G Ninja`
step, and asserts `cl` is on `PATH` (`where.exe cl`). Without that step Ninja on
`windows-latest` would pick up whatever compiler it finds. `scripts/build.ps1`
does the equivalent locally via `vcvars64.bat` when `cl` is not already on PATH.

### Release fault-injection guard

`cargo tree --workspace --edges features` must not mention `fault-injection`.
`cargo tree` has no `--release` flag; the feature is off by default in the
`faults` crate and nothing enables it, so its absence from the feature graph is
the check.

### Test binary dependency

The integration harness (`space-test-harness`) and E2E suites spawn the built
`space-cloud` binary. CI runs `cargo build --workspace` before `cargo nextest
run --workspace` so the binary exists; the harness locates it by walking up from
the test executable to `target/<profile>/`, or honours `SPACE_CLOUD_BIN`.



## Phase 1 and Later



Filesystem mount and unmount stress tests will run on a self-hosted Windows runner named:



`space-fs-runner`



Mount-related tests are tagged:



`#[ignore]/mount`



These tests require a stable Windows environment with a predictable drive-letter configuration.



## Rationale



Mount stress and process-kill recovery tests require a stable, non-ephemeral filesystem environment.



The hosted `windows-latest` runner is appropriate for Phase 0 foundation tests, but filesystem mount stress testing from Phase 1 onward requires a controlled Windows runner.

