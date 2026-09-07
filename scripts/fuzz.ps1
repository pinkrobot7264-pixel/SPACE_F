# SPACE Phase 1 -- fuzzing (manual section 14.1).
#
# Pass condition: no panic escaping the boundary, no UB (ASan is on by default
# under cargo-fuzz), no hang beyond the deadline, every call returns a valid
# ErrorCode, no invariant violation. Each target runs >= 30 minutes.
#
# **What fuzzing does not prove: correctness.** It proves the absence of the
# specific classes above over the inputs explored. The exit gate must not imply
# otherwise.
#
# Every crash input becomes a permanent regression test in
# tests/fixtures/fuzz-corpus/.
#
# Windows note, and the reason this script exists rather than a bare
# `cargo fuzz run`: the targets are built with -Zsanitizer=address and need the
# MSVC ASan runtime (clang_rt.asan_dynamic-x86_64.dll) on PATH. Without it the
# binary builds fine and then dies with STATUS_DLL_NOT_FOUND (0xc0000135),
# which looks like a fuzzing failure and is not one.

param(
    [int]$Seconds = 1800,
    [string[]]$Targets = @("fuzz_path", "fuzz_handle", "fuzz_cursor",
                           "fuzz_read_args", "fuzz_write_args", "fuzz_op_sequence"),
    [string]$Out = "docs\evidence\phase-1\fuzz-results.txt"
)

$ErrorActionPreference = "Continue"
$repo = Split-Path $PSScriptRoot -Parent

# ---- locate the ASan runtime ------------------------------------------
$vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
$msvcRoot = & $vswhere -latest -products * -property installationPath
$asanDir = Get-ChildItem "$msvcRoot\VC\Tools\MSVC" -Directory |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName "bin\Hostx64\x64" } |
    Where-Object { Test-Path (Join-Path $_ "clang_rt.asan_dynamic-x86_64.dll") } |
    Select-Object -First 1

if (-not $asanDir) {
    Write-Host "FAIL  clang_rt.asan_dynamic-x86_64.dll not found under $msvcRoot" -ForegroundColor Red
    Write-Host "      cargo-fuzz targets will die with STATUS_DLL_NOT_FOUND without it." -ForegroundColor Red
    exit 1
}
$env:PATH = "$asanDir;$env:PATH"
Write-Host "ASan runtime: $asanDir" -ForegroundColor DarkGray

$lines = @()
$lines += "SPACE Phase 1 fuzzing (manual section 14.1)"
$lines += "date: $(Get-Date -Format o)"
$lines += "per-target budget: $Seconds seconds"
$lines += "asan runtime: $asanDir"
$lines += "commit: $(git -C $repo rev-parse HEAD)"
$lines += ""

$fail = 0
Push-Location (Join-Path $repo "fuzz")
try {
    foreach ($t in $Targets) {
        Write-Host "=== $t ($Seconds s) ===" -ForegroundColor Cyan
        $started = Get-Date
        $output = & cargo +nightly fuzz run $t -- -max_total_time=$Seconds -print_final_stats=1 2>&1 | Out-String
        $code = $LASTEXITCODE
        $elapsed = [math]::Round(((Get-Date) - $started).TotalSeconds)

        $runs = 0
        if ($output -match "Done (\d+) runs") { $runs = [int]$Matches[1] }
        elseif ($output -match "stat::number_of_executed_units:\s*(\d+)") { $runs = [int]$Matches[1] }

        if ($code -eq 0) {
            Write-Host "PASS  $t : $runs runs in ${elapsed}s, no crash" -ForegroundColor Green
            $lines += "PASS  $t  runs=$runs  seconds=$elapsed"
        } else {
            Write-Host "FAIL  $t : exit $code after ${elapsed}s" -ForegroundColor Red
            $lines += "FAIL  $t  exit=$code  seconds=$elapsed"
            # Keep the crashing input: every crash becomes a permanent
            # regression test.
            $lines += ($output -split "`n" | Select-Object -Last 40)
            $fail++
        }
    }
} finally {
    Pop-Location
}

# ---- preserve crash inputs as regression fixtures ----------------------
$artifacts = Join-Path $repo "fuzz\artifacts"
$corpusOut = Join-Path $repo "tests\fixtures\fuzz-corpus"
New-Item -ItemType Directory -Force -Path $corpusOut | Out-Null
if (Test-Path $artifacts) {
    $crashes = Get-ChildItem $artifacts -Recurse -File -ErrorAction SilentlyContinue
    if ($crashes) {
        Copy-Item $artifacts\* $corpusOut -Recurse -Force
        $lines += ""
        $lines += "crash inputs preserved as regression fixtures: $($crashes.Count)"
        Write-Host "$($crashes.Count) crash input(s) copied to tests/fixtures/fuzz-corpus" -ForegroundColor Yellow
    } else {
        $lines += ""
        $lines += "crash inputs: none"
    }
}

$lines += ""
$lines += "NOTE: fuzzing proves the absence of panics, UB, hangs, invalid error"
$lines += "codes and invariant violations over the inputs explored. It does NOT"
$lines += "prove correctness, and the exit gate must not imply otherwise."

New-Item -ItemType Directory -Force -Path (Split-Path (Join-Path $repo $Out)) | Out-Null
$lines -join "`r`n" | Out-File -Encoding utf8 (Join-Path $repo $Out)

Write-Host ""
Write-Host "results: $Out" -ForegroundColor Cyan
if ($fail -gt 0) { Write-Host "$fail target(s) failed" -ForegroundColor Red; exit 1 }
Write-Host "all fuzz targets clean" -ForegroundColor Green
exit 0
