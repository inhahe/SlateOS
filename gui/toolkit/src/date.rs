//! Civil dates: one answer to "what day is this?" for the whole toolkit.
//!
//! # Why this exists
//!
//! Eight places in the GUI tree had written their own calendar: the desktop's
//! calendar panel, the file dialog, and the calendar, contacts, habits,
//! reminders, RSS-reader and system-tray applications. Each had its own
//! `is_leap_year` and `days_in_month`, spelled over a different integer type
//! (`i32` in three of them, `u16`, `u64`, `u8` in the others), and four had
//! their own day-of-week. They agreed by luck rather than by construction, and
//! every one of them carried the same class of latent fault: a bound proved in
//! one statement and relied on in another —
//!
//! ```text
//! if self.view_month == 1 { (year - 1, 12) } else { (year, self.view_month - 1) }
//! ```
//!
//! — where the subtraction is only safe because of the test above it, so an
//! edit that moves either half breaks the other silently.
//!
//! A `Date` fixes that by construction. It is **a day number, not a triple**:
//! internally one `i32` counting days from 1970-01-01, so stepping a day, a
//! week or a month is integer addition that cannot produce 31 February or
//! month 13, ordering is integer ordering, and the difference of two dates is
//! a subtraction rather than a nested loop over years and months. The
//! `(year, month, day)` view is *derived* on demand, and derived through
//! [`tzrules`] — the same era arithmetic the libc's `localtime`, the shell's
//! `%(…)T` and the taskbar clock render local time through. A ninth private
//! calendar in the toolkit would be a ninth thing for the user to catch
//! disagreeing with `date`.
//!
//! # What it deliberately does not do
//!
//! `Date` is a *civil* date — a square on a wall calendar. It carries no time
//! of day and no zone. Converting an instant to the date it fell on is the
//! caller's business, because only the caller knows which zone the user meant;
//! [`Date::from_unix_utc`] does the UTC case, and a local one is
//! `Date::from_unix_utc(t + zone.lookup(t).gmtoff)`.

use core::cmp::Ordering;

/// Seconds in a day. Unix time has no leap seconds, so this is exact.
const SECS_PER_DAY: i64 = 86_400;

/// 1970-01-01 was a Thursday; this is its offset from Sunday.
const EPOCH_WEEKDAY: i32 = 4;

/// A day of the week.
///
/// `Sunday` is zero because that is what the underlying arithmetic produces,
/// not because weeks start then — which week day a *calendar* starts on is a
/// user preference and is passed to [`Date::month_grid`] separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    Sunday = 0,
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
}

impl Weekday {
    /// The weekday `index` days after Sunday, wrapping.
    ///
    /// Total: every `i32` names a weekday, because `rem_euclid` is defined for
    /// negatives too. That is what lets callers write `weekday.index() - 1`
    /// without first proving the result stays in range.
    #[must_use]
    pub fn from_index(index: i32) -> Self {
        match index.rem_euclid(7) {
            1 => Self::Monday,
            2 => Self::Tuesday,
            3 => Self::Wednesday,
            4 => Self::Thursday,
            5 => Self::Friday,
            6 => Self::Saturday,
            // `rem_euclid(7)` is 0..=6, so this arm is 0 and nothing else.
            _ => Self::Sunday,
        }
    }

    /// Days from Sunday to this weekday, 0..=6.
    #[must_use]
    pub fn index(self) -> i32 {
        self as i32
    }

    /// Days from `start` forward to this weekday, 0..=6.
    ///
    /// This is the offset a month grid needs: how many trailing days of the
    /// previous month to draw before the 1st, given where the user's week
    /// begins. Written as a subtraction plus `rem_euclid` rather than an
    /// `if first == Sunday { 6 } else { first - 1 }`, which is the same
    /// value with the wrap-around stated in a different statement from the
    /// subtraction that needs it.
    #[must_use]
    pub fn days_since(self, start: Weekday) -> u32 {
        u32::try_from(self.index().saturating_sub(start.index()).rem_euclid(7)).unwrap_or(0)
    }

    /// ISO 8601 weekday number: Monday is 1, Sunday is 7.
    #[must_use]
    pub fn iso_index(self) -> u32 {
        match self {
            Self::Sunday => 7,
            other => u32::try_from(other.index()).unwrap_or(7),
        }
    }

