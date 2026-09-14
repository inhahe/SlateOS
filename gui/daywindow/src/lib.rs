//! A recurring window of the day, which may wrap past midnight.
//!
//! Three shell features schedule themselves this way — do-not-disturb quiet
//! hours, night light, and focus assist — and each had grown its own copy:
//! four public `u8` fields, and at the point of use `hour * 60 + minute` on
//! each end followed by `if start <= end { … } else { … }`.
//!
//! Every copy had the same hole, and one of them shipped it. The fields were
//! public and validated nowhere, so a start of `25:00` became 1500 minutes —
//! past 1439, the largest time of day there is — which made the window compare
//! as overnight and then never open. The feature simply stopped happening,
//! with nothing reporting that it had. `gui/notifications` carried exactly
//! that bug; see `TD-GUI-CRATES-OPT-OUT-OF-THE-WORKSPACE-LINTS` in
//! `known-issues.md`.
//!
//! Written once, the bad state is unrepresentable. [`TimeOfDay`] cannot hold a
//! value that is not a time, so there is no arithmetic left at the call site
//! to get wrong, and no `if` to place a few statements away from the code it
//! justifies.

/// Minutes in a day. The first value that is *not* a time of day.
pub const MINUTES_PER_DAY: u16 = 24 * 60;

/// A wall-clock time of day, as minutes from midnight.
///
/// Constructible only through a check, so `hour()` is always `< 24` and
/// `minute()` always `< 60`. There is no `Ord`-free ambiguity here — earlier
/// in the day is smaller — so it derives one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfDay {
    /// Minutes from midnight, always `< MINUTES_PER_DAY`.
    minutes: u16,
}

impl TimeOfDay {
    /// Midnight — the earliest time of day.
    pub const MIDNIGHT: Self = Self { minutes: 0 };

    /// A time of day, or `None` if `hour:minute` does not name one.
    ///
    /// The multiplication cannot overflow once the hour is known to be under
    /// 24 — the largest result is `23 * 60 + 59 = 1439` — but it is written
    /// with `checked_*` anyway, so the bound is enforced by the code rather
    /// than by this sentence.
    #[must_use]
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        if hour >= 24 || minute >= 60 {
            return None;
        }
        u16::from(hour)
            .checked_mul(60)?
            .checked_add(u16::from(minute))
            .filter(|m| *m < MINUTES_PER_DAY)
            .map(|minutes| Self { minutes })
    }

    /// A time of day from minutes past midnight, or `None` at or past a day.
    #[must_use]
    pub const fn from_minutes(minutes: u16) -> Option<Self> {
        if minutes < MINUTES_PER_DAY {
            Some(Self { minutes })
        } else {
            None
        }
    }

    /// Minutes from midnight, `0..MINUTES_PER_DAY`.
    #[must_use]
    pub const fn minutes(self) -> u16 {
        self.minutes
    }

    /// The hour, `0..24`.
    #[must_use]
    pub fn hour(self) -> u8 {
        // `< 24` by construction, so the conversion cannot fail — written as a
        // conversion rather than a cast so that stays true if the field ever
        // widens, instead of silently truncating.
        u8::try_from(self.minutes / 60).unwrap_or(0)
    }

    /// The minute within the hour, `0..60`.
    #[must_use]
    pub fn minute(self) -> u8 {
        u8::try_from(self.minutes % 60).unwrap_or(0)
    }
}

/// A window of the day that recurs daily and may wrap past midnight.
///
/// The window is half-open — it contains its start and not its end — which is
/// what makes `start == end` an *empty* window rather than an all-day one.
/// That is a degenerate schedule, not an invalid one: a user who sets both
/// ends of quiet hours to 22:00 has asked for no quiet hours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DailyWindow {
    start: TimeOfDay,
    end: TimeOfDay,
}

impl DailyWindow {
    /// A window between two times of day.
    #[must_use]
    pub const fn new(start: TimeOfDay, end: TimeOfDay) -> Self {
        Self { start, end }
    }

    /// A window from wall-clock hours and minutes, or `None` if either end is
    /// not a real time of day.
    #[must_use]
    pub fn from_hm(start_hour: u8, start_minute: u8, end_hour: u8, end_minute: u8) -> Option<Self> {
        Some(Self {
            start: TimeOfDay::new(start_hour, start_minute)?,
            end: TimeOfDay::new(end_hour, end_minute)?,
        })
    }

    /// When the window opens.
    #[must_use]
    pub const fn start(self) -> TimeOfDay {
        self.start
    }

    /// When the window closes. Not itself inside the window.
    #[must_use]
    pub const fn end(self) -> TimeOfDay {
        self.end
    }

