//! Shared E2E helpers.
#![allow(dead_code)]

use std::sync::Arc;

use space_client_core::CloudClient;
use space_cloud_api::{router, AppState};

/// A small chunk size for E2E: crosses every boundary without moving MiBs.
pub const E2E_CHUNK: u64 = 64 * 1024;

/// An in-process cloud server on a real port. Returns a client, a handle to the
/// server state (for fault injection), and a guard that shuts the server down on
/// drop.
pub struct InProcCloud {
    pub client: CloudClient,
    pub state: Arc<AppState>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl InProcCloud {
    pub async fn start() -> Self {
        let state = Arc::new(AppState::default());
        let app = router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
        // wait for readiness
        let client = CloudClient::new(format!("http://{addr}"));
        for _ in 0..100 {
            if client.health().await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Self {
            client,
            state,
            shutdown: Some(tx),
            handle: Some(handle),
        }
    }
}

impl Drop for InProcCloud {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}
