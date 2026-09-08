//! The generational slot table (Phase 1 §5.2, ADR-0008).
//!
//! One implementation serves nodes, handles and cursors. Three copies of the
//! resolve-and-validate logic would be three places for INV-ID-3 to be got
//! wrong.
//!
//! `resolve` checks, in order:
//!
//! 1. index within range
//! 2. slot live
//! 3. generation matches
//!
//! Any mismatch is an error -- **never a panic, never a dereference**
//! (INV-ID-3). This is the strongest safety property in Phase 1.

use std::marker::PhantomData;

use contracts::{ErrorCode, SpaceError};

use crate::vfs::ids::{next_generation, GenId, FIRST_GENERATION};

struct Slot<T> {
    /// Advances on every free. A stale id carrying the old value fails here.
    generation: u32,
    value: Option<T>,
}

/// A bounded, generational slot table.
pub struct GenerationalTable<K: GenId, T> {
    slots: Vec<Slot<T>>,
    /// Indices of freed slots, reused before growing. Reuse is safe precisely
    /// because the generation advanced on free.
    free: Vec<u32>,
    live: usize,
    max: usize,
    /// The error a failed resolve reports. Handles report `InvalidHandle`;
    /// cursors report `InvalidParameter`, because a bad cursor is a bad
    /// argument to `dir_next`, not a filesystem handle (INV-DIR-3).
    invalid_code: ErrorCode,
    /// The error `alloc` reports when the table is full.
    exhausted_detail: &'static str,
    _k: PhantomData<K>,
}