    /// The English name, e.g. `"Wednesday"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sunday => "Sunday",
            Self::Monday => "Monday",
            Self::Tuesday => "Tuesday",
            Self::Wednesday => "Wednesday",
            Self::Thursday => "Thursday",
            Self::Friday => "Friday",
            Self::Saturday => "Saturday",
        }
    }

    /// The three-letter English abbreviation, e.g. `"Wed"`.
    ///
    /// Sliced from [`Weekday::name`] would be one byte-index too many for a
    /// name that is ASCII only by convention; the table is spelled out so the
    /// abbreviation cannot be a panic waiting on a translation.
    #[must_use]
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Sunday => "Sun",
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
        }
    }
}

/// A civil date — a square on a wall calendar.
///
/// Stored as days since 1970-01-01, so every date that can be named is
/// representable and every step between them is an integer addition. See the
/// module docs for why this is a day count rather than a `(y, m, d)` triple.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Date {
    /// Days since 1970-01-01. `i32` spans about ±5.8 million years, which is
    /// wider than the Gregorian calendar is meaningful over.
    days: i32,
}

impl Date {
    /// 1970-01-01, the Unix epoch.
    pub const EPOCH: Date = Date { days: 0 };

    /// The date `days` after 1970-01-01. Negative values are before it.
    #[must_use]
    pub fn from_days_since_epoch(days: i32) -> Self {
        Self { days }
    }

    /// The date named by `year`, `month` and `day`.
    ///
    /// **Out-of-range parts are clamped, not rejected**, so this is total:
    /// month 0 and month 13 become January and December, and a day past the
    /// end of the month becomes its last day — 2025-02-31 is 2025-02-28. That
    /// is the behaviour a date picker wants when the user changes February to
    /// January with 31 in the day box, and it means callers never hold an
    /// `Option<Date>` they have to prove is `Some` before drawing a grid.
    #[must_use]
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Self {
        let month = month.clamp(1, 12);
        let day = day.clamp(1, days_in_month(year, month));
        let days = tzrules::days_from_civil(i64::from(year), month, day);
        Self {
            days: i32::try_from(days).unwrap_or(i32::MAX),
        }
    }

    /// The UTC date on which the Unix instant `secs` fell.
    ///
    /// For a local date, offset the instant first:
    /// `Date::from_unix_utc(t.saturating_add(zone.lookup(t).gmtoff.into()))`.
    #[must_use]
    pub fn from_unix_utc(secs: i64) -> Self {
        // `div_euclid`, not `/`: instants before 1970 must round *down* to the
        // day that contains them, and truncating division rounds toward zero,
        // which would put 1969-12-31 23:00 on 1970-01-01.
        Self {
            days: i32::try_from(secs.div_euclid(SECS_PER_DAY)).unwrap_or(i32::MIN),
        }
    }

    /// Days since 1970-01-01.
    #[must_use]
    pub fn days_since_epoch(self) -> i32 {
        self.days
    }

    /// Midnight at the start of this date, as a Unix instant in UTC.
    #[must_use]
    pub fn unix_secs_utc(self) -> i64 {
        i64::from(self.days).saturating_mul(SECS_PER_DAY)
    }

    /// The `(year, month, day)` this date names, with `month` and `day`
    /// 1-based.
    /// This is the exact inverse of [`tzrules::days_from_civil`], which
    /// [`Date::from_ymd`] builds with, so the two directions cannot disagree.
    /// It used to walk the months subtracting `days_in_month` from a
    /// `year_of_day` result — which was the same projection computed twice,
    /// since `year_of_day` derives the year by computing the month and day and
    /// discarding them. See `requests/b-c-tzrules-now-exports-civil-from-days.md`.
    #[must_use]
    pub fn ymd(self) -> (i32, u32, u32) {
        let (year, month, day) = tzrules::civil_from_days(i64::from(self.days));
        (i32::try_from(year).unwrap_or(i32::MAX), month, day)
    }

    /// The Gregorian year.
    #[must_use]
    pub fn year(self) -> i32 {
        self.ymd().0
    }

