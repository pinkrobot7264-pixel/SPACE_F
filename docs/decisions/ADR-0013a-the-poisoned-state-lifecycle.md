# ADR-0013a — The poisoned-state lifecycle

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §2.3, §4.1, §4.3, §15.1
- **Extends:** ADR-0013

## Context

ADR-0013 says a panic poisons the filesystem. That leaves the hard questions
unanswered: what happens to callbacks already running, what happens to callbacks
that arrive after, who performs the teardown, and how the teardown avoids
deadlocking against the dispatcher threads it is waiting for.

## Decision — the state machine

```
RUNNING   ──panic / unrecoverable invariant failure──▶ POISONING
POISONING ──flag published, unmount signalled────────▶ POISONED
POISONED  ──main thread begins teardown──────────────▶ STOPPING
STOPPING  ──dispatcher stopped, mount removed────────▶ UNMOUNTED
```

## Decision — the rules

| Concern | Rule |
|---|---|
| New callbacks in POISONING / POISONED / STOPPING | Return `STATUS_INTERNAL_ERROR` immediately. **Do not acquire the state lock. Do not read or mutate filesystem state.** |
| In-flight callbacks | Allowed to run to completion or to their deadline. They are never aborted; aborting them is what would leave Windows in an undefined state. |
| `Cleanup` and `Close` after poisoning | Become no-ops. They are `void` and cannot report failure, and touching poisoned state is exactly what poisoning forbids. Memory is reclaimed by process exit, which is imminent. **This is a deliberate, bounded leak — documented, not accidental.** |
| Who owns shutdown | **The main thread. Only the main thread.** |
| Who initiates | The poisoning thread sets the flag and signals a channel. It performs no teardown itself. |
| Deadlock prevention | `FspFileSystemStopDispatcher` **must never be called from a dispatcher thread** — it waits for dispatcher threads to drain, including the caller. The signal-and-return design makes this structurally impossible. |
| Fully stopped | After `space_adapter_unmount()` returns and `space_core_stop()` completes. `os-safety-check.ps1` must then pass. |

The same lifecycle is entered by an unrecoverable invariant failure detected in
a debug build (§3.5).

## One poison mechanism, not two

`parking_lot` is used for VFS state rather than `std::sync::Mutex` partly
because `std`'s mutex poisons on panic. Two overlapping poison mechanisms — the
`LifeState` atomic and mutex poisoning — would mean two definitions of
"poisoned" that can disagree. `parking_lot` does not poison, so the `LifeState`
atomic is the single explicit mechanism.

## Enforcement

`accepting_work()` checked before any state access; `enter_poisoning()` sets
state and signals only; the §2.5 panic-model table (including "poisoning from a
dispatcher thread → no deadlock; unmount completes"); §15.1's poisoned-shutdown
case; `os-safety-check.ps1`.
