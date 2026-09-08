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
| T5 I/O stress 30 min + 500 MB robocopy (§16.2) | NOT RUN | — | 1800s | — | queued |
| T6 soak 4 hours (§16.5) | NOT RUN | — | 240 min | — | queued |
| T7 CI incl. no-WinFsp job (§17.3/17.7) | RUNNING | GitHub Actions | — | — | in flight |
| T8 collect-evidence (§17.1) | NOT RUN | — | — | — | must be last |
| Explorer 10 steps (§9.2) | NEEDS HUMAN | — | — | — | human |
| Notepad (§16.3) | NEEDS HUMAN | — | — | — | human |
| 7-Zip (§16.3) | NEEDS HUMAN | — | — | — | human |
| Explorer responsive under hang (§13.2) | NEEDS HUMAN | — | — | — | human |
| ProcMon write confinement (§16.4) | NEEDS HUMAN | — | — | — | human |
