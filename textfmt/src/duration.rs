//! Spans of time written the way a person reads them: `01:01:01`, `2d 3h`,
//! `5m ago`.
//!
//! # Why this is a module and not six lines at the call site
//!
//! It was six lines at the call site, thirty-eight times. Unlike a byte count,
//! a duration has no single right answer — a media scrubber wants `01:05`, an
//! uptime readout wants `2d 3h`, a notification wants `5m ago` — so the copies
//! could not simply be collapsed. That is exactly what made them dangerous:
//! because variety was legitimate, *disagreement went unnoticed*.
//!
//! The audit on 2026-08-21 found the same number rendered two ways in a single
//! file. `gui/desktop/src/screen_capture.rs` assigns
//! `duration_ms: self.stats.elapsed_ms` — one field copied from another, the
//! same integer — and then formats it with two different functions. A recording
//! of one hour and one minute read **`01:01:01`** in the capture overlay and
//! **`61:01`** the instant it appeared in the recordings list, because
//! `elapsed_display` rolled over into hours and `duration_display` did not.
//!
//! Both had tests. Both passed. `elapsed_display` was asserted at 3 661 000 ms
//! and `duration_display` at 125 000 ms, so neither test ever evaluated the
//! other's input, and the disagreement had no single place it could be seen.
//! That is the shape this module exists to remove: not duplication, but
//! **duplication whose copies are each locally proven correct.**
//!
//! # Choosing a shape
//!
//! Pick by what the reader is doing, not by what the number is:
//!
//! | The reader is… | Use | Looks like |
//! |---|---|---|
//! | tracking a position in media, or watching a timer run | [`clock`] | `01:05`, `01:01:01` |
//! | the same, but the milliseconds matter (a lap, a stopwatch) | [`clock_ms`] | `01:05.250` |
//! | reading a finished span exactly (a job took *how* long?) | [`units`] | `2d 3h 4m 5s` |
//! | glancing at a *measured* span (uptime, a transfer ETA) | [`coarse`] | `2d 3h`, `1h 1m`, `1m 30s` |
//! | glancing at an *estimate*, which is not accurate to the second | [`coarse_minutes`] | `2d 3h`, `1h 1m`, `45m` |
//! | asking how long ago something happened | [`relative`] | `just now`, `5m ago`, `yesterday` |
//!
//! [`clock`] **always rolls over into hours.** That is the invariant the
//! screen-capture bug violated, and it is not configurable here: a `mm:ss` that
//! silently prints `61:01` is not a shorter format, it is a wrong one. If a
//! caller genuinely knows its input cannot reach an hour, [`clock`] still costs
//! it nothing — the hours field is omitted when it is zero.
//!
//! [`units`] and [`coarse`] differ only in how much they keep: [`units`] prints
//! every component from the largest non-zero one down to seconds, [`coarse`]
//! prints the two most significant. Neither ever prints a leading `0d` or
//! `0h`, and both print `0s` for a zero span rather than an empty string —
//! thirty-eight hand-written copies produced four different answers for zero,
//! including `""`.
//!
//! [`coarse_minutes`] is [`coarse`] with a floor: it never names seconds. The
//! two exist because the audit found the glanceable sites splitting cleanly
//! into two populations, and forcing either into the other's shape would be a
//! defect rather than a cosmetic loss. A backup that *took* ninety seconds took
//! `1m 30s`, and dropping the `30s` discards a measurement. A battery estimate
//! of ninety seconds is not accurate to the second, and printing `1m 30s`
//! *invents* one — the estimate will read `1m 40s` a minute later. Precision
//! you did not measure is a lie in the same way that precision you measured and
//! then threw away is a loss.

use alloc::format;
use alloc::string::{String, ToString};
use core::num::NonZeroU64;

/// Seconds in a minute, as a divisor the compiler knows cannot be zero.
const MINUTE: NonZeroU64 = nz(60);
/// Seconds in an hour.
const HOUR: NonZeroU64 = nz(3_600);
/// Seconds in a day.
const DAY: NonZeroU64 = nz(86_400);
/// Milliseconds in a second.
const THOUSAND: NonZeroU64 = nz(1_000);

