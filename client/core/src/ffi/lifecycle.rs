//! The process lifecycle and poison path (ADR-0013, ADR-0013a).
//!
//! ```text
//! RUNNING   ──panic / unrecoverable invariant failure──▶ POISONING
//! POISONING ──flag published, unmount signalled────────▶ POISONED
//! POISONED  ──main thread begins teardown──────────────▶ STOPPING
//! STOPPING  ──dispatcher stopped, mount removed────────▶ UNMOUNTED
//! ```
//!
//! **Only the main thread performs teardown.** The poisoning thread sets the
//! flag and signals a channel; it does no teardown itself. That is not a style
//! preference: `FspFileSystemStopDispatcher` waits for dispatcher threads to
//! drain, *including the caller*, so calling it from a dispatcher thread is a
//! self-deadlock. The signal-and-return design makes that structurally
//! impossible.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;

/// Where the filesystem is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LifeState {
    Running = 0,
    Poisoning = 1,
    Poisoned = 2,
    Stopping = 3,
    Unmounted = 4,
}

impl LifeState {
    fn from_u8(v: u8) -> LifeState {
        match v {
            0 => LifeState::Running,
            1 => LifeState::Poisoning,
            2 => LifeState::Poisoned,
            3 => LifeState::Stopping,
            _ => LifeState::Unmounted,
        }
    }
}

/// Why shutdown was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Ctrl-C or an explicit stop.
    Requested,
    /// A panic was contained at the FFI boundary (ADR-0013).
    Poisoned,
}

static STATE: AtomicU8 = AtomicU8::new(LifeState::Running as u8);
static SHUTDOWN_TX: OnceLock<SyncSender<ShutdownReason>> = OnceLock::new();

/// Install the shutdown channel. Called once by `client/main` before mounting.
///
/// The channel is bounded and `try_send` is used, so a poisoning thread never
/// blocks -- if a shutdown is already queued, one signal is enough.
pub fn install_shutdown_channel() -> Receiver<ShutdownReason> {
    let (tx, rx) = sync_channel(4);
    // If a channel is already installed (a second mount in one process, which
    // only tests do), keep the first: the main thread is waiting on it.
    let _ = SHUTDOWN_TX.set(tx);
    rx
}

pub fn state() -> LifeState {
    LifeState::from_u8(STATE.load(Ordering::SeqCst))
}

/// The only question the hot path asks.
///
/// Once past RUNNING, **touch nothing**: return `STATUS_INTERNAL_ERROR`
/// immediately, do not acquire the state lock, do not read or mutate
/// filesystem state (ADR-0013a).
#[inline]
pub fn accepting_work() -> bool {
    STATE.load(Ordering::SeqCst) == LifeState::Running as u8
}

/// Called only from a dispatcher thread. Sets state and signals; performs **no
/// teardown**.
pub fn enter_poisoning() {
    STATE.store(LifeState::Poisoning as u8, Ordering::SeqCst);
    if let Some(tx) = SHUTDOWN_TX.get() {
        // try_send, never send: a dispatcher thread must not block here, and a
        // full queue means a shutdown is already on its way.
        let _ = tx.try_send(ShutdownReason::Poisoned);
    }
    STATE.store(LifeState::Poisoned as u8, Ordering::SeqCst);
}

/// Request a clean shutdown (Ctrl-C). Safe from any thread.
pub fn request_shutdown() {
    if let Some(tx) = SHUTDOWN_TX.get() {
        let _ = tx.try_send(ShutdownReason::Requested);
    }
}

/// Main thread only: begin teardown.
pub fn enter_stopping() {
    STATE.store(LifeState::Stopping as u8, Ordering::SeqCst);
}

/// Main thread only: teardown complete.
pub fn enter_unmounted() {
    STATE.store(LifeState::Unmounted as u8, Ordering::SeqCst);
}

/// Has the filesystem been poisoned? Used by shutdown to decide whether the
/// exit is clean or a contained failure.
pub fn is_poisoned() -> bool {
    matches!(state(), LifeState::Poisoning | LifeState::Poisoned)
}

/// Test-only: return to RUNNING so the next test starts from a known state.
///
/// Production never resets: a poisoned filesystem shuts the process down, and
/// the next `RUNNING` comes from a new process.
#[cfg(test)]
pub fn reset_for_test() {
    STATE.store(LifeState::Running as u8, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lifecycle is process-global, so these tests must not run
    /// concurrently with each other or with the FFI tests that poison it.
    /// `crate::ffi::tests::SERIAL` is the lock they all share.
    #[test]
    fn the_state_machine_advances_in_one_direction() {
        let _g = crate::ffi::test_support::serial();
        reset_for_test();

        assert_eq!(state(), LifeState::Running);
        assert!(accepting_work());

        enter_poisoning();
        // POISONING is published and then POISONED; both refuse work.
        assert_eq!(state(), LifeState::Poisoned);
        assert!(!accepting_work());
        assert!(is_poisoned());

        enter_stopping();
        assert_eq!(state(), LifeState::Stopping);
        assert!(!accepting_work());

        enter_unmounted();
        assert_eq!(state(), LifeState::Unmounted);
        assert!(!accepting_work());

        reset_for_test();
    }

    #[test]
    fn poisoning_never_blocks_even_with_no_channel_installed() {
        // A dispatcher thread must never block in enter_poisoning. With no
        // channel installed at all, it must still return promptly.
        let _g = crate::ffi::test_support::serial();
        reset_for_test();
        let started = std::time::Instant::now();
        enter_poisoning();
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        reset_for_test();
    }

    #[test]
    fn repeated_poisoning_is_idempotent() {
        let _g = crate::ffi::test_support::serial();
        reset_for_test();
        enter_poisoning();
        enter_poisoning();
        enter_poisoning();
        assert_eq!(state(), LifeState::Poisoned);
        reset_for_test();
    }
}
