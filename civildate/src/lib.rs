//! Civil dates from a Unix timestamp: the calendar half of telling the time.
//!
//! # Why this crate exists
//!
//! Converting "seconds since 1970" into "the 20th of May" is arithmetic that
//! looks trivial and is not, and on 2026-09-10 this tree had **six** separate
//! copies of it: `userspace/cal`, `gui/toolkit/src/date.rs`, and the
//! `archivemanager`, `backup`, `rssreader` and `taskscheduler` apps. Two lane-B
//! programs additionally had their own `days_in_month` and `day_of_week`
//! (`cal` and `cron`), which is four more copies of the easy part.
//!
//! That is the same shape as [`monoclock`] and `hmac`: a small calculation
//! that several programs need, that each of them gets slightly differently,
//! and whose errors are quiet. A wrong monotonic unit expired an SSH grace
//! period in 120 microseconds. A wrong civil date does not crash either — it
//! prints a plausible day.
//!
//! # What is deliberately *not* here
//!
//! **Reading the clock.** The boundary is at "what the number means", not at
//! "where it came from", exactly as [`monoclock`]'s is. A caller passes the
//! seconds it obtained and decides for itself what to do when it has none —
//! and that decision is the interesting one, because the wrong answer to it is
//! how `cal` came to print a calendar for January 2025 whenever the clock was
//! unreadable, and how `at` came to schedule jobs against a hard-coded
//! 2026-05-20.
//!
//! **Time zones and leap seconds.** Everything here is UTC and every day is
//! 86,400 seconds, which is what a Unix timestamp already assumes.
//!
//! # The conversion is not the obvious loop
//!
//! The copy this was extracted from counted years forward from 1970 and then
//! months forward from January. That is correct for dates after the epoch and
//! silently wrong before it: with a negative day count the year loop exits
//! immediately, the month loop exits immediately, and the result is 1970 with
//! a day number of zero or less. `unix_to_date(-1)` gave `(1970, 1, 0)` rather
//! than `(1969, 12, 31)`.
//!
//! [`civil_from_days`] uses Howard Hinnant's era-based algorithm instead,
//! which is branch-free, exact for the whole range, and correct on both sides
//! of the epoch. The tests pin both the vectors the old loop got right and the
//! ones it got wrong.

#![no_std]

/// A civil date and time of day, in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDateTime {
    /// Proleptic Gregorian year; may be negative.
    pub year: i32,
    /// 1..=12.
    pub month: u32,
    /// 1..=31.
    pub day: u32,
    /// 0..=23.
    pub hour: u32,
    /// 0..=59.
    pub minute: u32,
    /// 0..=59. Leap seconds do not exist in Unix time, so never 60.
    pub second: u32,
    /// 0 = Sunday, 6 = Saturday.
    pub weekday: u32,
}

/// Is `year` a leap year in the proleptic Gregorian calendar?
#[must_use]
pub const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1..=12) of `year`; 0 for a month number outside that range.
#[must_use]
pub const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since 1970-01-01 for a civil date, negative before the epoch.
///
/// Howard Hinnant's `days_from_civil`. Exact for every year the type can hold.
///
/// # Overflow
///
/// `clippy::arithmetic_side_effects` is allowed here, and the allow is a claim
/// that has to hold rather than a silence. Every operand is widened to `i64`
/// before any arithmetic, and the year comes from an `i32`, so `|y| <=
/// 2^31`. The largest intermediate is `era * 146_097`, bounded by
/// `2^31 * 146_097 ~= 3.1e14`, against an `i64` maximum of `9.2e18` -- four
/// orders of magnitude of headroom. `round_trips_across_four_centuries` walks
/// 1800..2200 in both directions, and the two epoch tests cover the sign
/// change, which is the only place the expression shape differs.
///
/// The widening is not cosmetic: this was `year - 1` on the `i32` directly,
/// which overflows at `i32::MIN` and is the one input that could have.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub const fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    // Widen first, then subtract: see "Overflow" above.
    let y = year as i64;
    let y = if month <= 2 { y - 1 } else { y };
    let m = month as i64;
    let d = day as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The civil date `z` days after 1970-01-01, `z` negative meaning before it.
///
/// Howard Hinnant's `civil_from_days`, the inverse of [`days_from_civil`].
///
/// Same overflow argument as [`days_from_civil`]: `z` is a day count, so a
/// value large enough to overflow `z + 719_468` is roughly 2.5e16 years and
/// cannot arrive from a `i64` second count divided by 86,400.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub const fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Day of the week for a civil date. 0 = Sunday.
#[must_use]
pub const fn day_of_week(year: i32, month: u32, day: u32) -> u32 {
    weekday_from_days(days_from_civil(year, month, day))
}

/// Day of the week `days` after the epoch. 1970-01-01 was a Thursday.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
const fn weekday_from_days(days: i64) -> u32 {
    // `rem_euclid` rather than `%`: for a date before the epoch `%` yields a
    // negative remainder and the weekday would be a negative index.
    ((days.rem_euclid(7) + 4) % 7) as u32
}

