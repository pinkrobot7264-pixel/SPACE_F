# Phase 0 verification: format, lint, tests, and the release fault-injection guard.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "== rustfmt ==" -ForegroundColor Cyan
cargo fmt --all -- --check
if ($LASTEXITCODE) { throw "formatting check failed" }

Write-Host "== clippy ==" -ForegroundColor Cyan
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE) { throw "clippy failed" }

# The integration harness and E2E suites spawn the built `space-cloud` binary;
# `cargo nextest` does not build workspace binaries, so build them first.
Write-Host "== build workspace (for test binaries) ==" -ForegroundColor Cyan
cargo build --workspace
if ($LASTEXITCODE) { throw "build failed" }

Write-Host "== nextest ==" -ForegroundColor Cyan
cargo nextest run --workspace
if ($LASTEXITCODE) { throw "tests failed" }

Write-Host "== release must not enable fault-injection ==" -ForegroundColor Cyan
$tree = cargo tree --workspace --edges features 2>&1 | Out-String
if ($tree -match "fault-injection") { throw "fault-injection feature is enabled" }

Write-Host "TESTS OK" -ForegroundColor Green
