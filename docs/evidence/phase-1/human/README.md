# Phase 1 — human validation package

# HUMAN TESTS HAVE NOT YET BEEN PERFORMED.

No result file in this directory exists. Nothing has been executed, simulated,
observed, or inferred from automated tests. If you find a result file here that
you did not write, treat it as suspect and delete it.

**Source of truth: `SPACE_Phase_1_Execution_Manual_FINAL.md`**, committed at the
repository root (116,397 bytes, sha256 `0290f66252865a2d…`). Every step below is
taken from it and the section is cited. Where this file and the manual disagree,
**the manual wins** — tell me and I will correct this file.

Nothing here is broadened or loosened. An earlier draft of this package was
written from memory and was looser than the manual in two places (the Explorer
steps, and materially the ProcMon pass condition). Both were corrected on
2026-09-10.

---

## Why these five cannot be automated

| gate | bullet | test |
|---|---|---|
| §17.2 Windows safety | Explorer never becomes permanently unresponsive, including during injected hangs | **H4** |
| §17.2 Windows safety | ProcMon evidence: writes confined to `runtime\logs` | **H5** |
| §17.5 Functional | Explorer opens `S:` and enumerates the root | **H1** |
| §17.5 Functional | Create/open/read/write/close/delete from Explorer, PowerShell, Notepad | **H1**, **H2** |
| §17.5 Functional | Path traversal cannot escape the namespace — logically **and** by ProcMon | **H5** |
| §17.7 Engineering | Compatibility matrix complete | **H1**, **H2**, **H3** |

**§17.2 states: any single failure blocks Phase 1.**

---

# GLOBAL RULES — read before starting

## Rule 1 — a failure stays a failure

If a step does not do what this file says it must, record **FAIL** with what you
actually saw. Do not retry until it passes and record the pass. Do not
reinterpret. Do not average. A test that failed once and passed later is
**UNRESOLVED**, not PASS — record both attempts.

## Rule 2 — STOP conditions

**STOP immediately, do not continue to the next test, and report**, if any of
these occur at any point:

| condition | why |
|---|---|
| Blue screen / bugcheck | §17.2: no BSOD or bugcheck across every run |
| `C:\Windows\MEMORY.DMP` appears or its timestamp changes | §17.2: kernel dump = uncontrolled kernel failure |
| The whole machine stops responding (not just the `S:` window) | §17.2: no system-wide hang |
| `S:` remains after the client has exited and will not clear | §17.2: no persistent stale mount |
| A `space-client` process cannot be killed | §17.2: no orphaned process |
| `os-safety-check.ps1` prints anything other than **OS safety OK** | §1.5: its output is gate evidence |

Do not attempt recovery beyond the documented cleanup below. Capture the state
as-is — a stale mount is more useful to me intact than cleaned up.

`docs/runbooks/stale-mount-recovery.md` exists if you need it, but read it
rather than improvising.

## Rule 3 — safety check after every test

After **each** of H1–H5, run:

```powershell
cd C:\SPACE\src\space
.\scripts\os-safety-check.ps1
```

It must print **OS safety OK** and exit 0. Paste its full output into that
test's result file. If it fails, see Rule 2.

Note: the check reports `WARN` lines for unrelated critical/error system events
(this machine has recurring TPM/BitLocker firmware warnings). `WARN` does not
fail the check — only `FAIL` lines do. The exit code is authoritative.

## Rule 4 — start clean, every time

Before each test:

```powershell
cd C:\SPACE\src\space
Get-Process space-client* -ErrorAction SilentlyContinue     # expect: no output
Test-Path "S:\"                                             # expect: False
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol # expect: no output
```

If any of the three is not as expected, **do not start the test.** That is a
contaminated environment and any result from it is void.

## Rule 5 — cleanup, every time

```powershell
Get-Process space-client* | Stop-Process -Force
Start-Sleep -Seconds 3
Test-Path "S:\"                                             # expect: False
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol # expect: no output
Remove-Item Env:\SPACE_FAULT -ErrorAction SilentlyContinue
```

---

# H1 — Explorer evidence (§9.2)

**STATUS: NEEDS HUMAN — not performed**

## Prerequisites

Rule 4 clean check. Then mount:

```powershell
cd C:\SPACE\src\space
Start-Process .\target\release\space-client.exe -ArgumentList "--config",".\config.toml"
Start-Sleep -Seconds 3
Get-PSDrive S
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol
```

