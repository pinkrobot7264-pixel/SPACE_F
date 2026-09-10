# SPACE Phase 1 -- evidence collection (manual section 17.1).
#
# Answers "how do we know Phase 1 passed?" with artifacts, not opinions.
# Everything it writes is hashed, so a later reader can tell whether the
# evidence they are looking at is the evidence that was produced.
#
# THIS SCRIPT MUST RUN LAST. It is the gate, and it is the one place where a
# weak check does the most damage, so three rules are enforced here:
#
#   1. A missing artifact is REPORTED, never silently skipped. The previous
#      version copied everything with -ErrorAction SilentlyContinue, so an
#      evidence file that was never produced simply did not appear in the
#      bundle and nothing said so.
#
#   2. The exit code reflects the results. The previous version ended in a
#      bare `exit 0`, so the final gate of Phase 1 reported success even when
#      cargo test, clippy or the OS safety check had just failed.
#
#   3. Stale evidence is flagged. An artifact older than the commit being
#      certified describes a different build. It is carried forward but marked,
#      because collecting evidence early and leaving it stale is how a bundle
#      ends up certifying code that never ran.

param([string]$Phase = "phase-1")

$ErrorActionPreference = "Continue"
$src = "docs\evidence\$Phase"
$out = "$src\$(Get-Date -Format yyyyMMdd-HHmmss)"

# Capture the working-tree state BEFORE creating the output directory.
#
# This used to be read after the directory existed, so the collector always saw
# its own untracked output as a dirty tree and reported "WORKING TREE DIRTY" on
# an otherwise clean checkout. Measured: the sole entry was
# `?? docs/evidence/phase-1/<timestamp>/`. A gate that fails because of its own
# side effect teaches the reader to ignore it.
$dirtyBefore = @(git status --porcelain | Where-Object { $_ })

New-Item -ItemType Directory -Force -Path $out, "$out\logs", "$out\fuzz" | Out-Null

$fail = 0
$notes = @()

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

$headSha = (git rev-parse HEAD).Trim()
$headDate = [datetime](git show -s --format=%cI HEAD)
$dirty = $dirtyBefore
if ($dirty) {
    $notes += "WORKING TREE DIRTY at collection time -- the bundle does not describe a committed state."
    Write-Host "WARN  working tree is dirty" -ForegroundColor Yellow
    $fail++
}

# ---- results ------------------------------------------------------------
# Plain `cargo test` as well as nextest: nextest's JSON is machine-readable,
# the plain run is what a human can read without a tool.
function Run-Check($name, $file, [scriptblock]$body) {
    & $body > "$out\$file" 2>&1
    $code = $LASTEXITCODE
    "$name exit code: $code" | Out-File -Append "$out\$file"
    if ($code -ne 0) {
        Write-Host "FAIL  $name exited $code" -ForegroundColor Red
        $script:notes += "FAIL  $name exited $code (see $file)"
        $script:fail++
    } else {
        Write-Host "PASS  $name" -ForegroundColor Green
        $script:notes += "PASS  $name"
    }
}

Run-Check "cargo test --workspace"          "test-results.txt"         { cargo test --workspace }
Run-Check "cargo test -p core --release"    "test-results-release.txt" { cargo test -p space-client-core --release }
Run-Check "cargo clippy -D warnings"        "clippy.txt"               { cargo clippy --workspace --all-targets -- -D warnings }
Run-Check "cargo fmt --check"               "fmt.txt"                  { cargo fmt --all -- --check }
Run-Check "os-safety-check"                 "os-safety.txt"            { & ".\scripts\os-safety-check.ps1" }

