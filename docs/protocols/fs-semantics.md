# SPACE filesystem semantics (Phase 1 §3.3)

Part of the versioned contract (ADR-0011), `VFS_CONTRACT_VERSION = 1`.

**Phase 2 implements this document, not Phase 1's code.** Where the two
disagree, this document wins and the code is a bug.

Every rule below has a conformance test named for it
(`client/core/src/conformance/`). The suite runs against any `Vfs`
implementation with no WinFsp installed and no mount.

---

## 1. Handle lifecycle — corrected against the WinFsp contract

The commonly repeated summary *"Cleanup fires when the last handle to the file
closes"* is **wrong** and produces real bugs. The accurate contract:

- Each successful `Create`/`Open` corresponds to one kernel **file object**, and
  to exactly one `FileContext` — one `HandleId`.
- A file object's handle may be duplicated (`DuplicateHandle`); all duplicates
  share one file object.
- **`Cleanup` is called per file object**, when the last *handle to that file
  object* is released. Other file objects for the same file may still be open.
- **`Close` is called exactly once per `Create`/`Open`, always**, when the file
  object is destroyed. After `Close` the `FileContext` must not be used.
- `Cleanup` always precedes `Close` for a given file object when it is posted at
  all.
- **`Cleanup` posting is conditional on volume parameters.** With
  `PostCleanupWhenModifiedOnly = 1`, WinFsp suppresses `Cleanup` for file objects
  that neither modified the file nor have a delete pending.

### Bookkeeping placement

Two consequences that are easy to get wrong:

| Bookkeeping | Where it must happen | Why |
|---|---|---|
| decrement `open_count`, free the handle slot | **`Close`** | `Close` is the only callback guaranteed to fire for every open |
| unlink the node from the namespace | **`Cleanup`, when `FspCleanupDelete` is set** | this is the only signal Windows gives that the delete should take effect |
| reclaim node storage | **`Close`, when `open_count` reaches 0 and the node is unlinked** | deterministic, and independent of whether `Cleanup` was posted |

**Phase 1 sets `PostCleanupWhenModifiedOnly = 0`** so `Cleanup` is always posted
(§4.1). Determinism is worth more than the optimisation here. The bookkeeping
rules above hold either way, which is the point of splitting them.

### `CanDelete` versus `SetDelete`

WinFsp offers both; when `SetDelete` is provided it supersedes `CanDelete`.
Phase 1 implements **`CanDelete` only** — a pure query, simplest to reason
about — and does not provide `SetDelete`. Recorded so Phase 2 knows it is a
choice, not an oversight.

### Rules

| Rule | Behaviour | Invariant |
|---|---|---|
| `Close` count | exactly one per `create`/`open` | INV-FS-3 |
| I/O after `Close` | `InvalidHandle` | INV-FS-3 |
| I/O after `Cleanup`, before `Close` | `InvalidHandle` — Windows sends no further I/O on that file object, so any such call is a bug or an attack | INV-FS-3 |
| duplicate `close` on one `HandleId` | second is a silent no-op at the FFI (`void`); the handle manager reports `InvalidHandle` internally; no panic, no double free | INV-ID-3 |
| stale `HandleId` after slot reuse | `InvalidHandle` | INV-ID-3 |

---

## 2. Deleted-but-open lifecycle

```
namespace entry exists, open_count = N
        │
        ├─ CanDelete           → may I?  (DirectoryNotEmpty if a non-empty dir)
        │
        ├─ Cleanup(FspCleanupDelete)
        │     → node unlinked from parent; path lookup now fails
        │     → node marked unlinked; content and metadata intact
        │     → existing handles continue to work: read, write, file_info, set_file_size
        │     → rename of an unlinked node is rejected (InvalidParameter)
        │     → creating a new file at the old path succeeds and is a *different* node
        │
        ├─ Close ... (open_count decrements)
        │
        └─ Close (open_count reaches 0, node unlinked)
              → storage released, NodeId retired
```

Reclamation is **deterministic**: exactly at the `Close` that brings
`open_count` to zero on an unlinked node. Never earlier, never later, never on a
timer.

**Invariant carve-out:** INV-NS-2 (every non-root node has exactly one parent)
**does not apply to unlinked nodes.** The checker excludes them explicitly
(§5.3). This is a documented exception, not a silent one.

---

## 3. Open and create

| Situation | Result |
|---|---|
| open nonexistent | `FileNotFound` |
| open with missing parent directory | `ObjectPathNotFound` (distinct; Windows tools distinguish them) |
| **a component of the parent chain exists but is a file** | **`ObjectPathNotFound`** — see the note below |
| create existing | `FileExists` |
| open directory with `FILE_NON_DIRECTORY_FILE` | `FileIsADirectory` |
| open file with `FILE_DIRECTORY_FILE` | `NotADirectory` |
| `FILE_DELETE_ON_CLOSE` | WinFsp sets `FspCleanupDelete` at cleanup; the VFS unlinks then |
| same file opened twice | two `HandleId`s, one `NodeId`, `open_count == 2` |
| share access | WinFsp-owned (ADR-0012); the core records `granted_access` only |

### Note — a file in the middle of a path

The Phase 1 manual's §3.3.3 table does not name the case where a path traverses
*through* something that exists but is not a directory (`\a.txt\b`). It is
decided here rather than left to the implementation:

**Any failure to resolve the parent chain is `ObjectPathNotFound`.**
`NotADirectory` is reserved for `FILE_DIRECTORY_FILE` applied to the target
itself.

Two reasons:

