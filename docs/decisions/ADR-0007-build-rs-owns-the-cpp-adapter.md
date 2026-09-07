# ADR-0007 — `build.rs` owns the C++ adapter; CMake is retired

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2

## Context

Phase 0 built the C++ WinFsp adapter with CMake + Ninja, separately from
`cargo`. That gives two build systems, two dependency graphs and two ways for
the adapter and the Rust core to drift out of step.

## Decision

`client/main` (Rust) is the single binary. The C++ adapter is a **static
library compiled by the `cc` crate from `client/main/build.rs`**. One build
system, one dependency graph, one `cargo build`.

The Phase 0 `CMakeLists.txt` files remain in the tree as a standalone linkage
check, **off the build path**. They are not invoked by CI and not required to
produce the binary.

## Consequences

- `cargo build` is sufficient to produce a working `space-client.exe`.
- `build.rs` owns the WinFsp include/lib discovery and the delay-load link
  arguments (§2.4). Getting the delay-load wrong is the failure mode this
  centralisation is meant to make unrepeatable.
- `println!("cargo:rerun-if-changed=../winfsp-adapter")` keeps C++ edits
  triggering a rebuild.

## Enforcement

`client/main/build.rs` compiles `host.cpp` and `callbacks.cpp`. If the binary
links, the adapter was built by `cc`. CI builds with `cargo` only.
