//! Fault-injection tests (§13.1, §13.4, §13.5).
//!
//! These run only with the `fault-injection` feature, which is dev-only; CI
//! asserts it is off in release builds.
//!
//! The §13.2/§13.3 half of fault testing -- Explorer stays responsive, unmount
//! still succeeds, `os-safety-check.ps1` is clean -- is driven through a real
//! mount by `scripts/fault-injection-test.ps1`, because it is a claim about
//! Windows and cannot be made from inside the process.

#![cfg(feature = "fault-injection")]

use std::time::{Duration, Instant};

use contracts::ErrorCode;
use faults::FaultAction;

use super::lifecycle::{self, LifeState};
use super::ntstatus::*;
use super::test_support::{serial, start_test_core, stop_test_core, wide};
use super::types::SpaceFileInfo;
use super::*;

/// Create a file and leave it open; returns its handle.
fn open_a_file() -> SpaceHandle {
    let path = wide("\\fault.txt");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    let s = unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) };
    assert_eq!(s, STATUS_SUCCESS);
    h
}

fn read_once(h: SpaceHandle) -> SpaceStatus {
    let mut buf = [0u8; 16];
    let mut got: u32 = 0;
    unsafe {
        space_core_read(
            h,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            0,
            buf.len() as u32,
            &mut got,
        )
    }
}

// ---------------------------------------------------------------------------
// The fault set is wired up at all (§13.1)
// ---------------------------------------------------------------------------

#[test]
fn every_phase_1_fault_point_is_reachable_from_the_boundary() {
    // A registered point that no code consults is a point that will silently
    // stop working. Each of the eight must map to an operation.
    for point in faults::PHASE_1_FAULT_POINTS {
        let found = [
            "read",
            "write",
            "open",
            "create",
            "dir_open",
            "get_file_info",
            "rename",
            "cleanup",
        ]
        .iter()
        .any(|op| fault_point_for(op) == Some(*point));
        assert!(found, "fault point {point} is registered but unreachable");
    }
}

#[test]
fn corrupt_bytes_is_not_armable() {
    // §13.1, asserted here as well as in the faults crate because this is the
    // layer that would consume it.
    let _g = serial();
    assert!(faults::arm("winfsp_pre_read", FaultAction::CorruptBytes).is_err());
}

// ---------------------------------------------------------------------------
// Invariant-targeting faults (§13.5)
// ---------------------------------------------------------------------------

#[test]
fn fail_returns_the_injected_code() {
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    faults::arm("winfsp_pre_read", FaultAction::Fail(ErrorCode::DiskFull)).unwrap();
    assert_eq!(read_once(h), STATUS_DISK_FULL);
    faults::disarm_all();

    // ...and the filesystem is fine afterwards.
    assert_eq!(read_once(h), STATUS_END_OF_FILE); // empty file
    stop_test_core();
}

#[test]
fn stale_handle_targets_inv_id_3_and_changes_no_state() {
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    // Write something first so there is state a fault could damage.
    let data = b"unchanged";
    let mut n: u32 = 0;
    let mut winfo = SpaceFileInfo::default();
    unsafe {
        space_core_write(
            h,
            data.as_ptr() as *const std::ffi::c_void,
            0,
            data.len() as u32,
            0,
            0,
            &mut n,
            &mut winfo,
        )
    };

    faults::arm("winfsp_pre_read", FaultAction::StaleHandle).unwrap();
    assert_eq!(read_once(h), STATUS_INVALID_HANDLE);
    faults::disarm_all();

    // State byte-identical: the handle still works and the bytes are intact.
    let mut buf = vec![0u8; data.len()];
    let mut got: u32 = 0;
    let s = unsafe {
        space_core_read(
            h,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            0,
            data.len() as u32,
            &mut got,
        )
    };
    assert_eq!(s, STATUS_SUCCESS);
    assert_eq!(&buf, data, "an injected stale handle mutated file content");

    stop_test_core();
}

#[test]
fn resource_exhausted_targets_inv_res_2_and_3() {
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    faults::arm("winfsp_pre_write", FaultAction::ResourceExhausted).unwrap();
    let data = b"rejected";
    let mut n: u32 = 0;
    let mut winfo = SpaceFileInfo::default();
    let s = unsafe {
        space_core_write(
            h,
            data.as_ptr() as *const std::ffi::c_void,
            0,
            data.len() as u32,
            0,
            0,
            &mut n,
            &mut winfo,
        )
    };
    assert_eq!(s, STATUS_INSUFFICIENT_RESOURCES);
    assert_eq!(n, 0, "a rejected write reported a transfer");
    faults::disarm_all();

    // INV-RES-3: state exactly as before -- the file is still empty.
    let mut info = SpaceFileInfo::default();
    unsafe { space_core_get_file_info(h, &mut info) };
    assert_eq!(info.file_size, 0, "a rejected write changed the file size");

    stop_test_core();
}

#[test]
fn invalid_input_is_a_specific_code_never_a_panic() {
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    faults::arm("winfsp_pre_getinfo", FaultAction::InvalidInput).unwrap();
    let mut info = SpaceFileInfo::default();
    let s = unsafe { space_core_get_file_info(h, &mut info) };
    assert_eq!(s, STATUS_INVALID_PARAMETER);
    faults::disarm_all();

    // Not poisoned: an injected error is not a panic.
    assert_eq!(lifecycle::state(), LifeState::Running);
    stop_test_core();
}

