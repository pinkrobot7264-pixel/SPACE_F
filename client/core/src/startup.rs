//! Client startup sequence (M0.11).
//!
//! Order matters and mirrors the dependency order of the crates it uses:
//!  1. parse args (`--config <path>`, `--version`)
//!  2. load + validate config (M0.5)
//!  3. initialise logging with redaction (M0.6)
//!  4. mint a startup `RequestId`, log version + a **redacted** config summary
//!  5. install a Ctrl-C handler
//!  6. idle
//!  7. on signal: clean shutdown within a bounded deadline, exit 0
//!
//! No WinFsp. No mount. No drive letter.

use std::path::PathBuf;
use std::time::Duration;

use contracts::logging::{self, LogSink};
use contracts::{ErrorCode, RequestId, SpaceError};
use space_config::Config;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub config: PathBuf,
    pub print_version: bool,
}

/// Parse `--config <path>` / `--version`. Unknown flags are an error.
pub fn parse_args<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, SpaceError> {
    let mut config = PathBuf::from("config.toml");
    let mut print_version = false;
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--version" | "-V" => print_version = true,
            "--config" => {
                config = it
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| SpaceError::invalid_param("--config requires a path"))?;
            }
            other => {
                return Err(SpaceError::invalid_param(format!(
                    "unknown argument: {other}"
                )));
            }
        }
    }
    Ok(Args {
        config,
        print_version,
    })
}

/// Outcome of a startup attempt, so tests can assert without inspecting stderr.
pub enum Startup {
    PrintedVersion,
    ConfigRejected(SpaceError),
    Ready(Box<Config>),
}

/// Steps 1-4. Returns the validated config (or the reason it was rejected)
/// without entering the idle loop, so it is unit-testable.
pub fn prepare(args: &Args, log_sink: LogSink<'_>) -> Startup {
    if args.print_version {
        println!("space-client {VERSION}");
        return Startup::PrintedVersion;
    }

    let cfg = match Config::load(&args.config) {
        Ok(c) => c,
        Err(e) => return Startup::ConfigRejected(e),
    };

    logging::init("space-client", log_sink);

    let startup_id = RequestId::new();
    tracing::info!(
        request_id = %startup_id,
        operation = "startup",
        result = "ok",
        version = VERSION,
        worker_threads = cfg.client.worker_threads,
        chunk_size_bytes = cfg.chunking.chunk_size_bytes,
        cloud_base_url = %cfg.cloud.base_url,
        mount_drive_letter = %cfg.client.mount_drive_letter,
        msg = "client starting (no mount in Phase 0)"
    );

    Startup::Ready(Box::new(cfg))
}

/// The full binary entry point. Returns the process exit code.
pub async fn run<I: IntoIterator<Item = String>>(argv: I) -> i32 {
    let args = match parse_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    let log_dir = std::env::var("SPACE_CLIENT_LOG_DIR")
        .ok()
        .map(PathBuf::from);
    let sink = match &log_dir {
        Some(d) => LogSink::Directory(d),
        None => LogSink::Stderr,
    };

    let cfg = match prepare(&args, sink) {
        Startup::PrintedVersion => return 0,
        Startup::ConfigRejected(e) => {
            let code = match e.code {
                ErrorCode::ConfigMissing => "CONFIG_MISSING",
                ErrorCode::ConfigUnsupportedVersion => "CONFIG_UNSUPPORTED_VERSION",
                _ => "CONFIG_INVALID",
            };
            // stderr line carries the specific code; the message is already
            // redaction-safe (config.load rejects credential-bearing files).
            eprintln!("{code}: {}", e.message);
            return 1;
        }
        Startup::Ready(cfg) => cfg,
    };

    let deadline = Duration::from_millis(cfg.client.shutdown_deadline_ms);
    let _ = tokio::signal::ctrl_c().await;

    let shutdown = tokio::time::timeout(deadline, async {
        tracing::info!(
            operation = "shutdown",
            result = "ok",
            msg = "clean shutdown"
        );
    })
    .await;

    if shutdown.is_err() {
        tracing::error!(
            operation = "shutdown",
            result = "error",
            error_code = "CANCELLED",
            msg = "shutdown exceeded deadline"
        );
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_handles_the_supported_flags() {
        assert!(parse_args(["--version".to_string()]).unwrap().print_version);
        let a = parse_args(["--config".to_string(), "x.toml".to_string()]).unwrap();
        assert_eq!(a.config, PathBuf::from("x.toml"));
        assert!(parse_args(["--bogus".to_string()]).is_err());
        assert!(parse_args(["--config".to_string()]).is_err());
    }

    #[test]
    fn missing_config_is_reported_with_its_code() {
        let args = Args {
            config: PathBuf::from("no/such/config.toml"),
            print_version: false,
        };
        match prepare(&args, LogSink::Stderr) {
            Startup::ConfigRejected(e) => assert_eq!(e.code, ErrorCode::ConfigMissing),
            _ => panic!("expected rejection"),
        }
    }
}
