# SPACE — Phase 1 Execution Manual
## FINAL — implementation baseline

Supersedes v3. This is the Phase 1 specification of record. Continues from `SPACE_Phase_0_Execution_Manual.md`; assumes `phase/0-foundation` is tagged and the guest has the `phase-0-complete` snapshot.

**Phase 1 exists to prove:** *Windows can safely use SPACE as a filesystem, and the contracts Phase 2 will implement are correct, deterministic, testable and stable.*

**The stack Phase 1 builds:**

```
Application → Windows I/O → Kernel I/O Manager → WinFsp
   → C++ WinFsp adapter → C ABI → Rust VFS → MemVfs
```

Phase 2 replaces `MemVfs` only. Everything above it is frozen here.

**Out of scope, without exception:** S3, AWS, PostgreSQL, cloud sync, chunking, BLAKE3 content addressing, byte cache, durable state, WAL, manifests, versions, transfer engine, remote metadata, distributed locking, cloud authentication. References to those phases are allowed; implementations are not.

**Time estimate:** 13–17 focused days.

**Progress tracker:**

```
[ ] S1  Scope, decisions, safety harness
[ ] S2  M1.1  FFI boundary contract
[ ] S3  M1.2  Contracts: identity, trait, semantics, bounds, invariants, concurrency
[ ] S4  M1.3  WinFsp host — mount and unmount
[ ] S5  M1.4  MemVfs + handle manager + invariant checker
[ ] S6  M1.5  Volume info, security, file info
[ ] S7  M1.6  Create / Open / Cleanup / Close
[ ] S8  M1.7  Read / Write + boundary validation
[ ] S9  M1.8  Directory enumeration + Explorer
[ ] S10 M1.9  Rename / Delete / SetBasicInfo / SetFileSize / Flush
[ ] S11 M1.10 VFS conformance suite (no WinFsp required)
[ ] S12 M1.11 Request tracing + NTSTATUS translation
[ ] S13 M1.12 Deadlines, cancellation, fault injection
[ ] S14 M1.13 Fuzz + property tests + resource limits
[ ] S15 M1.14 Shutdown, poison path, kill-while-mounted
[ ] S16 M1.15 Stress, compatibility matrix, ProcMon, soak
[ ] S17 M1.16 Evidence + exit gate
```

---

# SESSION 1 — Scope, decisions, safety harness

## 1.1 Snapshot first

Push everything to your remote, shut the guest down, **Snapshots → Take → `phase-1-start`**. Phase 1 is the first phase that can wedge a mount. Treat the guest as expendable.

## 1.2 Decisions of record (ADRs)

### ADR-0007 — `build.rs` owns the C++ adapter; CMake is retired

`client/main` (Rust) is the single binary. The C++ adapter is a static library compiled by the `cc` crate from `build.rs`. One build system, one dependency graph, one `cargo build`. The Phase 0 CMake file remains as a standalone linkage check, off the build path.

### ADR-0008 — Identifiers are opaque and generational, never pointers

No identifier crossing the FFI is or derives from a memory address. Handles and cursors are `u64` values carrying a generation, so a stale value fails validation instead of dereferencing freed memory. Full identity model in §3.1. Enforced by INV-ID-1…5.

### ADR-0009 — Every callback runs under a deadline

WinFsp callbacks are synchronous: a dispatcher thread blocks until you return, and the kernel waits as long as user mode takes. **The deadline is the only bound that exists.** It applies to the whole callback *including lock acquisition* (§3.6). Expiry produces `OperationTimeout`.

*Revisit when:* Phase 4 introduces network-backed reads. The answer then is progress-based extension or partial reads, not a longer timeout.

### ADR-0010 — The VFS trait is synchronous; dispatcher threads bound concurrency

A future network-backed implementation blocks on a runtime internally, so the interface survives Phase 4 unchanged. Consequence, which must not stay implicit: **the WinFsp dispatcher thread count is the concurrency ceiling for every backing store**, so it is a configured bound:

```toml
[client]
dispatcher_threads = 4     # 0 = WinFsp default; explicit value caps concurrency
```

### ADR-0011 — The contract is versioned, not frozen

```rust
pub const VFS_CONTRACT_VERSION: u32 = 1;
```

The versioned contract is: the `Vfs` trait (§3.2), the semantics document (§3.3), the resource limits (§3.4), the invariants (§3.5), the concurrency model (§3.6), and the `Capabilities` set (§11.5). Changing any of them requires a version bump and an ADR.

### ADR-0012 — Share access is WinFsp-owned

WinFsp's FSD performs share-access checking. The core records `granted_access` per handle for diagnostics and does not enforce. Phase 1 tests the layering: an exclusive open followed by a second open yields `STATUS_SHARING_VIOLATION`.

### ADR-0013 — Panic containment, poisoning, and the limits of both

**Two distinct failure classes. Do not conflate them.**

**Class A — Rust panic.** A recoverable-in-principle bug detected inside Rust: a failed assertion, an unwrap on `None`, an arithmetic overflow in a debug build. `catch_unwind` at the FFI boundary contains these.

```
Rust panic
  → catch_unwind at the FFI entry point (every build profile)
  → log message + request_id at error level
  → return STATUS_INTERNAL_ERROR to Windows
  → transition RUNNING → POISONING
  → controlled shutdown, owned by the main thread
```

Why catch rather than abort: the log line carrying the `request_id` is the only diagnostic you get, and an abort discards it. Why poison rather than continue: a panic means an assumption was violated, possibly mid-mutation. A filesystem that recovers from a panic and keeps serving reads has, by definition, continued past a broken invariant.

**This requires `panic = "unwind"`.** Phase 0 set `panic = "abort"` in the workspace release profile. **Amend it** — the guard must be effective in release, or debug and release have different panic semantics, which is worse than either:

```toml
[profile.release]
debug = 1
# panic = "abort" removed in Phase 1 — see ADR-0013.
```

Record the amendment in the Phase 0 document. Do not leave it describing behaviour the code no longer has.

**Class B — process-level failure.** Access violation, stack overflow, allocation failure/abort, a fault inside the C++ adapter, `TerminateProcess`, external kill.

**`catch_unwind` provides no protection against any of these, and this manual never claims it does.** For Class B the recovery mechanism is external: the process dies, WinFsp's FSD observes the termination, and the volume is torn down. `S:` disappears and the system stays healthy. That is what §15.2's kill tests certify — and it is the only thing they certify, because Phase 1 has no durable state (§15.4).

| Failure | Contained by | Windows sees | Certified by |
|---|---|---|---|
| Rust panic | `catch_unwind` + poison | `STATUS_INTERNAL_ERROR`, then clean unmount | §13.5, §15.1 |
| Access violation / stack overflow / OOM abort | nothing in-process | process death, FSD tears down volume | §15.2 |
| C++ adapter fault | nothing in-process | process death, FSD tears down volume | §15.2 |
| External kill | nothing in-process | process death, FSD tears down volume | §15.2 |

### ADR-0013a — The poisoned-state lifecycle

```
RUNNING ──panic / unrecoverable invariant failure──▶ POISONING
POISONING ──flag published, unmount signalled──────▶ POISONED
POISONED ──main thread begins teardown────────────▶ STOPPING
STOPPING ──dispatcher stopped, mount removed──────▶ UNMOUNTED
```

| Concern | Rule |
|---|---|
| New callbacks in POISONING/POISONED/STOPPING | Return `STATUS_INTERNAL_ERROR` immediately. Do not acquire the state lock. Do not read or mutate filesystem state. |
| In-flight callbacks | Allowed to run to completion or to their deadline. They are never aborted; aborting them is what would leave Windows in an undefined state. |
| `Cleanup` and `Close` after poisoning | Become no-ops. They are `void` and cannot report failure, and touching poisoned state is exactly what poisoning forbids. Memory is reclaimed by process exit, which is imminent. This is a deliberate, bounded leak — documented, not accidental. |
| Who owns shutdown | **The main thread. Only the main thread.** |
| Who initiates | The poisoning thread sets the flag and signals a channel. It performs no teardown itself. |
| Deadlock prevention | `FspFileSystemStopDispatcher` **must never be called from a dispatcher thread** — it waits for dispatcher threads to drain, including the caller. The signal-and-return design makes this structurally impossible. |
| Fully stopped | After `space_adapter_unmount()` returns and `space_core_stop()` completes. `os-safety-check.ps1` must then pass. |

The same lifecycle is entered by an unrecoverable invariant failure detected in a debug build (§3.5).

### ADR-0014 — Error taxonomy: local now, remote later

| Class | Codes | Produced by |
|---|---|---|
| **Filesystem / VFS** (Phase 1) | `FileNotFound`, `ObjectPathNotFound`, `FileExists`, `NotADirectory`, `FileIsADirectory`, `DirectoryNotEmpty`, `CannotDelete`, `InvalidParameter`, `InvalidHandle`, `ObjectNameInvalid`, `NameTooLong`, `EndOfFile`, `SharingViolation`, `PermissionDenied`, `DiskFull`, `ResourceExhausted`, `OperationTimeout`, `Cancelled`, `BufferOverflow`, `InternalError` | FFI boundary and VFS |
| **Remote / network** (Phase 4+, reserved) | `NetworkTimeout`, `NetworkUnavailable`, `ProtocolViolation`, `AuthFailed` | transfer engine only |

**Rules:**

- `OperationTimeout` means *a filesystem operation exceeded its configured execution deadline.* It is the only timeout Phase 1 can produce.
- `NetworkTimeout` means *a remote request exceeded its deadline.* **The VFS layer may never produce it.** A future network-backed implementation handles its own network timeouts internally and surfaces `OperationTimeout` at the VFS boundary. The Windows-facing layer does not care why something was slow.
- Enforced by a conformance assertion (§11.7).

**On granularity:** `InvalidParameter` covers bad offsets, bad lengths, and overflow. Splitting it into `InvalidOffset` and `InvalidLength` would add codes that map to the same NTSTATUS and change no behaviour; the specifics belong in the log line, which carries offset and length on every operation (§12.1). Deliberately not added.

**On placement:** the NTSTATUS mapping does **not** live in `contracts`. See ADR-0015.

### ADR-0015 — NTSTATUS translation lives at the Windows boundary

`contracts` is shared with the cloud service, which has no business knowing about NTSTATUS. The error *taxonomy* is a contract concern; the *translation* is a Windows concern.

- `contracts::ErrorCode` — the taxonomy. No Windows types, no NTSTATUS.
- `client/core/src/ffi/ntstatus.rs` — the mapping table and the exhaustiveness test.
- The VFS never sees an NTSTATUS or a Win32 error code. It returns `SpaceError` and nothing else.

Move Phase 0's `ErrorCode::ntstatus()` accordingly, and move M0.4's mapping test with it. The test iterates `ErrorCode::ALL`, so it keeps working unchanged.

**Multiple codes may map to one NTSTATUS.** `OperationTimeout` and `NetworkTimeout` both map to `STATUS_IO_TIMEOUT`; the integrity codes all map to `STATUS_FILE_CORRUPT_ERROR`. The test asserts **totality** — every code has a mapping — not injectivity. If your Phase 0 test asserted a bijection, relax it and note why in the test comment.

## 1.3 Error registry additions

Phase 1 adds these to `contracts::ErrorCode`. M0.4's exhaustiveness test refuses to compile until each is classified.

| Code | Retryable | NTSTATUS | Value |
|---|---|---|---|
| `ObjectNameInvalid` | no | `STATUS_OBJECT_NAME_INVALID` | `0xC000_0033` |
| `ObjectPathNotFound` | no | `STATUS_OBJECT_PATH_NOT_FOUND` | `0xC000_003A` |
| `NotADirectory` | no | `STATUS_NOT_A_DIRECTORY` | `0xC000_0103` |
| `FileIsADirectory` | no | `STATUS_FILE_IS_A_DIRECTORY` | `0xC000_00BA` |
| `EndOfFile` | no | `STATUS_END_OF_FILE` | `0xC000_0011` |
| `NameTooLong` | no | `STATUS_NAME_TOO_LONG` | `0xC000_0106` |
| `CannotDelete` | no | `STATUS_CANNOT_DELETE` | `0xC000_0121` |
| `BufferOverflow` | no | `STATUS_BUFFER_OVERFLOW` | `0x8000_0005` |
| `OperationTimeout` | yes | `STATUS_IO_TIMEOUT` | `0xC000_00B5` |

`BufferOverflow` is a *warning* status, not an error — `GetSecurityByName` returns it to report an undersized buffer. `SpaceError` must carry it without treating it as a failure, or it falls into `InternalError` (§6.2).

## 1.4 Tooling

```powershell
winget install -e --id Microsoft.Sysinternals.ProcessMonitor
winget install -e --id Microsoft.Sysinternals.ProcessExplorer
```

Process Monitor produces the external half of INV-NS-6. You cannot assert "no writes outside the boundary" from inside your own process.

## 1.5 `scripts/os-safety-check.ps1`

Run after every stress test in Sessions 13–16. Its output is gate evidence.

```powershell
param([datetime]$Since = (Get-Date).AddHours(-1))

$fail = 0
Write-Host "=== OS safety check since $Since ===" -ForegroundColor Cyan

$kp = Get-WinEvent -FilterHashtable @{LogName='System'; Id=41; StartTime=$Since} -ErrorAction SilentlyContinue
if ($kp) { Write-Host "FAIL  Kernel-Power 41 (unexpected shutdown)" -ForegroundColor Red; $fail++ }
else     { Write-Host "PASS  no unexpected shutdown" -ForegroundColor Green }

$bc = Get-WinEvent -FilterHashtable @{LogName='System'; Id=1001; StartTime=$Since} -ErrorAction SilentlyContinue |
      Where-Object { $_.ProviderName -like "*BugCheck*" }
if ($bc) { Write-Host "FAIL  BugCheck recorded" -ForegroundColor Red; $fail++ }
else     { Write-Host "PASS  no bugcheck" -ForegroundColor Green }

if (Test-Path "C:\Windows\MEMORY.DMP") {
  $d = (Get-Item "C:\Windows\MEMORY.DMP").LastWriteTime
  if ($d -gt $Since) { Write-Host "FAIL  kernel dump written $d" -ForegroundColor Red; $fail++ }
}

$sys = Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2; StartTime=$Since} -ErrorAction SilentlyContinue
if ($sys) {
  Write-Host "WARN  $($sys.Count) critical/error system events:" -ForegroundColor Yellow
  $sys | Select-Object TimeCreated, Id, ProviderName, Message -First 10 | Format-List
}

$vols = & "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol 2>&1 | Out-String
if ($vols -match "SPACE") { Write-Host "FAIL  stale SPACE volume:`n$vols" -ForegroundColor Red; $fail++ }
else                      { Write-Host "PASS  no stale SPACE volume" -ForegroundColor Green }

