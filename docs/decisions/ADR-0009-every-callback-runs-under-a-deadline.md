# ADR-0009 — Every callback runs under a deadline

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §3.4 (L9), §3.6, §13

## Context

WinFsp callbacks are **synchronous**: a dispatcher thread blocks until the
callback returns, and the kernel waits as long as user mode takes. There is no
kernel-side timeout that rescues a filesystem that stops answering. An
application that issues a read against a wedged callback hangs in an
uninterruptible wait, and Explorer hangs with it.

## Decision

**Every FFI entry point runs under a deadline**, carried in `OpCtx` and derived
from `config.client.callback_timeout_ms`. Expiry produces `OperationTimeout`,
which the boundary translates to `STATUS_IO_TIMEOUT`.

The deadline applies to the **whole callback, including lock acquisition**. This
is the load-bearing detail: a deadline that covers only the operation body but
waits unboundedly for the state lock is not a bound at all. `MemVfs` therefore
acquires its state lock with `parking_lot::Mutex::try_lock_until(cx.deadline)`,
not `lock()`.

**The deadline is the only bound that exists.**

## Consequences

- `std::sync::Mutex` cannot be used for VFS state — it has no deadline-bounded
  acquisition. `parking_lot` is a required dependency, not a preference.
- Under an injected hang, unrelated operations block on the state lock and then
  return `OperationTimeout` rather than hanging. That is the documented Phase 1
  behaviour (§3.6), confirmed empirically in §13.2, not assumed.
- A deadline that fires *early* is as much a defect as one that never fires;
  §13.4 arms a delay just under the deadline and requires success.

## Revisit when

Phase 4 introduces network-backed reads. The answer then is progress-based
extension or partial reads — **not** a longer timeout.

## Enforcement

`OpCtx::check()`, the deadline-bounded `state()` accessor, the §13.2–13.4 fault
procedures, and the conformance assertion that an expired deadline yields
`OperationTimeout`.
