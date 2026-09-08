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
#
# WHAT THIS SCRIPT ASSERTS, and why it is written this way:
#
# An earlier version recorded all four columns but only ever checked that *an
# error occurred*. Columns 2 and 3 were hand-typed labels the caller passed in,
# and column 4 was never compared against them -- so a row that returned
# ERROR_ACCESS_DENIED where STATUS_OBJECT_NAME_NOT_FOUND was expected was
# written into the evidence table as a verified translation. A matrix nobody
# checks is a table of assertions, not evidence.
#
# The expected Win32 value is therefore NOT typed in here either. It is computed
# from the expected NTSTATUS by asking the OS, through ntdll!RtlNtStatusToDosError
# -- the same function the kernel path uses. That keeps the author's memory of
# the mapping out of the assertion entirely: the row states the NTSTATUS our code
# claims to return, Windows says what that becomes, and the observed value must
# match.

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

Add-Type -Namespace SpaceNt -Name Rtl -MemberDefinition @'
[DllImport("ntdll.dll")] public static extern uint RtlNtStatusToDosError(uint Status);
'@

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
function Win32Name($c) { if ($win32.ContainsKey([int]$c)) { $win32[[int]$c] } else { "win32 $c" } }

# NTSTATUS values, mirroring client/core/src/ffi/ntstatus.rs. Only the constants
# the rows below reference.
#
# The `L` suffix is load-bearing. PowerShell parses a bare 0xC0000033 as a
# SIGNED Int32, which is negative, so [uint32] on it throws -- and the throw
# left every expected value at 0, which made every row MISMATCH against a
# perfectly correct observation. Int64 literals cast to uint32 cleanly.
$NT = @{
    STATUS_OBJECT_NAME_INVALID   = 0xC0000033L
    STATUS_OBJECT_NAME_NOT_FOUND = 0xC0000034L
    STATUS_OBJECT_NAME_COLLISION = 0xC0000035L
    STATUS_OBJECT_PATH_NOT_FOUND = 0xC000003AL
    STATUS_FILE_IS_A_DIRECTORY   = 0xC00000BAL
    STATUS_DIRECTORY_NOT_EMPTY   = 0xC0000101L
    STATUS_NOT_A_DIRECTORY       = 0xC0000103L
    STATUS_NAME_TOO_LONG         = 0xC0000106L
}

# CreateFile special-cases one status: the kernel's generic translation of
# STATUS_OBJECT_NAME_COLLISION is ERROR_ALREADY_EXISTS, but a CREATE_NEW
# disposition reports ERROR_FILE_EXISTS for the same status. Both are correct
# for their call, so this row accepts either -- documented, not silent.
$alternates = @{ 183 = @(80) }