if (Get-Process space-client -ErrorAction SilentlyContinue) {
  Write-Host "FAIL  space-client still running" -ForegroundColor Red; $fail++
} else { Write-Host "PASS  no orphaned client process" -ForegroundColor Green }

if (Test-Path "S:\") { Write-Host "FAIL  S: still present" -ForegroundColor Red; $fail++ }
else                 { Write-Host "PASS  S: released" -ForegroundColor Green }

if ($fail -gt 0) { Write-Host "`n$fail OS-safety check(s) FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`nOS safety OK" -ForegroundColor Green
```

## 1.6 Explicitly out of Phase 1

Chunking · BLAKE3 content addressing · content-integrity verification · byte cache · durable state · WAL · manifests · versions · transfer engine · cloud, S3, AWS · PostgreSQL · remote metadata · synchronization · distributed locking · cloud authentication · oplocks and fine-grained locking (Phase 10) · reparse points, alternate streams, extended attributes · any security model beyond one fixed descriptor · performance optimisation · thread-sanitization tooling (meaningless under §3.6's model) · the `CorruptBytes` fault (§13.1).

If Phase 1 makes you want to change a Phase 0 schema, stop and decide deliberately.

```powershell
git checkout -b phase/1-winfsp
git commit -am "M1.0: scope, ADR-0007..0015, error registry, OS-safety script"
```

---

# SESSION 2 — M1.1: The FFI boundary contract

**Time: 1–2 days.** The only surface between C++ and Rust.

## 2.1 Boundary rules

| # | Rule |
|---|---|
| 1 | **Representation.** Only `#[repr(C)]` structs of fixed-width scalars and pointers cross. No Rust enums, slices, `String`, `Option`, or trait objects. |
| 2 | **Ownership.** Every buffer is caller-allocated and caller-owned. Rust never returns memory that C++ must free, and never retains a caller pointer past the call. |
| 3 | **Lifetime.** Pointer parameters are valid only for the duration of the call. The one exception is `space_dir_entry.name`, valid until the next `space_core_dir_next` on the same cursor — stated in the header, at the field. |
| 4 | **Nullability.** Every pointer parameter is documented non-null unless explicitly marked optional. Null where non-null is required returns `STATUS_INVALID_PARAMETER`; it never dereferences. |
| 5 | **Strings.** UTF-16, NUL-terminated. Rust scans with a hard length cap (§3.4), validates UTF-16, and copies. Invalid encoding → `ObjectNameInvalid`. Missing terminator → `NameTooLong`. |
| 6 | **Lengths.** All lengths are `uint32_t` and are validated against `max_io_bytes` before any allocation or indexing. |
| 7 | **Alignment and size.** Every shared struct's size and field offsets are asserted on both sides (§2.5). |
| 8 | **Identifiers.** Opaque `u64`, generational, never pointer-derived. `0` is never valid (§3.1). |
| 9 | **Errors.** Every fallible entry point returns `NTSTATUS`. No out-of-band channel. The two `void` entry points cannot fail by contract. |
| 10 | **Panics.** Contained by `catch_unwind` in every build profile (ADR-0013). No unwind ever reaches a C++ frame. |
| 11 | **Deadlines.** Every entry point is bounded (ADR-0009), including lock acquisition. |

## 2.2 `client/winfsp-adapter/include/space_core.h`

```c
#pragma once
#include <stdint.h>
#include <stddef.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint32_t space_status;     /* an NTSTATUS value */
typedef uint64_t space_handle;     /* opaque, generational; 0 is never valid */
typedef uint64_t space_cursor;     /* opaque, generational; 0 is never valid */
#define SPACE_INVALID_HANDLE ((space_handle)0)
#define SPACE_INVALID_CURSOR ((space_cursor)0)

typedef struct {
    uint32_t file_attributes;
    uint32_t reparse_tag;          /* always 0 in Phase 1 */
    uint64_t allocation_size;
    uint64_t file_size;
    uint64_t creation_time;        /* Windows FILETIME, 100ns since 1601 */
    uint64_t last_access_time;
    uint64_t last_write_time;
    uint64_t change_time;
    uint64_t index_number;         /* Windows-visible identity; see §3.1 */
} space_file_info;                 /* 64 bytes; asserted on both sides */

typedef struct { uint64_t total_size; uint64_t free_size; } space_volume_info;

typedef struct {
    const wchar_t*  name;          /* borrowed: valid until the next dir_next on this cursor */
    space_file_info info;
} space_dir_entry;

/* ---- lifecycle ---- */
space_status space_core_start(const wchar_t* config_path);
space_status space_core_stop(void);

/* ---- volume ---- */
space_status space_core_get_volume_info(space_volume_info* out);

/* ---- open / close ---- */
space_status space_core_create(const wchar_t* path, uint32_t create_options,
                               uint32_t granted_access, uint32_t file_attributes,
                               uint64_t allocation_size,
                               space_handle* out_handle, space_file_info* out_info);
space_status space_core_open(const wchar_t* path, uint32_t create_options,
                             uint32_t granted_access,
                             space_handle* out_handle, space_file_info* out_info);
space_status space_core_overwrite(space_handle h, uint32_t file_attributes,
                                  uint8_t replace_attributes, uint64_t allocation_size,
                                  space_file_info* out_info);
void         space_core_cleanup(space_handle h, const wchar_t* path /*optional, may be NULL*/,
                                uint32_t flags);
void         space_core_close(space_handle h);

/* ---- io ---- */
space_status space_core_read(space_handle h, void* buffer, uint64_t offset,
                             uint32_t length, uint32_t* out_transferred);
space_status space_core_write(space_handle h, const void* buffer, uint64_t offset,
                              uint32_t length, uint8_t write_to_eof, uint8_t constrained_io,
                              uint32_t* out_transferred, space_file_info* out_info);
space_status space_core_flush(space_handle h /*0 = whole volume*/, space_file_info* out_info);

/* ---- info ---- */
space_status space_core_get_file_info(space_handle h, space_file_info* out_info);
space_status space_core_set_basic_info(space_handle h, uint32_t attrs,
                                       uint64_t ctime, uint64_t atime,
                                       uint64_t wtime, uint64_t chtime,
                                       space_file_info* out_info);
space_status space_core_set_file_size(space_handle h, uint64_t new_size,
                                      uint8_t set_allocation, space_file_info* out_info);
space_status space_core_get_security_by_name(const wchar_t* path,
                                             uint32_t* out_attrs /*optional, may be NULL*/,
                                             void* sd_buf /*optional, may be NULL*/,
                                             size_t* sd_size);

/* ---- namespace mutation ---- */
space_status space_core_can_delete(space_handle h, const wchar_t* path);
space_status space_core_rename(space_handle h, const wchar_t* path,
                               const wchar_t* new_path, uint8_t replace_if_exists);

/* ---- directory enumeration (cursor scoped to ONE ReadDirectory call) ---- */
space_status space_core_dir_open(space_handle h, const wchar_t* pattern /*may be NULL*/,
                                 const wchar_t* marker /*may be NULL*/, space_cursor* out_cursor);
space_status space_core_dir_next(space_cursor cursor, space_dir_entry* out_entry,
                                 uint8_t* out_has_more);
void         space_core_dir_close(space_cursor cursor);

#ifdef __cplusplus
}
#endif
```

## 2.3 The guard — `client/core/src/ffi/mod.rs`

Implements ADR-0013 and ADR-0013a.

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, Ordering};

#[repr(u8)]
pub enum LifeState { Running = 0, Poisoning = 1, Poisoned = 2, Stopping = 3, Unmounted = 4 }

static STATE: AtomicU8 = AtomicU8::new(LifeState::Running as u8);

#[inline]
pub fn accepting_work() -> bool { STATE.load(Ordering::SeqCst) == LifeState::Running as u8 }

fn guard<F>(op: &'static str, f: F) -> space_status
where F: FnOnce(&OpCtx) -> Result<(), SpaceError>
{
    // ADR-0013a: once past RUNNING, touch nothing.
    if !accepting_work() { return ntstatus::INTERNAL_ERROR; }

    let cx = OpCtx::new(callback_deadline());
    let started = std::time::Instant::now();

    // ADR-0013: a panic never reaches a C++ frame, in any profile.
    match catch_unwind(AssertUnwindSafe(|| f(&cx))) {
        Ok(Ok(())) => { log_boundary(op, &cx, started, None); ntstatus::SUCCESS }
        Ok(Err(e)) => {
            log_boundary(op, &cx, started, Some(&e));
            ntstatus::from_error_code(e.code)
        }
        Err(payload) => {
            tracing::error!(op, request_id = %cx.request_id,
                            "PANIC at FFI boundary: {}", panic_message(&payload));
            enter_poisoning();          // sets state, signals main thread, returns
            ntstatus::INTERNAL_ERROR
        }
    }
}

/// Called only from a dispatcher thread. Sets state and signals; performs NO teardown.
fn enter_poisoning() {
    STATE.store(LifeState::Poisoning as u8, Ordering::SeqCst);
    SHUTDOWN_TX.get().map(|tx| { let _ = tx.try_send(ShutdownReason::Poisoned); });
    STATE.store(LifeState::Poisoned as u8, Ordering::SeqCst);
}
```

`cleanup` and `close` bypass `guard` because they are `void`. They check `accepting_work()` first and return immediately if false (ADR-0013a):

```rust
#[no_mangle]
pub extern "C" fn space_core_close(h: space_handle) {
    if !accepting_work() { return; }         // deliberate bounded leak; process exit reclaims
    let _ = catch_unwind(AssertUnwindSafe(|| core().close(HandleId::from_raw(h))));
}
```

String conversion, used by every path parameter:

```rust
unsafe fn wstr(p: *const u16) -> Result<String, SpaceError> {
    if p.is_null() { return Err(SpaceError::new(ErrorCode::ObjectNameInvalid, "null path")); }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
        if len > limits::MAX_PATH_CHARS {      // §3.4
            return Err(SpaceError::new(ErrorCode::NameTooLong, "unterminated or oversized path"));
        }
    }
    String::from_utf16(std::slice::from_raw_parts(p, len))
        .map_err(|_| SpaceError::new(ErrorCode::ObjectNameInvalid, "invalid UTF-16"))
}
```

The length cap is not decoration: a missing NUL terminator would otherwise walk memory until it faults.

## 2.4 Build glue — `client/main/build.rs`

```rust
fn main() {
    let winfsp = winfsp_dir();                       // registry, then default path
    println!("cargo:rerun-if-changed=../winfsp-adapter");

    cc::Build::new()
        .cpp(true).std("c++20")
        .include(format!("{winfsp}/inc"))
        .include("../winfsp-adapter/include")
        .file("../winfsp-adapter/src/host.cpp")
        .file("../winfsp-adapter/src/callbacks.cpp")
        .compile("space_winfsp_adapter");

    println!("cargo:rustc-link-search=native={winfsp}/lib");
    println!("cargo:rustc-link-lib=dylib=winfsp-x64");
    println!("cargo:rustc-link-lib=delayimp");
    println!("cargo:rustc-link-arg=/DELAYLOAD:winfsp-x64.dll");
}
```

⚠️ **The delay-load line will cost you an afternoon if you skip it.** `winfsp-x64.dll` lives in `C:\Program Files (x86)\WinFsp\bin`, not System32. A direct link builds cleanly and fails at launch. With delay-loading plus `FspLoad(nullptr)`, WinFsp locates its own DLL through its registry entry.

## 2.5 Tests

**Layout (rule 7):**

```cpp
static_assert(sizeof(space_file_info) == 64, "space_file_info layout drift");
static_assert(offsetof(space_file_info, file_size) == 16, "field offset drift");
```

```rust
#[test] fn file_info_layout_matches_c() {
    assert_eq!(std::mem::size_of::<SpaceFileInfo>(), 64);
    assert_eq!(std::mem::offset_of!(SpaceFileInfo, file_size), 16);
}
```

Layout drift otherwise surfaces as garbage file sizes and takes days to trace.

**Strings and nullability:** valid ASCII; valid surrogate pair; unpaired surrogate → `ObjectNameInvalid`; empty → `Ok("")`; null → `ObjectNameInvalid`; 40,000 chars without NUL → `NameTooLong`, no crash; null `out_handle` → `InvalidParameter`, no dereference.

**Panic model (ADR-0013) — all must pass in debug *and* release:**

| Test | Expected |
|---|---|
| deliberate `panic!()` inside a guarded entry point | `STATUS_INTERNAL_ERROR`; no abort; no unwind past the guard |
| panic message + `request_id` in the log | present at `error` level |
| next callback after a panic | `STATUS_INTERNAL_ERROR` immediately, state lock never acquired |
| `close`/`cleanup` after poisoning | return silently, no panic, no state access |
| poisoning from a dispatcher thread | no deadlock; unmount completes; `os-safety-check.ps1` clean |

**Timeouts:** an expired `OpCtx` yields `OperationTimeout` → `STATUS_IO_TIMEOUT`; no core path can return `NetworkTimeout` (grep test over the crate plus §11.7).

```powershell
git commit -am "M1.1: FFI boundary contract, panic containment, poison lifecycle, build glue"; git tag M1.1
```

---

# SESSION 3 — M1.2: Identity, trait, semantics, limits, invariants, concurrency

**Time: 3 days.** No implementation here. This session produces the contract Phase 2 implements.

## 3.1 Identity model — `docs/protocols/identity.md`

Five identifiers. They do not collapse into each other.

| Identifier | Meaning | Scope & lifetime | Crosses FFI | Generational | Visible to Windows |
|---|---|---|---|---|---|
| `NodeId` | SPACE's internal object identity — a file or directory | process lifetime; stable across rename, move, and unlink | **no** | yes (slab index + generation) | no |
| `HandleId` | an opened filesystem handle; one per successful `create`/`open` | from `create`/`open` until `close` | yes, as WinFsp's `FileContext` | yes | no |
| `CursorId` | directory enumeration state | **a single `ReadDirectory` call** (§3.3) | yes | yes | no |
| `RequestId` | one boundary request, for tracing | one callback | no (minted in Rust) | n/a (UUIDv7) | no |
| `index_number` | Windows-visible file identity, in `FSP_FSCTL_FILE_INFO` | life of the node; stable across rename | yes, inside `space_file_info` | no (monotonic counter) | **yes** |

Rules:

- **`index_number` is not derived from `NodeId`.** A slab index is reused after free; Windows treats `index_number` as a file identity key and reuse causes Explorer to conflate distinct files. Allocate from a monotonic `u64` counter that is never reused within a process lifetime.
- **`RequestId` is minted at the first Rust frame** (the `guard`, §2.3). The C++ adapter performs no logging and no decision-making, so there is nothing above that point to correlate. This is honest about the trace's true origin; do not add a request-id parameter to the FFI for a layer that never uses it.
- **No identifier is or derives from a memory address** (ADR-0008, INV-ID-5).
- **Encoding for `HandleId` and `CursorId`:** `(generation as u64) << 32 | (index as u64 + 1)`. The `+1` guarantees `0` is never valid.
- **Stale resolution is always an error, never an aliasing.** After a slot is freed and reused, the old value fails the generation check (INV-ID-3).

## 3.2 The `Vfs` trait — `client/core/src/vfs/mod.rs`

```rust
pub const VFS_CONTRACT_VERSION: u32 = 1;

