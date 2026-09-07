# SPACE logging protocol (Phase 1 §12)

## One request ID per callback

`RequestId` is minted in `guard()` (§2.3) and carried through `OpCtx` into every
VFS call and every log line.

```json
{"ts":"2026-09-07T10:22:03.117Z","level":"debug","component":"winfsp",
 "operation":"read","request_id":"r_0192...","path":"\\dir\\file.txt",
 "handle":"h_4294967297","offset":4096,"length":4096,
 "duration_ms":0,"result":"ok","error_code":null,"msg":"read served"}
```

**Trace path:** WinFsp callback → C++ adapter (**no logging**) → FFI guard
(`request_id` minted) → VFS → result.

The adapter is deliberately silent — it makes no decisions, so it has nothing to
say. §3.1 records this so nobody later looks for adapter log lines that were
never designed to exist.

The `void` entry points (`cleanup`, `close`, `dir_close`) cannot use `guard`,
because they have no way to report failure. They log anyway: a delete that
leaves no trace is exactly how a namespace bug hides. That gap was real during
Phase 1 development and cost real time.

## Level discipline

Explorer generates **hundreds of callbacks per second** while merely displaying
a directory.

| Level | Used for |
|---|---|
| `trace` | callback entry/exit |
| `debug` | operations with parameters |
| `info` | mount, unmount, startup, shutdown **only** |
| `warn` | recoverable anomalies |
| `error` | a bug in SPACE; a panic (ADR-0013) always logs here |

**`FileNotFound` from `probe` is not an error** — it is the normal existence
check that `Create` depends on. Logging it at error level buries real problems.
A single mounted session was measured at 14,560 `FileNotFound` results, all at
`debug`, and zero `error` lines.

The statuses treated as normal filesystem conversation are listed in
`ffi::is_expected`: `FileNotFound`, `ObjectPathNotFound`, `FileExists`,
`EndOfFile`, `NotADirectory`, `FileIsADirectory`, `DirectoryNotEmpty`,
`BufferOverflow`.

## Never log content

- **No file contents, no raw buffers, no binary blobs — ever.**
- Asserted by test: a file containing a known marker string is written and read
  back through the boundary, and the marker must appear nowhere in the captured
  log output.
- `offset` and `length` are logged; the bytes at that offset are not. That
  distinction is what lets `InvalidParameter` stay a single code (ADR-0014) —
  the specifics live in the log line, not in the taxonomy.

## Paths are logged in Phase 1

Paths **are** logged, because all Phase 1 data is synthetic and a path is the
most useful diagnostic available. The `dir_open` line even carries the
enumeration marker, which is what made the `UmFileContextIsUserContext2` bug
(fs-semantics §1) diagnosable at all.

> **REVISIT BEFORE REAL CUSTOMER DATA (Phase 9).** A path is user content. When
> SPACE holds data that is not synthetic, this decision has to be re-taken:
> either redact path components, hash them, or log only the node's
> `index_number`. The redaction machinery from M0.6 already exists; what is
> missing is the decision, not the mechanism.

## Structured, not formatted

Logging is JSON (`logging.format = "json"`, the only accepted value). Fields are
structured so they can be filtered — `operation`, `request_id`, `handle`,
`path`, `offset`, `length`, `duration_ms`, `result`, `error_code`. Grepping a
prose message is not a diagnostic strategy; the Phase 1 investigation above
depended on filtering by field.
