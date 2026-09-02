# One-shot setup for a fresh clone: refresh PATH, create runtime directories,
# seed config.toml, ensure gitleaks, install the pre-commit hook, fetch crates,
# verify the toolchain. Idempotent.
#
# Assumes the machine-level installs from docs/runbooks/dev-machine-setup.md
# (Git, VS C++ build tools, WinFsp, Rust, PostgreSQL, Python) are already done.
# This script only handles per-clone state and the scriptable pieces.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Pick up anything a prior installer added to PATH in this same session
# (winget shims, PostgreSQL\18\bin, ...).
$env:PATH = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
[Environment]::GetEnvironmentVariable("Path", "User")

New-Item -ItemType Directory -Force -Path `
    "C:\SPACE\runtime\logs", "C:\SPACE\runtime\crash", `
    "C:\SPACE\runtime\cache\bytes", "C:\SPACE\runtime\cache\chunks", `
    "C:\SPACE\runtime\durable\wal", "C:\SPACE\runtime\durable\upload-queue", `
    "C:\SPACE\runtime\durable\sync-state", "C:\SPACE\runtime\temp", `
    "C:\SPACE\test-data\generated", "C:\SPACE\test-data\corruption", `
    "C:\SPACE\test-data\exports", "C:\SPACE\test-data\fixtures", `
    "C:\SPACE\secrets" | Out-Null

if (-not (Test-Path "$root\config.toml")) {
    Copy-Item "$root\config.example.toml" "$root\config.toml"
    Write-Host "Created config.toml from config.example.toml." -ForegroundColor Yellow
}

# Ensure gitleaks is available (documented Phase 0 install). Try PATH, then
# winget, then re-check. The pre-commit hook resolves the binary itself, so it
# does not depend on PATH once installed.
if (-not (Get-Command gitleaks -ErrorAction SilentlyContinue)) {
    Write-Host "gitleaks not found; installing via winget..." -ForegroundColor Yellow
    try {
        winget install -e --id Gitleaks.Gitleaks --silent `
            --accept-source-agreements --accept-package-agreements | Out-Null
    } catch {
        Write-Host "  winget install failed: $_" -ForegroundColor Red
    }
    $env:PATH = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
    [Environment]::GetEnvironmentVariable("Path", "User")
}
if (Get-Command gitleaks -ErrorAction SilentlyContinue) {
    Write-Host "gitleaks: $((Get-Command gitleaks).Source)" -ForegroundColor Green
} else {
    Write-Host "gitleaks still unavailable -- install it and re-run bootstrap." -ForegroundColor Red
}

# Install the secret-scan pre-commit hook.
if (Test-Path "$root\.git\hooks") {
    Copy-Item "$root\scripts\pre-commit.sh" "$root\.git\hooks\pre-commit" -Force
    Write-Host "Installed .git/hooks/pre-commit (gitleaks)." -ForegroundColor Yellow
}

cargo fetch

& "$root\scripts\verify-env.ps1"
