# SPACE Phase 1 -- kill-while-mounted (manual section 15.2).
#
# What this certifies (ADR-0013 Class B): Windows and the system recover safely
# when the user-mode filesystem process disappears -- no bugcheck, no hang, no
# stale mount, no orphaned process, and a clean remount.
#
# What it does NOT certify: durable filesystem recovery. Phase 1 has no
# persistence by design (section 15.4); after a kill all filesystem content is
# gone and that is the CORRECT outcome. Crash-durability belongs to Phase 5 and
# must not be claimed here.
#
# Run in three states, as the manual requires: idle, mid-write, and
# mid-enumeration of a large directory.

# EXCLUSIVE: this script starts and stops clients. Nothing else may use the
# mount while it runs -- a concurrent test will see its mount vanish mid-call
# and report failures that look like defects and are not. See
# docs/evidence/phase-1/PHASE-1-CERTIFICATION.md, "Harness discipline".

param(
    [int]$Iterations = 20,
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client.exe",
    [string]$Out = "docs\evidence\phase-1\kill-matrix.txt"
)

# This script used to write no evidence file, so its result existed only as
# console output. A run that was killed partway through therefore left nothing
# behind but a half-written console capture in the host's encoding -- which is
# how a completed "idle x20 PASS, mid-write x20 PASS" was lost. Every other
# harness writes its own evidence; this one now does too.

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0
$lines = @()
$lines += "SPACE Phase 1 -- kill-while-mounted (manual section 15.2)"
$lines += "date: $(Get-Date -Format o)"
$lines += "iterations: $Iterations per state, states: idle, mid-write, mid-enumeration"
$lines += "commit: $(git rev-parse HEAD)"
$lines += ""

function Start-Client {
    $p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\kill-out.log" `
        -RedirectStandardError  "C:\SPACE\runtime\logs\kill-err.log"
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-Path "$root\") { return $p }
    }
    throw "mount did not appear within 15s"
}

function Wait-Released {
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { return $true }
    }
    return $false
}

Write-Host "=== kill-while-mounted: $Iterations iterations x 3 states ===" -ForegroundColor Cyan

foreach ($state in @("idle", "mid-write", "mid-enumeration")) {
    Write-Host "--- state: $state ---" -ForegroundColor Cyan
    $stateFail = 0
    $job = $null

    for ($n = 1; $n -le $Iterations; $n++) {
        $p = Start-Client

        switch ($state) {
            "idle" { Start-Sleep -Milliseconds 300 }

            "mid-write" {
                # Start writing and kill while it is in flight.
                $job = Start-Job -ScriptBlock {
                    param($r)
                    1..2000 | ForEach-Object {
                        try { [IO.File]::WriteAllText("$r\stress-$_.txt", "data $_") } catch { }
                    }
                } -ArgumentList $root
                Start-Sleep -Milliseconds 800
            }

            "mid-enumeration" {
                # Build a 5,000-entry directory, then kill mid-listing.
                1..5000 | ForEach-Object {
                    try { [IO.File]::WriteAllText("$root\e$_.txt", "$_") } catch { }
                }
                $job = Start-Job -ScriptBlock {
                    param($r)
                    while ($true) { try { Get-ChildItem $r | Out-Null } catch { } }
                } -ArgumentList $root
                Start-Sleep -Milliseconds 800
            }
        }

        Stop-Process -Id $p.Id -Force
        if ($job) { Stop-Job $job -ErrorAction SilentlyContinue; Remove-Job $job -Force -ErrorAction SilentlyContinue; $job = $null }

        if (-not (Wait-Released)) {
            Write-Host "FAIL  [$state $n] $root was not released after a force kill" -ForegroundColor Red
            $lines += "FAIL  [$state $n] -- $root was not released after a force kill"
            $fail++; $stateFail++
        }

        # The client must be able to mount again immediately.
        $p2 = $null
        try { $p2 = Start-Client } catch {
            Write-Host "FAIL  [$state $n] remount failed: $($_.Exception.Message)" -ForegroundColor Red
            $lines += "FAIL  [$state $n] -- remount failed: $($_.Exception.Message)"
            $fail++; $stateFail++
        }
        if ($p2) {
            Stop-Process -Id $p2.Id -Force
            Wait-Released | Out-Null
        }
    }
    # Conditional. This line was previously printed unconditionally, so a state
    # that recorded failures still logged "PASS <state> x 20" -- and that log is
    # the evidence. The run-level exit code was correct; the transcript was not.
    if ($stateFail -eq 0) {
        Write-Host "PASS  $state x $Iterations" -ForegroundColor Green
        $lines += "PASS  $state x $Iterations -- every kill released the mount and every remount succeeded"
    } else {
        Write-Host "FAIL  $state x $Iterations -- $stateFail failure(s) in this state" -ForegroundColor Red
        $lines += "FAIL  $state x $Iterations -- $stateFail failure(s) in this state"
    }
    # Write after every state, so a run interrupted partway still leaves the
    # states it did finish on disk instead of nothing at all.
    New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
    ($lines + @("", "(run in progress -- states after this one had not started)")) -join "`r`n" |
        Out-File -Encoding utf8 $Out
}

Write-Host ""
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
$osExit = $LASTEXITCODE
if ($osExit -ne 0) { $fail++ }
$lines += ""
$lines += "os-safety-check exit: $osExit"
$lines += ""
$lines += "NOTE: this certifies Class B recovery -- Windows survives the filesystem"
$lines += "process disappearing, with no bugcheck, hang, stale mount or orphan, and a"
$lines += "clean remount. It does NOT certify durable filesystem recovery: Phase 1 has"
$lines += "no persistence by design (section 15.4), so content loss after a kill is the"
$lines += "correct outcome and crash-durability belongs to Phase 5."
$lines += ""
$lines += "result: $(if ($fail -gt 0) { "$fail FAILURE(S)" } else { 'PASS -- all three states' })"
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$lines -join "`r`n" | Out-File -Encoding utf8 $Out

if ($fail -gt 0) {
    Write-Host "`n$fail kill-matrix failure(s) -- see $Out" -ForegroundColor Red
    exit 1
}
Write-Host "`nkill matrix OK -- Windows survived every force kill, no stale mount" -ForegroundColor Green
exit 0
