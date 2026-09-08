# SPACE Phase 1 -- concurrent I/O stress (manual section 16.2).
#
# EXCLUSIVE: this script starts and stops a client. Nothing else may use the
# mount while it runs -- a concurrent test will see its mount vanish mid-call
# and report failures that look like defects and are not. See
# docs/evidence/phase-1/PHASE-1-CERTIFICATION.md, "Harness discipline".
#
# Section 16.2, mounted, CONCURRENTLY for 30 minutes:
#
#   A. create / write / read / verify / delete across the section 8.1 sizes
#   B. enumerate a 5,000-entry directory
#   C. rename between directories
#   D. robocopy a 500 MB tree in and out, with hash comparison
#
# This is deliberately NOT part of scripts/soak.ps1. The soak is section 16.5:
# a four-hour LEAK detector with pass rules about RSS slope, and its workload is
# a single sequential job that verifies file LENGTH rather than content. Section
# 16.2 asks for something different -- concurrency, a 5,000-entry enumeration,
# and byte-exact verification through robocopy -- so it gets its own runner with
# its own pass condition: every operation either succeeds or fails with a
# controlled error, and every byte read back equals the byte written.
#
# "verify" here means SHA-256 of content, not a length check. A length check
# would pass against a filesystem that returned the right number of wrong bytes.
#
# The 500 MB tree fits under vfs.max_bytes (L5, 1 GiB) with room for the other
# three workers. If L5 trips, that is the limit CORRECTLY enforcing itself and
# must be reported as such -- do not raise the limit to make this script pass.

