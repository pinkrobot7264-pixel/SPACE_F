//! Logging discipline tests (§12.2, §12.3).
//!
//! These assert the two rules that are easy to state and easy to break: file
//! content never reaches the log, and a normal existence check never logs at
//! error level.

use std::sync::{Arc, Mutex};

use super::test_support::{serial, start_test_core, stop_test_core, wide};
use super::types::SpaceFileInfo;
use super::*;

/// Captures log output so a test can assert on what was written.
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

impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap().clone()).to_string()
    }
}

fn with_capture_at(level: tracing::Level, f: impl FnOnce()) -> String {
    let cap = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(cap.clone())
        .with_max_level(level)
        .finish();
    tracing::subscriber::with_default(subscriber, f);
    cap.text()
}

#[test]
fn file_content_never_reaches_the_log() {
    // §12.3: "read a file containing a known marker string, assert the marker
    // appears nowhere in the log output."
    //
    // The marker is deliberately distinctive so a partial or encoded leak still
    // trips the assertion.
    const MARKER: &str = "SPACE-SECRET-MARKER-9d41f2c7";

    let _g = serial();
    start_test_core();

    let out = with_capture_at(tracing::Level::TRACE, || {
        let path = wide("\\secret.txt");
        let mut h: SpaceHandle = 0;
        let mut info = SpaceFileInfo::default();
        unsafe {
            space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info);
        }

        let data = MARKER.as_bytes();
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
            );
        }

        let mut buf = vec![0u8; data.len()];
        let mut got: u32 = 0;
        unsafe {
            space_core_read(
                h,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                0,
                data.len() as u32,
                &mut got,
            );
        }
        assert_eq!(&buf, data, "the test must actually move the marker bytes");

        unsafe { space_core_cleanup(h, std::ptr::null(), 0) };
        space_core_close(h);
    });

    assert!(
        !out.contains(MARKER),
        "file content leaked into the log:\n{out}"
    );
    // The offset and length ARE logged -- that is the point of the rule: the
    // parameters are diagnostic, the bytes are not.
    assert!(
        out.contains("length"),
        "the log should still carry length; only content is forbidden:\n{out}"
    );

    stop_test_core();
}

#[test]
fn a_missing_file_probe_never_logs_at_error_level() {
    // §12.2: FileNotFound from probe is the normal existence check that Create
    // depends on. Logging it at error level buries real problems.
    let _g = serial();
    start_test_core();

    let out = with_capture_at(tracing::Level::ERROR, || {
        for name in ["\\nope.txt", "\\also-missing.txt", "\\dir\\deep.txt"] {
            let path = wide(name);
            let mut h: SpaceHandle = 0;
            let mut info = SpaceFileInfo::default();
            unsafe {
                space_core_open(path.as_ptr(), 0, 0, &mut h, &mut info);
            }
            let mut attrs: u32 = 0;
            let mut size: usize = 0;
            unsafe {
                space_core_get_security_by_name(
                    path.as_ptr(),
                    &mut attrs,
                    std::ptr::null_mut(),
                    &mut size,
                );
            }
        }
    });

    assert!(
        out.trim().is_empty(),
        "an ordinary missing-file probe logged at error level:\n{out}"
    );

    stop_test_core();
}

#[test]
fn every_boundary_line_carries_a_request_id() {
    // §12.1. Without it a log line cannot be correlated to the request that
    // produced it, which is the entire purpose of minting one per callback.
    let _g = serial();
    start_test_core();

    let out = with_capture_at(tracing::Level::DEBUG, || {
        let path = wide("\\traced.txt");
        let mut h: SpaceHandle = 0;
        let mut info = SpaceFileInfo::default();
        unsafe {
            space_core_create(path.as_ptr(), 0, 0, 0, 0, &mut h, &mut info);
        }
        unsafe { space_core_get_file_info(h, &mut info) };
        unsafe { space_core_cleanup(h, std::ptr::null(), 0) };
        space_core_close(h);
    });

    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty(), "no boundary lines were logged at all");
    for line in &lines {
        assert!(
            line.contains("request_id"),
            "a boundary line has no request_id: {line}"
        );
    }

    // Including the void entry points, which cannot report failure any other
    // way and were silent until Phase 1 development proved that hides bugs.
    assert!(
        lines.iter().any(|l| l.contains("cleanup")),
        "cleanup produced no log line:\n{out}"
    );
    assert!(
        lines.iter().any(|l| l.contains("close")),
        "close produced no log line:\n{out}"
    );

    stop_test_core();
}

#[test]
fn a_panic_logs_at_error_level_with_its_request_id() {
    // The one thing that MUST reach error level (ADR-0013). The log line is the
    // only diagnostic a contained panic leaves behind, which is why the guard
    // catches rather than aborts.
    let _g = serial();
    start_test_core();

    let out = with_capture_at(tracing::Level::ERROR, || {
        guard("log_test_panic", Trace::none(), |_cx, _core, _tr| {
            panic!("deliberate logging test panic");
        });
    });

    assert!(out.contains("PANIC at FFI boundary"), "{out}");
    assert!(out.contains("request_id"), "{out}");

    stop_test_core();
}
