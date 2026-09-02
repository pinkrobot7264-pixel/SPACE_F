# Full Phase 0 build: Rust workspace (debug + release) and the C++ WinFsp adapter.
# The C++ step initialises the MSVC developer environment so Ninja invokes cl.exe,
# never MinGW (see docs/runbooks/ci.md).

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "== Rust workspace (debug) ==" -ForegroundColor Cyan
cargo build --workspace
if ($LASTEXITCODE) { throw "cargo build failed" }

Write-Host "== Rust workspace (release) ==" -ForegroundColor Cyan
cargo build --workspace --release
if ($LASTEXITCODE) { throw "cargo build --release failed" }

Write-Host "== C++ WinFsp adapter (MSVC + Ninja) ==" -ForegroundColor Cyan
$cl = Get-Command cl -ErrorAction SilentlyContinue
if ($cl) {
    cmake -B build -S . -G Ninja
    if ($LASTEXITCODE) { throw "cmake configure failed" }
    cmake --build build
    if ($LASTEXITCODE) { throw "cmake build failed" }
} else {
    $vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $vcvars)) {
        $vcvars = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
    }
    if (-not (Test-Path $vcvars)) { throw "vcvars64.bat not found; install VS 2022 C++ build tools" }
    cmd /c "call `"$vcvars`" >nul && cmake -B build -S . -G Ninja && cmake --build build"
    if ($LASTEXITCODE) { throw "C++ adapter build failed" }
}

Write-Host "BUILD OK" -ForegroundColor Green
