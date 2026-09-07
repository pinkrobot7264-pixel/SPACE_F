# SPACE Phase 1 -- Windows compatibility matrix (manual section 16.3).
#
# **Records observed behaviour, not expected behaviour.** A cell says what the
# client actually did; a failure here is data, not necessarily a defect.
#
# Explorer, Notepad and 7-Zip are GUI clients and cannot be driven honestly from
# a script. They are left for a human and marked as such in the output, rather
# than being faked with a filesystem call that "stands in for" the GUI -- which
# would put unearned PASSes in the exit gate.

param(
    [string]$Drive = "S",
    [string]$Out = "docs\evidence\phase-1\compatibility-matrix.md"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"
$rows = @()

function Try-Op($client, $op, [scriptblock]$body) {
    try {
        & $body
        return [pscustomobject]@{ Client = $client; Op = $op; Result = "ok"; Note = "" }
    } catch {
        return [pscustomobject]@{ Client = $client; Op = $op; Result = "FAIL"; Note = $_.Exception.Message }
    }
}

Write-Host "=== compatibility matrix on $root ===" -ForegroundColor Cyan

# ---- PowerShell --------------------------------------------------------
$d = "$root\compat-ps"
$rows += Try-Op "PowerShell" "create"    { New-Item -ItemType Directory -Path $d -Force | Out-Null; Set-Content "$d\a.txt" "hello" }
$rows += Try-Op "PowerShell" "read"      { if ((Get-Content "$d\a.txt" -Raw).Trim() -ne "hello") { throw "content mismatch" } }
$rows += Try-Op "PowerShell" "write"     { Add-Content "$d\a.txt" "more" }
$rows += Try-Op "PowerShell" "rename"    { Rename-Item "$d\a.txt" "b.txt" }
$rows += Try-Op "PowerShell" "enumerate" { if ((Get-ChildItem $d | Measure-Object).Count -lt 1) { throw "empty listing" } }
$rows += Try-Op "PowerShell" "properties"{ $i = Get-Item "$d\b.txt"; if ($i.Length -le 0) { throw "zero length" }; if ($i.LastWriteTime.Year -lt 2020) { throw "bad mtime" } }
$rows += Try-Op "PowerShell" "delete"    { Remove-Item "$d\b.txt" -Force; Remove-Item $d -Recurse -Force }

# ---- cmd.exe -----------------------------------------------------------
$d = "$root\compat-cmd"
$rows += Try-Op "cmd.exe" "create"    { cmd /c "mkdir $d" | Out-Null; cmd /c "echo hello> $d\a.txt" | Out-Null; if (-not (Test-Path "$d\a.txt")) { throw "not created" } }
$rows += Try-Op "cmd.exe" "read"      { $o = cmd /c "type $d\a.txt"; if ($o -notmatch "hello") { throw "content mismatch: $o" } }
$rows += Try-Op "cmd.exe" "write"     { cmd /c "echo more>> $d\a.txt" | Out-Null }
$rows += Try-Op "cmd.exe" "rename"    { cmd /c "ren $d\a.txt b.txt" | Out-Null; if (-not (Test-Path "$d\b.txt")) { throw "rename failed" } }
$rows += Try-Op "cmd.exe" "enumerate" { $o = cmd /c "dir /b $d"; if ($o -notmatch "b.txt") { throw "not listed" } }
$rows += Try-Op "cmd.exe" "delete"    { cmd /c "del /q $d\b.txt" | Out-Null; cmd /c "rmdir /s /q $d" | Out-Null; if (Test-Path $d) { throw "not removed" } }

# ---- copy / xcopy ------------------------------------------------------
$src = Join-Path $env:TEMP "compat-src.bin"
if (-not (Test-Path $src)) { $b = New-Object byte[] (1MB); (New-Object Random 7).NextBytes($b); [IO.File]::WriteAllBytes($src, $b) }
$d = "$root\compat-copy"
$rows += Try-Op "copy" "create"    { New-Item -ItemType Directory -Path $d -Force | Out-Null; cmd /c "copy /y `"$src`" $d\c.bin" | Out-Null; if (-not (Test-Path "$d\c.bin")) { throw "copy failed" } }
$rows += Try-Op "copy" "read"      { $a = (Get-FileHash $src).Hash; $b = (Get-FileHash "$d\c.bin").Hash; if ($a -ne $b) { throw "hash mismatch" } }
$rows += Try-Op "xcopy" "create"   { cmd /c "xcopy /y /q `"$src`" $d\x.bin*" | Out-Null; if (-not (Test-Path "$d\x.bin")) { throw "xcopy failed" } }
$rows += Try-Op "copy" "delete"    { Remove-Item $d -Recurse -Force }

# ---- robocopy ----------------------------------------------------------
$tree = Join-Path $env:TEMP "compat-tree"
New-Item -ItemType Directory -Path $tree -Force | Out-Null
1..5 | ForEach-Object { $sd = Join-Path $tree "d$_"; New-Item -ItemType Directory -Path $sd -Force | Out-Null; 1..5 | ForEach-Object { Set-Content (Join-Path $sd "f$_.dat") ("y" * 256) } }
$rows += Try-Op "robocopy" "create"    { robocopy $tree "$root\compat-rc" /E /NFL /NDL /NJH /NJS /NP | Out-Null; if ($LASTEXITCODE -ge 8) { throw "robocopy in exit $LASTEXITCODE" } }
$rows += Try-Op "robocopy" "enumerate" { $n = (Get-ChildItem "$root\compat-rc" -Recurse -File | Measure-Object).Count; if ($n -ne 25) { throw "expected 25 files, saw $n" } }
$rows += Try-Op "robocopy" "read"      { $back = Join-Path $env:TEMP "compat-back"; robocopy "$root\compat-rc" $back /E /NFL /NDL /NJH /NJS /NP | Out-Null; if ($LASTEXITCODE -ge 8) { throw "robocopy out exit $LASTEXITCODE" } }
$rows += Try-Op "robocopy" "delete"    { Remove-Item "$root\compat-rc" -Recurse -Force }

# ---- .NET / System.IO --------------------------------------------------
$d = "$root\compat-net"
$rows += Try-Op ".NET System.IO" "create"    { [IO.Directory]::CreateDirectory($d) | Out-Null; [IO.File]::WriteAllText("$d\a.txt","hello") }
$rows += Try-Op ".NET System.IO" "read"      { if ([IO.File]::ReadAllText("$d\a.txt") -ne "hello") { throw "content mismatch" } }
$rows += Try-Op ".NET System.IO" "write"     { [IO.File]::AppendAllText("$d\a.txt","more") }
$rows += Try-Op ".NET System.IO" "rename"    { [IO.File]::Move("$d\a.txt","$d\b.txt") }
$rows += Try-Op ".NET System.IO" "enumerate" { if ([IO.Directory]::GetFiles($d).Count -ne 1) { throw "wrong count" } }
$rows += Try-Op ".NET System.IO" "delete"    { [IO.File]::Delete("$d\b.txt"); [IO.Directory]::Delete($d) }

# ---- report ------------------------------------------------------------
$build = (Get-ComputerInfo | Select-Object -ExpandProperty OsBuildNumber)
$osname = (Get-ComputerInfo | Select-Object -ExpandProperty OsName)
$winfsp = (& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" ver 2>&1 | Out-String).Trim()

$clients = $rows | Select-Object -ExpandProperty Client -Unique
$ops = @("create","read","write","rename","delete","enumerate","properties")

$md = @()
$md += "# Windows compatibility matrix (Phase 1 section 16.3)"
$md += ""
$md += "**Observed behaviour, not expected behaviour.**"
$md += ""
$md += "- Generated: $(Get-Date -Format o)"
$md += "- OS: $osname (build $build)"
$md += "- WinFsp: $winfsp"
$md += "- Mount: $root"
$md += ""
$md += "| Client | " + ($ops -join " | ") + " | notes |"
$md += "|---" * ($ops.Count + 2) + "|"

foreach ($c in $clients) {
    $cells = @()
    foreach ($o in $ops) {
        $r = $rows | Where-Object { $_.Client -eq $c -and $_.Op -eq $o } | Select-Object -First 1
        if ($null -eq $r) { $cells += "--" } elseif ($r.Result -eq "ok") { $cells += "ok" } else { $cells += "**FAIL**" }
    }
    $notes = ($rows | Where-Object { $_.Client -eq $c -and $_.Result -eq "FAIL" } | ForEach-Object { "$($_.Op): $($_.Note)" }) -join "; "
    $md += "| $c | " + ($cells -join " | ") + " | $notes |"
}

$md += "| Explorer | \<human\> | \<human\> | \<human\> | \<human\> | \<human\> | \<human\> | \<human\> | GUI client -- see EXPLORER-CHECKLIST.md |"
$md += "| Notepad | \<human\> | \<human\> | \<human\> | -- | -- | -- | -- | GUI client -- see EXPLORER-CHECKLIST.md |"
$md += "| 7-Zip | \<human\> | \<human\> | \<human\> | -- | -- | \<human\> | -- | GUI client -- see EXPLORER-CHECKLIST.md |"
$md += ""
$md += "`--` means the operation does not apply to that client."
$md += ""
$md += "GUI rows are deliberately left for a human. Driving Explorer from a"
$md += "script would put a filesystem call in a cell labelled ``Explorer``,"
$md += "which is an unearned PASS in the exit gate."

$failed = ($rows | Where-Object { $_.Result -eq "FAIL" }).Count
$md += ""
$md += "**Scripted result: $($rows.Count - $failed) ok, $failed failed.**"

New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$md -join "`r`n" | Out-File -Encoding utf8 $Out

Write-Host ""
$rows | Format-Table -AutoSize
Write-Host "Written to $Out" -ForegroundColor Green
if ($failed -gt 0) { Write-Host "$failed operation(s) failed -- recorded as observed" -ForegroundColor Yellow }
exit 0
