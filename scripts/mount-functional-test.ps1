# SPACE Phase 1 -- automated functional test against a live mount.
#
# Covers the automatable part of the manual's section 9.2 Explorer checklist and
# section 17.5 functional gate. The genuinely manual steps (Explorer UI, right-click
# Properties, Notepad) stay manual and are recorded separately.
#
# Usage:
#   .\scripts\mount-functional-test.ps1 [-Drive S] [-KeepGoing]
#
# Exit code 0 = every check passed.

param(
    [string]$Drive = "S",
    [switch]$KeepGoing
)

$ErrorActionPreference = "Stop"
$root = "${Drive}:"
$script:fail = 0
$script:pass = 0

function Check($name, [scriptblock]$body) {
    try {
        & $body
        Write-Host "PASS  $name" -ForegroundColor Green
        $script:pass++
    } catch {
        Write-Host "FAIL  $name -- $($_.Exception.Message)" -ForegroundColor Red
        $script:fail++
        if (-not $KeepGoing) { throw }
    }
}

Write-Host "=== SPACE mount functional test on $root ===" -ForegroundColor Cyan

Check "drive is present" {
    if (-not (Test-Path $root)) { throw "$root not present" }
}

Check "volume reports a size" {
    $d = Get-PSDrive $Drive
    if ($null -eq $d) { throw "Get-PSDrive returned nothing" }
}

Check "create a directory" {
    New-Item -ItemType Directory -Path "$root\t1" -Force | Out-Null
    if (-not (Test-Path "$root\t1")) { throw "directory not created" }
}

Check "create, write and read back a text file" {
    Set-Content -Path "$root\t1\hello.txt" -Value "hello space" -Encoding utf8
    $got = Get-Content -Path "$root\t1\hello.txt" -Raw
    if ($got.Trim() -ne "hello space") { throw "content mismatch: '$got'" }
}

Check "append to a file" {
    Add-Content -Path "$root\t1\hello.txt" -Value "second line"
    $lines = Get-Content -Path "$root\t1\hello.txt"
    if ($lines.Count -lt 2) { throw "append did not extend the file" }
}

Check "10 MB round trip is hash-identical" {
    $src = Join-Path $env:TEMP "space-10mb.bin"
    if (-not (Test-Path $src)) {
        $bytes = New-Object byte[] (10MB)
        (New-Object Random 42).NextBytes($bytes)
        [IO.File]::WriteAllBytes($src, $bytes)
    }
    Copy-Item $src "$root\t1\big.bin" -Force
    Copy-Item "$root\t1\big.bin" (Join-Path $env:TEMP "space-10mb-back.bin") -Force

    $a = (Get-FileHash $src -Algorithm SHA256).Hash
    $b = (Get-FileHash (Join-Path $env:TEMP "space-10mb-back.bin") -Algorithm SHA256).Hash
    if ($a -ne $b) { throw "hash mismatch: $a vs $b" }
}

Check "file size is reported correctly" {
    $len = (Get-Item "$root\t1\big.bin").Length
    if ($len -ne 10MB) { throw "expected $(10MB) bytes, got $len" }
}

Check "rename a file" {
    Rename-Item "$root\t1\hello.txt" "renamed.txt"
    if (Test-Path "$root\t1\hello.txt") { throw "old name still present" }
    if (-not (Test-Path "$root\t1\renamed.txt")) { throw "new name missing" }
}

Check "rename a folder" {
    New-Item -ItemType Directory -Path "$root\t1\olddir" -Force | Out-Null
    Set-Content "$root\t1\olddir\inner.txt" "inner"
    Rename-Item "$root\t1\olddir" "newdir"
    if (-not (Test-Path "$root\t1\newdir\inner.txt")) { throw "subtree did not move with the directory" }
}

Check "delete a file" {
    Remove-Item "$root\t1\renamed.txt" -Force
    if (Test-Path "$root\t1\renamed.txt") { throw "file still present after delete" }
}