impl<K: GenId, T> GenerationalTable<K, T> {
    pub fn new(max: usize, invalid_code: ErrorCode, exhausted_detail: &'static str) -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
            max,
            invalid_code,
            exhausted_detail,
            _k: PhantomData,
        }
    }

    pub fn live_count(&self) -> usize {
        self.live
    }

    pub fn max(&self) -> usize {
        self.max
    }

    fn invalid(&self) -> SpaceError {
        SpaceError::new(self.invalid_code, "identifier does not resolve")
    }

    /// Allocate a slot, enforcing the table's limit.
    ///
    /// On limit the table is left **exactly** as it was -- no slot pushed, no
    /// free-list entry consumed, no counter moved (INV-RES-3).
    pub fn alloc(&mut self, value: T) -> Result<K, SpaceError> {
        if self.live >= self.max {
            return Err(SpaceError::new(
                ErrorCode::ResourceExhausted,
                self.exhausted_detail,
            ));
        }

        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none(), "free list held a live slot");
            slot.value = Some(value);
            self.live += 1;
            return Ok(K::from_parts(index, slot.generation));
        }

        // Growing past u32::MAX slots is unreachable -- every table's max is far
        // below it -- but the cast must not be the thing that wraps if a future
        // limit is raised carelessly.
        if self.slots.len() >= u32::MAX as usize {
            return Err(SpaceError::new(
                ErrorCode::ResourceExhausted,
                self.exhausted_detail,
            ));
        }

        let index = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: FIRST_GENERATION,
            value: Some(value),
        });
        self.live += 1;
        Ok(K::from_parts(index, FIRST_GENERATION))
    }

    /// Resolve an id to its value, or report a controlled error.
    pub fn resolve(&self, id: K) -> Result<&T, SpaceError> {
        let index = id.index().ok_or_else(|| self.invalid())? as usize;
        let slot = self.slots.get(index).ok_or_else(|| self.invalid())?;
        let value = slot.value.as_ref().ok_or_else(|| self.invalid())?;
        if slot.generation != id.generation() {
            // The stale value does NOT reach the new object. INV-ID-3.
            return Err(self.invalid());
        }
        Ok(value)
    }

    pub fn resolve_mut(&mut self, id: K) -> Result<&mut T, SpaceError> {
        let invalid = SpaceError::new(self.invalid_code, "identifier does not resolve");
        let index = id.index().ok_or_else(|| invalid.clone())? as usize;
        let slot = self.slots.get_mut(index).ok_or_else(|| invalid.clone())?;
        if slot.generation != id.generation() {
            return Err(invalid);
        }
        slot.value.as_mut().ok_or(invalid)
    }

    /// Free a slot and advance its generation, returning the stored value.
    ///
    /// A second free of the same id reports the same controlled error as any
    /// other stale id -- no panic, no double free.
    pub fn free(&mut self, id: K) -> Result<T, SpaceError> {
        let invalid = SpaceError::new(self.invalid_code, "identifier does not resolve");
        let index = id.index().ok_or_else(|| invalid.clone())? as usize;
        let slot = self.slots.get_mut(index).ok_or_else(|| invalid.clone())?;
        if slot.generation != id.generation() {
            return Err(invalid);
        }
        let value = slot.value.take().ok_or(invalid)?;
        slot.generation = next_generation(slot.generation);
        self.free.push(index as u32);
        self.live -= 1;
        Ok(value)
    }

    /// Iterate live values. Read-only: used by the invariant checker.
    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.value
                .as_ref()
                .map(|v| (K::from_parts(i as u32, s.generation), v))
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut T)> {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| {
            let gen = s.generation;
            s.value.as_mut().map(|v| (K::from_parts(i as u32, gen), v))
        })
    }

    pub fn contains(&self, id: K) -> bool {
        self.resolve(id).is_ok()
    }

    /// Number of slots ever allocated, live or not. Diagnostics only.
    pub fn capacity_used(&self) -> usize {
        self.slots.len()
    }

    /// Test-only: force a slot's generation, so wraparound can be reached
    /// without performing `u32::MAX` frees.
    #[cfg(test)]
    pub fn set_generation_for_test(&mut self, index: u32, generation: u32) {
        self.slots[index as usize].generation = generation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::ids::HandleId;

    fn table(max: usize) -> GenerationalTable<HandleId, u32> {
        GenerationalTable::new(max, ErrorCode::InvalidHandle, "handle limit reached")
    }

    #[test]
    fn resolve_never_allocated_handle_is_invalid_handle() {
        let t = table(16);
        let h = HandleId::from_parts(0, 1);
        assert_eq!(t.resolve(h).unwrap_err().code, ErrorCode::InvalidHandle);
    }

    #[test]
    fn resolve_handle_zero_is_invalid_handle() {
        // INV-ID-5: 0 is never valid.
        let mut t = table(16);
        t.alloc(7).unwrap();
        assert_eq!(
            t.resolve(HandleId::INVALID).unwrap_err().code,
            ErrorCode::InvalidHandle
        );
    }

    #[test]
    fn resolve_after_free_is_invalid_handle() {
        let mut t = table(16);
        let h = t.alloc(7).unwrap();
        assert_eq!(*t.resolve(h).unwrap(), 7);
        t.free(h).unwrap();
        assert_eq!(t.resolve(h).unwrap_err().code, ErrorCode::InvalidHandle);
    }

    #[test]
    fn free_twice_is_an_error_not_a_panic_or_double_free() {
        let mut t = table(16);
        let h = t.alloc(7).unwrap();
        assert_eq!(t.free(h).unwrap(), 7);
        assert_eq!(t.free(h).unwrap_err().code, ErrorCode::InvalidHandle);
        assert_eq!(t.live_count(), 0);
    }

    #[test]
    fn index_out_of_range_is_invalid_handle() {
        let t = table(16);
        let h = HandleId::from_parts(u32::MAX - 1, 1);
        assert_eq!(t.resolve(h).unwrap_err().code, ErrorCode::InvalidHandle);
    }

    #[test]
    fn generation_mismatch_after_slot_reuse_does_not_reach_the_new_object() {
        // INV-ID-3, the strongest safety property in Phase 1.
        let mut t = table(16);
        let old = t.alloc(111).unwrap();
        t.free(old).unwrap();
        let new = t.alloc(222).unwrap();

        // The slot index was reused...
        assert_eq!(old.index(), new.index());
        // ...but the generation advanced, so the old id is dead...
        assert_ne!(old.generation(), new.generation());
        assert_eq!(t.resolve(old).unwrap_err().code, ErrorCode::InvalidHandle);
        // ...and it did NOT reach the new object.
        assert_eq!(*t.resolve(new).unwrap(), 222);
    }

    #[test]
    fn ten_thousand_alloc_free_cycles_reuse_indices_and_advance_generations() {
        let mut t = table(64);
        let mut last_gen = 0;
        for i in 0..10_000u32 {
            let h = t.alloc(i).unwrap();
            assert_eq!(h.index(), Some(0), "index should be reused");
            assert!(h.generation() > last_gen, "generation must advance");
            last_gen = h.generation();
            t.free(h).unwrap();
        }
        assert_eq!(t.live_count(), 0);
        // Exactly one slot was ever created.
        assert_eq!(t.capacity_used(), 1);
    }

    #[test]
    fn generation_wraparound_skips_zero() {
        // Contrived -- it needs u32::MAX frees of one slot -- but the fix is one
        // line and finding it later means finding it through a corruption
        // report (§5.4).
        //
        // The generation must be forced *before* the allocation whose free
        // triggers the wrap. Forcing it while a handle is outstanding just makes
        // that handle stale, which proves nothing about wraparound.
        let mut t = table(16);
        let warmup = t.alloc(1).unwrap();
        t.free(warmup).unwrap();
        t.set_generation_for_test(0, u32::MAX);

        let last = t.alloc(1).unwrap();
        assert_eq!(last.generation(), u32::MAX, "setup did not take effect");
        t.free(last).unwrap();

        let wrapped = t.alloc(2).unwrap();
        assert_eq!(wrapped.generation(), FIRST_GENERATION);
        assert_ne!(wrapped.generation(), 0, "generation 0 must be skipped");
        assert_eq!(*t.resolve(wrapped).unwrap(), 2);

        // The handle from just before the wrap is dead, as for any free.
        assert!(t.resolve(last).is_err());

        // A forged or zeroed id at generation 0 must not resolve to the wrapped
        // slot -- this is exactly what skipping generation 0 prevents.
        assert!(t.resolve(HandleId::from_parts(0, 0)).is_err());
        assert!(t.resolve(HandleId::INVALID).is_err());
    }

    #[test]
    fn wraparound_aliasing_residual_is_documented_not_silently_absent() {
        // Honest statement of a real limit. The encoding fixed by §3.1 --
        // (generation as u64) << 32 | (index as u64 + 1) -- gives 32 generation
        // bits, so an identifier held across 2^32 frees of the *same slot* will
        // eventually collide with a live one. Skipping generation 0 removes the
        // collision with a never-allocated slot; it cannot remove this one, and
        // widening the counter would change the contract encoding.
        //
        // This test pins the behaviour so nobody later reads "no aliasing" as a
        // stronger claim than the mechanism can support.
        let mut t = table(16);
        let ancient = t.alloc(1).unwrap();
        assert_eq!(ancient.generation(), FIRST_GENERATION);
        t.free(ancient).unwrap();

        t.set_generation_for_test(0, u32::MAX);
        let last = t.alloc(9).unwrap();
        t.free(last).unwrap();

        let wrapped = t.alloc(2).unwrap();
        assert_eq!(wrapped.as_raw(), ancient.as_raw());
        // 2^32 frees of one slot is a different order of program error, and it
        // is bounded by L7 x lifetime rather than reachable by a caller.
    }

    #[test]
    fn exceeding_the_limit_is_resource_exhausted_with_no_partial_state() {
        // INV-RES-2 and INV-RES-3: the error, and state byte-identical.
        let mut t = table(3);
        let ids: Vec<_> = (0..3).map(|i| t.alloc(i).unwrap()).collect();

        let live_before = t.live_count();
        let cap_before = t.capacity_used();

        let err = t.alloc(99).unwrap_err();
        assert_eq!(err.code, ErrorCode::ResourceExhausted);

        assert_eq!(t.live_count(), live_before, "live count changed on failure");
        assert_eq!(
            t.capacity_used(),
            cap_before,
            "a slot was pushed on failure"
        );
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(
                *t.resolve(*id).unwrap(),
                i as u32,
                "existing entry disturbed"
            );
        }
    }

    #[test]
    fn freeing_after_exhaustion_makes_room_again() {
        let mut t = table(2);
        let a = t.alloc(1).unwrap();
        let b = t.alloc(2).unwrap();
        assert!(t.alloc(3).is_err());
        t.free(a).unwrap();
        let c = t.alloc(3).unwrap();
        assert_eq!(*t.resolve(c).unwrap(), 3);
        assert_eq!(*t.resolve(b).unwrap(), 2);
        assert_eq!(t.live_count(), 2);
    }

    #[test]
    fn arbitrary_raw_values_never_panic() {
        // The fuzz targets feed arbitrary u64 here (§14.1).
        let mut t = table(8);
        let live = t.alloc(5).unwrap();
        for v in [
            0u64,
            1,
            u64::MAX,
            0xFFFF_FFFF,
            0x1_0000_0000,
            live.as_raw().wrapping_add(1),
            live.as_raw() ^ 0xDEAD_BEEF,
        ] {
            let id = HandleId::from_raw(v);
            let _ = t.resolve(id);
            let _ = t.contains(id);
        }
        // The live handle is untouched by all that probing.
        assert_eq!(*t.resolve(live).unwrap(), 5);
    }

    #[test]
    fn iter_yields_only_live_slots_with_resolvable_ids() {
        let mut t = table(8);
        let a = t.alloc(10).unwrap();
        let b = t.alloc(20).unwrap();
        let c = t.alloc(30).unwrap();
        t.free(b).unwrap();

        let mut seen: Vec<u32> = t.iter().map(|(_, v)| *v).collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![10, 30]);

        for (id, _) in t.iter() {
            assert!(t.resolve(id).is_ok(), "iter produced an unresolvable id");
        }
        assert_eq!(t.live_count(), 2);
        let _ = (a, c);
    }
}
