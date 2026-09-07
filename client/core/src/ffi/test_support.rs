//! Test-only helpers for the FFI boundary.
//!
//! The FFI is process-global by nature: one core, one `LifeState`. Tests that
//! poison it, stop it, or restart it therefore cannot run concurrently, so they
//! all take [`serial`] first.

use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::vfs::limits::{Limits, PathLimits};
use crate::vfs::types::{Capabilities, VfsConfig};

/// The lock every FFI test holds. `parking_lot` rather than `std` so a panicking
/// test (several deliberately panic) does not poison it for the rest.
static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

/// Serialize access to the process-global FFI state.
///
/// Returns a guard; a panicking test would poison a `std::sync::Mutex`, so the
/// poison is cleared rather than propagated -- otherwise one deliberate panic
/// test would fail every later test for the wrong reason.
pub fn serial() -> MutexGuard<'static, ()> {
    let m = SERIAL.get_or_init(|| Mutex::new(()));
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Bring up a core directly, without a config file.
///
/// `space_core_start` reads a config from disk, which is the production path
/// and is tested separately. Most boundary tests only need *a* running core.
pub fn start_test_core() {
    start_test_core_with(VfsConfig {
        limits: Limits {
            max_bytes: 8 * 1024 * 1024,
            max_open_handles: 64,
            max_open_cursors: 16,
            max_dir_entries: 32,
            max_io_bytes: crate::vfs::limits::MAX_IO_BYTES,
        },
        path_limits: PathLimits::DEFAULT,
        capabilities: Capabilities::PHASE_1,
    });
}

pub fn start_test_core_with(cfg: VfsConfig) {
    super::lifecycle::reset_for_test();
    let core = super::Core {
        vfs: crate::vfs::memvfs::MemVfs::new(cfg),
        // The real descriptor needs Windows; boundary tests that do not
        // exercise GetSecurityByName do not need real bytes, and the ones that
        // do are cfg(windows).
        security_descriptor: super::security::default_descriptor()
            .unwrap_or_else(|_| vec![0u8; 20]),
        cursor_names: parking_lot::Mutex::new(std::collections::HashMap::new()),
    };
    *super::CORE.write() = Some(std::sync::Arc::new(core));
    super::CALLBACK_TIMEOUT_MS.store(30_000, std::sync::atomic::Ordering::Relaxed);
}

/// Tear the test core down and return the lifecycle to RUNNING.
pub fn stop_test_core() {
    *super::CORE.write() = None;
    super::lifecycle::reset_for_test();
}

/// A NUL-terminated UTF-16 buffer, for passing paths across the boundary.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
