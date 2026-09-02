//! M0.6 (T0.6) -- the structured-logging schema and structural redaction.
//!
//! `logging::init` installs a process-global subscriber, so this whole file is
//! one test (nextest gives it its own process).

use std::io::Write;

use contracts::logging::{self, LogSink, REQUIRED_FIELDS};
use contracts::redaction::{sanitize_url, Token};
use contracts::{RequestId, SpaceError};

#[test]
fn every_line_is_json_with_all_required_keys_and_redaction_holds() {
    let (writer, buf) = logging::memory_sink();
    logging::init("space-test", LogSink::Buffer(writer));

    let token: Token = Token::new("wJalrXUtnFEMI-super-secret".to_string());
    assert_eq!(format!("{token}"), "[redacted]");
    assert_eq!(format!("{token:?}"), "[redacted]");
    assert_eq!(
        sanitize_url("https://s3/obj?X-Amz-Signature=deadbeef"),
        "https://s3/obj?[redacted]"
    );

    let rid = RequestId::new().to_string();
    let span = tracing::info_span!("op", request_id = %rid, file_id = "f_demo");
    let _g = span.enter();

    tracing::info!(
        operation = "read_chunk",
        duration_ms = 12u64,
        result = "ok",
        chunk_id = "b3:00",
        msg = "completed"
    );
    tracing::error!(
        operation = "commit",
        result = "error",
        error_code = "INTEGRITY_MANIFEST_INVALID",
        // a secret handed to the logger must still not appear
        secret = %token,
        msg = "commit rejected"
    );

    drop(_g);

    let bytes = buf.lock().unwrap().clone();
    let text = String::from_utf8(bytes).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert!(lines.len() >= 2, "expected at least two log lines");

    let mut saw_ok = false;
    let mut saw_err = false;
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
        for &key in REQUIRED_FIELDS {
            assert!(v.get(key).is_some(), "line missing key `{key}`: {line}");
        }
        assert_eq!(v["component"], "space-test");
        assert!(
            !line.contains("wJalrXUtnFEMI"),
            "secret leaked into a log line"
        );

        if v["result"] == "ok" {
            saw_ok = true;
            assert_eq!(v["request_id"], rid, "request_id propagates from the span");
            assert_eq!(v["file_id"], "f_demo");
            assert_eq!(v["operation"], "read_chunk");
            assert_eq!(v["duration_ms"], 12);
            assert_eq!(v["msg"], "completed");
        }
        if v["result"] == "error" {
            saw_err = true;
            assert_eq!(v["error_code"], "INTEGRITY_MANIFEST_INVALID");
            assert_eq!(v["request_id"], rid);
        }
    }
    assert!(saw_ok && saw_err);
}

#[test]
fn space_error_display_never_prints_a_secret() {
    let e = SpaceError::new(contracts::ErrorCode::AuthFailed, "credential rejected");
    let mut sink = Vec::new();
    write!(sink, "{e}").unwrap();
    let s = String::from_utf8(sink).unwrap();
    assert!(s.contains("AuthFailed") && !s.contains("wJalrXUtnFEMI"));
}
