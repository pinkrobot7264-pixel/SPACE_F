# SPACE Phase 1 -- evidence collection (manual section 17.1).
#
# Answers "how do we know Phase 1 passed?" with artifacts, not opinions.
# Everything it writes is hashed, so a later reader can tell whether the
# evidence they are looking at is the evidence that was produced.

param([string]$Phase = "phase-1")

$ErrorActionPreference = "Continue"
$out = "docs\evidence\$Phase\$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -ItemType Directory -Force -Path $out | Out-Null
New-Item -ItemType Directory -Force -Path "$out\logs" | Out-Null
New-Item -ItemType Directory -Force -Path "$out\fuzz" | Out-Null

Write-Host "Collecting Phase 1 evidence into $out" -ForegroundColor Cyan

# ---- environment + toolchain -------------------------------------------
Get-ComputerInfo | Select-Object OsName,OsVersion,OsBuildNumber,CsTotalPhysicalMemory |
  ConvertTo-Json > "$out\environment.json"
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" ver > "$out\winfsp-version.txt" 2>&1
rustc --version  > "$out\toolchain.txt"
cargo --version >> "$out\toolchain.txt"
# `cl` is not on PATH unless a developer prompt is active; the cc crate finds it
# through vswhere. Record what vswhere reports so the C++ toolchain is named.
$vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
  "msvc: " + (& $vswhere -latest -products * -property installationPath) >> "$out\toolchain.txt"
}

# ---- revision + decisions ----------------------------------------------
git rev-parse HEAD > "$out\git-commit.txt"
git tag --points-at HEAD > "$out\git-tags.txt"
git status --porcelain > "$out\git-dirty.txt"
Get-ChildItem docs\decisions\*.md | Select-Object Name,LastWriteTime |
  Export-Csv "$out\adr-status.csv" -NoTypeInformation

# ---- requirement traceability ------------------------------------------
Copy-Item "docs\evidence\phase-1\REQUIREMENTS.md" "$out\" -ErrorAction SilentlyContinue

# ---- results ------------------------------------------------------------
# Plain `cargo test` as well as nextest: nextest's JSON is machine-readable,
# the plain run is what a human can read without a tool.
cargo test --workspace > "$out\test-results.txt" 2>&1
"cargo test exit code: $LASTEXITCODE" | Out-File -Append "$out\test-results.txt"

cargo test -p space-client-core --release > "$out\test-results-release.txt" 2>&1
"cargo test --release exit code: $LASTEXITCODE" | Out-File -Append "$out\test-results-release.txt"

cargo clippy --workspace --all-targets > "$out\clippy.txt" 2>&1
"clippy exit code: $LASTEXITCODE" | Out-File -Append "$out\clippy.txt"

# ---- safety -------------------------------------------------------------
& ".\scripts\os-safety-check.ps1" > "$out\os-safety.txt" 2>&1
"os-safety exit code: $LASTEXITCODE" | Out-File -Append "$out\os-safety.txt"

# ---- artifacts carried forward ------------------------------------------
Copy-Item "fuzz\artifacts\*" "$out\fuzz\" -Recurse -Force -ErrorAction SilentlyContinue
foreach ($f in "soak-samples.csv","procmon-writes.csv","compatibility-matrix.md",
                "mount-functional-test.txt","kill-matrix.txt","mount-stress.txt",
                "EXPLORER-CHECKLIST.md") {
  Copy-Item "docs\evidence\phase-1\$f" "$out\" -ErrorAction SilentlyContinue
}
Copy-Item "C:\SPACE\runtime\logs\*.log" "$out\logs\" -Force -ErrorAction SilentlyContinue

# ---- hashes -------------------------------------------------------------
Get-ChildItem $out -Recurse -File | Get-FileHash | Export-Csv "$out\hashes.csv" -NoTypeInformation

Write-Host "Evidence written to $out" -ForegroundColor Green
Write-Host "  files: $((Get-ChildItem $out -Recurse -File | Measure-Object).Count)"
exit 0
