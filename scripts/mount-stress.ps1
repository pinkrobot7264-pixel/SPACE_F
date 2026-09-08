# SPACE Phase 1 -- mount/unmount stress (manual section 16.1).
#
# Alternates force-kill and graceful shutdown across iterations: they exercise
# different paths (Class B process death vs the ADR-0013a main-thread teardown),
# and a filesystem can pass one while failing the other.

# EXCLUSIVE: this script starts and stops clients. Nothing else may use the
# mount while it runs -- a concurrent test will see its mount vanish mid-call
# and report failures that look like defects and are not. See
# docs/evidence/phase-1/PHASE-1-CERTIFICATION.md, "Harness discipline".

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
        # Graceful: Ctrl-C. SetConsoleCtrlHandler in the client signals the
        # main thread, which owns teardown (ADR-0013a).
        #
        # This branch previously called Stop-Process -Force, identical to the
        # branch below, while still counting itself as "graceful" -- so the
        # script reported a 100/100 split having performed 200 force kills.
        # The two paths are genuinely different (ADR-0013a main-thread teardown
        # vs Class B process death) and a filesystem can pass one and fail the
        # other, which is the entire point of section 16.1 alternating them.
        $graceful++
        $r = Start-Process -FilePath "powershell" -PassThru -Wait -WindowStyle Hidden `
            -ArgumentList @("-NoProfile", "-File", "$PSScriptRoot\send-ctrl-c.ps1", "-TargetPid", $p.Id)
        if ($r.ExitCode -ne 0) {
            Write-Host "FAIL  iteration $n : could not deliver Ctrl-C (graceful path untested)" -ForegroundColor Red
            $fail++
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        }
    } else {
        # Forced: Class B process death, no teardown runs at all.
        $forced++
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
Write-Host "$Iterations cycles complete ($graceful graceful Ctrl-C / $forced forced kill)" -ForegroundColor Cyan
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
if ($LASTEXITCODE -ne 0) { $fail++ }

if ($fail -gt 0) { Write-Host "`n$fail stress failure(s)" -ForegroundColor Red; exit 1 }
Write-Host "`nmount/unmount stress OK" -ForegroundColor Green
exit 0
