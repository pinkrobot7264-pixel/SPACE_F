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



## Phase 1 and Later



Filesystem mount and unmount stress tests will run on a self-hosted Windows runner named:



`space-fs-runner`



Mount-related tests are tagged:



`#[ignore]/mount`



These tests require a stable Windows environment with a predictable drive-letter configuration.



## Rationale



Mount stress and process-kill recovery tests require a stable, non-ephemeral filesystem environment.



The hosted `windows-latest` runner is appropriate for Phase 0 foundation tests, but filesystem mount stress testing from Phase 1 onward requires a controlled Windows runner.

