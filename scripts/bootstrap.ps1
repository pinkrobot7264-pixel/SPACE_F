# One-shot setup for a fresh clone: create runtime directories, seed config.toml,
# fetch crates, verify the toolchain. Idempotent.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

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

# Install the secret-scan pre-commit hook.
$hook = "$root\.git\hooks\pre-commit"
if (Test-Path "$root\.git\hooks") {
    Copy-Item "$root\scripts\pre-commit.sh" $hook -Force
    Write-Host "Installed .git/hooks/pre-commit (gitleaks)." -ForegroundColor Yellow
}

cargo fetch

& "$root\scripts\verify-env.ps1"
