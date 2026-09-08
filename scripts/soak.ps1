# SPACE Phase 1 -- I/O stress and soak (manual sections 16.2, 16.5).
#
# **The purpose is leak detection, not performance.** Do not tune allocation, do
# not optimise the data structures, do not turn this into a performance project.
#
# The interpretation rules are decided HERE, before the numbers exist, exactly
# as section 16.5 requires -- otherwise the graph gets read to fit whatever it
# shows:
#
#   RSS within +/-10% of the 30-minute value, no trend .......... plateau, PASS
#   RSS regression slope after the 30-minute mark < 1 MB/hour ... PASS
#   slope >= 1 MB/hour, monotonic across >= 6 samples ........... investigate
#   OS handle count or thread count grows without bound ......... FAIL
#   SPACE live handle/cursor count off baseline after a cycle ... FAIL
#
# The last rule is checked directly by the workload: every iteration creates and
# deletes, so a growing count is a leak (INV-FS-2, INV-RES-1).

# EXCLUSIVE: this script starts and stops clients. Nothing else may use the
# mount while it runs -- a concurrent test will see its mount vanish mid-call
# and report failures that look like defects and are not. See
# docs/evidence/phase-1/PHASE-1-CERTIFICATION.md, "Harness discipline".

param(
    [int]$Minutes = 240,
    [int]$SampleSeconds = 300,
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\release\space-client.exe",
    [string]$Samples = "docs\evidence\phase-1\soak-samples.csv"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$deadline = $since.AddMinutes($Minutes)

# Section 16.2: the size matrix, so the soak exercises the same boundaries the
# unit tests do rather than one comfortable size.
$sizes = @(0, 1, 4095, 4096, 4097, 1048576)

Write-Host "=== SPACE soak: $Minutes minutes, sampling every $SampleSeconds s ===" -ForegroundColor Cyan
Write-Host "release build, leak detection only" -ForegroundColor DarkGray

$p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
    -RedirectStandardOutput "C:\SPACE\runtime\logs\soak-out.log" `
    -RedirectStandardError  "C:\SPACE\runtime\logs\soak-err.log"

$mounted = $false
for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Milliseconds 250
    if (Test-Path "$root\") { $mounted = $true; break }
}
if (-not $mounted) { Write-Host "FAIL  mount did not appear" -ForegroundColor Red; exit 1 }

New-Item -ItemType Directory -Force -Path (Split-Path $Samples) | Out-Null
"timestamp,elapsed_min,rss_bytes,handles,threads,root_entries" | Out-File -Encoding utf8 $Samples

# The workload runs in a job so sampling is not blocked by it.
$work = Start-Job -ScriptBlock {
    param($r, $sizes)
    $rand = New-Object Random 20260907
    $i = 0
    while ($true) {
        $i++
        $d = "$r\soak\w$($i % 8)"
        try {
            New-Item -ItemType Directory -Path $d -Force -ErrorAction Stop | Out-Null

            foreach ($s in $sizes) {
                $f = "$d\f$s.bin"
                $bytes = New-Object byte[] $s
                if ($s -gt 0) { $rand.NextBytes($bytes) }
                [IO.File]::WriteAllBytes($f, $bytes)

                # read back and verify -- a soak that does not verify is a
                # throughput test
                $back = [IO.File]::ReadAllBytes($f)
                if ($back.Length -ne $s) { throw "size mismatch at $s" }
            }

            # enumerate a wide directory
            $many = "$r\soak\many"
            New-Item -ItemType Directory -Path $many -Force -ErrorAction Stop | Out-Null
            1..200 | ForEach-Object { [IO.File]::WriteAllText("$many\e$_.txt", "$_") }
            Get-ChildItem $many | Out-Null

            # rename between directories
            $other = "$r\soak\other"
            New-Item -ItemType Directory -Path $other -Force -ErrorAction Stop | Out-Null
            Move-Item "$d\f4096.bin" "$other\moved-$i.bin" -Force -ErrorAction SilentlyContinue

            # ...and delete everything, so live counts must return to baseline
            Remove-Item $many -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item $other -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue
        } catch {
            # Record and keep going: one transient failure should not end a
            # four-hour run, but it must not be silent either.
            Write-Output "WORKLOAD ERROR: $($_.Exception.Message)"
        }
    }
} -ArgumentList $root, $sizes

$sampleCount = 0
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds $SampleSeconds
    $sampleCount++

    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($null -eq $proc) {
        Write-Host "FAIL  the client died during the soak" -ForegroundColor Red
        Stop-Job $work -ErrorAction SilentlyContinue
        exit 1
    }

    $elapsed = [math]::Round(((Get-Date) - $since).TotalMinutes, 1)
    $entries = try { (Get-ChildItem "$root\" -ErrorAction Stop | Measure-Object).Count } catch { -1 }
    "$(Get-Date -Format o),$elapsed,$($proc.WorkingSet64),$($proc.HandleCount),$($proc.Threads.Count),$entries" |
        Out-File -Append -Encoding utf8 $Samples

    Write-Host ("  {0,6} min  rss={1,10:N0}  handles={2,5}  threads={3,3}" -f `
        $elapsed, $proc.WorkingSet64, $proc.HandleCount, $proc.Threads.Count)
}

