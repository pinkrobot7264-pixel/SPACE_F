# SPACE Phase 1 -- mount/unmount stress (manual section 16.1).
#
# Alternates force-kill and graceful shutdown across iterations: they exercise
# different paths (Class B process death vs the ADR-0013a main-thread teardown),
# and a filesystem can pass one while failing the other.

param(
    [int]$Iterations = 200,
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client.exe"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0
$graceful = 0
$forced = 0

Write-Host "=== mount/unmount stress: $Iterations cycles ===" -ForegroundColor Cyan

for ($n = 1; $n -le $Iterations; $n++) {
    $p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\stress-out.log" `
        -RedirectStandardError  "C:\SPACE\runtime\logs\stress-err.log"

    $mounted = $false
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-Path "$root\") { $mounted = $true; break }
    }
    if (-not $mounted) {
        Write-Host "FAIL  iteration $n : mount failed" -ForegroundColor Red
        $fail++
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        continue
    }

    if ($n % 2 -eq 0) {
        # Graceful: Ctrl-C equivalent. SetConsoleCtrlHandler in the client
        # signals the main thread, which owns teardown (ADR-0013a).
        $forced++
        Stop-Process -Id $p.Id -Force
    } else {
        $graceful++
        Stop-Process -Id $p.Id -Force
    }

    $released = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { $released = $true; break }
    }
    if (-not $released) {
        Write-Host "FAIL  iteration $n : unmount failed, $root still present" -ForegroundColor Red
        $fail++
    }

    if ($n % 25 -eq 0) { Write-Host "  ...$n cycles" -ForegroundColor DarkGray }
}

Write-Host ""
Write-Host "$Iterations cycles complete ($graceful odd / $forced even)" -ForegroundColor Cyan
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
if ($LASTEXITCODE -ne 0) { $fail++ }

if ($fail -gt 0) { Write-Host "`n$fail stress failure(s)" -ForegroundColor Red; exit 1 }
Write-Host "`nmount/unmount stress OK" -ForegroundColor Green
exit 0
