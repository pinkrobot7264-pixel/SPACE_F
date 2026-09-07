//! The FFI boundary (Phase 1 §2) -- the only surface between C++ and Rust.
//!
//! **Treat this boundary as hostile.** Every pointer is checked, every length
//! is validated before it sizes an allocation, every string is scanned with a
//! hard cap, and every panic is contained. A successful call is not enough: the
//! boundary must also fail safely.
//!
//! The eleven boundary rules (§2.1) and where each is enforced:
//!
//! | # | Rule | Enforced by |
//! |---|---|---|
//! | 1 | `#[repr(C)]` scalars and pointers only | [`types`] |
//! | 2 | every buffer is caller-allocated and caller-owned | no entry point returns memory |
//! | 3 | pointers valid only for the call, except `dir_entry.name` | the per-cursor name table |
//! | 4 | null where non-null is required returns `STATUS_INVALID_PARAMETER` | [`out_ptr`] / [`wstr`] |
//! | 5 | UTF-16, NUL-terminated, hard length cap | [`wstr`] |
//! | 6 | lengths validated against `max_io_bytes` before allocation | [`space_core_read`] / [`space_core_write`] |
//! | 7 | struct size and offsets asserted on both sides | [`types`] tests + `space_core.h` |
//! | 8 | opaque generational `u64`; `0` never valid | [`crate::vfs::ids`] |
//! | 9 | every fallible entry point returns `NTSTATUS` | [`guard`] |
//! | 10 | panics contained in every build profile | [`guard`] + ADR-0013 |
//! | 11 | every entry point is bounded, including lock acquisition | [`OpCtx`] + §3.6 |

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use contracts::{ErrorCode, SpaceError};
use parking_lot::{Mutex, RwLock};

use crate::vfs::ids::{CursorId, HandleId};
use crate::vfs::memvfs::MemVfs;
use crate::vfs::types::*;
use crate::vfs::{OpCtx, Vfs, VfsPath, VfsSectionExt};

pub mod lifecycle;
pub mod ntstatus;
pub mod security;
pub mod types;

#[cfg(test)]
pub mod test_support;
#[cfg(test)]
mod fault_tests;
#[cfg(test)]
mod log_tests;
#[cfg(test)]
mod tests;

use lifecycle::{accepting_work, enter_poisoning};
use ntstatus::NtStatus;
use types::{SpaceDirEntry, SpaceFileInfo, SpaceVolumeInfo};

pub type SpaceStatus = NtStatus;
pub type SpaceHandle = u64;
pub type SpaceCursor = u64;

// ---------------------------------------------------------------------------
// The core singleton
// ---------------------------------------------------------------------------

/// Everything `space_core_start` brings up.
pub struct Core {
    pub vfs: MemVfs,
    /// The one Phase 1 security descriptor, converted once at startup (§6.1).
    pub security_descriptor: Vec<u8>,
    /// Per-cursor UTF-16 name buffers.
    ///
    /// Boundary rule 3: `space_dir_entry.name` is borrowed and valid until the
    /// next `dir_next` on the same cursor. The buffer lives here rather than in
    /// the VFS so that `DirEntry.name` stays an ordinary `String` and the VFS
    /// carries no FFI concern. Heap buffers are stable while not reallocated,
    /// and the only thing that reallocates one is the next `dir_next` on that
    /// same cursor -- which is exactly the documented lifetime.
    cursor_names: Mutex<HashMap<u64, Vec<u16>>>,
}

static CORE: RwLock<Option<Arc<Core>>> = RwLock::new(None);

/// Callback deadline in milliseconds (L9), published at startup so the hot path
/// reads it without taking a lock.
static CALLBACK_TIMEOUT_MS: AtomicU64 = AtomicU64::new(30_000);

fn core() -> Option<Arc<Core>> {
    CORE.read().clone()
}

fn callback_deadline() -> Duration {
    Duration::from_millis(CALLBACK_TIMEOUT_MS.load(Ordering::Relaxed))
}

/// The core, or `InternalError` if the filesystem is not running.
fn require_core() -> Result<Arc<Core>, SpaceError> {
    core().ok_or_else(|| {
        SpaceError::new(ErrorCode::InternalError, "space_core_start has not run")
    })
}

// ---------------------------------------------------------------------------
// The guard (§2.3)
// ---------------------------------------------------------------------------