# ---- artifacts carried forward ------------------------------------------
# Every artifact Phase 1 requires, and who has to produce it. A missing
# automated artifact is a FAILURE -- it means a required run never happened, or
# happened and produced nothing. A missing human artifact is not a failure of
# the automation, but it does mean Phase 1 is not certifiable yet, and it is
# reported that way rather than being quietly absent.
$artifacts = @(
    @{ Name = "REQUIREMENTS.md";                    Kind = "auto" }
    @{ Name = "PHASE-1-CERTIFICATION.md";           Kind = "auto" }
    @{ Name = "LEDGER.md";                          Kind = "auto" }
    @{ Name = "fuzz-results.txt";                   Kind = "auto" }
    @{ Name = "fuzz-op-sequence-fullbudget.txt";    Kind = "auto" }
    @{ Name = "mount-functional-test.txt";          Kind = "auto" }
    @{ Name = "kill-matrix.txt";                    Kind = "auto" }
    @{ Name = "mount-stress.txt";                   Kind = "auto" }
    @{ Name = "ntstatus-matrix.md";                 Kind = "auto" }
    @{ Name = "compatibility-matrix.md";            Kind = "auto" }
    @{ Name = "io-stress.txt";                      Kind = "auto" }
    @{ Name = "soak-samples.csv";                   Kind = "auto" }
    @{ Name = "shutdown-states.txt";                Kind = "auto" }
    @{ Name = "fault-injection.txt";                Kind = "auto" }
    @{ Name = "enumeration-scaling.md";             Kind = "auto" }
    @{ Name = "EXPLORER-CHECKLIST.md";              Kind = "auto" }
    @{ Name = "procmon-writes.csv";                 Kind = "human" }
    @{ Name = "explorer";                           Kind = "human" }
)

$inventory = @()
foreach ($a in $artifacts) {
    $p = Join-Path $src $a.Name
    if (Test-Path $p) {
        Copy-Item $p "$out\" -Recurse -Force
        $item = Get-Item $p
        $stale = $item.LastWriteTime -lt $headDate
        $inventory += [pscustomobject]@{
            Artifact = $a.Name; Kind = $a.Kind; Present = "yes"
            Modified = $item.LastWriteTime.ToString("o")
            Status = if ($stale) { "STALE (older than HEAD)" } else { "current" }
        }
        if ($stale) {
            Write-Host "STALE $($a.Name) predates HEAD" -ForegroundColor Yellow
            $notes += "STALE $($a.Name) was last written before the commit being certified."
            $fail++
        }
    } else {
        $inventory += [pscustomobject]@{
            Artifact = $a.Name; Kind = $a.Kind; Present = "NO"; Modified = ""
            Status = if ($a.Kind -eq "human") { "OUTSTANDING (needs human)" } else { "MISSING" }
        }
        if ($a.Kind -eq "human") {
            Write-Host "NEEDS HUMAN  $($a.Name) not present" -ForegroundColor Yellow
            $notes += "NEEDS HUMAN  $($a.Name) has not been produced."
        } else {
            Write-Host "MISSING  $($a.Name)" -ForegroundColor Red
            $notes += "MISSING  $($a.Name) -- a required automated run did not produce it."
            $fail++
        }
    }
}

Copy-Item "fuzz\artifacts\*" "$out\fuzz\" -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item "C:\SPACE\runtime\logs\*.log" "$out\logs\" -Force -ErrorAction SilentlyContinue

$inventory | Export-Csv "$out\inventory.csv" -NoTypeInformation

# ---- manifest -----------------------------------------------------------
$md = @()
$md += "# Phase 1 evidence bundle"
$md += ""
$md += "- Collected: $(Get-Date -Format o)"
$md += "- Commit: ``$headSha`` (committed $($headDate.ToString('o')))"
$md += "- Working tree: $(if ($dirty) { '**DIRTY**' } else { 'clean' })"

