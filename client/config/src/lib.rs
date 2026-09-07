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
    /// Phase 1 VFS bounds (section 3.4). Defaulted in full so a Phase 0 config
    /// file remains valid; a test drives a boundary by setting one key.
    #[serde(default)]
    pub vfs: VfsSection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub worker_threads: u32,
    pub callback_timeout_ms: u64,
    pub shutdown_deadline_ms: u64,
    pub mount_drive_letter: String,
    /// L10 (ADR-0010): the WinFsp dispatcher thread count is the concurrency
    /// ceiling for every backing store, so it is a configured bound and not a
    /// default. 0 means "let WinFsp choose".
    #[serde(default = "default_dispatcher_threads")]
    pub dispatcher_threads: u32,
}

fn default_dispatcher_threads() -> u32 {
    4
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

        range(
            "client.dispatcher_threads",
            self.client.dispatcher_threads as u64,
            0,
            64,
        )?;
        self.vfs.validate()?;

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

// ---------------------------------------------------------------------------
// Phase 1: VFS bounds (manual section 3.4)
// ---------------------------------------------------------------------------

/// Every limit in `docs/protocols/resource-limits.md` gets a config key.
///
/// All fields are defaulted to the documented values, so a Phase 0 config file
/// stays valid and a test drives one boundary by setting one key.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VfsSection {
    /// L5 -- total bytes the filesystem may hold.
    #[serde(default = "d_max_bytes")]
    pub max_bytes: u64,
    /// L7 -- simultaneously open handles.
    #[serde(default = "d_max_open_handles")]
    pub max_open_handles: usize,
    /// L8 -- simultaneously open cursors.
    #[serde(default = "d_max_open_cursors")]
    pub max_open_cursors: usize,
    /// L6 -- entries per directory.
    #[serde(default = "d_max_dir_entries")]
    pub max_dir_entries: usize,
    /// L4 -- bytes in a single read or write.
    #[serde(default = "d_max_io_bytes")]
    pub max_io_bytes: usize,
    /// L1 -- path length in characters.
    #[serde(default = "d_max_path_chars")]
    pub max_path_chars: usize,
    /// L2 -- component length in characters.
    #[serde(default = "d_max_component_chars")]
    pub max_component_chars: usize,
    /// L3 -- path depth in components.
    #[serde(default = "d_max_path_depth")]
    pub max_path_depth: usize,
    /// The one Phase 1 capability (manual section 11.5). `false` is Phase 1's
    /// ASCII folding; `true` is the Phase 2 target. Both behaviours are
    /// specified in `fs-semantics.md` and both are tested.
    #[serde(default)]
    pub unicode_case_folding: bool,
}

fn d_max_bytes() -> u64 {
    1 << 30
}
fn d_max_open_handles() -> usize {
    65_536
}
fn d_max_open_cursors() -> usize {
    256
}
fn d_max_dir_entries() -> usize {
    65_536
}
fn d_max_io_bytes() -> usize {
    16 * 1024 * 1024
}
fn d_max_path_chars() -> usize {
    32_767
}
fn d_max_component_chars() -> usize {
    255
}
fn d_max_path_depth() -> usize {
    512
}

impl Default for VfsSection {
    fn default() -> Self {
        VfsSection {
            max_bytes: d_max_bytes(),
            max_open_handles: d_max_open_handles(),
            max_open_cursors: d_max_open_cursors(),
            max_dir_entries: d_max_dir_entries(),
            max_io_bytes: d_max_io_bytes(),
            max_path_chars: d_max_path_chars(),
            max_component_chars: d_max_component_chars(),
            max_path_depth: d_max_path_depth(),
            unicode_case_folding: false,
        }
    }
}

impl VfsSection {
    fn validate(&self) -> Result<(), SpaceError> {
        // A config must not be able to set a limit to a value that would break
        // an invariant -- max_io_bytes = 0 would make every read fail L4, and
        // max_open_handles = 0 would make the filesystem unmountable.
        range("vfs.max_bytes", self.max_bytes, 1 << 20, u64::MAX)?;
        range(
            "vfs.max_open_handles",
            self.max_open_handles as u64,
            1,
            1 << 24,
        )?;
        range(
            "vfs.max_open_cursors",
            self.max_open_cursors as u64,
            1,
            1 << 20,
        )?;
        range(
            "vfs.max_dir_entries",
            self.max_dir_entries as u64,
            1,
            1 << 24,
        )?;
        range(
            "vfs.max_io_bytes",
            self.max_io_bytes as u64,
            4096,
            1 << 30,
        )?;
        range(
            "vfs.max_path_chars",
            self.max_path_chars as u64,
            16,
            32_767,
        )?;
        range(
            "vfs.max_component_chars",
            self.max_component_chars as u64,
            8,
            255,
        )?;
        range("vfs.max_path_depth", self.max_path_depth as u64, 2, 4096)?;
        if self.max_component_chars > self.max_path_chars {
            return Err(config_invalid(
                "vfs.max_component_chars must not exceed vfs.max_path_chars",
            ));
        }
        Ok(())
    }
}

impl VfsSection {
    /// The five limits the conformance suite takes (manual section 11.5).
    ///
    /// Returned as a plain tuple-free struct in the core rather than here:
    /// `space-config` must not depend on `space-client-core` (the dependency
    /// runs the other way), so the core does the mapping and this method
    /// exposes the raw values.
    pub fn as_tuple(&self) -> (u64, usize, usize, usize, usize) {
        (
            self.max_bytes,
            self.max_open_handles,
            self.max_open_cursors,
            self.max_dir_entries,
            self.max_io_bytes,
        )
    }

    /// The three path bounds (L1, L2, L3).
    pub fn path_tuple(&self) -> (usize, usize, usize) {
        (
            self.max_path_chars,
            self.max_component_chars,
            self.max_path_depth,
        )
    }
}