/// Per-operation context, created at the FFI boundary and threaded everywhere.
pub struct OpCtx {
    pub request_id: RequestId,
    pub deadline: std::time::Instant,
}
impl OpCtx {
    pub fn remaining(&self) -> Option<std::time::Duration> {
        self.deadline.checked_duration_since(std::time::Instant::now())
    }
    pub fn check(&self) -> Result<(), SpaceError> {
        match self.remaining() {
            Some(_) => Ok(()),
            None => Err(SpaceError::new(ErrorCode::OperationTimeout, "operation deadline exceeded")),
        }
    }
}

pub trait Vfs: Send + Sync {
    fn contract_version(&self) -> u32 { VFS_CONTRACT_VERSION }

    fn volume_info(&self, cx: &OpCtx) -> Result<VolumeInfo, SpaceError>;

    /// Existence + attributes probe. WinFsp calls this before create/open.
    fn probe(&self, cx: &OpCtx, path: &VfsPath) -> Result<Probe, SpaceError>;

    fn create(&self, cx: &OpCtx, path: &VfsPath, o: CreateOptions) -> Result<Opened, SpaceError>;
    fn open(&self, cx: &OpCtx, path: &VfsPath, o: OpenOptions) -> Result<Opened, SpaceError>;
    fn overwrite(&self, cx: &OpCtx, h: HandleId, attrs: u32, replace_attrs: bool, alloc: u64)
        -> Result<FileInfo, SpaceError>;
    fn cleanup(&self, cx: &OpCtx, h: HandleId, flags: CleanupFlags);
    fn close(&self, cx: &OpCtx, h: HandleId);

    /// Fills `buf`, returns bytes transferred. `&mut [u8]` so the WinFsp buffer is
    /// written directly — no intermediate allocation, now or in Phase 4.
    fn read(&self, cx: &OpCtx, h: HandleId, offset: u64, buf: &mut [u8]) -> Result<u32, SpaceError>;
    fn write(&self, cx: &OpCtx, h: HandleId, offset: u64, buf: &[u8], mode: WriteMode)
        -> Result<(u32, FileInfo), SpaceError>;
    fn flush(&self, cx: &OpCtx, h: Option<HandleId>) -> Result<Option<FileInfo>, SpaceError>;

    fn file_info(&self, cx: &OpCtx, h: HandleId) -> Result<FileInfo, SpaceError>;
    fn set_basic_info(&self, cx: &OpCtx, h: HandleId, patch: BasicInfoPatch) -> Result<FileInfo, SpaceError>;
    fn set_file_size(&self, cx: &OpCtx, h: HandleId, new_size: u64, set_allocation: bool)
        -> Result<FileInfo, SpaceError>;

    fn can_delete(&self, cx: &OpCtx, h: HandleId) -> Result<(), SpaceError>;
    fn rename(&self, cx: &OpCtx, h: HandleId, to: &VfsPath, replace: bool) -> Result<(), SpaceError>;

    fn dir_open(&self, cx: &OpCtx, h: HandleId, pattern: Option<&str>, marker: Option<&str>)
        -> Result<CursorId, SpaceError>;
    fn dir_next(&self, cx: &OpCtx, cursor: CursorId) -> Result<Option<DirEntry>, SpaceError>;
    fn dir_close(&self, cx: &OpCtx, cursor: CursorId);
}

/// Read-only structural self-check. Required of every Vfs implementation. §3.5.
pub trait VfsDiagnostics {
    fn check_invariants(&self) -> Result<(), InvariantViolation>;
}
```

Note `cleanup`, `can_delete`, and `rename` take the handle and **not** a source path: the handle already identifies the node, and accepting a redundant path invites the two to disagree. WinFsp supplies a `FileName` on these callbacks; the adapter uses it only for logging.

`VfsPath` is a validated, normalized type. Construction enforces every naming rule in §3.3, so no implementation can receive an invalid path.

## 3.3 Filesystem semantics — `docs/protocols/fs-semantics.md`

Write this before implementing. Phase 2 implements this document, not your Phase 1 code.

### 3.3.1 Handle lifecycle — corrected against the WinFsp contract

The commonly repeated summary "Cleanup fires when the last handle to the file closes" is **wrong** and produces real bugs. The accurate contract:

- Each successful `Create`/`Open` corresponds to one kernel **file object**, and to exactly one `FileContext` — one `HandleId`.
- A file object's handle may be duplicated (`DuplicateHandle`); all duplicates share one file object.
- **`Cleanup` is called per file object**, when the last *handle to that file object* is released. Other file objects for the same file may still be open.
- **`Close` is called exactly once per `Create`/`Open`, always**, when the file object is destroyed. After `Close` the `FileContext` must not be used.
- `Cleanup` always precedes `Close` for a given file object when it is posted at all.
- **`Cleanup` posting is conditional on volume parameters.** With `PostCleanupWhenModifiedOnly = 1`, WinFsp suppresses `Cleanup` for file objects that neither modified the file nor have a delete pending.

Two consequences that are easy to get wrong and that this manual fixes:

| Bookkeeping | Where it must happen | Why |
|---|---|---|
| decrement `open_count`, free the handle slot | **`Close`** | `Close` is the only callback guaranteed to fire for every open |
| unlink the node from the namespace | **`Cleanup`, when `FspCleanupDelete` is set** | this is the only signal Windows gives that the delete should take effect |
| reclaim node storage | **`Close`, when `open_count` reaches 0 and the node is unlinked** | deterministic, and independent of whether `Cleanup` was posted |

**Phase 1 sets `PostCleanupWhenModifiedOnly = 0`** so `Cleanup` is always posted (§4.1). Determinism is worth more than the optimisation here. The bookkeeping rules above hold either way, which is the point of splitting them.

`CanDelete` versus `SetDelete`: WinFsp offers both; when `SetDelete` is provided it supersedes `CanDelete`. Phase 1 implements **`CanDelete` only** — a pure query, simplest to reason about — and does not provide `SetDelete`. Recorded so Phase 2 knows it is a choice, not an oversight.

| Rule | Behaviour | Invariant |
|---|---|---|
| `Close` count | exactly one per `create`/`open` | INV-FS-3 |
| I/O after `Close` | `InvalidHandle` | INV-FS-3 |
| I/O after `Cleanup`, before `Close` | `InvalidHandle` — Windows sends no further I/O on that file object, so any such call is a bug or an attack | INV-FS-3 |
| duplicate `close` on one `HandleId` | second is a silent no-op at the FFI (`void`); the handle manager reports `InvalidHandle` internally; no panic, no double free | INV-ID-3 |
| stale `HandleId` after slot reuse | `InvalidHandle` | INV-ID-3 |

### 3.3.2 Deleted-but-open lifecycle

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

Reclamation is deterministic: exactly at the `Close` that brings `open_count` to zero on an unlinked node. Never earlier, never later, never on a timer.

Invariant note: **INV-NS-2 (every non-root node has exactly one parent) does not apply to unlinked nodes.** The checker carves them out explicitly (§5.3); this is a documented exception, not a silent one.

### 3.3.3 Open and create

| Situation | Result |
|---|---|
| open nonexistent | `FileNotFound` |
| open with missing parent directory | `ObjectPathNotFound` (distinct; Windows tools distinguish them) |
| create existing | `FileExists` |
| open directory with `FILE_NON_DIRECTORY_FILE` | `FileIsADirectory` |
| open file with `FILE_DIRECTORY_FILE` | `NotADirectory` |
| `FILE_DELETE_ON_CLOSE` | WinFsp sets `FspCleanupDelete` at cleanup; the VFS unlinks then |
| same file opened twice | two `HandleId`s, one `NodeId`, `open_count == 2` |
| share access | WinFsp-owned (ADR-0012); the core records `granted_access` only |

### 3.3.4 Read and write

| Situation | Result |
|---|---|
| read at or past EOF | `EndOfFile`, zero transferred — not success-with-zero |
| read crossing EOF | success, short transfer — normal, not an error |
| read length 0 | success, zero transferred |
| `offset + length` overflows `u64` | `InvalidParameter`, no panic |
| length above `max_io_bytes` | `InvalidParameter`, before any allocation |
| write past EOF | file extends, gap zero-filled |
| `WriteToEndOfFile` | ignore offset, append at current EOF |
| `ConstrainedIo`, `offset >= file_size` | zero transferred, success, file unchanged |
| `ConstrainedIo` crossing EOF | write truncated to the existing size |
| write exceeding `max_bytes` | `DiskFull`, **file byte-identical to before** (INV-RES-3) |

`ConstrainedIo` comes from the cache manager's write-behind path. Ignoring it produces files that grow during a copy.

### 3.3.5 Rename and delete

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
| delete while open | §3.3.2 | INV-FS-4 |

### 3.3.6 Timestamps

| Operation | created | accessed | written | changed |
|---|---|---|---|---|
| create | set | set | set | set |
| read | — | set | — | — |
| write | — | — | set | set |
| set_file_size | — | — | set | set |
| set_basic_info | explicit | explicit | explicit | set |
| rename | — | — | — | set |

`set_basic_info` treats `0` as "do not change" per field.

### 3.3.7 Names and paths

`CaseSensitiveSearch = 0`, `CasePreservedNames = 1`. Names are stored as given; lookup uses a folded key.

This is the only permitted capability (§11.5), and **both variants are specified**, so neither is a gap:

| `unicode_case_folding` | Required behaviour |
|---|---|
| `false` (Phase 1) | `A`/`a` collide; `Å`/`å` do **not** collide; case preserved on both |
| `true` (Phase 2 target) | `A`/`a` collide; `Å`/`å` **do** collide, per Unicode simple case folding; case preserved |

Rejected at `VfsPath` construction: empty; no leading `\`; `.` or `..` components; repeated separators; `:` in a component; reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or without extension); trailing space or dot in a component; `< > " | ? *`; embedded NUL; invalid UTF-16; any bound in §3.4 exceeded.

**Path resolution never escapes the SPACE namespace** (INV-NS-6).

### 3.3.8 Directory enumeration — the semantic contract

This subsection is the **VFS contract**. The WinFsp protocol that carries it is §9.1, in the adapter, and does not leak upward.

| Rule | Definition |
|---|---|
| Order | `.`, then `..` (both omitted for the root), then children in ascending folded-name order. The order is total and stable for a given directory state. |
| Marker | An enumeration with `marker = Some(name)` yields entries **strictly after** `name` in that order. `marker = None` starts at the beginning. |
| Completeness | Over any sequence of calls whose markers chain correctly, each entry is yielded exactly once. | 
| Termination | An enumeration of a directory with N children yields at most N + 2 entries. |
| Cursor lifetime | **A cursor is valid for one `ReadDirectory` call only.** It is opened, drained or abandoned, and closed within that call. Resumption across calls is carried by `marker`, never by a cursor. |
| Pattern | `pattern` is accepted and **ignored** in Phase 1; WinFsp performs pattern filtering. Recorded as a dependency, not a gap. |
| Mutation during enumeration | Cannot occur in Phase 1 — §3.6 serializes operations. Phase 2 must define it if that changes. |

Scoping the cursor to a single call eliminates a whole class of leak and staleness bugs, and makes the concurrent-cursor count bounded by dispatcher threads rather than by client behaviour.

## 3.4 Resource limits — `docs/protocols/resource-limits.md`

| # | Limit | Value | Enforced at | Error | State on failure | Atomic |
|---|---|---|---|---|---|---|
| L1 | path length | 32,767 chars | `VfsPath::parse` | `NameTooLong` | unchanged | n/a |
| L2 | component length | 255 chars | `VfsPath::parse` | `NameTooLong` | unchanged | n/a |
| L3 | path depth | 512 components | `VfsPath::parse` | `NameTooLong` | unchanged | n/a |
| L4 | single read/write | 16 MiB | FFI boundary, before allocation | `InvalidParameter` | unchanged | n/a |
| L5 | total bytes | `config.max_bytes` | write / set_file_size | `DiskFull` | **unchanged** | yes |
| L6 | entries per directory | 65,536 | create / rename-in | `ResourceExhausted` | **unchanged** | yes |
| L7 | open handles | 65,536 | handle manager `alloc` | `ResourceExhausted` | **unchanged** | yes |
| L8 | open cursors | 256 | cursor table `alloc` | `ResourceExhausted` | **unchanged** | yes |
| L9 | callback duration | `config.callback_timeout_ms` | `OpCtx`, incl. lock wait | `OperationTimeout` | see note | no |
| L10 | concurrent callbacks | `config.dispatcher_threads` | WinFsp | queued by WinFsp | n/a | n/a |

Notes:

- **L6 is 65,536, not one million.** Phase 1 tests boundary correctness, not scale; a million-entry directory turns `check_invariants()` into a multi-minute operation and buys nothing. The 5,000-entry Explorer test (§9.2) remains, well inside the bound.
- **L8 is 256** because a cursor lives for one call (§3.3.8) and concurrency is bounded by dispatcher threads. The limit exists to catch a leak, not to serve a workload.
- **L9 state on failure:** a timeout during lock acquisition leaves state untouched. A timeout detected mid-operation may leave a *completed* prefix; the operation reports what it transferred. This is stated rather than promised as atomic because Phase 4 will make it real.
- Every limit gets a config key, a limit test, and a limit+1 test (§14.3).

## 3.5 VFS invariants — `docs/protocols/vfs-invariants.md`

An invariant holds **after every operation, successful or failed**. Semantics say what a call returns; invariants say the filesystem is still well-formed afterwards. The second is what catches a Phase 2 namespace bug.

**The checker is read-only.** `check_invariants()` observes and reports. It never mutates, never repairs, never allocates node state, and never resolves a violation it finds. A diagnostic that repairs is a diagnostic that hides bugs. It must be safe to call at any point in any test, and calling it must not change any subsequent result.

### Identity — INV-ID