- It matches what Windows reports for `\file.txt\child` —
  `ERROR_PATH_NOT_FOUND`, *"could not find a part of the path"* — so
  applications see the status they already handle.
- It is depth-independent. The alternative made `\a.txt\b` report
  `NotADirectory` while `\a.txt\b\c` reported `ObjectPathNotFound`, which is the
  same situation reported two ways. The conformance suite caught exactly that
  inconsistency on its first run.

---

## 4. Read and write

| Situation | Result |
|---|---|
| read at or past EOF | `EndOfFile`, zero transferred — **not** success-with-zero |
| read crossing EOF | success, short transfer — normal, not an error |
| read length 0 | success, zero transferred |
| `offset + length` overflows `u64` | `InvalidParameter`, no panic |
| length above `max_io_bytes` | `InvalidParameter`, **before any allocation** |
| write past EOF | file extends, gap zero-filled |
| `WriteToEndOfFile` | ignore offset, append at current EOF |
| `ConstrainedIo`, `offset >= file_size` | zero transferred, success, file unchanged |
| `ConstrainedIo` crossing EOF | write truncated to the existing size |
| write exceeding `max_bytes` | `DiskFull`, **file byte-identical to before** (INV-RES-3) |

`ConstrainedIo` comes from the cache manager's write-behind path. **Ignoring it
produces files that grow during a copy.**

`read length 0` returning success while `read at EOF` returns `EndOfFile` is not
an inconsistency: a zero-length read asks for nothing and gets nothing, whereas
a non-empty read starting at EOF cannot be satisfied at all. The zero-length
case is checked first.

---

## 5. Rename and delete

| Situation | Result | Invariant |
|---|---|---|
| rename within or across directories | one operation | |
| rename over existing, `replace = false` | `FileExists` | INV-NS-5 |
| rename over existing, `replace = true` | replace; a directory target may not be replaced by a file | |
| rename a directory | whole subtree moves | |
| rename a directory into its own subtree | `InvalidParameter` | INV-NS-4 |
| rename an unlinked node | `InvalidParameter` | INV-NS-2 |
| open handle survives rename | yes — handles reference `NodeId`, not path | INV-ID-1 |
| delete non-empty directory | `DirectoryNotEmpty` | |
| delete while open | §2 above | INV-FS-4 |

`can_delete` is a pure query: `DirectoryNotEmpty` for a non-empty directory,
success otherwise. Removal happens at `Cleanup` with `FspCleanupDelete`.

---

## 6. Timestamps

| Operation | created | accessed | written | changed |
|---|---|---|---|---|
| create | set | set | set | set |
| read | — | set | — | — |
| write | — | — | set | set |
| set_file_size | — | — | set | set |
| set_basic_info | explicit | explicit | explicit | set |
| rename | — | — | — | set |

`set_basic_info` treats `0` as **"do not change"** per field.

Times are Windows `FILETIME`: 100-nanosecond intervals since 1601-01-01 UTC.

---

## 7. Names and paths

`CaseSensitiveSearch = 0`, `CasePreservedNames = 1`. Names are **stored as
given**; lookup uses a **folded key**.

This is the only permitted capability (§11.5), and **both variants are
specified**, so neither is a gap:

| `unicode_case_folding` | Required behaviour |
|---|---|
| `false` (Phase 1) | `A`/`a` collide; `Å`/`å` do **not** collide; case preserved on both |
| `true` (Phase 2 target) | `A`/`a` collide; `Å`/`å` **do** collide, per Unicode simple case folding; case preserved |

### Rejected at `VfsPath` construction

- empty
- no leading `\`
- `.` or `..` components
- repeated separators
- `:` in a component
- reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
  `LPT1`–`LPT9`), with or without extension
- trailing space or dot in a component
- `<` `>` `"` `|` `?` `*`
- embedded NUL
- invalid UTF-16
- any bound in §3.4 exceeded (L1 path length, L2 component length, L3 depth)

**Path resolution never escapes the SPACE namespace** (INV-NS-6). This has two
halves deliberately: a logical assertion proving the resolver, and external
ProcMon evidence proving the process.

---

## 8. Directory enumeration — the semantic contract

This section is the **VFS contract**. The WinFsp protocol that carries it (§9.1,
buffer packing, the end-of-enumeration marker) lives in the adapter and does not
leak upward. The Rust core knows nothing about `FSP_FSCTL_DIR_INFO`.

| Rule | Definition |
|---|---|
| Order | `.`, then `..` (both omitted for the root), then children in ascending folded-name order. The order is total and stable for a given directory state. |
| Marker | An enumeration with `marker = Some(name)` yields entries **strictly after** `name` in that order. `marker = None` starts at the beginning. |
| Completeness | Over any sequence of calls whose markers chain correctly, each entry is yielded exactly once. |
| Termination | An enumeration of a directory with N children yields at most N + 2 entries. |
| Cursor lifetime | **A cursor is valid for one `ReadDirectory` call only.** It is opened, drained or abandoned, and closed within that call. Resumption across calls is carried by `marker`, never by a cursor. |
| Pattern | `pattern` is accepted and **ignored** in Phase 1; WinFsp performs pattern filtering. Recorded as a dependency, not a gap. |
| Mutation during enumeration | Cannot occur in Phase 1 — §3.6 serializes operations. Phase 2 must define it if that changes. |

Scoping the cursor to a single call eliminates a whole class of leak and
staleness bugs, and makes the concurrent-cursor count bounded by dispatcher
threads rather than by client behaviour (which is why L8 can be 256).

**INV-DIR-2 is the executable form of the marker bug that silently truncates
large directories.** Correct at 50 entries and truncated at 5,000 is the classic
symptom.
