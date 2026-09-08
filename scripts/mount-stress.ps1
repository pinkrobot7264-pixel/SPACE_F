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
    [string]$Exe = ".\target\debug\space-client.exe",
    [string]$Out = "docs\evidence\phase-1\mount-stress.txt"
)

# MUST RUN WITH A CONSOLE. The graceful branch delivers CTRL_C_EVENT, which
# only exists for processes attached to a console, and a client inherits its
# console from this script. Launching this script with its streams redirected
# by a shell -- `powershell ... *> file` from bash, say -- leaves it without a
# usable console, and the symptom is not an error: the first graceful cycle
# appears to deliver the signal, the client never tears down, S: stays mounted,
# and from then on `Test-Path S:` is true for every later iteration so each one
# believes it mounted instantly. One 200-cycle run produced 299 failures that
# way, none of them a filesystem defect. Start it with
# `Start-Process powershell -ArgumentList '-File', '<this script>'`, which
# gives it a console of its own, and let it write $Out itself.

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$fail = 0
$graceful = 0
$forced = 0
$consecutive = 0
$gracefulProven = 0
$forcedProven = 0
$lines = @()
$lines += "SPACE Phase 1 -- mount/unmount stress (manual section 16.1)"
$lines += "date: $(Get-Date -Format o)"
$lines += "iterations: $Iterations, alternating forced kill (odd) and graceful Ctrl-C (even)"
$lines += "commit: $(git rev-parse HEAD)"
$lines += ""

function Write-Evidence {
    $lines += ""
    New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
    $script:lines -join "`r`n" | Out-File -Encoding utf8 $Out
}

Add-Type -Namespace MS -Name W -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();
'@