| ID | Invariant | Checked by |
|---|---|---|
| INV-ID-1 | Every live node has a unique `NodeId`, stable across rename, move and unlink | checker |
| INV-ID-2 | Every live node has a unique `index_number`, never reused within a process lifetime | checker |
| INV-ID-3 | A freed or stale `HandleId`/`CursorId` never resolves; after slot reuse the old value fails the generation check and does not reach the new object | conformance + fuzz |
| INV-ID-4 | Every live `HandleId` is unique and refers to exactly one live node | checker |
| INV-ID-5 | No identifier's value derives from a memory address; `0` is never a valid `HandleId` or `CursorId` | code rule + conformance |

INV-ID-3 is the strongest safety property in Phase 1 and the entire justification for ADR-0008.

### Namespace — INV-NS

| ID | Invariant | Checked by |
|---|---|---|
| INV-NS-1 | The root exists, is a directory, and has no parent | checker |
| INV-NS-2 | Every live **linked** non-root node has exactly one parent and appears exactly once in that parent's child map. Unlinked nodes (§3.3.2) are exempt | checker |
| INV-NS-3 | The parent chain from any linked node reaches the root in at most L3 steps — no cycles | checker |
| INV-NS-4 | No directory is its own ancestor | checker |
| INV-NS-5 | No two children of one directory have colliding folded names | checker |
| INV-NS-6 | No operation resolves to, reads, or writes anything outside the SPACE namespace | conformance (§7.2) + ProcMon (§16.4) |

INV-NS-6 has two halves deliberately: a logical assertion proving the resolver, and external evidence proving the process.

### File state — INV-FS

| ID | Invariant | Checked by |
|---|---|---|
| INV-FS-1 | `file_size <= allocation_size`, and `allocation_size` is a multiple of the allocation unit | checker |
| INV-FS-2 | A node's `open_count` equals the number of live handles referencing it; it never underflows | checker |
| INV-FS-3 | A closed handle performs no I/O and no metadata mutation; every attempt is `InvalidHandle` | conformance |
| INV-FS-4 | An unlinked node remains usable through existing handles, is unreachable by path, and is reclaimed exactly at the `Close` that brings `open_count` to zero | checker + conformance |
| INV-FS-5 | Every state transition outside §3.3.1 is rejected and leaves state unchanged. A panic is not a transition — it poisons (ADR-0013a) | conformance |

### Enumeration — INV-DIR

Duplicate-entry and self-containment live in INV-NS-5 and INV-NS-4 and are not restated.

| ID | Invariant | Checked by |
|---|---|---|
| INV-DIR-1 | An enumeration of a directory with N children terminates within N + 2 entries for any buffer size | conformance + property |
| INV-DIR-2 | Marker-chained enumeration returns each entry exactly once — no duplicates, no omissions — across any sequence of buffer sizes | property (§14.2) |
| INV-DIR-3 | A closed or never-allocated `CursorId` resolves to a controlled error, never to another directory's state | conformance + fuzz |

INV-DIR-2 is the executable form of the marker bug that silently truncates large directories.

### Resources — INV-RES

| ID | Invariant | Checked by |
|---|---|---|
| INV-RES-1 | For every limit in §3.4, `used <= limit` at all times | checker |
| INV-RES-2 | Exceeding a limit produces the §3.4 error and never a panic, an abort, or an unbounded allocation | conformance + fuzz |
| INV-RES-3 | An operation that fails on a limit leaves state exactly as before — no partial write, no orphaned handle, no half-created node | conformance |

INV-RES-3 matters more than the error code. A limit that fails *and* corrupts is worse than no limit.

### Considered and excluded

- *"Malformed input cannot corrupt internal state"* — that is INV-ID/NS/FS holding after fuzzed input, which is exactly how §14.1 asserts it. A separate rule would create two definitions of one requirement.
- *"Handle counts never negative"* — folded into INV-FS-2 (unsigned type, checked decrement).
- Content integrity, checksums, durability — **Phases 3 and 5.** Phase 1 has no authoritative second copy, so such an invariant would be unfalsifiable here.

### Mechanism

```rust
pub struct InvariantViolation { pub id: &'static str, pub detail: String }
```

- Compiled under the existing `fault-injection` feature (no second flag).
- Called by the conformance harness after **every** operation (§11.6) and by the fuzz harness after every operation in a sequence (§14.1).
- Never called on the Windows-facing path in a release build: it is O(nodes) and would make a 65,536-entry directory unusable.
- Every violation reports its invariant ID, so a failure names the rule.
- On violation **in a debug build**, the filesystem enters POISONING (ADR-0013a). It does not attempt repair.

## 3.6 Concurrency model — `docs/protocols/concurrency.md`

Phase 1 is deliberately simple, and the simplicity is documented rather than discovered.

**Two layers of serialization:**

1. **WinFsp's operation guard**, set to `FSP_FILE_SYSTEM_OPERATION_GUARD_STRATEGY_COARSE` (§4.1): one lock for the whole filesystem, taken exclusively by mutating operations and shared by reading ones.
2. **The VFS's own lock:** `MemVfs` holds all state behind a single `parking_lot::Mutex`.

**The effective model: exactly one VFS operation executes at a time.** Layer 2 dominates. Layer 1 is retained because it is the strategy Phase 10 will tune, and changing it later should not be the first time it is exercised.

| Question | Phase 1 answer |
|---|---|
| Which operations run concurrently? | None, inside the VFS. Multiple dispatcher threads may be *in flight*, but at most one holds the state lock. |
| Which serialize? | All of them. |
| What happens during an injected hang? | The hung operation holds the state lock. Every other operation blocks on lock acquisition **until its own deadline expires**, then returns `OperationTimeout`. Nothing hangs indefinitely; nothing succeeds while the lock is held. |
| Does one hung callback block unrelated operations? | **Yes**, and this is the documented Phase 1 behaviour, bounded by L9. Phase 10 owns fixing it. §13.2 confirms it empirically rather than assuming it. |
| How does shutdown interact with in-flight work? | In-flight callbacks run to completion or deadline; new work is refused (ADR-0013a); the main thread then stops the dispatcher, which waits for the dispatcher threads to drain. |

**Implementation requirement:** lock acquisition must be deadline-bounded, or L9 is a lie.

```rust
fn state<'a>(&'a self, cx: &OpCtx) -> Result<MutexGuard<'a, MemVfsState>, SpaceError> {
    self.inner
        .try_lock_until(cx.deadline)
        .ok_or_else(|| SpaceError::new(ErrorCode::OperationTimeout, "state lock deadline exceeded"))
}
```

This is why `parking_lot` is used rather than `std::sync::Mutex` — `std` has no deadline-bounded acquisition. `parking_lot` also does not poison on panic, which is correct here: **poisoning is a single explicit mechanism** (the `LifeState` atomic, §2.3), not two overlapping ones.

## 3.7 Tests for this session

- `VfsPath::parse` accepts every valid form and rejects every item in §3.3.7 — one test per rule, named for the rule
- every limit in §3.4 at limit and limit+1 (detail in §14.3)
- `OpCtx::check` yields `OperationTimeout` past the deadline
- `check_invariants()` on a fresh empty filesystem passes
- **the checker fires:** construct a structure violating each of INV-NS-1, NS-2, NS-3, NS-5, ID-2, ID-4, FS-1, FS-2, RES-1 and assert the correct ID is reported. *A checker that never fires is not a checker.*
- the checker is read-only: run it against a fixture, snapshot state before and after, assert byte-identical
- `VFS_CONTRACT_VERSION == 1`

```powershell
git commit -am "M1.2: identity model, Vfs trait, semantics, limits, invariants, concurrency model"; git tag M1.2
```

---

# SESSION 4 — M1.3: The WinFsp host

**Time: 1–2 days. Target: `S:` appears in Explorer and unmounts cleanly.**

## 4.1 `client/winfsp-adapter/src/host.cpp`

```cpp
#include <winfsp/winfsp.h>
#include "space_core.h"

static FSP_FILE_SYSTEM *g_FileSystem = nullptr;
extern FSP_FILE_SYSTEM_INTERFACE g_SpaceInterface;

extern "C" NTSTATUS space_adapter_mount(const wchar_t *MountPoint, UINT32 DispatcherThreads)
{
    NTSTATUS Result = FspLoad(nullptr);      // locate winfsp-x64.dll via the registry
    if (!NT_SUCCESS(Result)) return Result;

    FSP_FSCTL_VOLUME_PARAMS VolumeParams = {};
    VolumeParams.SectorSize                  = 4096;
    VolumeParams.SectorsPerAllocationUnit    = 1;
    VolumeParams.VolumeSerialNumber          = 0x53504143;   // 'SPAC'
    VolumeParams.FileInfoTimeout             = 0;   // see note 1
    VolumeParams.CaseSensitiveSearch         = 0;
    VolumeParams.CasePreservedNames          = 1;
    VolumeParams.UnicodeOnDisk               = 1;
    VolumeParams.PersistentAcls              = 1;
    VolumeParams.PostCleanupWhenModifiedOnly = 0;   // see note 2
    VolumeParams.FlushAndPurgeOnCleanup      = 1;   // see note 3
    wcscpy_s(VolumeParams.FileSystemName,
             sizeof VolumeParams.FileSystemName / sizeof(WCHAR), L"SPACE");

    Result = FspFileSystemCreate(const_cast<PWSTR>(L"" FSP_FSCTL_DISK_DEVICE_NAME),
                                 &VolumeParams, &g_SpaceInterface, &g_FileSystem);
    if (!NT_SUCCESS(Result)) return Result;

    FspFileSystemSetOperationGuardStrategy(
        g_FileSystem, FSP_FILE_SYSTEM_OPERATION_GUARD_STRATEGY_COARSE);   // §3.6

    Result = FspFileSystemSetMountPoint(g_FileSystem, const_cast<PWSTR>(MountPoint));
    if (!NT_SUCCESS(Result)) { FspFileSystemDelete(g_FileSystem); g_FileSystem = nullptr; return Result; }

    Result = FspFileSystemStartDispatcher(g_FileSystem, DispatcherThreads);   // ADR-0010
    if (!NT_SUCCESS(Result)) {
        FspFileSystemRemoveMountPoint(g_FileSystem);
        FspFileSystemDelete(g_FileSystem);
        g_FileSystem = nullptr;
        return Result;
    }
    return STATUS_SUCCESS;
}

/* MUST be called only from the main thread — see ADR-0013a. */
extern "C" void space_adapter_unmount(void)
{
    if (!g_FileSystem) return;
    FspFileSystemStopDispatcher(g_FileSystem);
    FspFileSystemRemoveMountPoint(g_FileSystem);
    FspFileSystemDelete(g_FileSystem);
    g_FileSystem = nullptr;
}
```

**Volume parameter notes — each chosen for Phase 1 determinism, not performance:**

1. **`FileInfoTimeout = 0`** — no kernel-side caching of file information. Every query reaches the VFS, so a test that writes and immediately stats sees the new size. A non-zero timeout makes size and timestamp assertions intermittently stale, and intermittent test failures in Phase 1 are far more expensive than the extra round trips.
2. **`PostCleanupWhenModifiedOnly = 0`** — `Cleanup` is posted for every file object. With `1`, cleanup is suppressed for unmodified opens, which makes cleanup useless for per-open bookkeeping and produces the "why didn't cleanup fire" confusion described in §3.3.1. Phase 1 wants every callback it can observe.
3. **`FlushAndPurgeOnCleanup = 1`** — the cache manager flushes and purges at cleanup, so data written through one handle is visible to a subsequent open without depending on cache timing.

**Unmount ordering is load-bearing:** stop dispatcher → remove mount point → delete. Removing the mount point while the dispatcher still serves requests is how a drive letter lingers after process exit.

**`space_adapter_unmount` must never be called from a dispatcher thread.** `FspFileSystemStopDispatcher` waits for dispatcher threads to drain, including the caller — a self-deadlock. The poison path signals the main thread (§2.3) precisely to avoid this.

## 4.2 Minimal callback table

`callbacks.cpp` begins with `GetVolumeInfo` only; every other slot is `nullptr`, which WinFsp treats as unimplemented.

```cpp
static NTSTATUS GetVolumeInfo(FSP_FILE_SYSTEM *, FSP_FSCTL_VOLUME_INFO *VolumeInfo)
{
    space_volume_info vi{};
    space_status s = space_core_get_volume_info(&vi);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    VolumeInfo->TotalSize = vi.total_size;
    VolumeInfo->FreeSize  = vi.free_size;
    VolumeInfo->VolumeLabelLength = 0;
    return STATUS_SUCCESS;
}

FSP_FILE_SYSTEM_INTERFACE g_SpaceInterface = { .GetVolumeInfo = GetVolumeInfo };
```

## 4.3 Wire into `client/main`

Startup: parse args → load and validate config → init logging → `space_core_start(config_path)` → `space_adapter_mount(L"S:", cfg.client.dispatcher_threads)` → block on a shutdown channel.

Shutdown, **on the main thread only**, triggered by Ctrl-C or by the poison signal: set state to STOPPING → `space_adapter_unmount()` → `space_core_stop()` → set UNMOUNTED → exit.

## 4.4 ✅ Verify

```powershell
cargo build
.\target\debug\space-client.exe --config .\config.toml
```

```powershell
Get-PSDrive S
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" lsvol
```

Explorer → This PC → `S:` appears, empty and mostly unusable. Correct for this session. Ctrl-C, then `.\scripts\os-safety-check.ps1` must print **OS safety OK**.

```powershell
git commit -am "M1.3: WinFsp host, deterministic volume params, mount/unmount lifecycle"; git tag M1.3
```

---

# SESSION 5 — M1.4: MemVfs, handle manager, invariant checker

**Time: 2 days.** The first `impl Vfs`. Phase 2 writes the second.

## 5.1 The model

```rust
pub struct MemNode {
    pub name: String,                              // display name, case preserved
    pub is_dir: bool,
    pub attributes: u32,
    pub data: Vec<u8>,
    pub children: BTreeMap<FoldedName, NodeId>,    // stable sorted enumeration, free
    pub parent: Option<NodeId>,
    pub created: u64, pub accessed: u64, pub written: u64, pub changed: u64,
    pub index_number: u64,                         // monotonic, never reused (§3.1)
    pub open_count: u32,
    pub unlinked: bool,                            // §3.3.2
}

pub struct MemVfsState {
    nodes: SlotMap<NodeId, MemNode>,
    root: NodeId,
    handles: HandleTable,
    cursors: CursorTable,
    total_bytes: u64,
    next_index_number: u64,
}

pub struct MemVfs { inner: parking_lot::Mutex<MemVfsState> }   // §3.6
```

`BTreeMap` gives the total, stable enumeration order §3.3.8 requires. `FoldedName` is the comparison key; the display name lives on the node (INV-NS-5, §3.3.7).

`unlinked` rather than `pending_delete`: it names the state, not an intent. Phase 2 will read this field.

