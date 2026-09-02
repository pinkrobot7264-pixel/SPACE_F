//! `space-client` -- the Phase 0 client skeleton (M0.11).
//!
//! It loads and validates config, brings up structured logging with redaction,
//! logs one startup line, then idles until Ctrl-C and shuts down within the
//! configured deadline. There is deliberately no filesystem mount yet.

#[tokio::main]
async fn main() {
    let argv = std::env::args().skip(1).collect::<Vec<_>>();
    let code = space_client_core::startup::run(argv).await;
    std::process::exit(code);
}
