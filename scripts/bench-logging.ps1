# SPACE Phase 1 -- logging/build-profile benchmark.
#
# EXCLUSIVE: starts and stops clients. Must not run while another mount test is
# running. In particular it must NOT run during section 15.2, which is what it
# exists to speed up.
#
# WHY THIS EXISTS
#
# Section 15.2's mid-enumeration state builds a 5,000-entry directory per
# iteration and was measured at ~125 file creates per minute -- roughly 40
# minutes per iteration, 13+ hours for the state. The client's own log explains
# where the time goes: 10 log lines and 3,453 bytes per file create, every line
# taken through a single `Arc<Mutex<Box<dyn Write + Send>>>` and, with the
# default sink, written unbuffered to stderr.
#
# This measures the actual cost of each contributing factor rather than assuming
# which one dominates.
#
# WHAT IS AND IS NOT VARIED
#
# The workload is identical in every arm: create 5,000 files in one directory,
# the exact section 15.2 mid-enumeration fixture. No acceptance criterion is
# touched. The only variables are the build profile and where log lines go --
# neither of which changes what the filesystem does.
#
# Levels the harnesses depend on all survive a move to INFO, so raising the
# level does not blind any test:
#   "clean shutdown"        INFO   (mount-stress proves the graceful path with it)
#   "FAULT INJECTION ARMED" WARN   (fault-injection refuses to run without it)
#   OperationTimeout rows   WARN   (fault-injection measures callback duration_ms)
#   "PANIC at FFI boundary" ERROR  (shutdown-states and the 13.5 panic row)
# Only the DEBUG success-path boundary lines are lost, and those are the
# evidence for sections 12.1 and 12.2, which is gathered in its own run at debug
# level and must stay that way.

param(
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [int]$Files = 5000,
    [string]$Out = "docs\evidence\phase-1\bench-logging.md"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$fsp  = "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe"

function Assert-Clean {
    $c = @(Get-Process space-client* -ErrorAction SilentlyContinue).Count
    $v = (& $fsp lsvol 2>&1 | Out-String).Trim()
    if ($c -gt 0 -or (Test-Path "$root\") -or $v) {
        throw "environment not clean: clients=$c S:=$(Test-Path "$root\") volumes='$v'"
    }
}

# One arm. Returns elapsed seconds for creating $Files files in one directory.
function Measure-Arm($label, $exe, $logDir) {
    Assert-Clean
    if ($logDir) { $env:SPACE_CLIENT_LOG_DIR = $logDir } else { Remove-Item Env:\SPACE_CLIENT_LOG_DIR -ErrorAction SilentlyContinue }
    $errLog = "C:\SPACE\runtime\logs\bench-err.log"
    if (Test-Path $errLog) { [IO.File]::Delete($errLog) }

    $p = Start-Process -PassThru -FilePath $exe -ArgumentList "--config", $Config `
        -RedirectStandardOutput "C:\SPACE\runtime\logs\bench-out.log" `
        -RedirectStandardError  $errLog
    $up = $false
    for ($i = 0; $i -lt 120; $i++) { Start-Sleep -Milliseconds 250; if (Test-Path "$root\") { $up = $true; break } }
    if (-not $up) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; throw "$label -- mount did not appear" }

    New-Item -ItemType Directory -Path "$root\bench" -Force | Out-Null
    $sw = [Diagnostics.Stopwatch]::StartNew()
    for ($i = 1; $i -le $Files; $i++) { [IO.File]::WriteAllText("$root\bench\e$i.txt", "$i") }
    $sw.Stop()
    $secs = [math]::Round($sw.Elapsed.TotalSeconds, 1)

    # How much did it log, and where?
    $stderrBytes = if (Test-Path $errLog) { (Get-Item $errLog).Length } else { 0 }
    $dirBytes = 0
    if ($logDir -and (Test-Path $logDir)) {
        $dirBytes = (Get-ChildItem $logDir -Filter "space-client*.jsonl" -ErrorAction SilentlyContinue |
                     Measure-Object -Property Length -Sum).Sum
    }

    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; if (-not (Test-Path "$root\")) { break } }
    Remove-Item Env:\SPACE_CLIENT_LOG_DIR -ErrorAction SilentlyContinue

    $rate = if ($secs -gt 0) { [math]::Round($Files / $secs, 1) } else { 0 }
    Write-Host ("{0,-34} {1,8}s  {2,8} files/s  stderr={3:N0}B dir={4:N0}B" -f $label, $secs, $rate, $stderrBytes, $dirBytes) -ForegroundColor Cyan
    return [pscustomobject]@{ Arm = $label; Seconds = $secs; Rate = $rate; StderrBytes = $stderrBytes; DirBytes = $dirBytes }
}

$logDir = "C:\SPACE\runtime\logs\bench-jsonl"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
Get-ChildItem $logDir -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

Write-Host "=== logging / build-profile benchmark: $Files creates in one directory ===" -ForegroundColor Cyan
$rows = @()
$rows += Measure-Arm "debug   + stderr (15.2 today)"   ".\target\debug\space-client.exe"   $null
Get-ChildItem $logDir -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
$rows += Measure-Arm "debug   + jsonl appender"        ".\target\debug\space-client.exe"   $logDir
$rows += Measure-Arm "release + stderr"                ".\target\release\space-client.exe" $null
Get-ChildItem $logDir -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
$rows += Measure-Arm "release + jsonl appender"        ".\target\release\space-client.exe" $logDir

$base = ($rows | Where-Object { $_.Arm -like "debug   + stderr*" }).Seconds

$md = @()
$md += "# Logging and build-profile benchmark (section 15.2 cost)"
$md += ""
$md += "- Generated: $(Get-Date -Format o)"
$md += "- Commit: ``$(git rev-parse HEAD)``"
$md += "- Workload: create $Files files in one directory -- the section 15.2 mid-enumeration fixture, unchanged"
$md += ""
$md += "| configuration | seconds | files/sec | speedup | stderr bytes | jsonl bytes |"
$md += "|---|---:|---:|---:|---:|---:|"
foreach ($r in $rows) {
    $sp = if ($r.Seconds -gt 0) { "{0:N1}x" -f ($base / $r.Seconds) } else { "-" }
    $md += "| $($r.Arm) | $($r.Seconds) | $($r.Rate) | $sp | $('{0:N0}' -f $r.StderrBytes) | $('{0:N0}' -f $r.DirBytes) |"
}
$md += ""
$md += "The workload, the filesystem and every acceptance criterion are identical across"
$md += "arms. Only the build profile and the log sink differ."
$md += ""
$md += "Both sink arms still log at DEBUG: the configured ``logging.level`` is parsed and"
$md += "validated and then never applied -- ``contracts::logging::init`` builds"
$md += "``registry().with(layer)`` with no filter -- so this benchmark measures the cost of"
$md += "*where* lines go, not of suppressing them. The additional gain from honouring the"
$md += "configured level is on top of whatever the appender arms show."

New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$md -join "`r`n" | Out-File -Encoding utf8 $Out
Write-Host ""
Write-Host "written to $Out" -ForegroundColor Green
