$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

Write-Host "Building Rust workspace..."
cargo build --workspace

Write-Host "Building Rust workspace (release)..."
cargo build --workspace --release

$CMake = Get-Command cmake -ErrorAction SilentlyContinue
$Ninja = Get-Command ninja -ErrorAction SilentlyContinue

if (-not $CMake) {
    $CMakePath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"

    if (-not (Test-Path $CMakePath)) {
        throw "CMake was not found at '$CMakePath'."
    }

    $CMakeExe = $CMakePath
}
else {
    $CMakeExe = $CMake.Source
}

if (-not $Ninja) {
    $NinjaPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja\ninja.exe"

    if (-not (Test-Path $NinjaPath)) {
        throw "Ninja was not found at '$NinjaPath'."
    }

    $NinjaExe = $NinjaPath
}
else {
    $NinjaExe = $Ninja.Source
}

Write-Host "Using CMake: $CMakeExe"
Write-Host "Using Ninja: $NinjaExe"

Write-Host "Configuring CMake..."
& $CMakeExe -B build -S . -G Ninja "-DCMAKE_MAKE_PROGRAM=$NinjaExe"

Write-Host "Building C++ adapter..."
& $CMakeExe --build build

Write-Host "BUILD OK"