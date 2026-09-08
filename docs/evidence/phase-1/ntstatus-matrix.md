# NTSTATUS translation matrix (Phase 1 section 12.4)

Four columns: **injected condition -> SPACE error code -> NTSTATUS -> the
Win32 error Windows reports.** The first three are asserted in-process by
`client/core/src/ffi/ntstatus.rs`; only the fourth can be observed from
outside, because that translation is the kernel's.

The **expected** Win32 column is not hand-written: it is computed from the
NTSTATUS by `ntdll!RtlNtStatusToDosError`, so the row asserts what Windows
says the mapping is rather than what the script's author remembered.

- Generated: 2026-09-08T14:46:12.4111014+02:00
- Mount: S:

| injected condition | SPACE code | NTSTATUS | Win32 expected | Win32 observed | HRESULT | result |
|---|---|---|---|---|---|---|
| open a nonexistent file | `FileNotFound` | `STATUS_OBJECT_NAME_NOT_FOUND` | `ERROR_FILE_NOT_FOUND` | `ERROR_FILE_NOT_FOUND` | 0x80070002 | match |
| open under a missing directory | `ObjectPathNotFound` | `STATUS_OBJECT_PATH_NOT_FOUND` | `ERROR_PATH_NOT_FOUND` | `ERROR_PATH_NOT_FOUND` | 0x80070003 | match |
| create an existing file exclusively | `FileExists` | `STATUS_OBJECT_NAME_COLLISION` | `ERROR_ALREADY_EXISTS` | `ERROR_FILE_EXISTS` | 0x80070050 | match (documented alternate) |
| remove a non-empty directory | `DirectoryNotEmpty` | `STATUS_DIRECTORY_NOT_EMPTY` | `ERROR_DIR_NOT_EMPTY` | `ERROR_DIR_NOT_EMPTY` | 0x80070091 | match |
| open a directory as a file | `FileIsADirectory` | `STATUS_FILE_IS_A_DIRECTORY` | `ERROR_ACCESS_DENIED` | `ERROR_ACCESS_DENIED` | 0x80070005 | match |
| FILE_DIRECTORY_FILE against a file (RemoveDirectory) | `NotADirectory` | `STATUS_NOT_A_DIRECTORY` | `ERROR_DIRECTORY` | `ERROR_DIRECTORY` | 0x8007010B | match |
| a reserved device name | `ObjectNameInvalid` | `STATUS_OBJECT_NAME_INVALID` | `ERROR_INVALID_NAME` | `ERROR_INVALID_NAME` | 0x8007007B | match |
| a component past L2 | `NameTooLong` | `STATUS_NAME_TOO_LONG` | `ERROR_FILENAME_EXCED_RANGE` | `ERROR_FILENAME_EXCED_RANGE` | 0x800700CE | match |

**Provenance note for the `NotADirectory` row.** For that condition the
status is produced by the WinFsp FSD, not by SPACE: the FSD sees
`FILE_DIRECTORY_FILE` against the non-directory attributes returned by
`GetSecurityByName` and fails the request without calling our `Open`.
Verified by log inspection -- `NotADirectory` never appears in the client
log for this case. SPACE's own mapping for it is real (`memvfs::open` and
`dir_open`) and is asserted in-process by the conformance suite; it is
simply not the layer that answers a Windows client here. The Win32 column
is still what section 12.4 asks for: what Windows reports for the condition.

`UNEXPECTED SUCCESS` means the condition did not fail at all -- a defect in
the row or in the filesystem, not a translation problem. `MISMATCH` means
Windows reported something other than what our NTSTATUS translates to, which
is a translation defect. `NOT REACHED` means .NET rejected the path before
any syscall, so the row proves nothing and must not be counted as evidence.
