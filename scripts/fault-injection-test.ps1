# SPACE Phase 1 -- fault injection through a real mount (manual sections 13.2, 13.3).
#
# EXCLUSIVE: this script starts and stops clients. Nothing else may use the
# mount while it runs.
#
# The in-process half lives in client/core/src/ffi/fault_tests.rs. This script
# covers the half that is a claim about WINDOWS and cannot be asserted from
# inside the process:
#
#   1. the callback returns within callback_timeout_ms + 500 ms
#   2. the issuing application receives a controlled error, not a hang
#   3. Explorer stays responsive                       [HUMAN -- see checklist]
#   4. OTHER operations return OperationTimeout rather than hanging -- the
#      section 3.6 model PREDICTS this; here it is confirmed
#   5. unmount still succeeds
#   6. check_invariants() passes afterwards            [in-process]
#   7. os-safety-check.ps1 passes
#
# Section 13.3 requires the hang to be repeated for read, write, open, readdir,
# getinfo and rename, and then the whole bound re-measured with a SMALLER
# callback_timeout_ms to prove it scales with configuration rather than being an
# artefact of fast operations.
#
# Requires a client built with --features fault-injection.
#
# --------------------------------------------------------------------------
# WHAT ASSERTION 1 MEASURES, and why it is not the obvious thing
#
# Assertion 1 is about the CALLBACK, so it is measured on the callback, from
# the client's own boundary log (`duration_ms` on the faulted operation).
#
# An earlier version timed the application-visible operation instead --
# stopwatch around `Get-Content` -- and reported FAIL at 60873ms against a
# 30500ms bound. That was not a deadline defect. Windows RETRIES a request that
# fails this way, and the log showed each individual callback returning in
# 30000, 30004, 30006, 30007, 30008 ms: every one inside the bound, several of
# them per application call. Timing the application measures
# `retries x deadline` and can never satisfy a per-callback bound.
#
# The application-visible time and the retry count are still recorded, as
# context rather than as the pass condition, because "the application waited a
# minute" is worth knowing even when every callback behaved.
# --------------------------------------------------------------------------

param(
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client-fault.exe",
    [string]$Out = "docs\evidence\phase-1\fault-injection.txt",
    # Wall-clock budget for a faulted client to present a usable mount.
    # 240s is generous against a 30s callback deadline: a mount needs a handful
    # of callbacks, so a point that merely slows startup still fits, while a
    # point that makes the volume permanently unusable fails fast instead of
    # grinding for hours.
    [int]$MountWaitSeconds = 240
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0
$errLog = "C:\SPACE\runtime\logs\fault-err.log"

# This harness REQUIRES debug-level client logging and says so explicitly.
#
# `logging.level` is now honoured (it used to be dead configuration), and the
# shipped config is "info". Assertion 1 survives that -- a faulted callback is
# logged at WARN -- but assertion 4, "no callback anywhere in the run exceeded
# the bound", reads `duration_ms` from the per-operation "operation served"
# line, which is DEBUG. At info level those lines do not exist, so the
# assertion would quietly narrow to WARN/ERROR events only and still report
# PASS. That is the exact failure mode this file has already been through once.
#
# So derive a debug-level config rather than depending on whatever the shipped
# one happens to say. Every other harness is fine at info: mount-stress and
# shutdown-states read "clean shutdown" (INFO), "FAULT INJECTION ARMED" (WARN)
# and "PANIC at FFI boundary" (ERROR).
$debugCfg = Join-Path $env:TEMP "space-fault-debuglog.toml"
(Get-Content $Config -Raw) -replace '(?m)^\s*level\s*=\s*"[a-z]+"', 'level = "debug"' |
    Out-File -Encoding utf8 $debugCfg
$Config = $debugCfg
$lines0 = Select-String -Path $Config -Pattern '^\s*level\s*=' | Select-Object -First 1
Write-Host "client logging level for this run: $($lines0.Line.Trim())" -ForegroundColor DarkGray
$lines = @()
$lines += "SPACE Phase 1 -- fault injection through a live mount (sections 13.2, 13.3)"
$lines += "date: $(Get-Date -Format o)"
$lines += "assertion 1 is measured on the CALLBACK (boundary-log duration_ms), not on the"
$lines += "application call -- Windows retries, so the application call is retries x deadline."
$lines += ""

function Get-TimeoutMs([string]$cfg) {
    $m = Select-String -Path $cfg -Pattern 'callback_timeout_ms\s*=\s*(\d+)' | Select-Object -First 1
    if ($m) { return [int]$m.Matches[0].Groups[1].Value }
    return 30000
}

function Stop-Client($p) {
    if ($p) { taskkill /PID $p.Id /F /T 2>&1 | Out-Null }
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { break }
    }
    Remove-Item Env:\SPACE_FAULT -ErrorAction SilentlyContinue
}