/// The per-request diagnostic fields carried into the boundary log line
/// (§12.1).
///
/// The manual's example line is
/// `{"operation":"read","path":"\\dir\\file.txt","handle":"h_4294967297",
///   "offset":4096,"length":4096,...}`, so these are part of the contract, not
/// debugging leftovers: without the handle a stale-handle report names no
/// handle, and without offset/length an `InvalidParameter` names no parameter
/// (which is exactly why ADR-0014 declined to split that code).
#[derive(Default)]
pub(crate) struct Trace {
    /// A `Cell` because `create` and `open` only learn their handle after the
    /// call succeeds, and a line for those two without the handle they just
    /// minted is the one line you always want when tracing a handle's life.
    handle: std::cell::Cell<Option<u64>>,
    offset: Option<u64>,
    length: Option<u32>,
    /// Filled in by the closure once the path has been validated. Phase 1 logs
    /// paths because all data is synthetic; see `logging.md` for the Phase 9
    /// revisit.
    path: std::cell::RefCell<Option<String>>,
}

impl Trace {
    fn none() -> Trace {
        Trace::default()
    }
    fn handle(h: SpaceHandle) -> Trace {
        Trace {
            handle: std::cell::Cell::new(Some(h)),
            ..Default::default()
        }
    }
    fn io(h: SpaceHandle, offset: u64, length: u32) -> Trace {
        Trace {
            handle: std::cell::Cell::new(Some(h)),
            offset: Some(offset),
            length: Some(length),
            ..Default::default()
        }
    }
    fn set_handle(&self, h: SpaceHandle) {
        self.handle.set(Some(h));
    }
    fn set_path(&self, p: &str) {
        *self.path.borrow_mut() = Some(p.to_string());
    }
}

/// Wrap a fallible entry point: lifecycle check, deadline, panic containment,
/// NTSTATUS translation.
fn guard<F>(op: &'static str, trace: Trace, f: F) -> SpaceStatus
where
    F: FnOnce(&OpCtx, &Core, &Trace) -> Result<(), SpaceError>,
{
    // ADR-0013a: once past RUNNING, touch nothing.
    if !accepting_work() {
        return ntstatus::STATUS_INTERNAL_ERROR;
    }

    let cx = OpCtx::new(callback_deadline());
    let started = std::time::Instant::now();

    // ADR-0013: a panic never reaches a C++ frame, in any profile.
    match catch_unwind(AssertUnwindSafe(|| {
        // Fault injection sits inside catch_unwind on purpose: the `Panic`
        // action must travel the same path a real panic does, or the §13.5 row
        // would be testing the harness rather than the guard.
        apply_fault(op, &cx)?;
        let core = require_core()?;
        f(&cx, &core, &trace)
    })) {
        Ok(Ok(())) => {
            log_boundary(op, &trace, &cx, started, None);
            ntstatus::STATUS_SUCCESS
        }
        Ok(Err(e)) => {
            log_boundary(op, &trace, &cx, started, Some(&e));
            ntstatus::from_error_code(e.code)
        }
        Err(payload) => {
            tracing::error!(
                op,
                request_id = %cx.request_id,
                handle = trace.handle.get(),
                "PANIC at FFI boundary: {}",
                panic_message(&payload)
            );
            enter_poisoning(); // sets state, signals main thread, returns
            ntstatus::STATUS_INTERNAL_ERROR
        }
    }
}

/// One structured line per boundary request (§12.1).
///
/// Level discipline (§12.2): a `FileNotFound` from `probe` is the normal
/// existence check, not an error. Logging it at error level buries real
/// problems, so only genuine bugs reach `error`.
fn log_boundary(
    op: &'static str,
    trace: &Trace,
    cx: &OpCtx,
    started: std::time::Instant,
    err: Option<&SpaceError>,
) {
    let duration_ms = started.elapsed().as_millis() as u64;
    let path = trace.path.borrow().clone();
    // `h_<raw>` matches the Display form of HandleId, so a log line and a test
    // failure name the same thing.
    let handle = trace.handle.get().map(|h| format!("h_{h}"));
    let offset = trace.offset;
    let length = trace.length;

    match err {
        None => tracing::debug!(
            component = "winfsp",
            operation = op,
            request_id = %cx.request_id,
            path,
            handle,
            offset,
            length,
            duration_ms,
            result = "ok",
            "operation served"
        ),
        Some(e) if is_expected(e.code) => tracing::debug!(
            component = "winfsp",
            operation = op,
            request_id = %cx.request_id,
            path,
            handle,
            offset,
            length,
            duration_ms,
            result = "error",
            error_code = ?e.code,
            "operation completed with an expected status"
        ),
        Some(e) if e.code == ErrorCode::InternalError => tracing::error!(
            component = "winfsp",
            operation = op,
            request_id = %cx.request_id,
            path,
            handle,
            offset,
            length,
            duration_ms,
            result = "error",
            error_code = ?e.code,
            "internal error at the boundary: {}",
            e.message
        ),
        Some(e) => tracing::warn!(
            component = "winfsp",
            operation = op,
            request_id = %cx.request_id,
            path,
            handle,
            offset,
            length,
            duration_ms,
            result = "error",
            error_code = ?e.code,
            "operation failed: {}",
            e.message
        ),
    }
}

/// Map a boundary operation to its registered fault point (§13.1).
///
/// Returns `None` for operations with no registered point; the eight names are
/// fixed by `docs/protocols/fault-points.md` and are not invented here.
fn fault_point_for(op: &str) -> Option<&'static str> {
    Some(match op {
        "read" => "winfsp_pre_read",
        "write" => "winfsp_pre_write",
        "open" => "winfsp_pre_open",
        "create" => "winfsp_pre_create",
        "dir_open" => "winfsp_pre_readdir",
        "get_file_info" => "winfsp_pre_getinfo",
        "rename" => "winfsp_pre_rename",
        "cleanup" => "winfsp_pre_cleanup",
        _ => return None,
    })
}

