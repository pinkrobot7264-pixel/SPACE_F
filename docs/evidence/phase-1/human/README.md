# Phase 1 — human validation package

**STATUS: NEEDS HUMAN — none of these has been performed.**

Five validations require a person at the Windows machine. They are prepared
here: what to do, what counts as a pass, and where to put the result. Nothing in
this directory has been executed, simulated, or inferred from automated tests.

No automated result substitutes for any of them. Where an automated test covers
an adjacent claim, that is stated so the human knows what is *not* already
proven.

## Before starting any of these

The machine must be clean, and `S:` must not already be mounted:

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

Unmount when done: stop the `space-client` process, then confirm `S:` is gone
and `lsvol` is empty.

---

## H1 — Explorer, the ten steps (§9.2)

**STATUS: NEEDS HUMAN**

Perform each step in Explorer itself, not from a shell.

| # | step | pass condition |
|---|---|---|
| 1 | Open `S:` in Explorer | volume opens, no error dialog |
| 2 | Create a folder via right-click → New → Folder | folder appears and persists after F5 |
| 3 | Create a text file inside it | file appears |
| 4 | Type into the file, save, close, reopen | content is what was typed |
| 5 | Rename the file | new name shown, old name gone |
| 6 | Copy a >10 MB file in from `C:` | copy completes, size matches source |
| 7 | Right-click → Properties on that file | size, created and modified times are sane |
| 8 | Enumerate a large directory | listing completes, no truncation, no hang |
| 9 | Delete a file, then a folder | both disappear; recycle-bin behaviour noted |
| 10 | Unmount while Explorer has `S:` open | no crash, no Explorer restart, `S:` disappears |

Record: for each step, PASS/FAIL, what was observed, and any dialog text
verbatim. Save as `H1-explorer.md` in this directory.

*Automated coverage that does NOT substitute:* `mount-functional-test.ps1`
performs equivalent filesystem operations programmatically (21/21 PASS). It
proves the filesystem serves those operations; it proves nothing about the
Explorer shell.

---

## H2 — Notepad (§16.3)

**STATUS: NEEDS HUMAN**

1. Open Notepad, type text, Save As to `S:\notepad-test.txt`.
2. Close Notepad, reopen the file from `S:`. Content must match.
3. Append text, save again, reopen. Both parts must be present.
4. Save As over an existing file on `S:` and accept the overwrite prompt.

Record observed behaviour and any error dialog verbatim as `H2-notepad.md`.

*Why a human:* Notepad's save path uses replace-and-rename semantics that the
scripted `.NET`/`cmd` clients in `compatibility-matrix.md` do not exercise.

---

## H3 — 7-Zip (§16.3)

**STATUS: NEEDS HUMAN**

1. Create an archive **from** a folder on `S:`.
2. Extract that archive **to** a folder on `S:`.
3. Compare extracted contents against the originals (7-Zip's own CRC test is
   acceptable, `Get-FileHash` comparison is better).
4. Open an archive stored on `S:` and browse it without extracting.

Record as `H3-7zip.md`, including 7-Zip's version and any reported CRC errors.

---

## H4 — Explorer responsiveness during a hung operation (§13.2 assertion 3)

**STATUS: NEEDS HUMAN**

This is the one assertion of §13.2 that automation cannot make. The other
assertions are proven — see `fault-injection-run-2026-09-10-1946.txt`.

```powershell
cd C:\SPACE\src\space
$env:SPACE_FAULT = "winfsp_pre_read=hang"
Start-Process .\target\debug\space-client-fault.exe -ArgumentList "--config",".\config.toml"
# wait for S: to appear, then create a file on it and try to READ it from Explorer
```

While the read is hung (up to `callback_timeout_ms`, 30 s by default), observe:

- Does Explorer as a whole stay responsive — can you interact with other windows
  and other drives?
- Does the Explorer window showing `S:` grey out or show "Not Responding"?
- After the deadline expires, does an error dialog appear rather than an
  indefinite hang?

Record as `H4-explorer-under-hang.md`. Note the exact wait before the error
appeared. Then stop the client and clear `SPACE_FAULT`.

---

## H5 — ProcMon write confinement (§16.4)

**STATUS: NEEDS HUMAN — currently BLOCKED on this**

1. Start Process Monitor.
2. Filter: `Process Name is space-client.exe`, include `WriteFile`,
   `CreateFile` with write access, `SetDispositionInformationFile`.
3. With that filter live, exercise the mount: create, write, rename and delete
   files on `S:`.
4. Inspect every captured write path.

**Pass condition:** no write outside `C:\SPACE\runtime` and the mounted volume
itself. Specifically none to `C:\Windows`, none to the user profile, none to
`Program Files`.

Export the filtered capture as `procmon-writes.csv` into
`docs/evidence/phase-1/` — `collect-evidence.ps1` looks for it by that exact
name and currently reports it OUTSTANDING.

Record conclusions as `H5-procmon.md`.

---

## What to do with the results

Drop the five `.md` files (and `procmon-writes.csv`) into place, then re-run:

```powershell
cd C:\SPACE\src\space
.\scripts\collect-evidence.ps1
```

It will pick up `procmon-writes.csv` and stop reporting it as outstanding. The
five requirement rows in `REQUIREMENTS.md` — R-S4-5, R-S9-2, R-S13-3 (Explorer
assertion), R-S16-3 (GUI rows) and R-S16-4 — should then be updated **by the
person who performed the tests**, with their observations as the evidence.