param(
    [int]$Minutes = 30,
    [string]$Drive = "S",
    [string]$Config = ".\config.toml",
    [string]$Exe = ".\target\release\space-client.exe",
    [int]$TreeMB = 500,
    [int]$DirEntries = 5000,
    [string]$Out = "docs\evidence\phase-1\io-stress.txt"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$since = Get-Date
$deadline = $since.AddMinutes($Minutes)
$stage = Join-Path $env:TEMP "space-io-stress"
$fail = 0

$lines = @()
$lines += "SPACE Phase 1 -- concurrent I/O stress (manual section 16.2)"
$lines += "date: $(Get-Date -Format o)"
$lines += "duration: $Minutes minutes, four concurrent workers"
$lines += "commit: $(git rev-parse HEAD)"
$lines += ""

Write-Host "=== section 16.2 concurrent I/O stress: $Minutes minutes ===" -ForegroundColor Cyan

# ---- stage the 500 MB source tree on a real disk, once ------------------
# Built before the clock starts: the requirement is 30 minutes of concurrent
# filesystem activity, not 30 minutes minus however long staging takes.
$src = Join-Path $stage "src"
$dst = Join-Path $stage "dst"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $src, $dst | Out-Null

Write-Host "staging a $TreeMB MB source tree at $src ..." -ForegroundColor DarkGray
$rand = New-Object Random 20260908
$fileMB = 25
$count = [int]($TreeMB / $fileMB)
for ($i = 1; $i -le $count; $i++) {
    $sub = Join-Path $src ("d" + ($i % 5))
    New-Item -ItemType Directory -Force -Path $sub | Out-Null
    $buf = New-Object byte[] ($fileMB * 1MB)
    $rand.NextBytes($buf)
    [IO.File]::WriteAllBytes((Join-Path $sub "f$i.bin"), $buf)
}
$srcHashes = Get-ChildItem $src -Recurse -File | ForEach-Object {
    [pscustomobject]@{ Rel = $_.FullName.Substring($src.Length); Hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash }
}
$lines += "staged tree: $($srcHashes.Count) files, $TreeMB MB, hashed with SHA-256"
Write-Host "staged $($srcHashes.Count) files" -ForegroundColor DarkGray

# ---- mount --------------------------------------------------------------
$p = Start-Process -PassThru -FilePath $Exe -ArgumentList "--config", $Config `
    -RedirectStandardOutput "C:\SPACE\runtime\logs\iostress-out.log" `
    -RedirectStandardError  "C:\SPACE\runtime\logs\iostress-err.log"

$mounted = $false
for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Milliseconds 250
    if (Test-Path "$root\") { $mounted = $true; break }
}
if (-not $mounted) {
    Write-Host "FAIL  mount did not appear" -ForegroundColor Red
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    exit 1
}

# ---- worker A: the section 8.1 size matrix, byte-verified ---------------
$wA = Start-Job -Name ioA -ScriptBlock {
    param($r, $end)
    $sizes = @(0, 1, 4095, 4096, 4097, 1048576)
    $sha = [Security.Cryptography.SHA256]::Create()
    $rand = New-Object Random 11
    $ok = 0
    New-Item -ItemType Directory -Path "$r\sizes" -Force -ErrorAction SilentlyContinue | Out-Null
    while ((Get-Date) -lt $end) {
        foreach ($s in $sizes) {
            $f = "$r\sizes\a$s.bin"
            try {
                $bytes = New-Object byte[] $s
                if ($s -gt 0) { $rand.NextBytes($bytes) }
                $want = [BitConverter]::ToString($sha.ComputeHash($bytes))
                [IO.File]::WriteAllBytes($f, $bytes)
                $back = [IO.File]::ReadAllBytes($f)
                $got = [BitConverter]::ToString($sha.ComputeHash($back))
                if ($got -ne $want) { Write-Output "ERROR A: content mismatch at size $s" }
                elseif ($back.Length -ne $s) { Write-Output "ERROR A: length $($back.Length) != $s" }
                else { $ok++ }
                Remove-Item $f -Force -ErrorAction Stop
                if (Test-Path $f) { Write-Output "ERROR A: $f survived delete" }
            } catch {
                Write-Output "ERROR A: size $s -- $($_.Exception.Message)"
            }
        }
    }
    Write-Output "STAT A: $ok verified round trips across the size matrix"
} -ArgumentList $root, $deadline

# ---- worker B: enumerate a 5,000-entry directory ------------------------
$wB = Start-Job -Name ioB -ScriptBlock {
    param($r, $end, $n)
    $d = "$r\wide"
    try {
        New-Item -ItemType Directory -Path $d -Force -ErrorAction Stop | Out-Null
        for ($i = 1; $i -le $n; $i++) { [IO.File]::WriteAllText("$d\e$i.txt", "$i") }
    } catch {
        Write-Output "ERROR B: building the $n-entry directory -- $($_.Exception.Message)"
        return
    }
    $rounds = 0
    while ((Get-Date) -lt $end) {
        try {
            $c = (Get-ChildItem $d -ErrorAction Stop | Measure-Object).Count
            if ($c -ne $n) { Write-Output "ERROR B: enumerated $c of $n entries" }
            else { $rounds++ }
        } catch {
            Write-Output "ERROR B: enumeration -- $($_.Exception.Message)"
        }
    }
    Write-Output "STAT B: $rounds full enumerations of a $n-entry directory"
} -ArgumentList $root, $deadline, $DirEntries

# ---- worker C: rename between directories -------------------------------
$wC = Start-Job -Name ioC -ScriptBlock {
    param($r, $end)
    $a = "$r\ren\a"; $b = "$r\ren\b"
    New-Item -ItemType Directory -Path $a, $b -Force -ErrorAction SilentlyContinue | Out-Null
    $i = 0; $ok = 0
    while ((Get-Date) -lt $end) {
        $i++
        try {
            $payload = "payload-$i"
            [IO.File]::WriteAllText("$a\m$i.txt", $payload)
            Move-Item "$a\m$i.txt" "$b\m$i.txt" -ErrorAction Stop
            if (Test-Path "$a\m$i.txt") { Write-Output "ERROR C: source survived the rename" }
            $back = [IO.File]::ReadAllText("$b\m$i.txt")
            if ($back -ne $payload) { Write-Output "ERROR C: content changed across a rename" }
            else { $ok++ }
            Remove-Item "$b\m$i.txt" -Force -ErrorAction Stop
        } catch {
            Write-Output "ERROR C: iteration $i -- $($_.Exception.Message)"
        }
    }
    Write-Output "STAT C: $ok verified renames between directories"
} -ArgumentList $root, $deadline

# ---- worker D: robocopy 500 MB in and out, hash-compared ----------------
$wD = Start-Job -Name ioD -ScriptBlock {
    param($r, $end, $src, $dst, $hashes)
    $rounds = 0
    while ((Get-Date) -lt $end) {
        $inDir = "$r\robo"
        try {
            # IN
            $null = robocopy $src $inDir /E /NFL /NDL /NJH /NJS /R:0 /W:0
            if ($LASTEXITCODE -ge 8) { Write-Output "ERROR D: robocopy in failed, exit $LASTEXITCODE"; break }

            # hash-compare every file INSIDE the filesystem
            foreach ($h in $hashes) {
                $f = Join-Path $inDir $h.Rel
                if (-not (Test-Path $f)) { Write-Output "ERROR D: $($h.Rel) missing after copy in"; continue }
                $got = (Get-FileHash $f -Algorithm SHA256).Hash
                if ($got -ne $h.Hash) { Write-Output "ERROR D: hash mismatch in-tree for $($h.Rel)" }
            }

            # OUT
            Remove-Item "$dst\*" -Recurse -Force -ErrorAction SilentlyContinue
            $null = robocopy $inDir $dst /E /NFL /NDL /NJH /NJS /R:0 /W:0
            if ($LASTEXITCODE -ge 8) { Write-Output "ERROR D: robocopy out failed, exit $LASTEXITCODE"; break }

            foreach ($h in $hashes) {
                $f = Join-Path $dst $h.Rel
                if (-not (Test-Path $f)) { Write-Output "ERROR D: $($h.Rel) missing after copy out"; continue }
                $got = (Get-FileHash $f -Algorithm SHA256).Hash
                if ($got -ne $h.Hash) { Write-Output "ERROR D: hash mismatch round-tripped for $($h.Rel)" }
            }

            $rounds++
            Remove-Item $inDir -Recurse -Force -ErrorAction SilentlyContinue
        } catch {
            Write-Output "ERROR D: $($_.Exception.Message)"
        }
    }
    Write-Output "STAT D: $rounds hash-compared 500 MB robocopy round trips"
} -ArgumentList $root, $deadline, $src, $dst, $srcHashes

# ---- wait, reporting progress -------------------------------------------
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 60
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($null -eq $proc) {
        Write-Host "FAIL  the client died during the stress run" -ForegroundColor Red
        $lines += "FAIL  the client process died mid-run"
        $fail++
        break
    }
    $el = [math]::Round(((Get-Date) - $since).TotalMinutes, 1)
    Write-Host ("  {0,5} min  rss={1,12:N0}  handles={2,5}" -f $el, $proc.WorkingSet64, $proc.HandleCount)
}