    /// The month, 1..=12.
    #[must_use]
    pub fn month(self) -> u32 {
        self.ymd().1
    }

    /// The day of the month, 1..=31.
    #[must_use]
    pub fn day(self) -> u32 {
        self.ymd().2
    }

    /// The day of the week.
    #[must_use]
    pub fn weekday(self) -> Weekday {
        Weekday::from_index(self.days.wrapping_add(EPOCH_WEEKDAY))
    }

    /// The date `n` days later; `n` may be negative.
    #[must_use]
    pub fn add_days(self, n: i32) -> Self {
        Self {
            days: self.days.saturating_add(n),
        }
    }

    /// The number of days from `self` to `other`, negative if `other` is
    /// earlier.
    #[must_use]
    pub fn days_until(self, other: Date) -> i32 {
        other.days.saturating_sub(self.days)
    }

    /// The 1st of this date's month.
    #[must_use]
    pub fn start_of_month(self) -> Self {
        let (y, m, _) = self.ymd();
        Self::from_ymd(y, m, 1)
    }

    /// The date `n` months later, `n` possibly negative, **with the day
    /// clamped into the target month**.
    ///
    /// 31 January plus one month is 28 February (29 in a leap year), which is
    /// the only choice that keeps "next month" a total function. Note that it
    /// is therefore not reversible: stepping back again gives 28 January.
    /// Month-grid callers should step [`start_of_month`](Self::start_of_month)
    /// so the clamp never arises.
    #[must_use]
    pub fn add_months(self, n: i32) -> Self {
        let (y, m, d) = self.ymd();
        // Months since year 0, so the year rollover is a division rather than
        // an `if month == 12` sitting above the increment it authorises.
        let total = i64::from(y)
            .saturating_mul(12)
            .saturating_add(i64::from(m).saturating_sub(1))
            .saturating_add(i64::from(n));
        let year = i32::try_from(total.div_euclid(12)).unwrap_or(i32::MAX);
        let month = u32::try_from(total.rem_euclid(12))
            .unwrap_or(0)
            .saturating_add(1);
        Self::from_ymd(year, month, d)
    }

    /// The date `n` years later, with 29 February clamped to the 28th in a
    /// common year.
    #[must_use]
    pub fn add_years(self, n: i32) -> Self {
        self.add_months(n.saturating_mul(12))
    }

    /// The number of days in this date's month.
    #[must_use]
    pub fn days_in_month(self) -> u32 {
        let (y, m, _) = self.ymd();
        days_in_month(y, m)
    }

    /// Whether this date's year is a leap year.
    #[must_use]
    pub fn is_leap_year(self) -> bool {
        is_leap_year(self.year())
    }

