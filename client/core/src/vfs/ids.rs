//! Generational, opaque identifiers (Phase 1 §3.1, ADR-0008).
//!
//! **No identifier here is, or derives from, a memory address** (INV-ID-5).
//! Encoding:
//!
//! ```text
//! (generation as u64) << 32 | (index as u64 + 1)
//! ```
//!
//! The `+ 1` guarantees `0` is never valid, which is what
//! `SPACE_INVALID_HANDLE` / `SPACE_INVALID_CURSOR` rely on: a zeroed struct, a
//! missed initialisation, or a hostile caller passing `0` all fail cleanly
//! rather than resolving to slot 0.

/// Generation 0 is never used. It is skipped at allocation and skipped again on
/// wraparound, so a wrapped slot can never collide with the initial state of a
/// never-allocated slot (§3.1).
pub const FIRST_GENERATION: u32 = 1;

/// The shared shape of every generational identifier.
///
/// Exists so one `GenerationalTable` implementation serves nodes, handles and
/// cursors. Three copies of the resolve-and-validate logic would be three
/// places for INV-ID-3 to be got wrong.
pub trait GenId: Copy + Eq + std::fmt::Debug {
    fn from_parts(index: u32, generation: u32) -> Self;
    fn from_raw(v: u64) -> Self;
    fn as_raw(self) -> u64;
    fn index(self) -> Option<u32>;
    fn generation(self) -> u32;
}

macro_rules! generational_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Opaque and generational (ADR-0008). Construct only via
        /// `from_parts`; decode only via `index` / `generation`.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            /// The value that is never valid.
            pub const INVALID: $name = $name(0);

            /// Build from a slot index and generation.
            pub fn from_parts(index: u32, generation: u32) -> Self {
                // index + 1 cannot overflow u32 into the generation field
                // because tables are bounded far below u32::MAX (L7 is 65,536),
                // but use u64 arithmetic so the encoding is correct by
                // construction rather than by that argument.
                Self(((generation as u64) << 32) | (index as u64 + 1))
            }

            /// Reconstruct from a raw FFI value. Never dereferences; validation
            /// happens in the owning table's `resolve`.
            pub fn from_raw(v: u64) -> Self {
                Self(v)
            }

            pub fn as_raw(self) -> u64 {
                self.0
            }

            /// The slot index, or `None` if this is the invalid value.
            pub fn index(self) -> Option<u32> {
                let low = self.0 & 0xFFFF_FFFF;
                if low == 0 {
                    None
                } else {
                    Some((low - 1) as u32)
                }
            }

            pub fn generation(self) -> u32 {
                (self.0 >> 32) as u32
            }

            pub fn is_valid_shape(self) -> bool {
                self.0 != 0 && self.generation() != 0
            }
        }

        impl GenId for $name {
            fn from_parts(index: u32, generation: u32) -> Self {
                $name::from_parts(index, generation)
            }
            fn from_raw(v: u64) -> Self {
                $name::from_raw(v)
            }
            fn as_raw(self) -> u64 {
                $name::as_raw(self)
            }
            fn index(self) -> Option<u32> {
                $name::index(self)
            }
            fn generation(self) -> u32 {
                $name::generation(self)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.index() {
                    Some(i) => write!(f, "{}(idx={}, gen={})", stringify!($name), i, self.generation()),
                    None => write!(f, "{}(INVALID)", stringify!($name)),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                // Log form. Prefixed so a handle cannot be mistaken for a
                // cursor in a log line.
                write!(f, "{}_{}", $name::LOG_PREFIX, self.0)
            }
        }
    };
}

generational_id!(HandleId, "An opened filesystem handle -- one per successful `create`/`open`.");
generational_id!(CursorId, "Directory enumeration state, valid for one `ReadDirectory` call.");
generational_id!(
    NodeId,
    "SPACE's internal object identity -- a file or directory. **Never crosses the FFI** (§3.1)."
);

impl HandleId {
    const LOG_PREFIX: &'static str = "h";
}
impl CursorId {
    const LOG_PREFIX: &'static str = "c";
}
impl NodeId {
    const LOG_PREFIX: &'static str = "n";
}

