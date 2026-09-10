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
| What happens during an injected hang? | The hung operation holds the state lock until **its own** deadline expires, then returns `OperationTimeout` and releases it. Another operation blocks on acquisition meanwhile — nothing succeeds while the lock is held — and then **acquires it and completes normally**. Nothing hangs indefinitely. See the measured correction under "Verification, not assumption": an unrelated operation is bounded and **succeeds**; it does not itself return `OperationTimeout`. |
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
4. **other operations are bounded rather than hanging** — see the correction
   below; this model predicted `OperationTimeout`, and measurement disagreed
5. unmount still succeeds
6. `check_invariants()` passes afterwards
7. `os-safety-check.ps1` passes

**If the observed behaviour differs from this document, the document is wrong —
fix the document, do not quietly accept the difference.**

### Correction, 2026-09-10 — point 4 as written was wrong

Measured on a live mount at commit `353b23f`, all six fault points, with the
hang armed and an unrelated operation issued while the fault was in flight
(`docs/evidence/phase-1/fault-injection.txt`):

| armed point | unrelated operation | slowest callback anywhere |
|---|---|---|
| `winfsp_pre_read` | **completed**, 61703 ms wall | 30009 ms |
| `winfsp_pre_write` | **completed**, 60608 ms wall | 30011 ms |
| `winfsp_pre_open` | **completed**, 58173 ms wall | 30009 ms |
| `winfsp_pre_readdir` | **completed**, 60071 ms wall | 30005 ms |
| `winfsp_pre_getinfo` | **completed**, 58329 ms wall | 30011 ms |
| `winfsp_pre_rename` | **completed**, 30657 ms wall | 30003 ms |

The unrelated operation **succeeds**. It does not return `OperationTimeout`.

Why the prediction was wrong: the hung callback holds the state lock only until
**its own** deadline expires, at which point it returns `OperationTimeout` and
releases the lock. The unrelated operation then acquires it and completes
normally. Its wall time is long because Windows retries the *faulted* request
and the coarse guard serialises the queue — not because the unrelated operation
is itself timing out. Each dispatch carries a fresh deadline, so it rarely
reaches one.

The claim above that "**nothing succeeds while the lock is held**" remains
correct, and is the property that matters: the lock is genuinely exclusive. What
was wrong was inferring from it that other operations must therefore *fail*.
They wait, then succeed.

**The safety property Phase 1 depends on is unchanged and is proven**: no
callback exceeds `callback_timeout_ms + 500 ms` (measured maximum 30011 ms
against a 30500 ms bound, across every fault point), and nothing hangs
indefinitely. L9 is not a lie. What changes is only the predicted *outcome* for
the unrelated caller — bounded and successful, rather than bounded and failed.

Phase 10 still owns removing the single-lock serialisation; that judgement is
unaffected.

## Thread-sanitization tooling

Explicitly out of scope for Phase 1: under a single global state lock it would
be meaningless. It becomes relevant when Phase 10 introduces real concurrency,
and this document is rewritten and re-versioned then.

## Phase 2 obligation

This document describes a **known simplification**. Phase 2 must design real
concurrency, and §3.6 must be rewritten with a `VFS_CONTRACT_VERSION` bump and
an ADR. "Mutation during enumeration" becomes definable at that point and must
be defined (fs-semantics §8).
