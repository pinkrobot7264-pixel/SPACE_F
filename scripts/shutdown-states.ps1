# SPACE Phase 1 -- clean shutdown in all five states (manual section 15.1).
#
# EXCLUSIVE: this script starts and stops clients. Nothing else may use the
# mount while it runs.
#
# Section 15.1 requires the main-thread teardown path
#   STOPPING -> stop dispatcher -> remove mount point -> delete filesystem
#   -> space_core_stop() -> UNMOUNTED
# to be exercised with:
#   1. no open handles
#   2. 10 open handles
#   3. an operation in flight
#   4. an operation hung via a fault point (unmount must still complete;
#      record how long and why)
#   5. a poisoned filesystem (ADR-0013a)
#
# States 1-4 need a GRACEFUL shutdown, which is the Ctrl-C path. State 5 needs
# none: the poisoned filesystem signals its own main thread and tears itself
# down, which is the whole point of ADR-0013a.

param(
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client.exe",
    [string]$FaultExe = ".\target\debug\space-client.exe",
    [string]$Out = "docs\evidence\phase-1\shutdown-states.txt"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0
$lines = @()
$lines += "SPACE Phase 1 -- clean shutdown states (manual section 15.1)"
$lines += "date: $(Get-Date -Format o)"
$lines += ""

# Ctrl-C delivery lives in scripts/send-ctrl-c.ps1 and MUST run as an isolated
# child process -- see the comment there for why.

function Start-Client([string]$fault) {
    if ($fault) { $env:SPACE_FAULT = $fault } else { Remove-Item Env:\SPACE_FAULT -ErrorAction SilentlyContinue }
    $exe = if ($fault) { $FaultExe } else { $Exe }
    $p = Start-Process -PassThru -FilePath $exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\shutdown-out.log" `
        -RedirectStandardError  "C:\SPACE\runtime\logs\shutdown-err.log"
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-Path "$root\") { return $p }
    }
    throw "mount did not appear"
}

