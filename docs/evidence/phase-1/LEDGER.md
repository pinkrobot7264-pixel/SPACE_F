# Phase 1 autonomous completion ledger

Live status of the autonomous queue. Statuses: PASS / FAIL / RUNNING / NOT RUN /
BLOCKED / NEEDS HUMAN / INCOMPLETE. No row reaches PASS without the required
duration or count actually having executed.

| Requirement | Status | Evidence | Duration/Count | Failure? | Remaining |
|---|---|---|---|---|---|
| T1 fuzz_op_sequence full budget (§17.6) | RUNNING | `fuzz-op-sequence-fullbudget.txt` | target 1800s | — | in flight |
| T2 kill matrix mid-enumeration (§15.2) | NOT RUN | — | 20 required | — | queued |
| T3 NTSTATUS 4-column (§12.4) | NOT RUN | — | all reachable codes | — | queued |
| T4 compatibility scripted rows (§16.3) | NOT RUN | — | 4 clients × 7 ops | — | queued |
| T5 I/O stress 30 min + 500 MB robocopy (§16.2) | NOT RUN | — | 1800s | — | runner written |
| T6 soak 4 hours (§16.5) | NOT RUN | — | 240 min | — | queued |
| T7 CI incl. no-WinFsp job (§17.3/17.7) | RUNNING | GitHub Actions | — | **yes, once** | see below |
| T8 collect-evidence (§17.1) | NOT RUN | — | — | — | must be last |
| T9 shutdown, five states (§15.1) | NOT RUN | — | 5 states | — | runner written |
| T10 fault injection, six ops + scaling (§13.3) | NOT RUN | — | 6 ops + 1 scaled | — | runner written |
| Explorer 10 steps (§9.2) | NEEDS HUMAN | — | — | — | human |
| Notepad (§16.3) | NEEDS HUMAN | — | — | — | human |
| 7-Zip (§16.3) | NEEDS HUMAN | — | — | — | human |
| Explorer responsive under hang (§13.2) | NEEDS HUMAN | — | — | — | human |
| ProcMon write confinement (§16.4) | NEEDS HUMAN | — | — | — | human |

## Requirements added to the queue after re-reading the manual

T9 and T10 were not in the original eight. Both are §-numbered requirements
with no runner, found by re-reading §15.1 and §13.3 verbatim:

- **T9 (§15.1)** — the main-thread teardown path must be exercised in five
  states, including *an operation hung via a fault point* and *a poisoned
  filesystem*. Nothing exercised the graceful path at all.
- **T10 (§13.3)** — the hang must be repeated for write, open, readdir,
  getinfo and rename, not read alone, and the bound must then be shown to
  **scale with configuration** at `callback_timeout_ms = 1000`. Only read was
  covered.

## Failures recorded during this run

**T7, run `34224457328` (commit `33d0b7a`) — FAILURE.** The `windows` job failed
at step 7, `cargo fmt --all -- --check`. That step gates the rest, so Lint,
Build workspace, Test, Release build, the fault-injection feature assertion, the
C++ adapter build and the secret scan were all **skipped** — the push proved
nothing about any of them. The `conformance-no-winfsp` job passed independently.

Root cause was not the drift but the absence of a local gate: `scripts/pre-commit.sh`
ran only a secret scan, so formatting state was never checked before a commit
could reach CI. Fixed in `cb3de47` — `cargo fmt --all` applied, and the hook now
runs the same command CI does. The full chain was then re-run locally before
re-pushing: clippy `-D warnings` clean, workspace build clean, nextest
**270/270 passed**, release build clean, no `fault-injection` feature in the
release tree, gitleaks found no leaks.

**`scripts/mount-stress.ps1` — fabricated evidence, fixed.** §16.1 requires
alternating force-kill and graceful shutdown because they exercise different
paths (Class B process death vs the ADR-0013a main-thread teardown). Both
branches called `Stop-Process -Force`, while the summary still reported a
graceful/forced split — so a 200-cycle run claimed 100 graceful shutdowns and
performed none. The graceful branch now delivers a real `CTRL_C_EVENT` via
`scripts/send-ctrl-c.ps1`, and a failure to deliver it is counted as a failure
rather than silently downgraded to a force kill. **The §16.1 result on record
was produced by the broken harness and must be re-run.**

**`scripts/soak.ps1` does not satisfy §16.2.** It verifies file *length*, not
content; enumerates 200 entries, not 5,000; has no robocopy; and runs one
sequential worker, not concurrent ones. §16.2 now has its own runner,
`scripts/io-stress.ps1`, with four concurrent workers, SHA-256 content
verification, a 5,000-entry enumeration and a hash-compared 500 MB robocopy in
and out. `soak.ps1` remains the §16.5 leak detector, which is a different test
with different pass rules.
