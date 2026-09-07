# ADR-0013 — Panic containment, poisoning, and the limits of both

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §2.3, §13.5, §15.2

## Context

"The filesystem is crash-safe" is the kind of claim that gets made once and then
covers two entirely different failure modes, only one of which is actually
handled. This ADR separates them and states plainly what each mechanism does
**and does not** cover.

## Decision — two distinct failure classes. Do not conflate them.

### Class A — Rust panic

A recoverable-in-principle bug detected inside Rust: a failed assertion, an
`unwrap` on `None`, an arithmetic overflow in a debug build.

```
Rust panic
  → catch_unwind at the FFI entry point (every build profile)
  → log message + request_id at error level
  → return STATUS_INTERNAL_ERROR to Windows
  → transition RUNNING → POISONING
  → controlled shutdown, owned by the main thread
```

**Why catch rather than abort:** the log line carrying the `request_id` is the
only diagnostic you get, and an abort discards it.

**Why poison rather than continue:** a panic means an assumption was violated,
possibly mid-mutation. A filesystem that recovers from a panic and keeps serving
reads has, by definition, continued past a broken invariant.

**This requires `panic = "unwind"`.** Phase 0's `panic = "abort"` in the
workspace release profile is **amended** by this ADR — recorded in
`docs/evidence/phase-0/PHASE-0-REPORT.md`, amendment A1. The guard must be
effective in release, or debug and release have different panic semantics, which
is worse than either.

### Class B — process-level failure

Access violation, stack overflow, allocation failure/abort, a fault inside the
C++ adapter, `TerminateProcess`, external kill.

**`catch_unwind` provides no protection against any of these, and this project
never claims it does.** For Class B the recovery mechanism is external: the
process dies, WinFsp's FSD observes the termination, and the volume is torn
down. `S:` disappears and the system stays healthy.

That is what §15.2's kill tests certify — and it is the **only** thing they
certify, because Phase 1 has no durable state (§15.4). Crash-durability
certification belongs to Phase 5 and must not be claimed here.

## Coverage table

| Failure | Contained by | Windows sees | Certified by |
|---|---|---|---|
| Rust panic | `catch_unwind` + poison | `STATUS_INTERNAL_ERROR`, then clean unmount | §13.5, §15.1 |
| Access violation / stack overflow / OOM abort | nothing in-process | process death, FSD tears down volume | §15.2 |
| C++ adapter fault | nothing in-process | process death, FSD tears down volume | §15.2 |
| External kill | nothing in-process | process death, FSD tears down volume | §15.2 |

## Enforcement

The `guard()` function in `client/core/src/ffi/mod.rs`; the panic-model test
table in §2.5, which must pass in **debug and release**; the §13.5 `Panic` fault
row driven through a real mount; the §15.2 kill matrix; `os-safety-check.ps1`.