## 5.2 Handle and cursor tables

```rust
pub trait HandleManager: Send + Sync {
    fn alloc(&mut self, node: NodeId, granted_access: u32, is_dir: bool) -> Result<HandleId, SpaceError>;
    fn resolve(&self, h: HandleId) -> Result<&HandleSlot, SpaceError>;
    fn free(&mut self, h: HandleId) -> Result<HandleSlot, SpaceError>;
    fn live_count(&self) -> usize;
}
```

`resolve` checks, in order: index within range → slot live → generation matches. Any mismatch is `InvalidHandle` — never a panic, never a dereference (INV-ID-3). `alloc` enforces L7; the cursor table enforces L8 identically.

## 5.3 The invariant checker

Implement `VfsDiagnostics for MemVfs` **now**, with the model, not later. One read-only tree walk.

```rust
impl VfsDiagnostics for MemVfs {
    fn check_invariants(&self) -> Result<(), InvariantViolation> {
        let s = self.inner.lock();          // diagnostic path: unbounded wait is acceptable
        let mut index_numbers = HashSet::new();
        let mut counted: HashMap<NodeId, u32> = HashMap::new();

        // INV-NS-1
        let root = s.nodes.get(s.root).ok_or_else(|| viol("INV-NS-1", "root missing"))?;
        if root.parent.is_some() || !root.is_dir {
            return Err(viol("INV-NS-1", "root malformed"));
        }

        for (id, node) in s.nodes.iter() {
            // INV-ID-2
            if !index_numbers.insert(node.index_number) {
                return Err(viol("INV-ID-2", format!("duplicate index_number {}", node.index_number)));
            }
            // INV-FS-1
            if node.data.len() as u64 > alloc_size(node) {
                return Err(viol("INV-FS-1", "file_size > allocation_size"));
            }
            // INV-NS-2 — unlinked nodes are exempt by §3.3.2. Deliberate, not an oversight.
            if id != s.root && !node.unlinked {
                let p = node.parent.ok_or_else(|| viol("INV-NS-2", "linked non-root without parent"))?;
                let parent = s.nodes.get(p).ok_or_else(|| viol("INV-NS-2", "dangling parent"))?;
                if parent.children.values().filter(|c| **c == id).count() != 1 {
                    return Err(viol("INV-NS-2", "node not exactly once in parent"));
                }
            }
            // INV-NS-3 / INV-NS-4
            let (mut cur, mut steps) = (node.parent, 0usize);
            while let Some(p) = cur {
                if p == id { return Err(viol("INV-NS-4", "node is its own ancestor")); }
                steps += 1;
                if steps > limits::MAX_PATH_DEPTH {
                    return Err(viol("INV-NS-3", "parent cycle or excessive depth"));
                }
                cur = s.nodes.get(p).and_then(|n| n.parent);
            }
            // INV-NS-5 — BTreeMap keys are unique by construction; assert agreement anyway
            if node.is_dir && node.children.len() > limits::MAX_DIR_ENTRIES {
                return Err(viol("INV-RES-1", "directory entry limit exceeded"));
            }
        }

        // INV-ID-4 / INV-FS-2
        for slot in s.handles.live_slots() {
            if !s.nodes.contains_key(slot.node) {
                return Err(viol("INV-ID-4", "handle references dead node"));
            }
            *counted.entry(slot.node).or_default() += 1;
        }
        for (id, n) in counted {
            if s.nodes[id].open_count != n {
                return Err(viol("INV-FS-2", "open_count disagrees with live handles"));
            }
        }
        // INV-FS-4: an unlinked node with open_count == 0 should already be reclaimed
        for (_, node) in s.nodes.iter() {
            if node.unlinked && node.open_count == 0 {
                return Err(viol("INV-FS-4", "unlinked node with no handles was not reclaimed"));
            }
        }
        // INV-RES-1
        if s.total_bytes > s.max_bytes { return Err(viol("INV-RES-1", "used exceeds max_bytes")); }
        if s.handles.live_count() > limits::MAX_OPEN_HANDLES {
            return Err(viol("INV-RES-1", "handle limit exceeded"));
        }
        if s.cursors.live_count() > limits::MAX_OPEN_CURSORS {
            return Err(viol("INV-RES-1", "cursor limit exceeded"));
        }
        Ok(())
    }
}
```

No branch of this function mutates. That is the §3.5 read-only rule, and §3.7 tests it by snapshot comparison.

## 5.4 Tests

| Test | Expected |
|---|---|
| resolve never-allocated handle | `InvalidHandle` |
| resolve handle `0` | `InvalidHandle` |
| resolve after free | `InvalidHandle` |
| free twice | second → `InvalidHandle`, no panic |
| index out of range (`u32::MAX`) | `InvalidHandle` |
| generation mismatch after slot reuse | `InvalidHandle`, and **does not reach the new object** (INV-ID-3) |
| 10,000 alloc/free cycles | indices reused, generations advance, `live_count` returns to zero |
| generation wraparound at `u32::MAX` | documented; generation 0 skipped on wrap; no aliasing |
| `index_number` after 10,000 create/delete cycles | strictly increasing, never reused (INV-ID-2) |
| exceed L7 / L8 | `ResourceExhausted`, no partial state (INV-RES-2, INV-RES-3) |
| `check_invariants` after each of the above | passes |

The wraparound case is contrived; write it anyway. The fix is one line, and finding it later means finding it through a corruption report.

```powershell
git commit -am "M1.4: MemVfs, generational handle/cursor tables, read-only invariant checker"; git tag M1.4
```

---

# SESSION 6 — M1.5: Volume info, security, file info

**Time: 1 day.**

## 6.1 The default security descriptor

One descriptor for everything in Phase 1:

```
O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;WD)
```

Full access for SYSTEM, Administrators, Everyone. Convert once at startup and hold the bytes in Rust; keeping it out of C++ keeps the adapter thin.

## 6.2 `probe` / `GetSecurityByName` — the callback with the unusual contract

WinFsp calls this before create/open to check existence and permissions. Three parts are routinely got wrong:

1. **File does not exist → `STATUS_OBJECT_NAME_NOT_FOUND`.** This is normal, not an error worth logging at error level (§12.2). `Create` depends on it.
2. **`PFileAttributes` may be `NULL`.** Check before writing.
3. **Buffer protocol.** If `SecurityDescriptor` is `NULL`, the caller wants only the size: write it and return success. If the buffer is too small, write the required size and return **`STATUS_BUFFER_OVERFLOW`**.

Getting (3) wrong produces an `S:` that Explorer can see but not open, with no useful error anywhere.

## 6.3 File info and time

```rust
pub fn now_filetime() -> u64 {
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;   // 1601 → 1970
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap();
    EPOCH_DIFF_100NS + d.as_secs() * 10_000_000 + (d.subsec_nanos() as u64) / 100
}
```

`index_number` comes from the monotonic counter (§3.1, INV-ID-2). `allocation_size` is `file_size` rounded up to 4096 (INV-FS-1); reporting it below `file_size` causes oddities in size columns and in some applications' EOF handling.

## 6.4 Tests

- volume info: `free_size <= total_size`
- default SD converts and round-trips
- probe on a missing path → `FileNotFound`
- probe with `sd_buf = NULL` → success, size written
- probe with a 1-byte buffer → `BufferOverflow`, required size written
- probe with `out_attrs = NULL` → success, no write through the null pointer
- FILETIME conversion against a known value (1970-01-01 → `116444736000000000`)
- `allocation_size` is a multiple of 4096 and `>= file_size` for sizes 0, 1, 4095, 4096, 4097

```powershell
git commit -am "M1.5: volume info, security descriptor, file info and time"; git tag M1.5
```

---

# SESSION 7 — M1.6: Create, Open, Cleanup, Close

**Time: 1–2 days.** Implements §3.3.1, §3.3.2 and §3.3.3.

## 7.1 Create options honoured

| Flag | Behaviour |
|---|---|
| `FILE_DIRECTORY_FILE` | target must be a directory; else `NotADirectory` |
| `FILE_NON_DIRECTORY_FILE` | target must not be a directory; else `FileIsADirectory` |
| `FILE_DELETE_ON_CLOSE` | WinFsp raises `FspCleanupDelete` at cleanup; the VFS unlinks then |

## 7.2 Tests

Every row of §3.3.1, §3.3.2 and §3.3.3 becomes a test named for the row. Additionally:

**Bookkeeping placement (the §3.3.1 correction):**
- open a file twice, close one handle → `open_count == 1`, node alive
- close the second → `open_count == 0`
- open, cleanup with delete, then close → unlinked at cleanup, reclaimed at close
- open twice, delete via one handle's cleanup, read through the other → succeeds; path lookup fails; reclaimed only at the second close
- create a new file at the deleted path while the old node is still open → succeeds, distinct `NodeId` and `index_number` (INV-ID-1, INV-ID-2)

**Invalid input** — each yields a specific status and satisfies INV-NS-6: empty path; no leading `\`; `..`; repeated separators; `:` in a component; reserved device names; 300-char component; 40,000-char path; embedded NUL; unpaired surrogate; trailing dot; trailing space.

The traversal case is a two-part assertion — the parse rejects, *and* the host filesystem is untouched:

```rust
#[test]
fn inv_ns_6_path_traversal_cannot_escape_namespace() {
    const HOSTS: &str = r"C:\Windows\System32\drivers\etc\hosts";
    let before = std::fs::metadata(HOSTS).unwrap().modified().unwrap();

    for evil in [
        r"\..\..\Windows\System32\drivers\etc\hosts",
        r"\a\..\..\..\Windows\win.ini",
        r"\\?\C:\Windows\win.ini",
        "\\a\\..\u{202E}\\..\\Windows",
    ] {
        assert!(VfsPath::parse(evil).is_err(), "accepted traversal: {evil}");
    }

    assert_eq!(std::fs::metadata(HOSTS).unwrap().modified().unwrap(), before);
    vfs.check_invariants().unwrap();
}
```

```powershell
git commit -am "M1.6: create/open/cleanup/close, corrected bookkeeping, path validation"; git tag M1.6
```

---

# SESSION 8 — M1.7: Read and Write

**Time: 1–2 days.** Implements §3.3.4. These boundary tests are what Phases 3 and 4 will depend on.

```rust
fn read(&self, cx: &OpCtx, h: HandleId, offset: u64, buf: &mut [u8]) -> Result<u32, SpaceError> {
    cx.check()?;                       // OperationTimeout (ADR-0014)
    let s = self.state(cx)?;           // deadline-bounded lock (§3.6)
    let node = s.node_of(h)?;          // InvalidHandle (INV-ID-3)
    if offset >= node.data.len() as u64 {
        return Err(SpaceError::new(ErrorCode::EndOfFile, "read at or past EOF"));
    }
    let end = offset.checked_add(buf.len() as u64)
        .ok_or_else(|| SpaceError::new(ErrorCode::InvalidParameter, "offset+length overflow"))?
        .min(node.data.len() as u64);
    let n = (end - offset) as usize;
    buf[..n].copy_from_slice(&node.data[offset as usize..end as usize]);
    Ok(n as u32)
}
```

`checked_add` is not optional. `offset = u64::MAX, length = 4096` arrives from the fuzzer within the hour.

## 8.1 Matrix

Run each against files of size 0, 1, 4095, 4096, 4097, 1 MiB:

| Case | Expected |
|---|---|
| read whole file / first byte / last byte | exact bytes |
| read length 0 | success, 0 transferred |
| read at `file_size` | `EndOfFile` |
| read at `file_size - 1`, length 4096 | success, 1 byte |
| read at `u64::MAX` | `EndOfFile` or `InvalidParameter`, no panic |
| `offset = u64::MAX - 1`, length 4096 | overflow caught → `InvalidParameter` |
| length above L4 | `InvalidParameter`, no allocation |
| write at 0 to an empty file | grows |
| write past EOF | gap zero-filled |
| write length 0 | success, no change |
| `WriteToEndOfFile` | appends regardless of offset |
| `ConstrainedIo` past EOF | 0 transferred, success, unchanged |
| `ConstrainedIo` crossing EOF | truncated to existing size |
| write exceeding L5 | `DiskFull`, file byte-identical (INV-RES-3) |
| read after write | exactly what was written |

The last one is a **property test** over random offsets, lengths and content, with the seed recorded. It is the cheapest real integrity check available in Phase 1, and the ancestor of what Phase 3 will do with content hashing.

```powershell
git commit -am "M1.7: read/write, EOF, append, constrained IO, overflow validation"; git tag M1.7
```

---

# SESSION 9 — M1.8: Directory enumeration and Explorer

**Time: 1–2 days.** At the end of this session `S:` becomes browsable.

**Layering:** §3.3.8 is the semantic contract and lives in the VFS. Everything below is WinFsp protocol mechanics and lives in the adapter. The Rust core knows nothing about `FSP_FSCTL_DIR_INFO`, buffer packing, or the end-of-enumeration signal.

## 9.1 The adapter

```cpp
static NTSTATUS ReadDirectory(FSP_FILE_SYSTEM *FileSystem, PVOID FileContext,
    PWSTR Pattern, PWSTR Marker, PVOID Buffer, ULONG Length, PULONG PBytesTransferred)
{
    space_cursor cursor = SPACE_INVALID_CURSOR;
    space_status s = space_core_dir_open((space_handle)(uintptr_t)FileContext,
                                         Pattern, Marker, &cursor);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;

    union {
        FSP_FSCTL_DIR_INFO D;
        UINT8 B[sizeof(FSP_FSCTL_DIR_INFO) + MAX_PATH * sizeof(WCHAR)];
    } Entry;
    space_dir_entry e{};
    uint8_t more = 1;
    NTSTATUS result = STATUS_SUCCESS;

    while (more) {
        s = space_core_dir_next(cursor, &e, &more);
        if (s != STATUS_SUCCESS) { result = (NTSTATUS)s; goto done; }
        if (!more) break;

        size_t nameLen = wcslen(e.name);
        memset(&Entry.D, 0, sizeof Entry.D);
        Entry.D.Size = (UINT16)(sizeof(FSP_FSCTL_DIR_INFO) + nameLen * sizeof(WCHAR));
        CopyFileInfo(&Entry.D.FileInfo, &e.info);
        memcpy(Entry.D.FileNameBuf, e.name, nameLen * sizeof(WCHAR));

        // Buffer full: stop and report success. WinFsp will call again with
        // Marker set to the last name we returned. Returning an error here
        // silently truncates large directories.
        if (!FspFileSystemAddDirInfo(&Entry.D, Buffer, Length, PBytesTransferred))
            goto done;
    }

    // End-of-enumeration marker. Without it Explorer shows a directory that
    // never finishes loading.
    FspFileSystemAddDirInfo(nullptr, Buffer, Length, PBytesTransferred);

done:
    space_core_dir_close(cursor);      // cursor lifetime is this call only (§3.3.8)
    return result;
}
```

Three protocol facts encoded above, each a common first-attempt bug:

- **Buffer-full is `STATUS_SUCCESS`,** not an error.
- **The `NULL` end marker is mandatory** and only sent when the enumeration truly reached the end.
- **`Marker` is a resume point, not a filter.** The VFS resumes strictly after it, in the §3.3.8 order.

**Deliberately not used:** WinFsp's directory-buffer helpers (`FspFileSystemAcquireDirectoryBuffer` / `FillDirectoryBuffer` / `ReadDirectoryBuffer`). They snapshot the whole directory into adapter-owned state, which would push per-file-object state into C++ and violate the thin-adapter rule (ADR-0007's spirit). Marker-based resume keeps all state in the VFS. Recorded as a choice.

## 9.2 ✅ Explorer evidence

Manual, and part of the gate. Mount, then:

1. Explorer → `S:` opens and shows the root
2. Right-click → New → Folder
3. Create a text file; open in Notepad; type; save; close; reopen — content persists
4. Copy a 10 MB file into `S:`
5. Copy it back out under a new name; `Get-FileHash` both and compare
6. Rename a file; rename a folder
7. Delete a file; delete a folder
8. **Create 5,000 files in one directory; `Get-ChildItem S:\many | Measure-Object` returns exactly 5,000**
9. Create a 10-level nested path and browse to the bottom
10. Right-click → Properties — size and timestamps sane

Evidence to `docs/evidence/phase-1/explorer/`. Step 8 is the manual counterpart of INV-DIR-2; correct at 50 entries and truncated at 5,000 is the classic marker bug.

```powershell
git commit -am "M1.8: directory enumeration, marker resume, Explorer browsable"; git tag M1.8
```

---

# SESSION 10 — M1.9: Rename, delete, metadata mutation

**Time: 1–2 days.** Implements §3.3.5 and §3.3.6.

`can_delete` is a pure query: `DirectoryNotEmpty` for a non-empty directory, success otherwise. Removal happens at `Cleanup` with `FspCleanupDelete` (§3.3.1).

`set_file_size` with `set_allocation = true` adjusts allocation only; with `false` it sets the actual size — truncation discards, extension zero-fills. Both preserve INV-FS-1.

## 10.1 Tests

Every row of §3.3.5 and §3.3.6, plus:

- open handle survives rename; reads through it return correct bytes (INV-ID-1)
- rename a directory into its own subtree → `InvalidParameter`; `check_invariants` passes afterwards (INV-NS-4)
- rename an unlinked node → `InvalidParameter`
- delete while open → handle works; path lookup fails; reclaimed at last close (INV-FS-4)
- close after delete → no error, no leak; `check_invariants` passes
- truncate to 0, to smaller, extend with zero-fill; verify content at boundaries
- `set_basic_info` with `0` timestamps leaves them unchanged
- exceed L6 by renaming a file into a full directory → `ResourceExhausted`, source unchanged (INV-RES-3)
- share access: exclusive open, then a second open → `STATUS_SHARING_VIOLATION` (verifying ADR-0012's layering, not our code)

```powershell
git commit -am "M1.9: rename, delete, size and metadata mutation"; git tag M1.9
```

---

# SESSION 11 — M1.10: The VFS conformance suite

**Time: 2–3 days. The highest-leverage session in Phase 1.**

```
                Vfs contract (§3.2, §3.3, §3.4, §3.5, §3.6)
                            │
                 ┌──────────┴──────────┐
              MemVfs                Real VFS
             (Phase 1)              (Phase 2)
                 └──────────┬──────────┘
                     same conformance suite
