# CI verification (§11.2, §17.3, §17.7)

Verified at **job and step level** via the GitHub Actions API, not by reading
the workflow YAML and not by trusting the run's top-level conclusion.

- Repository: `pinkrobot7264-pixel/SPACE_F`
- Branch: `phase/1-winfsp`
- Commit: **`abaf369`**
- Run id: **`34380048183`**
- Conclusion: **success**

## Job `conformance-no-winfsp` — success

The §11.2 requirement is that the conformance suite runs with **no WinFsp
installed**, in its own CI job. The job asserts the absence rather than assuming
it.

| step | result |
|---|---|
| Set up job | success |
| Run actions/checkout@v4 | success |
| Run dtolnay/rust-toolchain@stable | success |
| **Assert WinFsp is NOT installed** | **success** |
| **Build the core without WinFsp** | **success** |
| **Conformance suite, no mount** | **success** |
| **Conformance suite in release (ADR-0013 panic model)** | **success** |
| Post checkout / Complete job | success |

## Job `windows` — success

| step | result |
|---|---|
| Set up job, checkout | success |
| Install WinFsp | success |
| Verify WinFsp developer files | success |
| rust-toolchain@stable, install-action@nextest | success |
| **Format** (`cargo fmt --all -- --check`) | **success** |
| **Lint** (`cargo clippy --workspace --all-targets -- -D warnings`) | **success** |
| **Build workspace (debug)** | **success** |
| **Test** (`cargo nextest run --workspace`) | **success** |
| **Release build** | **success** |
| **Release build must not enable fault injection** | **success** |
| Set up MSVC | success |
| **C++ WinFsp adapter (MSVC + Ninja)** | **success** |
| **Secret scan** (gitleaks) | **success** |
| Post checkout / Complete job | success |

Every gated step ran; none was skipped. This is the state that a `Format`
failure previously prevented — that failure short-circuited Lint, Build, Test,
Release build, the fault-injection assertion, the adapter build and the secret
scan, so the run proved nothing about any of them.

## Outstanding

**This evidence is for `abaf369`, which is not the final certification HEAD.**
Commits landed after it. A final push and a fresh CI run at the certification
HEAD are required before `collect-evidence.ps1`, and this file must be updated
with that run id. Until then §17.3/§17.7 are **PASS at `abaf369`, re-run
required at final HEAD**.
