# SPACE Phase 1 -- OS safety check (manual section 1.5).
#
# Run after every stress test in Sessions 13-16. Its output is gate evidence.
# Exit code 0 = safe, 1 = at least one check failed.

param([datetime]$Since = (Get-Date).AddHours(-1))

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

$vols = & "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol 2>&1 | Out-String
if ($vols -match "SPACE") { Write-Host "FAIL  stale SPACE volume:`n$vols" -ForegroundColor Red; $fail++ }
else                      { Write-Host "PASS  no stale SPACE volume" -ForegroundColor Green }

if (Get-Process space-client -ErrorAction SilentlyContinue) {
  Write-Host "FAIL  space-client still running" -ForegroundColor Red; $fail++
} else { Write-Host "PASS  no orphaned client process" -ForegroundColor Green }

if (Test-Path "S:\") { Write-Host "FAIL  S: still present" -ForegroundColor Red; $fail++ }
else                 { Write-Host "PASS  S: released" -ForegroundColor Green }

if ($fail -gt 0) { Write-Host "`n$fail OS-safety check(s) FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`nOS safety OK" -ForegroundColor Green
# Deviation from the manual's listing, deliberate: the listing has no explicit
# exit on the success path, so the script inherits $LASTEXITCODE from the last
# native command it ran. `fsptool lsvol` returns 433 (ERROR_NO_SUCH_DEVICE) when
# no volume is mounted -- which is the *passing* case -- so without this line a
# clean run reports failure to CI and to collect-evidence.ps1.
exit 0
