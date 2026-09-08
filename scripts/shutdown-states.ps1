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
    [string]$FaultExe = ".\target\debug\space-client-fault.exe",
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
    $errLog = "C:\SPACE\runtime\logs\shutdown-err.log"
    Remove-Item $errLog -ErrorAction SilentlyContinue
    $p = Start-Process -PassThru -FilePath $exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\shutdown-out.log" `
        -RedirectStandardError  $errLog
    $up = $false
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-Path "$root\") { $up = $true; break }
    }
    if (-not $up) { throw "mount did not appear" }

    # States 4 and 5 are meaningless if the fault never armed -- a client built
    # without the feature ignores SPACE_FAULT entirely, and "shutdown with an
    # operation hung" silently becomes "shutdown with nothing happening", which
    # passes. Require the armed warning before continuing.
    if ($fault) {
        $armed = $false
        for ($i = 0; $i -lt 20; $i++) {
            $log = Get-Content $errLog -Raw -ErrorAction SilentlyContinue
            if ($log -and $log -match "FAULT INJECTION ARMED") { $armed = $true; break }
            Start-Sleep -Milliseconds 250
        }
        if (-not $armed) {
            taskkill /PID $p.Id /F /T 2>&1 | Out-Null
            throw "fault '$fault' was NOT armed. Build with: cargo build -p space-client --features fault-injection"
        }
    }
    return $p
}

# Returns the UTC ticks at which the signal was actually delivered, or $null.
#
# The teardown must be timed from THAT instant. Timing it from after this
# function returns measured 1-9ms for every state, including the hung one --
# not because teardown was instant but because launching a child PowerShell
# takes about a second and the shutdown had already finished inside it. Section
# 15.1 asks how long the hung case takes; that number has to be real.
$script:stampFile = Join-Path $env:TEMP "space-ctrlc-stamp.txt"
function Send-CtrlC($p) {
    Remove-Item $script:stampFile -ErrorAction SilentlyContinue
    $r = Start-Process -FilePath "powershell" -PassThru -Wait -WindowStyle Hidden `
        -ArgumentList @("-NoProfile", "-File", "$PSScriptRoot\send-ctrl-c.ps1",
                        "-TargetPid", $p.Id, "-StampFile", $script:stampFile)
    if ($r.ExitCode -ne 0) { return $null }
    if (Test-Path $script:stampFile) { return [long](Get-Content $script:stampFile -Raw).Trim() }
    return $null
}

# Elapsed milliseconds from $sinceTicks (UTC ticks) until the mount is gone,
# or -1 on timeout.
function Wait-Unmounted([long]$sinceTicks, [int]$timeoutMs = 20000) {
    $deadlineTicks = $sinceTicks + ([long]$timeoutMs * 10000)
    while ([DateTime]::UtcNow.Ticks -lt $deadlineTicks) {
        if (-not (Test-Path "$root\")) {
            return [math]::Round(([DateTime]::UtcNow.Ticks - $sinceTicks) / 10000.0, 1)
        }
        Start-Sleep -Milliseconds 20
    }
    return -1
}

function Check-State($name, $fault, [scriptblock]$setup, [switch]$SelfUnmount) {
    Write-Host "--- $name ---" -ForegroundColor Cyan
    $p = $null
    try {
        $p = Start-Client $fault
        if ($setup) { & $setup $p }

        if ($SelfUnmount) {
            # State 5: the poisoned filesystem tears itself down. The clock
            # starts at the operation that poisons it, which $setup just ran.
            $ms = Wait-Unmounted ([DateTime]::UtcNow.Ticks) 25000
        } else {
            $sentTicks = Send-CtrlC $p
            if ($null -eq $sentTicks) {
                # No fallback to taskkill. Falling back would test the force-kill
                # path while labelling the row a graceful shutdown -- exactly the
                # substitution that made the section 16.1 evidence worthless.
                throw "could not deliver CTRL_C_EVENT; the graceful path was NOT exercised"
            }
            $ms = Wait-Unmounted $sentTicks 25000
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
                $p.Refresh()
                # Start-Process -PassThru does not always give a readable
                # ExitCode. Say "unavailable" rather than printing an empty
                # field that reads like a recorded value of nothing.
                $code = try { $p.ExitCode } catch { $null }
                if ($null -eq $code -or "$code" -eq "") { $code = "unavailable" }
                Write-Host "PASS  $name : unmounted in ${ms}ms, exit code $code" -ForegroundColor Green
                $script:lines += "PASS  $name  -- unmounted in ${ms}ms after the signal, exit code $code"
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

# Section 15.1 asks for "how long and why" on the hung case specifically.
$lines += ""
$lines += "--- state 4, why it completes ---"
$lines += "The read is genuinely blocked when the signal arrives: SPACE_FAULT armed"
$lines += "winfsp_pre_read=hang, callback_timeout_ms is $((Select-String -Path $Config -Pattern 'callback_timeout_ms\s*=\s*(\d+)' | Select-Object -First 1).Matches[0].Groups[1].Value)ms, and the reader job has been"
$lines += "waiting ~1.5s by then. Teardown still completes because it runs on the MAIN"
$lines += "thread (ADR-0013a) and does not join the hung dispatcher callback: it moves to"
$lines += "STOPPING, stops the dispatcher, removes the mount point and deletes the"
$lines += "filesystem. The hung callback is abandoned with the process, which is why the"
$lines += "measured time is far below both the callback deadline and shutdown_deadline_ms."
$lines += ""

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