    /// The day of the year, 1..=366.
    #[must_use]
    pub fn day_of_year(self) -> u32 {
        let (y, _, _) = self.ymd();
        let jan1 = Self::from_ymd(y, 1, 1);
        u32::try_from(jan1.days_until(self))
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// The ISO 8601 week: `(iso_year, week)`, week 1 being the one containing
    /// the year's first Thursday.
    ///
    /// The ISO year is not always the calendar year — 2027-01-01 is a Friday
    /// and so belongs to week 53 of 2026. The old implementation handled that
    /// with two special cases either side of a main path, each recomputing a
    /// day-of-week from scratch; here it falls out of asking the *Thursday of
    /// this week* what year it is in, which is the definition.
    #[must_use]
    pub fn iso_week(self) -> (i32, u32) {
        // ISO weeks run Monday..Sunday, so `days_since(Monday)` is 0 for
        // Monday and 6 for Sunday; Thursday is three days after Monday.
        let from_monday = i32::try_from(self.weekday().days_since(Weekday::Monday)).unwrap_or(0);
        let thursday = self.add_days(3_i32.saturating_sub(from_monday));
        let iso_year = thursday.year();
        // The Thursday's ordinal within its own year fixes the week outright:
        // ordinals 1..=7 are week 1, 8..=14 week 2, and so on.
        let ordinal = thursday.day_of_year();
        let week = ordinal
            .saturating_sub(1)
            .checked_div(7)
            .unwrap_or(0)
            .saturating_add(1);
        (iso_year, week)
    }

    /// The six-week grid a month is drawn in: 42 consecutive dates starting on
    /// the `week_start` on or before the 1st.
    ///
    /// Always 42, never 35 — a fixed grid means the panel does not change
    /// height as the user pages through the year. Cells outside the month are
    /// ordinary dates from the neighbouring months, so a caller decides they
    /// are "other month" with `d.month() != first.month()` rather than being
    /// handed a flag it has to keep in step. That replaces three separate
    /// loops (lead-in, the month, spill-over) and the `prev_days - offset + 1`
    /// arithmetic that made the lead-in only correct while `offset` was
    /// provably no larger than the previous month.
    pub fn month_grid(self, week_start: Weekday) -> impl Iterator<Item = Date> {
        let first = self.start_of_month();
        let lead = i32::try_from(first.weekday().days_since(week_start)).unwrap_or(0);
        // `saturating_neg`, not `-lead`: unary minus traps on `i32::MIN`, and
        // "but `lead` is 0..=6" is a proof living two functions away from the
        // operation that depends on it — the exact shape this module exists to
        // remove.
        let start = first.add_days(lead.saturating_neg());
        (0..42_i32).map(move |i| start.add_days(i))
    }

    /// Every date in this date's month, 1st to last.
    pub fn month_days(self) -> impl Iterator<Item = Date> {
        let first = self.start_of_month();
        let len = i32::try_from(first.days_in_month()).unwrap_or(28);
        (0..len).map(move |i| first.add_days(i))
    }
}

impl core::fmt::Display for Date {
    /// ISO 8601: `2026-08-17`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// Whether `year` is a Gregorian leap year.
#[must_use]
pub fn is_leap_year(year: i32) -> bool {
    tzrules::is_leap(i64::from(year))
}

/// The number of days in `month` (1..=12) of `year`.
///
/// An out-of-range month is clamped rather than answered with 0. A zero here
/// was a live hazard: `calendar.rs` walked `while days >= days_in_month(..)`,
/// a loop whose termination depended on the month never leaving 1..=12 — a
/// fact proved somewhere else entirely.
#[must_use]
pub fn days_in_month(year: i32, month: u32) -> u32 {
    tzrules::days_in_month(month.clamp(1, 12), i64::from(year))
}

/// The English name of `month` (1..=12), e.g. `"August"`.
#[must_use]
pub fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    NAMES
        .get(month.clamp(1, 12).saturating_sub(1) as usize)
        .copied()
        .unwrap_or("January")
}

/// The three-letter English abbreviation of `month` (1..=12), e.g. `"Aug"`.
#[must_use]
pub fn month_short_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    NAMES
        .get(month.clamp(1, 12).saturating_sub(1) as usize)
        .copied()
        .unwrap_or("Jan")
}

/// Order by day number, which is the calendar order.
impl Date {
    /// Whether `self` falls strictly between `start` and `end`.
    #[must_use]
    pub fn is_between(self, start: Date, end: Date) -> bool {
        matches!(self.cmp(&start), Ordering::Greater) && matches!(self.cmp(&end), Ordering::Less)
    }
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
    fn the_epoch_is_a_thursday_and_knows_its_own_date() {
        assert_eq!(Date::EPOCH.ymd(), (1970, 1, 1));
        assert_eq!(Date::EPOCH.weekday(), Weekday::Thursday);
        assert_eq!(Date::EPOCH.to_string(), "1970-01-01");
    }

    #[test]
    fn ymd_round_trips_across_a_century_of_days() {
        // Every day from 1950 to 2050: decompose and rebuild. A single-day
        // error anywhere in the era arithmetic shows up here.
        let start = Date::from_ymd(1950, 1, 1);
        let end = Date::from_ymd(2050, 1, 1);
        let mut d = start;
        while d < end {
            let (y, m, day) = d.ymd();
            assert!((1..=12).contains(&m), "month {m} out of range at {d}");
            assert!((1..=31).contains(&day), "day {day} out of range at {d}");
            assert_eq!(Date::from_ymd(y, m, day), d, "round trip failed at {d}");
            d = d.add_days(1);
        }
    }

