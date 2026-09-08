//! The VFS conformance suite (Phase 1 §11).
//!
//! ```text
//!                 Vfs contract (§3.2, §3.3, §3.4, §3.5, §3.6)
//!                             │
//!                  ┌──────────┴──────────┐
//!               MemVfs                Real VFS
//!              (Phase 1)              (Phase 2)
//!                  └──────────┬──────────┘
//!                      same conformance suite
//! ```
//!
//! **The suite runs with no WinFsp installed and no mount.** Consequently it
//! runs on any CI runner, in parallel, in seconds; a failure points at your
//! logic rather than a drive letter, Explorer, or the kernel.
//!
//! This module imports nothing from the WinFsp adapter and compiles without any
//! Windows crate (§11.2, §11.7). CI runs it as a separate job on a runner
//! without WinFsp; if that job ever needs WinFsp, something leaked.
//!
//! **The Phase 1 -> Phase 2 acceptance test is that this suite passes
//! unmodified against the real VFS.** If it cannot, Phase 1 froze the wrong
//! contract.

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::Capabilities;
use crate::vfs::Vfs;

pub mod boundaries;
pub mod create_open;
pub mod directory;
pub mod errors;
pub mod handles;
pub mod invariants;
pub mod lifecycle;
pub mod metadata;
pub mod naming;
pub mod read_write;
pub mod rename_delete;

mod util;

#[cfg(test)]
mod runner_tests;
pub use util::{cx, expect_err, step, Ctx};

/// Run every conformance module against one implementation.
///
/// The caller supplies the `Capabilities` and `Limits` the implementation was
/// built with. The suite never assumes a value; it reads them, so §3.4 stays
/// the single source of truth for the numbers (§14.3).
pub fn run_conformance_suite<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    lifecycle::all(vfs, caps, limits); // §3.3.1, §3.3.2
    create_open::all(vfs, caps, limits); // §3.3.3
    read_write::all(vfs, caps, limits); // §3.3.4
    rename_delete::all(vfs, caps, limits); // §3.3.5
    metadata::all(vfs, caps, limits); // §3.3.6
    naming::all(vfs, caps, limits); // §3.3.7
    directory::all(vfs, caps, limits); // §3.3.8
    handles::all(vfs, caps, limits); // §3.1
    boundaries::all(vfs, caps, limits); // §3.4
    invariants::all(vfs, caps, limits); // §3.5
    errors::all(vfs, caps, limits); // ADR-0014
}
