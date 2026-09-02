//! Typed configuration and resource bounds (M0.5).
//!
//! [`Config::load`] does three things, in order, and fails at *startup* rather
//! than hours later at first use:
//!  1. read + parse the TOML (`deny_unknown_fields`, so a typo is an error);
//!  2. check every numeric key against a hard `[min, max]` bound;
//!  3. run three cross-cutting validators -- no credentials in the file, every
//!     configured path is writable, and test-only values are refused in a
//!     release build.
//!
//! The configuration keys and their bounds are documented in
//! `config.example.toml`.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use contracts::{ErrorCode, SpaceError};
use serde::Deserialize;

/// Root configuration object.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub config_version: u32,
    pub client: ClientConfig,
    pub paths: PathsConfig,
    pub chunking: ChunkingConfig,
    pub cache: CacheConfig,
    pub transfer: TransferConfig,
    pub write: WriteConfig,
    pub scheduler: SchedulerConfig,
    pub cloud: CloudConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub worker_threads: u32,
    pub callback_timeout_ms: u64,
    pub shutdown_deadline_ms: u64,
    pub mount_drive_letter: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathsConfig {
    pub runtime_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub durable_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl PathsConfig {
    fn all(&self) -> [(&str, &Path); 4] {
        [
            ("paths.runtime_dir", &self.runtime_dir),
            ("paths.cache_dir", &self.cache_dir),
            ("paths.durable_dir", &self.durable_dir),
            ("paths.log_dir", &self.log_dir),
        ]
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkingConfig {
    pub chunk_size_bytes: u64,
    pub hash_algorithm: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    pub max_bytes: u64,
    pub eviction_policy: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferConfig {
    pub max_concurrent_requests: u32,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub max_retries: u32,
    pub backoff_base_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteConfig {
    pub max_dirty_bytes: u64,
    pub wal_fsync_policy: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    pub foreground_priority: u32,
    pub background_priority: u32,
    pub max_prefetch_requests: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudConfig {
    pub base_url: String,
    pub auth_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

fn config_invalid(msg: impl Into<String>) -> SpaceError {
    SpaceError::new(ErrorCode::ConfigInvalid, msg)
}

fn range(name: &str, value: u64, min: u64, max: u64) -> Result<(), SpaceError> {
    if value < min || value > max {
        return Err(config_invalid(format!(
            "{name} = {value} out of range [{min}, {max}]"
        )));
    }
    Ok(())
}

fn one_of(name: &str, value: &str, allowed: &[&str]) -> Result<(), SpaceError> {
    if !allowed.contains(&value) {
        return Err(config_invalid(format!(
            "{name} = {value:?} not one of {allowed:?}"
        )));
    }
    Ok(())
}

impl Config {
    /// Load, parse and fully validate a config file.
    pub fn load(path: &Path) -> Result<Self, SpaceError> {
        let text = std::fs::read_to_string(path)
            .map_err(|_| SpaceError::new(ErrorCode::ConfigMissing, "config file not found"))?;
        Self::from_str_validated(&text)
    }

    /// Same as [`Config::load`] but from an in-memory string. Used by tests.
    pub fn from_str_validated(text: &str) -> Result<Self, SpaceError> {
        reject_secret_keys(text)?;
        let cfg: Config = toml::from_str(text).map_err(|e| config_invalid(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), SpaceError> {
        if self.config_version != 1 {
            return Err(SpaceError::new(
                ErrorCode::ConfigUnsupportedVersion,
                format!(
                    "config_version {} is not supported (expected 1)",
                    self.config_version
                ),
            ));
        }

        range(
            "client.worker_threads",
            self.client.worker_threads as u64,
            1,
            64,
        )?;
        range(
            "client.callback_timeout_ms",
            self.client.callback_timeout_ms,
            100,
            300_000,
        )?;
        range(
            "client.shutdown_deadline_ms",
            self.client.shutdown_deadline_ms,
            100,
            120_000,
        )?;
        if self.client.mount_drive_letter.len() != 1
            || !self
                .client
                .mount_drive_letter
                .chars()
                .next()
                .unwrap()
                .is_ascii_alphabetic()
        {
            return Err(config_invalid(
                "client.mount_drive_letter must be a single A-Z",
            ));
        }

        range(
            "chunking.chunk_size_bytes",
            self.chunking.chunk_size_bytes,
            4096,
            1 << 30,
        )?;
        one_of(
            "chunking.hash_algorithm",
            &self.chunking.hash_algorithm,
            &["blake3"],
        )?;

        range(
            "cache.max_bytes",
            self.cache.max_bytes,
            self.chunking.chunk_size_bytes,
            u64::MAX,
        )?;
        one_of(
            "cache.eviction_policy",
            &self.cache.eviction_policy,
            &["lru", "slru"],
        )?;

        range(
            "transfer.max_concurrent_requests",
            self.transfer.max_concurrent_requests as u64,
            1,
            256,
        )?;
        range(
            "transfer.connect_timeout_ms",
            self.transfer.connect_timeout_ms,
            100,
            120_000,
        )?;
        range(
            "transfer.request_timeout_ms",
            self.transfer.request_timeout_ms,
            100,
            600_000,
        )?;
        range(
            "transfer.max_retries",
            self.transfer.max_retries as u64,
            0,
            20,
        )?;
        range(
            "transfer.backoff_base_ms",
            self.transfer.backoff_base_ms,
            1,
            60_000,
        )?;

        range(
            "write.max_dirty_bytes",
            self.write.max_dirty_bytes,
            self.chunking.chunk_size_bytes,
            u64::MAX,
        )?;
        one_of(
            "write.wal_fsync_policy",
            &self.write.wal_fsync_policy,
            &["always", "interval", "never"],
        )?;

        range(
            "scheduler.foreground_priority",
            self.scheduler.foreground_priority as u64,
            1,
            100,
        )?;
        range(
            "scheduler.background_priority",
            self.scheduler.background_priority as u64,
            1,
            100,
        )?;
        if self.scheduler.background_priority >= self.scheduler.foreground_priority {
            return Err(config_invalid(
                "scheduler.background_priority must be < scheduler.foreground_priority",
            ));
        }
        range(
            "scheduler.max_prefetch_requests",
            self.scheduler.max_prefetch_requests as u64,
            0,
            64,
        )?;

        one_of("cloud.auth_mode", &self.cloud.auth_mode, &["token", "none"])?;
        if self.cloud.base_url.split_once("://").is_none() {
            return Err(config_invalid("cloud.base_url must be an absolute URL"));
        }

        one_of(
            "logging.level",
            &self.logging.level,
            &["trace", "debug", "info", "warn", "error"],
        )?;
        one_of("logging.format", &self.logging.format, &["json"])?;

        self.check_paths_writable()?;
        self.check_release_only_rules()?;
        Ok(())
    }

    /// Every configured path must exist and accept a file *now*, not at first
    /// write hours later.
    fn check_paths_writable(&self) -> Result<(), SpaceError> {
        for (name, dir) in self.paths.all() {
            if !dir.is_dir() {
                return Err(config_invalid(format!(
                    "{name} = {} is not a directory",
                    dir.display()
                )));
            }
            let probe = dir.join(".space-write-probe");
            match std::fs::write(&probe, b"probe") {
                Ok(()) => {
                    let _ = std::fs::remove_file(&probe);
                }
                Err(_) => {
                    return Err(config_invalid(format!(
                        "{name} = {} is not writable",
                        dir.display()
                    )))
                }
            }
        }
        Ok(())
    }

    /// Test-only values must never run in a release build.
    #[cfg(not(debug_assertions))]
    fn check_release_only_rules(&self) -> Result<(), SpaceError> {
        if self.write.wal_fsync_policy == "never" {
            return Err(config_invalid(
                "write.wal_fsync_policy = \"never\" is test-only and refused in a release build",
            ));
        }
        if self.cloud.auth_mode == "none" {
            return Err(config_invalid(
                "cloud.auth_mode = \"none\" is test-only and refused in a release build",
            ));
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn check_release_only_rules(&self) -> Result<(), SpaceError> {
        Ok(())
    }
}

/// Scan the raw TOML for keys that would carry a credential. Secrets belong in
/// `C:\SPACE\secrets` or the environment, never in a config file.
pub fn reject_secret_keys(raw: &str) -> Result<(), SpaceError> {
    const SUFFIXES: &[&str] = &["_password", "_secret", "_token", "_key"];
    let value: toml::Value = raw
        .parse()
        .map_err(|e: toml::de::Error| config_invalid(e.to_string()))?;
    let mut offenders = Vec::new();
    walk_keys(&value, "", &mut |key| {
        let leaf = key.rsplit('.').next().unwrap_or(key);
        if SUFFIXES.iter().any(|s| leaf.ends_with(s)) {
            offenders.push(key.to_string());
        }
    });
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(config_invalid(format!(
            "config carries credential-like keys: {}. Move secrets out of the config file.",
            offenders.join(", ")
        )))
    }
}

fn walk_keys(value: &toml::Value, prefix: &str, f: &mut impl FnMut(&str)) {
    if let Some(table) = value.as_table() {
        for (k, v) in table {
            let path = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            f(&path);
            walk_keys(v, &path, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config.example.toml");

    fn with_example_paths(body: &str, dir: &Path) -> String {
        // Rewrite the [paths] block to point at a real temp directory.
        let d = dir.display().to_string().replace('\\', "/");
        body.replace("C:/SPACE/runtime", &d)
    }

    #[test]
    fn example_config_loads() {
        let tmp = tempfile::tempdir().unwrap();
        for sub in ["", "cache", "durable", "logs"] {
            std::fs::create_dir_all(tmp.path().join(sub)).unwrap();
        }
        let cfg = Config::from_str_validated(&with_example_paths(EXAMPLE, tmp.path())).unwrap();
        assert_eq!(cfg.config_version, 1);
        assert_eq!(cfg.chunking.chunk_size_bytes, 32 * 1024 * 1024);
    }

    fn good(dir: &Path) -> String {
        with_example_paths(EXAMPLE, dir)
    }

    fn tmp_with_dirs() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for sub in ["", "cache", "durable", "logs"] {
            std::fs::create_dir_all(tmp.path().join(sub)).unwrap();
        }
        tmp
    }

    #[test]
    fn missing_file_is_config_missing() {
        let err = Config::load(Path::new("does/not/exist.toml")).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigMissing);
    }

    #[test]
    fn invalid_toml_is_config_invalid() {
        let err = Config::from_str_validated("this is not = toml [[[").unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
    }

    #[test]
    fn truncated_mid_table_is_config_invalid() {
        let err = Config::from_str_validated("[client]\nworker_threads =").unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
    }

    #[test]
    fn wrong_version_is_unsupported() {
        let tmp = tmp_with_dirs();
        let body = good(tmp.path()).replace("config_version = 1", "config_version = 999");
        let err = Config::from_str_validated(&body).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigUnsupportedVersion);
    }

    #[test]
    fn unknown_key_is_rejected() {
        let tmp = tmp_with_dirs();
        let body = good(tmp.path()).replace(
            "worker_threads = 4",
            "wrker_threads = 4\nworker_threads = 4",
        );
        let err = Config::from_str_validated(&body).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
    }

    #[test]
    fn worker_threads_bounds() {
        let tmp = tmp_with_dirs();
        for (v, ok) in [("0", false), ("1", true), ("64", true), ("65", false)] {
            let body =
                good(tmp.path()).replace("worker_threads = 4", &format!("worker_threads = {v}"));
            assert_eq!(Config::from_str_validated(&body).is_ok(), ok, "v={v}");
        }
    }

    #[test]
    fn callback_timeout_bounds() {
        let tmp = tmp_with_dirs();
        for (v, ok) in [
            ("99", false),
            ("100", true),
            ("300000", true),
            ("300001", false),
        ] {
            let body = good(tmp.path()).replace(
                "callback_timeout_ms = 30000",
                &format!("callback_timeout_ms = {v}"),
            );
            assert_eq!(Config::from_str_validated(&body).is_ok(), ok, "v={v}");
        }
    }

    #[test]
    fn hash_algorithm_must_be_blake3() {
        let tmp = tmp_with_dirs();
        let body = good(tmp.path()).replace(
            "hash_algorithm   = \"blake3\"",
            "hash_algorithm = \"sha256\"",
        );
        assert_eq!(
            Config::from_str_validated(&body).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );
    }

    #[test]
    fn eviction_policy_rejects_random() {
        let tmp = tmp_with_dirs();
        let body =
            good(tmp.path()).replace("eviction_policy = \"slru\"", "eviction_policy = \"random\"");
        assert_eq!(
            Config::from_str_validated(&body).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );
    }

    #[test]
    fn nonexistent_runtime_dir_fails_at_startup() {
        // point paths at a dir that does not exist
        let body = EXAMPLE.replace("C:/SPACE/runtime", "C:/SPACE/definitely/not/here/xyz");
        assert_eq!(
            Config::from_str_validated(&body).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );
    }

    #[test]
    fn config_with_a_token_key_is_rejected() {
        let tmp = tmp_with_dirs();
        let mut body = good(tmp.path());
        body.push_str("\n[extra]\napi_token = \"x\"\n");
        // deny_unknown_fields would also catch [extra]; the secret scan runs first.
        let err = Config::from_str_validated(&body).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        assert!(err.message.contains("credential-like"));
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn release_build_rejects_wal_fsync_never() {
        let tmp = tmp_with_dirs();
        let body = good(tmp.path()).replace(
            "wal_fsync_policy = \"always\"",
            "wal_fsync_policy = \"never\"",
        );
        assert_eq!(
            Config::from_str_validated(&body).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );
    }

    #[test]
    fn writing_a_probe_file_actually_happens() {
        // sanity: check_paths_writable really touches the dir
        let tmp = tmp_with_dirs();
        let _ = Config::from_str_validated(&good(tmp.path())).unwrap();
        let mut leftovers = std::fs::read_dir(tmp.path()).unwrap();
        assert!(!leftovers.any(|e| e.unwrap().file_name() == ".space-write-probe"));
    }
}
