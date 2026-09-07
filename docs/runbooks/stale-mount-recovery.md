# Runbook — stale mount recovery (Phase 1 §15.3)

**You will want this at 11pm in Phase 5.** Read the whole page before typing;
the destructive steps are at the bottom for a reason.

A "stale mount" is any state where `S:` or its WinFsp volume outlives the
process that served it. Phase 1 has no durable state, so **nothing is lost by
tearing a stale mount down** — the only cost is the running process.

---

## 1. Establish what is actually stale

Run the safety check first. It distinguishes the four states that look identical
from Explorer:

```powershell
.\scripts\os-safety-check.ps1
```

| Check | Meaning when it FAILs |
|---|---|
| `stale S: volume still registered with WinFsp` | the FSD still has a volume; a process may or may not be serving it |
| `space-client still running` | our process is alive — this is an orphan, not a stale mount |
| `S: still present` | the drive letter resolves |

Then look directly:

```powershell
Get-PSDrive S
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol
Get-Process space-client | Select-Object Id, StartTime, WorkingSet
```

`fsptool lsvol` prints the **mount point and device path**, e.g.
`S:  \Device\Volume{e3a5ead4-...}`. It does **not** print the filesystem name —
which is why the safety check matches on the drive letter. If you find yourself
grepping its output for `SPACE`, you will match nothing and conclude wrongly
that all is well.

---

## 2. The ordinary case — an orphaned client process

**Symptom:** `space-client` is running, `S:` works, but nobody meant it to be up
(a test left it behind).

```powershell
Get-Process space-client | Stop-Process -Force
Start-Sleep -Seconds 2
.\scripts\os-safety-check.ps1
```

Force-killing is safe and is the **certified** recovery path (ADR-0013 Class B,
§15.2): the process dies, the FSD observes the termination, and the volume is
torn down. This is exercised 20 times by the kill matrix and is expected to
leave the machine clean. If `os-safety-check.ps1` passes afterwards, you are
done.

Prefer Ctrl-C in the client's own console when you have one — it takes the
graceful path (STOPPING → unmount → `space_core_stop`) and logs a clean
shutdown line. Both paths are tested; neither is dangerous.

---

## 3. The volume outlived the process

**Symptom:** no `space-client` process, but `fsptool lsvol` still lists `S:`, or
`S:` still appears in Explorer.

1. Give the FSD a few seconds. Teardown is not instantaneous, and the kill tests
   allow for it.
2. Close anything holding a handle on `S:` — an Explorer window sitting in the
   directory, a shell whose working directory is `S:\`, an editor with a file
   open. **A handle held by another process is the most common reason a volume
   lingers.** Process Explorer → Find → Handle or DLL → `S:\` names the culprit.
3. Re-check:

```powershell
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol
```

---

## 4. When a WinFsp service restart is warranted

Only after steps 2 and 3, and only when `lsvol` still lists the volume with **no
owning process and no open handles**. This restarts the FSD for *every* WinFsp
filesystem on the machine, so check that nothing else is mounted first.

```powershell
# Requires an elevated shell.
Get-Service WinFsp.Launcher | Restart-Service
```

If the volume persists after that, the FSD holds kernel state that only a reboot
clears. Reboot; do not go looking for a driver-level workaround.

---

## 5. What NOT to do

- **Do not** `subst /d S:` or otherwise fight the drive letter. The letter is a
  symptom; the volume is the thing.
- **Do not** delete anything under `C:\SPACE\runtime` to "reset" a stale mount.
  It has no bearing on the mount, and the logs there are the evidence you will
  want in the next step.
- **Do not** assume data loss. Phase 1 is in-memory by design (§15.4): a killed
  process losing all filesystem content is the **correct** outcome, not a
  failure. Do not go looking for a recovery mechanism that Phase 1 deliberately
  does not have. Crash-durability arrives in Phase 5.

---

## 6. Before you close the incident

Capture the evidence while it is still there:

```powershell
.\scripts\os-safety-check.ps1 > stale-mount-$(Get-Date -Format yyyyMMdd-HHmmss).txt 2>&1
Copy-Item C:\SPACE\runtime\logs\*.log .\incident\
```

Then look for the last `operation` lines before the process disappeared. Every
boundary line carries a `request_id`, a `handle` and a `path` (§12.1), so the
final callback the filesystem served is identifiable. If the last line is
`PANIC at FFI boundary`, this was a Class A failure and the poison path did its
job — the `request_id` on that line is the thread to pull.
