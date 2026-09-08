//! `space-client` -- the Phase 1 filesystem host (manual §4.3).
//!
//! Startup, in order:
//!
//! ```text
//! parse args -> load and validate config -> init logging
//!   -> space_core_start(config_path)
//!   -> space_adapter_mount(L"S:", cfg.client.dispatcher_threads)
//!   -> block on the shutdown channel
//! ```
//!
//! Shutdown, **on the main thread only**, triggered by Ctrl-C or by the poison
//! signal (ADR-0013a):
//!
//! ```text
//! STOPPING -> space_adapter_unmount() -> space_core_stop() -> UNMOUNTED -> exit
//! ```
//!
//! The main thread owns teardown because `FspFileSystemStopDispatcher` waits
//! for dispatcher threads to drain, including the caller. A dispatcher thread
//! that tried to tear down would deadlock against itself, so the poison path
//! signals and returns instead.

use std::time::Duration;

use space_client_core::ffi::{self, lifecycle};
use space_client_core::startup::{self, Args, Startup};

// ---------------------------------------------------------------------------
// The C++ adapter (ADR-0007: compiled by build.rs into this binary)
// ---------------------------------------------------------------------------

extern "C" {
    fn space_adapter_mount(mount_point: *const u16, dispatcher_threads: u32) -> u32;
    fn space_adapter_unmount();
}

// ---------------------------------------------------------------------------
// Ctrl-C, without a dependency
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod console {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    /// Runs on a control thread supplied by Windows. It only signals -- all
    /// teardown belongs to the main thread (ADR-0013a).
    unsafe extern "system" fn handler(_ctrl_type: u32) -> i32 {
        super::lifecycle::request_shutdown();
        1 // handled
    }

    pub fn install() {
        // SAFETY: `handler` is a valid `extern "system"` function pointer and
        // outlives the process.
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(not(windows))]
mod console {
    pub fn install() {}
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(argv));
}

fn run(argv: Vec<String>) -> i32 {
    let args: Args = match startup::parse_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    let log_dir = std::env::var("SPACE_CLIENT_LOG_DIR")
        .ok()
        .map(std::path::PathBuf::from);
    let sink = match &log_dir {
        Some(d) => contracts::logging::LogSink::Directory(d),
        None => contracts::logging::LogSink::Stderr,
    };

    // Steps 1-4: args, config, logging, startup line.
    let cfg = match startup::prepare(&args, sink) {
        Startup::PrintedVersion => return 0,
        Startup::ConfigRejected(e) => {
            let code = match e.code {
                contracts::ErrorCode::ConfigMissing => "CONFIG_MISSING",
                contracts::ErrorCode::ConfigUnsupportedVersion => "CONFIG_UNSUPPORTED_VERSION",
                _ => "CONFIG_INVALID",
            };
            eprintln!("{code}: {}", e.message);
            return 1;
        }
        Startup::Ready(cfg) => cfg,
    };

    // The shutdown channel must exist before anything can panic into it.
    let shutdown = lifecycle::install_shutdown_channel();
    console::install();

    // Bring the core up from the already-validated config, rather than making
    // the FFI entry point re-read the file. One parse, one validation.
    if let Err(e) = ffi::start_with_config(&cfg) {
        tracing::error!(error_code = ?e.code, "core startup failed: {}", e.message);
        eprintln!("core startup failed: {e}");
        return 1;
    }

    let mount_point = format!("{}:", cfg.client.mount_drive_letter);
    let wide_mount = wide(&mount_point);

    // SAFETY: `wide_mount` is a NUL-terminated UTF-16 buffer that outlives the
    // call; the adapter copies what it needs.
    let status = unsafe { space_adapter_mount(wide_mount.as_ptr(), cfg.client.dispatcher_threads) };
    if status != 0 {
        tracing::error!(
            operation = "mount",
            result = "error",
            ntstatus = format!("{status:#010X}"),
            mount_point = %mount_point,
            "mount failed"
        );
        eprintln!("mount failed: NTSTATUS {status:#010X}");
        let _ = ffi::space_core_stop();
        return 1;
    }

    tracing::info!(
        operation = "mount",
        result = "ok",
        mount_point = %mount_point,
        dispatcher_threads = cfg.client.dispatcher_threads,
        callback_timeout_ms = cfg.client.callback_timeout_ms,
        "mounted"
    );
    println!("SPACE mounted at {mount_point}. Press Ctrl-C to unmount.");

    // Block until Ctrl-C or a poison signal. In-flight callbacks continue to
    // run on dispatcher threads while this thread waits.
    let reason = shutdown
        .recv()
        .unwrap_or(lifecycle::ShutdownReason::Requested);

    match reason {
        lifecycle::ShutdownReason::Poisoned => tracing::error!(
            operation = "shutdown",
            reason = "poisoned",
            "the filesystem was poisoned by a contained panic; unmounting"
        ),
        lifecycle::ShutdownReason::Requested => tracing::info!(
            operation = "shutdown",
            reason = "requested",
            "shutdown requested; unmounting"
        ),
    }

    // ---- teardown, main thread only (ADR-0013a) ----
    let deadline = Duration::from_millis(cfg.client.shutdown_deadline_ms);
    let started = std::time::Instant::now();

    lifecycle::enter_stopping();

    // Ordering is load-bearing: stop dispatcher, remove mount point, delete.
    // SAFETY: called from the main thread, exactly once.
    unsafe { space_adapter_unmount() };

    let _ = ffi::space_core_stop();
    lifecycle::enter_unmounted();

    let elapsed = started.elapsed();
    if elapsed > deadline {
        tracing::error!(
            operation = "shutdown",
            result = "error",
            error_code = "OPERATION_TIMEOUT",
            elapsed_ms = elapsed.as_millis() as u64,
            deadline_ms = cfg.client.shutdown_deadline_ms,
            "shutdown exceeded its deadline"
        );
        return 1;
    }

    tracing::info!(
        operation = "shutdown",
        result = "ok",
        elapsed_ms = elapsed.as_millis() as u64,
        "clean shutdown"
    );

    // A poisoned filesystem shut down cleanly, but it did not shut down
    // *healthily*: the exit code says so, because a supervisor restarting the
    // process needs to know the difference.
    if reason == lifecycle::ShutdownReason::Poisoned {
        3
    } else {
        0
    }
}
