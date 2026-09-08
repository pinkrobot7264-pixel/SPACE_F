//! Invariant coverage (§17.4: "every invariant in §3.5 has a checker branch or
//! a named test; none unclaimed").
//!
//! This is a gate item, not a nicety. Twenty-two invariants are easy to write
//! down and easy to leave half-covered; a documented invariant that nothing
//! checks is worse than no invariant, because it reads as coverage in the exit
//! gate.
//!
//! The table below is the claim. Each row names **where** the invariant is
//! enforced, and the test asserts the claimed location actually contains
//! something that mentions it — so deleting the enforcing test breaks this test
//! too, rather than silently leaving the invariant unclaimed.

#![cfg(test)]

use crate::vfs::invariants::ALL_INVARIANTS;

/// Where each invariant is enforced, and the source that must mention it.
///
/// `Checker` — a branch in `memvfs/check.rs` that returns this ID, plus a
/// `fires_inv_*` test proving the branch fires.
/// `Conformance` — a behavioural assertion in the conformance suite.
/// `Property` — a proptest law.
/// `Fuzz` — a fuzz target assertion.
/// `CodeRule` — a structural property enforced by construction and asserted by
/// a unit test.
struct Claim {
    id: &'static str,
    kind: &'static str,
    /// Sources that must mention the invariant ID or its named test.
    evidence: &'static [&'static str],
}

const CHECK_RS: &str = include_str!("memvfs/check.rs");
const MEMVFS_TESTS: &str = include_str!("memvfs/tests.rs");
const TABLE_RS: &str = include_str!("memvfs/table.rs");
const IDS_RS: &str = include_str!("ids.rs");
const PROPERTIES_RS: &str = include_str!("properties.rs");
const CONF_HANDLES: &str = include_str!("../conformance/handles.rs");
const CONF_LIFECYCLE: &str = include_str!("../conformance/lifecycle.rs");
const CONF_DIRECTORY: &str = include_str!("../conformance/directory.rs");
const CONF_NAMING: &str = include_str!("../conformance/naming.rs");
const CONF_BOUNDARIES: &str = include_str!("../conformance/boundaries.rs");
const CONF_INVARIANTS: &str = include_str!("../conformance/invariants.rs");
const CONF_RENAME: &str = include_str!("../conformance/rename_delete.rs");

fn claims() -> Vec<Claim> {
    vec![
        Claim {
            id: "INV-ID-1",
            kind: "Conformance + Property",
            evidence: &[CONF_HANDLES, PROPERTIES_RS],
        },
        Claim {
            id: "INV-ID-2",
            kind: "Checker + Conformance",
            evidence: &[CHECK_RS, MEMVFS_TESTS, CONF_HANDLES],
        },
        Claim {
            id: "INV-ID-3",
            kind: "Conformance + Fuzz",
            evidence: &[CONF_HANDLES, CONF_LIFECYCLE, TABLE_RS],
        },
        Claim {
            id: "INV-ID-4",
            kind: "Checker + Conformance",
            evidence: &[CHECK_RS, MEMVFS_TESTS, CONF_HANDLES],
        },
        Claim {
            id: "INV-ID-5",
            kind: "CodeRule + Conformance",
            evidence: &[IDS_RS, CONF_HANDLES],
        },
        Claim {
            id: "INV-NS-1",
            kind: "Checker",
            evidence: &[CHECK_RS, MEMVFS_TESTS],
        },
        Claim {
            id: "INV-NS-2",
            kind: "Checker",
            evidence: &[CHECK_RS, MEMVFS_TESTS],
        },
        Claim {
            id: "INV-NS-3",
            kind: "Checker",
            evidence: &[CHECK_RS, MEMVFS_TESTS],
        },
        Claim {
            id: "INV-NS-4",
            kind: "Checker + Conformance",
            evidence: &[CHECK_RS, MEMVFS_TESTS, CONF_RENAME],
        },
        Claim {
            id: "INV-NS-5",
            kind: "Checker + Conformance",
            evidence: &[CHECK_RS, MEMVFS_TESTS, CONF_RENAME],
        },
        // The other half of INV-NS-6 is ProcMon evidence (§16.4) and cannot be
        // asserted from inside this process by construction.
        Claim {
            id: "INV-NS-6",
            kind: "Conformance + ProcMon",
            evidence: &[CONF_NAMING],
        },
        Claim {
            id: "INV-FS-1",
            kind: "Checker",
            evidence: &[CHECK_RS, MEMVFS_TESTS],
        },
        Claim {
            id: "INV-FS-2",
            kind: "Checker",
            evidence: &[CHECK_RS, MEMVFS_TESTS],
        },
        Claim {
            id: "INV-FS-3",
            kind: "Conformance",
            evidence: &[CONF_LIFECYCLE],
        },
        Claim {
            id: "INV-FS-4",
            kind: "Checker + Conformance",
            evidence: &[CHECK_RS, MEMVFS_TESTS, CONF_RENAME],
        },
        Claim {
            id: "INV-FS-5",
            kind: "Conformance",
            evidence: &[CONF_INVARIANTS],
        },
        Claim {
            id: "INV-DIR-1",
            kind: "Conformance + Property",
            evidence: &[CONF_DIRECTORY, PROPERTIES_RS],
        },
        Claim {
            id: "INV-DIR-2",
            kind: "Conformance + Property",
            evidence: &[CONF_DIRECTORY, PROPERTIES_RS],
        },
        Claim {
            id: "INV-DIR-3",
            kind: "Conformance + Fuzz",
            evidence: &[CONF_DIRECTORY],
        },
        Claim {
            id: "INV-RES-1",
            kind: "Checker",
            evidence: &[CHECK_RS, MEMVFS_TESTS],
        },
        Claim {
            id: "INV-RES-2",
            kind: "Conformance",
            evidence: &[CONF_BOUNDARIES, CONF_INVARIANTS],
        },
        Claim {
            id: "INV-RES-3",
            kind: "Conformance",
            evidence: &[CONF_BOUNDARIES],
        },
    ]
}