/// `NonZeroU64::new` in const position without an `unwrap`.
///
/// Every caller passes a literal that is plainly non-zero, so the `None` arm is
/// unreachable; it yields `MIN` (one) rather than panicking, because a
/// `const fn` that can panic is a `const fn` that can fail a build for a reason
/// the reader cannot see. Dividing by one is wrong, but it is wrong *visibly*.
const fn nz(value: u64) -> NonZeroU64 {
    match NonZeroU64::new(value) {
        Some(n) => n,
        None => NonZeroU64::MIN,
    }
}

/// A span split into days, hours, minutes and seconds.
///
/// Kept as one value rather than four locals because the four are only correct
/// *together*: the recurring hand-written mistake was computing minutes as
/// `secs / 60` beside an hours term, so the minutes never wrapped and a
/// two-hour span read `2h 120m`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Parts {
    days: u64,
    hours: u64,
    minutes: u64,
    seconds: u64,
}

impl Parts {
    /// Split `secs` into its components, each reduced into its own range.
    ///
    /// Deliberately not a `const fn`: `Div<NonZeroU64>` is not const, so a
    /// const version would have to divide by `DAY.get()` — a plain `u64` the
    /// compiler must assume could be zero. That trades the one guarantee this
    /// type exists for against a `const` no caller uses.
    fn split(secs: u64) -> Self {
        Self {
            days: secs / DAY,
            hours: (secs % DAY) / HOUR,
            minutes: (secs % HOUR) / MINUTE,
            seconds: secs % MINUTE,
        }
    }
}

