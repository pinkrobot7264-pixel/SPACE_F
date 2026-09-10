# Phase 1 — human validation package

**STATUS: NEEDS HUMAN — none of these has been performed.**

Five validations require a person at the Windows machine. Nothing here has been
executed, simulated, or inferred from automated tests.

**Every step below is taken from the authoritative manual**, now committed at
`SPACE_Phase_1_Execution_Manual_FINAL.md`. Sections are cited so a reader can
check the wording rather than trusting this file. An earlier version of this
package was written from memory and was **looser than the manual in two places**
— the Explorer steps and, more seriously, the ProcMon pass condition. Both are
corrected here.

These map onto the exit gates as follows:

| gate | bullet | covered by |
|---|---|---|
| §17.2 Windows safety | Explorer never becomes permanently unresponsive, including during injected hangs | **H4** |
| §17.2 Windows safety | ProcMon evidence: writes confined to `runtime\logs` | **H5** |
| §17.5 Functional | Explorer opens `S:` and enumerates the root | **H1** |
| §17.5 Functional | Create/open/read/write/close/delete from Explorer, PowerShell, Notepad | **H1**, **H2** |
| §17.5 Functional | Path traversal cannot escape the namespace — logically **and** by ProcMon | **H5** |
| §17.7 Engineering | Compatibility matrix complete | **H1**, **H2**, **H3** |

§17.2 states: **any single failure blocks Phase 1.**

## Before starting

```powershell
cd C:\SPACE\src\space
Get-Process space-client* -ErrorAction SilentlyContinue     # expect none
Test-Path "S:\"                                             # expect False
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol # expect empty
```

Mount for H1, H2, H3, H5:

```powershell
Start-Process .\target\release\space-client.exe -ArgumentList "--config",".\config.toml"
```

Stop the `space-client` process when done; confirm `S:` is gone and `lsvol` is empty.

---

## H1 — Explorer evidence (§9.2) — the ten steps verbatim

**STATUS: NEEDS HUMAN**

Perform in Explorer itself, not from a shell. Mount, then:

1. Explorer → `S:` opens and shows the root
2. Right-click → New → Folder
3. Create a text file; open in Notepad; type; save; close; reopen — content persists
4. Copy a 10 MB file into `S:`
5. Copy it back out under a new name; `Get-FileHash` both and compare
6. Rename a file; rename a folder
7. Delete a file; delete a folder
8. **Create 5,000 files in one directory; `Get-ChildItem S:\many | Measure-Object` returns exactly 5,000**
9. Create a 10-level nested path and browse to the bottom
10. Right-click → Properties — size and timestamps sane

**Evidence goes to `docs/evidence/phase-1/explorer/`** — that exact directory;
`collect-evidence.ps1` looks for it and currently reports it OUTSTANDING.

The manual notes: *step 8 is the manual counterpart of INV-DIR-2; correct at 50
entries and truncated at 5,000 is the classic marker bug.* Record the exact
count returned, not "looked fine".

*Automated coverage that does NOT substitute:* `mount-functional-test.ps1` does
equivalent operations programmatically (21/21 PASS). That proves the filesystem
serves them. It proves nothing about the Explorer shell, which is what §9.2 and
§17.5 ask for.

---

## H2 — Notepad (§16.3)

**STATUS: NEEDS HUMAN**

Row `Notepad` of the §16.3 matrix: create, read, write, rename, delete,
enumerate, properties — record **observed behaviour, not expected**.

1. Open Notepad, type text, Save As to `S:\notepad-test.txt`.
2. Close Notepad, reopen from `S:`. Content must match.
3. Append, save, reopen. Both parts present.
4. Save As over an existing file on `S:`; accept the overwrite prompt.

Record verbatim error dialog text if any. File as `H2-notepad.md`, and add the
row to `docs/evidence/phase-1/compatibility-matrix.md`.

*Why a human:* Notepad saves via replace-and-rename, which the scripted `.NET`
and `cmd` clients do not exercise.

---

## H3 — 7-Zip or similar (§16.3)

**STATUS: NEEDS HUMAN**

Row `7-Zip or similar` of the §16.3 matrix.

1. Create an archive **from** a folder on `S:`.
2. Extract that archive **to** a folder on `S:`.
3. Verify extracted contents against the originals (7-Zip CRC test acceptable;
   `Get-FileHash` comparison better).
4. Open an archive stored on `S:` and browse it without extracting.

Record 7-Zip version and any CRC errors. File as `H3-7zip.md`, and add the row
to the compatibility matrix.

---

## H4 — Explorer responsiveness during an injected hang (§13.2 point 3)

**STATUS: NEEDS HUMAN**

This is the one point of §13.2 automation cannot make. Points 1, 2, 4, 5, 6 and
7 are proven — see `fault-injection.txt` (31 of 32 assertions, all six fault
points, max callback 30011 ms against a 30500 ms bound).

```powershell
cd C:\SPACE\src\space
$env:SPACE_FAULT = "winfsp_pre_read=hang"
Start-Process .\target\debug\space-client-fault.exe -ArgumentList "--config",".\config.toml"
# wait for S:, create a file on it, then try to READ it from Explorer
```

While the read is hung (up to `callback_timeout_ms`, 30 s), observe:

- Does Explorer stay responsive — other windows, other drives usable?
- Does the window showing `S:` grey out or say "Not Responding"?
- After the deadline, does an error dialog appear rather than an indefinite hang?

§17.2's bullet is *"Explorer never becomes **permanently** unresponsive"* —
temporary blocking of the `S:` window is expected under §3.6; permanent is a
gate failure. Record the exact wait before the error appeared.

File as `H4-explorer-under-hang.md`. Then stop the client and clear `SPACE_FAULT`.

---

## H5 — No-arbitrary-writes evidence (§16.4, INV-NS-6 external half)

**STATUS: NEEDS HUMAN — §16.4 is currently BLOCKED on this**

Follow §16.4 exactly. The pass condition is narrower than "somewhere sensible".

1. Start Process Monitor.
2. Filter: **`Process Name is space-client.exe`** *and* **`Operation is WriteFile`**.
   (Only `WriteFile` — that is what the manual specifies.)
3. **Run the full I/O stress** with the filter live:
   ```powershell
   .\scripts\io-stress.ps1 -Minutes 30 -Drive S
   ```
4. Inspect every captured write path.

**Pass condition, verbatim from §16.4:** the only write targets are
`C:\SPACE\runtime\logs\*` — **nothing under `C:\Windows`, nothing in the source
tree, nothing under `C:\Users`.**

> Correction to the earlier draft of this file: it said "no write outside
> `C:\SPACE\runtime` **and the mounted volume**". That is looser than the manual
> and would have passed a run the manual fails. The manual says
> `C:\SPACE\runtime\logs\*`.

Export the filtered capture as CSV to **`docs/evidence/phase-1/procmon-writes.csv`**
— that exact path and name; `collect-evidence.ps1` looks for it and currently
reports it OUTSTANDING.

File conclusions as `H5-procmon.md`. This also supplies the ProcMon half of
§17.5's *"path traversal cannot escape the namespace — logically and by
ProcMon"*; the logical half is already proven by `conformance/naming.rs`.

---

## After the five are done

```powershell
cd C:\SPACE\src\space
.\scripts\collect-evidence.ps1
```

It will pick up `procmon-writes.csv` and `explorer/` and stop reporting them
OUTSTANDING. Then update these rows in `REQUIREMENTS.md` — **by the person who
performed the tests**, with their observations as the evidence: R-S4-5, R-S9-2,
R-S13-3 (Explorer point only), R-S16-3 (GUI rows) and R-S16-4.