```

## 11.1 Shape

```rust
pub fn run_conformance_suite<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    lifecycle::all(vfs, caps, limits);       // §3.3.1, §3.3.2
    create_open::all(vfs, caps, limits);     // §3.3.3
    read_write::all(vfs, caps, limits);      // §3.3.4
    rename_delete::all(vfs, caps, limits);   // §3.3.5
    metadata::all(vfs, caps, limits);        // §3.3.6
    naming::all(vfs, caps, limits);          // §3.3.7
    directory::all(vfs, caps, limits);       // §3.3.8
    handles::all(vfs, caps, limits);         // §3.1
    boundaries::all(vfs, caps, limits);      // §3.4
    invariants::all(vfs, caps, limits);      // §3.5
    errors::all(vfs, caps, limits);          // ADR-0014
}
```

## 11.2 The property that makes it valuable

**The suite runs with no WinFsp installed and no mount.** Consequently it runs on any CI runner, in parallel, in seconds; a failure points at your logic rather than a drive letter, Explorer, or the kernel; and roughly 80% of Phase 1's tests leave the mount path.

Enforce it: the conformance module imports nothing from `winfsp-adapter`, and CI runs it as a separate job on a runner without WinFsp. If that job ever needs WinFsp, something leaked.

## 11.3 Restructure earlier tests

Sessions 5–10 wrote tests against `MemVfs` directly. Move every test expressing a *semantic rule* or an *invariant* into the suite. Keep only implementation-specific tests in `MemVfs`'s own module: slab reuse, generation wraparound, `BTreeMap` ordering, and the checker's own behaviour.

The dividing question: *would Phase 2's VFS also have to pass this?* If yes, it belongs in conformance.

## 11.4 Lifecycle as tests

Encode §3.3.1 directly:

```
New ──create/open──▶ Open ──io──▶ Active ──cleanup──▶ Cleaned ──close──▶ Closed
```

For each state, assert legal operations succeed and every illegal one returns the specified error (INV-FS-3, INV-FS-5). The valuable cases are the illegal transitions: I/O after cleanup, I/O after close, double close, close of a never-allocated handle.

## 11.5 Capabilities — a fork in the contract, never an escape hatch

1. **A capability selects which documented correct behaviour is expected. It never decides whether a rule is checked.** Every capability-dependent test runs in both configurations and asserts that configuration's specified behaviour.
2. **Both variants must be written into `fs-semantics.md`.** If you cannot write both, you do not have a capability — you have a gap.
3. **Capabilities are part of the versioned contract** (ADR-0011). Adding one requires a version bump and an ADR.
4. **The default is that no capability is needed.**

Phase 1 ships exactly one:

```rust
#[derive(Clone, Copy)]
pub struct Capabilities {
    /// false = ASCII folding (Phase 1); true = Unicode simple case folding (Phase 2 target).
    /// Both behaviours specified in fs-semantics.md §3.3.7; both tested.
    pub unicode_case_folding: bool,
}

/// NOT capabilities — configured limits the suite needs in order to test boundaries.
#[derive(Clone, Copy)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_open_handles: usize,
    pub max_open_cursors: usize,
    pub max_dir_entries: usize,
    pub max_io_bytes: usize,
}
```

```rust
#[test]
fn capability_count_is_deliberate() {
    // Adding a capability weakens contract uniformity. If this fails, bump
    // VFS_CONTRACT_VERSION and write an ADR — do not just edit the number.
    assert_eq!(std::mem::size_of::<Capabilities>(), 1);
}
```

## 11.6 Per-step invariant checking

```rust
fn step<V: Vfs + VfsDiagnostics, T>(vfs: &V, label: &str, f: impl FnOnce() -> T) -> T {
    let r = f();
    if let Err(v) = vfs.check_invariants() {
        panic!("invariant {} violated after {}: {}", v.id, label, v.detail);
    }
    r
}
```

Every conformance test becomes an invariant test at no authoring cost. Keep fixtures small so the O(nodes) walk stays cheap; the L6-boundary test checks once at the end rather than per step.

## 11.7 Error-model assertions

- every callback-reachable `ErrorCode` is produced by at least one conformance test
- **no conformance test ever observes `NetworkTimeout`** (ADR-0014)
- an expired deadline yields `OperationTimeout`
- no VFS method returns a Windows type; the suite compiles without any Windows crate (ADR-0015)

```powershell
cargo nextest run -p space-client-core --lib conformance
git commit -am "M1.10: conformance suite, capability discipline, per-step invariant checking"; git tag M1.10
```

---

# SESSION 12 — M1.11: Request tracing and NTSTATUS translation

**Time: 1 day.**

## 12.1 One request ID per callback

`RequestId` is minted in `guard()` (§2.3) and carried through `OpCtx` into every VFS call and every log line.

```json
{"ts":"2026-09-07T10:22:03.117Z","level":"debug","component":"winfsp.read",
 "request_id":"r_0192...","operation":"read","path":"\\dir\\file.txt",
 "handle":"h_4294967297","offset":4096,"length":4096,
 "duration_ms":0,"result":"ok","error_code":null,"msg":"read served"}
```

Trace path: **WinFsp callback → C++ adapter (no logging) → FFI guard (`request_id` minted) → VFS → result.** The adapter is deliberately silent — it makes no decisions, so it has nothing to say. §3.1 records this so nobody later looks for adapter log lines that were never designed to exist.

## 12.2 Level discipline

Explorer generates hundreds of callbacks per second while merely displaying a directory.

- `trace` — callback entry/exit
- `debug` — operations with parameters
- `info` — mount, unmount, startup, shutdown only
- `warn` — recoverable anomalies
- `error` — a bug in SPACE; a panic (ADR-0013) always logs here

`FileNotFound` from `probe` is **not** an error — it is the normal existence check. Logging it at error level buries real problems.

## 12.3 Never log content

- No file contents, no raw buffers, no binary blobs — ever.
- Test: read a file containing a known marker string, assert the marker appears nowhere in the log output.
- **Paths are logged in Phase 1** because all data is synthetic and paths are the most useful diagnostic available. Mark this in `docs/protocols/logging.md` as **revisit before real customer data (Phase 9)**.

## 12.4 The four-column translation matrix

For every callback-reachable `ErrorCode`, drive an operation that produces it and assert the whole chain:

**injected condition → SPACE error code → NTSTATUS → the Win32 error Windows reports**

```powershell
try { Get-Content S:\does-not-exist } catch { $_.Exception.HResult.ToString("X") }
```

Table-driven, one row per code, all four columns asserted. Include `OperationTimeout` → `STATUS_IO_TIMEOUT` and panic → `STATUS_INTERNAL_ERROR`. The mapping lives in `client/core/src/ffi/ntstatus.rs` (ADR-0015), and the exhaustiveness test over `ErrorCode::ALL` lives beside it.

```powershell
git commit -am "M1.11: request tracing, log discipline, NTSTATUS translation at the boundary"; git tag M1.11
```

---

# SESSION 13 — M1.12: Deadlines, cancellation, fault injection

**Time: 1–2 days. The hardest exit criterion in Phase 1.**

## 13.1 The Phase 1 fault set

```rust
pub enum FaultAction {
    None,
    Fail(ErrorCode),
    Delay(Duration),
    Hang,
    Cancel,
    InvalidInput,        // corrupt the operation's parameters
    StaleHandle,         // force a generation mismatch      → INV-ID-3
    ResourceExhausted,   // simulate hitting a §3.4 limit    → INV-RES-2/3
    Panic,               // → ADR-0013
}
```

**`CorruptBytes` is excluded from Phase 1.** Phase 1 has no integrity mechanism: content lives in a `Vec<u8>` in-process, with no checksum, no authoritative second copy, and no cache to reconstruct from. Flipping a byte would produce a test whose expected result is "the flipped byte comes back," which asserts nothing — and would sit in the exit gate looking like integrity coverage. It returns in **Phase 3**, injected between chunk-store read and content verification, expecting an integrity error.

```rust
#[test]
fn corrupt_bytes_is_not_armable_in_phase_1() {
    assert!(faults::arm("winfsp_pre_read", FaultAction::CorruptBytes).is_err());
}
```

Points registered in `docs/protocols/fault-points.md`:

```
winfsp_pre_read, winfsp_pre_write, winfsp_pre_open, winfsp_pre_create,
winfsp_pre_readdir, winfsp_pre_getinfo, winfsp_pre_rename, winfsp_pre_cleanup
```

## 13.2 What "bounded" must mean

With `winfsp_pre_read` armed to `Hang`:

1. The callback returns `STATUS_IO_TIMEOUT` within `callback_timeout_ms` + 500 ms
2. The issuing application receives a controlled error, not a hang
3. **Explorer stays responsive**
4. Other operations return `OperationTimeout` rather than hanging — the §3.6 model predicts they block on the state lock until their own deadlines expire
5. Unmount still succeeds
6. `check_invariants()` passes afterwards
7. `os-safety-check.ps1` passes

Point 4 is the point of §3.6: the behaviour is *predicted* by the documented model and then *confirmed* here. Record the measured numbers in the evidence folder. If the observed behaviour differs from §3.6, the document is wrong — fix the document, do not quietly accept the difference.

## 13.3 Procedure

```powershell
# terminal 1
cargo run --features fault-injection -- --config .\config.toml

