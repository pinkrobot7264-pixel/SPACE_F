# ADR-0001: Language boundary -- thin C++ adapter, Rust core, shared contracts crate

- Status: Accepted
- Date: 2026-09-02

## Context

WinFsp's supported integration surface is a C/C++ callback table. The rest of
SPACE -- VFS model, chunk store, transfer engine, cloud service -- benefits from
Rust's memory safety, error model and test tooling. We must decide how much
logic lives on each side of the FFI line.

## Decision

- The **C++ adapter** (`client/winfsp-adapter/`) is thin. Its only jobs are:
  translate WinFsp callbacks to a small FFI surface, translate results/errors
  back, enforce bounded timeouts and cancellation, and never block Windows I/O
  indefinitely. It holds no business logic.
- The **Rust core** (`client/*`, `cloud/*`) owns everything else.
- A single **`contracts`** crate is the shared vocabulary: identity types, the
  error model + NTSTATUS map, the domain model + invariants, the API DTOs,
  redaction, and the logging schema. `contracts` depends on nothing in the
  workspace; everything depends on it and nothing the other way.
- No custom kernel filesystem driver. We use WinFsp's signed kernel component
  (guard rail #9). The Windows Driver Kit is deliberately not installed.

## Consequences

- Phase 1 implements the callback table in C++ but delegates immediately to Rust.
- FFI surface stays auditable and small.
- `contracts` compiling for both client and cloud keeps wire and in-process
  representations identical by construction.

## Revisit when

The FFI surface grows beyond a dozen functions, or a measured per-call FFI cost
shows up in the Phase 11 performance numbers.
