//! Instants: one answer to "what time is it, and where?" for the whole tree.
//!
//! # Why this exists
//!
//! [`date::Date`](crate::date) says plainly that it is a *civil* date, and
//! that turning an instant into one "is the caller's business, because only
//! the caller knows which zone the user meant". That is true of **which
//! zone**. It is not true of **how the conversion is done** — and thirteen
//! callers each wrote that half for themselves.
//!
//! What they produced, all rendering the same kind of thing (a moment a file
//! was written, a backup ran, a snapshot was taken):
//!
//! | Program | Same instant renders as |
//! |---|---|
//! | file explorer | `2026-08-18 16:30` |
//! | archive manager | `2026-08-18 16:30` |
//! | task scheduler | `2026-08-18 16:30` |
//! | backup | `2026-08-18 16:30:45` |
//! | RSS reader | `2026-08-18 16:30` |
//! | **undelete** | **`2026-09-04 16:30`** |
//! | **system restore** | **`D20683`** |
//! | **backup settings** | **`Day 20683 16:30`** |
//!
//! The last three are not style differences.
//!
//! * **Undelete was fourteen days wrong, and getting worse.** It computed the
//!   year as `days / 365` and the month as `remaining / 30` — not an
//!   approximation but a drift, about five days per year of extra error,
//!   already a fortnight by 2026. The deletion date is the one column a user
//!   scans to tell two copies of a file apart, so this is the field whose
//!   wrongness costs the most.
//! * **System restore labelled restore points `D20683`.** A restore point is
//!   the most consequence-laden thing in the system to pick by date.
//! * **Backup settings listed runs as `Day 20683 16:30`** while the backup
//!   application itself listed the same runs as `2026-08-18 16:30:45`.
//!
//! None of the thirteen applied a timezone. Every one of them was `secs %
//! 86400`, which is UTC — the same defect the taskbar clock had (see
//! `gui/desktop/src/main.rs`'s `current_time_string`).
//!
//! # The zone is an argument, and is never defaulted
//!
//! Every function here takes a [`Tz`]. A caller with no zone to offer must
//! write [`Tz::utc()`] and say why, which leaves a mark that can be found by
//! grep and fixed when zone plumbing reaches that program. `secs % 86400`
//! leaves no such mark: it is UTC by accident, indistinguishable at a glance
//! from arithmetic that meant it, which is how thirteen programs shipped a
//! UTC clock without one of them recording the decision.
//!
//! # What it deliberately does not do
//!
//! It does not choose 12- or 24-hour, and it does not choose whether seconds
//! are shown. Those are *settings*, and a setting belongs with whatever owns
//! it — the taskbar's `ClockDisplay`, the alarm clock's format enum. This
//! module offers both renderings ([`DateTime::clock`] and
//! [`DateTime::clock12`]) and lets the owner of the setting pick.

use crate::date::{Date, month_name, month_short_name};
use tzrules::Tz;

/// Seconds in a day.
const SECS_PER_DAY: i64 = 86_400;
/// Seconds in an hour.
const SECS_PER_HOUR: u32 = 3_600;
/// Seconds in a minute.
const SECS_PER_MINUTE: u32 = 60;

/// A wall-clock reading: a civil date plus a time of day.
///
/// Stored as a [`Date`] and a seconds-into-the-day count rather than six
/// integers, for the reason [`Date`] itself is a day number rather than a
/// triple: every value that can be constructed is a real one, and the parts
/// are derived on demand. There is no way to build a `DateTime` naming
/// 25 o'clock.
///
/// It carries **no zone**. It is the answer a zone has already been applied
/// to — "what a clock in New York read at that moment" — so subtracting two
/// of them, or turning one back into an instant, is not offered: those are
/// questions about instants, and the instant is what you still have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DateTime {
    /// The civil date this reading falls on.
    date: Date,
    /// Seconds since local midnight, `0..86_400`.
    secs_of_day: u32,
}

impl DateTime {
    /// What a clock in `tz` read at the Unix instant `unix_secs`.
    ///
    /// ```
    /// use guitk::datetime::DateTime;
    /// use guitk::tzrules::Tz;
    ///
    /// // 2026-08-18 16:30:45 UTC, as New York reads it (EDT, -4h).
    /// let ny = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("POSIX TZ");
    /// assert_eq!(DateTime::at(1_787_070_645, &ny).stamp(), "2026-08-18 12:30");
    /// ```
    #[must_use]
    pub fn at(unix_secs: i64, tz: &Tz) -> Self {
        let local = unix_secs.saturating_add(i64::from(tz.lookup(unix_secs).gmtoff));
        Self::utc(local)
    }

