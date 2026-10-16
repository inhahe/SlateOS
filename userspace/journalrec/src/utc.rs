//! A record's time as a UTC date: `syslogd` writes it as a record's `time`
//! field, and both readers show it.
//!
//! `journalctl` and `syslogd` each turned seconds into a date by subtracting
//! one year's days at a time from the day count. That is one step per year
//! since 1970 -- harmless for a real clock, and about 584 billion steps for a
//! record whose `ts` is near `u64::MAX`, which anything that can write a line
//! into the log can put there: `journalctl` and `syslogd tail` then spun for
//! many minutes on the one record. Here it is a fixed handful of steps for any
//! value -- Howard Hinnant's `civil_from_days`, as `userdb` uses.

/// A moment, as UTC calendar fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    /// The year: 1970 and on, and past 9999 for a large enough value.
    pub year: u64,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    /// 0 to 23.
    pub hour: u32,
    /// 0 to 59.
    pub minute: u32,
    /// 0 to 59.
    pub second: u32,
}

/// Days in 400 Gregorian years.
const DAYS_PER_ERA: u64 = 146_097;
/// Days from 0000-03-01, where an era starts, to 1970-01-01.
const EPOCH_FROM_ERA_START: u64 = 719_468;

impl Utc {
    /// The date and time `secs` seconds after 1970-01-01 00:00:00 UTC, in a
    /// fixed number of steps for any `secs`.
    #[must_use]
    pub fn from_unix(secs: u64) -> Utc {
        let of_day = secs % 86_400;
        // `secs / 86_400` is at most 2.2e14, so adding the epoch's offset
        // cannot overflow; saturating says so without an assertion.
        let z = (secs / 86_400).saturating_add(EPOCH_FROM_ERA_START);
        // Every quantity below is bounded by the comment beside it, each far
        // inside u64, so plain arithmetic would not overflow either; the
        // checked forms are the workspace's rule, not a live concern.
        let era = z / DAYS_PER_ERA;
        let doe = z % DAYS_PER_ERA; // day of era, [0, 146096]
        let yoe = doe
            .saturating_sub(doe / 1_460)
            .saturating_add(doe / 36_524)
            .saturating_sub(doe / 146_096)
            / 365; // year of era, [0, 399]
        let doy = doe.saturating_sub(
            yoe.saturating_mul(365)
                .saturating_add(yoe / 4)
                .saturating_sub(yoe / 100),
        ); // day of year from March 1, [0, 365]
        let mp = doy.saturating_mul(5).saturating_add(2) / 153; // March = 0, [0, 11]
        let day = doy
            .saturating_sub(mp.saturating_mul(153).saturating_add(2) / 5)
            .saturating_add(1); // [1, 31]
        let month = if mp < 10 {
            mp.saturating_add(3)
        } else {
            mp.saturating_sub(9)
        }; // [1, 12]
        let year = era
            .saturating_mul(400)
            .saturating_add(yoe)
            .saturating_add(u64::from(month <= 2));
        Utc {
            year,
            month: small(month),
            day: small(day),
            hour: small(of_day / 3_600),
            minute: small(of_day % 3_600 / 60),
            second: small(of_day % 60),
        }
    }
}

impl Utc {
    /// The seconds since the epoch this is, or `None` when it is not a real
    /// moment -- a month, or a day for that month and year, or an hour,
    /// minute or second out of range -- or one before 1970, which no record
    /// can carry. The inverse of [`Utc::from_unix`], in as few steps (Howard
    /// Hinnant's `days_from_civil`).
    ///
    /// `journalctl --since` counted the days to a date a year at a time from
    /// 1970 -- unbounded for a year a user can type -- took a date before
    /// 1970 as the same day of 1970, and let February 31 and 25:99 roll over
    /// into the next month and hour.
    #[must_use]
    pub fn to_unix(&self) -> Option<u64> {
        let Utc {
            year,
            month,
            day,
            hour,
            minute,
            second,
        } = *self;
        if year < 1970
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        let m = u64::from(month);
        // The year counted from March, so a leap day ends it.
        let y = if m <= 2 { year.saturating_sub(1) } else { year };
        let era = y / 400;
        let yoe = y % 400; // [0, 399]
        let mp = if m > 2 {
            m.saturating_sub(3)
        } else {
            m.saturating_add(9)
        }; // [0, 11]
        let doy = (mp.saturating_mul(153).saturating_add(2) / 5)
            .saturating_add(u64::from(day).saturating_sub(1)); // [0, 365]
        let doe = yoe
            .saturating_mul(365)
            .saturating_add(yoe / 4)
            .saturating_sub(yoe / 100)
            .saturating_add(doy); // [0, 146096]
        let days = era
            .checked_mul(DAYS_PER_ERA)?
            .checked_add(doe)?
            .checked_sub(EPOCH_FROM_ERA_START)?;
        let of_day = u64::from(hour)
            .saturating_mul(3_600)
            .saturating_add(u64::from(minute).saturating_mul(60))
            .saturating_add(u64::from(second));
        days.checked_mul(86_400)?.checked_add(of_day)
    }
}