/// Evaluate the fault point for this operation, if one is armed.
///
/// In a build without the `fault-injection` feature every `fault_point` call is
/// `#[inline(always)]` and returns `None`, so this collapses to nothing.
fn apply_fault(op: &'static str, cx: &OpCtx) -> Result<(), SpaceError> {
    let Some(point) = fault_point_for(op) else {
        return Ok(());
    };

    match faults::fault_point(point) {
        faults::FaultAction::None => Ok(()),

        faults::FaultAction::Fail(code) => {
            Err(SpaceError::new(code, "injected failure"))
        }

        // §13.4's near-miss: a delay just under the deadline must still
        // succeed, so this sleeps and then proceeds rather than failing.
        faults::FaultAction::Delay(d) => {
            std::thread::sleep(d);
            Ok(())
        }

        // Block until the deadline expires, then report it. This is what makes
        // L9 falsifiable: the callback must return within
        // callback_timeout_ms rather than hanging the kernel's dispatcher
        // thread forever (ADR-0009).
        faults::FaultAction::Hang => {
            while cx.remaining().is_some() {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(SpaceError::new(
                ErrorCode::OperationTimeout,
                "injected hang exceeded the callback deadline",
            ))
        }

        faults::FaultAction::Cancel => Err(SpaceError::new(
            ErrorCode::Cancelled,
            "injected cancellation",
        )),

        faults::FaultAction::InvalidInput => Err(SpaceError::new(
            ErrorCode::InvalidParameter,
            "injected invalid input",
        )),

        faults::FaultAction::StaleHandle => Err(SpaceError::new(
            ErrorCode::InvalidHandle,
            "injected stale handle",
        )),

        faults::FaultAction::ResourceExhausted => Err(SpaceError::new(
            ErrorCode::ResourceExhausted,
            "injected resource exhaustion",
        )),

        faults::FaultAction::Panic => {
            panic!("injected panic at fault point {point}")
        }

        // Unreachable: `arm` refuses it in Phase 1. Handled explicitly rather
        // than with a wildcard so that Phase 3, which makes it armable, gets a
        // compile error here instead of a silent no-op.
        faults::FaultAction::CorruptBytes => Err(SpaceError::new(
            ErrorCode::InternalError,
            "CorruptBytes is not armable in Phase 1",
        )),
    }
}

/// Statuses that are part of normal filesystem conversation, not anomalies.
fn is_expected(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::FileNotFound
            | ErrorCode::ObjectPathNotFound
            | ErrorCode::FileExists
            | ErrorCode::EndOfFile
            | ErrorCode::NotADirectory
            | ErrorCode::FileIsADirectory
            | ErrorCode::DirectoryNotEmpty
            | ErrorCode::BufferOverflow
    )
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

// ---------------------------------------------------------------------------
// Pointer and string helpers (boundary rules 4 and 5)
// ---------------------------------------------------------------------------

/// Read a NUL-terminated UTF-16 string with a hard length cap.
///
/// # Safety
/// `p` must be null or point to a readable sequence of `u16`.
///
/// The length cap is not decoration: a missing NUL terminator would otherwise
/// walk memory until it faults.
unsafe fn wstr(p: *const u16) -> Result<String, SpaceError> {
    if p.is_null() {
        return Err(SpaceError::new(ErrorCode::ObjectNameInvalid, "null path"));
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
        if len > crate::vfs::limits::MAX_PATH_CHARS {
            return Err(SpaceError::new(
                ErrorCode::NameTooLong,
                "unterminated or oversized path",
            ));
        }
    }
    String::from_utf16(std::slice::from_raw_parts(p, len))
        .map_err(|_| SpaceError::new(ErrorCode::ObjectNameInvalid, "invalid UTF-16"))
}

/// Optional wide string: null yields `None` rather than an error.
///
/// # Safety
/// As [`wstr`].
unsafe fn wstr_opt(p: *const u16) -> Result<Option<String>, SpaceError> {
    if p.is_null() {
        Ok(None)
    } else {
        wstr(p).map(Some)
    }
}

/// Validate a required out-pointer (boundary rule 4).
///
/// Returns `InvalidParameter` rather than dereferencing.
fn out_ptr<'a, T>(p: *mut T, what: &'static str) -> Result<&'a mut T, SpaceError> {
    if p.is_null() {
        return Err(SpaceError::new(
            ErrorCode::InvalidParameter,
            format!("null out pointer: {what}"),
        ));
    }
    // SAFETY: checked non-null; the caller owns the pointee for the call's
    // duration (boundary rule 3).
    Ok(unsafe { &mut *p })
}