    /// What a clock in UTC read at the Unix instant `unix_secs`.
    ///
    /// Also the constructor for an already-offset local instant, which is why
    /// it is public: [`at`](Self::at) is the composition of "shift by the
    /// offset in force" and this.
    #[must_use]
    pub fn utc(unix_secs: i64) -> Self {
        // `div_euclid`/`rem_euclid`, not `/` and `%`: instants before 1970
        // must land on the day that *contains* them, and truncating division
        // rounds toward zero — which would put 1969-12-31 23:00 on
        // 1970-01-01 at minus one hour. Euclidean division is the one that
        // agrees with a calendar.
        let days = unix_secs.div_euclid(SECS_PER_DAY);
        let secs_of_day = unix_secs.rem_euclid(SECS_PER_DAY);
        Self {
            date: Date::from_days_since_epoch(i32::try_from(days).unwrap_or(i32::MAX)),
            // `rem_euclid` by a positive divisor is in `0..86_400`, so this
            // conversion cannot fail; `unwrap_or` rather than `expect` keeps
            // the module free of panics on any input at all.
            secs_of_day: u32::try_from(secs_of_day).unwrap_or(0),
        }
    }

    /// A reading built from its parts, with out-of-range parts clamped.
    ///
    /// Total, for the same reason [`Date::from_ymd`] is: a caller assembling
    /// a reading from user input or from a filesystem's on-disk fields should
    /// not be made to hold an `Option` it must discharge before it can draw
    /// anything. 25 o'clock becomes 23:59:59.
    #[must_use]
    pub fn from_parts(date: Date, hour: u32, minute: u32, second: u32) -> Self {
        let secs_of_day = hour
            .min(23)
            .saturating_mul(SECS_PER_HOUR)
            .saturating_add(minute.min(59).saturating_mul(SECS_PER_MINUTE))
            .saturating_add(second.min(59));
        Self { date, secs_of_day }
    }

    /// The civil date this reading falls on.
    #[must_use]
    pub fn date(self) -> Date {
        self.date
    }

    /// The hour, `0..=23`.
    #[must_use]
    pub fn hour(self) -> u32 {
        self.secs_of_day / SECS_PER_HOUR
    }

    /// The minute, `0..=59`.
    #[must_use]
    pub fn minute(self) -> u32 {
        (self.secs_of_day % SECS_PER_HOUR) / SECS_PER_MINUTE
    }

    /// The second, `0..=59`.
    #[must_use]
    pub fn second(self) -> u32 {
        self.secs_of_day % SECS_PER_MINUTE
    }

    /// Seconds since local midnight, `0..86_400`.
    #[must_use]
    pub fn secs_of_day(self) -> u32 {
        self.secs_of_day
    }

    /// The hour on a 12-hour clock, with `"AM"` or `"PM"`.
    ///
    /// One formula, not a ladder: the 12-hour clock *is* "the hour modulo 12,
    /// with 0 written as 12". Three of the four arms this replaces existed
    /// only to keep a subtraction from being reached with `hour < 12`.
    #[must_use]
    pub fn hour12(self) -> (u32, &'static str) {
        let hour = self.hour();
        let h12 = match hour % 12 {
            0 => 12,
            h => h,
        };
        (h12, if hour < 12 { "AM" } else { "PM" })
    }

    // ---- renderings -------------------------------------------------------

