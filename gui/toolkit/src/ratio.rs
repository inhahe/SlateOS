//! Part of a whole: fractions, percentages, and the question every copy of
//! them forgot to ask in the same statement it divided.
//!
//! Fifteen types across the shell and the applications report progress as a
//! percentage, and all fifteen wrote it out:
//!
//! ```ignore
//! pub fn progress_pct(&self) -> u32 {
//!     if self.total_bytes == 0 {
//!         return 0;
//!     }
//!     ((self.transferred_bytes * 100) / self.total_bytes) as u32
//! }
//! ```
//!
//! The guard is correct and the division is correct, and they are two
//! statements — which is the shape this codebase keeps paying for. A reader
//! checking the division has to look up to find the proof, an editor moving
//! the division does not have to move the proof, and a *new* copy of the
//! division is written without one. That is not hypothetical: the copies here
//! disagreed about the answer they guard *for*. Four different values were
//! returned for "what percentage of nothing is nothing" — `0`, `0.0`, `100`,
//! and `100.0` — and each was right for its own caller, which is exactly why
//! it is not a decision a shared helper should make on their behalf.
//!
//! So [`percent`] returns `Option`. There is no percentage of an empty whole,
//! the type says so, and `.unwrap_or(0.0)` puts each caller's answer in the
//! same expression as the division that needed it:
//!
//! ```
//! use guitk::ratio;
//!
//! // An empty disk is 0% used …
//! assert_eq!(ratio::percent(0_u64, 0_u64).unwrap_or(0.0), 0.0);
//! // … but a battery with no recorded design capacity is assumed healthy.
//! assert_eq!(ratio::percent(0_u64, 0_u64).unwrap_or(100.0), 100.0);
//!
//! assert_eq!(ratio::percent(1_u64, 4_u64), Some(25.0));
//! assert_eq!(ratio::fraction(1_u64, 4_u64), Some(0.25));
//! ```
//!
//! The copies disagreed about clamping too. Only one of the fifteen bounded
//! its result, so a disk whose reported `used` exceeded its `total` — stale
//! numbers from a filesystem mid-write — drew a progress bar past the end of
//! its own track. [`percent`] is faithful and [`percent_clamped`] is bounded;
//! naming the choice is what stops it being made by accident.

/// A number that can be a part or a whole.
///
/// Implemented for the integers and floats, so a `usize` count and a `u64`
/// byte total can be divided by each other without a cast at the call site.
/// Casts are how a byte count becomes a negative number.
pub trait Quantity: Copy {
    /// This value as an `f64`.
    ///
    /// Lossy above 2^53 for the 64-bit integers, which for a percentage is
    /// 9 quadrillion bytes' worth of not mattering: the loss is in digits the
    /// division divides away.
    #[must_use]
    fn as_f64(self) -> f64;
}

macro_rules! impl_quantity {
    ($($t:ty),+ $(,)?) => {$(
        impl Quantity for $t {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_lossless,
                reason = "the whole point of this trait is the one place that conversion happens"
            )]
            fn as_f64(self) -> f64 {
                self as f64
            }
        }
    )+};
}

impl_quantity!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
);

/// `part / whole`, or `None` if there is no whole to be part of.
///
/// `None` when `whole` is zero, negative, or not a number — every case where
/// the quotient would be a value the caller cannot use, gathered into the one
/// answer the caller has to handle. The result is *not* clamped: a `part`
/// larger than its `whole` gives a fraction above 1, which is a true report
/// of a caller whose numbers do not add up, and better than quietly rounding
/// it into looking fine.
#[must_use]
pub fn fraction(part: impl Quantity, whole: impl Quantity) -> Option<f64> {
    let whole = whole.as_f64();
    // `NaN` is tested for by name rather than left to `<=`, which it loses
    // against as it loses against everything — a `whole <= 0.0` guard alone
    // would wave it through to produce a `NaN` quotient.
    if whole.is_nan() || whole <= 0.0 {
        return None;
    }
    let quotient = part.as_f64() / whole;
    quotient.is_finite().then_some(quotient)
}

/// `part` as a percentage of `whole`, or `None` if there is no whole.
///
/// Faithful, not bounded: see [`percent_clamped`] for the one to draw with.
#[must_use]
pub fn percent(part: impl Quantity, whole: impl Quantity) -> Option<f64> {
    fraction(part, whole).map(|f| f * 100.0)
}

/// `part` as a percentage of `whole`, bounded to `0.0..=100.0`.
///
/// For a number that is about to become the width of a progress bar, where a
/// value outside the range is not a report but a drawing bug.
#[must_use]
pub fn percent_clamped(part: impl Quantity, whole: impl Quantity) -> Option<f64> {
    percent(part, whole).map(|p| p.clamp(0.0, 100.0))
}

