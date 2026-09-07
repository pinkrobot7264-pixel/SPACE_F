//! Fault-injection points (M0.6, extended by Phase 1 §13.1).
//!
//! A *fault point* is a named location in real code where the crash,
//! concurrency and fault-injection suites can, **in test builds only**, ask the
//! process to misbehave. In a release build every [`fault_point`] call compiles
//! to a no-op that returns [`FaultAction::None`].
//!
//! The canonical list of point names is `docs/protocols/fault-points.md`. Names
//! are registered there before the code they live in exists, so later phases
//! have a stable vocabulary.
//!
//! The `fault-injection` Cargo feature gates the armable registry. It is
//! dev-only; CI asserts it is off in release.

#![forbid(unsafe_code)]

use std::time::Duration;

use contracts::ErrorCode;

/// What a fault point should do when hit.
///
/// The Phase 1 set (§13.1). Each action targets a specific invariant or ADR,
/// which is why they are distinct variants rather than one generic "fail".
#[derive(Clone, Debug, PartialEq)]
pub enum FaultAction {
    /// Behave normally.
    None,
    /// Return this error to the caller.
    Fail(ErrorCode),
    /// Sleep, then continue. Used for the §13.4 near-miss: a delay just under
    /// the deadline must still succeed.
    Delay(Duration),
    /// Block until the operation's deadline expires. Proves L9 is real
    /// (ADR-0009) and that §3.6's prediction about unrelated operations holds.
    Hang,
    /// Cancel the operation -> `Cancelled` -> `STATUS_CANCELLED` (ADR-0009).
    Cancel,
    /// Corrupt the operation's parameters. Targets INV-NS-6, INV-RES-2.
    InvalidInput,
    /// Force a generation mismatch. Targets INV-ID-3.
    StaleHandle,
    /// Simulate hitting a §3.4 limit. Targets INV-RES-2 / INV-RES-3.
    ResourceExhausted,
    /// Panic -> ADR-0013, ADR-0013a.
    Panic,
    /// **Not armable in Phase 1.** Corrupt the bytes flowing through this point.
    ///
    /// Phase 1 has no integrity mechanism: content lives in a `Vec<u8>`
    /// in-process, with no checksum, no authoritative second copy, and no cache
    /// to reconstruct from. Flipping a byte would produce a test whose expected
    /// result is "the flipped byte comes back", which asserts nothing -- while
    /// sitting in the exit gate looking like integrity coverage.
    ///
    /// It returns in **Phase 3**, injected between chunk-store read and content
    /// verification, expecting an integrity error. [`arm`] rejects it until
    /// then.
    CorruptBytes,
}

/// Why an `arm` call was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmError(pub String);

impl std::fmt::Display for ArmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ArmError {}

/// Phase 0 fault points (Phases 3-8). Registered before their code exists.
pub const PHASE_0_FAULT_POINTS: &[&str] = &[
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

/// Phase 1 fault points (§13.1) -- the WinFsp boundary.
pub const PHASE_1_FAULT_POINTS: &[&str] = &[
    "winfsp_pre_read",
    "winfsp_pre_write",
    "winfsp_pre_open",
    "winfsp_pre_create",
    "winfsp_pre_readdir",
    "winfsp_pre_getinfo",
    "winfsp_pre_rename",
    "winfsp_pre_cleanup",
];

/// Every registered point name. Keep in sync with
/// `docs/protocols/fault-points.md`.
pub fn all_fault_points() -> Vec<&'static str> {
    let mut v = PHASE_0_FAULT_POINTS.to_vec();
    v.extend_from_slice(PHASE_1_FAULT_POINTS);
    v
}

/// Is this a registered point name?
pub fn is_registered(name: &str) -> bool {
    PHASE_0_FAULT_POINTS.contains(&name) || PHASE_1_FAULT_POINTS.contains(&name)
}

#[cfg(not(feature = "fault-injection"))]
mod imp {
    use super::{ArmError, FaultAction};

    #[inline(always)]
    pub fn fault_point(_name: &'static str) -> FaultAction {
        FaultAction::None
    }

    /// Present in every profile so the "not armable" test compiles without the
    /// feature; refuses everything, because a release build has no registry.
    pub fn arm(_name: &str, _action: FaultAction) -> Result<(), ArmError> {
        Err(ArmError(
            "fault injection is not compiled in (enable the fault-injection feature)".into(),
        ))
    }

    pub fn disarm_all() {}

    pub fn armed_count() -> usize {
        0
    }
}

