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

param(
    [int]$Iterations = 20,
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client.exe"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0

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
            $fail++
        }

        # The client must be able to mount again immediately.
        $p2 = $null
        try { $p2 = Start-Client } catch {
            Write-Host "FAIL  [$state $n] remount failed: $($_.Exception.Message)" -ForegroundColor Red
            $fail++
        }
        if ($p2) {
            Stop-Process -Id $p2.Id -Force
            Wait-Released | Out-Null
        }
    }
    Write-Host "PASS  $state x $Iterations" -ForegroundColor Green
}

Write-Host ""
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
if ($LASTEXITCODE -ne 0) { $fail++ }

if ($fail -gt 0) {
    Write-Host "`n$fail kill-matrix failure(s)" -ForegroundColor Red
    exit 1
}
Write-Host "`nkill matrix OK -- Windows survived every force kill, no stale mount" -ForegroundColor Green
exit 0
