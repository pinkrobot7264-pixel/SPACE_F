# Staleness provenance — the 16 STALE artifacts in the final evidence bundle

`collect-evidence.ps1` flags an artifact STALE when its mtime predates the HEAD
being certified. At final collection it flagged **16**. Every flag is retained.

**The staleness rule was not modified, relaxed, or special-cased to obtain a
pass.** No historical artifact was renamed, re-dated, or presented as current.
This document records where each artifact came from, what changed after it was
produced, and what does and does not provide current validation.

## The baseline

The most recent commit touching any path outside `docs/`:

| | |
|---|---|
| commit | **`d70b623`** |
| date | 2026-09-10T20:39:42+02:00 |
| subject | fix: two defects in the 17.1 evidence collector, found by running it |
| files changed | `scripts/collect-evidence.ps1` |

The commit before it that touched anything outside `docs/`:

| | |
|---|---|
| commit | **`fd41d67`** |
| date | 2026-09-10T20:13:40+02:00 |
| files changed | `client/core/src/vfs/memvfs/tests.rs`, `docs/evidence/phase-1/human/README.md` |

## What changed after the evidence was produced

Exactly two things, in this order:

1. **`fd41d67` → `client/core/src/vfs/memvfs/tests.rs`.** A `#[cfg(test)]`
   module. The change made
   `enumeration_work_per_call_does_not_grow_with_directory_size` take the best
   of three timing samples instead of one, because a single sample failed under
   a full 270-test parallel run and passed in isolation. The assertion is
   unchanged at `ratio < 4.0`.

2. **`d70b623` → `scripts/collect-evidence.ps1`.** The evidence collector
   itself: it was reading the working-tree status after creating its own output
   directory, and its hashing pipeline was enumerating the file it was writing.

**No shipped product code changed after any artifact was produced.** The last
commit touching `client/`, `contracts/`, `cloud/`, `objectstore/` or `faults/`
in a non-test capacity is `d7e981e` (2026-09-09T18:09:32), which predates every
artifact below except four documents carried forward from 2026-09-08.

Neither `tests.rs` nor `collect-evidence.ps1` is compiled into
`space-client.exe`. A `#[cfg(test)]` module is excluded from the shipped binary
by the compiler; the collector is a PowerShell script that runs after testing.

## The 16 artifacts and their provenance

| artifact | mtime | produced by |
|---|---|---|
| `soak-samples.csv` | 2026-09-10 18:54 | §16.5 soak, 240.1 min, 48 samples, at `a406772` |
| `fault-injection.txt` | 2026-09-10 20:06 | §13.3, 7/8 rows, 20 m 14 s, at `353b23f` |
| `REQUIREMENTS.md` | 2026-09-10 20:23 | traceability matrix, updated continuously |
| `io-stress.txt` | 2026-09-10 10:08 | §16.2, 30 min, 0 worker errors, at `da21bdf` |
| `mount-stress.txt` | 2026-09-10 09:36 | §16.1, 200/200, 24 m 48 s, at `da21bdf` |
| `kill-matrix.txt` | 2026-09-10 09:11 | §15.2, 60/60, 9 m 00 s, at `da21bdf` |
| `shutdown-states.txt` | 2026-09-10 09:02 | §15.1, 5/5, 72.6 s, at HEAD of that time |
| `compatibility-matrix.md` | 2026-09-10 09:00 | §16.3, 29/29 scripted, 9.9 s |
| `ntstatus-matrix.md` | 2026-09-10 09:00 | §12.4, 8/8 rows match, 1.4 s |
| `mount-functional-test.txt` | 2026-09-10 09:00 | §17.5, 21/21, 15.1 s |
| `fuzz-results.txt` | 2026-09-09 22:00 | §14.1, 6 targets × ≥1800 s, 43,987,951 runs, at `abaf369` |
| `fuzz-op-sequence-fullbudget.txt` | 2026-09-08 14:38 | the post-INV-ID-4 full-budget re-run, at `33d0b7a` |
| `LEDGER.md` | 2026-09-08 15:16 | live queue ledger from the autonomous run |
| `PHASE-1-CERTIFICATION.md` | 2026-09-08 14:36 | certification narrative, superseded in part by later evidence |
| `enumeration-scaling.md` | 2026-09-08 03:14 | O(N²) enumeration measurement, after the windowed-cursor fix |
| `EXPLORER-CHECKLIST.md` | 2026-09-08 01:21 | human checklist, not a test result |

## What provides current validation for the only affected behaviour

The single test whose behaviour `fd41d67` altered is
`enumeration_work_per_call_does_not_grow_with_directory_size`. Its current
validation is **not** any file above — it is the CI run at the final HEAD:

| | |
|---|---|
| run | **`34513250205`** |
| commit | **`a68d3d8`** |
| job | `windows` → step **Test** (`cargo nextest run --workspace`) |
| result | **success** |

That step runs the whole workspace, including the modified test, on the
certification commit. It is current by construction: it executed *against* the
change rather than before it. Locally the same suite passed 270/270 in four
consecutive full-workspace runs after the change.

`d70b623` came after that CI run, and changes only `collect-evidence.ps1`, whose
output is the bundle itself rather than an input to any test.

## What this does NOT establish

- It does not make the 16 artifacts current. They are stale, they are flagged
  stale, and the bundle's verdict reflects that.
- It does not re-validate the soak, fuzz, mount-stress, io-stress, kill-matrix,
  shutdown-states or fault-injection runs at the final HEAD. Those results were
  produced at the commits named above and nothing since has re-run them.
- It does not assert that a documentation-only or test-only commit *cannot*
  matter. It asserts what changed, and leaves the significance to the reader.

A reader who requires every artifact to postdate HEAD should treat this bundle
as not certifying Phase 1 and re-run the full suite — approximately 8 h 30 m,
dominated by the 4-hour soak and 3-hour fuzz. That was offered and the decision
taken was to keep the flags and document provenance instead.
