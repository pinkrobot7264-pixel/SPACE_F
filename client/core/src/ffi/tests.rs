//! FFI boundary tests (§2.5).
//!
//! **Treat the boundary as hostile.** A successful call is not enough: the
//! boundary must also fail safely. Every test here drives the `extern "C"`
//! entry points exactly as the C++ adapter does.
//!
//! These tests share process-global state (the core singleton and the
//! `LifeState` atomic), so each takes `test_support::serial()` first.

use std::ptr;

use super::lifecycle::{self, LifeState};
use super::ntstatus::*;
use super::test_support::{serial, start_test_core, stop_test_core, wide};
use super::types::{SpaceDirEntry, SpaceFileInfo, SpaceVolumeInfo};
use super::*;

// ---------------------------------------------------------------------------
// Strings and nullability (§2.5)
// ---------------------------------------------------------------------------

#[test]
fn wstr_accepts_valid_ascii() {
    let w = wide("\\dir\\file.txt");
    assert_eq!(unsafe { wstr(w.as_ptr()) }.unwrap(), "\\dir\\file.txt");
}

#[test]
fn wstr_accepts_a_valid_surrogate_pair() {
    // U+1F600, which is a surrogate pair in UTF-16.
    let w = wide("\\emoji-\u{1F600}.txt");
    assert_eq!(
        unsafe { wstr(w.as_ptr()) }.unwrap(),
        "\\emoji-\u{1F600}.txt"
    );
}

#[test]
fn wstr_rejects_an_unpaired_surrogate() {
    // A lone high surrogate is not valid UTF-16.
    let w: Vec<u16> = vec![0x005C, 0xD800, 0x0000];
    let e = unsafe { wstr(w.as_ptr()) }.unwrap_err();
    assert_eq!(e.code, ErrorCode::ObjectNameInvalid);
}

#[test]
fn wstr_accepts_the_empty_string() {
    let w: Vec<u16> = vec![0];
    assert_eq!(unsafe { wstr(w.as_ptr()) }.unwrap(), "");
}

#[test]
fn wstr_rejects_null() {
    let e = unsafe { wstr(ptr::null()) }.unwrap_err();
    assert_eq!(e.code, ErrorCode::ObjectNameInvalid);
}

#[test]
fn wstr_rejects_40000_chars_without_a_nul_and_does_not_crash() {
    // The length cap is not decoration: a missing NUL terminator would
    // otherwise walk memory until it faults. The buffer here is deliberately
    // *not* NUL-terminated within the cap.
    let w: Vec<u16> = vec![0x0041; 40_000];
    let e = unsafe { wstr(w.as_ptr()) }.unwrap_err();
    assert_eq!(e.code, ErrorCode::NameTooLong);
}

#[test]
fn wstr_opt_maps_null_to_none() {
    assert_eq!(unsafe { wstr_opt(ptr::null()) }.unwrap(), None);
    let w = wide("*.txt");
    assert_eq!(
        unsafe { wstr_opt(w.as_ptr()) }.unwrap(),
        Some("*.txt".to_string())
    );
}

#[test]
fn a_null_out_handle_is_invalid_parameter_and_never_dereferenced() {
    let _g = serial();
    start_test_core();

    let path = wide("\\nullout.txt");
    let mut info = SpaceFileInfo::default();
    let status = unsafe {
        space_core_create(
            path.as_ptr(),
            0,
            0,
            0,
            0,
            ptr::null_mut(), // out_handle
            &mut info,
        )
    };
    assert_eq!(status, STATUS_INVALID_PARAMETER);

    // ...and the same for out_info.
    let mut handle: SpaceHandle = 0;
    let status =
        unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut handle, ptr::null_mut()) };
    assert_eq!(status, STATUS_INVALID_PARAMETER);

    stop_test_core();
}

#[test]
fn every_out_pointer_is_checked() {
    let _g = serial();
    start_test_core();

    assert_eq!(
        unsafe { space_core_get_volume_info(ptr::null_mut()) },
        STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { space_core_get_file_info(1, ptr::null_mut()) },
        STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { space_core_read(1, ptr::null_mut(), 0, 0, ptr::null_mut()) },
        STATUS_INVALID_PARAMETER
    );
    let mut n: u32 = 0;
    assert_eq!(
        unsafe { space_core_write(1, ptr::null(), 0, 0, 0, 0, &mut n, ptr::null_mut()) },
        STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { space_core_dir_open(1, ptr::null(), ptr::null(), ptr::null_mut()) },
        STATUS_INVALID_PARAMETER
    );
    let mut e = SpaceDirEntry::default();
    assert_eq!(
        unsafe { space_core_dir_next(1, &mut e, ptr::null_mut()) },
        STATUS_INVALID_PARAMETER
    );

    stop_test_core();
}