function Send-CtrlC($p) {
    $r = Start-Process -FilePath "powershell" -PassThru -Wait -WindowStyle Hidden `
        -ArgumentList @("-NoProfile", "-File", "$PSScriptRoot\send-ctrl-c.ps1", "-TargetPid", $p.Id)
    return ($r.ExitCode -eq 0)
}

function Wait-Unmounted([int]$timeoutMs = 20000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        if (-not (Test-Path "$root\")) { $sw.Stop(); return $sw.ElapsedMilliseconds }
        Start-Sleep -Milliseconds 100
    }
    $sw.Stop()
    return -1
}

function Check-State($name, $fault, [scriptblock]$setup, [switch]$SelfUnmount) {
    Write-Host "--- $name ---" -ForegroundColor Cyan
    $p = $null
    try {
        $p = Start-Client $fault
        if ($setup) { & $setup $p }

        if ($SelfUnmount) {
            # State 5: the poisoned filesystem tears itself down.
            $ms = Wait-Unmounted 25000
        } else {
            $sent = Send-CtrlC $p
            if (-not $sent) {
                Write-Host "  WARN  could not attach to console; falling back to graceful taskkill" -ForegroundColor Yellow
                taskkill /PID $p.Id 2>&1 | Out-Null
            }
            $ms = Wait-Unmounted 25000
        }

        if ($ms -lt 0) {
            Write-Host "FAIL  $name : $root still present after 25s" -ForegroundColor Red
            $script:lines += "FAIL  $name  -- mount not released within 25s"
            $script:fail++
            taskkill /PID $p.Id /F /T 2>&1 | Out-Null
        } else {
            # The process must actually exit, not just drop the mount.
            $exited = $p.WaitForExit(15000)
            if (-not $exited) {
                Write-Host "FAIL  $name : unmounted but process did not exit" -ForegroundColor Red
                $script:lines += "FAIL  $name  -- unmounted in ${ms}ms but process did not exit"
                $script:fail++
                taskkill /PID $p.Id /F /T 2>&1 | Out-Null
            } else {
                Write-Host "PASS  $name : unmounted in ${ms}ms, exit code $($p.ExitCode)" -ForegroundColor Green
                $script:lines += "PASS  $name  -- unmounted in ${ms}ms, exit code $($p.ExitCode)"
            }
        }
    } catch {
        Write-Host "FAIL  $name : $($_.Exception.Message)" -ForegroundColor Red
        $script:lines += "FAIL  $name  -- $($_.Exception.Message)"
        $script:fail++
        if ($p) { taskkill /PID $p.Id /F /T 2>&1 | Out-Null }
    } finally {
        Remove-Item Env:\SPACE_FAULT -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 500
    }
}

Write-Host "=== section 15.1 clean shutdown, five states ===" -ForegroundColor Cyan

# --- 1. no open handles -------------------------------------------------
Check-State "state 1: no open handles" $null $null

# --- 2. ten open handles ------------------------------------------------
$script:held = @()
Check-State "state 2: 10 open handles" $null {
    param($p)
    [IO.File]::WriteAllText("$root\sd.txt", "shutdown")
    $script:held = 1..10 | ForEach-Object {
        [IO.File]::Open("$root\sd.txt", [IO.FileMode]::Open, [IO.FileAccess]::Read,
                        [IO.FileShare]::ReadWrite)
    }
}
foreach ($h in $script:held) { try { $h.Dispose() } catch {} }
$script:held = @()

# --- 3. an operation in flight ------------------------------------------
Check-State "state 3: operation in flight" $null {
    param($p)
    New-Item -ItemType Directory -Path "$root\inflight" -Force | Out-Null
    Start-Job -Name spaceInflight -ScriptBlock {
        param($r)
        $i = 0
        while ($true) {
            $i++
            try { [IO.File]::WriteAllText("$r\inflight\f$i.txt", ("x" * 4096)) } catch { break }
        }
    } -ArgumentList $root | Out-Null
    Start-Sleep -Milliseconds 1200
}
Get-Job -Name spaceInflight -ErrorAction SilentlyContinue | Stop-Job -ErrorAction SilentlyContinue
Get-Job -Name spaceInflight -ErrorAction SilentlyContinue | Remove-Job -Force -ErrorAction SilentlyContinue

# --- 4. an operation hung via a fault point -----------------------------
# "unmount must still complete; record how long and why"
Check-State "state 4: operation hung via fault point" "winfsp_pre_read=hang" {
    param($p)
    [IO.File]::WriteAllText("$root\hung.txt", "payload")
    Start-Job -Name spaceHung -ScriptBlock {
        param($r)
        try { Get-Content "$r\hung.txt" -ErrorAction SilentlyContinue | Out-Null } catch {}
    } -ArgumentList $root | Out-Null
    Start-Sleep -Milliseconds 1500
}
Get-Job -Name spaceHung -ErrorAction SilentlyContinue | Stop-Job -ErrorAction SilentlyContinue
Get-Job -Name spaceHung -ErrorAction SilentlyContinue | Remove-Job -Force -ErrorAction SilentlyContinue

# --- 5. a poisoned filesystem (ADR-0013a) --------------------------------
Check-State "state 5: poisoned filesystem" "winfsp_pre_read=panic" {
    param($p)
    [IO.File]::WriteAllText("$root\poison.txt", "payload")
    try { Get-Content "$root\poison.txt" -ErrorAction SilentlyContinue | Out-Null } catch {}
    Start-Sleep -Milliseconds 500
} -SelfUnmount

# State 5 extras: the log must carry the contained panic, and cleanup/close
# must have been silent no-ops rather than touching poisoned state.
$plog = Get-Content "C:\SPACE\runtime\logs\shutdown-err.log" -Raw -ErrorAction SilentlyContinue
if ($plog -and $plog -match "PANIC at FFI boundary") {
    Write-Host "PASS  state 5: panic logged at the boundary" -ForegroundColor Green
    $lines += "PASS  state 5 detail -- 'PANIC at FFI boundary' present in the log"
} else {
    Write-Host "FAIL  state 5: no PANIC line in the log" -ForegroundColor Red
    $lines += "FAIL  state 5 detail -- no PANIC line in the log"
    $fail++
}
if ($plog -and $plog -match "poisoned") {
    $lines += "PASS  state 5 detail -- shutdown recorded reason=poisoned"
}

Write-Host ""
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
$osExit = $LASTEXITCODE
$lines += ""
$lines += "os-safety-check exit: $osExit"
if ($osExit -ne 0) { $fail++ }

$lines += ""
$lines += "result: $(if ($fail -gt 0) { "$fail FAILURE(S)" } else { 'all five states PASS' })"
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$lines -join "`r`n" | Out-File -Encoding utf8 $Out

Write-Host ""
if ($fail -gt 0) { Write-Host "$fail shutdown-state failure(s)" -ForegroundColor Red; exit 1 }
Write-Host "all five shutdown states PASS" -ForegroundColor Green
exit 0