function Observe($label, $expectedCode, $ntName, [scriptblock]$body) {
    $expectedNt = [uint32]$NT[$ntName]
    $expectedWin32 = [SpaceNt.Rtl]::RtlNtStatusToDosError($expectedNt)

    try {
        & $body
        return [pscustomobject]@{
            Condition = $label; SpaceCode = $expectedCode; NtStatus = $ntName
            Expected = Win32Name $expectedWin32; Win32 = "(no error)"; Raw = ""
            Result = "UNEXPECTED SUCCESS"
        }
    } catch {
        $ex = $_.Exception
        $dotnet = $ex -is [ArgumentException]
        while ($ex.InnerException) { $ex = $ex.InnerException }
        $hr = $ex.HResult
    }

    # A Win32 error surfaces as HRESULT 0x8007xxxx; the low 16 bits are the code.
    $code = $hr -band 0xFFFF

    if ($dotnet) {
        # .NET rejected the path before any syscall, so the filesystem was never
        # asked and this row proves nothing about our translation. Say so rather
        # than banking it as a pass.
        $result = "NOT REACHED (rejected by .NET path validation)"
    } elseif ($code -eq $expectedWin32) {
        $result = "match"
    } elseif ($alternates.ContainsKey([int]$expectedWin32) -and
              $alternates[[int]$expectedWin32] -contains [int]$code) {
        $result = "match (documented alternate)"
    } else {
        $result = "MISMATCH"
    }

    [pscustomobject]@{
        Condition = $label; SpaceCode = $expectedCode; NtStatus = $ntName
        Expected = Win32Name $expectedWin32; Win32 = Win32Name $code
        Raw = ("0x{0:X8}" -f $hr); Result = $result
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
# RemoveDirectory, not Directory.GetFiles.
#
# GetFiles appends a search pattern, making the path "...\file.txt\*", which the
# Win32 path parser rejects with ERROR_INVALID_NAME before any I/O is issued --
# the filesystem is never asked, so the row proved nothing about our
# translation. Confirmed by log inspection: the last operation SPACE saw for
# that path was get_security_by_name returning ok, and NotADirectory never
# appeared in the log at all.
#
# RemoveDirectory does issue FILE_DIRECTORY_FILE against a file and yields
# ERROR_DIRECTORY. Note for the reader: for this condition the status
# originates in the WinFsp FSD, which sees FILE_DIRECTORY_FILE against the
# non-directory attributes returned by GetSecurityByName and fails the request
# without calling our Open at all. Our own NotADirectory mapping for this case
# is real (memvfs open() and dir_open()) and is asserted in-process by the
# conformance suite; it is simply not the layer that answers here.
$rows += Observe "FILE_DIRECTORY_FILE against a file (RemoveDirectory)" "NotADirectory" "STATUS_NOT_A_DIRECTORY" {
    [IO.Directory]::Delete("$root\nts\file.txt")
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
$md += "The **expected** Win32 column is not hand-written: it is computed from the"
$md += "NTSTATUS by ``ntdll!RtlNtStatusToDosError``, so the row asserts what Windows"
$md += "says the mapping is rather than what the script's author remembered."
$md += ""
$md += "- Generated: $(Get-Date -Format o)"
$md += "- Mount: $root"
$md += ""
$md += "| injected condition | SPACE code | NTSTATUS | Win32 expected | Win32 observed | HRESULT | result |"
$md += "|---|---|---|---|---|---|---|"
foreach ($r in $rows) {
    $md += "| $($r.Condition) | ``$($r.SpaceCode)`` | ``$($r.NtStatus)`` | ``$($r.Expected)`` | ``$($r.Win32)`` | $($r.Raw) | $($r.Result) |"
}
$md += ""
$md += "**Provenance note for the ``NotADirectory`` row.** For that condition the"
$md += "status is produced by the WinFsp FSD, not by SPACE: the FSD sees"
$md += "``FILE_DIRECTORY_FILE`` against the non-directory attributes returned by"
$md += "``GetSecurityByName`` and fails the request without calling our ``Open``."
$md += "Verified by log inspection -- ``NotADirectory`` never appears in the client"
$md += "log for this case. SPACE's own mapping for it is real (``memvfs::open`` and"
$md += "``dir_open``) and is asserted in-process by the conformance suite; it is"
$md += "simply not the layer that answers a Windows client here. The Win32 column"
$md += "is still what section 12.4 asks for: what Windows reports for the condition."
$md += ""
$md += "``UNEXPECTED SUCCESS`` means the condition did not fail at all -- a defect in"
$md += "the row or in the filesystem, not a translation problem. ``MISMATCH`` means"
$md += "Windows reported something other than what our NTSTATUS translates to, which"
$md += "is a translation defect. ``NOT REACHED`` means .NET rejected the path before"
$md += "any syscall, so the row proves nothing and must not be counted as evidence."

Remove-Item "$root\nts" -Recurse -Force -ErrorAction SilentlyContinue

New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$md -join "`r`n" | Out-File -Encoding utf8 $Out

$rows | Format-Table Condition, NtStatus, Expected, Win32, Result -AutoSize
Write-Host "Written to $Out" -ForegroundColor Green

$bad = @($rows | Where-Object { $_.Result -eq "MISMATCH" -or $_.Result -eq "UNEXPECTED SUCCESS" })
$unproven = @($rows | Where-Object { $_.Result -like "NOT REACHED*" })
if ($unproven.Count -gt 0) {
    Write-Host "$($unproven.Count) row(s) never reached the filesystem -- not evidence" -ForegroundColor Yellow
}
if ($bad.Count -gt 0) {
    Write-Host "$($bad.Count) row(s) failed" -ForegroundColor Red
    $bad | ForEach-Object { Write-Host "    $($_.Condition): expected $($_.Expected), got $($_.Win32)" -ForegroundColor Red }
    exit 1
}
exit 0