#[test]
fn a_null_path_is_rejected_without_dereferencing() {
    let _g = serial();
    start_test_core();

    let mut handle: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    let status = unsafe { space_core_create(ptr::null(), 0, 0, 0, 0, &mut handle, &mut info) };
    assert_eq!(status, STATUS_OBJECT_NAME_INVALID);

    stop_test_core();
}

// ---------------------------------------------------------------------------
// Round trip through the boundary
// ---------------------------------------------------------------------------

#[test]
fn a_file_round_trips_through_the_c_abi() {
    let _g = serial();
    start_test_core();

    let path = wide("\\rt.txt");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    assert_eq!(
        unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) },
        STATUS_SUCCESS
    );
    assert_ne!(h, 0, "a valid handle is never 0 (INV-ID-5)");
    assert_eq!(info.file_size, 0);

    let data = b"boundary round trip";
    let mut transferred: u32 = 0;
    let mut winfo = SpaceFileInfo::default();
    assert_eq!(
        unsafe {
            space_core_write(
                h,
                data.as_ptr() as *const std::ffi::c_void,
                0,
                data.len() as u32,
                0,
                0,
                &mut transferred,
                &mut winfo,
            )
        },
        STATUS_SUCCESS
    );
    assert_eq!(transferred as usize, data.len());
    assert_eq!(winfo.file_size, data.len() as u64);

    let mut buf = vec![0u8; data.len()];
    let mut got: u32 = 0;
    assert_eq!(
        unsafe {
            space_core_read(
                h,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                0,
                data.len() as u32,
                &mut got,
            )
        },
        STATUS_SUCCESS
    );
    assert_eq!(got as usize, data.len());
    assert_eq!(&buf, data);

    unsafe { space_core_cleanup(h, ptr::null(), 0) };
    space_core_close(h);

    stop_test_core();
}

#[test]
fn volume_info_crosses_the_boundary() {
    let _g = serial();
    start_test_core();
    let mut vi = SpaceVolumeInfo::default();
    assert_eq!(
        unsafe { space_core_get_volume_info(&mut vi) },
        STATUS_SUCCESS
    );
    assert!(
        vi.free_size <= vi.total_size,
        "free_size must not exceed total_size"
    );
    assert!(vi.total_size > 0);
    stop_test_core();
}

#[test]
fn directory_enumeration_crosses_the_boundary() {
    let _g = serial();
    start_test_core();

    // Create two files in the root.
    for name in ["\\alpha.txt", "\\beta.txt"] {
        let p = wide(name);
        let mut h: SpaceHandle = 0;
        let mut i = SpaceFileInfo::default();
        assert_eq!(
            unsafe { space_core_create(p.as_ptr(), 0, 0, 0, 0, &mut h, &mut i) },
            STATUS_SUCCESS
        );
        unsafe { space_core_cleanup(h, ptr::null(), 0) };
        space_core_close(h);
    }

    let root = wide("\\");
    let mut dh: SpaceHandle = 0;
    let mut di = SpaceFileInfo::default();
    assert_eq!(
        unsafe { space_core_open(root.as_ptr(), 0x1, 0, &mut dh, &mut di) },
        STATUS_SUCCESS
    );

    let mut cursor: SpaceCursor = 0;
    assert_eq!(
        unsafe { space_core_dir_open(dh, ptr::null(), ptr::null(), &mut cursor) },
        STATUS_SUCCESS
    );
    assert_ne!(cursor, 0, "a valid cursor is never 0 (INV-ID-5)");

    let mut names = Vec::new();
    loop {
        let mut entry = SpaceDirEntry::default();
        let mut more: u8 = 0;
        assert_eq!(
            unsafe { space_core_dir_next(cursor, &mut entry, &mut more) },
            STATUS_SUCCESS
        );
        if more == 0 {
            break;
        }
        assert!(!entry.name.is_null(), "an entry name must not be null");
        // Boundary rule 3: the name is borrowed and valid until the next
        // dir_next on this cursor -- read it now, exactly as the adapter does.
        let mut len = 0usize;
        while unsafe { *entry.name.add(len) } != 0 {
            len += 1;
        }
        let slice = unsafe { std::slice::from_raw_parts(entry.name, len) };
        names.push(String::from_utf16(slice).unwrap());
    }
    space_core_dir_close(cursor);
    space_core_close(dh);

    assert_eq!(names, vec!["alpha.txt", "beta.txt"]);
    stop_test_core();
}