`Get-PSDrive S` must succeed and `lsvol` must list `S:` (§4.4).

## The ten steps — manual §9.2 verbatim

Perform **in Explorer**, not from a shell, except where a command is given.

| # | action | PASS criterion |
|---|---|---|
| 1 | Explorer → `S:` | opens and shows the root; no error dialog |
| 2 | Right-click → New → Folder | folder appears; still there after F5 |
| 3 | Create a text file; open in Notepad; type; save; close; reopen | **content persists** exactly as typed |
| 4 | Copy a 10 MB file into `S:` | copy completes; no error |
| 5 | Copy it back out under a new name; `Get-FileHash` both and compare | **hashes identical** |
| 6 | Rename a file; rename a folder | both renames take effect; old names gone |
| 7 | Delete a file; delete a folder | both disappear |
| 8 | Create 5,000 files in one directory; then run the command below | returns **exactly 5000** |
| 9 | Create a 10-level nested path; browse to the bottom | bottom level reachable in Explorer |
| 10 | Right-click → Properties on a file | size and timestamps **sane** |

Step 5 command:

```powershell
Get-FileHash "C:\path\to\original.bin"
Get-FileHash "S:\copied-back.bin"
```

Step 8 command — the manual specifies this exact form:

```powershell
Get-ChildItem S:\many | Measure-Object
```

The manual notes for step 8: *"correct at 50 entries and truncated at 5,000 is
the classic marker bug."* Record the **exact number** returned. "Looked right"
is not a result. If it returns anything other than 5000, that is **FAIL** —
record the number.

To create the 5,000 files (this part may be scripted):

```powershell
New-Item -ItemType Directory -Path "S:\many" -Force | Out-Null
1..5000 | ForEach-Object { Set-Content "S:\many\f$_.txt" "$_" }
```

## FAIL criteria

Any step not meeting its PASS criterion. Also FAIL if Explorer shows an error
dialog at any point — record the dialog text **verbatim**.

## Evidence to capture

Create **`docs/evidence/phase-1/explorer/`** — the manual names this exact
directory, and `collect-evidence.ps1` looks for it.

- `docs/evidence/phase-1/explorer/H1-explorer.md` — one line per step: number,
  PASS/FAIL, what you observed, exact numbers for steps 5 and 8, verbatim text
  of any dialog
- `docs/evidence/phase-1/explorer/` — screenshots if you take them (optional;
  name them `step-<n>.png`)
- Paste the `os-safety-check.ps1` output (Rule 3) at the end of `H1-explorer.md`

## Cleanup

Rule 5.

---

# H2 — Notepad (§16.3)

**STATUS: NEEDS HUMAN — not performed**

Fills the `Notepad` row of the §16.3 compatibility matrix. The manual says:
**record observed behaviour, not expected.**

## Prerequisites

Rule 4, then mount as in H1.

## Actions

1. Open Notepad, type text, **Save As** → `S:\notepad-test.txt`
2. Close Notepad. Reopen the file from `S:`
3. Append more text; save; close; reopen
4. **Save As** over an existing file on `S:`; accept the overwrite prompt

## PASS criteria

1. Save succeeds, no error dialog
2. Content matches exactly what was typed
3. Both original and appended text present
4. Overwrite succeeds; reopening shows the new content

## FAIL criteria

Any save or open error; content differing in any way; overwrite failing or
silently not taking effect. Record dialog text verbatim.

## Evidence

- `docs/evidence/phase-1/human/H2-notepad.md` — the four actions with PASS/FAIL
  and what you observed, plus the `os-safety-check.ps1` output
- Add the `Notepad` row to `docs/evidence/phase-1/compatibility-matrix.md`
  with columns: create, read, write, rename, delete, enumerate, properties

*Why a human:* Notepad saves by replace-and-rename, which the scripted `.NET`
and `cmd` clients do not exercise.

## Cleanup

Rule 5.

---

# H3 — 7-Zip or similar (§16.3)

**STATUS: NEEDS HUMAN — not performed**

Fills the `7-Zip or similar` row of the §16.3 matrix.

## Prerequisites

Rule 4, then mount as in H1. Note the 7-Zip version.

## Actions

