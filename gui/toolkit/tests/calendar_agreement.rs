//! The tree has two shared calendars. This proves they are the same calendar.
//!
//! `tzrules` and `civildate` both convert between a Unix day number and a
//! civil `(year, month, day)`. Lane C's widgets reach the calendar through
//! `tzrules` (via [`guitk::date`]); `civildate` is the root crate lane B
//! extracted, and `kernel/src/timekeeping.rs`, `optionalfile`, `userspace/at`
//! and `userspace/hwclock` are on it. So a date rendered in a window and the
//! same date printed by `at` travel through different code.
//!
//! Two implementations of one idea is the defect this tree keeps finding, and
//! the usual fix is to delete one. That is not available here yet: the two
//! disagree *deliberately* on one function. `civildate::days_in_month` answers
//! 0 for a month outside 1..=12 and `userspace/at` has a test requiring the 0;
//! `guitk::date::days_in_month` clamps, because a 0 there was a live hazard —
//! `calendar.rs` walked `while days >= days_in_month(..)`, a loop whose
//! termination depended on a fact proved in another file. Both choices are
//! documented and both are right for their callers.
//!
//! What has no such fork is the arithmetic underneath: the leap rule and the
//! two civil conversions. Those must agree exactly or a date differs between
//! the desktop and the command line, and nothing in either crate would say so.
//! This test is what makes that checkable rather than assumed, and it is the
//! precondition for ever sharing the kernel between them.
//!
//! It lives in `guitk` because `guitk` is lane C's front door onto the
//! calendar and lane C may write here. If the crates are ever layered, this
//! test is what proves the layering did not change an answer.

/// Roughly ±1,100 years around the epoch, which covers every date either
/// crate's callers can produce and both century and 400-year leap boundaries
/// many times over.
const DAY_SPAN: i64 = 400_000;

#[test]
fn the_two_calendars_agree_on_the_leap_rule() {
    for year in -4000..=4000_i32 {
        assert_eq!(
            civildate::is_leap_year(year),
            tzrules::is_leap(i64::from(year)),
            "the two calendars disagree on whether {year} is a leap year"
        );
    }
}

#[test]
fn the_two_calendars_agree_turning_a_day_number_into_a_date() {
    for days in -DAY_SPAN..=DAY_SPAN {
        let (cy, cm, cd) = civildate::civil_from_days(days);
        let (ty, tm, td) = tzrules::civil_from_days(days);
        assert_eq!(
            (i64::from(cy), cm, cd),
            (ty, tm, td),
            "day {days} is {cy}-{cm:02}-{cd:02} to civildate and {ty}-{tm:02}-{td:02} to tzrules"
        );
    }
}

#[test]
fn the_two_calendars_agree_turning_a_date_into_a_day_number() {
    // Driven from `civil_from_days` rather than from a nested y/m/d loop, so
    // every input is a real date and the comparison is never about how the
    // two crates handle an impossible one -- that is the documented fork, and
    // this test is deliberately not about it.
    for days in -DAY_SPAN..=DAY_SPAN {
        let (y, m, d) = civildate::civil_from_days(days);
        assert_eq!(
            civildate::days_from_civil(y, m, d),
            tzrules::days_from_civil(i64::from(y), m, d),
            "{y}-{m:02}-{d:02} is a different day number to each crate"
        );
    }
}

#[test]
fn each_calendar_round_trips_against_itself_across_the_whole_span() {
    // Agreement is not correctness: two identical transcriptions of a wrong
    // algorithm agree perfectly. The round trip is the independent check, and
    // the day number is the one value neither crate can fudge.
    for days in -DAY_SPAN..=DAY_SPAN {
        let (y, m, d) = civildate::civil_from_days(days);
        assert_eq!(
            civildate::days_from_civil(y, m, d),
            days,
            "civildate lost {y}-{m:02}-{d:02} on the way back"
        );

        let (y, m, d) = tzrules::civil_from_days(days);
        assert_eq!(
            tzrules::days_from_civil(y, m, d),
            days,
            "tzrules lost {y}-{m:02}-{d:02} on the way back"
        );
    }
}

#[test]
fn the_dates_that_a_naive_transcription_gets_wrong() {
    // Named rather than swept, because these are the ones a wrong calendar
    // passes a casual test on and fails in the field. The tree has already
    // shipped one: `apps/backup` records that the file manager's sibling
    // transcription "was wrong for every date before 2000-03-01".
    //
    // The sweeps above prove the two crates agree with *each other*, which two
    // identical transcriptions of a wrong algorithm would also do. These
    // expectations are the outside check, so where they come from matters:
    // every one below was generated with Python's `datetime`, a third
    // implementation neither crate shares. That was not the original plan --
    // they were computed by hand, and two of the eight were wrong (day 11,017
    // is 1 March, not 29 February; day 19,784 is 2 March, not 29 February).
    // The crates were right both times. A hand-written expectation is another
    // transcription of the same arithmetic, made by the person least able to
    // notice they have made it twice.
    //
    // The exception is the shift origin, which predates year 1 and is outside
    // `datetime`'s range; it is the algorithm's own constant, and what pins it
    // here is the round trip above rather than an independent date.
    for (days, expect) in [
        (0_i64, (1970, 1, 1)),
        (-1, (1969, 12, 31)),
        (-719_468, (0, 3, 1)),   // the shift origin of Hinnant's algorithm
        (11_016, (2000, 2, 29)), // a century year that IS a leap year
        (-25_567, (1900, 1, 1)), // just after 1900, which is NOT
        (19_782, (2024, 2, 29)),
        (365, (1971, 1, 1)),
        (-36_524, (1870, 1, 1)),
    ] {
        let (y, m, d) = civildate::civil_from_days(days);
        assert_eq!((i64::from(y), m, d), expect, "civildate on day {days}");
        let (y, m, d) = tzrules::civil_from_days(days);
        assert_eq!((y, m, d), expect, "tzrules on day {days}");
    }
}
