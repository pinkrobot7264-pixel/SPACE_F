# Open issue — intermittent mount failure during §16.1 cycling

**Status: OPEN, cause not established.** Recorded here rather than buried in a
run log, because §16.1 is otherwise reported PASS and a reader deciding whether
to certify Phase 1 needs to weigh this.

## What was observed

Three mount failures in 1,350 mount/unmount cycles (0.22%), all inside a single
25-minute window, none reproduced in the 950 cycles since.

| window | cycles | failures | notes |
|---|---:|---:|---|
| run 3, 21:21 | 200 | 1 | iteration 106 |
| run 4, 21:42 | 200 | 2 | iterations 18 (21:44:36) and 24 (21:45:27) |
| run 5, 22:03 | 200 | 0 | |
| reproducer, 22:22 | 150 | 0 | deliberate CPU + process-creation load |
| run 6, 22:34 | 200 | 0 | |
| run 7, 22:53 | 200 | 0 | |
| run 8, 23:12 | 200 | 0 | deliberate `Test-Path S:\` + `Get-ChildItem S:\` every 150 ms |
| **total** | **1,350** | **3** | |

In every failure the harness reported `mount did not appear within 15s` and the
client process had **already exited**. That rules out the harness timeout being
too tight: a merely slow mount would still be running at the 15-second mark.
The client genuinely failed to mount and exited, which means `client/main/src/main.rs`
logged a `mount failed` line with an NTSTATUS.

**That NTSTATUS was never captured.** The capture added after run 3 read the
client's stderr while the process was still writing, saving 274 bytes for
iteration 18 — the startup config line, cut mid-JSON — and nothing at all for
iteration 24. Run 4 had already been parsed with that broken version, so the fix
could not take effect mid-run. The capture now waits for the process to exit,
forces it if it will not, waits for the flush, and additionally records
`fsptool lsvol` at the moment of failure. If this recurs, the NTSTATUS and the
volume state will both be in `docs/evidence/phase-1/mount-stress-failure-iter<N>.log`.

## Hypotheses tested and rejected

- **Harness timeout too tight.** Rejected: the client had already exited.
- **System load.** Rejected: 150 rapid remount cycles under deliberate CPU and
  process-creation load produced 0 failures.
- **Mount immediately after a forced kill.** Rejected: every cycle of that
  150-cycle reproducer was a forced kill, so every mount followed one.
- **Contention from concurrent access to the mount root.** This was the leading
  hypothesis, because the only two runs that failed were the two during which
  diagnostic commands (`ls S:/`, `Test-Path 'S:\'`) were being run against the
  mount under test. Rejected on two independent grounds: run 8 polled `S:` every
  150 ms for a full 200-cycle run and saw 0 failures, and the failure timestamps
  (21:44:36, 21:45:27) do not coincide with when those commands actually ran
  (~21:53), when no failure occurred.
- **A stale volume colliding with the next mount.** Not supported: no failing
  attempt was preceded by a registered volume in any instrumented run, and
  `Test-Path` going false was never observed to lag `lsvol`.

## What is NOT claimed

That the failures are understood, benign, or attributable to the environment.
They are unexplained. Four consecutive clean 200-cycle runs do not retire an
intermittent fault; they bound its rate.

## Why §16.1 is nonetheless reported PASS

The §16.1 criterion is 200 mount/unmount cycles with no stale mount, a clean
remount each time, and `os-safety-check` clean. That criterion was met in full
by four independent consecutive runs (5, 6, 7, 8 — 800 cycles), each with 100
graceful and 100 forced shutdowns whose paths were proven from the client's own
log. The acceptance criteria were not changed to obtain this result; the two
failing runs are reported as failing runs and their evidence is preserved.

A reader who considers a 0.22% unexplained mount failure disqualifying for
Phase 1 should treat §16.1 as BLOCKED rather than PASS. That is a judgement
about risk tolerance, not about the evidence, and the evidence is here to
support either decision.

## Preserved evidence

- `mount-stress-run3-1failure-iter106.txt`
- `mount-stress-run4-2failures-iter18-24.txt`
- `mount-stress-run6.txt`, `mount-stress-run7.txt`, `mount-stress-run8-POLLED.txt`
- `mount-stress-failure-iter18.log`, `mount-stress-failure-iter24.log` (both truncated — see above)
- `mount-stress-INVALID-fake-graceful-harness.txt` (earlier harness defect, unrelated)
- `mount-stress-run2-INVALID-no-console.txt` (earlier launch-context defect, unrelated)
