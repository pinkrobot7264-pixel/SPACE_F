# SPACE concurrency model (Phase 1 §3.6)

Part of the versioned contract (ADR-0011).

Phase 1 is deliberately simple, and **the simplicity is documented rather than
discovered.**

## Two layers of serialization

1. **WinFsp's operation guard**, set to
   `FSP_FILE_SYSTEM_OPERATION_GUARD_STRATEGY_COARSE` (§4.1): one lock for the
   whole filesystem, taken exclusively by mutating operations and shared by
   reading ones.
2. **The VFS's own lock:** `MemVfs` holds all state behind a single
   `parking_lot::Mutex`.

**The effective model: exactly one VFS operation executes at a time.** Layer 2
dominates. Layer 1 is retained because it is the strategy Phase 10 will tune,
and changing it later should not be the first time it is exercised.

## The model, stated as answers

| Question | Phase 1 answer |
|---|---|
| Which operations run concurrently? | **None**, inside the VFS. Multiple dispatcher threads may be *in flight*, but at most one holds the state lock. |
| Which serialize? | All of them. |
| What happens during an injected hang? | The hung operation holds the state lock. Every other operation blocks on lock acquisition **until its own deadline expires**, then returns `OperationTimeout`. Nothing hangs indefinitely; nothing succeeds while the lock is held. |
| Does one hung callback block unrelated operations? | **Yes**, and this is the documented Phase 1 behaviour, bounded by L9. Phase 10 owns fixing it. §13.2 confirms it **empirically** rather than assuming it. |
| How does shutdown interact with in-flight work? | In-flight callbacks run to completion or deadline; new work is refused (ADR-0013a); the main thread then stops the dispatcher, which waits for the dispatcher threads to drain. |

## Implementation requirement

**Lock acquisition must be deadline-bounded, or L9 is a lie.**

```rust
fn state<'a>(&'a self, cx: &OpCtx) -> Result<MutexGuard<'a, MemVfsState>, SpaceError> {
    self.inner
        .try_lock_until(cx.deadline)
        .ok_or_else(|| SpaceError::new(ErrorCode::OperationTimeout, "state lock deadline exceeded"))
}
```

This is why `parking_lot` is used rather than `std::sync::Mutex` — `std` has no
deadline-bounded acquisition.

`parking_lot` also **does not poison on panic**, which is correct here:
**poisoning is a single explicit mechanism** (the `LifeState` atomic, §2.3), not
two overlapping ones that can disagree.

## The diagnostic path is exempt

`check_invariants()` takes the state lock with an unbounded `lock()`. It is a
diagnostic, never on the Windows-facing path in a release build, and a
diagnostic that gives up on a timeout would report "healthy" for a wedged
filesystem — the worst possible answer.

## Verification, not assumption

§13.2 arms `winfsp_pre_read` to `Hang` and measures:

1. the callback returns `STATUS_IO_TIMEOUT` within `callback_timeout_ms` + 500 ms
2. the issuing application receives a controlled error, not a hang
3. Explorer stays responsive
4. **other operations return `OperationTimeout` rather than hanging** — this
   model predicts it; the test confirms it
5. unmount still succeeds
6. `check_invariants()` passes afterwards
7. `os-safety-check.ps1` passes

**If the observed behaviour differs from this document, the document is wrong —
fix the document, do not quietly accept the difference.**

## Thread-sanitization tooling

Explicitly out of scope for Phase 1: under a single global state lock it would
be meaningless. It becomes relevant when Phase 10 introduces real concurrency,
and this document is rewritten and re-versioned then.

## Phase 2 obligation

This document describes a **known simplification**. Phase 2 must design real
concurrency, and §3.6 must be rewritten with a `VFS_CONTRACT_VERSION` bump and
an ADR. "Mutation during enumeration" becomes definable at that point and must
be defined (fs-semantics §8).
