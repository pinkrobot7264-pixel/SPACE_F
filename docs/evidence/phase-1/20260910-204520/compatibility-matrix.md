# Windows compatibility matrix (Phase 1 section 16.3)

**Observed behaviour, not expected behaviour.**

- Generated: 2026-09-10T09:00:36.3663638+02:00
- OS: Microsoft Windows 11 Pro (build 26200)
- WinFsp: 2.1 (SxS=20260901T102116Z)
- Mount: S:

| Client | create | read | write | rename | delete | enumerate | properties | notes |
|---|---|---|---|---|---|---|---|---|
| PowerShell | ok | ok | ok | ok | ok | ok | ok |  |
| cmd.exe | ok | ok | ok | ok | ok | ok | ok |  |
| copy | ok | ok | -- | -- | ok | -- | -- |  |
| xcopy | ok | -- | -- | -- | -- | -- | -- |  |
| robocopy | ok | ok | -- | -- | ok | ok | -- |  |
| .NET System.IO | ok | ok | ok | ok | ok | ok | ok |  |
| Explorer | \<human\> | \<human\> | \<human\> | \<human\> | \<human\> | \<human\> | \<human\> | GUI client -- see EXPLORER-CHECKLIST.md |
| Notepad | \<human\> | \<human\> | \<human\> | -- | -- | -- | -- | GUI client -- see EXPLORER-CHECKLIST.md |
| 7-Zip | \<human\> | \<human\> | \<human\> | -- | -- | \<human\> | -- | GUI client -- see EXPLORER-CHECKLIST.md |

-- means the operation does not apply to that client: copy, xcopy and
obocopy have no in-place write or rename of their own, and Notepad and
7-Zip do not rename or enumerate as filesystem clients. It does **not** mean
"not tested" -- every applicable cell above was executed.

GUI rows are deliberately left for a human. Driving Explorer from a
script would put a filesystem call in a cell labelled `Explorer`,
which is an unearned PASS in the exit gate.

**Scripted result: 29 ok, 0 failed.**