/// A running clock: `mm:ss`, widening to `hh:mm:ss` past an hour.
///
/// Fields are zero-padded to two digits so the text does not change width as
/// the number crosses ten — a timer that jumps from `9:59` to `10:00` drags
/// every glyph beside it one pixel left on each tick.
///
/// Hours are **not** capped at 24; a span of three days reads `72:00:00`. A
/// clock is a running total, and a scrubber that wrapped to `00:00:00` after a
/// day would be lying about a quantity the user is actively watching. Use
/// [`units`] or [`coarse`] when days should be named.
///
/// ```
/// # use textfmt::duration::clock;
/// assert_eq!(clock(0), "00:00");
/// assert_eq!(clock(65), "01:05");
/// assert_eq!(clock(3_599), "59:59");
/// assert_eq!(clock(3_600), "01:00:00");
/// assert_eq!(clock(3_661), "01:01:01");
/// // The screen-capture regression: this must never read "61:01".
/// assert_eq!(clock(3_661), "01:01:01");
/// ```
#[must_use]
pub fn clock(secs: u64) -> String {
    let minutes_total = secs / MINUTE;
    let seconds = secs % MINUTE;
    let hours = minutes_total / MINUTE;
    let minutes = minutes_total % MINUTE;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// A running clock with milliseconds: `mm:ss.mmm`, widening past an hour.
///
/// ```
/// # use textfmt::duration::clock_ms;
/// assert_eq!(clock_ms(0), "00:00.000");
/// assert_eq!(clock_ms(65_250), "01:05.250");
/// assert_eq!(clock_ms(3_661_007), "01:01:01.007");
/// ```
#[must_use]
pub fn clock_ms(total_ms: u64) -> String {
    let millis = total_ms % THOUSAND;
    let secs = total_ms / THOUSAND;
    format!("{}.{millis:03}", clock(secs))
}

/// Every component from the largest non-zero one down to seconds: `2d 3h 4m 5s`.
///
/// Use this when the exact span matters — how long a backup ran, how long a
/// process has been alive. Leading zero components are dropped, trailing ones
/// are not: `3661` is `1h 1m 1s`, not `1h 1s`, because dropping an interior
/// zero would make `1h 0m 1s` and `1h 1s` render alike while meaning spans an
/// hour apart.
///
/// ```
/// # use textfmt::duration::units;
/// assert_eq!(units(0), "0s");
/// assert_eq!(units(45), "45s");
/// assert_eq!(units(61), "1m 1s");
/// assert_eq!(units(3_601), "1h 0m 1s");
/// assert_eq!(units(90_061), "1d 1h 1m 1s");
/// ```
#[must_use]
pub fn units(secs: u64) -> String {
    let p = Parts::split(secs);
    if p.days > 0 {
        format!("{}d {}h {}m {}s", p.days, p.hours, p.minutes, p.seconds)
    } else if p.hours > 0 {
        format!("{}h {}m {}s", p.hours, p.minutes, p.seconds)
    } else if p.minutes > 0 {
        format!("{}m {}s", p.minutes, p.seconds)
    } else {
        format!("{}s", p.seconds)
    }
}

/// The two most significant components: `2d 3h`, `1h 1m`, `1m 1s`, `5s`.
///
/// Use this where the span is context rather than content — an uptime readout,
/// a transfer ETA, a battery estimate. Two components is the point at which a
/// glance stops being a read: `2d 3h` is taken in at once, `2d 3h 4m 5s` is
/// not, and the trailing precision is noise on a number that is itself an
/// estimate.
///
/// ```
/// # use textfmt::duration::coarse;
/// assert_eq!(coarse(0), "0s");
/// assert_eq!(coarse(45), "45s");
/// assert_eq!(coarse(3_661), "1h 1m");
/// assert_eq!(coarse(90_061), "1d 1h");
/// // Truncating, never rounding: 1d 23h is not "2d".
/// assert_eq!(coarse(172_799), "1d 23h");
/// ```
#[must_use]
pub fn coarse(secs: u64) -> String {
    let p = Parts::split(secs);
    if p.days > 0 {
        format!("{}d {}h", p.days, p.hours)
    } else if p.hours > 0 {
        format!("{}h {}m", p.hours, p.minutes)
    } else if p.minutes > 0 {
        format!("{}m {}s", p.minutes, p.seconds)
    } else {
        format!("{}s", p.seconds)
    }
}

/// [`coarse`], but never finer than a minute: `2d 3h`, `1h 1m`, `45m`, `30s`.
///
/// Use this for a number nobody measured to the second — a battery estimate, a
/// "time remaining", a device's connected-for readout. [`coarse`] would render
/// a 45-minute battery estimate as `45m 0s`, and that trailing `0s` is not a
/// harmless zero: it asserts a precision the estimate does not have, on a
/// figure that will read `44m 51s` a moment later. The rule this encodes is
/// that a display should not be more precise than its input.
///
/// The minutes rank therefore shows one component rather than two — there is
/// nothing below it left to show. Below a minute it falls back to seconds,
/// because `0m` is strictly less informative than `45s` and a device that was
/// plugged in seconds ago should say so.
///
/// ```
/// # use textfmt::duration::coarse_minutes;
/// assert_eq!(coarse_minutes(0), "0s");
/// assert_eq!(coarse_minutes(45), "45s");
/// assert_eq!(coarse_minutes(90), "1m");
/// assert_eq!(coarse_minutes(2_700), "45m");
/// assert_eq!(coarse_minutes(3_661), "1h 1m");
/// assert_eq!(coarse_minutes(90_061), "1d 1h");
/// ```
#[must_use]
pub fn coarse_minutes(secs: u64) -> String {
    let p = Parts::split(secs);
    if p.days > 0 {
        format!("{}d {}h", p.days, p.hours)
    } else if p.hours > 0 {
        format!("{}h {}m", p.hours, p.minutes)
    } else if p.minutes > 0 {
        format!("{}m", p.minutes)
    } else {
        format!("{}s", p.seconds)
    }
}

/// How long ago something happened: `just now`, `5m ago`, `yesterday`, `3w ago`.
///
/// The argument is an *elapsed* span, not a timestamp — compute it with
/// `now.saturating_sub(then)` so that a clock that has stepped backwards yields
/// zero rather than an enormous span.
///
/// The ladder above a week is deliberately approximate and says so in its
/// units: a month is 30 days and a year is 365, because this is a phrase a user
/// skims, not an interval they compute with. Anything needing real calendar
/// arithmetic should use `tzrules` and print a date.
///
/// ```
/// # use textfmt::duration::relative;
/// assert_eq!(relative(0), "just now");
/// assert_eq!(relative(59), "just now");
/// assert_eq!(relative(60), "1m ago");
/// assert_eq!(relative(3_600), "1h ago");
/// assert_eq!(relative(86_400), "yesterday");
/// assert_eq!(relative(172_800), "2d ago");
/// assert_eq!(relative(604_800), "1w ago");
/// ```
#[must_use]
pub fn relative(elapsed_secs: u64) -> String {
    const WEEK: NonZeroU64 = nz(604_800);
    const MONTH: NonZeroU64 = nz(2_592_000);
    const YEAR: NonZeroU64 = nz(31_536_000);

    if elapsed_secs < MINUTE.get() {
        "just now".to_string()
    } else if elapsed_secs < HOUR.get() {
        format!("{}m ago", elapsed_secs / MINUTE)
    } else if elapsed_secs < DAY.get() {
        format!("{}h ago", elapsed_secs / HOUR)
    } else if elapsed_secs < DAY.get().saturating_mul(2) {
        "yesterday".to_string()
    } else if elapsed_secs < WEEK.get() {
        format!("{}d ago", elapsed_secs / DAY)
    } else if elapsed_secs < MONTH.get() {
        format!("{}w ago", elapsed_secs / WEEK)
    } else if elapsed_secs < YEAR.get() {
        format!("{}mo ago", elapsed_secs / MONTH)
    } else {
        format!("{}y ago", elapsed_secs / YEAR)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests {
    use super::{clock, clock_ms, coarse, coarse_minutes, relative, units, Parts};
    use alloc::format;

    // --- the regression this module was written for ---

    /// `gui/desktop/src/screen_capture.rs` copied one field into another
    /// (`duration_ms: self.stats.elapsed_ms`) and formatted the two with
    /// different functions, so one recording had two lengths. There is now one
    /// function, so the only way to reintroduce the split is to stop calling
    /// it — which this test cannot prevent, but the next one names.
    #[test]
    fn the_same_span_has_exactly_one_clock_rendering() {
        let elapsed_ms: u64 = 3_661_000;
        assert_eq!(clock(elapsed_ms / 1000), "01:01:01");
        assert_eq!(clock_ms(elapsed_ms), "01:01:01.000");
    }

    /// The specific wrong answer: a `mm:ss` with no hour branch prints the
    /// total minutes, so an hour-long span reads `61:01`. Nothing may produce
    /// that string.
    #[test]
    fn a_clock_past_an_hour_never_prints_total_minutes() {
        for secs in [3_600_u64, 3_661, 7_322, 86_399, 86_400, 359_999] {
            let s = clock(secs);
            let colons = s.matches(':').count();
            assert_eq!(colons, 2, "{secs}s rendered as {s}, which has no hours field");
        }
        assert_eq!(clock(3_661), "01:01:01");
        assert_ne!(clock(3_661), "61:01");
    }

    // --- clock ---

    #[test]
    fn clock_pads_both_fields() {
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(9), "00:09");
        assert_eq!(clock(69), "01:09");
        assert_eq!(clock(600), "10:00");
    }

    #[test]
    fn clock_boundary_at_the_hour() {
        assert_eq!(clock(3_599), "59:59");
        assert_eq!(clock(3_600), "01:00:00");
    }

    /// A clock is a running total, so hours accumulate past a day rather than
    /// wrapping. Three days of uptime on a stopwatch is `72:00:00`.
    #[test]
    fn clock_hours_are_not_capped_at_a_day() {
        assert_eq!(clock(86_400), "24:00:00");
        assert_eq!(clock(259_200), "72:00:00");
    }

    #[test]
    fn clock_does_not_overflow_at_the_top_of_the_range() {
        let s = clock(u64::MAX);
        assert!(s.matches(':').count() == 2, "{s}");
    }

    // --- clock_ms ---

    #[test]
    fn clock_ms_pads_millis_to_three() {
        assert_eq!(clock_ms(0), "00:00.000");
        assert_eq!(clock_ms(7), "00:00.007");
        assert_eq!(clock_ms(1_070), "00:01.070");
        assert_eq!(clock_ms(65_250), "01:05.250");
    }

    #[test]
    fn clock_ms_agrees_with_clock_on_the_seconds_part() {
        for ms in [0_u64, 999, 1_000, 65_250, 3_599_999, 3_600_000, 3_661_007] {
            let expected = format!("{}.", clock(ms / 1000));
            assert!(clock_ms(ms).starts_with(&expected),
                    "clock_ms({ms}) = {} disagrees with clock({})", clock_ms(ms), ms / 1000);
        }
    }

    // --- units ---

    #[test]
    fn units_drops_leading_components_only() {
        assert_eq!(units(0), "0s");
        assert_eq!(units(5), "5s");
        assert_eq!(units(60), "1m 0s");
        assert_eq!(units(3_600), "1h 0m 0s");
        assert_eq!(units(86_400), "1d 0h 0m 0s");
    }

    /// An interior zero is kept: `1h 1s` would be indistinguishable from a span
    /// an hour shorter written the same way.
    #[test]
    fn units_keeps_interior_zeroes() {
        assert_eq!(units(3_601), "1h 0m 1s");
        assert_ne!(units(3_601), "1h 1s");
    }

    #[test]
    fn units_components_stay_in_range() {
        assert_eq!(units(90_061), "1d 1h 1m 1s");
        assert_eq!(units(172_800), "2d 0h 0m 0s");
    }

    // --- coarse ---

    #[test]
    fn coarse_keeps_two_components() {
        assert_eq!(coarse(0), "0s");
        assert_eq!(coarse(5), "5s");
        assert_eq!(coarse(61), "1m 1s");
        assert_eq!(coarse(3_661), "1h 1m");
        assert_eq!(coarse(90_061), "1d 1h");
    }

    /// Truncation, not rounding — `1d 23h 59m` must not become `2d 0h`, which
    /// would show a span as longer than it is.
    #[test]
    fn coarse_truncates_and_never_rounds_up() {
        assert_eq!(coarse(172_799), "1d 23h");
        assert_eq!(coarse(3_599), "59m 59s");
    }

    #[test]
    fn coarse_never_exceeds_units() {
        for secs in [0_u64, 1, 59, 60, 3_599, 3_600, 86_399, 86_400, 1_000_000] {
            let c = coarse(secs);
            let u = units(secs);
            assert!(c.split_whitespace().count() <= u.split_whitespace().count(),
                    "coarse({secs}) = {c} is longer than units({secs}) = {u}");
        }
    }

    // --- coarse_minutes ---

    #[test]
    fn coarse_minutes_never_names_seconds_above_a_minute() {
        // The whole point of the shape: an estimate must not claim a
        // precision it does not have. Sweep the first six hours, where the
        // seconds field would otherwise appear.
        for secs in (60..21_600_u64).step_by(37) {
            let s = coarse_minutes(secs);
            assert!(
                !s.ends_with('s'),
                "coarse_minutes({secs}) = {s} names seconds"
            );
        }
    }

    #[test]
    fn coarse_minutes_agrees_with_coarse_wherever_coarse_is_minute_floored() {
        // Above an hour the two shapes are the same function. Any divergence
        // there would mean one of them had grown a second definition of what
        // "the two most significant components" are -- which is exactly the
        // failure this module was written to end, reintroduced inside it.
        for secs in (3_600..2_000_000_u64).step_by(1_009) {
            assert_eq!(coarse(secs), coarse_minutes(secs), "at {secs}s");
        }
    }

    #[test]
    fn coarse_minutes_prefers_seconds_to_a_bare_zero_below_a_minute() {
        assert_eq!(coarse_minutes(0), "0s");
        assert_eq!(coarse_minutes(1), "1s");
        assert_eq!(coarse_minutes(59), "59s");
        assert_eq!(coarse_minutes(60), "1m");
    }

    // --- relative ---

    #[test]
    fn relative_ladder_boundaries() {
        assert_eq!(relative(0), "just now");
        assert_eq!(relative(59), "just now");
        assert_eq!(relative(60), "1m ago");
        assert_eq!(relative(3_599), "59m ago");
        assert_eq!(relative(3_600), "1h ago");
        assert_eq!(relative(86_399), "23h ago");
        assert_eq!(relative(86_400), "yesterday");
        assert_eq!(relative(172_799), "yesterday");
        assert_eq!(relative(172_800), "2d ago");
        assert_eq!(relative(604_799), "6d ago");
        assert_eq!(relative(604_800), "1w ago");
        assert_eq!(relative(2_591_999), "4w ago");
        assert_eq!(relative(2_592_000), "1mo ago");
        assert_eq!(relative(31_535_999), "12mo ago");
        assert_eq!(relative(31_536_000), "1y ago");
    }

    /// The ladder must be monotonic: a longer span may never read as a shorter
    /// one. This is the property the five hand-written copies could not state,
    /// because each covered a different part of the range.
    #[test]
    fn relative_never_reports_a_longer_span_as_a_shorter_rank() {
        const RANK: [(&str, u8); 8] = [
            ("just now", 0), ("m ago", 1), ("h ago", 2), ("yesterday", 3),
            ("d ago", 4), ("w ago", 5), ("mo ago", 6), ("y ago", 7),
        ];
        fn rank(s: &str) -> u8 {
            // `mo ago` must be tested before `m ago`, and `yesterday` is its
            // own rank rather than a suffix.
            if s == "just now" { return 0; }
            if s == "yesterday" { return 3; }
            for (suffix, r) in RANK.iter().rev() {
                if s.ends_with(suffix) { return *r; }
            }
            u8::MAX
        }
        let mut last = 0_u8;
        let mut secs = 0_u64;
        while secs < 40_000_000 {
            let r = rank(&relative(secs));
            assert_ne!(r, u8::MAX, "relative({secs}) = {} is unranked", relative(secs));
            assert!(r >= last, "relative({secs}) = {} ranks below the previous span", relative(secs));
            last = r;
            secs = secs.saturating_add(997);
        }
    }

    #[test]
    fn relative_does_not_overflow_at_the_top_of_the_range() {
        assert!(relative(u64::MAX).ends_with("y ago"));
    }

    // --- Parts ---

    #[test]
    fn parts_reduce_each_component_into_its_own_range() {
        for secs in [0_u64, 1, 59, 60, 3_599, 3_600, 86_399, 86_400, 1_000_000, u64::MAX] {
            let p = Parts::split(secs);
            assert!(p.hours < 24, "{secs}: hours {} out of range", p.hours);
            assert!(p.minutes < 60, "{secs}: minutes {} out of range", p.minutes);
            assert!(p.seconds < 60, "{secs}: seconds {} out of range", p.seconds);
        }
    }

    /// The components must add back up to the input — the check that catches a
    /// missing `%`, which is how the hand-written copies went wrong.
    #[test]
    fn parts_round_trip() {
        for secs in [0_u64, 1, 59, 61, 3_601, 86_401, 90_061, 1_000_000] {
            let p = Parts::split(secs);
            let back = p.days.saturating_mul(86_400)
                .saturating_add(p.hours.saturating_mul(3_600))
                .saturating_add(p.minutes.saturating_mul(60))
                .saturating_add(p.seconds);
            assert_eq!(back, secs, "components of {secs} do not sum back");
        }
    }
}
