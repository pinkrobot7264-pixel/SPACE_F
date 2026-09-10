# §13.3 finding — the `open` row does not terminate, and the cause is the harness

**Classification: TEST-HARNESS DEFECT, compounded by a test-design limitation.
Not a product defect.**

Run: 2026-09-10 10:25:23 → stopped under control 14:36:53. HEAD `8922440`.
Evidence preserved in `fault-injection-open-row-nontermination/`.

## What was observed

The `open` row (`SPACE_FAULT=winfsp_pre_open=hang`) ran **4 h 08 m** without
reaching its trigger, producing **491 faulted callbacks** at a perfectly steady
2.00 per minute, with no sign of convergence. The previous attempt had been
killed at 183 callbacks; this run passed that by 2.7×.

## Root cause — established, not inferred

Every faulted callback is an open of the **volume root**, not of the row's
trigger file:

```
paths in get_security_by_name preceding each faulted open:
    490  "path":"\\"
```

490 of 490 on `\`. The application trigger (`ReadAllText("$r\o.txt")`) was never
reached, so Windows was not retrying anything on the application's behalf.

The source is this harness's own readiness probe, in `Start-Faulted`:

```powershell
for ($i = 0; $i -lt 720; $i++) {
    Start-Sleep -Milliseconds 250
    if (Test-Path "$root\") { $up = $true; break }
}
if (-not $up) { ... throw "mount did not appear within 180s" }
```

The loop was written for **180 seconds** — 720 iterations × 250 ms — and the
comment says so. That assumes `Test-Path` returns promptly. With
`winfsp_pre_open` armed to hang, `Test-Path "S:\"` **is itself an open of the
root**, so each iteration costs 250 ms + the full 30 s callback deadline.

    720 iterations x 30.25s = 21,780s = 6 hours, not 180 seconds

So the row is **bounded, at ~6 hours**. At the controlled stop it was 491/720,
with ~115 minutes still to run. It was never going to hang forever; it was going
to take 6 hours to report "mount did not appear within 180s".

## The deeper limitation

Fixing the loop does not make this row work. `SPACE_FAULT` is read **once at
startup** (`arm_fault_from_env`), so the fault is armed before the volume is
usable. With `winfsp_pre_open` hung, *every* open costs 30 s — including the
readiness probe, the seed write, and the trigger. The row cannot reach the
condition it exists to test, because the mount is unusable by construction.

Arming this point post-mount would need a control channel, which ADR-0013's
fault design deliberately does not have in Phase 1.

## What the run DOES prove

The §13.2 assertion 1 requirement — *the callback returns within
`callback_timeout_ms` + 500 ms* — is proven for `winfsp_pre_open` **491 times
over**, which is far stronger evidence than the single trigger the row was
designed to produce:

| measure | value |
|---|---|
| faulted `open` callbacks | **491** |
| minimum duration | **30000 ms** |
| maximum duration | **30015 ms** |
| bound (`callback_timeout_ms` + 500) | **30500 ms** |
| callbacks exceeding the bound | **0** |
| first / last | 08:28:40.62Z / 12:36:42.09Z |

Every callback returned inside the bound. The deadline holds under a hung open,
sustained over four hours.

## What it does NOT prove

For the `open` point specifically, these were **NOT REACHED** — not failed:

- assertion 2, the application receives a controlled error
- assertion 4, an unrelated operation stays bounded while the fault is in flight
- assertion 5, unmount still succeeds after the injected hang

They remain proven for `read` and `write`, which completed normally in this same
run.

## Classification

| candidate | verdict |
|---|---|
| SPACE product defect | **No.** 491/491 callbacks inside the bound; the deadline is enforced exactly as specified. |
| **Test-harness defect** | **Yes.** The readiness loop is bounded by iteration count, not wall clock, so its own probe multiplies its cost by the deadline it is testing. |
| **Test-design limitation** | **Yes.** A startup-armed hang on `open` makes the volume unusable, so the row cannot reach its trigger by construction. |
| Expected Windows/WinFsp behaviour | Partly — Windows correctly surfaces the timeout each time; it is not retrying pathologically. |
| Insufficient evidence | No. The path distribution settles it. |

## Termination boundary used, and why it is defensible

Stopped at **491 callbacks / 4 h 08 m**, in controlled order (harness first so it
could not respawn a client, then the client), with the environment verified
clean afterwards: no client, `S:` absent, no WinFsp volume.

Defensible because the requirement's assertion for this fault point was already
satisfied 491 times with a maximum of 30015 ms against a 30500 ms bound, and the
remaining ~115 minutes of the loop could only have added more of the same before
throwing a misleading "mount did not appear within 180s". Continuing would have
consumed the window needed for the §16.5 soak without producing new evidence.

**§13.3 is NOT marked PASS on the strength of this.** The `open` row is recorded
as PARTIAL with the assertions above listed as NOT REACHED, and the requirement
is not rewritten to make the partial result look complete.
