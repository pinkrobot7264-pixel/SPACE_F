# SPACE resource limits (Phase 1 §3.4)

Part of the versioned contract (ADR-0011). **This document is the single source
of truth for limit values.** Tests read them from the `Limits` struct; no test
restates a number.

| # | Limit | Value | Config key | Enforced at | Error | State on failure | Atomic |
|---|---|---|---|---|---|---|---|
| L1 | path length | 32,767 chars | `vfs.max_path_chars` | `VfsPath::parse` | `NameTooLong` | unchanged | n/a |
| L2 | component length | 255 chars | `vfs.max_component_chars` | `VfsPath::parse` | `NameTooLong` | unchanged | n/a |
| L3 | path depth | 512 components | `vfs.max_path_depth` | `VfsPath::parse` | `NameTooLong` | unchanged | n/a |
| L4 | single read/write | 16 MiB | `vfs.max_io_bytes` | FFI boundary, **before allocation** | `InvalidParameter` | unchanged | n/a |
| L5 | total bytes | `vfs.max_bytes` | `vfs.max_bytes` | write / set_file_size | `DiskFull` | **unchanged** | yes |
| L6 | entries per directory | 65,536 | `vfs.max_dir_entries` | create / rename-in | `ResourceExhausted` | **unchanged** | yes |
| L7 | open handles | 65,536 | `vfs.max_open_handles` | handle manager `alloc` | `ResourceExhausted` | **unchanged** | yes |
| L8 | open cursors | 256 | `vfs.max_open_cursors` | cursor table `alloc` | `ResourceExhausted` | **unchanged** | yes |
| L9 | callback duration | `client.callback_timeout_ms` | `client.callback_timeout_ms` | `OpCtx`, **incl. lock wait** | `OperationTimeout` | see note | no |
| L10 | concurrent callbacks | `client.dispatcher_threads` | `client.dispatcher_threads` | WinFsp | queued by WinFsp | n/a | n/a |

## Notes

**L4 is enforced before allocation.** A length field arriving from the FFI is
validated against `max_io_bytes` *before* any buffer is sized from it. Otherwise
a hostile or corrupt length is an unbounded allocation, which is a denial of
service that no later check can undo.

**L6 is 65,536, not one million.** Phase 1 tests boundary correctness, not
scale; a million-entry directory turns `check_invariants()` into a multi-minute
operation and buys nothing. The 5,000-entry Explorer test (§9.2) remains, well
inside the bound.

**L8 is 256** because a cursor lives for one call (§3.3.8) and concurrency is
bounded by dispatcher threads (ADR-0010). The limit exists to **catch a leak**,
not to serve a workload.

**L9 state on failure:** a timeout during lock acquisition leaves state
untouched. A timeout detected mid-operation may leave a *completed prefix*; the
operation reports what it transferred. This is stated rather than promised as
atomic because Phase 4 will make it real.

## Testing obligation

Every limit gets a config key, a **limit** test, and a **limit + 1** test
(§14.3).

For every limit+1 case, assert in this order:

1. the specified error code (INV-RES-2)
2. **`state_before == state_after`** — file content, tree shape, handle count,
   cursor count and byte usage compared field by field (INV-RES-3)
3. `check_invariants()` passes (INV-RES-1)

**Assertion 2 matters more than assertion 1. A limit that fails *and* corrupts
is worse than no limit.**

Phase 1 does not benchmark limits. L6 is 65,536 precisely so that boundary
correctness can be tested in seconds; exhausting enormous limits is a Phase 11
activity.

## Configuration

Every key above lives under `[vfs]` (or `[client]` for L9/L10) in
`config.toml`. All `[vfs]` keys carry defaults equal to the values in this
table, so an existing config file remains valid; setting one explicitly is how a
test drives a boundary without recompiling.

Config values are themselves range-checked at load, so a config cannot set a
limit to a value that would break an invariant (for example `max_io_bytes = 0`).
