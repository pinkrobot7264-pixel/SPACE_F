# Explorer and GUI-client checklist (Phase 1 §9.2, §16.3)

**These steps require a human at the keyboard.** They are deliberately not
scripted: driving a filesystem API from PowerShell and recording the result in a
cell labelled `Explorer` would put an unearned PASS in the exit gate. The
automated counterparts live in `scripts/mount-functional-test.ps1` (19/19
passing) and `scripts/compatibility-matrix.ps1`.

## Before you start

```powershell
cd C:\SPACE\src\space
cargo build -p space-client
.\target\debug\space-client.exe --config .\config.toml
```

Leave that console open — it prints `SPACE mounted at S:. Press Ctrl-C to
unmount.` and streams the boundary log. Ctrl-C when you are done.

Record the result of each step in the table at the bottom, then run:

```powershell
.\scripts\os-safety-check.ps1
```

## §9.2 — the ten Explorer steps

| # | Step | What to look for |
|---|---|---|
| 1 | Explorer → This PC → open `S:` | The drive appears and the root opens. If `S:` is visible but will not open, the `GetSecurityByName` buffer protocol is wrong (§6.2). |
| 2 | Right-click → New → Folder | The folder is created and can be renamed inline. |
| 3 | Create a text file; open in Notepad; type; save; close; reopen | The content persists. This exercises `FlushAndPurgeOnCleanup`. |
| 4 | Copy a 10 MB file into `S:` | Completes without an error dialog. Watch the progress bar — a stall means a callback is not returning. |
| 5 | Copy it back out under a new name, `Get-FileHash` both | Hashes match. (Also covered automatically.) |
| 6 | Rename a file; rename a folder | Both take effect immediately in the view. |
| 7 | Delete a file; delete a folder | Both disappear. Check the Recycle Bin prompt behaves sanely. |
| 8 | Create 5,000 files in one directory, then `Get-ChildItem S:\many \| Measure-Object` | **Exactly 5,000.** This is the manual counterpart of INV-DIR-2. Correct at 50 and truncated at 5,000 is the classic marker bug. (Automated: passing.) |
| 9 | Create a 10-level nested path and browse to the bottom | Each level opens. |
| 10 | Right-click → Properties | Size and timestamps are sane; the size matches what you wrote. |

**Watch for:** a directory that spins forever without finishing (missing
end-of-enumeration marker), a size column showing nonsense (`space_file_info`
layout drift), or two distinct files showing each other's properties
(`index_number` reuse).

## §16.3 — GUI client rows

| Client | create | read | write | rename | delete | enumerate | properties |
|---|---|---|---|---|---|---|---|
| Explorer | | | | | | | |
| Notepad | | | | — | — | — | — |
| 7-Zip (or similar) | | | | — | — | | — |

For 7-Zip: create an archive **from** files on `S:`, and extract an archive
**into** `S:`. Archivers read and write in unusual patterns and are worth one
pass.

## §13.2 step 3 — Explorer stays responsive under an injected hang

This one needs the fault build:

```powershell
cargo build -p space-client --features fault-injection
$env:SPACE_FAULT = "winfsp_pre_read=hang"
.\target\debug\space-client.exe --config .\config.toml
```

With a read hung, confirm that **Explorer itself stays responsive** — you can
still click around, open other windows, and browse other drives. An error or a
delay on `S:` is correct; a frozen Explorer is a Phase 1 blocker.

Then Ctrl-C and confirm the unmount completes.

## §16.4 — ProcMon evidence (INV-NS-6, external half)

You cannot assert "no writes outside the boundary" from inside your own process,
which is why this half is external.

1. Start Process Monitor.
2. Filter: `Process Name` **is** `space-client.exe`, **and** `Operation` **is**
   `WriteFile`.
3. Run `.\scripts\mount-functional-test.ps1`.
4. Assert the only write targets are `C:\SPACE\runtime\logs\*` — nothing under
   `C:\Windows`, nothing in the source tree, nothing under `C:\Users`.
5. File → Save → CSV → `docs\evidence\phase-1\procmon-writes.csv`.

---

## Results

| Step | Result | Notes |
|---|---|---|
| 9.2.1 Explorer opens S: | | |
| 9.2.2 New folder | | |
| 9.2.3 Notepad round trip | | |
| 9.2.4 Copy 10 MB in | | |
| 9.2.5 Copy out, hash match | | |
| 9.2.6 Rename file / folder | | |
| 9.2.7 Delete file / folder | | |
| 9.2.8 5,000 files enumerate | | |
| 9.2.9 10-level nesting | | |
| 9.2.10 Properties sane | | |
| 16.3 Explorer row | | |
| 16.3 Notepad row | | |
| 16.3 7-Zip row | | |
| 13.2.3 Explorer responsive under hang | | |
| 16.4 ProcMon writes confined | | |

**Environment**

- OS build: `Get-ComputerInfo | Select OsBuildNumber` →
- WinFsp version: `fsptool-x64.exe ver` →
- Commit: `git rev-parse HEAD` →
- Date:
- Tester:
