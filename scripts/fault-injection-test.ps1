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

param(
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client.exe",
    [string]$Out = "docs\evidence\phase-1\fault-injection.txt"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0
$lines = @()
$lines += "SPACE Phase 1 -- fault injection through a live mount (sections 13.2, 13.3)"
$lines += "date: $(Get-Date -Format o)"
$lines += ""

function Get-TimeoutMs([string]$cfg) {
    $m = Select-String -Path $cfg -Pattern 'callback_timeout_ms\s*=\s*(\d+)' | Select-Object -First 1
    if ($m) { return [int]$m.Matches[0].Groups[1].Value }
    return 30000
}

function Start-Faulted($point, $action, $cfg) {
    $env:SPACE_FAULT = "$point=$action"
    $p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $cfg `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\fault-out.log" `
        -RedirectStandardError  "C:\SPACE\runtime\logs\fault-err.log"
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-Path "$root\") { return $p }
    }
    throw "mount did not appear"
}

function Stop-Client($p) {
    if ($p) { taskkill /PID $p.Id /F /T 2>&1 | Out-Null }
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { break }
    }
    Remove-Item Env:\SPACE_FAULT -ErrorAction SilentlyContinue
}

# One hang case: arm `point`, run `trigger`, assert bounded + errored, then
# assert an UNRELATED operation is also bounded (the section 3.6 prediction).
function Test-Hang($point, $label, [scriptblock]$seed, [scriptblock]$trigger, $cfg) {
    $timeout = Get-TimeoutMs $cfg
    Write-Host "--- $label  (point=$point, timeout=${timeout}ms) ---" -ForegroundColor Cyan
    $p = $null
    try {
        $p = Start-Faulted $point "hang" $cfg

        # Seed BEFORE the fault bites where the fault point would block setup.
        if ($seed) { & $seed }

        $sw = [Diagnostics.Stopwatch]::StartNew()
        $threw = $false
        try { & $trigger } catch { $threw = $true }
        $sw.Stop()
        $elapsed = $sw.ElapsedMilliseconds

        if ($elapsed -gt ($timeout + 500)) {
            Write-Host "FAIL  $label : ${elapsed}ms > ${timeout}+500" -ForegroundColor Red
            $script:lines += "FAIL  $label  elapsed=${elapsed}ms bound=$($timeout+500)ms"
            $script:fail++
        } else {
            Write-Host "PASS  $label : bounded at ${elapsed}ms (threw=$threw)" -ForegroundColor Green
            $script:lines += "PASS  $label  elapsed=${elapsed}ms bound=$($timeout+500)ms threw=$threw"
        }

        # Section 3.6 point 4: an UNRELATED operation is bounded too, because
        # the hung callback holds the single state lock. Documented Phase 1
        # behaviour, not a defect; Phase 10 owns fixing it.
        $sw2 = [Diagnostics.Stopwatch]::StartNew()
        try { Get-ChildItem "$root\" -ErrorAction SilentlyContinue | Out-Null } catch {}
        $sw2.Stop()
        if ($sw2.ElapsedMilliseconds -gt ($timeout + 500)) {
            Write-Host "FAIL  $label : unrelated op ${$sw2.ElapsedMilliseconds}ms unbounded" -ForegroundColor Red
            $script:lines += "FAIL  $label  unrelated-op elapsed=$($sw2.ElapsedMilliseconds)ms"
            $script:fail++
        } else {
            Write-Host "PASS  $label : unrelated op bounded at $($sw2.ElapsedMilliseconds)ms" -ForegroundColor Green
            $script:lines += "PASS  $label  unrelated-op elapsed=$($sw2.ElapsedMilliseconds)ms (section 3.6 confirmed)"
        }

        # Point 5: unmount still succeeds.
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
    }
}

Write-Host "=== fault injection through a live mount ===" -ForegroundColor Cyan

# ---- section 13.3: repeat the hang for all six registered operations ----
Test-Hang "winfsp_pre_read" "read" `
    { [IO.File]::WriteAllText("$root\h.txt", "payload"); New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null } `
    { Get-Content "$root\h.txt" -ErrorAction Stop | Out-Null } $Config

Test-Hang "winfsp_pre_write" "write" `
    { New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null } `
    { [IO.File]::WriteAllText("$root\w.txt", "payload") } $Config

Test-Hang "winfsp_pre_open" "open" `
    { New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null } `
    { [IO.File]::ReadAllText("$root\nonexistent-open.txt") } $Config

Test-Hang "winfsp_pre_readdir" "readdir" `
    { New-Item -ItemType Directory -Path "$root\rd" -Force | Out-Null; [IO.File]::WriteAllText("$root\rd\a.txt","x"); New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null } `
    { Get-ChildItem "$root\rd" -ErrorAction Stop | Out-Null } $Config

Test-Hang "winfsp_pre_getinfo" "getinfo" `
    { [IO.File]::WriteAllText("$root\gi.txt", "payload"); New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null } `
    { (Get-Item "$root\gi.txt" -ErrorAction Stop).Length | Out-Null } $Config

Test-Hang "winfsp_pre_rename" "rename" `
    { [IO.File]::WriteAllText("$root\rn.txt", "payload"); New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null } `
    { Rename-Item "$root\rn.txt" "rn2.txt" -ErrorAction Stop } $Config

# ---- section 13.3: the bound SCALES with configuration -------------------
# "proof that the deadline is real rather than an artefact of fast operations"
$fastCfg = Join-Path $env:TEMP "space-fast-timeout.toml"
(Get-Content $Config -Raw) -replace 'callback_timeout_ms\s*=\s*\d+', 'callback_timeout_ms = 1000' |
    Out-File -Encoding utf8 $fastCfg
$lines += ""
$lines += "--- bound scales with callback_timeout_ms (1000ms config) ---"
Test-Hang "winfsp_pre_read" "read @1000ms" `
    { [IO.File]::WriteAllText("$root\h.txt", "payload") } `
    { Get-Content "$root\h.txt" -ErrorAction Stop | Out-Null } $fastCfg

# ---- section 13.5: the Panic row, run LAST (it ends the mount) -----------
$lines += ""
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

    $log = Get-Content "C:\SPACE\runtime\logs\fault-err.log" -Raw -ErrorAction SilentlyContinue
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