1. Create an archive **from** a folder on `S:`
2. Extract that archive **to** a folder on `S:`
3. Verify extracted contents against the originals
4. Open an archive stored on `S:` and browse it **without extracting**

Step 3 — prefer hash comparison over 7-Zip's own CRC test:

```powershell
Get-FileHash "S:\original\*" | Sort-Object Path
Get-FileHash "S:\extracted\*" | Sort-Object Path
```

## PASS criteria

Archive creates; extraction completes; **every file's hash matches**; archive
browsable in place with no extraction.

## FAIL criteria

Any CRC error; any hash mismatch; archive unreadable in place. Record 7-Zip's
exact error text.

## Evidence

- `docs/evidence/phase-1/human/H3-7zip.md` — 7-Zip version, the four actions,
  hash comparison results, `os-safety-check.ps1` output
- Add the `7-Zip or similar` row to `compatibility-matrix.md`

## Cleanup

Rule 5.

---

# H4 — Explorer responsiveness during an injected hang (§13.2 point 3)

**STATUS: NEEDS HUMAN — not performed**

This is the **only** point of §13.2 that automation cannot make. Points 1, 2, 4,
5, 6 and 7 are already proven — `fault-injection.txt`, 31 of 32 assertions
across all six fault points, slowest callback 30011 ms against a 30500 ms bound.

## READ THIS FIRST — what is deliberate and what is not

This test **deliberately** hangs one filesystem callback for
`callback_timeout_ms` (30 seconds by default). During those 30 seconds:

**EXPECTED — this is the test working, not a fault:**
- The Explorer window showing `S:` blocks, greys out, or says "Not Responding"
- The read you triggered does not return
- Other operations touching `S:` are slow

§3.6 documents this: one hung callback holds the state lock and other operations
wait. It is bounded by design and Phase 10 owns removing it.

**FAIL — the gate condition:**
- Explorer becomes **permanently** unresponsive — still dead well after the
  30-second deadline passes, and does not recover
- **Other** Explorer windows, on `C:` or elsewhere, become unresponsive
- The Windows shell as a whole dies or restarts

**STOP — not this test's business (Rule 2):**
- Blue screen, bugcheck, `MEMORY.DMP`
- The whole machine stops responding
- The client process crashes rather than returning an error
- `S:` cannot be unmounted afterwards

The distinction the gate draws is **temporary blocking (expected) versus
permanent unresponsiveness (FAIL) versus crash or unsafe state (STOP)**.

## Prerequisites

Rule 4 clean check. Note this uses the **fault-injection build**, not the
release build.

```powershell
cd C:\SPACE\src\space
$env:SPACE_FAULT = "winfsp_pre_read=hang"
Start-Process .\target\debug\space-client-fault.exe -ArgumentList "--config",".\config.toml"
Start-Sleep -Seconds 3
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol
```

`lsvol` must list `S:`. Confirm the fault armed — this line must appear in the
client's output:

```
FAULT INJECTION ARMED -- this build is not fit for production use
```

**If that line is absent, STOP.** The fault is not armed and the test would
measure nothing. That exact failure mode wasted four hours during automation.

Create a file to read (creation is not faulted):

```powershell
Set-Content "S:\hang-test.txt" "payload"
```

## Actions

1. Start a stopwatch
2. In **Explorer**, open `S:` and double-click `hang-test.txt` (or preview it)
3. While it is blocked, **try to use another Explorer window on `C:`** — this is
   the actual gate question
4. Wait for the deadline to expire (~30 s)
5. Record the elapsed time until an error appeared or the operation returned

## PASS criteria

- Other Explorer windows and the rest of the shell **stayed usable throughout**
- The `S:` window **recovered** after the deadline — it is not permanently dead
- An **error** appeared rather than an indefinite hang
- Elapsed time before the error is in the region of 30 s, not unbounded

## FAIL criteria

Explorer permanently unresponsive; the shell restarting; no error ever appearing
and the window never recovering.

## Evidence

- `docs/evidence/phase-1/human/H4-explorer-under-hang.md` containing:
  - confirmation the `FAULT INJECTION ARMED` line was present
  - **exact elapsed time** before the error appeared
  - the error dialog text **verbatim**
  - explicitly: did other Explorer windows stay usable? did the `S:` window
    recover?
  - `os-safety-check.ps1` output

## Cleanup