// ---------------------------------------------------------------------------
// Lifecycle entry points
// ---------------------------------------------------------------------------

/// Bring the core up from a config file.
///
/// # Safety
/// `config_path` must be a valid NUL-terminated UTF-16 string.
#[no_mangle]
pub unsafe extern "C" fn space_core_start(config_path: *const u16) -> SpaceStatus {
    // Not routed through `guard`: it runs before the filesystem exists, and the
    // errors it can produce are the startup-only ones (§ADR-0015).
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), SpaceError> {
        let path = wstr(config_path)?;
        let cfg = space_config::Config::load(std::path::Path::new(&path))?;
        start_with_config(&cfg)
    }));

    match result {
        Ok(Ok(())) => ntstatus::STATUS_SUCCESS,
        Ok(Err(e)) => {
            tracing::error!(error_code = ?e.code, "space_core_start failed: {}", e.message);
            ntstatus::from_error_code(e.code)
        }
        Err(p) => {
            tracing::error!("PANIC in space_core_start: {}", panic_message(&p));
            ntstatus::STATUS_INTERNAL_ERROR
        }
    }
}

/// Build the core from an already-validated config. Shared by the FFI entry
/// point and by `client/main`, so both bring up an identical filesystem.
pub fn start_with_config(cfg: &space_config::Config) -> Result<(), SpaceError> {
    CALLBACK_TIMEOUT_MS.store(cfg.client.callback_timeout_ms, Ordering::Relaxed);

    let vfs_cfg = VfsConfig {
        limits: cfg.vfs.limits(),
        path_limits: cfg.vfs.path_limits(),
        capabilities: Capabilities {
            unicode_case_folding: cfg.vfs.unicode_case_folding,
        },
    };

    let core = Core {
        vfs: MemVfs::new(vfs_cfg),
        security_descriptor: security::default_descriptor()?,
        cursor_names: Mutex::new(HashMap::new()),
    };
    *CORE.write() = Some(Arc::new(core));

    arm_fault_from_env();
    Ok(())
}