#[test]
fn every_documented_invariant_is_claimed() {
    let claims = claims();
    for id in ALL_INVARIANTS {
        assert!(
            claims.iter().any(|c| c.id == *id),
            "{id} is documented in §3.5 but claimed by nothing. A documented \
             invariant that nothing checks reads as coverage in the exit gate \
             while being none."
        );
    }
    assert_eq!(
        claims.len(),
        ALL_INVARIANTS.len(),
        "the claim table and the invariant list have drifted apart"
    );
}

#[test]
fn every_claim_names_a_real_invariant() {
    // The other direction: a claim for an invariant that no longer exists means
    // the table was not updated when §3.5 changed.
    for c in claims() {
        assert!(
            ALL_INVARIANTS.contains(&c.id),
            "{} is claimed but is not a documented invariant",
            c.id
        );
    }
}

#[test]
fn every_claimed_location_actually_mentions_its_invariant() {
    // The claim is only worth something if the named source really enforces
    // the invariant. Requiring the ID to appear there means deleting the
    // enforcing test breaks THIS test too, instead of quietly leaving the
    // invariant unclaimed.
    for c in claims() {
        let mentioned = c.evidence.iter().any(|src| src.contains(c.id));
        assert!(
            mentioned,
            "{} is claimed as {} but none of its named sources mention it",
            c.id, c.kind
        );
    }
}

#[test]
fn the_checker_branches_are_exactly_the_ones_claimed_as_checker() {
    // Every ID the checker can return must be claimed as a Checker invariant,
    // and vice versa. This catches a branch added to check.rs without a
    // corresponding fires_inv_* test.
    // `viol("INV-X", ...)` is frequently wrapped across lines by rustfmt, so
    // match against a whitespace-collapsed copy rather than the raw source.
    // Matching the raw text would make this test pass or fail on formatting,
    // which is exactly the kind of brittleness that gets a gate disabled.
    let collapsed: String = CHECK_RS.split_whitespace().collect::<Vec<_>>().join(" ");

    for c in claims() {
        let in_checker = collapsed.contains(&format!("viol( \"{}\"", c.id))
            || collapsed.contains(&format!("viol(\"{}\"", c.id));
        let claimed_checker = c.kind.contains("Checker");
        assert_eq!(
            in_checker,
            claimed_checker,
            "{}: check.rs {} a branch for it, but the claim says {}",
            c.id,
            if in_checker { "has" } else { "has no" },
            c.kind
        );
    }
}

#[test]
fn every_checker_branch_has_a_firing_test() {
    // §3.7: "A checker that never fires is not a checker."
    for c in claims() {
        if !c.kind.contains("Checker") {
            continue;
        }
        assert!(
            MEMVFS_TESTS.contains(c.id),
            "{} has a checker branch but no test proving it fires",
            c.id
        );
    }
}
