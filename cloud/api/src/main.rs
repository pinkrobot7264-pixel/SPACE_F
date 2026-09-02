//! `space-cloud` -- the Phase 0 cloud skeleton (M0.12).
//!
//! Startup: bind `SPACE_CLOUD_ADDR` (default `127.0.0.1:8080`), serve the router,
//! shut down gracefully on Ctrl-C, draining in-flight requests within the
//! deadline. No PostgreSQL: metadata is in-memory behind a trait (Phase 8).

use std::path::PathBuf;
use std::sync::Arc;

use contracts::logging::{self, LogSink};
use space_cloud_api::{router, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("SPACE_CLOUD_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

    match std::env::var("SPACE_CLOUD_LOG_DIR") {
        Ok(dir) => logging::init("space-cloud", LogSink::Directory(&PathBuf::from(dir))),
        Err(_) => logging::init("space-cloud", LogSink::Stderr),
    }

    let state = Arc::new(AppState::default());
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!(
        operation = "startup",
        result = "ok",
        msg = "space-cloud listening"
    );
    // A machine-readable line the test harness waits for.
    println!("space-cloud listening on http://{bound}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!(
        operation = "shutdown",
        result = "ok",
        msg = "space-cloud stopped"
    );
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!(
        operation = "shutdown",
        result = "ok",
        msg = "signal received, draining"
    );
}