/// Arm one fault point from `SPACE_FAULT=<point>=<action>`, for the
/// through-the-mount fault tests (§13.3).
///
/// Deliberately crude -- one point, one action, read once at startup. A real
/// control channel is a Phase 10 concern, and building one here would add
/// untested surface to the very component whose failure behaviour is under
/// test. Compiled out entirely without the `fault-injection` feature, so a
/// release build cannot be steered by an environment variable.
#[cfg(feature = "fault-injection")]
fn arm_fault_from_env() {
    let Ok(spec) = std::env::var("SPACE_FAULT") else {
        return;
    };
    let Some((point, action)) = spec.split_once('=') else {
        tracing::warn!(spec, "SPACE_FAULT must be <point>=<action>; ignored");
        return;
    };

    let action = match action.to_ascii_lowercase().as_str() {
        "hang" => faults::FaultAction::Hang,
        "panic" => faults::FaultAction::Panic,
        "cancel" => faults::FaultAction::Cancel,
        "stalehandle" => faults::FaultAction::StaleHandle,
        "invalidinput" => faults::FaultAction::InvalidInput,
        "resourceexhausted" => faults::FaultAction::ResourceExhausted,
        other => {
            if let Some(ms) = other.strip_prefix("delay:") {
                match ms.parse::<u64>() {
                    Ok(ms) => faults::FaultAction::Delay(Duration::from_millis(ms)),
                    Err(_) => {
                        tracing::warn!(other, "bad delay in SPACE_FAULT; ignored");
                        return;
                    }
                }
            } else {
                tracing::warn!(other, "unknown action in SPACE_FAULT; ignored");
                return;
            }
        }
    };

    match faults::arm(point, action.clone()) {
        Ok(()) => tracing::warn!(
            point,
            ?action,
            "FAULT INJECTION ARMED -- this build is not fit for production use"
        ),
        Err(e) => tracing::warn!(point, "could not arm fault: {e}"),
    }
}

#[cfg(not(feature = "fault-injection"))]
fn arm_fault_from_env() {}

/// Tear the core down. Main thread only (ADR-0013a).
#[no_mangle]
pub extern "C" fn space_core_stop() -> SpaceStatus {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        *CORE.write() = None;
    }));
    lifecycle::enter_unmounted();
    ntstatus::STATUS_SUCCESS
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

/// # Safety
/// `out` must be a valid, writable `space_volume_info`.
#[no_mangle]
pub unsafe extern "C" fn space_core_get_volume_info(out: *mut SpaceVolumeInfo) -> SpaceStatus {
    guard("get_volume_info", Trace::none(), |cx, core, _tr| {
        let slot = out_ptr(out, "out")?;
        let vi = core.vfs.volume_info(cx)?;
        *slot = SpaceVolumeInfo {
            total_size: vi.total_size,
            free_size: vi.free_size,
        };
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Open and close
// ---------------------------------------------------------------------------

/// # Safety
/// `path` must be a valid NUL-terminated UTF-16 string; `out_handle` and
/// `out_info` must be writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_create(
    path: *const u16,
    create_options: u32,
    granted_access: u32,
    file_attributes: u32,
    allocation_size: u64,
    out_handle: *mut SpaceHandle,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("create", Trace::none(), |cx, core, _tr| {
        let h_slot = out_ptr(out_handle, "out_handle")?;
        let i_slot = out_ptr(out_info, "out_info")?;
        let s = wstr(path)?;
        _tr.set_path(&s);
        let p = core.vfs.parse_path(&s)?;
        let opened = core.vfs.create(
            cx,
            &p,
            CreateOptions {
                create_options,
                granted_access,
                file_attributes,
                allocation_size,
            },
        )?;
        *h_slot = opened.handle.as_raw();
        _tr.set_handle(opened.handle.as_raw());
        *i_slot = opened.info.into();
        Ok(())
    })
}

/// # Safety
/// As [`space_core_create`].
#[no_mangle]
pub unsafe extern "C" fn space_core_open(
    path: *const u16,
    create_options: u32,
    granted_access: u32,
    out_handle: *mut SpaceHandle,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("open", Trace::none(), |cx, core, _tr| {
        let h_slot = out_ptr(out_handle, "out_handle")?;
        let i_slot = out_ptr(out_info, "out_info")?;
        let s = wstr(path)?;
        _tr.set_path(&s);
        let p = core.vfs.parse_path(&s)?;
        let opened = core.vfs.open(
            cx,
            &p,
            OpenOptions {
                create_options,
                granted_access,
            },
        )?;
        *h_slot = opened.handle.as_raw();
        _tr.set_handle(opened.handle.as_raw());
        *i_slot = opened.info.into();
        Ok(())
    })
}

/// # Safety
/// `out_info` must be writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_overwrite(
    h: SpaceHandle,
    file_attributes: u32,
    replace_attributes: u8,
    allocation_size: u64,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("overwrite", Trace::handle(h), |cx, core, _tr| {
        let i_slot = out_ptr(out_info, "out_info")?;
        let info = core.vfs.overwrite(
            cx,
            HandleId::from_raw(h),
            file_attributes,
            replace_attributes != 0,
            allocation_size,
        )?;
        *i_slot = info.into();
        Ok(())
    })
}

/// # Safety
/// `path` may be null; if non-null it must be a valid wide string.
#[no_mangle]
pub unsafe extern "C" fn space_core_cleanup(h: SpaceHandle, _path: *const u16, flags: u32) {
    // `cleanup` bypasses `guard` because it is `void` and cannot report
    // failure. It checks accepting_work() first and returns immediately if
    // false (ADR-0013a). WinFsp supplies a FileName here; the adapter uses it
    // only for logging, so it is deliberately unused.
    if !accepting_work() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(core) = core() {
            let cx = OpCtx::new(callback_deadline());
            // The void entry points cannot use `guard` (they return nothing to
            // report through), but they are still boundary requests and §12.1
            // asks for a line per request. Without one, a delete is invisible
            // in the trace -- which is exactly how a namespace bug hides.
            tracing::debug!(
                component = "winfsp",
                operation = "cleanup",
                request_id = %cx.request_id,
                handle = format!("h_{h}"),
                flags = format!("{flags:#x}"),
                delete = CleanupFlags(flags).delete(),
                "cleanup"
            );
            core.vfs
                .cleanup(&cx, HandleId::from_raw(h), CleanupFlags(flags));
        }
    }));
}

