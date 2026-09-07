# SPACE identity model (Phase 1 §3.1)

Part of the versioned contract (ADR-0011). Changing anything here requires a
`VFS_CONTRACT_VERSION` bump and an ADR.

**Five identifiers. They do not collapse into each other.**

| Identifier | Meaning | Scope & lifetime | Crosses FFI | Generational | Visible to Windows |
|---|---|---|---|---|---|
| `NodeId` | SPACE's internal object identity -- a file or directory | process lifetime; stable across rename, move, and unlink | **no** | yes (slab index + generation) | no |
| `HandleId` | an opened filesystem handle; one per successful `create`/`open` | from `create`/`open` until `close` | yes, as WinFsp's `FileContext` | yes | no |
| `CursorId` | directory enumeration state | **a single `ReadDirectory` call** (§3.3.8) | yes | yes | no |
| `RequestId` | one boundary request, for tracing | one callback | no (minted in Rust) | n/a (UUIDv7) | no |
| `index_number` | Windows-visible file identity, in `FSP_FSCTL_FILE_INFO` | life of the node; stable across rename | yes, inside `space_file_info` | no (monotonic counter) | **yes** |

## Rules

### `index_number` is not derived from `NodeId`

A slab index is reused after free. Windows treats `index_number` as a file
identity key, and reuse causes Explorer to conflate distinct files — two
unrelated files that report the same index number are, as far as Windows is
concerned, the same file with two names.

`index_number` is allocated from a **monotonic `u64` counter that is never
reused within a process lifetime** (INV-ID-2).

The failure this prevents is subtle and slow to diagnose: create a file, delete
it, create another, and watch Explorer show the wrong properties for one of
them. See the troubleshooting table, "Explorer conflates distinct files".

### `RequestId` is minted at the first Rust frame

`RequestId` is created in `guard()` (§2.3), not passed in from C++.

The C++ adapter performs no logging and makes no decisions, so there is nothing
above that point to correlate. This is honest about the trace's true origin. **Do
not add a request-id parameter to the FFI for a layer that never uses it.**

Trace path: `WinFsp callback → C++ adapter (silent) → FFI guard (request_id
minted) → VFS → result`.

### No identifier is or derives from a memory address

ADR-0008, INV-ID-5. This is the entire justification for the generational
scheme: a stale identifier must fail validation, not dereference freed memory.

### Encoding for `HandleId` and `CursorId`

```
(generation as u64) << 32 | (index as u64 + 1)
```

The `+ 1` guarantees **`0` is never valid**, which is what
`SPACE_INVALID_HANDLE` and `SPACE_INVALID_CURSOR` rely on. A zeroed struct, a
missed initialisation, or a hostile caller passing `0` all fail cleanly.

Resolution checks, in order:

1. index within range
2. slot live
3. generation matches

Any mismatch is `InvalidHandle` (handles) or `InvalidParameter` (cursors) —
never a panic, never a dereference.

### Generation wraparound

Generations are `u32` and advance on every free. At `u32::MAX` they wrap.
**Generation 0 is skipped on wrap**, so a wrapped slot cannot collide with the
initial state of a never-allocated slot. The case is contrived; it is
implemented and tested anyway, because discovering it later means discovering it
through a corruption report.

### Stale resolution is always an error, never an aliasing

After a slot is freed and reused, the old value fails the generation check
(INV-ID-3). It does **not** reach the new object. This is the strongest safety
property in Phase 1.

## Invariants

- **INV-ID-1** — every live node has a unique `NodeId`, stable across rename,
  move and unlink
- **INV-ID-2** — every live node has a unique `index_number`, never reused
  within a process lifetime
- **INV-ID-3** — a freed or stale `HandleId`/`CursorId` never resolves; after
  slot reuse the old value fails the generation check and does not reach the new
  object
- **INV-ID-4** — every live `HandleId` is unique and refers to exactly one live
  node
- **INV-ID-5** — no identifier's value derives from a memory address; `0` is
  never a valid `HandleId` or `CursorId`