    #[test]
    fn the_weekday_advances_one_step_per_day_without_a_break() {
        // Any off-by-one in the epoch weekday, or a `%` where `rem_euclid`
        // belongs, breaks the chain — including on the pre-1970 side, which
        // is where truncating arithmetic goes wrong.
        let mut d = Date::from_ymd(1965, 3, 3);
        let end = Date::from_ymd(1975, 3, 3);
        while d < end {
            let next = d.add_days(1);
            let expected = Weekday::from_index(d.weekday().index() + 1);
            assert_eq!(next.weekday(), expected, "weekday broke at {d}");
            d = next;
        }
    }

    #[test]
    fn known_weekdays_match_the_calendar() {
        // Dates whose weekday is a matter of record rather than of derivation.
        assert_eq!(Date::from_ymd(2000, 1, 1).weekday(), Weekday::Saturday);
        assert_eq!(Date::from_ymd(2026, 8, 17).weekday(), Weekday::Monday);
        assert_eq!(Date::from_ymd(1969, 7, 20).weekday(), Weekday::Sunday);
        assert_eq!(Date::from_ymd(1900, 1, 1).weekday(), Weekday::Monday);
        assert_eq!(Date::from_ymd(2100, 1, 1).weekday(), Weekday::Friday);
    }

    #[test]
    fn the_century_rule_is_applied_not_just_the_four_year_one() {
        assert!(is_leap_year(2000), "2000 is divisible by 400");
        assert!(!is_leap_year(1900), "1900 is divisible by 100 but not 400");
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2025));
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(Date::from_ymd(2024, 2, 1).days_in_month(), 29);
    }

    #[test]
    fn an_out_of_range_month_or_day_is_clamped_rather_than_wrapped() {
        // The point of the clamp is that no caller has to hold an Option.
        assert_eq!(Date::from_ymd(2025, 2, 31).ymd(), (2025, 2, 28));
        assert_eq!(Date::from_ymd(2024, 2, 31).ymd(), (2024, 2, 29));
        assert_eq!(Date::from_ymd(2025, 0, 1).ymd(), (2025, 1, 1));
        assert_eq!(Date::from_ymd(2025, 13, 1).ymd(), (2025, 12, 1));
        assert_eq!(Date::from_ymd(2025, 4, 0).ymd(), (2025, 4, 1));
        // And an out-of-range month never reports zero days, which is what
        // made the old `while days >= days_in_month(..)` loop unbounded.
        assert!(days_in_month(2025, 0) > 0);
        assert!(days_in_month(2025, 99) > 0);
    }

    #[test]
    fn stepping_a_month_lands_on_the_same_day_or_the_last_one() {
        assert_eq!(
            Date::from_ymd(2025, 1, 15).add_months(1).ymd(),
            (2025, 2, 15)
        );
        assert_eq!(
            Date::from_ymd(2025, 1, 31).add_months(1).ymd(),
            (2025, 2, 28)
        );
        assert_eq!(
            Date::from_ymd(2024, 1, 31).add_months(1).ymd(),
            (2024, 2, 29)
        );
        // Across both year boundaries, which is where the old `if month == 12`
        // and `if month == 1` guards lived.
        assert_eq!(
            Date::from_ymd(2025, 12, 10).add_months(1).ymd(),
            (2026, 1, 10)
        );
        assert_eq!(
            Date::from_ymd(2025, 1, 10).add_months(-1).ymd(),
            (2024, 12, 10)
        );
        assert_eq!(
            Date::from_ymd(2025, 6, 10).add_months(-18).ymd(),
            (2023, 12, 10)
        );
        assert_eq!(
            Date::from_ymd(2025, 6, 10).add_months(30).ymd(),
            (2027, 12, 10)
        );
    }

    #[test]
    fn stepping_forward_and_back_a_month_returns_to_the_start_when_the_day_fits() {
        for month in 1..=12u32 {
            let d = Date::from_ymd(2025, month, 15);
            assert_eq!(d.add_months(1).add_months(-1), d, "month {month}");
            assert_eq!(d.add_months(-1).add_months(1), d, "month {month}");
        }
    }

    #[test]
    fn leap_day_plus_a_year_is_the_28th() {
        assert_eq!(
            Date::from_ymd(2024, 2, 29).add_years(1).ymd(),
            (2025, 2, 28)
        );
        assert_eq!(
            Date::from_ymd(2024, 2, 29).add_years(4).ymd(),
            (2028, 2, 29)
        );
    }

    #[test]
    fn a_month_grid_is_always_six_weeks_and_starts_on_the_users_first_day() {
        for start in [Weekday::Sunday, Weekday::Monday] {
            for month in 1..=12u32 {
                let first = Date::from_ymd(2025, month, 1);
                let cells: Vec<Date> = first.month_grid(start).collect();
                assert_eq!(cells.len(), 42, "month {month}");
                assert_eq!(cells[0].weekday(), start, "grid must open on {start:?}");
                // Consecutive, with no gap where the month changes.
                for pair in cells.windows(2) {
                    assert_eq!(pair[0].add_days(1), pair[1]);
                }
                // The 1st is in the first row, and the whole month is covered.
                let in_month: Vec<Date> = cells
                    .iter()
                    .copied()
                    .filter(|d| d.month() == month)
                    .collect();
                assert_eq!(
                    in_month.len() as u32,
                    first.days_in_month(),
                    "month {month}"
                );
                assert_eq!(in_month[0], first);
                let lead = cells.iter().position(|d| *d == first).unwrap();
                assert!(lead < 7, "the 1st must be in the first row, was at {lead}");
            }
        }
    }

    #[test]
    fn a_month_that_starts_on_the_week_start_still_shows_a_full_lead_row() {
        // June 2025 begins on a Sunday. With a Sunday week start the lead-in
        // is empty, which is the case a `prev_days - offset` expression gets
        // wrong first.
        let june = Date::from_ymd(2025, 6, 1);
        assert_eq!(june.weekday(), Weekday::Sunday);
        let cells: Vec<Date> = june.month_grid(Weekday::Sunday).collect();
        assert_eq!(cells[0], june, "no lead-in when the 1st is the week start");
        let cells: Vec<Date> = june.month_grid(Weekday::Monday).collect();
        assert_eq!(
            cells[0].ymd(),
            (2025, 5, 26),
            "a Monday grid leads in six days"
        );
    }

    #[test]
    fn month_days_yields_exactly_the_month() {
        let days: Vec<Date> = Date::from_ymd(2024, 2, 14).month_days().collect();
        assert_eq!(days.len(), 29);
        assert_eq!(days[0].ymd(), (2024, 2, 1));
        assert_eq!(days[28].ymd(), (2024, 2, 29));
    }

    #[test]
    fn iso_weeks_match_the_standards_own_worked_examples() {
        // From ISO 8601's own edge cases: a year can have 52 or 53 weeks, and
        // the first days of January often belong to the previous ISO year.
        assert_eq!(Date::from_ymd(2026, 12, 31).iso_week(), (2026, 53));
        assert_eq!(Date::from_ymd(2027, 1, 1).iso_week(), (2026, 53));
        assert_eq!(Date::from_ymd(2027, 1, 4).iso_week(), (2027, 1));
        assert_eq!(Date::from_ymd(2025, 1, 1).iso_week(), (2025, 1));
        assert_eq!(Date::from_ymd(2024, 12, 30).iso_week(), (2025, 1));
        assert_eq!(Date::from_ymd(2021, 1, 1).iso_week(), (2020, 53));
        assert_eq!(Date::from_ymd(2020, 12, 31).iso_week(), (2020, 53));
        assert_eq!(Date::from_ymd(1977, 1, 1).iso_week(), (1976, 53));
        assert_eq!(Date::from_ymd(1977, 1, 2).iso_week(), (1976, 53));
        assert_eq!(Date::from_ymd(1977, 1, 3).iso_week(), (1977, 1));
    }

    #[test]
    fn every_iso_week_is_1_to_53_and_constant_across_its_own_week() {
        let mut d = Date::from_ymd(1990, 1, 1);
        let end = Date::from_ymd(2060, 1, 1);
        while d < end {
            let (_, week) = d.iso_week();
            assert!((1..=53).contains(&week), "week {week} at {d}");
            // Every day Monday..Sunday reports the same (year, week).
            let monday = d.add_days(-(d.weekday().days_since(Weekday::Monday) as i32));
            assert_eq!(
                d.iso_week(),
                monday.iso_week(),
                "week changed mid-week at {d}"
            );
            d = d.add_days(1);
        }
    }

    #[test]
    fn the_day_of_year_counts_from_one_and_ends_at_the_year_length() {
        assert_eq!(Date::from_ymd(2025, 1, 1).day_of_year(), 1);
        assert_eq!(Date::from_ymd(2025, 12, 31).day_of_year(), 365);
        assert_eq!(Date::from_ymd(2024, 12, 31).day_of_year(), 366);
        assert_eq!(
            Date::from_ymd(2024, 3, 1).day_of_year(),
            61,
            "after the leap day"
        );
        assert_eq!(Date::from_ymd(2025, 3, 1).day_of_year(), 60);
    }

    #[test]
    fn unix_instants_map_to_the_day_that_contains_them_on_both_sides_of_the_epoch() {
        assert_eq!(Date::from_unix_utc(0).ymd(), (1970, 1, 1));
        assert_eq!(
            Date::from_unix_utc(86_399).ymd(),
            (1970, 1, 1),
            "23:59:59 is still day one"
        );
        assert_eq!(Date::from_unix_utc(86_400).ymd(), (1970, 1, 2));
        // Truncating division would round this toward zero and report
        // 1970-01-01 for an instant in 1969.
        assert_eq!(Date::from_unix_utc(-1).ymd(), (1969, 12, 31));
        assert_eq!(Date::from_unix_utc(-86_400).ymd(), (1969, 12, 31));
        assert_eq!(Date::from_unix_utc(-86_401).ymd(), (1969, 12, 30));
        assert_eq!(Date::from_ymd(2026, 8, 17).unix_secs_utc(), 1_786_924_800);
        assert_eq!(Date::from_unix_utc(1_786_924_800).ymd(), (2026, 8, 17));
    }

    #[test]
    fn dates_order_and_subtract_as_days() {
        let a = Date::from_ymd(2025, 1, 1);
        let b = Date::from_ymd(2025, 12, 31);
        assert!(a < b);
        assert_eq!(a.days_until(b), 364);
        assert_eq!(b.days_until(a), -364);
        assert_eq!(a.days_until(a), 0);
        assert!(Date::from_ymd(2025, 6, 1).is_between(a, b));
        assert!(!a.is_between(a, b), "the ends are not between");
    }

    #[test]
    fn weekday_offsets_wrap_in_both_directions() {
        assert_eq!(Weekday::Sunday.days_since(Weekday::Monday), 6);
        assert_eq!(Weekday::Monday.days_since(Weekday::Sunday), 1);
        assert_eq!(Weekday::Monday.days_since(Weekday::Monday), 0);
        assert_eq!(Weekday::from_index(-1), Weekday::Saturday);
        assert_eq!(Weekday::from_index(7), Weekday::Sunday);
        assert_eq!(Weekday::from_index(-7), Weekday::Sunday);
        assert_eq!(Weekday::Sunday.iso_index(), 7, "ISO puts Sunday last");
        assert_eq!(Weekday::Monday.iso_index(), 1);
    }

    #[test]
    fn month_and_weekday_names_are_defined_for_every_input() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(12), "December");
        assert_eq!(month_short_name(8), "Aug");
        // Out of range, rather than indexing past the end of the table.
        assert_eq!(month_name(0), "January");
        assert_eq!(month_name(13), "December");
        assert_eq!(month_short_name(99), "Dec");
        for m in 1..=12u32 {
            assert!(month_name(m).starts_with(month_short_name(m)));
        }
        for i in 0..7 {
            let d = Weekday::from_index(i);
            assert!(d.name().starts_with(d.short_name()));
        }
    }

    #[test]
    fn extreme_day_numbers_do_not_panic() {
        // `Date` is total by construction; nothing a caller can build should
        // be able to trap. These are absurd dates, but they must not crash a
        // panel that is only drawing them.
        for days in [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX] {
            let d = Date::from_days_since_epoch(days);
            let _ = d.ymd();
            let _ = d.weekday();
            let _ = d.iso_week();
            let _ = d.days_in_month();
            let _ = d.add_days(1);
            let _ = d.add_months(1);
            let _ = d.to_string();
            assert_eq!(d.month_grid(Weekday::Monday).count(), 42);
        }
    }
}
