# Host event — future-dated Kernel-Power 41 poisons the §1.5 safety gate

**Status: environment defect, not a SPACE defect. The gate must be re-run once
the event falls outside the check window (after 20:58:56 on 2026-09-09).**

## What happened

`scripts/os-safety-check.ps1` is the §1.5 gate and runs after every stress test.
For the §15.2 run of 18:10:04 it reported:

```
FAIL  Kernel-Power 41 (unexpected shutdown)
PASS  no bugcheck
PASS  no stale S: volume
PASS  no orphaned client process
PASS  S: released
```

The functional result of that run was **60/60 PASS** (idle ×20, mid-write ×20,
mid-enumeration ×20). Only the host gate failed.

## Why it is not a filesystem-induced crash

| observation | value |
|---|---|
| event `TimeCreated` | **20:58:56** |
| wall clock when observed | **18:18:44** |
| event is in the future | **yes, by 2.7 hours** |
| `BugcheckCode` | **0** |
| bugcheck event (id 1001) | **none in 24 h** |
| `C:\Windows\MEMORY.DMP` | **absent** |
| last boot | **17:28:53** |

A filesystem that crashes Windows produces a bugcheck. There is none: no 1001
event, no crash dump, and the Kernel-Power 41 record itself carries
`BugcheckCode = 0`.

The surrounding System-log entries are firmware, not storage:

- `Bootmgr failed to obtain the BitLocker volume master key from the TPM`
- `... because Secure Boot configuration changed unexpectedly`
- `The system firmware failed to enable overwriting of system memory on restart`

Together with a boot at 17:28:53 and an event dated 20:58:56, this is an
unclean host shutdown followed by the clock being corrected **backwards**,
leaving the event stamped in the future.

This also explains two earlier §15.2 runs that died with no completion record:
the host went down at roughly 17:21–17:28 while they were executing.

## Consequence for the remaining gates

`os-safety-check.ps1` filters `Id=41` with `StartTime = $Since`, where `$Since`
is the moment the test started. Because the offending event is dated 20:58:56,
**every test started before that instant will flag it**, regardless of what the
test did. T10, T5 and T6 will all report the same failure until then.

## What was NOT done

The check was not modified, filtered, or relaxed. Excluding a specific event —
even a demonstrably bogus one — would turn the one gate that exists to catch
SPACE destabilising the machine into a gate that reports what we would like to
see. The §16.1 harness has already been through that failure mode once.

## Required action

Re-run the gate, and the short tests that carry it, after **20:58:56**:

- `scripts/os-safety-check.ps1` standalone, to confirm the host is quiet
- §15.2 kill-matrix (7 min at current speed) for clean gate evidence
- §16.2 io-stress and §16.5 soak started after that instant

Until then, any gate FAIL attributable solely to this event must be recorded as
such, with the run's functional result reported separately — as it is for the
18:10:04 §15.2 run above.