#[cfg(feature = "fault-injection")]
mod imp {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::{ArmError, FaultAction};

    static REGISTRY: Mutex<Option<HashMap<String, FaultAction>>> = Mutex::new(None);

    pub fn fault_point(name: &'static str) -> FaultAction {
        let guard = match REGISTRY.lock() {
            Ok(g) => g,
            // A poisoned registry must not take the filesystem down with it: a
            // fault point is a diagnostic, and failing open is the safe
            // direction here.
            Err(p) => p.into_inner(),
        };
        guard
            .as_ref()
            .and_then(|m| m.get(name).cloned())
            .unwrap_or(FaultAction::None)
    }

    pub fn arm(name: &str, action: FaultAction) -> Result<(), ArmError> {
        if !super::is_registered(name) {
            return Err(ArmError(format!(
                "unknown fault point: {name} (add it to the point list and fault-points.md)"
            )));
        }
        if action == FaultAction::CorruptBytes {
            return Err(ArmError(
                "CorruptBytes is not armable in Phase 1: there is no integrity mechanism for it \
                 to violate, so the test would assert nothing while appearing in the exit gate as \
                 integrity coverage. It returns in Phase 3."
                    .into(),
            ));
        }
        let mut guard = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        guard
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), action);
        Ok(())
    }

    pub fn disarm_all() {
        let mut guard = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(m) = guard.as_mut() {
            m.clear();
        }
    }

    pub fn armed_count() -> usize {
        let guard = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        guard.as_ref().map(|m| m.len()).unwrap_or(0)
    }
}

pub use imp::{arm, armed_count, disarm_all, fault_point};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_is_unique_and_the_counts_are_documented() {
        let all = all_fault_points();
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "duplicate fault point name");
        assert_eq!(PHASE_0_FAULT_POINTS.len(), 12);
        assert_eq!(PHASE_1_FAULT_POINTS.len(), 8, "Phase 1 registers 8 points");
        assert_eq!(all.len(), 20);
    }

    #[test]
    fn corrupt_bytes_is_not_armable_in_phase_1() {
        // §13.1. The variant exists -- Phase 3 needs it -- but arming it is an
        // error, so it cannot quietly become exit-gate evidence for integrity
        // coverage that Phase 1 does not have.
        //
        // The refusal must hold in **both** profiles, which is why the
        // assertion is on `is_err()`: without the feature nothing is armable at
        // all, and with it this is a deliberate policy refusal. Only the
        // second case can carry a reason, so only that one is checked for it.
        let r = arm("winfsp_pre_read", FaultAction::CorruptBytes);
        assert!(r.is_err(), "CorruptBytes must not be armable in Phase 1");

        #[cfg(feature = "fault-injection")]
        assert!(
            r.unwrap_err().to_string().contains("Phase 3"),
            "the refusal should say when it returns"
        );
    }

    #[test]
    fn an_unknown_point_is_refused_rather_than_silently_ignored() {
        assert!(arm("no_such_point", FaultAction::Panic).is_err());
    }

    #[cfg(not(feature = "fault-injection"))]
    #[test]
    fn release_build_fault_point_is_a_no_op() {
        for name in all_fault_points() {
            // `fault_point` takes &'static str; the list is 'static.
            assert_eq!(fault_point(name), FaultAction::None);
        }
        assert_eq!(armed_count(), 0);
    }

    #[cfg(feature = "fault-injection")]
    #[test]
    fn armed_point_returns_its_action_then_disarms() {
        arm("mid_upload", FaultAction::Fail(ErrorCode::NetworkTimeout)).unwrap();
        assert_eq!(
            fault_point("mid_upload"),
            FaultAction::Fail(ErrorCode::NetworkTimeout)
        );
        disarm_all();
        assert_eq!(fault_point("mid_upload"), FaultAction::None);
    }

    #[cfg(feature = "fault-injection")]
    #[test]
    fn every_phase_1_point_can_be_armed_with_every_phase_1_action() {
        let actions = [
            FaultAction::Fail(ErrorCode::DiskFull),
            FaultAction::Delay(Duration::from_millis(1)),
            FaultAction::Hang,
            FaultAction::Cancel,
            FaultAction::InvalidInput,
            FaultAction::StaleHandle,
            FaultAction::ResourceExhausted,
            FaultAction::Panic,
        ];
        for point in PHASE_1_FAULT_POINTS {
            for a in &actions {
                arm(point, a.clone()).unwrap_or_else(|e| panic!("{point}: {e}"));
            }
        }
        disarm_all();
        assert_eq!(armed_count(), 0);
    }
}
