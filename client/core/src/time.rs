//! Windows `FILETIME` conversion (Phase 1 §6.3).

/// 1601-01-01 to 1970-01-01, in 100ns intervals.
pub const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;

/// Now, as a Windows `FILETIME`: 100ns intervals since 1601-01-01 UTC.
pub fn now_filetime() -> u64 {
    // A clock before the Unix epoch would mean the system clock is set to
    // before 1970, which no Windows guest reports. Saturating rather than
    // unwrapping so a filesystem callback can never panic on a clock reading
    // (a panic here would poison the whole filesystem, ADR-0013).
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    EPOCH_DIFF_100NS + d.as_secs() * 10_000_000 + (d.subsec_nanos() as u64) / 100
}

/// Convert Unix seconds to `FILETIME`. Used by tests that need a known value.
pub fn filetime_from_unix_secs(secs: u64) -> u64 {
    EPOCH_DIFF_100NS + secs * 10_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unix_epoch_converts_to_the_known_value() {
        // §6.4: 1970-01-01 -> 116444736000000000.
        assert_eq!(filetime_from_unix_secs(0), 116_444_736_000_000_000);
        assert_eq!(EPOCH_DIFF_100NS, 116_444_736_000_000_000);
    }

    #[test]
    fn one_second_after_the_epoch_is_ten_million_intervals_later() {
        assert_eq!(
            filetime_from_unix_secs(1) - filetime_from_unix_secs(0),
            10_000_000
        );
    }

    #[test]
    fn now_is_after_2020_and_advances() {
        // 2020-01-01 in FILETIME.
        let y2020 = filetime_from_unix_secs(1_577_836_800);
        let a = now_filetime();
        assert!(a > y2020, "clock returned a pre-2020 time: {a}");
        // Monotonic enough for timestamp ordering: a later call is never
        // earlier. (SystemTime can step backwards; this asserts the common
        // case, not a guarantee we rely on.)
        let b = now_filetime();
        assert!(b >= a);
    }
}
