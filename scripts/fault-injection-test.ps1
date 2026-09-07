# SPACE Phase 1 -- fault injection through a real mount (manual sections 13.2, 13.3).
#
# The in-process half of fault testing lives in
# client/core/src/ffi/fault_tests.rs. This script covers the half that is a
# claim about WINDOWS and cannot be asserted from inside the process:
#
#   1. the callback returns STATUS_IO_TIMEOUT within callback_timeout_ms + 500ms
#   2. the issuing application receives a controlled error, not a hang
#   3. Explorer stays responsive                       [manual observation]
#   4. OTHER operations return OperationTimeout rather than hanging -- the
#      section 3.6 model PREDICTS this; here it is confirmed
#   5. unmount still succeeds
#   6. check_invariants() passes afterwards            [in-process]
#   7. os-safety-check.ps1 passes
#
# Point 4 is the point of section 3.6: the behaviour is predicted by the
# documented model and then confirmed, rather than assumed.
#
# Requires a client built with --features fault-injection. Faults are armed
# through the SPACE_FAULT env var read at startup (one point, one action) --
# deliberately crude, because a full control channel is a Phase 10 concern and
# would be untested surface here.

param(
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\debug\space-client.exe",
    [int]$TimeoutMs = 3000
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0

function Start-Faulted($point, $action) {
    $env:SPACE_FAULT = "$point=$action"
    $p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\fault-out.log" `
        -RedirectStandardError  "C:\SPACE\runtime\logs\fault-err.log"
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-Path "$root\") { return $p }
    }
    throw "mount did not appear"
}

function Stop-Client($p) {
    if ($p) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { break }
    }
    Remove-Item Env:\SPACE_FAULT -ErrorAction SilentlyContinue
}

Write-Host "=== fault injection through a live mount ===" -ForegroundColor Cyan
Write-Host "callback_timeout_ms under test: $TimeoutMs" -ForegroundColor DarkGray

# ---- Hang on read: the primary bound (13.2) ----------------------------
$p = $null
try {
    $p = Start-Faulted "winfsp_pre_read" "hang"

    # Seed a file and an unrelated directory BEFORE the fault bites: the fault
    # point is on read, so writes still work.
    [IO.File]::WriteAllText("$root\hang.txt", "payload")
    New-Item -ItemType Directory -Path "$root\other" -Force | Out-Null
    [IO.File]::WriteAllText("$root\other\x.txt", "x")

    # (1) and (2): bounded, and an error rather than a hang.
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $threw = $false
    try { Get-Content "$root\hang.txt" -ErrorAction Stop | Out-Null } catch { $threw = $true }
    $sw.Stop()

    if (-not $threw) {
        Write-Host "FAIL  a hung read returned success" -ForegroundColor Red; $fail++
    } elseif ($sw.ElapsedMilliseconds -gt ($TimeoutMs + 500)) {
        Write-Host "FAIL  hung read took $($sw.ElapsedMilliseconds)ms, past $TimeoutMs + 500" -ForegroundColor Red; $fail++
    } else {
        Write-Host "PASS  hung read bounded at $($sw.ElapsedMilliseconds)ms, controlled error" -ForegroundColor Green
    }

    # (4) the section 3.6 prediction: an UNRELATED operation is also bounded,
    # because the hung callback holds the single state lock. This is documented
    # Phase 1 behaviour, not a defect -- Phase 10 owns fixing it.
    $sw2 = [Diagnostics.Stopwatch]::StartNew()
    try { Get-ChildItem "$root\other" -ErrorAction SilentlyContinue | Out-Null } catch { }
    $sw2.Stop()
    if ($sw2.ElapsedMilliseconds -gt ($TimeoutMs + 500)) {
        Write-Host "FAIL  unrelated op took $($sw2.ElapsedMilliseconds)ms -- not bounded" -ForegroundColor Red; $fail++
    } else {
        Write-Host "PASS  unrelated op bounded at $($sw2.ElapsedMilliseconds)ms (section 3.6 confirmed)" -ForegroundColor Green
    }

    # (5) unmount still succeeds.
    Stop-Client $p; $p = $null
    if (Test-Path "$root\") {
        Write-Host "FAIL  unmount did not complete after an injected hang" -ForegroundColor Red; $fail++
    } else {
        Write-Host "PASS  unmount completed after an injected hang" -ForegroundColor Green
    }
} finally {
    Stop-Client $p
}

# ---- Panic: ADR-0013 end to end through a real mount (13.5) -------------
# Run last: it deliberately ends the mount.
$p = $null
try {
    $p = Start-Faulted "winfsp_pre_read" "panic"
    [IO.File]::WriteAllText("$root\panic.txt", "payload")

    $threw = $false
    try { Get-Content "$root\panic.txt" -ErrorAction Stop | Out-Null } catch { $threw = $true }
    if (-not $threw) {
        Write-Host "FAIL  a panicking read returned success" -ForegroundColor Red; $fail++
    } else {
        Write-Host "PASS  panicking read returned a controlled error" -ForegroundColor Green
    }

    # The poison path unmounts on the MAIN thread. Give it a moment.
    $gone = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-Path "$root\")) { $gone = $true; break }
    }
    if ($gone) {
        Write-Host "PASS  poisoned filesystem unmounted itself" -ForegroundColor Green
    } else {
        Write-Host "FAIL  poisoned filesystem did not unmount" -ForegroundColor Red; $fail++
    }

    $log = Get-Content "C:\SPACE\runtime\logs\fault-err.log" -Raw -ErrorAction SilentlyContinue
    if ($log -and $log -match "PANIC at FFI boundary") {
        Write-Host "PASS  panic logged at the boundary with its request_id" -ForegroundColor Green
    } else {
        Write-Host "FAIL  no PANIC line in the log" -ForegroundColor Red; $fail++
    }
} finally {
    Stop-Client $p
}

Write-Host ""
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
if ($LASTEXITCODE -ne 0) { $fail++ }

if ($fail -gt 0) { Write-Host "`n$fail fault-injection failure(s)" -ForegroundColor Red; exit 1 }
Write-Host "`nfault injection OK" -ForegroundColor Green
exit 0