#[no_mangle]
pub extern "C" fn space_core_close(h: SpaceHandle) {
    // Deliberate bounded leak once poisoned; process exit reclaims (ADR-0013a).
    if !accepting_work() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(core) = core() {
            let cx = OpCtx::new(callback_deadline());
            tracing::debug!(
                component = "winfsp",
                operation = "close",
                request_id = %cx.request_id,
                handle = format!("h_{h}"),
                "close"
            );
            core.vfs.close(&cx, HandleId::from_raw(h));
        }
    }));
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

/// # Safety
/// `buffer` must be writable for `length` bytes; `out_transferred` must be
/// writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_read(
    h: SpaceHandle,
    buffer: *mut std::ffi::c_void,
    offset: u64,
    length: u32,
    out_transferred: *mut u32,
) -> SpaceStatus {
    guard("read", Trace::io(h, offset, length), |cx, core, _tr| {
        let n_slot = out_ptr(out_transferred, "out_transferred")?;
        *n_slot = 0;

        // Boundary rule 6: validate the length against max_io_bytes **before**
        // any allocation or indexing.
        let max = core.vfs.config().limits.max_io_bytes;
        if length as usize > max {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "length exceeds max_io_bytes (L4)",
            ));
        }
        if buffer.is_null() && length != 0 {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "null buffer with non-zero length",
            ));
        }

        let buf: &mut [u8] = if length == 0 {
            &mut []
        } else {
            std::slice::from_raw_parts_mut(buffer as *mut u8, length as usize)
        };
        let n = core.vfs.read(cx, HandleId::from_raw(h), offset, buf)?;
        *n_slot = n;
        Ok(())
    })
}

/// # Safety
/// `buffer` must be readable for `length` bytes; the out pointers must be
/// writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn space_core_write(
    h: SpaceHandle,
    buffer: *const std::ffi::c_void,
    offset: u64,
    length: u32,
    write_to_eof: u8,
    constrained_io: u8,
    out_transferred: *mut u32,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("write", Trace::io(h, offset, length), |cx, core, _tr| {
        let n_slot = out_ptr(out_transferred, "out_transferred")?;
        let i_slot = out_ptr(out_info, "out_info")?;
        *n_slot = 0;

        let max = core.vfs.config().limits.max_io_bytes;
        if length as usize > max {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "length exceeds max_io_bytes (L4)",
            ));
        }
        if buffer.is_null() && length != 0 {
            return Err(SpaceError::new(
                ErrorCode::InvalidParameter,
                "null buffer with non-zero length",
            ));
        }

        let buf: &[u8] = if length == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(buffer as *const u8, length as usize)
        };
        let (n, info) = core.vfs.write(
            cx,
            HandleId::from_raw(h),
            offset,
            buf,
            WriteMode {
                write_to_eof: write_to_eof != 0,
                constrained_io: constrained_io != 0,
            },
        )?;
        *n_slot = n;
        *i_slot = info.into();
        Ok(())
    })
}

