# SPACE error protocol (M0.4, extended by Phase 1 §1.3)

One error type crosses every boundary: `contracts::SpaceError`
(`{ code, message, operation_id?, request_id?, source? }`). It serialises
identically in-process, over HTTP, and in logs. Every API error response is
`{ "contract_version": 1, "error": <SpaceError> }`.

## Three rules

1. **No code path blocks indefinitely.** Every wait resolves to a timeout code,
   `CANCELLED`, or success. Network and backend failures become controlled
   filesystem errors or bounded retries -- never an infinite wait
   (guard rail #8).
2. **Retryability is a property of the code**, read from the table below by the
   transfer engine. It is never a per-call-site judgement.
   `ErrorCode::retryable()` is the single source of truth.
3. **Local timeouts and remote timeouts are different codes** (ADR-0014).
   `OPERATION_TIMEOUT` means *a filesystem operation exceeded its configured
   execution deadline*; it is the only timeout Phase 1 can produce.
   `NETWORK_TIMEOUT` means *a remote request exceeded its deadline* and is
   reserved for the Phase 4+ transfer engine. **The VFS layer may never produce
   `NETWORK_TIMEOUT`** -- a network-backed implementation handles its own
   network timeouts internally and surfaces `OPERATION_TIMEOUT` at the VFS
   boundary. The Windows-facing layer does not care why something was slow.

## Where the NTSTATUS mapping lives (ADR-0015)

The `NTSTATUS` column below is **documentation of the mapping, not of this
crate's contents.** `contracts` holds the error *taxonomy* only -- the codes,
`retryable()`, `origin()` and `is_startup_only()` -- and mentions no Windows
type, because `contracts` is shared with the cloud service.

The mapping table and its exhaustiveness test live at the Windows boundary, in
`client/core/src/ffi/ntstatus.rs`. The VFS never sees an NTSTATUS or a Win32
error code; it returns `SpaceError` and nothing else.

**The mapping is total, not injective.** Several codes legitimately share one
NTSTATUS: `OPERATION_TIMEOUT` and `NETWORK_TIMEOUT` both map to
`STATUS_IO_TIMEOUT`, and the four integrity codes all map to
`STATUS_FILE_CORRUPT_ERROR`. The test asserts every code *has* a mapping, never
that mappings are unique.

## Registry

`origin`: which side can raise it. `retry`: transfer engine may retry.
`NTSTATUS`: value the WinFsp adapter returns to Windows (Phase 1+).
Startup-only codes have no NTSTATUS -- they happen before the mount exists.

| Code | origin | retry | NTSTATUS | Meaning |
|---|---|---|---|---|
| `CONFIG_MISSING` | client | no | -- | config file not found |
| `CONFIG_INVALID` | client | no | -- | config unparseable or a bound violated |
| `CONFIG_UNSUPPORTED_VERSION` | client | no | -- | `config_version` != 1 |
| `FILE_NOT_FOUND` | either | no | `0xC0000034` STATUS_OBJECT_NAME_NOT_FOUND | no such file |
| `FILE_EXISTS` | either | no | `0xC0000035` STATUS_OBJECT_NAME_COLLISION | name already taken |
| `DIRECTORY_NOT_EMPTY` | either | no | `0xC0000101` STATUS_DIRECTORY_NOT_EMPTY | rmdir on non-empty dir |
| `OBJECT_NAME_INVALID` | client | no | `0xC0000033` STATUS_OBJECT_NAME_INVALID | name breaks a §3.3.7 naming rule, or invalid UTF-16 |
| `OBJECT_PATH_NOT_FOUND` | either | no | `0xC000003A` STATUS_OBJECT_PATH_NOT_FOUND | a *parent* directory on the path does not exist |
| `NOT_A_DIRECTORY` | either | no | `0xC0000103` STATUS_NOT_A_DIRECTORY | directory operation on a file |
| `FILE_IS_A_DIRECTORY` | either | no | `0xC00000BA` STATUS_FILE_IS_A_DIRECTORY | file operation on a directory |
| `END_OF_FILE` | either | no | `0xC0000011` STATUS_END_OF_FILE | read at or past EOF -- **not** success-with-zero |
| `NAME_TOO_LONG` | client | no | `0xC0000106` STATUS_NAME_TOO_LONG | L1/L2/L3 exceeded, or an unterminated wide string |
| `CANNOT_DELETE` | either | no | `0xC0000121` STATUS_CANNOT_DELETE | delete refused by filesystem state |
| `BUFFER_OVERFLOW` | client | no | `0x80000005` STATUS_BUFFER_OVERFLOW | **warning, not an error**: caller's buffer too small; required size written |
| `OPERATION_TIMEOUT` | client | **yes** | `0xC00000B5` STATUS_IO_TIMEOUT | a filesystem operation exceeded its execution deadline (L9) |
| `VERSION_NOT_FOUND` | either | no | `0xC0000034` | no such version, or it is still a Candidate |
| `MANIFEST_NOT_FOUND` | either | no | `0xC0000034` | no such manifest |
| `CHUNK_NOT_FOUND` | either | no | `0xC0000034` | object store has no such chunk |
| `INVALID_PARAMETER` | either | no | `0xC000000D` STATUS_INVALID_PARAMETER | bad offset/length/id/field, including overflow |
| `INVALID_HANDLE` | client | no | `0xC0000008` STATUS_INVALID_HANDLE | stale or unknown handle |
| `INTEGRITY_HASH_MISMATCH` | either | no | `0xC0000102` STATUS_FILE_CORRUPT_ERROR | bytes do not match their `ChunkId` |
| `INTEGRITY_LENGTH_MISMATCH` | either | no | `0xC0000102` | object shorter than the requested/declared range |
| `INTEGRITY_MANIFEST_INVALID` | either | no | `0xC0000102` | a manifest invariant (1-13) is broken |
| `INTEGRITY_CHUNK_ID_CONFLICT` | either | no | `0xC0000102` | id already stores different bytes, or does not address the bytes |
| `NETWORK_TIMEOUT` | client | **yes** | `0xC00000B5` STATUS_IO_TIMEOUT | bounded wait elapsed -- **transfer engine only** (Phase 4+) |
| `NETWORK_UNAVAILABLE` | client | **yes** | `0xC00000C4` STATUS_UNEXPECTED_NETWORK_ERROR | connect/transport failure, backend down |
| `PROTOCOL_VIOLATION` | either | no | `0xC0000185` STATUS_IO_DEVICE_ERROR | malformed response, wrong content-length |
| `STORAGE_ERROR` | either | no | `0xC0000185` | local storage I/O failure |
| `DISK_FULL` | either | no | `0xC000007F` STATUS_DISK_FULL | out of local space (L5) |
| `RESOURCE_EXHAUSTED` | either | **yes** | `0xC000009A` STATUS_INSUFFICIENT_RESOURCES | queue/memory/connection bound hit (L6, L7, L8) |
| `CANCELLED` | client | no | `0xC0000120` STATUS_CANCELLED | operation cancelled or shutdown |
| `SHARING_VIOLATION` | client | no | `0xC0000043` STATUS_SHARING_VIOLATION | conflicting open mode -- raised by WinFsp's FSD, not by SPACE (ADR-0012) |
| `AUTH_FAILED` | server | no | `0xC0000022` STATUS_ACCESS_DENIED | credentials rejected |
| `PERMISSION_DENIED` | server | no | `0xC0000022` | authenticated but not authorised |
| `INTERNAL_ERROR` | either | no | `0xC00000E5` STATUS_INTERNAL_ERROR | unexpected bug, or a contained panic (ADR-0013); never the documented behaviour of any path |

`ErrorCode::ALL` lists all **35** variants -- 26 from Phase 0 plus the 9
filesystem codes added by Phase 1 §1.3. Three tests fail the build if a variant
is added without a full classification:

- `all_slice_covers_every_variant` (contracts) -- the variant is in `ALL`;
- `every_error_code_is_fully_classified` (contracts) -- it has a retryability
  and an origin;
- the NTSTATUS exhaustiveness test (`client/core/src/ffi/ntstatus.rs`) -- it has
  a mapping unless it is startup-only.

Exhaustive `match` over `ErrorCode` elsewhere in the workspace (for example
`cloud/api`'s HTTP status map) is the fourth guard: adding a code breaks
compilation until every consumer classifies it deliberately.

## On granularity -- deliberately not split

`INVALID_PARAMETER` covers bad offsets, bad lengths, and arithmetic overflow.
Splitting it into `INVALID_OFFSET` and `INVALID_LENGTH` would add codes that map
to the same NTSTATUS and change no behaviour; the specifics belong in the log
line, which carries `offset` and `length` on every operation (§12.1).