/// Advance a generation, skipping 0 on wraparound (§3.1).
///
/// The wraparound case is contrived -- it needs `u32::MAX` frees of one slot --
/// but the fix is one line and finding it later means finding it through a
/// corruption report.
pub fn next_generation(current: u32) -> u32 {
    match current.checked_add(1) {
        Some(g) => g,
        None => FIRST_GENERATION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_never_a_valid_identifier() {
        // INV-ID-5.
        assert_eq!(HandleId::INVALID.as_raw(), 0);
        assert_eq!(CursorId::INVALID.as_raw(), 0);
        assert!(HandleId::INVALID.index().is_none());
        assert!(!HandleId::INVALID.is_valid_shape());
    }

    #[test]
    fn index_zero_generation_one_is_not_zero() {
        // The "+1" in the encoding exists precisely for this case: without it,
        // the first handle ever allocated would encode as a value that is also
        // the "invalid" sentinel.
        let h = HandleId::from_parts(0, 1);
        assert_ne!(h.as_raw(), 0);
        assert_eq!(h.index(), Some(0));
        assert_eq!(h.generation(), 1);
    }

    #[test]
    fn encoding_round_trips_across_the_range() {
        for (idx, gen) in [
            (0u32, 1u32),
            (1, 1),
            (65_535, 7),
            (u32::MAX - 1, u32::MAX),
            (1234, 5678),
        ] {
            let h = HandleId::from_parts(idx, gen);
            assert_eq!(h.index(), Some(idx), "idx {idx} gen {gen}");
            assert_eq!(h.generation(), gen, "idx {idx} gen {gen}");
        }
    }

    #[test]
    fn encoding_matches_the_documented_formula() {
        // (generation as u64) << 32 | (index as u64 + 1)
        let h = HandleId::from_parts(4, 9);
        assert_eq!(h.as_raw(), (9u64 << 32) | 5);
    }

    #[test]
    fn generation_wraparound_skips_zero() {
        // §3.1: generation 0 skipped on wrap; no aliasing with a
        // never-allocated slot.
        assert_eq!(next_generation(1), 2);
        assert_eq!(next_generation(u32::MAX - 1), u32::MAX);
        assert_eq!(next_generation(u32::MAX), FIRST_GENERATION);
        assert_ne!(next_generation(u32::MAX), 0);
    }

    #[test]
    fn handle_and_cursor_are_distinct_types() {
        // The type system stops a CursorId being passed where a HandleId is
        // expected. Both wrap u64, so only the newtypes prevent the mix-up.
        let h = HandleId::from_parts(3, 2);
        let c = CursorId::from_parts(3, 2);
        assert_eq!(h.as_raw(), c.as_raw());
        // ...but they Display distinguishably, so a log line is unambiguous.
        assert_ne!(h.to_string(), c.to_string());
        assert!(h.to_string().starts_with("h_"));
        assert!(c.to_string().starts_with("c_"));
    }

    #[test]
    fn arbitrary_raw_values_decode_without_panicking() {
        // The fuzz targets feed arbitrary u64s here (§14.1). Decoding must be
        // total; validation is the table's job.
        for v in [0u64, 1, u64::MAX, 0xFFFF_FFFF, 0x1_0000_0000, 42] {
            let h = HandleId::from_raw(v);
            let _ = h.index();
            let _ = h.generation();
            let _ = h.is_valid_shape();
            let _ = format!("{h:?}");
        }
    }

    #[test]
    fn no_identifier_derives_from_an_address() {
        // INV-ID-5, asserted structurally: two ids built from the same parts
        // are equal regardless of where anything lives in memory, and building
        // the same parts twice in different allocations gives the same value.
        let a = Box::new(HandleId::from_parts(7, 3));
        let b = Box::new(HandleId::from_parts(7, 3));
        assert_eq!(*a, *b);
        assert_eq!(a.as_raw(), (3u64 << 32) | 8);
    }
}