/// `part` as a whole-number percentage of `whole`, `0..=100`.
///
/// Truncates rather than rounding, which is the property worth having: a
/// truncated percentage reaches 100 only when the part really has reached the
/// whole, so `pct == 100` means finished and `pct >= 90` means genuinely past
/// nine tenths. A rounded one would report a job at 99.5% as complete, and
/// several callers compare this value rather than only displaying it.
#[must_use]
pub fn percent_whole(part: impl Quantity, whole: impl Quantity) -> Option<u32> {
    percent_clamped(part, whole).map(|p| {
        // `p` is in `0.0..=100.0` by construction, so the cast cannot
        // truncate meaningfully or lose a sign — and a float-to-integer `as`
        // saturates rather than wrapping in any case.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the value is clamped to 0..=100 one line above"
        )]
        {
            p as u32
        }
    })
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn a_part_of_a_whole_is_the_quotient_and_the_quotient_times_a_hundred() {
        assert_eq!(fraction(1_u64, 4_u64), Some(0.25));
        assert_eq!(percent(1_u64, 4_u64), Some(25.0));
        assert_eq!(percent(3_u32, 3_u32), Some(100.0));
        assert_eq!(percent(0_u64, 512_u64), Some(0.0));
        assert_eq!(percent_whole(1_u64, 3_u64), Some(33));
    }

    /// The guard the fifteen copies each wrote as a separate statement, now
    /// the only value the function can return for an empty whole — so a
    /// caller cannot divide without having answered it.
    #[test]
    fn there_is_no_percentage_of_nothing() {
        assert_eq!(fraction(0_u64, 0_u64), None);
        assert_eq!(percent(5_u64, 0_u64), None);
        assert_eq!(percent_clamped(5_u64, 0_u64), None);
        assert_eq!(percent_whole(5_u64, 0_u64), None);

        // And each caller supplies its own answer, in the same expression.
        assert_eq!(percent(0_u64, 0_u64).unwrap_or(0.0), 0.0);
        assert_eq!(percent(0_u64, 0_u64).unwrap_or(100.0), 100.0);
    }

    /// A whole that is not a positive number is not a whole. `NaN` is the one
    /// that matters: it compares `false` to everything, so a `whole <= 0.0`
    /// guard would have let it through to produce a `NaN` percentage that
    /// then silently loses every comparison downstream.
    #[test]
    fn a_whole_must_be_a_positive_number() {
        assert_eq!(percent(1_f64, -4_f64), None);
        assert_eq!(percent(1_f64, f64::NAN), None);
        assert_eq!(percent(1_f64, 0.0_f64), None);
        assert_eq!(
            percent(f64::NAN, 4_f64),
            None,
            "a NaN part has no percentage"
        );
        assert_eq!(percent(1_f64, f64::INFINITY), Some(0.0));
    }

    /// Only one of the fifteen copies bounded its result, so numbers that did
    /// not add up drew past the end of their own progress bar. Both answers
    /// are available now, and each is named for what it does.
    #[test]
    fn a_part_larger_than_its_whole_is_reported_faithfully_or_clamped_on_request() {
        assert_eq!(percent(3_u64, 2_u64), Some(150.0));
        assert_eq!(percent_clamped(3_u64, 2_u64), Some(100.0));
        assert_eq!(percent_whole(3_u64, 2_u64), Some(100));

        assert_eq!(percent(-1_i64, 2_i64), Some(-50.0));
        assert_eq!(percent_clamped(-1_i64, 2_i64), Some(0.0));
        assert_eq!(percent_whole(-1_i64, 2_i64), Some(0));
    }

    /// Truncating, not rounding — so `100` means finished, and the callers
    /// that compare rather than display are not told a job is done early.
    #[test]
    fn a_whole_number_percentage_reaches_a_hundred_only_at_the_end() {
        assert_eq!(percent_whole(999_u64, 1000_u64), Some(99));
        assert_eq!(percent_whole(9999_u64, 10_000_u64), Some(99));
        assert_eq!(percent_whole(1000_u64, 1000_u64), Some(100));
        // And it does not round a barely-started job up to 1%.
        assert_eq!(percent_whole(1_u64, 1000_u64), Some(0));
    }

    #[test]
    fn a_part_and_a_whole_may_be_different_types() {
        let count: usize = 3;
        let total: u64 = 4;
        assert_eq!(percent(count, total), Some(75.0));
        assert_eq!(percent(1.5_f32, 3_u32), Some(50.0));
    }

    /// Byte counts run past 2^53, where `f64` stops being exact. The loss is
    /// in digits the division removes, so the percentage is still right.
    #[test]
    fn a_percentage_of_a_very_large_whole_is_still_right() {
        let whole = u64::MAX;
        assert_eq!(percent_whole(whole, whole), Some(100));
        assert_eq!(percent_whole(whole / 2, whole), Some(50));
        assert_eq!(percent_whole(0_u64, whole), Some(0));
    }
}
