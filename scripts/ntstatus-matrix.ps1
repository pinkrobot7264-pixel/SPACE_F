# SPACE Phase 1 -- the four-column translation matrix (manual section 12.4).
#
#   injected condition -> SPACE error code -> NTSTATUS -> the Win32 error
#                                                          Windows reports
#
# The first three columns are asserted in-process by
# client/core/src/ffi/ntstatus.rs. Only the FOURTH can be observed here,
# because it is what Windows hands the application after translating our
# NTSTATUS -- and that translation is the kernel's, not ours.
#
# Table-driven, one row per callback-reachable code.

param(
    [string]$Drive = "S",
    [string]$Out = "docs\evidence\phase-1\ntstatus-matrix.md"
)

$ErrorActionPreference = "Continue"
$root = "${Drive}:"

if (-not (Test-Path "$root\")) {
    Write-Host "FAIL  $root is not mounted; start the client first" -ForegroundColor Red
    exit 1
}

# Win32 error codes, for naming what we observe.
$win32 = @{
    2   = "ERROR_FILE_NOT_FOUND"
    3   = "ERROR_PATH_NOT_FOUND"
    5   = "ERROR_ACCESS_DENIED"
    6   = "ERROR_INVALID_HANDLE"
    32  = "ERROR_SHARING_VIOLATION"
    38  = "ERROR_HANDLE_EOF"
    80  = "ERROR_FILE_EXISTS"
    87  = "ERROR_INVALID_PARAMETER"
    112 = "ERROR_DISK_FULL"
    123 = "ERROR_INVALID_NAME"
    145 = "ERROR_DIR_NOT_EMPTY"
    183 = "ERROR_ALREADY_EXISTS"
    206 = "ERROR_FILENAME_EXCED_RANGE"
    267 = "ERROR_DIRECTORY"
    1392 = "ERROR_FILE_CORRUPT"
}

function Observe($label, $expectedCode, $expectedNtstatus, [scriptblock]$body) {
    $hr = $null
    $msg = ""
    try {
        & $body
        return [pscustomobject]@{
            Condition = $label; SpaceCode = $expectedCode; NtStatus = $expectedNtstatus
            Win32 = "(no error)"; Raw = ""; Result = "UNEXPECTED SUCCESS"
        }
    } catch {
        $ex = $_.Exception
        while ($ex.InnerException) { $ex = $ex.InnerException }
        $hr = $ex.HResult
        $msg = $ex.Message
    }

    # A Win32 error surfaces as HRESULT 0x8007xxxx; the low 16 bits are the code.
    $code = $hr -band 0xFFFF
    $name = if ($win32.ContainsKey($code)) { $win32[$code] } else { "win32 $code" }
    [pscustomobject]@{
        Condition = $label; SpaceCode = $expectedCode; NtStatus = $expectedNtstatus
        Win32 = $name; Raw = ("0x{0:X8}" -f $hr); Result = "observed"
    }
}

# Fixture.
New-Item -ItemType Directory -Path "$root\nts" -Force | Out-Null
[IO.File]::WriteAllText("$root\nts\file.txt", "abc")
New-Item -ItemType Directory -Path "$root\nts\full" -Force | Out-Null
[IO.File]::WriteAllText("$root\nts\full\child.txt", "x")

$rows = @()

$rows += Observe "open a nonexistent file" "FileNotFound" "STATUS_OBJECT_NAME_NOT_FOUND" {
    [IO.File]::ReadAllText("$root\nts\missing.txt")
}
$rows += Observe "open under a missing directory" "ObjectPathNotFound" "STATUS_OBJECT_PATH_NOT_FOUND" {
    [IO.File]::ReadAllText("$root\nts\no-such-dir\f.txt")
}
$rows += Observe "create an existing file exclusively" "FileExists" "STATUS_OBJECT_NAME_COLLISION" {
    $fs = [IO.File]::Open("$root\nts\file.txt", [IO.FileMode]::CreateNew)
    $fs.Dispose()
}
$rows += Observe "remove a non-empty directory" "DirectoryNotEmpty" "STATUS_DIRECTORY_NOT_EMPTY" {
    [IO.Directory]::Delete("$root\nts\full")
}
$rows += Observe "open a directory as a file" "FileIsADirectory" "STATUS_FILE_IS_A_DIRECTORY" {
    $fs = [IO.File]::Open("$root\nts\full", [IO.FileMode]::Open, [IO.FileAccess]::Read)
    $fs.Dispose()
}
$rows += Observe "enumerate a file as a directory" "NotADirectory" "STATUS_NOT_A_DIRECTORY" {
    [IO.Directory]::GetFiles("$root\nts\file.txt")
}
$rows += Observe "a reserved device name" "ObjectNameInvalid" "STATUS_OBJECT_NAME_INVALID" {
    [IO.File]::WriteAllText("$root\nts\CON", "x")
}
$rows += Observe "a component past L2" "NameTooLong" "STATUS_NAME_TOO_LONG" {
    [IO.File]::WriteAllText("$root\nts\$('a' * 300).txt", "x")
}

# Report.
$md = @()
$md += "# NTSTATUS translation matrix (Phase 1 section 12.4)"
$md += ""
$md += "Four columns: **injected condition -> SPACE error code -> NTSTATUS -> the"
$md += "Win32 error Windows reports.** The first three are asserted in-process by"
$md += "``client/core/src/ffi/ntstatus.rs``; only the fourth can be observed from"
$md += "outside, because that translation is the kernel's."
$md += ""
$md += "- Generated: $(Get-Date -Format o)"
$md += "- Mount: $root"
$md += ""
$md += "| injected condition | SPACE code | NTSTATUS | Win32 observed | HRESULT |"
$md += "|---|---|---|---|---|"
foreach ($r in $rows) {
    $md += "| $($r.Condition) | ``$($r.SpaceCode)`` | ``$($r.NtStatus)`` | ``$($r.Win32)`` | $($r.Raw) |"
}
$md += ""
$md += "Rows marked ``UNEXPECTED SUCCESS`` mean the condition did not fail at all,"
$md += "which is a defect in the row or in the filesystem -- not a translation"
$md += "problem."

Remove-Item "$root\nts" -Recurse -Force -ErrorAction SilentlyContinue

New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$md -join "`r`n" | Out-File -Encoding utf8 $Out

$rows | Format-Table -AutoSize
$bad = ($rows | Where-Object { $_.Result -ne "observed" }).Count
Write-Host "Written to $Out" -ForegroundColor Green
if ($bad -gt 0) { Write-Host "$bad row(s) did not produce an error" -ForegroundColor Red; exit 1 }
exit 0