/// # Safety
/// `out_info` may be null when flushing the whole volume.
#[no_mangle]
pub unsafe extern "C" fn space_core_flush(
    h: SpaceHandle,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("flush", Trace::handle(h), |cx, core, _tr| {
        // h == 0 means "the whole volume" by contract.
        let handle = if h == 0 {
            None
        } else {
            Some(HandleId::from_raw(h))
        };
        let info = core.vfs.flush(cx, handle)?;
        if let Some(info) = info {
            if !out_info.is_null() {
                *out_info = info.into();
            }
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Info
// ---------------------------------------------------------------------------

/// # Safety
/// `out_info` must be writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_get_file_info(
    h: SpaceHandle,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("get_file_info", Trace::handle(h), |cx, core, _tr| {
        let slot = out_ptr(out_info, "out_info")?;
        *slot = core.vfs.file_info(cx, HandleId::from_raw(h))?.into();
        Ok(())
    })
}

/// # Safety
/// `out_info` must be writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn space_core_set_basic_info(
    h: SpaceHandle,
    attrs: u32,
    ctime: u64,
    atime: u64,
    wtime: u64,
    chtime: u64,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("set_basic_info", Trace::handle(h), |cx, core, _tr| {
        let slot = out_ptr(out_info, "out_info")?;
        let patch = BasicInfoPatch::from_raw(attrs, ctime, atime, wtime, chtime);
        *slot = core
            .vfs
            .set_basic_info(cx, HandleId::from_raw(h), patch)?
            .into();
        Ok(())
    })
}

/// # Safety
/// `out_info` must be writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_set_file_size(
    h: SpaceHandle,
    new_size: u64,
    set_allocation: u8,
    out_info: *mut SpaceFileInfo,
) -> SpaceStatus {
    guard("set_file_size", Trace::handle(h), |cx, core, _tr| {
        let slot = out_ptr(out_info, "out_info")?;
        *slot = core
            .vfs
            .set_file_size(cx, HandleId::from_raw(h), new_size, set_allocation != 0)?
            .into();
        Ok(())
    })
}

/// The callback with the unusual contract (§6.2).
///
/// Three parts that are routinely got wrong:
///
/// 1. file does not exist -> `STATUS_OBJECT_NAME_NOT_FOUND`, which is normal
///    and not logged at error level;
/// 2. `out_attrs` may be null -- check before writing;
/// 3. buffer protocol: null `sd_buf` means "size only"; a too-small buffer
///    writes the required size and returns `STATUS_BUFFER_OVERFLOW`.
///
/// Getting (3) wrong produces an `S:` that Explorer can see but not open.
///
/// # Safety
/// `path` must be a valid wide string; `sd_size` must be writable when
/// non-null.
#[no_mangle]
pub unsafe extern "C" fn space_core_get_security_by_name(
    path: *const u16,
    out_attrs: *mut u32,
    sd_buf: *mut std::ffi::c_void,
    sd_size: *mut usize,
) -> SpaceStatus {
    guard("get_security_by_name", Trace::none(), |cx, core, _tr| {
        let s = wstr(path)?;
        _tr.set_path(&s);
        let p = core.vfs.parse_path(&s)?;
        let probe = core.vfs.probe(cx, &p)?;

        // (2) PFileAttributes may be NULL.
        if !out_attrs.is_null() {
            *out_attrs = probe.file_attributes;
        }

        // (3) the buffer protocol.
        if sd_size.is_null() {
            // No size out-parameter means the caller wants attributes only.
            return Ok(());
        }
        let needed = core.security_descriptor.len();
        let capacity = *sd_size;
        *sd_size = needed;

        if sd_buf.is_null() {
            // Size query: write the size and return success.
            return Ok(());
        }
        if capacity < needed {
            // BufferOverflow is a *warning*, not a failure: the required size
            // has been written and the caller will call again.
            return Err(SpaceError::new(
                ErrorCode::BufferOverflow,
                "security descriptor buffer too small",
            ));
        }
        std::ptr::copy_nonoverlapping(
            core.security_descriptor.as_ptr(),
            sd_buf as *mut u8,
            needed,
        );
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Namespace mutation
// ---------------------------------------------------------------------------

/// # Safety
/// `path` may be null; it is used only for logging.
#[no_mangle]
pub unsafe extern "C" fn space_core_can_delete(h: SpaceHandle, _path: *const u16) -> SpaceStatus {
    guard("can_delete", Trace::handle(h), |cx, core, _tr| {
        core.vfs.can_delete(cx, HandleId::from_raw(h))
    })
}

/// # Safety
/// `new_path` must be a valid wide string; `path` may be null.
#[no_mangle]
pub unsafe extern "C" fn space_core_rename(
    h: SpaceHandle,
    _path: *const u16,
    new_path: *const u16,
    replace_if_exists: u8,
) -> SpaceStatus {
    guard("rename", Trace::handle(h), |cx, core, _tr| {
        let s = wstr(new_path)?;
        _tr.set_path(&s);
        let to = core.vfs.parse_path(&s)?;
        core.vfs
            .rename(cx, HandleId::from_raw(h), &to, replace_if_exists != 0)
    })
}

// ---------------------------------------------------------------------------
// Directory enumeration
// ---------------------------------------------------------------------------

/// # Safety
/// `pattern` and `marker` may be null; `out_cursor` must be writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_dir_open(
    h: SpaceHandle,
    pattern: *const u16,
    marker: *const u16,
    out_cursor: *mut SpaceCursor,
) -> SpaceStatus {
    guard("dir_open", Trace::handle(h), |cx, core, _tr| {
        let slot = out_ptr(out_cursor, "out_cursor")?;
        *slot = 0;
        let pattern = wstr_opt(pattern)?;
        let marker = wstr_opt(marker)?;
        // The marker is the resume point WinFsp hands back; logging it makes the
        // enumeration protocol visible in the trace.
        _tr.set_path(marker.as_deref().unwrap_or("<no marker>"));
        let cursor = core.vfs.dir_open(
            cx,
            HandleId::from_raw(h),
            pattern.as_deref(),
            marker.as_deref(),
        )?;
        *slot = cursor.as_raw();
        Ok(())
    })
}

/// # Safety
/// `out_entry` and `out_has_more` must be writable.
#[no_mangle]
pub unsafe extern "C" fn space_core_dir_next(
    cursor: SpaceCursor,
    out_entry: *mut SpaceDirEntry,
    out_has_more: *mut u8,
) -> SpaceStatus {
    guard("dir_next", Trace::handle(cursor), |cx, core, _tr| {
        let entry_slot = out_ptr(out_entry, "out_entry")?;
        let more_slot = out_ptr(out_has_more, "out_has_more")?;
        *more_slot = 0;

        let next = core.vfs.dir_next(cx, CursorId::from_raw(cursor))?;
        match next {
            None => {
                *entry_slot = SpaceDirEntry::default();
                *more_slot = 0;
                Ok(())
            }
            Some(e) => {
                // Boundary rule 3: the name buffer is owned here and stays
                // valid until the next dir_next on this cursor.
                let mut names = core.cursor_names.lock();
                let buf = names.entry(cursor).or_default();
                buf.clear();
                buf.extend(e.name.encode_utf16());
                buf.push(0);
                entry_slot.name = buf.as_ptr();
                entry_slot.info = e.info.into();
                *more_slot = 1;
                Ok(())
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn space_core_dir_close(cursor: SpaceCursor) {
    if !accepting_work() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(core) = core() {
            let cx = OpCtx::new(callback_deadline());
            core.vfs.dir_close(&cx, CursorId::from_raw(cursor));
            core.cursor_names.lock().remove(&cursor);
        }
    }));
}

// ---------------------------------------------------------------------------
// Helpers used by client/main
// ---------------------------------------------------------------------------

/// Parse a path with the running core's limits. Used by tests and diagnostics.
pub fn parse_path_with_core(s: &str) -> Result<VfsPath, SpaceError> {
    require_core()?.vfs.parse_path(s)
}

/// The running core, for the main binary's diagnostics endpoint.
pub fn running_core() -> Option<Arc<Core>> {
    core()
}
