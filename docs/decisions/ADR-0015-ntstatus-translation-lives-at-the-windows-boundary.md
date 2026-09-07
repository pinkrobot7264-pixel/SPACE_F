# ADR-0015 — NTSTATUS translation lives at the Windows boundary

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §12.4
- **Amends:** Phase 0 M0.4

## Context

Phase 0 put `ErrorCode::ntstatus()`, the `NtStatus` type alias and the
`STATUS_*` constants in `contracts`. `contracts` is also a dependency of the
cloud service, which runs on Linux and has no business knowing what an NTSTATUS
is.

## Decision

- **`contracts::ErrorCode`** — the taxonomy. No Windows types, no NTSTATUS.
- **`client/core/src/ffi/ntstatus.rs`** — the mapping table and the
  exhaustiveness test.
- **The VFS never sees an NTSTATUS or a Win32 error code.** It returns
  `SpaceError` and nothing else.

Phase 0's `ErrorCode::ntstatus()` moves accordingly, and M0.4's mapping test
moves with it. The test iterates `ErrorCode::ALL`, so it keeps working
unchanged.

## Totality, not injectivity

**Multiple codes may map to one NTSTATUS.** `OperationTimeout` and
`NetworkTimeout` both map to `STATUS_IO_TIMEOUT`; the integrity codes all map to
`STATUS_FILE_CORRUPT_ERROR`. The test asserts **totality** — every code has a
mapping — not injectivity. Phase 0's test did not assert a bijection, so no
relaxation was needed; the totality property is stated explicitly in the moved
test's comment so nobody strengthens it by mistake.

## Consequences

- `contracts` compiles for a non-Windows target without conditional compilation.
- The conformance suite compiles without any Windows crate, which is what makes
  the no-WinFsp CI job possible (§11.2, §11.7).
- The Phase 0 report carries amendment A2 recording the move.

## Enforcement

`ntstatus::from_error_code` + its exhaustiveness test over `ErrorCode::ALL`; the
§11.7 assertion that no VFS method returns a Windows type; the §12.4 four-column
matrix.