    /// `2026-08-18` — the ISO 8601 calendar date.
    ///
    /// ```
    /// use guitk::datetime::DateTime;
    /// assert_eq!(DateTime::utc(1_787_070_645).iso_date(), "2026-08-18");
    /// ```
    #[must_use]
    pub fn iso_date(self) -> String {
        let (y, m, d) = self.date.ymd();
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// `16:30` — a 24-hour clock reading, to the minute.
    #[must_use]
    pub fn clock(self) -> String {
        format!("{:02}:{:02}", self.hour(), self.minute())
    }

    /// `16:30:45` — a 24-hour clock reading, to the second.
    #[must_use]
    pub fn clock_secs(self) -> String {
        format!(
            "{:02}:{:02}:{:02}",
            self.hour(),
            self.minute(),
            self.second()
        )
    }

    /// `4:30 PM` — a 12-hour clock reading, to the minute.
    ///
    /// The hour is *not* zero-padded, because a 12-hour clock face does not
    /// pad it; `04:30 PM` is a shape no clock in the world shows.
    #[must_use]
    pub fn clock12(self) -> String {
        let (h, ampm) = self.hour12();
        format!("{h}:{:02} {ampm}", self.minute())
    }

    /// `4:30:45 PM` — a 12-hour clock reading, to the second.
    #[must_use]
    pub fn clock12_secs(self) -> String {
        let (h, ampm) = self.hour12();
        format!("{h}:{:02}:{:02} {ampm}", self.minute(), self.second())
    }

    /// `2026-08-18 16:30` — the list-column stamp.
    ///
    /// The default shape for "when did this happen" in a table, because it is
    /// the one five of the tree's programs had already converged on, it sorts
    /// lexicographically in the same order it sorts chronologically, and it
    /// is unambiguous in every locale.
    #[must_use]
    pub fn stamp(self) -> String {
        format!("{} {}", self.iso_date(), self.clock())
    }

    /// `2026-08-18 16:30:45` — the stamp, to the second.
    ///
    /// For records where two entries can share a minute and the user needs to
    /// tell them apart: backup runs, screen recordings, log lines.
    #[must_use]
    pub fn stamp_secs(self) -> String {
        format!("{} {}", self.iso_date(), self.clock_secs())
    }

    /// `Tuesday, August 18, 2026` — the long date, for a header or a panel.
    #[must_use]
    pub fn long_date(self) -> String {
        let (y, m, d) = self.date.ymd();
        format!("{}, {} {d}, {y}", self.date.weekday().name(), month_name(m))
    }

    /// `Aug 18, 2026` — the long date's short form, for a narrow column.
    #[must_use]
    pub fn medium_date(self) -> String {
        let (y, m, d) = self.date.ymd();
        format!("{} {d}, {y}", month_short_name(m))
    }
}

// ---- free functions, for the common one-line call -------------------------

/// `2026-08-18 16:30` in `tz`. See [`DateTime::stamp`].
#[must_use]
pub fn stamp(unix_secs: i64, tz: &Tz) -> String {
    DateTime::at(unix_secs, tz).stamp()
}

/// `2026-08-18 16:30:45` in `tz`. See [`DateTime::stamp_secs`].
#[must_use]
pub fn stamp_secs(unix_secs: i64, tz: &Tz) -> String {
    DateTime::at(unix_secs, tz).stamp_secs()
}

/// `2026-08-18` in `tz`. See [`DateTime::iso_date`].
#[must_use]
pub fn iso_date(unix_secs: i64, tz: &Tz) -> String {
    DateTime::at(unix_secs, tz).iso_date()
}

/// `16:30` in `tz`. See [`DateTime::clock`].
#[must_use]
pub fn clock(unix_secs: i64, tz: &Tz) -> String {
    DateTime::at(unix_secs, tz).clock()
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

    use super::{DateTime, clock, iso_date, stamp, stamp_secs};
    use crate::date::Date;
    use tzrules::Tz;

    /// 2026-08-18 16:30:45 UTC.
    const INSTANT: i64 = 1_787_070_645;

    /// New York: EST in winter, EDT in summer.
    fn new_york() -> Tz {
        Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("a valid POSIX TZ string")
    }

    #[test]
    fn the_parts_of_a_known_instant() {
        let dt = DateTime::utc(INSTANT);
        assert_eq!(dt.date().ymd(), (2026, 8, 18));
        assert_eq!(dt.hour(), 16);
        assert_eq!(dt.minute(), 30);
        assert_eq!(dt.second(), 45);
        assert_eq!(dt.secs_of_day(), 16 * 3600 + 30 * 60 + 45);
    }

    #[test]
    fn every_rendering_of_that_instant() {
        let dt = DateTime::utc(INSTANT);
        assert_eq!(dt.iso_date(), "2026-08-18");
        assert_eq!(dt.clock(), "16:30");
        assert_eq!(dt.clock_secs(), "16:30:45");
        assert_eq!(dt.clock12(), "4:30 PM");
        assert_eq!(dt.clock12_secs(), "4:30:45 PM");
        assert_eq!(dt.stamp(), "2026-08-18 16:30");
        assert_eq!(dt.stamp_secs(), "2026-08-18 16:30:45");
        assert_eq!(dt.long_date(), "Tuesday, August 18, 2026");
        assert_eq!(dt.medium_date(), "Aug 18, 2026");
    }

    #[test]
    fn the_epoch_itself() {
        let dt = DateTime::utc(0);
        assert_eq!(dt.stamp_secs(), "1970-01-01 00:00:00");
        assert_eq!(dt.long_date(), "Thursday, January 1, 1970");
        assert_eq!(dt.clock12(), "12:00 AM");
    }

    #[test]
    fn noon_and_midnight_on_the_twelve_hour_clock() {
        // The two readings the four-armed ladder this replaces got wrong most
        // often: hour 0 is 12 AM, hour 12 is 12 PM, and neither is "0".
        let midnight = DateTime::from_parts(Date::from_ymd(2026, 8, 18), 0, 0, 0);
        let noon = DateTime::from_parts(Date::from_ymd(2026, 8, 18), 12, 0, 0);
        assert_eq!(midnight.clock12(), "12:00 AM");
        assert_eq!(noon.clock12(), "12:00 PM");
        assert_eq!(midnight.hour12(), (12, "AM"));
        assert_eq!(noon.hour12(), (12, "PM"));
    }

    #[test]
    fn the_zone_moves_the_reading_and_may_move_the_day() {
        // 03:00 UTC on the 18th is still the 17th in New York.
        let utc_0300 = INSTANT - 13 * 3600 - 30 * 60 - 45;
        assert_eq!(DateTime::utc(utc_0300).stamp(), "2026-08-18 03:00");
        assert_eq!(
            DateTime::at(utc_0300, &new_york()).stamp(),
            "2026-08-17 23:00"
        );
    }

    #[test]
    fn a_zone_that_observes_dst_uses_the_offset_in_force_at_that_instant() {
        let ny = new_york();
        // August: EDT, -4h.
        assert_eq!(DateTime::at(INSTANT, &ny).clock(), "12:30");
        // The same wall-clock instant six months earlier is EST, -5h.
        // 2026-02-18 16:30:45 UTC.
        let winter = 1_771_432_245;
        assert_eq!(DateTime::at(winter, &ny).stamp(), "2026-02-18 11:30");
    }

    #[test]
    fn instants_before_the_epoch_land_on_the_day_that_contains_them() {
        // Truncating division would call this 1970-01-01 and then have to
        // render a negative hour; Euclidean division agrees with a calendar.
        assert_eq!(DateTime::utc(-1).stamp_secs(), "1969-12-31 23:59:59");
        assert_eq!(DateTime::utc(-86_400).stamp_secs(), "1969-12-31 00:00:00");
        assert_eq!(DateTime::utc(-86_401).stamp_secs(), "1969-12-30 23:59:59");
    }

    #[test]
    fn out_of_range_parts_clamp_rather_than_wrap() {
        let d = Date::from_ymd(2026, 8, 18);
        assert_eq!(DateTime::from_parts(d, 25, 99, 99).clock_secs(), "23:59:59");
        assert_eq!(DateTime::from_parts(d, 0, 0, 0).clock_secs(), "00:00:00");
    }

    #[test]
    fn a_timestamp_beyond_what_a_date_can_hold_saturates_rather_than_wrapping() {
        // A timestamp comes off a filesystem, so it is bounded by nothing
        // this tree controls. Saturating lands in the far future; wrapping
        // would land in the past, and a file dated before the epoch sorts to
        // the top of a list ordered by date.
        let far = DateTime::utc(i64::MAX);
        assert!(far.date().year() > 2026, "{}", far.iso_date());
    }

    #[test]
    fn the_free_functions_agree_with_the_methods() {
        let ny = new_york();
        let dt = DateTime::at(INSTANT, &ny);
        assert_eq!(stamp(INSTANT, &ny), dt.stamp());
        assert_eq!(stamp_secs(INSTANT, &ny), dt.stamp_secs());
        assert_eq!(iso_date(INSTANT, &ny), dt.iso_date());
        assert_eq!(clock(INSTANT, &ny), dt.clock());
    }

    #[test]
    fn a_stamp_sorts_the_same_way_as_the_instant_it_renders() {
        // The reason `stamp` is the default shape rather than a locale one:
        // a table sorted on the rendered string is in chronological order, so
        // a column that sorts by text does not have to be special-cased.
        let mut instants = [INSTANT, 0, 1_000_000_000, -86_400, 1_750_000_000];
        let mut stamps: Vec<String> = instants.iter().map(|&t| DateTime::utc(t).stamp()).collect();
        instants.sort_unstable();
        stamps.sort();
        let expected: Vec<String> = instants.iter().map(|&t| DateTime::utc(t).stamp()).collect();
        assert_eq!(stamps, expected);
    }

    #[test]
    fn the_year_is_not_days_over_365() {
        // The bug this module was written for. `apps/undelete` derived the
        // year as `days / 365` and the month as `remaining / 30`, which is
        // not an approximation but a drift of about five days per year — a
        // fortnight of error by 2026, on the one column a user reads to tell
        // two deleted copies of a file apart.
        for (instant, correct) in [
            (INSTANT, "2026-08-18"),
            (1_000_000_000, "2001-09-09"),
            (1_750_000_000, "2025-06-15"),
        ] {
            assert_eq!(DateTime::utc(instant).iso_date(), correct);

            let days = instant / 86_400;
            let years = days / 365;
            let rem = days - years * 365;
            let old = format!(
                "{:04}-{:02}-{:02}",
                1970 + years,
                (rem / 30 + 1).min(12),
                (rem % 30 + 1).min(31)
            );
            assert_ne!(old, correct, "the old arithmetic was not merely a rounding");
        }
    }
}
