//! The `Vfs` contract (Phase 1 §3.2).
//!
//! This module is the interface Phase 2 implements. Phase 1 ships exactly one
//! implementation, [`memvfs::MemVfs`]; Phase 2 writes the second and must pass
//! the same conformance suite without modifying it (ADR-0011).
//!
//! The trait is **synchronous** (ADR-0010): a future network-backed
//! implementation blocks on a runtime internally, so this interface survives
//! Phase 4 unchanged. The consequence is that the WinFsp dispatcher thread
//! count is the concurrency ceiling for every backing store, which is why it is
//! a configured bound and not a default.

use contracts::{ErrorCode, RequestId, SpaceError};

pub mod config_map;
pub mod ids;
pub mod invariants;
pub mod limits;
pub mod memvfs;
pub mod path;
pub mod coverage_tests;
pub mod properties;
pub mod types;

pub use config_map::VfsSectionExt;
pub use ids::{CursorId, HandleId};
pub use invariants::{InvariantViolation, VfsDiagnostics};
pub use limits::{Limits, PathLimits};
pub use path::{FoldedName, VfsPath};
pub use types::{
    BasicInfoPatch, Capabilities, CleanupFlags, CreateOptions, DirEntry, FileInfo, OpenOptions,
    Opened, Probe, VfsConfig, VolumeInfo, WriteMode,
};

/// The version of the contract defined by §3.2, §3.3, §3.4, §3.5, §3.6 and the
/// `Capabilities` set.
///
/// Changing any of them requires a bump **and** an ADR (ADR-0011).
pub const VFS_CONTRACT_VERSION: u32 = 1;

/// Per-operation context, created at the FFI boundary and threaded everywhere.
///
/// Carries the two things every operation needs and neither layer should have
/// to rediscover: who is asking (for tracing) and how long they may take
/// (ADR-0009).
#[derive(Clone, Debug)]
pub struct OpCtx {
    pub request_id: RequestId,
    pub deadline: std::time::Instant,
}

impl OpCtx {
    /// Build a context with `timeout` remaining from now.
    pub fn new(timeout: std::time::Duration) -> Self {
        Self {
            request_id: RequestId::new(),
            deadline: std::time::Instant::now() + timeout,
        }
    }

    /// Build a context with an explicit deadline.
    pub fn with_deadline(deadline: std::time::Instant) -> Self {
        Self {
            request_id: RequestId::new(),
            deadline,
        }
    }

    /// Time left, or `None` if the deadline has passed.
    pub fn remaining(&self) -> Option<std::time::Duration> {
        self.deadline
            .checked_duration_since(std::time::Instant::now())
    }

    /// `OperationTimeout` once the deadline has passed (ADR-0014: the only
    /// timeout Phase 1 can produce).
    pub fn check(&self) -> Result<(), SpaceError> {
        match self.remaining() {
            Some(_) => Ok(()),
            None => Err(SpaceError::new(
                ErrorCode::OperationTimeout,
                "operation deadline exceeded",
            )),
        }
    }
}

/// The filesystem contract.
///
/// Note that `cleanup`, `can_delete` and `rename` take the handle and **not** a
/// source path: the handle already identifies the node, and accepting a
/// redundant path invites the two to disagree. WinFsp supplies a `FileName` on
/// these callbacks; the adapter uses it only for logging.
pub trait Vfs: Send + Sync {
    fn contract_version(&self) -> u32 {
        VFS_CONTRACT_VERSION
    }

    fn volume_info(&self, cx: &OpCtx) -> Result<VolumeInfo, SpaceError>;

    /// Existence + attributes probe. WinFsp calls this before create/open.
    fn probe(&self, cx: &OpCtx, path: &VfsPath) -> Result<Probe, SpaceError>;