# terminal 2
.\scripts\arm-fault.ps1 winfsp_pre_read Hang
Measure-Command { Get-Content S:\test.txt -ErrorAction SilentlyContinue }
# assert elapsed <= callback_timeout_ms + 500ms, and an error was returned
Measure-Command { Get-ChildItem S:\other\ -ErrorAction SilentlyContinue }
# assert also bounded (§3.6 point 4)
```

Repeat for write, open, readdir, getinfo, rename. Then set `callback_timeout_ms = 1000` in a test config and confirm the bound **scales with configuration** — proof that the deadline is real rather than an artefact of fast operations.

## 13.4 The near-miss

Arm `Delay(callback_timeout_ms - 200)`. The operation must **succeed**. A deadline that fires early is as much a defect as one that never fires, and it is the failure you would otherwise meet under Phase 4 network latency.

## 13.5 Invariant-targeting faults

| Fault | Target | Expected |
|---|---|---|
| `StaleHandle` | INV-ID-3 | `InvalidHandle`; no state change; `check_invariants` passes |
| `ResourceExhausted` | INV-RES-2/3 | `ResourceExhausted`; **state byte-identical to before**; `check_invariants` passes |
| `InvalidInput` | INV-NS-6, INV-RES-2 | a specific `ErrorCode`, never a panic; `check_invariants` passes |
| `Cancel` | ADR-0009 | `Cancelled` → `STATUS_CANCELLED`; no partial mutation |
| `Panic` | ADR-0013, ADR-0013a | `STATUS_INTERNAL_ERROR`; panic logged with `request_id`; state → POISONING → POISONED; subsequent callbacks fail immediately without acquiring the lock; `close`/`cleanup` become no-ops; main thread performs the unmount; `os-safety-check.ps1` clean |

Run the `Panic` row last in the session — it deliberately ends the mount. It is the end-to-end proof of ADR-0013 and ADR-0013a through a real mount, from Explorer's point of view, and it must pass in **both** debug and release builds.

```powershell
git commit -am "M1.12: fault set, deadlines, bounded cancellation, poison path proven"; git tag M1.12
```

---

# SESSION 14 — M1.13: Fuzzing, property tests, limits

**Time: 2 days.**

## 14.1 Fuzz the FFI boundary

The boundary's contract is: no malformed input produces a panic escaping the boundary, undefined behaviour, an unbounded hang, an invalid error code, or an invariant violation. Hand-written cases prove specific inputs safe; fuzzing looks for the ones you did not think of.

```powershell
cargo install cargo-fuzz --locked
cargo fuzz init
```

| Target | Input space | Asserts |
|---|---|---|
| `fuzz_path` | arbitrary `u16` sequences → `wstr` + `VfsPath::parse` | no panic; INV-NS-6 |
| `fuzz_read_args` | arbitrary handle, offset, length | no panic; INV-ID-3; L4 |
| `fuzz_write_args` | arbitrary handle, offset, buffer, mode flags | no panic; INV-RES-2 |
| `fuzz_handle` | arbitrary `u64` → `resolve` / `free` | INV-ID-3, INV-ID-5 |
| `fuzz_cursor` | arbitrary `u64` → `dir_next` / `dir_close` | INV-DIR-3 |
| `fuzz_op_sequence` | arbitrary sequences of create/open/read/write/rename/delete/cleanup/close/enumerate | **all invariants after every step** |

`fuzz_op_sequence` is the valuable one: it finds handle-lifecycle and namespace-state bugs that per-call fuzzing cannot. On failure, report the violated invariant ID *and* the operation sequence.

**Pass condition:** no panic escaping the boundary, no UB (run under AddressSanitizer where available), no hang beyond the deadline, every call returns a valid `ErrorCode`, no invariant violation. Run each target ≥30 minutes. Keep the corpus in `tests/fixtures/fuzz-corpus/`; every crash input becomes a permanent regression test.

**What fuzzing does not prove:** correctness. It proves the absence of the specific classes above over the inputs explored. Do not let the exit gate imply otherwise.

Note the interaction with ADR-0013: fuzz targets run without a mount and without the poison-and-unmount path, so a panic surfaces as a fuzz crash rather than a poisoned filesystem. That is intended — in fuzzing a panic is a finding, not a condition to handle.

## 14.2 Property tests

`proptest`, against the trait, no mount required:

| Property | Invariant |
|---|---|
| write → read returns the same bytes | — |
| write → size equals `max(old_size, offset + length)` unless `ConstrainedIo` | — |
| rename A→B→A leaves the tree identical | INV-NS-2, INV-ID-1 |
| after creating N files, enumeration with any buffer size returns exactly N entries, no duplicates, no omissions | **INV-DIR-2** |
| enumeration halts within N + 2 steps for any buffer size | **INV-DIR-1** |
| no two live handles share an ID; a freed ID never resolves | INV-ID-3, INV-ID-4 |
| `parse(display(parse(p))) == parse(p)` | — |
| any invalid input → no panic, no UB, no invariant violation | all |

Every failing case prints its seed (the M0.10 harness rule).

## 14.3 Limit tests

One test per row of §3.4, at the limit and at limit+1. Values are read from `Limits`; §3.4 is the single source of truth and is not restated here.

For every limit+1 case, assert in this order:

1. the specified error code (INV-RES-2)
2. **`state_before == state_after`** — file content, tree shape, handle count, cursor count and byte usage compared field by field (INV-RES-3)
3. `check_invariants()` passes (INV-RES-1)

Assertion 2 matters more than assertion 1. A limit that fails *and* corrupts is worse than no limit.

Phase 1 does not benchmark limits. L6 is 65,536 precisely so that boundary correctness can be tested in seconds; exhausting enormous limits is a Phase 11 activity.

```powershell
git commit -am "M1.13: fuzz targets, property laws, limit enforcement with state comparison"; git tag M1.13
```

---

# SESSION 15 — M1.14: Shutdown, poison path, kill-while-mounted

**Time: 1 day.**

## 15.1 Clean shutdown

Main thread only: STOPPING → stop dispatcher → remove mount point → delete filesystem → `space_core_stop()` → UNMOUNTED. Test with:

- no open handles
- 10 open handles
- an operation in flight
- an operation hung via a fault point (unmount must still complete; record how long and why)
- **a poisoned filesystem** (ADR-0013a): unmount completes, no callback succeeds after poisoning, `close`/`cleanup` are silent no-ops

## 15.2 Kill while mounted

```powershell
1..20 | ForEach-Object {
  $p = Start-Process -PassThru .\target\debug\space-client.exe -ArgumentList "--config .\config.toml"
  Start-Sleep -Seconds 3
  Start-Job { 1..500 | % { Set-Content "S:\stress-$_.txt" "data $_" } } | Out-Null
  Start-Sleep -Milliseconds 800
  Stop-Process -Id $p.Id -Force
  Start-Sleep -Seconds 2
  if (Test-Path "S:\") { throw "iteration $_ : S: not released" }
  Get-Job | Remove-Job -Force
}
.\scripts\os-safety-check.ps1
```

Assert: Windows continues normally; no BSOD; no system-wide hang; Explorer responsive (an error for `S:` is correct); `S:` disappears within seconds; no stale volume in `fsptool lsvol`; the client restarts and mounts immediately.

Run in three states: idle, mid-write, mid-enumeration of a 5,000-entry directory.

## 15.3 Stale-mount recovery

Write `docs/runbooks/stale-mount-recovery.md`: `fsptool lsvol`, checking for orphaned processes, and when a WinFsp service restart is warranted. You will want this at 11pm in Phase 5.

## 15.4 What the kill tests do and do not prove

**Prove:** Windows and the system recover safely when the user-mode filesystem process disappears — no bugcheck, no hang, no stale mount, no orphaned process, and a clean remount. This is the Class B recovery path from ADR-0013.

**Do not prove:** durable filesystem recovery. Phase 1 has no persistence by design; after a kill, all filesystem content is gone and that is the correct outcome. Crash-durability certification belongs to Phase 5 and must not be claimed here.

```powershell
git commit -am "M1.14: shutdown ordering, poison path shutdown, kill-while-mounted recovery"; git tag M1.14
```

---

# SESSION 16 — M1.15: Stress, compatibility, ProcMon, soak

**Time: 2 days including the soak.**

## 16.1 Mount/unmount stress

```powershell
1..200 | ForEach-Object {
  $p = Start-Process -PassThru .\target\debug\space-client.exe -ArgumentList "--config .\config.toml"
  Start-Sleep -Milliseconds 1500
  if (-not (Test-Path "S:\")) { throw "iteration $_ : mount failed" }
  Stop-Process -Id $p.Id -Force
  Start-Sleep -Milliseconds 700
  if (Test-Path "S:\") { throw "iteration $_ : unmount failed" }
}
.\scripts\os-safety-check.ps1
```

Alternate force-kill and graceful Ctrl-C across iterations; they exercise different paths.

## 16.2 I/O stress

Mounted, concurrently for 30 minutes: create/write/read/verify/delete across the §8.1 sizes; enumerate a 5,000-entry directory; rename between directories; `robocopy` a 500 MB tree in and out with hash comparison.

Run one 30-minute pass on a debug build with periodic `check_invariants()` (every 30 s), and the remainder on release for realistic resource figures.

## 16.3 Windows compatibility matrix

Record observed behaviour, not expected:

| Client | create | read | write | rename | delete | enumerate | properties | notes |
|---|---|---|---|---|---|---|---|---|
| Explorer | | | | | | | | |
| PowerShell | | | | | | | | |
| cmd.exe | | | | | | | | |
| Notepad | | | | | | | | |
| `copy` / `xcopy` | | | | | | | | |
| `robocopy` | | | | | | | | |
| 7-Zip or similar | | | | | | | | |

Capture the guest build (`Get-ComputerInfo | Select OsBuildNumber`) and the WinFsp version in the same file → `docs/evidence/phase-1/compatibility-matrix.md`.

## 16.4 No-arbitrary-writes evidence (INV-NS-6, external half)

Process Monitor, filter `Process Name is space-client.exe` **and** `Operation is WriteFile`. Run the full I/O stress. Assert the only write targets are `C:\SPACE\runtime\logs\*` — nothing under `C:\Windows`, nothing in the source tree, nothing under `C:\Users`. Export CSV → `docs/evidence/phase-1/procmon-writes.csv`.

## 16.5 Soak — leak detection, not performance

Four hours of mixed workload, mounted, release build. Sample every 5 minutes: RSS, handle count (Process Explorer), thread count, and SPACE's own live node / handle / cursor counts via the diagnostics endpoint.

Interpretation rules — decide these before you look at the graph:

| Signal | Verdict |
|---|---|
| RSS varies within ±10% of the 30-minute-mark value, no trend | **plateau — pass** |
| RSS linear-regression slope over samples after the 30-minute mark is **< 1 MB/hour** | pass |
| slope ≥ 1 MB/hour, monotonic across ≥ 6 consecutive samples | **suspicious growth — investigate before the gate** |
| OS handle count or thread count grows without bound | fail |
| SPACE live handle/cursor count does not return to baseline after a create/delete cycle | fail (INV-FS-2, INV-RES-1) |

The purpose is leak detection. Do not tune allocation, do not optimise the data structures, do not turn this into a performance project.

```powershell
git commit -am "M1.15: stress, compatibility matrix, ProcMon evidence, soak with thresholds"; git tag M1.15
```

---

# SESSION 17 — M1.16: Evidence and exit gate

## 17.1 `scripts/collect-evidence.ps1`

Answers "how do we know Phase 1 passed?" with artifacts, not opinions.

```powershell
param([string]$Phase = "phase-1")
$out = "docs\evidence\$Phase\$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -ItemType Directory -Force -Path $out | Out-Null

# environment + toolchain
Get-ComputerInfo | Select OsName,OsVersion,OsBuildNumber,CsTotalPhysicalMemory |
  ConvertTo-Json > "$out\environment.json"
& "C:\Program Files (x86)\WinFsp\bin\fsptool-x64.exe" ver > "$out\winfsp-version.txt"
rustc --version  > "$out\toolchain.txt"
cargo --version >> "$out\toolchain.txt"
cl 2>&1 | Select-Object -First 1 >> "$out\toolchain.txt"

# revision + decisions
git rev-parse HEAD > "$out\git-commit.txt"
git tag --points-at HEAD > "$out\git-tags.txt"
Get-ChildItem docs\decisions\*.md | Select-Object Name,LastWriteTime |
  Export-Csv "$out\adr-status.csv" -NoTypeInformation

# results
cargo nextest run --workspace --message-format json > "$out\test-results.json" 2>&1
cargo clippy --workspace --all-targets --message-format json > "$out\clippy.json" 2>&1
Copy-Item "fuzz\artifacts\*" "$out\fuzz\" -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item "docs\evidence\phase-1\soak-samples.csv" "$out\" -ErrorAction SilentlyContinue
Copy-Item "docs\evidence\phase-1\procmon-writes.csv" "$out\" -ErrorAction SilentlyContinue
Copy-Item "docs\evidence\phase-1\compatibility-matrix.md" "$out\" -ErrorAction SilentlyContinue

.\scripts\os-safety-check.ps1 > "$out\os-safety.txt" 2>&1

Copy-Item "C:\SPACE\runtime\logs\*" "$out\logs\" -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem $out -Recurse -File | Get-FileHash | Export-Csv "$out\hashes.csv" -NoTypeInformation

Write-Host "Evidence written to $out"
```

## 17.2 Windows safety gate — any single failure blocks Phase 1

- [ ] No BSOD or bugcheck across every run (Event Log + `MEMORY.DMP` clean)
- [ ] No system-wide hang
- [ ] No uncontrolled kernel failure attributable to SPACE or WinFsp
- [ ] No persistent stale mount after 200 mount/unmount cycles and 20 force-kills
- [ ] No orphaned `space-client` process
- [ ] No uncontrolled resource growth (§16.5 thresholds)
- [ ] Explorer never becomes permanently unresponsive, including during injected hangs
- [ ] Malformed filesystem requests never cause process-level instability (§14.1)
- [ ] ProcMon evidence: writes confined to `runtime\logs`

## 17.3 Contract gate

- [ ] `Vfs` trait defined; `VFS_CONTRACT_VERSION = 1`
- [ ] `docs/protocols/identity.md`, `fs-semantics.md`, `resource-limits.md`, `vfs-invariants.md`, `concurrency.md` complete
- [ ] Every semantic rule has a conformance test; every limit has limit and limit+1 tests; every invariant has a checker branch or a named test
- [ ] Conformance suite green against `MemVfs`
- [ ] **Conformance suite runs with no WinFsp installed** — separate CI job
- [ ] `Capabilities` has exactly one flag; both variants specified and tested; the deliberate-count test passes
- [ ] FFI struct layouts asserted on both sides
- [ ] No identifier derives from an address; `0` never valid (INV-ID-5)
- [ ] NTSTATUS mapping lives at the boundary, not in `contracts` (ADR-0015)
- [ ] ADR-0007 … ADR-0015 written and accepted

## 17.4 Invariant gate

- [ ] `check_invariants()` implemented, read-only, non-repairing, deterministic
- [ ] Proven to **fire**: a deliberately corrupted structure per invariant reports the correct ID (§3.7)
- [ ] Proven read-only: state byte-identical before and after (§3.7)
- [ ] Called after every conformance step and every fuzz operation
- [ ] Every invariant in §3.5 has a checker branch or a named test; none unclaimed
- [ ] No invariant violation in any test, fuzz, or debug stress run

## 17.5 Functional gate

- [ ] 200 mount/unmount cycles, graceful and forced
- [ ] Explorer opens `S:` and enumerates the root
- [ ] Create/open/read/write/close/delete from Explorer, PowerShell, Notepad
- [ ] 10 MB round-trip is hash-identical
- [ ] 5,000-entry directory enumerates completely
- [ ] Cleanup/Close bookkeeping correct: `open_count` on `Close`, unlink on `Cleanup`+delete, reclaim at last `Close` (§7.2)
- [ ] Deleted-but-open lifecycle behaves per §3.3.2
- [ ] Invalid, stale, and duplicate-closed handles → controlled errors, no panic, no UB
- [ ] Invalid paths, reserved names, oversized names, invalid UTF-16 → specific statuses
- [ ] Offset/length overflow, zero-length, EOF, past-EOF all as specified
- [ ] Open handle survives rename
- [ ] Share access verified as WinFsp-owned
- [ ] Path traversal cannot escape the namespace — logically **and** by ProcMon

## 17.6 Robustness gate

- [ ] Every callback returns within `callback_timeout_ms` under an injected hang, with `OperationTimeout`
- [ ] Unrelated operations during a hang return `OperationTimeout`, matching §3.6
- [ ] A delay just under the deadline still succeeds
- [ ] The bound scales with `callback_timeout_ms`
- [ ] **Panic path proven end to end in debug and release** (§13.5): caught, logged with `request_id`, `STATUS_INTERNAL_ERROR`, poisoned, main-thread unmount, OS-safety clean
- [ ] Class B failures certified only by process-death recovery (§15.2), with no durability claim (§15.4)
- [ ] `CorruptBytes` not armable
- [ ] All six fuzz targets ≥30 minutes each: no escaping panic, no UB, no hang, no invalid code, no invariant violation
- [ ] Crash inputs preserved as regression tests
- [ ] Property tests green; failures print seeds
- [ ] Soak meets the §16.5 plateau criteria

## 17.7 Engineering gate

- [ ] Four-column translation test for every callback-reachable code
- [ ] No code path returns `NetworkTimeout` (ADR-0014)
- [ ] `request_id` on every boundary log line; level discipline applied
- [ ] No file content in logs (asserted by test); path-logging revisit noted for Phase 9
- [ ] Compatibility matrix complete with guest build and WinFsp version
- [ ] `docs/runbooks/stale-mount-recovery.md` exists
- [ ] Phase 0 release-profile amendment applied and noted in the Phase 0 document
- [ ] CI green, including the no-WinFsp conformance job
- [ ] `collect-evidence.ps1` output committed under `docs/evidence/phase-1/`

## 17.8 Tag

```powershell
.\scripts\test.ps1
.\scripts\collect-evidence.ps1
git add -A
git commit -m "M1.16: Phase 1 exit gate — contract, invariants, functional, robustness, Windows safety"
git tag phase/1-winfsp
git push --tags
```

VirtualBox snapshot: **`phase-1-complete`**.

---

# Requirement traceability

Every major requirement, end to end. If a row cannot be completed, the requirement is not ready.

| Requirement | Contract | Implementation | Test | Evidence | Exit criterion |
|---|---|---|---|---|---|
| No panic crosses the FFI | ADR-0013 | §2.3 guard | §2.5, §13.5 | test-results, logs | §17.6 |
| Poisoned shutdown is deadlock-free | ADR-0013a | §2.3, §4.1, §4.3 | §13.5, §15.1 | os-safety.txt | §17.6 |
| Every callback is bounded | ADR-0009, §3.6 | §3.6 `state()` | §13.2–13.4 | timing measurements | §17.6 |
| Timeout semantics are local, not network | ADR-0014 | §3.2 `OpCtx` | §11.7, §12.4 | test-results | §17.7 |
| Identifiers are opaque and generational | ADR-0008, §3.1 | §5.2 | §5.4, §14.1 | fuzz artifacts | §17.3 |
| Cleanup/Close bookkeeping | §3.3.1 | §7 | §7.2 | test-results | §17.5 |
| Deleted-but-open lifecycle | §3.3.2 | §5.1, §7 | §7.2, §10.1 | test-results | §17.5 |
| Enumeration completeness | §3.3.8, INV-DIR-2 | §9.1 | §14.2 | property seeds | §17.4 |
| Resource limits are non-mutating | §3.4, INV-RES-3 | §5.2, §8 | §14.3 | test-results | §17.4 |
| Structure stays well-formed | §3.5 | §5.3 | §11.6, §14.1 | test-results | §17.4 |
| Namespace cannot be escaped | INV-NS-6 | `VfsPath` | §7.2 | procmon-writes.csv | §17.2, §17.5 |
| Windows survives our failure | ADR-0013 Class B | — | §15.2, §16.1 | os-safety.txt | §17.2 |
| No resource leak | §16.5 | — | §16.5 | soak-samples.csv | §17.2 |
| Contract is Phase-2-ready | ADR-0011 | §3 | §11 | test-results | §17.3 |

---

# Phase 1 → Phase 2 handoff

**Phase 2 replaces the implementation behind the contract, not the contract itself.**

Phase 2 builds Path Resolver, Namespace, Directory Manager, File Manager, Handle Manager, Metadata Resolver, read/write logic and locking — all behind the same `Vfs` trait.

## Frozen at `VFS_CONTRACT_VERSION = 1`

**Interfaces**

| Artifact | Location | Phase 2 obligation |
|---|---|---|
| `Vfs` trait | §3.2 | implement; bump version + ADR if it must change |
| FFI contract | §2.1, §2.2 | unchanged; the adapter should need no edits |
| WinFsp adapter contract | §4.1, §9.1 | unchanged, including volume parameters |

**Semantics** — §3.3, in full: open/create, read/write, rename, delete, cleanup/close, deleted-but-open, sharing, directory enumeration, timestamps, naming.

**Identity** — §3.1: `NodeId`, `HandleId`, `CursorId`, `RequestId`, `index_number`, including the rule that `index_number` is monotonic and never derived from a slab index.

**Errors** — ADR-0014 taxonomy and the local/remote split; ADR-0015 placement; the four-column NTSTATUS matrix. New codes require classification (M0.4's exhaustiveness test enforces it) and a matrix row.

**Safety** — §3.4 limits, §3.5 invariants (plus any namespace-specific additions), §13.1 fault points, ADR-0009 timeout behaviour, ADR-0013/0013a panic and poison model.

**Testing** — §11 conformance suite, §14.2 properties, §14.1 fuzz targets, §1.5 and §17.1 tooling.

## Known Phase 1 simplifications Phase 2 must resolve

| Simplification | Where | Phase 2 action |
|---|---|---|
| ASCII-only case folding | §3.3.7 | implement Unicode folding; flip the capability; both variants stay tested |
| Single global state lock | §3.6 | design real concurrency; §3.6 must be rewritten and re-versioned |
| `pattern` ignored in enumeration | §3.3.8 | decide whether to take over pattern matching |
| `CanDelete` only, no `SetDelete` | §3.3.1 | revisit if `SetDelete` becomes necessary |
| Mutation during enumeration undefined | §3.3.8 | define it once concurrency exists |

## The acceptance test for Phase 1 itself

```
Replace MemVfs → Real SPACE VFS
   → no WinFsp adapter changes
   → no semantic test rewrites
   → conformance suite passes unmodified
```

If that cannot happen, Phase 1 froze the wrong contract. Stop and fix the contract rather than patching around it.

---

# Appendix — troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| "winfsp-x64.dll was not found" at launch | not delay-loaded | §2.4 link args + `FspLoad(nullptr)` first |
| `SetMountPoint` → `STATUS_OBJECT_NAME_COLLISION` | `S:` in use or stale mount | `Get-PSDrive`; `fsptool lsvol`; §15.3 |
| `S:` appears but will not open | `GetSecurityByName` buffer protocol wrong | §6.2 point 3 |
| Directory spins forever in Explorer | missing `NULL` end marker | §9.1 |
| Large directory truncates | `Marker` resume wrong, or buffer-full returned as an error | §9.1; INV-DIR-2 detects it automatically |
| Deleted files reappear | unlink implemented in `Close` instead of `Cleanup` | §3.3.1 bookkeeping table |
| Cleanup never fires for read-only opens | `PostCleanupWhenModifiedOnly = 1` | §4.1 note 2 |
| Stat after write returns a stale size | non-zero `FileInfoTimeout` | §4.1 note 1 |
| Files grow during copy | `ConstrainedIo` ignored | §3.3.4 |
| Garbage sizes or timestamps | `space_file_info` layout drift | §2.5 static asserts |
| Explorer conflates distinct files | `index_number` reused | §3.1, INV-ID-2 |
| Process aborts instead of returning an error on panic | release profile still has `panic = "abort"` | ADR-0013 |
| Hang during unmount | `StopDispatcher` called from a dispatcher thread | §4.1, ADR-0013a |
| Everything returns `STATUS_INTERNAL_ERROR` | filesystem poisoned | find `PANIC at FFI boundary` and its `request_id` |
| Unrelated operations time out during a hang | expected: single state lock | §3.6 — documented, not a defect |
| Drive letter lingers after exit | unmount ordering | §4.1 |
| Memory climbs under stress | handle, cursor or node leak | §16.5 thresholds; INV-FS-2, INV-RES-1 |
| `check_invariants` fails on a deleted-but-open file | INV-NS-2 applied to an unlinked node | §5.3 — the carve-out is deliberate |
| Conformance job needs WinFsp | an adapter import leaked into core | §11.2 |

---

# Phase 1 Final Architecture Review

| Area | Decision | Reason |
|---|---|---|
| **Panic / FFI** | **Modified** | Separated Rust panic containment (Class A: `catch_unwind` → log → convert → poison) from process-level failure (Class B: access violation, stack overflow, OOM, adapter fault, external kill), which `catch_unwind` cannot touch and which is certified only by process-death recovery. Previous wording implied one mechanism covered both. |
| **Poison / shutdown model** | **Added** | New ADR-0013a defines RUNNING → POISONING → POISONED → STOPPING → UNMOUNTED, with explicit rules for new callbacks, in-flight callbacks, `cleanup`/`close` (silent no-ops, a documented bounded leak), shutdown ownership (main thread only), and the structural prevention of the `StopDispatcher`-from-dispatcher-thread deadlock. Also collapsed two overlapping poison mechanisms — the atomic flag and `std::sync::Mutex` poisoning — into one by moving to `parking_lot`. |
| **Timeout errors** | **Modified** | `OperationTimeout` is the only timeout Phase 1 can produce and means "a filesystem operation exceeded its configured deadline." `NetworkTimeout` is reserved for the Phase 4 transfer engine and may never be produced by the VFS, enforced by a conformance assertion. Rejected splitting `InvalidParameter` into `InvalidOffset`/`InvalidLength`: same NTSTATUS, no behavioural difference, and the log already carries offset and length. |
| **NTSTATUS translation** | **Modified** | Moved out of the shared `contracts` crate — which the cloud service also uses and which has no business knowing Windows types — into `client/core/src/ffi/ntstatus.rs`. The VFS now returns only `SpaceError`. The exhaustiveness test asserts **totality, not injectivity**, since several codes legitimately share one NTSTATUS. |
| **CorruptBytes** | **Removed** | Phase 1 has no integrity mechanism, so the fault's expected result would be "the flipped byte comes back," which asserts nothing while appearing in the exit gate as integrity coverage. Deferred to Phase 3 with a precise definition; `arm()` rejects it, with a test. |
| **Capabilities** | **Modified** | Hardened to a rule: a capability selects between documented correct behaviours and never disables a check. Reduced to one flag (`unicode_case_folding`) with both variants specified in the semantics document and both tested; `persistent` deleted (it could only skip tests) and `max_bytes` moved to a `Limits` struct (a configured limit is not a capability). A deliberately brittle size assertion makes adding a flag require an ADR. |
| **VFS invariants** | **Kept, tightened** | 21 numbered invariants across identity, namespace, file state, enumeration and resources, each cross-referenced from the semantics tables, faults, fuzz targets, traceability table, troubleshooting and exit gate. Added the explicit rule that the checker is **read-only, deterministic and non-repairing**, with a snapshot-comparison test proving it, and a "checker must fire" test proving it detects each violation class. Excluded content-integrity and durability invariants as unfalsifiable in Phase 1. |
| **Cleanup / Close semantics** | **Modified — correctness fix** | The previous statement that "Cleanup fires when the last handle to the file closes" was inaccurate. `Cleanup` is per **file object** and its posting is conditional on `PostCleanupWhenModifiedOnly`; `Close` is the only callback guaranteed once per open. Bookkeeping reassigned accordingly: `open_count` and slot release on `Close`, unlink on `Cleanup`+`FspCleanupDelete`, reclamation at the `Close` that brings `open_count` to zero. `PostCleanupWhenModifiedOnly` set to `0` for determinism; `CanDelete`-only recorded as a deliberate choice over `SetDelete`. |
| **ReadDirectory semantics** | **Modified** | Split into a VFS semantic contract (§3.3.8: order, marker meaning, completeness, termination, cursor lifetime, pattern handling) and adapter mechanics (§9.1: buffer packing, buffer-full-is-success, mandatory `NULL` end marker). Cursor lifetime narrowed to a single `ReadDirectory` call, which removes a leak class and makes the cursor limit meaningful. WinFsp's directory-buffer helpers explicitly declined, with the reason recorded. |
| **Identity model** | **Added** | A five-identifier table (`NodeId`, `HandleId`, `CursorId`, `RequestId`, `index_number`) with scope, lifetime, FFI visibility, generational status and Windows visibility. Two new rules: `index_number` must come from a monotonic counter and never from a reusable slab index, and `RequestId` is minted at the first Rust frame because the adapter makes no decisions and logs nothing. |
| **Concurrency model** | **Added** | A documented model rather than an emergent one: WinFsp's coarse guard plus a single VFS state lock means exactly one operation executes at a time, one hung callback blocks all others, and every other operation returns `OperationTimeout` rather than hanging — because lock acquisition is deadline-bounded via `parking_lot::try_lock_until`. §13.2 now *confirms* the predicted behaviour instead of discovering it. |
| **Resource limits** | **Modified** | Each limit now specifies value, enforcement point, error, state-on-failure, and atomicity. Directory entries reduced from 1,000,000 to 65,536 and cursors from 4,096 to 256 — Phase 1 tests boundary correctness, and the larger numbers only made the invariant checker slow. Limit+1 tests now assert `state_before == state_after` field by field, ahead of asserting the error code. |
| **Soak thresholds** | **Modified** | Replaced "memory must plateau" with decidable criteria: ±10% band, <1 MB/hour regression slope after the 30-minute mark, monotonic growth across ≥6 samples as the investigate trigger, and SPACE's own object counts returning to baseline. Explicitly scoped to leak detection, not optimisation. |
| **Kill-test claims** | **Modified** | Now states what the tests prove (Windows recovers when the process disappears) and what they do not (durable filesystem recovery, which Phase 1 cannot have and Phase 5 owns). |
| **Fuzzing claims** | **Modified** | Pass condition stated as the absence of specific defect classes over explored inputs, with an explicit note that fuzzing does not prove correctness. |
| **Milestones / structure** | **Kept** | Seventeen sessions, M1.0–M1.16, unchanged. Only §3's subsections were reorganised to fit the identity and concurrency contracts; no milestone was renumbered. |
| **Scope** | **Kept** | Audited for chunking, BLAKE3, integrity, cache, WAL, durability, manifests, versions, S3, PostgreSQL, cloud transfer, sync, remote metadata and distributed coordination. All remaining mentions are forward references marking where a concept belongs; none is implemented. |
