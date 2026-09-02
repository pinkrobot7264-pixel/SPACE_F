//! Fault-injection points (M0.6).
//!
//! A *fault point* is a named location in real code where later phases can, in
//! test builds only, ask the process to misbehave: panic, stall, return a
//! specific [`ErrorCode`], or corrupt bytes. In a release build every
//! [`fault_point`] call compiles to a no-op that returns [`FaultAction::None`].
//!
//! The canonical list of point names is `docs/protocols/fault-points.md`. The
//! names are registered there before the code they live in exists, so the crash
//! and fault-injection suites from Phase 5 onward have a stable vocabulary.
//!
//! The `fault-injection` Cargo feature gates the armable registry. It is a
//! dev-only feature; CI asserts it is off in release.

#![forbid(unsafe_code)]

use std::time::Duration;

use contracts::ErrorCode;

/// What a fault point should do when hit.
#[derive(Clone, Debug, PartialEq)]
pub enum FaultAction {
    /// Behave normally.
    None,
    /// Panic the current thread/process (crash-recovery tests).
    Panic,
    /// Sleep, then continue (timeout / backpressure tests).
    Delay(Duration),
    /// Return this error to the caller.
    Error(ErrorCode),
    /// Corrupt the bytes flowing through this point (integrity tests).
    CorruptBytes,
}

/// The canonical fault-point names. Keep in sync with
/// `docs/protocols/fault-points.md`.
pub const FAULT_POINTS: &[&str] = &[
    "post_wal_write",
    "pre_chunk_publish",
    "post_chunk_publish",
    "pre_manifest_build",
    "pre_manifest_commit",
    "post_manifest_commit",
    "mid_upload",
    "mid_download",
    "pre_cache_write",
    "post_cache_write",
    "pre_db_commit",
    "post_db_commit",
];

#[cfg(not(feature = "fault-injection"))]
mod imp {
    use super::FaultAction;

    #[inline(always)]
    pub fn fault_point(_name: &'static str) -> FaultAction {
        FaultAction::None
    }
}

#[cfg(feature = "fault-injection")]
mod imp {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::FaultAction;

    static REGISTRY: Mutex<Option<HashMap<&'static str, FaultAction>>> = Mutex::new(None);

    pub fn fault_point(name: &'static str) -> FaultAction {
        let guard = REGISTRY.lock().unwrap();
        guard
            .as_ref()
            .and_then(|m| m.get(name).cloned())
            .unwrap_or(FaultAction::None)
    }

    pub fn arm(name: &'static str, action: FaultAction) {
        assert!(
            super::FAULT_POINTS.contains(&name),
            "unknown fault point: {name} (add it to FAULT_POINTS and fault-points.md)"
        );
        let mut guard = REGISTRY.lock().unwrap();
        guard.get_or_insert_with(HashMap::new).insert(name, action);
    }

    pub fn disarm_all() {
        let mut guard = REGISTRY.lock().unwrap();
        if let Some(m) = guard.as_mut() {
            m.clear();
        }
    }
}

pub use imp::fault_point;

#[cfg(feature = "fault-injection")]
pub use imp::{arm, disarm_all};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_is_unique_and_documented_count_is_twelve() {
        let mut sorted = FAULT_POINTS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), FAULT_POINTS.len());
        assert_eq!(FAULT_POINTS.len(), 12);
    }

    #[cfg(not(feature = "fault-injection"))]
    #[test]
    fn release_build_fault_point_is_a_no_op() {
        for &name in FAULT_POINTS {
            assert_eq!(fault_point(name), FaultAction::None);
        }
    }

    #[cfg(feature = "fault-injection")]
    #[test]
    fn armed_point_returns_its_action_then_disarms() {
        arm("mid_upload", FaultAction::Error(ErrorCode::NetworkTimeout));
        assert_eq!(
            fault_point("mid_upload"),
            FaultAction::Error(ErrorCode::NetworkTimeout)
        );
        disarm_all();
        assert_eq!(fault_point("mid_upload"), FaultAction::None);
    }
}