$jobs = @($wA, $wB, $wC, $wD)
$jobs | Stop-Job -ErrorAction SilentlyContinue
$output = $jobs | ForEach-Object { Receive-Job $_ -ErrorAction SilentlyContinue }
$jobs | Remove-Job -Force -ErrorAction SilentlyContinue

$errors = @($output | Where-Object { $_ -like "ERROR *" })
$stats  = @($output | Where-Object { $_ -like "STAT *" })

$lines += ""
$lines += "--- worker totals ---"
foreach ($s in $stats) { $lines += "  $s" }
$lines += ""
if ($errors.Count -gt 0) {
    $lines += "--- $($errors.Count) worker error(s) ---"
    # Record every distinct error. Truncating this to a tidy summary would hide
    # exactly the evidence the run exists to produce.
    foreach ($e in ($errors | Group-Object | Sort-Object Count -Descending)) {
        $lines += "  [x$($e.Count)] $($e.Name)"
    }
    $fail += $errors.Count
} else {
    $lines += "worker errors: none"
}

Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue

& "$PSScriptRoot\os-safety-check.ps1" -Since $since -Drive $Drive
$osExit = $LASTEXITCODE
$lines += ""
$lines += "os-safety-check exit: $osExit"
if ($osExit -ne 0) { $fail++ }

$lines += ""
$lines += "result: $(if ($fail -gt 0) { "$fail FAILURE(S)" } else { 'PASS -- no errors across four concurrent workers' })"
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$lines -join "`r`n" | Out-File -Encoding utf8 $Out

Write-Host ""
foreach ($s in $stats) { Write-Host "  $s" -ForegroundColor Cyan }
if ($fail -gt 0) {
    Write-Host "`n$fail I/O stress failure(s) -- see $Out" -ForegroundColor Red
    $errors | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
    exit 1
}
Write-Host "`nsection 16.2 concurrent I/O stress OK" -ForegroundColor Green
exit 0