/// The date only, `secs` seconds after the Unix epoch.
#[must_use]
pub const fn unix_to_date(secs: i64) -> (i32, u32, u32) {
    civil_from_days(secs.div_euclid(86_400))
}

/// The full civil date and time of day, `secs` seconds after the Unix epoch.
///
/// `div_euclid`/`rem_euclid` are what make this correct before 1970: plain
/// `/` and `%` truncate toward zero, so a negative timestamp would land a day
/// late with a negative time of day.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub const fn unix_to_datetime(secs: i64) -> CivilDateTime {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400); // second of day, always [0, 86399]
    let (year, month, day) = civil_from_days(days);
    CivilDateTime {
        year,
        month,
        day,
        hour: (sod / 3600) as u32,
        minute: ((sod % 3600) / 60) as u32,
        second: (sod % 60) as u32,
        weekday: weekday_from_days(days),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        civil_from_days, day_of_week, days_from_civil, days_in_month, is_leap_year, unix_to_date,
        unix_to_datetime,
    };

    /// The vectors `userspace/cal` carried before this crate existed, so the
    /// extraction is pinned to the behaviour it replaced rather than merely
    /// to my reading of it.
    #[test]
    fn cal_vectors_still_hold() {
        assert_eq!(unix_to_date(0), (1970, 1, 1));
        assert_eq!(unix_to_date(86_399), (1970, 1, 1)); // one second to midnight
        assert_eq!(unix_to_date(86_400), (1970, 1, 2));
        assert_eq!(unix_to_date(951_782_400), (2000, 2, 29)); // the 400-year rule
        assert_eq!(unix_to_date(951_868_800), (2000, 3, 1));
        assert_eq!(unix_to_date(1_709_164_800), (2024, 2, 29));
        assert_eq!(unix_to_date(1_767_225_600), (2026, 1, 1));
    }

    /// The cases the loop this replaced got wrong. It counted forward from
    /// 1970, so a negative day count exited both loops immediately and left
    /// the day at zero or below.
    #[test]
    fn dates_before_the_epoch_are_right() {
        assert_eq!(unix_to_date(-1), (1969, 12, 31));
        assert_eq!(unix_to_date(-86_400), (1969, 12, 31));
        assert_eq!(unix_to_date(-86_401), (1969, 12, 30));
        // 1900 is not a leap year; 2000 is. Both sides of the century rule.
        assert_eq!(unix_to_date(-2_208_988_800), (1900, 1, 1));
        assert_eq!(unix_to_date(-2_203_977_600), (1900, 2, 28));
        assert_eq!(unix_to_date(-2_203_891_200), (1900, 3, 1));
    }

    #[test]
    fn time_of_day_is_never_negative_before_the_epoch() {
        let dt = unix_to_datetime(-1);
        assert_eq!((dt.year, dt.month, dt.day), (1969, 12, 31));
        assert_eq!((dt.hour, dt.minute, dt.second), (23, 59, 59));
    }

    #[test]
    fn time_of_day_splits_correctly() {
        let dt = unix_to_datetime(0);
        assert_eq!((dt.hour, dt.minute, dt.second), (0, 0, 0));
        let dt = unix_to_datetime(86_399);
        assert_eq!((dt.hour, dt.minute, dt.second), (23, 59, 59));
        // 2026-05-20 10:00:00 UTC -- the constant `at` used to hard-code.
        let dt = unix_to_datetime(1_779_271_200);
        assert_eq!((dt.year, dt.month, dt.day), (2026, 5, 20));
        assert_eq!((dt.hour, dt.minute), (10, 0));
    }

    #[test]
    fn weekdays_are_right_on_both_sides_of_the_epoch() {
        // 1970-01-01 was a Thursday.
        assert_eq!(unix_to_datetime(0).weekday, 4);
        assert_eq!(day_of_week(1970, 1, 1), 4);
        // 1969-12-31 was a Wednesday -- the case a plain `%` gets wrong.
        assert_eq!(day_of_week(1969, 12, 31), 3);
        assert_eq!(day_of_week(2000, 1, 1), 6); // Saturday
        assert_eq!(day_of_week(2026, 9, 10), 4); // Thursday
    }

    #[test]
    fn round_trips_across_four_centuries() {
        // Every 13th day from 1800 to 2200, both directions.
        let mut d = days_from_civil(1800, 1, 1);
        let end = days_from_civil(2200, 1, 1);
        while d < end {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "{y}-{m}-{dd}");
            assert!((1..=12).contains(&m));
            assert!(dd >= 1 && dd <= days_in_month(y, m));
            d += 13;
        }
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(1900)); // divisible by 100
        assert!(is_leap_year(2000)); // divisible by 400
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 13), 0); // out of range, not a panic
    }
}
