# ADR-0014 — Error taxonomy: local now, remote later

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §1.3, §11.7, §12.4

## Context

A filesystem that will later be network-backed has an obvious trap: the day a
remote call times out, the temptation is to surface "network timeout" to
Windows. Windows does not care why something was slow, and a Windows-facing
layer that knows about networks is a layer that will grow network-specific
special cases.

## Decision — two classes, one boundary between them

| Class | Codes | Produced by |
|---|---|---|
| **Filesystem / VFS** (Phase 1) | `FileNotFound`, `ObjectPathNotFound`, `FileExists`, `NotADirectory`, `FileIsADirectory`, `DirectoryNotEmpty`, `CannotDelete`, `InvalidParameter`, `InvalidHandle`, `ObjectNameInvalid`, `NameTooLong`, `EndOfFile`, `SharingViolation`, `PermissionDenied`, `DiskFull`, `ResourceExhausted`, `OperationTimeout`, `Cancelled`, `BufferOverflow`, `InternalError` | FFI boundary and VFS |
| **Remote / network** (Phase 4+, reserved) | `NetworkTimeout`, `NetworkUnavailable`, `ProtocolViolation`, `AuthFailed` | transfer engine only |

## Rules

- `OperationTimeout` means *a filesystem operation exceeded its configured
  execution deadline.* It is the only timeout Phase 1 can produce.
- `NetworkTimeout` means *a remote request exceeded its deadline.* **The VFS
  layer may never produce it.** A future network-backed implementation handles
  its own network timeouts internally and surfaces `OperationTimeout` at the VFS
  boundary.
- Enforced by a conformance assertion (§11.7) and a grep test over the crate.

## On granularity — deliberately not added

`InvalidParameter` covers bad offsets, bad lengths, and overflow. Splitting it
into `InvalidOffset` and `InvalidLength` would add codes that map to the same
NTSTATUS and change no behaviour; the specifics belong in the log line, which
carries offset and length on every operation (§12.1).

## On `BufferOverflow`

`BufferOverflow` is a *warning* status (`0x8000_0005`), not an error —
`GetSecurityByName` returns it to report an undersized buffer. `SpaceError` must
carry it without treating it as a failure, or it falls into `InternalError`
(§6.2).

## On placement

The NTSTATUS mapping does **not** live in `contracts`. See ADR-0015.

## Enforcement

`contracts::ErrorCode`; the §11.7 conformance assertions; the §12.4 four-column
translation matrix.