```powershell
Get-Process space-client* | Stop-Process -Force
Start-Sleep -Seconds 3
Remove-Item Env:\SPACE_FAULT
Test-Path "S:\"                                             # expect: False
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol # expect: no output
```

**Clearing `SPACE_FAULT` matters** — leaving it set would arm the fault in a
later session.

---

# H5 — No-arbitrary-writes evidence (§16.4, INV-NS-6 external half)

**STATUS: NEEDS HUMAN — §16.4 is BLOCKED on this**

The manual's §16.4 verbatim:

> Process Monitor, filter `Process Name is space-client.exe` **and**
> `Operation is WriteFile`. Run the full I/O stress. Assert the only write
> targets are `C:\SPACE\runtime\logs\*` — nothing under `C:\Windows`, nothing in
> the source tree, nothing under `C:\Users`. Export CSV →
> `docs/evidence/phase-1/procmon-writes.csv`.

**Time cost: ~35 minutes.** The manual requires the *full* I/O stress, which is
a 30-minute run plus staging. Budget for it; do not shorten it.

## Prerequisites

Rule 4 clean check. Process Monitor installed.

## Actions

1. Start Process Monitor
2. Set the filter — **exactly these two conditions, ANDed**:
   - `Process Name` **is** `space-client.exe`
   - `Operation` **is** `WriteFile`

   Only `WriteFile`. Do not add `CreateFile`, `SetDispositionInformationFile` or
   any other operation — the manual specifies `WriteFile`, and a wider filter
   makes the capture harder to judge, not safer.
3. Confirm capture is running (**File → Capture Events** ticked)
4. Run the full I/O stress — this mounts and unmounts on its own:

```powershell
cd C:\SPACE\src\space
.\scripts\io-stress.ps1 -Minutes 30 -Drive S
```

5. When it finishes, stop capture in ProcMon
6. Inspect **every** captured write path

## PASS criterion — exact, from §16.4

The only write targets are **`C:\SPACE\runtime\logs\*`**.

Specifically **nothing** under:
- `C:\Windows`
- the source tree (`C:\SPACE\src\...`)
- `C:\Users`

Anything outside `C:\SPACE\runtime\logs\` is **FAIL**, including anywhere else
under `C:\SPACE\runtime\`.

> This is narrower than it may look. An earlier draft of this file said
> "`C:\SPACE\runtime` and the mounted volume", which is **wrong** — it would
> pass a run the manual fails. The manual says `C:\SPACE\runtime\logs\*`.

If you see writes to `S:` itself in the capture, note them separately and tell
me — I need to judge them against §16.4's wording rather than have you decide.

## FAIL criteria

Any `WriteFile` by `space-client.exe` to a path outside `C:\SPACE\runtime\logs\`.
Record the full path and the count. Do not dismiss a single occurrence.

## Evidence

- **`docs/evidence/phase-1/procmon-writes.csv`** — exact path and filename;
  `collect-evidence.ps1` looks for it and currently reports it OUTSTANDING.
  Export via **File → Save… → Comma-Separated Values (CSV)**, "Events displayed
  using current filter"
- `docs/evidence/phase-1/human/H5-procmon.md` — the filter you used, total event
  count, the distinct write paths observed, PASS/FAIL, `os-safety-check.ps1`
  output

This also supplies the ProcMon half of §17.5's *"path traversal cannot escape
the namespace — logically **and** by ProcMon"*. The logical half is already
proven by `conformance/naming.rs`.

## Cleanup

Rule 5. Also stop ProcMon capture so it does not keep logging.

---

# After all five

Do **not** update `REQUIREMENTS.md` rows yourself unless you want to — tell me
the results and I will record them, or record them yourself as the person who
performed the tests. The rows are R-S4-5, R-S9-2, R-S13-3 (Explorer point only),
R-S16-3 (GUI rows) and R-S16-4.

Then re-run the collector so the bundle picks up the two outstanding artifacts:

```powershell
cd C:\SPACE\src\space
.\scripts\collect-evidence.ps1
```

It will stop reporting `procmon-writes.csv` and `explorer/` as OUTSTANDING.

## If anything failed

Leave it **FAIL** or **UNRESOLVED**. Do not re-run to get a better answer. Tell
me what happened and I will investigate the root cause — that is the loop this
project has followed throughout, and it is what turned up every real defect
found so far.
