//! Fixture generator CLI wrapper (Phase 0 stub).
//!
//! The deterministic generators live in `space-generators`; this crate will
//! grow a thin CLI around them (streaming 500 GB fixtures) in Phase 11. Kept as
//! a workspace member now so that entry point has a home.

pub use space_generators as generators;
