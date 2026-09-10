# §16.5 gap — SPACE's own live counts were never sampled

**Classification: EVIDENCE GAP against the manual. Not a known defect. Requires
a decision before Phase 1 is signed off.**

Found on 2026-09-10 by reading the authoritative manual after it was recovered
and committed. Earlier audits ran against `REQUIREMENTS.md`, the derived
transcription, which does not carry this detail — exactly the limitation that
was flagged and is now closed.

## What the manual requires

§16.5, verbatim:

> Four hours of mixed workload, mounted, release build. Sample every 5 minutes:
> RSS, handle count (Process Explorer), thread count, **and SPACE's own live
> node / handle / cursor counts via the diagnostics endpoint**.

And in the verdict table:

> | SPACE live handle/cursor count does not return to baseline after a
> create/delete cycle | **fail** (INV-FS-2, INV-RES-1) |

## What was actually sampled

`soak-samples.csv`, 48 rows over 240.1 minutes:

```
timestamp, elapsed_min, rss_bytes, handles, threads, root_entries
```

Three of the four required quantities are present: RSS, OS handle count, thread
count. **SPACE's own live node / handle / cursor counts are absent.**

## Why they are absent

There is no diagnostics endpoint to sample. `VfsDiagnostics`
(`client/core/src/vfs/invariants.rs:38`) exposes exactly one method:

```rust
pub trait VfsDiagnostics {
    fn check_invariants(&self) -> Result<(), InvariantViolation>;
}
```

No live node, handle or cursor count is exposed, and **nothing is exported over
the FFI** — `space_core.h` contains no diagnostics entry point. An external
sampler has nothing to call.

`scripts/soak.ps1` acknowledged the row and substituted an argument for it:
that the workload creates and deletes every iteration, so a growing count would
show up. That is a reasonable inference. It is not the measurement the manual
asks for.

## What the existing evidence does and does not support

**Does support:**

- `root_entries` held at exactly **1** across all 48 samples. The workload
  creates directories under the root and deletes them each iteration, so the
  node count demonstrably returned to baseline for the whole four hours.
- INV-FS-2 and INV-RES-1 are themselves verified continuously elsewhere:
  `check_invariants()` runs after every conformance step and every fuzz
  operation (§17.4), across 43,987,951 fuzz executions with zero violations.
- RSS ended **below** baseline (11,235,328 → 10,698,752) with handles flat at
  157 — inconsistent with a handle or cursor leak of any size.

**Does not support:**

- A time series of SPACE's internal live **handle** and **cursor** counts during
  the soak, which is what §16.5 names. The verdict row "does not return to
  baseline after a create/delete cycle" was therefore never evaluated directly
  during the soak.

## Options

**A. Accept the gap, documented.** The plateau criteria in §16.5 — the RSS
rows — were met and are what §17.6's gate bullet cites ("Soak meets the §16.5
plateau criteria"). The unmeasured row is a *fail condition*, and the indirect
evidence above is strongly against a leak. Cost: none. Risk: one of §16.5's
named sampling quantities remains unmeasured.

**B. Close it properly.** Add a diagnostics endpoint exposing live node, handle
and cursor counts over the FFI; extend `soak.ps1` to sample it; re-run the full
four-hour soak. Cost: new production code plus ~4 h. This is the only option
that produces the evidence the manual actually names.

**This document does not choose.** Option B adds production surface for
test observability and requires a soak re-run, both of which were explicitly
constrained. Option A leaves a named requirement unmeasured. That is a
certification judgement, not an engineering one.

## Status

`R-S16-5` is currently recorded **PASS** on the plateau criteria. That record is
accurate for what was measured and should be read together with this file. It is
**not** a claim that every §16.5 sampling requirement was satisfied.
