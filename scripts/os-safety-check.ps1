# SPACE Phase 1 -- OS safety check (manual section 1.5).
#
# Run after every stress test in Sessions 13-16. Its output is gate evidence.
# Exit code 0 = safe, 1 = at least one check failed.

param(
    [datetime]$Since = (Get-Date).AddHours(-1),
    [string]$Drive = "S"
)

$fail = 0
Write-Host "=== OS safety check since $Since ===" -ForegroundColor Cyan

$kp = Get-WinEvent -FilterHashtable @{LogName='System'; Id=41; StartTime=$Since} -ErrorAction SilentlyContinue
if ($kp) { Write-Host "FAIL  Kernel-Power 41 (unexpected shutdown)" -ForegroundColor Red; $fail++ }
else     { Write-Host "PASS  no unexpected shutdown" -ForegroundColor Green }

$bc = Get-WinEvent -FilterHashtable @{LogName='System'; Id=1001; StartTime=$Since} -ErrorAction SilentlyContinue |
      Where-Object { $_.ProviderName -like "*BugCheck*" }
if ($bc) { Write-Host "FAIL  BugCheck recorded" -ForegroundColor Red; $fail++ }
else     { Write-Host "PASS  no bugcheck" -ForegroundColor Green }

if (Test-Path "C:\Windows\MEMORY.DMP") {
  $d = (Get-Item "C:\Windows\MEMORY.DMP").LastWriteTime
  if ($d -gt $Since) { Write-Host "FAIL  kernel dump written $d" -ForegroundColor Red; $fail++ }
}

$sys = Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2; StartTime=$Since} -ErrorAction SilentlyContinue
if ($sys) {
  Write-Host "WARN  $($sys.Count) critical/error system events:" -ForegroundColor Yellow
  $sys | Select-Object TimeCreated, Id, ProviderName, Message -First 10 | Format-List
}

# Deviation from the manual's listing, deliberate and load-bearing: the listing
# greps lsvol output for "SPACE", but fsptool prints the drive letter and the
# device path -- e.g. "S:  \Device\Volume{e3a5ead4-...}" -- and never the
# filesystem name. Grepping for "SPACE" therefore matches nothing and the check
# passes unconditionally, which is worse than not having it: a stale volume is
# exactly what this gate exists to catch. Match the mount point instead.
$mountPoint = "${Drive}:"
$vols = & "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol 2>&1 | Out-String
if ($vols -match [regex]::Escape($mountPoint)) {
  Write-Host "FAIL  stale $mountPoint volume still registered with WinFsp:`n$vols" -ForegroundColor Red; $fail++
} else {
  Write-Host "PASS  no stale $mountPoint volume" -ForegroundColor Green
}

# Wildcard, not the bare name. Get-Process matches exactly without one, so
# `Get-Process space-client` does NOT match space-client-fault.exe -- the
# fault-injection build used by sections 13.3 and 15.1. That is not
# hypothetical: a hung space-client-fault.exe survived a stopped run, held S:
# in a state where even Test-Path blocked, and this check would have reported
# "no orphaned client process" while it was still holding the mount.
$orphans = @(Get-Process space-client* -ErrorAction SilentlyContinue)
if ($orphans.Count -gt 0) {
  Write-Host "FAIL  client process still running: $(($orphans | ForEach-Object { "$($_.ProcessName) (pid $($_.Id))" }) -join ', ')" -ForegroundColor Red
  $fail++
} else { Write-Host "PASS  no orphaned client process" -ForegroundColor Green }

if (Test-Path "${Drive}:\") { Write-Host "FAIL  ${Drive}: still present" -ForegroundColor Red; $fail++ }
else                 { Write-Host "PASS  ${Drive}: released" -ForegroundColor Green }

if ($fail -gt 0) { Write-Host "`n$fail OS-safety check(s) FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`nOS safety OK" -ForegroundColor Green
# Deviation from the manual's listing, deliberate: the listing has no explicit
# exit on the success path, so the script inherits $LASTEXITCODE from the last
# native command it ran. `fsptool lsvol` returns 433 (ERROR_NO_SUCH_DEVICE) when
# no volume is mounted -- which is the *passing* case -- so without this line a
# clean run reports failure to CI and to collect-evidence.ps1.
exit 0