function Start-Faulted($point, $action, $cfg) {
    $env:SPACE_FAULT = "$point=$action"
    Remove-Item $errLog -ErrorAction SilentlyContinue
    $p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $cfg `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\fault-out.log" `
        -RedirectStandardError  $errLog
    # Generous, because some fault points make mounting itself slow. With
    # winfsp_pre_getinfo=hang every get_file_info costs the full deadline and
    # bringing the volume up spends several of them; 20s timed out on a mount
    # that was coming up fine. And the client MUST be killed before throwing --
    # the earlier version leaked it, which is how a hung space-client-fault.exe
    # outlived the run and held S: in a state where even Test-Path blocked.
    # WALL-CLOCK bounded, not iteration-bounded.
    #
    # This loop used to be `for ($i = 0; $i -lt 720; $i++)` with a 250ms sleep,
    # written for 180 seconds. That assumes Test-Path returns promptly. It does
    # not when the armed fault is on the open path: `Test-Path "S:\"` IS an open
    # of the root, so each iteration cost 250ms + the full 30s deadline and the
    # loop became 720 x 30.25s = 6 hours. Measured: the open row ran 4h08m and
    # produced 491 faulted callbacks, every one an open of "\", before being
    # stopped at 491/720 -- see FINDING-13-3-open-row-nontermination.md.
    #
    # A deadline in seconds cannot be inflated by the thing it is waiting on.
    $deadline = (Get-Date).AddSeconds($MountWaitSeconds)
    $up = $false
    while ((Get-Date) -lt $deadline) {
        if (Test-Path "$root\") { $up = $true; break }
        Start-Sleep -Milliseconds 250
    }
    if (-not $up) {
        # Record what the faulted callbacks did even though the mount never
        # became usable. For a point like winfsp_pre_open that is the whole
        # dataset: assertion 1 is a claim about callback duration, and those
        # callbacks happened even though the row's trigger was never reached.
        # Throwing without reading them discards the only evidence the row
        # produced.
        # Point -> boundary operation name, mirroring fault_point_for() in
        # client/core/src/ffi/mod.rs.
        $opForPoint = @{
            "winfsp_pre_read"    = "read"
            "winfsp_pre_write"   = "write"
            "winfsp_pre_open"    = "open"
            "winfsp_pre_create"  = "create"
            "winfsp_pre_readdir" = "dir_open"
            "winfsp_pre_getinfo" = "get_file_info"
            "winfsp_pre_rename"  = "rename"
            "winfsp_pre_cleanup" = "cleanup"
        }[$point]
        $d = if ($opForPoint) { Get-CallbackDurations $opForPoint } else { @() }
        $op = $opForPoint
        $detail = if ($d.Count -gt 0) {
            $mx = ($d | Measure-Object -Maximum).Maximum
            $mn = ($d | Measure-Object -Minimum).Minimum
            "$($d.Count) faulted '$op' callback(s) observed while waiting, min ${mn}ms max ${mx}ms"
        } else { "no faulted '$op' callback observed while waiting" }
        Stop-Client $p
        throw "mount did not become usable within ${MountWaitSeconds}s with $point=$action armed; $detail"
    }

    # The fault MUST actually be armed. Without the fault-injection feature,
    # arm_fault_from_env() compiles to an empty function: SPACE_FAULT is
    # ignored, every callback returns in single-digit milliseconds, and this
    # script would record a bounded, controlled PASS for a deadline that was
    # never exercised. The client logs a warning when it arms; require it.
    $armed = $false
    for ($i = 0; $i -lt 20; $i++) {
        $log = Get-Content $errLog -Raw -ErrorAction SilentlyContinue
        if ($log -and $log -match "FAULT INJECTION ARMED") { $armed = $true; break }
        Start-Sleep -Milliseconds 250
    }
    if (-not $armed) {
        Stop-Client $p
        throw "fault '$point=$action' was NOT armed. Build the client with the feature: cargo build -p space-client --features fault-injection"
    }
    return $p
}

# The slowest callback of ANY operation in the current log, in milliseconds.
# Assertion 4's real claim: nothing anywhere overran its deadline.
function Get-WorstCallbackMs {
    $worst = 0
    foreach ($line in (Get-Content $errLog -ErrorAction SilentlyContinue)) {
        if ($line -notmatch '"duration_ms":([0-9]+)') { continue }
        $ms = [int]$Matches[1]
        if ($ms -gt $worst) { $worst = $ms }
    }
    return $worst
}

# Every faulted-callback duration the log recorded for $op, in milliseconds.
function Get-CallbackDurations($op) {
    $d = @()
    foreach ($line in (Get-Content $errLog -ErrorAction SilentlyContinue)) {
        if ($line -notmatch '"error_code":"OperationTimeout"') { continue }
        try { $j = $line | ConvertFrom-Json } catch { continue }
        if ($j.operation -eq $op -and $null -ne $j.duration_ms) { $d += [int]$j.duration_ms }
    }
    return $d
}

# One hang case.
#
# The trigger runs ASYNCHRONOUSLY so the unrelated operation can be timed while
# the fault is genuinely in flight. Timing it after the trigger returns -- which
# an earlier version did -- measures a filesystem with nothing hung in it, and
# duly reported "bounded at 1257ms" without testing the section 3.6 claim at all.
function Test-Hang($point, $op, $label, $seedCode, $triggerCode, $cfg) {
    $timeout = Get-TimeoutMs $cfg
    Write-Host "--- $label  (point=$point, timeout=${timeout}ms) ---" -ForegroundColor Cyan
    $p = $null
    try {
        $p = Start-Faulted $point "hang" $cfg
        if ($seedCode) { Invoke-Expression $seedCode }

        $job = Start-Job -ScriptBlock {
            param($r, $code)
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $threw = $false
            try { Invoke-Expression $code } catch { $threw = $true }
            $sw.Stop()
            [pscustomobject]@{ Ms = $sw.ElapsedMilliseconds; Threw = $threw }
        } -ArgumentList $root, $triggerCode

        # Let the callback actually enter the hang before measuring anything.
        Start-Sleep -Seconds 2

        # Assertion 4, section 3.6: an UNRELATED operation while the fault is in
        # flight. The coarse guard strategy means it waits on the hung callback,
        # so the bound it must respect is the same deadline -- it must not hang
        # forever. Documented Phase 1 behaviour, not a defect; Phase 10 owns it.
        $sw2 = [Diagnostics.Stopwatch]::StartNew()
        try { Get-ChildItem "$root\" -ErrorAction SilentlyContinue | Out-Null } catch {}
        $sw2.Stop()
        $unrelated = $sw2.ElapsedMilliseconds

        $r = Receive-Job -Job $job -Wait -ErrorAction SilentlyContinue
        Remove-Job $job -Force -ErrorAction SilentlyContinue
        $appMs = if ($r) { $r.Ms } else { -1 }
        $threw = if ($r) { $r.Threw } else { $false }

        # ---- assertion 1, on the callback ----
        $durations = Get-CallbackDurations $op
        if ($durations.Count -eq 0) {
            Write-Host "FAIL  $label : no faulted '$op' callback in the log -- the fault never fired" -ForegroundColor Red
            $script:lines += "FAIL  $label  -- no faulted '$op' callback logged; the fault never fired"
            $script:fail++
        } else {
            $max = ($durations | Measure-Object -Maximum).Maximum
            if ($max -gt ($timeout + 500)) {
                Write-Host "FAIL  $label : slowest callback ${max}ms > $($timeout+500)ms" -ForegroundColor Red
                $script:lines += "FAIL  $label  slowest callback=${max}ms bound=$($timeout+500)ms over $($durations.Count) callback(s)"
                $script:fail++
            } else {
                Write-Host "PASS  $label : $($durations.Count) callback(s), slowest ${max}ms, bound $($timeout+500)ms" -ForegroundColor Green
                $script:lines += "PASS  $label  callbacks=$($durations.Count) slowest=${max}ms bound=$($timeout+500)ms"
                $script:lines += "      application-visible time ${appMs}ms (Windows retried the request $($durations.Count)x; context, not the pass condition)"
            }
        }

        # ---- assertion 2, a controlled error ----
        if ($threw) {
            Write-Host "PASS  $label : application received a controlled error" -ForegroundColor Green
            $script:lines += "PASS  $label  the application received a controlled error, not a hang"
        } else {
            Write-Host "FAIL  $label : application saw success under an injected hang" -ForegroundColor Red
            $script:lines += "FAIL  $label  the application saw success under an injected hang"
            $script:fail++
        }

        # ---- assertion 4 ----
        #
        # Measured on the callback, for the same reason as assertion 1. Timing
        # the unrelated APPLICATION call reported 61552ms against a 30500ms
        # bound and looked like a deadline violation; the log showed the
        # unrelated callbacks (open, get_volume_info, dir_open) completing in
        # 0ms, with the slowest callback anywhere in the run at 30009ms. The
        # application waits because Windows retries the faulted request and the
        # coarse guard serialises them, so it can queue through several deadline
        # periods before its turn. That is bounded, documented section 3.6
        # behaviour -- not a hang, and not a callback overrunning its deadline.
        #
        # So: every callback in the run must be within the bound, and the
        # unrelated operation must actually complete. The wall time is context.
        $worst = Get-WorstCallbackMs
        if ($worst -gt ($timeout + 500)) {
            Write-Host "FAIL  $label : slowest callback anywhere ${worst}ms > $($timeout+500)ms" -ForegroundColor Red
            $script:lines += "FAIL  $label  slowest callback anywhere in the run=${worst}ms bound=$($timeout+500)ms"
            $script:fail++
        } else {
            Write-Host "PASS  $label : unrelated op completed; slowest callback anywhere ${worst}ms" -ForegroundColor Green
            $script:lines += "PASS  $label  unrelated-op completed while the fault was in flight; slowest callback anywhere=${worst}ms bound=$($timeout+500)ms"
            $script:lines += "      unrelated-op wall time ${unrelated}ms (queued behind retries of the faulted request; context, not the pass condition)"
        }

        # ---- assertion 5 ----
        Stop-Client $p; $p = $null
        if (Test-Path "$root\") {
            Write-Host "FAIL  $label : unmount did not complete" -ForegroundColor Red
            $script:lines += "FAIL  $label  unmount did not complete"
            $script:fail++
        } else {
            $script:lines += "PASS  $label  unmount completed after the injected hang"
        }
    } catch {
        Write-Host "FAIL  $label : $($_.Exception.Message)" -ForegroundColor Red
        $script:lines += "FAIL  $label  -- $($_.Exception.Message)"
        $script:fail++
    } finally {
        Stop-Client $p
        $script:lines += ""
    }
}

Write-Host "=== fault injection through a live mount ===" -ForegroundColor Cyan

# ---- section 13.3: repeat the hang for all six registered operations ----
# Trigger/seed are strings so they can be evaluated inside a background job,
# where `$r` is the mount root.
Test-Hang "winfsp_pre_read" "read" "read" `
    '[IO.File]::WriteAllText("$root\h.txt", "payload")' `
    'Get-Content "$r\h.txt" -ErrorAction Stop | Out-Null' $Config

Test-Hang "winfsp_pre_write" "write" "write" `
    $null `
    '[IO.File]::WriteAllText("$r\w.txt", "payload")' $Config

Test-Hang "winfsp_pre_open" "open" "open" `
    '[IO.File]::WriteAllText("$root\o.txt", "payload")' `
    '[IO.File]::ReadAllText("$r\o.txt")' $Config

Test-Hang "winfsp_pre_readdir" "dir_open" "readdir" `
    'New-Item -ItemType Directory -Path "$root\rd" -Force | Out-Null; [IO.File]::WriteAllText("$root\rd\a.txt","x")' `
    'Get-ChildItem "$r\rd" -ErrorAction Stop | Out-Null' $Config

# FileStream.Length, not Get-Item. Windows answers Get-Item from its attribute
# cache right after the seed wrote the file, so the callback is never reached
# and the application sees success under an armed hang -- which is what the
# first run reported. Measured on an unfaulted mount: Get-Item moved the
# get_file_info count 29 -> 43, but only after a cache miss; opening a stream
# and reading Length reaches it reliably (43 -> 57).
Test-Hang "winfsp_pre_getinfo" "get_file_info" "getinfo" `
    '[IO.File]::WriteAllText("$root\gi.txt", "payload")' `
    '$fs = [IO.File]::OpenRead("$r\gi.txt"); try { $null = $fs.Length } finally { $fs.Dispose() }' $Config

Test-Hang "winfsp_pre_rename" "rename" "rename" `
    '[IO.File]::WriteAllText("$root\rn.txt", "payload")' `
    'Rename-Item "$r\rn.txt" "rn2.txt" -ErrorAction Stop' $Config

# ---- section 13.3: the bound SCALES with configuration -------------------
# "proof that the deadline is real rather than an artefact of fast operations"
$fastCfg = Join-Path $env:TEMP "space-fast-timeout.toml"
(Get-Content $Config -Raw) -replace 'callback_timeout_ms\s*=\s*\d+', 'callback_timeout_ms = 1000' |
    Out-File -Encoding utf8 $fastCfg
$lines += "--- bound scales with callback_timeout_ms (1000ms config) ---"
Test-Hang "winfsp_pre_read" "read" "read @1000ms" `
    '[IO.File]::WriteAllText("$root\h.txt", "payload")' `
    'Get-Content "$r\h.txt" -ErrorAction Stop | Out-Null' $fastCfg

# ---- section 13.5: the Panic row, run LAST (it ends the mount) -----------
$lines += "--- section 13.5 Panic row (ADR-0013 end to end) ---"
$p = $null
try {
    $p = Start-Faulted "winfsp_pre_read" "panic" $Config
    [IO.File]::WriteAllText("$root\panic.txt", "payload")
    $threw = $false
    try { Get-Content "$root\panic.txt" -ErrorAction Stop | Out-Null } catch { $threw = $true }
    if (-not $threw) {
        Write-Host "FAIL  panic: read returned success" -ForegroundColor Red
        $lines += "FAIL  panic -- read returned success"; $fail++
    } else {
        Write-Host "PASS  panic: controlled error returned" -ForegroundColor Green
        $lines += "PASS  panic -- controlled error returned to the application"
    }

    $gone = $false
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { $gone = $true; break }
    }
    if ($gone) {
        Write-Host "PASS  panic: poisoned filesystem unmounted itself" -ForegroundColor Green
        $lines += "PASS  panic -- poisoned filesystem unmounted itself (main thread, ADR-0013a)"
    } else {
        Write-Host "FAIL  panic: did not unmount" -ForegroundColor Red
        $lines += "FAIL  panic -- poisoned filesystem did not unmount"; $fail++
    }

    $log = Get-Content $errLog -Raw -ErrorAction SilentlyContinue
    if ($log -and $log -match "PANIC at FFI boundary") {
        Write-Host "PASS  panic: logged with request_id" -ForegroundColor Green
        $lines += "PASS  panic -- 'PANIC at FFI boundary' logged with request_id"
    } else {
        Write-Host "FAIL  panic: no PANIC line in the log" -ForegroundColor Red
        $lines += "FAIL  panic -- no PANIC line in the log"; $fail++
    }
} finally {
    Stop-Client $p
}

Write-Host ""
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
$osExit = $LASTEXITCODE
$lines += ""
$lines += "os-safety-check exit: $osExit"
if ($osExit -ne 0) { $fail++ }

$lines += ""
$lines += "NOTE: section 13.2 point 3 -- 'Explorer stays responsive' -- is a claim about"
$lines += "the Windows shell and is NOT covered here. It remains NEEDS HUMAN."
$lines += ""
$lines += "result: $(if ($fail -gt 0) { "$fail FAILURE(S)" } else { 'all scripted fault cases PASS' })"

New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$lines -join "`r`n" | Out-File -Encoding utf8 $Out

if ($fail -gt 0) { Write-Host "`n$fail fault-injection failure(s)" -ForegroundColor Red; exit 1 }
Write-Host "`nfault injection OK" -ForegroundColor Green
exit 0