/// How many days `month` of `year` has.
fn days_in_month(year: u64, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Whether `year` is a leap year of the Gregorian calendar.
fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// A value already known to be small -- a month, a day, an hour -- as a u32.
fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(year: u64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Utc {
        Utc {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn the_epoch_and_known_moments() {
        assert_eq!(Utc::from_unix(0), utc(1970, 1, 1, 0, 0, 0));
        assert_eq!(Utc::from_unix(1_716_000_000), utc(2024, 5, 18, 2, 40, 0));
        // The last second of a leap day, and the first of the day after.
        assert_eq!(Utc::from_unix(951_868_799), utc(2000, 2, 29, 23, 59, 59));
        assert_eq!(Utc::from_unix(951_868_800), utc(2000, 3, 1, 0, 0, 0));
        // 2100 is not a leap year.
        assert_eq!(Utc::from_unix(4_107_542_400), utc(2100, 3, 1, 0, 0, 0));
        assert_eq!(Utc::from_unix(4_107_542_399), utc(2100, 2, 28, 23, 59, 59));
        assert_eq!(
            Utc::from_unix(253_402_300_799),
            utc(9999, 12, 31, 23, 59, 59)
        );
    }

    /// The value that took the old loop half a trillion steps.
    #[test]
    fn the_largest_value_is_a_date_at_once() {
        assert_eq!(
            Utc::from_unix(u64::MAX),
            utc(584_554_051_223, 11, 9, 7, 0, 15)
        );
    }

    /// Every moment goes there and back, the largest included.
    #[test]
    fn to_unix_inverts_from_unix() {
        let mut secs = 0u64;
        while secs < 300_000_000_000 {
            assert_eq!(Utc::from_unix(secs).to_unix(), Some(secs), "{secs}");
            secs = secs.saturating_add(86_400 * 389 + 7_777);
        }
        assert_eq!(Utc::from_unix(u64::MAX).to_unix(), Some(u64::MAX));
        assert_eq!(utc(2024, 5, 18, 2, 40, 0).to_unix(), Some(1_716_000_000));
        assert_eq!(utc(1970, 1, 1, 0, 0, 0).to_unix(), Some(0));
    }

    /// What is not a real moment, or is before any record, is refused
    /// rather than rolled over into the next month, hour or year.
    #[test]
    fn to_unix_refuses_what_is_not_a_moment() {
        for bad in [
            utc(2024, 2, 30, 0, 0, 0),
            utc(2023, 2, 29, 0, 0, 0),
            utc(2100, 2, 29, 0, 0, 0),
            utc(2024, 4, 31, 0, 0, 0),
            utc(2024, 13, 1, 0, 0, 0),
            utc(2024, 0, 1, 0, 0, 0),
            utc(2024, 1, 0, 0, 0, 0),
            utc(2024, 1, 1, 24, 0, 0),
            utc(2024, 1, 1, 0, 60, 0),
            utc(2024, 1, 1, 0, 0, 60),
            utc(1969, 12, 31, 23, 59, 59),
            // Past the largest moment there is.
            utc(584_554_051_223, 11, 9, 7, 0, 16),
            utc(u64::MAX, 12, 31, 0, 0, 0),
        ] {
            assert_eq!(bad.to_unix(), None, "{bad:?}");
        }
        assert!(utc(2024, 2, 29, 0, 0, 0).to_unix().is_some());
        assert!(utc(2000, 2, 29, 0, 0, 0).to_unix().is_some());
    }

    /// Against the old year-by-year walk, which is right where it finishes.
    #[test]
    fn it_agrees_with_counting_the_days() {
        fn leap(y: u64) -> bool {
            (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
        }
        let mut secs = 0u64;
        while secs < 5_000_000_000 {
            let mut days = secs / 86_400;
            let mut year = 1970;
            while days >= if leap(year) { 366 } else { 365 } {
                days -= if leap(year) { 366 } else { 365 };
                year += 1;
            }
            let lengths = [
                31,
                if leap(year) { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            let mut month = 1;
            for len in lengths {
                if days < len {
                    break;
                }
                days -= len;
                month += 1;
            }
            let got = Utc::from_unix(secs);
            assert_eq!(
                (got.year, got.month, u64::from(got.day)),
                (year, month, days + 1),
                "{secs}"
            );
            secs += 86_400 * 7 + 3_601;
        }
    }
}
