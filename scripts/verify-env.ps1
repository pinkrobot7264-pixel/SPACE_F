# Verifies the development toolchain (Session 1 of the Phase 0 manual).
# Exit 0 and "Environment OK." means a clean machine can build and test SPACE.

$fail = 0

function Check($name, $cmd, $pattern) {
    try {
        $out = Invoke-Expression $cmd 2>&1 | Out-String
        if ($out -match $pattern) { Write-Host "PASS $name" -ForegroundColor Green }
        else { Write-Host "FAIL $name (got: $($out.Trim()))" -ForegroundColor Red; $script:fail++ }
    } catch { Write-Host "FAIL $name (not found)" -ForegroundColor Red; $script:fail++ }
}

Check "git"      "git --version"           "\d+\.\d+\.\d+"
Check "cmake"    "cmake --version"         "3\.\d+"
Check "ninja"    "ninja --version"         "\d+\.\d+"
Check "rustc"    "rustc --version"         "\d+\.\d+\.\d+"
Check "cargo"    "cargo --version"         "\d+\.\d+\.\d+"
Check "nextest"  "cargo nextest --version" "\d+\.\d+"
Check "clippy"   "cargo clippy --version"  "\d+\.\d+"
Check "rustfmt"  "cargo fmt --version"     "\d+\.\d+"
Check "psql"     "psql --version"          "1[89]\."
Check "gitleaks" "gitleaks version"        "\d+\.\d+"
Check "winfsp"   "& 'C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe' ver" "\d+\.\d+"

foreach ($p in @(
        "C:\Program Files (x86)\WinFsp\inc\winfsp\winfsp.h",
        "C:\Program Files (x86)\WinFsp\lib\winfsp-x64.lib",
        "C:\SPACE\runtime", "C:\SPACE\test-data", "C:\SPACE\secrets")) {
    if (Test-Path $p) { Write-Host "PASS $p" -ForegroundColor Green }
    else { Write-Host "FAIL $p missing" -ForegroundColor Red; $fail++ }
}

# cl.exe is only on PATH inside a VS developer prompt; warn, do not fail.
if (Get-Command cl -ErrorAction SilentlyContinue) {
    Write-Host "PASS cl (MSVC on PATH)" -ForegroundColor Green
} else {
    Write-Host "WARN cl not on PATH -- run from 'x64 Native Tools' prompt or use scripts/build.ps1" -ForegroundColor Yellow
}

if ($fail -gt 0) { Write-Host "`n$fail check(s) failed." -ForegroundColor Red; exit 1 }
Write-Host "`nEnvironment OK." -ForegroundColor Green
