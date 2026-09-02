//! Deliberate corruption primitives (M0.10).
//!
//! Every function here breaks a specific invariant in a specific way, so a
//! negative test can say exactly what it injected. Used by the E2E negative
//! path (M0.14) and the fault-injection suites from Phase 3 onward.

#![forbid(unsafe_code)]

use contracts::{Manifest, ManifestEntry};

/// Flip every bit of one byte.
pub fn flip_byte(data: &mut [u8], at: usize) {
    if at < data.len() {
        data[at] ^= 0xFF;
    }
}

/// Cut the buffer to `to` bytes.
pub fn truncate(data: &mut Vec<u8>, to: usize) {
    data.truncate(to);
}

/// Append `by` non-zero bytes.
pub fn extend(data: &mut Vec<u8>, by: usize) {
    data.extend(std::iter::repeat(0x5Au8).take(by));
}

/// Swap two manifest entries (breaks invariant 1).
pub fn swap_entries(m: &mut Manifest, i: usize, j: usize) {
    m.entries.swap(i, j);
}

/// Remove one manifest entry, leaving `total_size`/`chunk_count` stale.
pub fn drop_entry(m: &mut Manifest, i: usize) {
    if i < m.entries.len() {
        m.entries.remove(i);
    }
}

/// Nudge `total_size` (breaks invariant 5).
pub fn alter_total_size(m: &mut Manifest, delta: i64) {
    m.total_size = (m.total_size as i128 + delta as i128).max(0) as u64;
}

/// Nudge `chunk_count` (breaks invariant 6).
pub fn alter_chunk_count(m: &mut Manifest, delta: i64) {
    m.chunk_count = (m.chunk_count as i128 + delta as i128).max(0) as u64;
}

/// Open a gap after entry `at` by pushing every later entry forward (breaks
/// invariant 2).
pub fn introduce_gap(m: &mut Manifest, at: usize) {
    for e in m.entries.iter_mut().skip(at + 1) {
        e.logical_offset += 1;
    }
    m.total_size += 1;
}

/// Overlap entry `at+1` back onto `at` (breaks invariant 3).
pub fn introduce_overlap(m: &mut Manifest, at: usize) {
    if let Some(e) = m.entries.get_mut(at + 1) {
        e.logical_offset = e.logical_offset.saturating_sub(1);
    }
}

/// Replace one entry's backing chunk reference with one that is not in
/// `chunks` (breaks invariant 9).
pub fn dangle_chunk_ref(m: &mut Manifest, at: usize) {
    if let Some(e) = m.entries.get_mut(at) {
        e.chunk_id = contracts::ChunkId::from_bytes(b"corruptor::dangling reference");
    }
}

/// Push a zero-length entry (breaks invariant 7).
pub fn insert_zero_length_entry(m: &mut Manifest) {
    if let Some(last) = m.entries.last().cloned() {
        m.entries.push(ManifestEntry {
            logical_offset: last.logical_offset + last.length,
            length: 0,
            chunk_id: last.chunk_id,
            chunk_offset: 0,
        });
        m.chunk_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::validate::validate_manifest;
    use space_generators::generate_manifest;

    fn m() -> Manifest {
        generate_manifest(4096 * 4, 4096, 3).0
    }

    #[test]
    fn baseline_is_valid() {
        validate_manifest(&m()).unwrap();
    }

    #[test]
    fn each_corruption_is_rejected() {
        let mut a = m();
        swap_entries(&mut a, 0, 3);
        assert!(validate_manifest(&a).is_err());

        let mut b = m();
        alter_total_size(&mut b, 1);
        assert!(validate_manifest(&b).is_err());

        let mut c = m();
        alter_chunk_count(&mut c, 1);
        assert!(validate_manifest(&c).is_err());

        let mut d = m();
        introduce_gap(&mut d, 0);
        assert!(validate_manifest(&d).is_err());

        let mut e = m();
        introduce_overlap(&mut e, 0);
        assert!(validate_manifest(&e).is_err());

        let mut f = m();
        dangle_chunk_ref(&mut f, 1);
        assert!(validate_manifest(&f).is_err());

        let mut g = m();
        insert_zero_length_entry(&mut g);
        assert!(validate_manifest(&g).is_err());

        let mut h = m();
        drop_entry(&mut h, 1);
        assert!(validate_manifest(&h).is_err());
    }

    #[test]
    fn byte_corruption_helpers_work() {
        let mut v = vec![1u8, 2, 3, 4];
        flip_byte(&mut v, 1);
        assert_eq!(v[1], 2 ^ 0xFF);
        truncate(&mut v, 2);
        assert_eq!(v.len(), 2);
        extend(&mut v, 3);
        assert_eq!(v.len(), 5);
        assert!(!v[2..].contains(&0));
    }
}