    /// Whether the window runs through midnight into the next day.
    #[must_use]
    pub const fn wraps_midnight(self) -> bool {
        self.start.minutes > self.end.minutes
    }

    /// Whether `at` falls inside the window.
    #[must_use]
    pub const fn contains(self, at: TimeOfDay) -> bool {
        let (start, end, now) = (self.start.minutes, self.end.minutes, at.minutes);
        if start <= end {
            // Same-day window, e.g. 08:00–17:00.
            now >= start && now < end
        } else {
            // Overnight window, e.g. 22:00–07:00.
            now >= start || now < end
        }
    }

    /// Whether a wall-clock `hour:minute` falls inside the window.
    ///
    /// A time that does not exist is not inside any window — the answer a
    /// caller holding a clock reading it did not validate wants, and the one
    /// the hand-rolled copies got wrong by computing with it anyway.
    #[must_use]
    pub fn contains_hm(self, hour: u8, minute: u8) -> bool {
        TimeOfDay::new(hour, minute).is_some_and(|at| self.contains(at))
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
    fn a_time_of_day_cannot_be_built_out_of_something_that_is_not_one() {
        assert_eq!(TimeOfDay::new(0, 0).map(TimeOfDay::minutes), Some(0));
        assert_eq!(TimeOfDay::new(23, 59).map(TimeOfDay::minutes), Some(1439));
        for (h, m) in [(24, 0), (25, 0), (0, 60), (23, 60), (255, 255), (12, 99)] {
            assert!(TimeOfDay::new(h, m).is_none(), "{h}:{m} built a time");
        }
        assert!(TimeOfDay::from_minutes(MINUTES_PER_DAY).is_none());
        assert_eq!(
            TimeOfDay::from_minutes(MINUTES_PER_DAY - 1).map(TimeOfDay::minutes),
            Some(1439),
        );
    }

    #[test]
    fn every_time_of_day_reads_back_as_the_clock_that_made_it() {
        for hour in 0..24u8 {
            for minute in 0..60u8 {
                let t = TimeOfDay::new(hour, minute).unwrap();
                assert_eq!((t.hour(), t.minute()), (hour, minute));
                assert_eq!(TimeOfDay::from_minutes(t.minutes()), Some(t));
            }
        }
    }

    #[test]
    fn a_same_day_window_holds_its_start_and_not_its_end() {
        let w = DailyWindow::from_hm(8, 0, 17, 0).unwrap();
        assert!(!w.wraps_midnight());
        assert!(w.contains_hm(8, 0));
        assert!(w.contains_hm(16, 59));
        assert!(!w.contains_hm(17, 0));
        assert!(!w.contains_hm(7, 59));
        assert!(!w.contains_hm(0, 0));
    }

    #[test]
    fn an_overnight_window_is_the_two_pieces_either_side_of_midnight() {
        let w = DailyWindow::from_hm(22, 0, 7, 0).unwrap();
        assert!(w.wraps_midnight());
        for (h, m) in [(22, 0), (23, 59), (0, 0), (6, 59)] {
            assert!(w.contains_hm(h, m), "{h}:{m} should be inside");
        }
        for (h, m) in [(7, 0), (12, 0), (21, 59)] {
            assert!(!w.contains_hm(h, m), "{h}:{m} should be outside");
        }
    }

    /// The regression the three hand-rolled copies shared: an hour past 23
    /// used to become a minute count past 1439, which compared as an overnight
    /// window and then never opened — so the feature stopped happening
    /// silently. There is now no way to build the window at all.
    #[test]
    fn a_window_cannot_be_built_from_a_time_that_does_not_exist() {
        assert!(DailyWindow::from_hm(25, 0, 7, 0).is_none());
        assert!(DailyWindow::from_hm(22, 0, 7, 60).is_none());
        assert!(DailyWindow::from_hm(24, 0, 24, 0).is_none());

        // And a clock reading that is not a time is inside no window, rather
        // than being arithmetic'd into one.
        let w = DailyWindow::from_hm(0, 0, 23, 59).unwrap();
        assert!(!w.contains_hm(24, 0));
        assert!(!w.contains_hm(12, 60));
    }

    #[test]
    fn a_window_that_starts_where_it_ends_is_empty_rather_than_all_day() {
        let w = DailyWindow::from_hm(22, 0, 22, 0).unwrap();
        assert!(!w.wraps_midnight());
        for hour in 0..24u8 {
            assert!(
                !w.contains_hm(hour, 0),
                "{hour}:00 is inside an empty window"
            );
        }
        // The all-day window is the one that ends a minute before it starts.
        let all = DailyWindow::from_hm(0, 1, 0, 0).unwrap();
        for hour in 0..24u8 {
            assert!(all.contains_hm(hour, 30));
        }
    }
}