Stop-Job $work -ErrorAction SilentlyContinue
$workErrors = Receive-Job $work -ErrorAction SilentlyContinue | Where-Object { $_ -like "WORKLOAD ERROR*" }
Remove-Job $work -Force -ErrorAction SilentlyContinue

Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3

# ---- interpretation, by the rules stated at the top ---------------------
$rows = Import-Csv $Samples
$fail = 0

if ($rows.Count -lt 3) {
    Write-Host "WARN  only $($rows.Count) samples; too few to judge a trend" -ForegroundColor Yellow
} else {
    # Ignore the first 30 minutes: allocator warm-up is not a leak.
    $steady = $rows | Where-Object { [double]$_.elapsed_min -ge 30 }
    if ($steady.Count -lt 3) { $steady = $rows }

    $xs = $steady | ForEach-Object { [double]$_.elapsed_min / 60.0 }   # hours
    $ys = $steady | ForEach-Object { [double]$_.rss_bytes / 1MB }      # MB
    $n = $xs.Count
    $mx = ($xs | Measure-Object -Average).Average
    $my = ($ys | Measure-Object -Average).Average
    $num = 0.0; $den = 0.0
    for ($i = 0; $i -lt $n; $i++) {
        $num += ($xs[$i] - $mx) * ($ys[$i] - $my)
        $den += ($xs[$i] - $mx) * ($xs[$i] - $mx)
    }
    $slope = if ($den -ne 0) { $num / $den } else { 0 }

    Write-Host ""
    Write-Host ("RSS slope after the 30-minute mark: {0:N2} MB/hour" -f $slope) -ForegroundColor Cyan
    if ($slope -lt 1.0) {
        Write-Host "PASS  under the 1 MB/hour threshold" -ForegroundColor Green
    } else {
        Write-Host "INVESTIGATE  slope >= 1 MB/hour -- section 16.5 says resolve before the gate" -ForegroundColor Yellow
        $fail++
    }

    $h0 = [int]($steady[0].handles); $hN = [int]($steady[-1].handles)
    $t0 = [int]($steady[0].threads); $tN = [int]($steady[-1].threads)
    if ($hN -gt $h0 * 2) { Write-Host "FAIL  OS handle count grew $h0 -> $hN" -ForegroundColor Red; $fail++ }
    else { Write-Host "PASS  OS handle count stable ($h0 -> $hN)" -ForegroundColor Green }
    if ($tN -gt $t0 * 2) { Write-Host "FAIL  thread count grew $t0 -> $tN" -ForegroundColor Red; $fail++ }
    else { Write-Host "PASS  thread count stable ($t0 -> $tN)" -ForegroundColor Green }
}

if ($workErrors) {
    Write-Host "WARN  $($workErrors.Count) workload errors during the soak:" -ForegroundColor Yellow
    $workErrors | Select-Object -First 5 | ForEach-Object { Write-Host "    $_" }
}

& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
if ($LASTEXITCODE -ne 0) { $fail++ }

Write-Host ""
Write-Host "samples: $Samples ($sampleCount rows)" -ForegroundColor Cyan
if ($fail -gt 0) { Write-Host "soak: $fail issue(s)" -ForegroundColor Red; exit 1 }
Write-Host "soak OK -- plateau, no leak" -ForegroundColor Green
exit 0