#[test]
fn cancel_maps_to_status_cancelled_with_no_partial_mutation() {
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    faults::arm("winfsp_pre_write", FaultAction::Cancel).unwrap();
    let data = b"cancelled";
    let mut n: u32 = 0;
    let mut winfo = SpaceFileInfo::default();
    let s = unsafe {
        space_core_write(
            h,
            data.as_ptr() as *const std::ffi::c_void,
            0,
            data.len() as u32,
            0,
            0,
            &mut n,
            &mut winfo,
        )
    };
    assert_eq!(s, STATUS_CANCELLED);
    faults::disarm_all();

    let mut info = SpaceFileInfo::default();
    unsafe { space_core_get_file_info(h, &mut info) };
    assert_eq!(info.file_size, 0, "a cancelled write partially applied");

    stop_test_core();
}

// ---------------------------------------------------------------------------
// Deadlines (§13.2, §13.4)
// ---------------------------------------------------------------------------

#[test]
fn a_hang_returns_within_the_callback_deadline() {
    // ADR-0009: the deadline is the only bound that exists. Without it a hung
    // callback blocks a WinFsp dispatcher thread forever and the kernel waits
    // as long as user mode takes.
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    CALLBACK_TIMEOUT_MS.store(500, std::sync::atomic::Ordering::Relaxed);
    faults::arm("winfsp_pre_read", FaultAction::Hang).unwrap();

    let started = Instant::now();
    let s = read_once(h);
    let elapsed = started.elapsed();

    faults::disarm_all();
    CALLBACK_TIMEOUT_MS.store(30_000, std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        s, STATUS_IO_TIMEOUT,
        "a hang must surface as STATUS_IO_TIMEOUT"
    );
    assert!(
        elapsed < Duration::from_millis(500 + 500),
        "callback took {elapsed:?}, past callback_timeout_ms + 500ms"
    );
    assert!(
        elapsed >= Duration::from_millis(400),
        "returned at {elapsed:?}, well before the deadline -- the bound is not real"
    );

    stop_test_core();
}

#[test]
fn the_bound_scales_with_the_configured_timeout() {
    // §13.3: "confirm the bound scales with configuration -- proof that the
    // deadline is real rather than an artefact of fast operations."
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    let mut timings = Vec::new();
    for timeout_ms in [300u64, 1200] {
        CALLBACK_TIMEOUT_MS.store(timeout_ms, std::sync::atomic::Ordering::Relaxed);
        faults::arm("winfsp_pre_read", FaultAction::Hang).unwrap();
        let started = Instant::now();
        let s = read_once(h);
        let elapsed = started.elapsed();
        faults::disarm_all();
        assert_eq!(s, STATUS_IO_TIMEOUT);
        timings.push((timeout_ms, elapsed));
    }
    CALLBACK_TIMEOUT_MS.store(30_000, std::sync::atomic::Ordering::Relaxed);

    let (short_ms, short) = timings[0];
    let (long_ms, long) = timings[1];
    assert!(
        long > short,
        "a {long_ms}ms deadline ({long:?}) did not take longer than a {short_ms}ms one ({short:?})"
    );
    // The longer bound should be recognisably longer, not marginally so.
    assert!(
        long >= Duration::from_millis(1000),
        "the 1200ms bound returned after only {long:?}"
    );

    stop_test_core();
}

#[test]
fn a_near_miss_delay_still_succeeds() {
    // §13.4: "A deadline that fires early is as much a defect as one that never
    // fires, and it is the failure you would otherwise meet under Phase 4
    // network latency."
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    CALLBACK_TIMEOUT_MS.store(1000, std::sync::atomic::Ordering::Relaxed);
    faults::arm(
        "winfsp_pre_read",
        FaultAction::Delay(Duration::from_millis(1000 - 800)),
    )
    .unwrap();

    let s = read_once(h);
    faults::disarm_all();
    CALLBACK_TIMEOUT_MS.store(30_000, std::sync::atomic::Ordering::Relaxed);

    // The file is empty, so EndOfFile is the correct success-path answer here:
    // what matters is that it is NOT a timeout.
    assert_ne!(
        s, STATUS_IO_TIMEOUT,
        "a delay comfortably inside the deadline was reported as a timeout"
    );
    assert_eq!(s, STATUS_END_OF_FILE);

    stop_test_core();
}

// ---------------------------------------------------------------------------
// The Panic row (§13.5) -- run last in the session; it ends the mount
// ---------------------------------------------------------------------------

#[test]
fn an_injected_panic_follows_the_adr_0013_path() {
    let _g = serial();
    start_test_core();
    let h = open_a_file();

    faults::arm("winfsp_pre_read", FaultAction::Panic).unwrap();
    let s = read_once(h);
    faults::disarm_all();

    // Contained, converted, poisoned.
    assert_eq!(s, STATUS_INTERNAL_ERROR);
    assert!(lifecycle::is_poisoned(), "an injected panic did not poison");

    // Subsequent callbacks fail immediately without touching state.
    let mut info = SpaceFileInfo::default();
    assert_eq!(
        unsafe { space_core_get_file_info(h, &mut info) },
        STATUS_INTERNAL_ERROR
    );
    // cleanup/close become silent no-ops.
    unsafe { space_core_cleanup(h, std::ptr::null(), 1) };
    space_core_close(h);

    stop_test_core();
}
