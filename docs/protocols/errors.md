# SPACE error protocol (M0.4)

One error type crosses every boundary: `contracts::SpaceError`
(`{ code, message, operation_id?, request_id?, source? }`). It serialises
identically in-process, over HTTP, and in logs. Every API error response is
`{ "contract_version": 1, "error": <SpaceError> }`.

## Two rules

1. **No code path blocks indefinitely.** Every wait resolves to
   `NETWORK_TIMEOUT`, `CANCELLED`, or success. Network and backend failures
   become controlled filesystem errors or bounded retries -- never an infinite
   wait (guard rail #8).
2. **Retryability is a property of the code**, read from the table below by the
   transfer engine. It is never a per-call-site judgement. `ErrorCode::retryable()`
   is the single source of truth.

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
| `VERSION_NOT_FOUND` | either | no | `0xC0000034` | no such version, or it is still a Candidate |
| `MANIFEST_NOT_FOUND` | either | no | `0xC0000034` | no such manifest |
| `CHUNK_NOT_FOUND` | either | no | `0xC0000034` | object store has no such chunk |
| `INVALID_PARAMETER` | either | no | `0xC000000D` STATUS_INVALID_PARAMETER | bad offset/length/id/field |
| `INVALID_HANDLE` | client | no | `0xC0000008` STATUS_INVALID_HANDLE | stale or unknown handle |
| `INTEGRITY_HASH_MISMATCH` | either | no | `0xC0000102` STATUS_FILE_CORRUPT_ERROR | bytes do not match their `ChunkId` |
| `INTEGRITY_LENGTH_MISMATCH` | either | no | `0xC0000102` | object shorter than the requested/declared range |
| `INTEGRITY_MANIFEST_INVALID` | either | no | `0xC0000102` | a manifest invariant (1-13) is broken |
| `INTEGRITY_CHUNK_ID_CONFLICT` | either | no | `0xC0000102` | id already stores different bytes, or does not address the bytes |
| `NETWORK_TIMEOUT` | client | **yes** | `0xC00000B5` STATUS_IO_TIMEOUT | bounded wait elapsed |
| `NETWORK_UNAVAILABLE` | client | **yes** | `0xC00000C4` STATUS_UNEXPECTED_NETWORK_ERROR | connect/transport failure, backend down |
| `PROTOCOL_VIOLATION` | either | no | `0xC0000185` STATUS_IO_DEVICE_ERROR | malformed response, wrong content-length |
| `STORAGE_ERROR` | either | no | `0xC0000185` | local storage I/O failure |
| `DISK_FULL` | either | no | `0xC000007F` STATUS_DISK_FULL | out of local space |
| `RESOURCE_EXHAUSTED` | either | **yes** | `0xC000009A` STATUS_INSUFFICIENT_RESOURCES | queue/memory/connection bound hit |
| `CANCELLED` | client | no | `0xC0000120` STATUS_CANCELLED | operation cancelled or shutdown |
| `SHARING_VIOLATION` | client | no | `0xC0000043` STATUS_SHARING_VIOLATION | conflicting open mode |
| `AUTH_FAILED` | server | no | `0xC0000022` STATUS_ACCESS_DENIED | credentials rejected |
| `PERMISSION_DENIED` | server | no | `0xC0000022` | authenticated but not authorised |
| `INTERNAL_ERROR` | either | no | `0xC00000E5` STATUS_INTERNAL_ERROR | unexpected bug; should never be the documented behaviour of any path |

`ErrorCode::ALL` lists all 26 variants. The
`every_error_code_is_fully_classified` and `all_slice_covers_every_variant`
tests fail the build if a variant is added without a retryability, an origin,
and (unless startup-only) an NTSTATUS.