# Staleness is judged against HEAD, strictly. But a reader deciding what a STALE
# flag means needs to know whether HEAD actually changed anything testable since
# the evidence was produced, or whether the intervening commits only touched
# documentation. Both facts are reported; neither replaces the other, and the
# strict comparison above is unchanged.
$lastCode = (git log -1 --format='%h %cI %s' -- ':!docs' 2>$null)
$md += "- Last commit touching a path outside ``docs/``: $(if ($lastCode) { "``$lastCode``" } else { 'none found' })"
if ($lastCode) {
    $lastCodeSha = ($lastCode -split ' ')[0]
    $touched = @(git show --name-only --format='' $lastCodeSha 2>$null | Where-Object { $_ -and $_ -notmatch '^docs/' })
    $md += "- Files it changed outside ``docs/``: $(if ($touched.Count) { '`' + ($touched -join '`, `') + '`' } else { 'none' })"
    $md += ""
    $md += "  A STALE artifact predates HEAD. Whether that matters depends on what"
    $md += "  changed in between: a commit touching only documentation or test"
    $md += "  scaffolding cannot alter product behaviour, while one touching"
    $md += "  ``client/``, ``contracts/`` or ``scripts/`` may invalidate the run that"
    $md += "  produced the artifact. This collector does not make that judgement --"
    $md += "  it reports both so the reader can."
}
$md += ""
$md += "## Checks run at collection time"
$md += ""
foreach ($n in $notes | Where-Object { $_ -like "PASS*" -or $_ -like "FAIL*" }) { $md += "- $n" }
$md += ""
$md += "## Artifact inventory"
$md += ""
$md += "| artifact | produced by | present | last written | status |"
$md += "|---|---|---|---|---|"
foreach ($i in $inventory) {
    $md += "| ``$($i.Artifact)`` | $($i.Kind) | $($i.Present) | $($i.Modified) | $($i.Status) |"
}
$md += ""
$outstanding = @($inventory | Where-Object { $_.Status -like "OUTSTANDING*" })
$missing     = @($inventory | Where-Object { $_.Status -eq "MISSING" })
$stale       = @($inventory | Where-Object { $_.Status -like "STALE*" })
$md += "## Verdict"
$md += ""
if ($fail -eq 0 -and $outstanding.Count -eq 0) {
    $md += "All required evidence is present, current, and every collection-time check passed."
} else {
    $md += "**This bundle does not certify Phase 1.**"
    $md += ""
    if ($missing.Count)     { $md += "- $($missing.Count) required automated artifact(s) MISSING." }
    if ($stale.Count)       { $md += "- $($stale.Count) artifact(s) STALE -- they predate the commit being certified." }
    if ($outstanding.Count) { $md += "- $($outstanding.Count) human validation(s) OUTSTANDING." }
    foreach ($n in $notes | Where-Object { $_ -like "FAIL*" -or $_ -like "WORKING*" }) { $md += "- $n" }
}
$md -join "`r`n" | Out-File -Encoding utf8 "$out\MANIFEST.md"

# ---- hashes -------------------------------------------------------------
# Hash into a temp file OUTSIDE the bundle, then move it in.
#
# Writing straight to "$out\hashes.csv" made the pipeline enumerate the file it
# was in the middle of creating: Get-FileHash then failed with "the process
# cannot access the file ... because it is being used by another process", and
# the bundle ended up with no hashes at all -- the one artifact that lets a
# later reader tell whether the evidence they hold is the evidence produced.
$hashTmp = Join-Path $env:TEMP "space-evidence-hashes-$(Get-Date -Format yyyyMMddHHmmss).csv"
Get-ChildItem $out -Recurse -File | Get-FileHash | Export-Csv $hashTmp -NoTypeInformation
Move-Item $hashTmp "$out\hashes.csv" -Force

Write-Host ""
Write-Host "Evidence written to $out" -ForegroundColor Cyan
Write-Host "  files: $((Get-ChildItem $out -Recurse -File | Measure-Object).Count)"
if ($outstanding.Count -gt 0) {
    Write-Host "  $($outstanding.Count) human validation(s) outstanding" -ForegroundColor Yellow
}
if ($fail -gt 0) {
    Write-Host "`n$fail problem(s) -- this bundle does NOT certify Phase 1. See $out\MANIFEST.md" -ForegroundColor Red
    exit 1
}
if ($outstanding.Count -gt 0) {
    Write-Host "`nAutomated evidence complete; Phase 1 awaits human validation." -ForegroundColor Yellow
    exit 2
}
Write-Host "`nPhase 1 evidence complete." -ForegroundColor Green
exit 0