// ---------------------------------------------------------------------------
// Lengths (boundary rule 6)
// ---------------------------------------------------------------------------

#[test]
fn a_length_above_l4_is_rejected_before_any_allocation() {
    let _g = serial();
    start_test_core();

    let path = wide("\\big.bin");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) };

    // A length far above L4 with a *null* buffer: the length must be rejected
    // before the buffer is ever looked at, so this must not fault.
    let mut got: u32 = 0;
    let status = unsafe { space_core_read(h, ptr::null_mut(), 0, u32::MAX, &mut got) };
    assert_eq!(status, STATUS_INVALID_PARAMETER);
    assert_eq!(got, 0);

    let mut winfo = SpaceFileInfo::default();
    let status =
        unsafe { space_core_write(h, ptr::null(), 0, u32::MAX, 0, 0, &mut got, &mut winfo) };
    assert_eq!(status, STATUS_INVALID_PARAMETER);

    unsafe { space_core_cleanup(h, ptr::null(), 0) };
    space_core_close(h);
    stop_test_core();
}

#[test]
fn a_null_buffer_with_a_nonzero_length_is_invalid_parameter() {
    let _g = serial();
    start_test_core();

    let path = wide("\\nb.bin");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) };

    let mut got: u32 = 0;
    assert_eq!(
        unsafe { space_core_read(h, ptr::null_mut(), 0, 16, &mut got) },
        STATUS_INVALID_PARAMETER
    );

    // ...but a null buffer with length 0 is a legal no-op.
    assert_eq!(
        unsafe { space_core_read(h, ptr::null_mut(), 0, 0, &mut got) },
        STATUS_SUCCESS
    );
    assert_eq!(got, 0);

    unsafe { space_core_cleanup(h, ptr::null(), 0) };
    space_core_close(h);
    stop_test_core();
}

// ---------------------------------------------------------------------------
// Invalid identifiers (INV-ID-3, INV-ID-5)
// ---------------------------------------------------------------------------

#[test]
fn forged_handles_and_cursors_never_resolve_across_the_boundary() {
    let _g = serial();
    start_test_core();

    let mut info = SpaceFileInfo::default();
    for raw in [0u64, 1, 2, u64::MAX, 0xDEAD_BEEF_CAFE_BABE, 0x1_0000_0000] {
        assert_eq!(
            unsafe { space_core_get_file_info(raw, &mut info) },
            STATUS_INVALID_HANDLE,
            "forged handle {raw:#x} resolved"
        );
        // The void entry points must be silent no-ops, never a crash.
        unsafe { space_core_cleanup(raw, ptr::null(), 1) };
        space_core_close(raw);
        space_core_dir_close(raw);
    }

    stop_test_core();
}

#[test]
fn a_double_close_is_a_silent_no_op() {
    let _g = serial();
    start_test_core();

    let path = wide("\\dc.txt");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) };
    unsafe { space_core_cleanup(h, ptr::null(), 0) };
    space_core_close(h);
    // Second and third close: no panic, no double free.
    space_core_close(h);
    space_core_close(h);

    stop_test_core();
}

// ---------------------------------------------------------------------------
// The panic model (ADR-0013) -- must pass in debug AND release
// ---------------------------------------------------------------------------

/// A guarded entry point that panics on demand, so the panic path can be driven
/// exactly as a real callback would hit it.
fn guarded_panic() -> SpaceStatus {
    guard("test_panic", Trace::none(), |_cx, _core, _tr| {
        panic!("deliberate test panic");
    })
}

#[test]
fn a_panic_inside_a_guarded_entry_point_returns_internal_error() {
    let _g = serial();
    start_test_core();

    let status = guarded_panic();
    assert_eq!(
        status, STATUS_INTERNAL_ERROR,
        "a panic must be converted, not propagated"
    );

    stop_test_core();
}

#[test]
fn a_panic_poisons_the_filesystem() {
    let _g = serial();
    start_test_core();
    assert_eq!(lifecycle::state(), LifeState::Running);

    guarded_panic();

    // RUNNING -> POISONING -> POISONED (ADR-0013a).
    assert!(lifecycle::is_poisoned());
    assert_eq!(lifecycle::state(), LifeState::Poisoned);

    stop_test_core();
}

