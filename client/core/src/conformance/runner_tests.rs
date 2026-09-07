//! Runs the conformance suite against `MemVfs` (§11.2).
//!
//! **No WinFsp, no mount.** This is the job CI runs on a runner without WinFsp
//! installed; if it ever needs WinFsp, something leaked from the adapter into
//! the core.
//!
//! The suite runs **twice** -- once per capability configuration -- because a
//! capability selects between documented behaviours and every
//! capability-dependent test must assert its own configuration's specified
//! behaviour (§11.5, rule 1).

use crate::conformance::run_conformance_suite;
use crate::vfs::limits::{Limits, PathLimits};
use crate::vfs::memvfs::MemVfs;
use crate::vfs::types::{Capabilities, VfsConfig};

/// Limits sized so every boundary is reachable in seconds.
///
/// `max_io_bytes` keeps its real value because §8.1 tests it directly. The rest
/// are lowered so the limit and limit+1 cases run quickly -- which is exactly
/// why `Limits` is passed to the suite rather than hard-coded inside it.
fn test_limits() -> Limits {
    Limits {
        max_bytes: 32 * 1024 * 1024,
        max_open_handles: 512,
        max_open_cursors: 64,
        max_dir_entries: 256,
        max_io_bytes: crate::vfs::limits::MAX_IO_BYTES,
    }
}

fn config(caps: Capabilities) -> VfsConfig {
    VfsConfig {
        limits: test_limits(),
        path_limits: PathLimits::DEFAULT,
        capabilities: caps,
    }
}

#[test]
fn conformance_suite_passes_with_ascii_case_folding() {
    let caps = Capabilities::PHASE_1;
    let cfg = config(caps);
    let vfs = MemVfs::new(cfg);
    run_conformance_suite(&vfs, caps, cfg.limits);
}

#[test]
fn conformance_suite_passes_with_unicode_case_folding() {
    // The Phase 2 target configuration. Running it now is what makes
    // `unicode_case_folding` a capability rather than a gap: both variants are
    // specified in fs-semantics §7, and both are tested here.
    let caps = Capabilities::PHASE_2_TARGET;
    let cfg = config(caps);
    let vfs = MemVfs::new(cfg);
    run_conformance_suite(&vfs, caps, cfg.limits);
}

#[test]
fn the_suite_needs_no_windows_types() {
    // §11.7: "no VFS method returns a Windows type; the suite compiles without
    // any Windows crate". That property is enforced by compilation -- this test
    // records the requirement so a future `windows` dependency in this crate is
    // a deliberate act with a test to argue with.
    //
    // The FFI layer does name NTSTATUS values, but as plain u32 constants
    // (ADR-0015), not via a Windows crate.
    let manifest = include_str!("../../Cargo.toml");
    for forbidden in ["windows-sys", "winapi", "windows ="] {
        assert!(
            !manifest.contains(forbidden),
            "space-client-core gained a Windows crate dependency ({forbidden}); \
             the no-WinFsp conformance job depends on its absence"
        );
    }
}

#[test]
fn contract_version_is_reported_by_the_implementation() {
    use crate::vfs::Vfs;
    let vfs = MemVfs::with_defaults();
    assert_eq!(vfs.contract_version(), crate::vfs::VFS_CONTRACT_VERSION);
}
