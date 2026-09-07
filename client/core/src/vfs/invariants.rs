//! The invariant checker's contract (Phase 1 §3.5).
//!
//! `docs/protocols/vfs-invariants.md` is the prose specification.
//!
//! **The checker is read-only.** It observes and reports; it never mutates,
//! never repairs, never allocates node state, and never resolves a violation it
//! finds. A diagnostic that repairs is a diagnostic that hides bugs.
//!
//! It must be safe to call at any point in any test, and calling it must not
//! change any subsequent result -- proven by the snapshot-comparison test in
//! §3.7.

/// A detected structural violation. Always names the invariant it broke, so a
/// failing test reports a rule rather than a symptom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvariantViolation {
    pub id: &'static str,
    pub detail: String,
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} violated: {}", self.id, self.detail)
    }
}

impl std::error::Error for InvariantViolation {}

/// Build a violation. Kept short because the checker is a long chain of them.
pub fn viol(id: &'static str, detail: impl Into<String>) -> InvariantViolation {
    InvariantViolation {
        id,
        detail: detail.into(),
    }
}

/// Read-only structural self-check. Required of every `Vfs` implementation.
pub trait VfsDiagnostics {
    /// Walk the whole structure and report the first violation found.
    ///
    /// Implementations must not mutate anything reachable from `&self`.
    fn check_invariants(&self) -> Result<(), InvariantViolation>;
}

/// Every invariant ID defined by §3.5.
///
/// Used by the "every invariant has a checker branch or a named test" gate
/// (§17.4) so an invariant cannot be documented and then quietly go unclaimed.
pub const ALL_INVARIANTS: &[&str] = &[
    "INV-ID-1",
    "INV-ID-2",
    "INV-ID-3",
    "INV-ID-4",
    "INV-ID-5",
    "INV-NS-1",
    "INV-NS-2",
    "INV-NS-3",
    "INV-NS-4",
    "INV-NS-5",
    "INV-NS-6",
    "INV-FS-1",
    "INV-FS-2",
    "INV-FS-3",
    "INV-FS-4",
    "INV-FS-5",
    "INV-DIR-1",
    "INV-DIR-2",
    "INV-DIR-3",
    "INV-RES-1",
    "INV-RES-2",
    "INV-RES-3",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_invariant_list_matches_the_documented_count() {
        // §3.5 defines 22 numbered invariants across identity, namespace, file
        // state, enumeration and resources. The Phase 1 architecture review
        // summarises them as "21"; the enumerated tables contain 22 rows
        // (ID 1-5, NS 1-6, FS 1-5, DIR 1-3, RES 1-3). The tables are the
        // normative text, so 22 is the number asserted here.
        assert_eq!(ALL_INVARIANTS.len(), 22);
        let mut sorted = ALL_INVARIANTS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ALL_INVARIANTS.len(), "duplicate invariant id");
    }

    #[test]
    fn a_violation_displays_its_id() {
        let v = viol("INV-NS-2", "node not exactly once in parent");
        assert!(v.to_string().starts_with("INV-NS-2"));
        assert!(v.to_string().contains("not exactly once"));
    }
}