#[test]
fn the_next_callback_after_a_panic_fails_immediately() {
    let _g = serial();
    start_test_core();

    guarded_panic();

    // Every subsequent callback returns STATUS_INTERNAL_ERROR immediately,
    // without acquiring the state lock or touching filesystem state.
    let mut vi = SpaceVolumeInfo::default();
    assert_eq!(
        unsafe { space_core_get_volume_info(&mut vi) },
        STATUS_INTERNAL_ERROR
    );
    assert_eq!(vi, SpaceVolumeInfo::default(), "poisoned state was read");

    let path = wide("\\after-panic.txt");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    assert_eq!(
        unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) },
        STATUS_INTERNAL_ERROR
    );
    assert_eq!(h, 0, "a poisoned create must not produce a handle");

    stop_test_core();
}

#[test]
fn close_and_cleanup_after_poisoning_are_silent_no_ops() {
    let _g = serial();
    start_test_core();

    // Open something first, so there is real state a careless no-op could touch.
    let path = wide("\\poisoned.txt");
    let mut h: SpaceHandle = 0;
    let mut info = SpaceFileInfo::default();
    unsafe { space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info) };

    guarded_panic();

    // These are `void` and cannot report failure, so they must return silently
    // without touching poisoned state. The memory is a deliberate, bounded leak
    // reclaimed by the imminent process exit (ADR-0013a).
    unsafe { space_core_cleanup(h, ptr::null(), 1) };
    space_core_close(h);
    space_core_dir_close(1);

    assert_eq!(lifecycle::state(), LifeState::Poisoned);
    stop_test_core();
}

#[test]
fn poisoning_from_a_dispatcher_thread_does_not_deadlock() {
    // The real shape of the failure: WinFsp calls the boundary on a dispatcher
    // thread, that thread panics, and the poison path must set the flag and
    // return promptly rather than attempting teardown itself. Teardown on that
    // thread would call StopDispatcher, which waits for dispatcher threads to
    // drain -- including the caller.
    let _g = serial();
    start_test_core();

    let started = std::time::Instant::now();
    let t = std::thread::spawn(guarded_panic);
    let status = t
        .join()
        .expect("the poisoning thread must not itself panic");
    let elapsed = started.elapsed();

    assert_eq!(status, STATUS_INTERNAL_ERROR);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "poisoning from a dispatcher thread took {elapsed:?} -- looks like a deadlock"
    );
    assert!(lifecycle::is_poisoned());

    stop_test_core();
}

#[test]
fn the_panic_message_and_request_id_reach_the_log() {
    // §2.5: "panic message + request_id in the log: present at error level".
    // The log line is the only diagnostic a panic leaves behind, which is the
    // entire reason ADR-0013 catches rather than aborts.
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    let _g = serial();
    start_test_core();

    let cap = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(cap.clone())
        .with_max_level(tracing::Level::ERROR)
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        guarded_panic();
    });

    let out = String::from_utf8_lossy(&cap.0.lock().unwrap().clone()).to_string();
    assert!(
        out.contains("PANIC at FFI boundary"),
        "the panic was not logged: {out:?}"
    );
    assert!(
        out.contains("deliberate test panic"),
        "the panic message was lost: {out:?}"
    );
    assert!(
        out.contains("request_id"),
        "the request_id was not carried into the panic log line: {out:?}"
    );
    assert!(
        out.contains("ERROR"),
        "the panic must log at error level: {out:?}"
    );

    stop_test_core();
}

// ---------------------------------------------------------------------------
// Timeouts (§2.5)
// ---------------------------------------------------------------------------

#[test]
fn an_expired_deadline_yields_status_io_timeout() {
    let _g = serial();
    start_test_core();

    // Drive the deadline to zero so every callback is already past it.
    CALLBACK_TIMEOUT_MS.store(0, std::sync::atomic::Ordering::Relaxed);

    let mut vi = SpaceVolumeInfo::default();
    let status = unsafe { space_core_get_volume_info(&mut vi) };
    assert_eq!(
        status, STATUS_IO_TIMEOUT,
        "an expired deadline must surface as STATUS_IO_TIMEOUT"
    );

    CALLBACK_TIMEOUT_MS.store(30_000, std::sync::atomic::Ordering::Relaxed);
    stop_test_core();
}

