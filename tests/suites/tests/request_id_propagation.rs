//! M0.6 (T0.16) -- one `request_id` traced across process boundaries.
//!
//! ```text
//! client CloudClient::send  --X-Request-Id-->  cloud request_id_layer span
//!        |                                            |
//!        v                                            v
//!  client JSONL log  ==  same request_id  ==  cloud JSONL log
//! ```
//!
//! This is the end-to-end counterpart of the in-process schema test in
//! `logging.rs`; it does not replace it.

use std::path::Path;
use std::time::{Duration, Instant};

use contracts::logging::{self, LogSink};
use serde_json::Value;

/// Poll every `.jsonl` file in `dir` until a line matching `pred` appears or the
/// deadline passes. Returns all matching lines.
async fn wait_for_lines(
    dir: &Path,
    timeout: Duration,
    pred: impl Fn(&Value) -> bool,
) -> Vec<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut hits = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&p) {
                    for line in text.lines().filter(|l| !l.trim().is_empty()) {
                        if let Ok(v) = serde_json::from_str::<Value>(line) {
                            if pred(&v) {
                                hits.push(v);
                            }
                        }
                    }
                }
            }
        }
        if !hits.is_empty() {
            return hits;
        }
        if Instant::now() >= deadline {
            panic!(
                "no matching log line under {} within {timeout:?}",
                dir.display()
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn request_id_appears_verbatim_in_both_client_and_cloud_logs() {
    // client-side logs -> a private temp dir
    let client_log_dir =
        std::env::temp_dir().join(format!("space-client-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&client_log_dir);
    std::fs::create_dir_all(&client_log_dir).unwrap();
    logging::init("space-client", LogSink::Directory(&client_log_dir));

    // cloud-side logs -> the harness's SPACE_CLOUD_LOG_DIR (separate process)
    let env = space_test_harness::TestEnv::start("reqid-propagation").await;
    let client = space_client_core::CloudClient::new(&env.cloud_url);

    // a real contract operation (not just /health)
    client
        .create_file(contracts::DirectoryId::new(), "reqid.bin", "reqid-key")
        .await
        .expect("create_file");

    // the request_id the client actually put on the wire for POST /v1/files
    let client_lines = wait_for_lines(&client_log_dir, Duration::from_secs(8), |v| {
        v["operation"] == "cloud_call" && v["path"] == "/v1/files" && v["method"] == "POST"
    })
    .await;
    let client_rid = client_lines[0]["request_id"]
        .as_str()
        .expect("client line has request_id")
        .to_string();
    assert!(
        client_rid.starts_with("r_"),
        "client request_id is a well-formed r_ id: {client_rid}"
    );

    // the cloud must have logged the SAME id against the SAME path
    let cloud_lines = wait_for_lines(env.cloud_log_dir(), Duration::from_secs(8), |v| {
        v["operation"] == "http_request" && v["path"] == "/v1/files"
    })
    .await;
    let matched = cloud_lines
        .iter()
        .find(|v| v["request_id"].as_str() == Some(client_rid.as_str()))
        .unwrap_or_else(|| {
            panic!(
                "client request_id {client_rid} not found in cloud log; cloud lines: {:?}",
                cloud_lines
                    .iter()
                    .map(|v| v["request_id"].clone())
                    .collect::<Vec<_>>()
            )
        });

    // not a coincidence: the cloud adopted the inbound id, it did not mint one
    assert_eq!(
        matched["request_id_minted"],
        Value::Bool(false),
        "cloud must adopt the inbound request_id, not generate its own"
    );
    assert_eq!(matched["method"], "POST");
    assert_eq!(matched["component"], "space-cloud");

    drop(env);
    let _ = std::fs::remove_dir_all(&client_log_dir);
}

#[tokio::test]
async fn malformed_inbound_request_id_is_replaced_and_marked_minted() {
    let env = space_test_harness::TestEnv::start("reqid-minted").await;
    // raw request with a deliberately invalid id
    reqwest::Client::new()
        .get(format!("{}/health", env.cloud_url))
        .header("x-request-id", "not-a-valid-id")
        .send()
        .await
        .unwrap();

    let lines = wait_for_lines(env.cloud_log_dir(), Duration::from_secs(8), |v| {
        v["operation"] == "http_request" && v["path"] == "/health"
    })
    .await;
    let line = &lines[0];
    assert_eq!(line["request_id_minted"], Value::Bool(true));
    let rid = line["request_id"].as_str().unwrap();
    assert!(rid.starts_with("r_") && rid != "not-a-valid-id");

    drop(env);
}