    fn create(&self, cx: &OpCtx, path: &VfsPath, o: CreateOptions) -> Result<Opened, SpaceError>;
    fn open(&self, cx: &OpCtx, path: &VfsPath, o: OpenOptions) -> Result<Opened, SpaceError>;
    fn overwrite(
        &self,
        cx: &OpCtx,
        h: HandleId,
        attrs: u32,
        replace_attrs: bool,
        alloc: u64,
    ) -> Result<FileInfo, SpaceError>;
    fn cleanup(&self, cx: &OpCtx, h: HandleId, flags: CleanupFlags);
    fn close(&self, cx: &OpCtx, h: HandleId);

    /// Fills `buf`, returns bytes transferred. `&mut [u8]` so the WinFsp buffer
    /// is written directly -- no intermediate allocation, now or in Phase 4.
    fn read(&self, cx: &OpCtx, h: HandleId, offset: u64, buf: &mut [u8])
        -> Result<u32, SpaceError>;
    fn write(
        &self,
        cx: &OpCtx,
        h: HandleId,
        offset: u64,
        buf: &[u8],
        mode: WriteMode,
    ) -> Result<(u32, FileInfo), SpaceError>;
    fn flush(&self, cx: &OpCtx, h: Option<HandleId>) -> Result<Option<FileInfo>, SpaceError>;

    fn file_info(&self, cx: &OpCtx, h: HandleId) -> Result<FileInfo, SpaceError>;
    fn set_basic_info(
        &self,
        cx: &OpCtx,
        h: HandleId,
        patch: BasicInfoPatch,
    ) -> Result<FileInfo, SpaceError>;
    fn set_file_size(
        &self,
        cx: &OpCtx,
        h: HandleId,
        new_size: u64,
        set_allocation: bool,
    ) -> Result<FileInfo, SpaceError>;

    fn can_delete(&self, cx: &OpCtx, h: HandleId) -> Result<(), SpaceError>;
    fn rename(&self, cx: &OpCtx, h: HandleId, to: &VfsPath, replace: bool)
        -> Result<(), SpaceError>;

    fn dir_open(
        &self,
        cx: &OpCtx,
        h: HandleId,
        pattern: Option<&str>,
        marker: Option<&str>,
    ) -> Result<CursorId, SpaceError>;
    fn dir_next(&self, cx: &OpCtx, cursor: CursorId) -> Result<Option<DirEntry>, SpaceError>;
    fn dir_close(&self, cx: &OpCtx, cursor: CursorId);

    /// Parse a path using this implementation's configured limits and
    /// capabilities.
    ///
    /// Path parsing goes through the implementation rather than
    /// `VfsPath::parse` directly so that a configured limit cannot disagree
    /// with the one actually enforced.
    fn parse_path(&self, s: &str) -> Result<VfsPath, SpaceError>;
}

/// Read-only structural self-check. Required of every `Vfs` implementation
/// (§3.5).
pub trait VfsDiagnosticsExt: Vfs + VfsDiagnostics {}
impl<T: Vfs + VfsDiagnostics> VfsDiagnosticsExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn contract_version_is_one() {
        // ADR-0011. Bumping this without an ADR is the failure this guards.
        assert_eq!(VFS_CONTRACT_VERSION, 1);
    }

    #[test]
    fn op_ctx_check_yields_operation_timeout_past_the_deadline() {
        let cx = OpCtx::with_deadline(std::time::Instant::now() - Duration::from_millis(1));
        assert!(cx.remaining().is_none());
        assert_eq!(
            cx.check().unwrap_err().code,
            ErrorCode::OperationTimeout,
            "an expired deadline must produce OperationTimeout, never NetworkTimeout"
        );
    }

    #[test]
    fn op_ctx_check_passes_before_the_deadline() {
        let cx = OpCtx::new(Duration::from_secs(30));
        assert!(cx.remaining().is_some());
        assert!(cx.check().is_ok());
    }

    #[test]
    fn each_op_ctx_gets_a_distinct_request_id() {
        let a = OpCtx::new(Duration::from_secs(1));
        let b = OpCtx::new(Duration::from_secs(1));
        assert_ne!(a.request_id, b.request_id);
    }
}