#[test]
fn no_core_path_can_return_network_timeout() {
    // ADR-0014, the grep half of the assertion (§2.5). The conformance half is
    // in conformance/errors.rs.
    //
    // NetworkTimeout is reserved for the Phase 4+ transfer engine. If it ever
    // appears in the VFS or FFI source, a network concern has leaked into the
    // Windows-facing layer.
    // The check is for *construction*, `ErrorCode::NetworkTimeout`, not for the
    // bare word. Prose that names the rule -- a doc comment, or an assertion
    // message reading "never NetworkTimeout" -- is the discipline working, and
    // flagging it would push the next person to delete the explanation to get
    // the test green. That would be gaming the test in reverse.
    //
    // The bare variant name is only reachable through a glob import, so the
    // absence of one is asserted too rather than assumed; otherwise
    // `use ErrorCode::*;` would silently reopen the hole.
    for (name, src) in [
        ("ffi/mod.rs", include_str!("mod.rs")),
        ("vfs/memvfs/imp.rs", include_str!("../vfs/memvfs/imp.rs")),
        ("vfs/path.rs", include_str!("../vfs/path.rs")),
        (
            "vfs/memvfs/table.rs",
            include_str!("../vfs/memvfs/table.rs"),
        ),
        ("vfs/mod.rs", include_str!("../vfs/mod.rs")),
    ] {
        assert!(
            !src.contains("ErrorCode::NetworkTimeout"),
            "{name} constructs NetworkTimeout; ADR-0014 forbids the VFS layer \
             from producing it -- a network-backed implementation surfaces \
             OperationTimeout at the VFS boundary instead"
        );
        assert!(
            !src.contains("use ErrorCode::*") && !src.contains("use contracts::ErrorCode::*"),
            "{name} glob-imports ErrorCode, which would let NetworkTimeout be \
             named without the ErrorCode:: prefix this test looks for"
        );
    }

    // `ffi/ntstatus.rs` is the one deliberate exception, and it is stated here
    // rather than left as a silent gap in the list above: the mapping table
    // *maps* NetworkTimeout because the taxonomy is total and the Phase 4
    // transfer engine will produce it. Mapping a code is not producing one, and
    // the table shares STATUS_IO_TIMEOUT between the two timeout codes on
    // purpose (ADR-0015: totality, not injectivity).
    let mapping = include_str!("ntstatus.rs");
    assert!(
        mapping.contains("NetworkTimeout => STATUS_IO_TIMEOUT"),
        "the mapping table must still carry NetworkTimeout; the taxonomy is total"
    );
}

// ---------------------------------------------------------------------------
// Startup and shutdown
// ---------------------------------------------------------------------------

#[test]
fn calls_before_start_are_rejected_rather_than_crashing() {
    let _g = serial();
    stop_test_core(); // ensure no core is installed

    let mut vi = SpaceVolumeInfo::default();
    let status = unsafe { space_core_get_volume_info(&mut vi) };
    assert_eq!(
        status, STATUS_INTERNAL_ERROR,
        "a call before space_core_start must be refused, not dereference a null core"
    );

    // The void entry points, too.
    space_core_close(1);
    unsafe { space_core_cleanup(1, ptr::null(), 1) };
    space_core_dir_close(1);
}

#[test]
fn stop_is_idempotent() {
    let _g = serial();
    start_test_core();
    assert_eq!(space_core_stop(), STATUS_SUCCESS);
    assert_eq!(space_core_stop(), STATUS_SUCCESS);
    assert_eq!(lifecycle::state(), LifeState::Unmounted);
    lifecycle::reset_for_test();
}

#[test]
fn start_with_a_missing_config_reports_a_startup_status() {
    let _g = serial();
    stop_test_core();
    let path = wide("C:\\SPACE\\does-not-exist\\config.toml");
    let status = unsafe { space_core_start(path.as_ptr()) };
    assert_ne!(status, STATUS_SUCCESS);
    // Startup-only codes have no meaningful NTSTATUS, so the boundary reports
    // "the filesystem never came up" (ADR-0015).
    assert_eq!(status, STATUS_DEVICE_NOT_READY);
    stop_test_core();
}

#[test]
fn start_with_a_null_config_path_is_rejected() {
    let _g = serial();
    stop_test_core();
    let status = unsafe { space_core_start(ptr::null()) };
    assert_eq!(status, STATUS_OBJECT_NAME_INVALID);
    stop_test_core();
}