# PRE-FLIGHT. Prove the graceful path can work in THIS launch context before
# spending 200 cycles finding out that it cannot.
#
# A client inherits its Ctrl-C disposition from the process that creates it.
# Started from a context where Ctrl-C is disabled -- bash with redirected
# streams, a service, a detached job -- the client inherits "ignore Ctrl-C" and
# the OS discards CTRL_C_EVENT before any registered handler runs. Nothing
# fails loudly: AttachConsole succeeds, GenerateConsoleCtrlEvent returns
# success, send-ctrl-c.ps1 exits 0, and the client simply carries on.
#
# Measured, both arms from a verified-clean machine, only the launch differing:
#   bash + redirected streams : S: never released (67547ms), client never
#                               exited, and the client logged NO shutdown line
#                               at all -- the handler never ran.
#   own console               : S: released in 307ms, client exited, log shows
#                               "shutdown requested; unmounting" then "clean
#                               shutdown" elapsed_ms=39.
#
# So refuse to start rather than produce a 100-cycle "graceful" half that
# exercised nothing. This is a precondition on the harness, not a relaxation of
# any product assertion.
function Test-GracefulPathWorks {
    $probeLog = "C:\SPACE\runtime\logs\stress-preflight.log"
    if (Test-Path $probeLog) { [IO.File]::Delete($probeLog) }
    $q = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\stress-preflight-out.log" `
        -RedirectStandardError  $probeLog
    for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; if (Test-Path "$root\") { break } }
    if (-not (Test-Path "$root\")) {
        Stop-Process -Id $q.Id -Force -ErrorAction SilentlyContinue
        return @{ Ok = $false; Why = "pre-flight client never mounted" }
    }
    $r = Start-Process -FilePath "powershell" -PassThru -Wait -WindowStyle Hidden `
        -ArgumentList @("-NoProfile", "-File", "$PSScriptRoot\send-ctrl-c.ps1", "-TargetPid", $q.Id)
    $released = $false
    for ($i = 0; $i -lt 40; $i++) { Start-Sleep -Milliseconds 250; if (-not (Test-Path "$root\")) { $released = $true; break } }
    $log = Get-Content $probeLog -Raw -ErrorAction SilentlyContinue
    $clean = $log -and ($log -match "clean shutdown")
    if (-not $released) {
        Stop-Process -Id $q.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 3
    }
    if ($released -and $clean) { return @{ Ok = $true; Why = "" } }
    return @{ Ok = $false; Why = "send-ctrl-c exited $($r.ExitCode), mount released=$released, clean-shutdown logged=$clean" }
}

Write-Host "=== mount/unmount stress: $Iterations cycles ===" -ForegroundColor Cyan
Write-Host "pre-flight: verifying the graceful path works in this launch context..." -ForegroundColor DarkGray
$hasConsole = [MS.W]::GetConsoleWindow() -ne [IntPtr]::Zero
$pre = Test-GracefulPathWorks
if (-not $pre.Ok) {
    Write-Host "ABORT: the graceful shutdown path does not work in this launch context." -ForegroundColor Red
    Write-Host "       $($pre.Why)" -ForegroundColor Red
    Write-Host "       console window present: $hasConsole" -ForegroundColor Red
    Write-Host "       Start this script with a console of its own, e.g." -ForegroundColor Yellow
    Write-Host "         Start-Process powershell -ArgumentList '-File','scripts\mount-stress.ps1'" -ForegroundColor Yellow
    Write-Host "       Running anyway would report a graceful/forced split having tested only forced kills." -ForegroundColor Yellow
    $lines += "ABORT -- the graceful path does not work in this launch context: $($pre.Why)"
    $lines += "        harness console window present: $hasConsole"
    $lines += "        No cycles were run. A run from here would have reported a graceful half that exercised nothing."
    Write-Evidence
    exit 1
}
Write-Host "pre-flight OK: Ctrl-C reaches the client and it shuts down cleanly" -ForegroundColor Green
$lines += "pre-flight: graceful path verified (Ctrl-C delivered, mount released, 'clean shutdown' logged)"
$lines += "harness console window present: $hasConsole"
$lines += ""

for ($n = 1; $n -le $Iterations; $n++) {
    # A leftover mount makes every later Test-Path succeed, so an iteration
    # that starts with S: already present is not testing anything -- it is
    # reading the previous iteration's failure as its own success. Stop here
    # instead: one clear failure beats 299 cascading ones burying it.
    if (Test-Path "$root\") {
        Write-Host "ABORT at iteration $n : $root was already present before starting a client" -ForegroundColor Red
        $lines += "ABORT at iteration $n -- $root still present from the previous cycle; the run cannot continue honestly"
        $fail++
        break
    }

    $iterLog = "C:\SPACE\runtime\logs\stress-err.log"
    if (Test-Path $iterLog) { [IO.File]::Delete($iterLog) }
    $p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\stress-out.log" `
        -RedirectStandardError  $iterLog

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
        $lines += "FAIL  iteration $n -- unmount failed, $root still present"
        $fail++
        $consecutive++
        # Force the leftover away so the next iteration's precondition check is
        # meaningful rather than inheriting this failure.
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
        if ($consecutive -ge 3) {
            Write-Host "ABORT: $consecutive consecutive unmount failures" -ForegroundColor Red
            $lines += "ABORT after $consecutive consecutive unmount failures at iteration $n"
            break
        }
    } else {
        $consecutive = 0
    }

    # Prove WHICH path this cycle took, from the client's own log. A graceful
    # cycle must show the ADR-0013a main-thread teardown; a forced cycle must
    # NOT, because nothing runs after a Class B process death. Without this the
    # two branches are indistinguishable in the evidence -- which is exactly how
    # a harness that force-killed in both branches reported a 100/100 split for
    # as long as it did.
    $log = Get-Content $iterLog -Raw -ErrorAction SilentlyContinue
    $cleanLogged = [bool]($log -and $log -match "clean shutdown")
    if ($n % 2 -eq 0) {
        if (-not $cleanLogged) {
            Write-Host "FAIL  iteration $n : graceful cycle did not log a clean shutdown" -ForegroundColor Red
            $lines += "FAIL  iteration $n -- graceful cycle released the mount but logged no clean shutdown; the ADR-0013a path did not run"
            $fail++
        } else { $gracefulProven++ }
    } else {
        if ($cleanLogged) {
            Write-Host "FAIL  iteration $n : forced cycle logged a clean shutdown" -ForegroundColor Red
            $lines += "FAIL  iteration $n -- forced cycle logged a clean shutdown; it was not a Class B process death"
            $fail++
        } else { $forcedProven++ }
    }

    if ($n % 25 -eq 0) { Write-Host "  ...$n cycles" -ForegroundColor DarkGray }
}

Write-Host ""
Write-Host "$Iterations cycles complete ($graceful graceful Ctrl-C / $forced forced kill)" -ForegroundColor Cyan
$lines += "cycles attempted: $($graceful + $forced) ($graceful graceful Ctrl-C / $forced forced kill)"
$lines += "paths proven from the client's own log: $gracefulProven graceful cycles logged a clean shutdown,"
$lines += "  $forcedProven forced cycles logged none (a Class B death runs no teardown)"
& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
$osExit = $LASTEXITCODE
$lines += "os-safety-check exit: $osExit"
if ($osExit -ne 0) { $fail++ }

$lines += ""
if ($fail -gt 0) {
    $lines += "result: $fail FAILURE(S)"
} else {
    $lines += "result: PASS -- $Iterations cycles, $graceful graceful and $forced forced, every mount released"
}
Write-Evidence

if ($fail -gt 0) { Write-Host "`n$fail stress failure(s) -- see $Out" -ForegroundColor Red; exit 1 }
Write-Host "`nmount/unmount stress OK -- evidence in $Out" -ForegroundColor Green
exit 0