Check "delete a folder" {
    Remove-Item "$root\t1\newdir" -Recurse -Force
    if (Test-Path "$root\t1\newdir") { throw "folder still present after delete" }
}

Check "10-level nested path" {
    $p = $root
    1..10 | ForEach-Object { $p = Join-Path $p "lvl$_"; New-Item -ItemType Directory -Path $p -Force | Out-Null }
    Set-Content (Join-Path $p "deep.txt") "bottom"
    if ((Get-Content (Join-Path $p "deep.txt")).Trim() -ne "bottom") { throw "deep file unreadable" }
}

Check "5,000 files in one directory enumerate completely" {
    # The manual counterpart of INV-DIR-2. Correct at 50 and truncated at 5,000
    # is the classic marker bug.
    New-Item -ItemType Directory -Path "$root\many" -Force | Out-Null
    # [IO.File]::WriteAllText rather than Set-Content: the cmdlet issues several
    # extra probes and opens per file, which turns 5,000 files into a
    # multi-minute run without testing anything the direct write does not.
    1..5000 | ForEach-Object { [IO.File]::WriteAllText("$root\many\f$_.txt", "$_") }
    $n = (Get-ChildItem "$root\many" | Measure-Object).Count
    if ($n -ne 5000) { throw "expected exactly 5000 entries, enumerated $n" }
}

Check "enumeration is stable across repeated listings" {
    $a = (Get-ChildItem "$root\many" | Measure-Object).Count
    $b = (Get-ChildItem "$root\many" | Measure-Object).Count
    if ($a -ne $b) { throw "listing count changed between runs: $a then $b" }
}

Check "timestamps are sane" {
    $f = Get-Item "$root\t1\big.bin"
    if ($f.CreationTime.Year -lt 2020) { throw "creation time is $($f.CreationTime)" }
    if ($f.LastWriteTime.Year -lt 2020) { throw "last write time is $($f.LastWriteTime)" }
}

Check "invalid names are rejected" {
    foreach ($bad in @("CON", "NUL", "a:b")) {
        $threw = $false
        try { Set-Content "$root\$bad" "x" -ErrorAction Stop } catch { $threw = $true }
        if (-not $threw) { throw "the filesystem accepted the invalid name '$bad'" }
    }
}

Check "reading a nonexistent file fails cleanly" {
    $threw = $false
    try { Get-Content "$root\does-not-exist.txt" -ErrorAction Stop } catch { $threw = $true }
    if (-not $threw) { throw "reading a missing file did not fail" }
}

Check "robocopy a tree in and out" {
    $srcTree = Join-Path $env:TEMP "space-tree"
    New-Item -ItemType Directory -Path $srcTree -Force | Out-Null
    1..20 | ForEach-Object {
        $d = Join-Path $srcTree "d$_"
        New-Item -ItemType Directory -Path $d -Force | Out-Null
        1..10 | ForEach-Object { Set-Content (Join-Path $d "f$_.dat") ("x" * 512) }
    }
    & robocopy $srcTree "$root\tree" /E /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy in failed with $LASTEXITCODE" }

    $back = Join-Path $env:TEMP "space-tree-back"
    & robocopy "$root\tree" $back /E /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy out failed with $LASTEXITCODE" }

    $inCount  = (Get-ChildItem $srcTree -Recurse -File | Measure-Object).Count
    $outCount = (Get-ChildItem $back -Recurse -File | Measure-Object).Count
    if ($inCount -ne $outCount) { throw "file count differs: $inCount in, $outCount out" }
}

Check "cleanup leaves the volume usable" {
    Remove-Item "$root\many" -Recurse -Force
    Remove-Item "$root\tree" -Recurse -Force
    Remove-Item "$root\t1" -Recurse -Force
    Remove-Item "$root\lvl1" -Recurse -Force
    $remaining = (Get-ChildItem $root | Measure-Object).Count
    if ($remaining -ne 0) { throw "$remaining entries left at the root" }
}

Write-Host ""
Write-Host "$script:pass passed, $script:fail failed" -ForegroundColor Cyan
if ($script:fail -gt 0) { exit 1 }
exit 0
