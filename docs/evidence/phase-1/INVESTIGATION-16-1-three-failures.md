# Investigation — the three §16.1 mount failures of 2026-09-08

Scope: only the three failures already on record. No test was started, stopped
or modified for this investigation; §16.2 was running throughout and was not
touched.

## The three failures

| # | run | iteration | timestamp | client state |
|---|---|---|---|---|
| 1 | run 3, started 21:21:17 | 106 | **not recorded** — bounded to 21:21:17–21:41:17, ~21:31 by position | — |
| 2 | run 4, started 21:42:44 | 18 | **21:44:36.82** | pid 12520, `exited=True` |
| 3 | run 4, started 21:42:44 | 24 | **21:45:27.04** | pid 1284, `exited=True` |

## Evidence loss — disclosed

`mount-stress-failure-iter18.log` and `mount-stress-failure-iter24.log` **no
longer exist and are not in git history**. Commit `fc9b070` committed only the
run summaries, not the captures. I deleted the captures from the working tree
before run 5 to keep captures per-run, without having committed them.

What they contained is known only from having read them at the time: 274 bytes
(a header plus the client's startup config line, cut mid-JSON) and 112 bytes
(header only, no client output). Both were truncated by the capture defect since
fixed — neither contained the `mount failed` NTSTATUS. So no diagnostic content
was lost, but **failure evidence was deleted, which should not have happened.**

Run 3's failure has no capture at all: the capture code did not exist yet.

## The network-interruption hypothesis — REFUTED

Tested directly and rejected on three independent grounds.

**1. No network events exist.** Queried for the window 21:00–22:10 and for the
whole of 2026-09-08:

| log | events |
|---|---|
| `NetworkProfile/Operational` | none |
| `Dhcp-Client/Admin` | none |
| `NCSI/Operational` | none |
| `DNS-Client/Operational` | none |
| `WLAN-AutoConfig/Operational` | none |
| System log, providers matching `Tcpip\|NDIS\|netbt\|Network` | **none, all day** |

No adapter disconnect, no DHCP renewal, no DNS failure, no connectivity change.

**2. No restart followed.** Boot/shutdown events (12, 13, 41, 1074, 6005, 6006,
6008) show the machine up **continuously** from `09-08 04:18:56` to
`09-09 05:30:22`. The failures at 21:31, 21:44:36 and 21:45:27 sit in the middle
of an uninterrupted uptime span. The next restart was roughly eight hours later
and is not correlated.

**3. The client cannot be affected by network state.** Phase 1 serves every
operation from `MemVfs`, entirely in memory. Reachability analysis:

```
CloudClient constructed in ffi/ + vfs/ + main/ : 0
reqwest referenced   in ffi/ + vfs/ + main/    : 0
```

The Phase 1 filesystem path performs **no network I/O at all**, so a network
interruption has no mechanism by which to cause a mount failure.

## What the OS recorded at the failure timestamps — nothing

| source | 21:15–21:50 |
|---|---|
| System log | **4 events**, all *after* both failures: TPM id 17 ×3 at 21:48:17, Service Control Manager 7040 at 21:48:37 |
| Application log | 5 events: `edgeupdate` 21:22:07, `Security-SPP` 21:28/21:40 — all benign, none at a failure time |
| Application Error / WER for `space-client` | **none** at 21:44 or 21:45 |
| WinFsp / Fsp provider events | **none, all day** |

There is no crash record for the client, and no OS-level event coincides with
any of the three failures.

Note against a WER-based theory: WER events do exist that day at 12:45, 22:59
and 23:48. The 22:59 and 23:48 clusters fall inside runs 6, 7 and 8 — which
**passed with zero failures**. WER activity therefore does not correlate with
mount failures either.

## Harness versus product

What is established: the harness reported `mount did not appear within 15s`, and
the client process had **already exited on its own** in both captured cases. A
merely slow mount would still have been running at the 15-second mark, so the
harness timeout is not the cause.

`client/main/src/main.rs` exits on mount failure after logging `mount failed`
with its NTSTATUS. That log line is the single piece of evidence that would
separate "the client failed to mount" (product) from "the client could not be
started correctly" (harness/environment), and it was never captured.

## Classification

| failure | classification |
|---|---|
| run 3, iteration 106 | **insufficient evidence** |
| run 4, iteration 18 | **insufficient evidence** |
| run 4, iteration 24 | **insufficient evidence** |

Not "likely network/environment": that hypothesis was tested and **refuted**.
Not "harness issue" and not "SPACE product issue": distinguishing those requires
the NTSTATUS, which does not exist for any of the three.

## Status unchanged

`OPEN-ISSUE-16-1-intermittent-mount-failure.md` remains **OPEN**. §16.1's PASS
status is unchanged and still carries that open issue. This investigation
eliminated one hypothesis; it did not establish a root cause.

Since the capture defect is now fixed — the harness waits for process exit,
forces it if needed, waits for the flush, and records `fsptool lsvol` at the
moment of failure — a recurrence will produce the NTSTATUS. Measured rate is 3
in 1,350 cycles (0.22%); 400 further cycles have since run clean (runs at
2026-09-09 22:53, 23:09, 23:12 and 2026-09-10 09:12), which bounds the rate
further without explaining it.
