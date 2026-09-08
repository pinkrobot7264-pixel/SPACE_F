# Phase 1 autonomous completion ledger

Live status of the autonomous queue. Statuses: PASS / FAIL / RUNNING / NOT RUN /
BLOCKED / NEEDS HUMAN / INCOMPLETE. No row reaches PASS without the required
duration or count actually having executed.

| Requirement | Status | Evidence | Duration/Count | Failure? | Remaining |
|---|---|---|---|---|---|
| T1 fuzz_op_sequence full budget (§17.6) | **PASS** | `fuzz-op-sequence-fullbudget.txt` | 896,448 runs / 1805s | none | — |
| T2 kill matrix mid-enumeration (§15.2) | NOT RUN | — | 20 required | — | queued |
| T3 NTSTATUS 4-column (§12.4) | **PASS** | `ntstatus-matrix.md` | 8/8 rows match | 2 harness | — |
| T4 compatibility scripted rows (§16.3) | **PASS** | `compatibility-matrix.md` | 29/29 ops ok | 2 harness | — |
| T5 I/O stress 30 min + 500 MB robocopy (§16.2) | NOT RUN | — | 1800s | — | runner written |
| T6 soak 4 hours (§16.5) | NOT RUN | — | 240 min | — | queued |
| T7 CI incl. no-WinFsp job (§17.3/17.7) | **PASS** | Actions runs for `cb3de47`, `6906b2d`, `89b5a82` | both jobs, every step | **yes, once** | re-verify after final push |
| T8 collect-evidence (§17.1) | NOT RUN | — | — | — | must be last |
| T9 shutdown, five states (§15.1) | **PASS** | `shutdown-states.txt` | 5/5 states | 1 harness | — |
| T10 fault injection, six ops + scaling (§13.3) | RUNNING | `fault-injection.txt` | 6 ops + 1 scaled + panic | 2 harness | in flight |
| §16.1 200 mount/unmount cycles | **FAIL (withdrawn)** | `mount-stress-INVALID-fake-graceful-harness.txt` | 200 | **yes** | re-run required |
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

## The pattern this run confirmed

Of the defects found so far in this run, **one was in the product and every
other one was in a harness or in the build configuration**. The product defect
is the missing `fault-injection` feature on the `space-client` binary, which
meant §13.3 and two §15.1 states could not be exercised at all. The rest were
tests that did not test what they claimed:

| Harness | What it claimed | What it did |
|---|---|---|
| `mount-stress.ps1` | 100 graceful + 100 forced shutdowns | 200 forced; the counter lied |
| `collect-evidence.ps1` | the §17.1 gate | `exit 0` regardless of failures, missing artifacts silently skipped |
| `kill-matrix.ps1` | `PASS <state> x 20` | printed unconditionally, including for failing states |
| `compatibility-matrix.ps1` | gate on scripted failures | `(pipeline).Count` was null; gate never fired |
| `compatibility-matrix.ps1` | cmd.exe properties works | `-notmatch` on an array filters; reported a defect that does not exist |
| `ntstatus-matrix.ps1` | four-column matrix verified | column 4 never compared to columns 2-3 |
| `ntstatus-matrix.ps1` | expected NTSTATUS values | `0xC0000033` parsed as signed; all 8 rows false MISMATCH |
| `ntstatus-matrix.ps1` | NotADirectory reaches the FS | Win32 path parser rejected it first; FS never asked |
| `shutdown-states.ps1` | teardown timing | clock started after the Ctrl-C helper; measured 1-9ms for everything |
| `fault-injection-test.ps1` | callback bounded by deadline | timed the application call, which is `retries x deadline` |
| `fault-injection-test.ps1` | §3.6 unrelated-op claim | measured after the fault finished, with nothing hung |
| *(would-be)* | §13.3 hang bounded | drove a binary with fault points compiled out; every hang a no-op |

The last row is the one to keep in mind when reading any PASS in this
directory: it would have produced seven green rows measuring nothing at all.
The guard that now requires `FAULT INJECTION ARMED` in the log exists because
of it.

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
release tree, gitleaks found no leaks. Subsequent runs for `cb3de47`, `6906b2d`
and `89b5a82` were green on **every step of both jobs**, including
`conformance-no-winfsp`.

**§16.1 evidence withdrawn.** `scripts/mount-stress.ps1` required alternating
force-kill and graceful shutdown because they exercise different paths (Class B
process death vs the ADR-0013a main-thread teardown). Both branches called
`Stop-Process -Force` while the summary reported a graceful/forced split, so a
200-cycle run claimed 100 graceful shutdowns and performed none. Fixed in
`3275f19`; the artifact is renamed to
`mount-stress-INVALID-fake-graceful-harness.txt` with the retraction appended,
and **§16.1 must be re-run**.

**§13.3 could not have run at all.** The `fault-injection` feature existed on
`space-client-core` but not on the `space-client` binary, so every script that
drives faults through a mount was driving a build in which
`arm_fault_from_env()` is an empty function. Measured directly: with the
ordinary binary and `SPACE_FAULT=winfsp_pre_read=hang` set, a read completed in
**1327ms** against a 30500ms bound. Both fault-driving scripts now require the
`FAULT INJECTION ARMED` line in the client log before proceeding.

**`scripts/soak.ps1` does not satisfy §16.2.** It verifies file *length*, not
content; enumerates 200 entries, not 5,000; has no robocopy; and runs one
sequential worker, not concurrent ones. §16.2 now has its own runner,
`scripts/io-stress.ps1`. `soak.ps1` remains the §16.5 leak detector, which is a
different test with different pass rules.

## Not a defect: the 60-second read

The first live §13.3 run reported `FAIL read : 60873ms > 30000+500`. This is
**not** a deadline defect and was not treated as one. The boundary log shows
each individual callback returning in 30000, 30004, 30006, 30007 and 30008 ms —
every one inside the bound. Windows *retries* a request that fails this way, so
the application-visible time is `retries x deadline`. §13.2 assertion 1 is a
claim about the callback, so it is now measured on the callback via the log's
`duration_ms`, with the application-visible time recorded alongside as context.
