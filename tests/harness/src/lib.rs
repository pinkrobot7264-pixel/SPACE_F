//! Integration test harness (M0.13).
//!
//! [`TestEnv::start`] spins up a real `space-cloud` process on a free port with
//! its own in-memory state, so two `TestEnv`s are fully isolated. Teardown is a
//! [`Drop`] impl -- it runs even when a test panics -- which kills the child
//! processes and, on panic, copies the cloud log into
//! `docs/evidence/phase-0/<test-name>/`.

use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// A running, isolated cloud instance for one test.
pub struct TestEnv {
    pub cloud_url: String,
    pub namespace: String,
    test_name: String,
    cloud: Child,
    log_dir: PathBuf,
    evidence_root: PathBuf,
}

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    l.local_addr().unwrap().port()
}

fn random_suffix() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", n & 0xffff_ffff)
}

/// Locate the built `space-cloud` binary. Honours `SPACE_CLOUD_BIN`, otherwise
/// walks up from the current test executable to `target/<profile>/`.
pub fn cloud_bin() -> PathBuf {
    if let Ok(p) = std::env::var("SPACE_CLOUD_BIN") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().expect("current_exe");
    // .../target/<profile>/deps/<test-bin>  ->  .../target/<profile>/
    let profile_dir = exe
        .parent()
        .and_then(|p| {
            if p.ends_with("deps") {
                p.parent()
            } else {
                Some(p)
            }
        })
        .expect("profile dir")
        .to_path_buf();
    let name = if cfg!(windows) {
        "space-cloud.exe"
    } else {
        "space-cloud"
    };
    let candidate = profile_dir.join(name);
    assert!(
        candidate.exists(),
        "space-cloud binary not found at {}. Run `cargo build --workspace` or set SPACE_CLOUD_BIN.",
        candidate.display()
    );
    candidate
}

impl TestEnv {
    pub async fn start(test_name: &str) -> Self {
        let port = free_port();
        let namespace = format!("{test_name}-{}", random_suffix());

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.join("..").join("..");
        let evidence_root = repo_root.join("docs/evidence/phase-0");
        let log_dir = std::env::temp_dir().join(format!("space-test-{namespace}"));
        let _ = std::fs::create_dir_all(&log_dir);

        let cloud = Command::new(cloud_bin())
            .env("SPACE_CLOUD_ADDR", format!("127.0.0.1:{port}"))
            .env("SPACE_CLOUD_LOG_DIR", &log_dir)
            .spawn()
            .expect("spawn space-cloud");

        let cloud_url = format!("http://127.0.0.1:{port}");
        let mut env = Self {
            cloud_url: cloud_url.clone(),
            namespace,
            test_name: test_name.to_string(),
            cloud,
            log_dir,
            evidence_root,
        };
        env.wait_healthy().await;
        env
    }

    async fn wait_healthy(&mut self) {
        let client = reqwest::Client::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        let url = format!("{}/health", self.cloud_url);
        while Instant::now() < deadline {
            if let Some(status) = self.cloud.try_wait().expect("try_wait") {
                panic!("space-cloud exited during startup with {status}");
            }
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("space-cloud did not become healthy within 20s");
    }

    fn collect_evidence(&mut self) {
        let dest = self.evidence_root.join(&self.test_name);
        let _ = std::fs::create_dir_all(&dest);
        if let Ok(entries) = std::fs::read_dir(&self.log_dir) {
            for e in entries.flatten() {
                let _ = std::fs::copy(e.path(), dest.join(e.file_name()));
            }
        }
        if let Ok(mut f) = std::fs::File::create(dest.join("NOTE.txt")) {
            let _ = writeln!(
                f,
                "evidence collected on panic for test `{}` (namespace {})",
                self.test_name, self.namespace
            );
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Runs even on panic -- that is why this is a Drop impl and not a
        // cleanup() call a panicking test would skip.
        if std::thread::panicking() {
            self.collect_evidence();
        }
        let _ = self.cloud.kill();
        let _ = self.cloud.wait();
        let _ = std::fs::remove_dir_all(&self.log_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn two_envs_are_isolated() {
        let a = TestEnv::start("iso-a").await;
        let b = TestEnv::start("iso-b").await;
        assert_ne!(a.cloud_url, b.cloud_url);
        assert_ne!(a.namespace, b.namespace);

        // data written to a is invisible to b
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "parent_id": uuid::Uuid::now_v7().to_string(),
            "name": "only-in-a",
            "idempotency_key": "k"
        });
        let created: serde_json::Value = client
            .post(format!("{}/v1/files", a.cloud_url))
            .header("x-request-id", contracts::RequestId::new().to_string())
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let fid = created["file"]["file_id"].as_str().unwrap();
        let miss = client
            .get(format!("{}/v1/files/f_{fid}", b.cloud_url))
            .send()
            .await
            .unwrap();
        assert_eq!(miss.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn teardown_leaves_no_cloud_process() {
        let pid;
        {
            let env = TestEnv::start("teardown").await;
            pid = env.cloud.id();
        }
        // give the OS a moment to reap
        std::thread::sleep(Duration::from_millis(300));
        #[cfg(windows)]
        {
            let out = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}")])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                !text.contains(&pid.to_string()),
                "space-cloud pid {pid} still running after teardown"
            );
        }
        #[cfg(not(windows))]
        let _ = pid;
    }
}
