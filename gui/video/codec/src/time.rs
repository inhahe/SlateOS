//! Time: a container's ticks to nanoseconds, and back for a seek.
//!
//! A Matroska track counts time in ticks of `num / den` seconds (its
//! `TimestampScale`, a millisecond in nearly every file). A frame's time is
//! given out in nanoseconds, which every container's ticks convert to
//! exactly when the tick is a whole number of nanoseconds -- as Matroska's
//! always is -- and which hold some 292 years either side of zero.

/// Nanoseconds in a second.
const SECOND: i128 = 1_000_000_000;

/// An `i128` of nanoseconds, held to what an `i64` holds.
fn saturate(ns: i128) -> i64 {
    i64::try_from(ns).unwrap_or(if ns < 0 { i64::MIN } else { i64::MAX })
}

/// `ticks` of `num / den` seconds, in nanoseconds: rounded down, and held to
/// the range of an `i64`.
pub(crate) fn to_ns(ticks: i64, (num, den): (u64, u64)) -> i64 {
    let den = i128::from(den.max(1));
    match i128::from(ticks)
        .checked_mul(i128::from(num))
        .and_then(|v| v.checked_mul(SECOND))
    {
        Some(v) => saturate(v.div_euclid(den)),
        // Only a product beyond 2^127 overflows, far past what an i64 holds.
        None => saturate(if ticks < 0 { i128::MIN } else { i128::MAX }),
    }
}

/// A duration of `ticks`, in nanoseconds: rounded down, and held to the
/// range of a `u64`.
pub(crate) fn duration_to_ns(ticks: u64, (num, den): (u64, u64)) -> u64 {
    let den = u128::from(den.max(1));
    u128::from(ticks)
        .checked_mul(u128::from(num))
        .and_then(|v| v.checked_mul(SECOND.unsigned_abs()))
        .and_then(|v| v.checked_div(den))
        .map_or(u64::MAX, |v| u64::try_from(v).unwrap_or(u64::MAX))
}

/// The last tick of `num / den` seconds whose time, as [`to_ns`] gives it,
/// is at or before `ns`: where a seek to `ns` looks from.
///
/// [`to_ns`] rounds down, so a tick `t` is at or before `ns` exactly when
/// `t * num * 10^9 < (ns + 1) * den`; the last such `t` is
/// `floor(((ns + 1) * den - 1) / (num * 10^9))`. (Rounding `ns * den / (num *
/// 10^9)` down instead would miss a frame by one where a tick is not a whole
/// number of nanoseconds.)
pub(crate) fn to_ticks(ns: i64, (num, den): (u64, u64)) -> i64 {
    let divisor = i128::from(num.max(1)).saturating_mul(SECOND);
    // |ns + 1| <= 2^63 and den < 2^64, so the product is within an i128 and
    // none of the saturating operations saturates.
    let product = i128::from(ns)
        .saturating_add(1)
        .saturating_mul(i128::from(den.max(1)));
    saturate(product.saturating_sub(1).div_euclid(divisor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A millisecond tick, Matroska's `TimestampScale` of 1,000,000.
    const MS: (u64, u64) = (1, 1000);

    #[test]
    fn ticks_become_nanoseconds_exactly() {
        assert_eq!(to_ns(0, MS), 0);
        assert_eq!(to_ns(1234, MS), 1_234_000_000);
        assert_eq!(to_ns(-80, MS), -80_000_000);
        // A tick of a 30000/1001 rate, in a third of a nanosecond's error:
        // rounded down.
        assert_eq!(to_ns(1, (1001, 30000)), 33_366_666);
        assert_eq!(to_ns(-1, (1001, 30000)), -33_366_667);
        assert_eq!(duration_to_ns(40, MS), 40_000_000);
    }

    #[test]
    fn a_time_past_an_i64_saturates() {
        assert_eq!(to_ns(i64::MAX, MS), i64::MAX);
        assert_eq!(to_ns(i64::MIN, MS), i64::MIN);
        assert_eq!(to_ns(i64::MAX, (u64::MAX, 1)), i64::MAX);
        assert_eq!(to_ns(i64::MIN, (u64::MAX, 1)), i64::MIN);
        assert_eq!(duration_to_ns(u64::MAX, (u64::MAX, 1)), u64::MAX);
        // A zero denominator, which no reduced time base has, is read as 1.
        assert_eq!(to_ns(2, (1, 0)), 2_000_000_000);
    }

    #[test]
    fn a_time_finds_the_tick_at_or_before_it() {
        assert_eq!(to_ticks(1_234_000_000, MS), 1234);
        assert_eq!(to_ticks(1_234_999_999, MS), 1234);
        assert_eq!(to_ticks(-1, MS), -1);
        assert_eq!(to_ticks(0, MS), 0);
        // And back: a frame's own time finds its own tick.
        for ticks in [-5i64, 0, 1, 999, 1_000_000] {
            assert_eq!(to_ticks(to_ns(ticks, (1001, 30000)), (1001, 30000)), ticks);
        }
        assert_eq!(to_ticks(i64::MAX, (1, u64::MAX)), i64::MAX);
    }
}
