//! POSIX `TZ`-string timezone rules — the shared engine.
//!
//! `no_std`, allocation-free and dependency-free, because it is linked into
//! both the libc (`posix`, which is `no_std` on the target) and userspace
//! programs that must agree with it byte for byte.
//!
//! ## Why this is a crate and not a module
//!
//! It has two consumers that cannot share a module: `posix`, whose
//! `tzset`/`localtime`/`mktime`/`strftime` are the C-visible interface, and
//! `userspace/oils`, whose `printf '%(FMT)T'` and `\D{...}` prompt escapes
//! render broken-down time without going through the libc at all.  Before
//! this crate existed both were independently hard-wired to UTC; fixing only
//! one would have left the shell disagreeing with every C program on the
//! machine about what time it is, which is worse than both being wrong
//! together.
//!
//! ## What is supported
//!
//! The full POSIX.1 `TZ` string grammar (the form a `TZ=` assignment carries
//! directly, as opposed to a zoneinfo file name):
//!
//! ```text
//! std offset [ dst [ offset ] [ , start [ /time ] , end [ /time ] ] ]
//! ```
//!
//! * **Names** are either a run of three or more alphabetic bytes (`EST`) or a
//!   `<…>`-quoted run of alphanumerics, `+` and `-` (`<-04>`), which is how
//!   zones whose abbreviation is a numeric offset are spelled.
//! * **Offsets** are `[+|-]hh[:mm[:ss]]` and use the POSIX sign convention:
//!   the value is what must be *added to local time to get UTC*, so `EST5`
//!   means UTC = local + 5 h, i.e. a `gmtoff` of −5 h.  This module stores and
//!   returns `gmtoff` (seconds **east** of Greenwich, the `tm_gmtoff` sense),
//!   because that is the sign every caller actually wants; the inversion
//!   happens once, here, at parse time.
//! * **DST offset** defaults to one hour ahead of standard time when omitted.
//! * **Transition rules** are `Jn` (1‥365, never counting February 29), `n`
//!   (0‥365, counting it), or `Mm.w.d` (month, week-of-month with 5 meaning
//!   "last", weekday with Sunday = 0).  The optional `/time` defaults to
//!   02:00:00 and may be signed and exceed 24 h, which real zones use to
//!   express "at 00:00 on the following day".
//! * **Omitted rules** with a DST name present default to the United States
//!   rules in force since 2007 (`M3.2.0,M11.1.0`), matching glibc and musl.
//!
//! ## Binary zoneinfo files
//!
//! A POSIX `TZ` string carries exactly one rule set, so it cannot express that
//! the United States moved the start of daylight saving in 2007.  The binary
//! TZif files under `/usr/share/zoneinfo` can, and [`TzFile`] reads them.  A
//! TZif v2+ file ends with a POSIX `TZ` string covering everything past its
//! last recorded transition, so [`Tz`] is the *tail* of [`TzFile`] rather than
//! being superseded by it — the two paths share one engine and cannot
//! disagree about a future date.
//!
//! ## What is not supported, and why that is safe
//!
//! Resolving a zoneinfo *name* (`America/New_York`) to a file still needs the
//! tzdata database to be installed, which SlateOS does not ship yet — that is
//! a packaging decision of its own, tracked in `known-issues.md`.  A `TZ` that
//! is neither a POSIX string nor a readable TZif file falls back to UTC, which
//! is exactly what glibc does when it cannot find the named file, so such a
//! program is no worse off than before this module existed.
//!
//! ## Precision
//!
//! Transitions are computed per-year from the rules, so this is correct for
//! any year the rules actually describe.  It does **not** model historical
//! changes (a zone that switched rules in 1987 renders post-2007 rules for
//! 1987), because a POSIX `TZ` string carries only one rule set by
//! construction — that limitation is in the format, not in this code.

#![no_std]
#![deny(clippy::all, clippy::pedantic)]
// Defensive lints per CLAUDE.md: this crate parses attacker-shaped input (a
// `TZ` string can come from anywhere) and is linked into the libc, so a panic
// here is a denial of service in every program on the machine.  Tests may
// panic freely — that is what an assertion is.
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]
#![cfg_attr(not(test), warn(clippy::indexing_slicing))]
#![cfg_attr(not(test), warn(clippy::arithmetic_side_effects))]

mod tzif;

pub use tzif::TzFile;

/// Longest zone abbreviation stored, in bytes.
///
/// POSIX only requires `TZNAME_MAX` (6), but real zone strings carry longer
/// names and glibc accepts them, so we size for the longest that occurs in
/// practice with room to spare.  A longer name is rejected rather than
/// truncated: a silently shortened abbreviation would print wrong output
/// forever, whereas a rejection falls back to UTC, which is at least a
/// recognisable answer.
pub const TZ_NAME_CAP: usize = 32;

/// Seconds in a day.
const SECS_PER_DAY: i64 = 86_400;

/// Default DST transition time when a rule omits `/time`: 02:00:00 local.
const DEFAULT_TRANSITION_TIME: i32 = 2 * 3600;

/// A zone abbreviation, stored inline so the whole [`Tz`] is `Copy` and needs
/// no allocator (this crate is `no_std` on the target).
#[derive(Clone, Copy)]
pub struct TzName {
    bytes: [u8; TZ_NAME_CAP],
    len: u8,
}

impl TzName {
    /// The name `UTC`.
    ///
    /// A `const` rather than a `TzName::new(b"UTC")` call so that [`Tz::UTC`]
    /// can be one too — a `static` holding the current zone needs a `const`
    /// initialiser, and a fallible constructor for a three-byte literal that
    /// obviously fits is a `unwrap` waiting to be written.
    #[allow(
        clippy::indexing_slicing,
        reason = "const-evaluated: these indices are bounds-checked at compile time"
    )]
    pub const UTC: Self = {
        let mut bytes = [0; TZ_NAME_CAP];
        bytes[0] = b'U';
        bytes[1] = b'T';
        bytes[2] = b'C';
        Self { bytes, len: 3 }
    };

    /// Build a name from bytes, rejecting anything that will not fit.
    fn new(src: &[u8]) -> Option<Self> {
        if src.len() > TZ_NAME_CAP {
            return None;
        }
        let mut bytes = [0u8; TZ_NAME_CAP];
        // `src.len() <= TZ_NAME_CAP` was just checked, so the slice is in range.
        bytes.get_mut(..src.len())?.copy_from_slice(src);
        Some(Self {
            bytes,
            len: src.len() as u8,
        })
    }

    /// The name's bytes, without a trailing NUL.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len as usize).unwrap_or(&[])
    }
}

impl core::fmt::Debug for TzName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Zone abbreviations are ASCII by grammar, so this never loses bytes.
        for &b in self.as_bytes() {
            write!(f, "{}", b as char)?;
        }
        Ok(())
    }
}

impl PartialEq for TzName {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for TzName {}

/// Which day of a year a DST transition falls on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TzDate {
    /// `Jn` — day `n` of 1‥365 with February 29 never counted, so `J60` is
    /// March 1 in every year, leap or not.
    JulianNoLeap(u16),
    /// `n` — day `n` of 0‥365 with February 29 counted, so day 59 is
    /// February 29 in a leap year and March 1 otherwise.
    ZeroBased(u16),
    /// `Mm.w.d` — the `w`-th `d`-day of month `m`.  `w == 5` means the last
    /// one in the month, however many there are.
    MonthWeekDay {
        /// Month, 1‥12.
        month: u8,
        /// Week of month, 1‥5 (5 = last).
        week: u8,
        /// Weekday, 0‥6 with Sunday = 0.
        day: u8,
    },
}

impl TzDate {
    /// This rule's day within `year`, as a 0-based index into that year's
    /// days (so February 29 of a leap year is index 59).
    #[allow(clippy::arithmetic_side_effects)]
    fn day_of_year(self, year: i64) -> i64 {
        let leap = is_leap(year);
        match self {
            // `n` is 1-based and skips February 29, so from March onwards the
            // real index is one higher in a leap year.  `n <= 59` is January
            // 1 through February 28, which is unaffected.
            Self::JulianNoLeap(n) => {
                let n = i64::from(n).clamp(1, 365);
                if leap && n > 59 { n } else { n - 1 }
            }
            Self::ZeroBased(n) => i64::from(n).clamp(0, if leap { 365 } else { 364 }),
            Self::MonthWeekDay { month, week, day } => {
                let month = i64::from(month).clamp(1, 12);
                let week = i64::from(week).clamp(1, 5);
                let day = i64::from(day).clamp(0, 6);
                // Weekday of the first of the month.  1970-01-01 was a
                // Thursday, index 4 with Sunday = 0.
                let first = days_from_civil(year, month as u32, 1);
                let first_wday = (first + 4).rem_euclid(7);
                // Days to step from the 1st to the first `day`-day.
                let mut mday = 1 + (day - first_wday).rem_euclid(7);
                // Then advance `week - 1` whole weeks, backing off if that
                // overshoots the month — which is what `week == 5` ("last")
                // relies on, and which also makes a nonsensical `M2.5.0` in a
                // short month mean "the last Sunday" rather than overflow.
                mday += (week - 1) * 7;
                let dim = i64::from(days_in_month(month as u32, year));
                while mday > dim {
                    mday -= 7;
                }
                days_from_civil(year, month as u32, mday as u32) - days_from_civil(year, 1, 1)
            }
        }
    }
}

/// One end of a DST period: which day, and at what local second of that day.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TzTransition {
    /// The day the change happens.
    pub date: TzDate,
    /// Seconds after local midnight.  May be negative or exceed one day —
    /// real zone strings use `/24` and `/-1` to say "midnight of the next
    /// day" and "an hour before midnight".
    pub time: i32,
}

/// The daylight-saving half of a zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TzDst {
    /// Abbreviation used while DST is in effect (`EDT`).
    pub name: TzName,
    /// Seconds **east** of Greenwich while DST is in effect.
    pub gmtoff: i32,
    /// When DST begins.
    pub start: TzTransition,
    /// When DST ends.
    pub end: TzTransition,
}

/// A parsed POSIX `TZ` string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tz {
    /// Abbreviation used outside DST (`EST`).
    pub std_name: TzName,
    /// Seconds **east** of Greenwich outside DST.
    pub std_gmtoff: i32,
    /// The DST half, absent for a zone that does not observe it.
    pub dst: Option<TzDst>,
}

/// The zone state in effect at one instant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TzInfo {
    /// Seconds east of Greenwich — add this to UTC to get local time.  This is
    /// the `tm_gmtoff` sense, the opposite of the POSIX `timezone` global.
    pub gmtoff: i32,
    /// Whether daylight saving is in effect.
    pub is_dst: bool,
    /// The abbreviation to print for `%Z`.
    pub name: TzName,
}

impl Tz {
    /// The UTC zone, used whenever `TZ` is unset, empty or unparseable.
    ///
    /// A `const` so a `static` holding the process's current zone can be
    /// initialised with it directly.
    pub const UTC: Self = Self {
        std_name: TzName::UTC,
        std_gmtoff: 0,
        dst: None,
    };

    /// The UTC zone.  See [`Tz::UTC`].
    #[must_use]
    pub fn utc() -> Self {
        Self::UTC
    }

    /// Parse a POSIX `TZ` string.
    ///
    /// Returns `None` for anything that is not one — most importantly a
    /// zoneinfo name like `America/New_York`, which needs a database we do not
    /// ship.  A leading `:` is stripped first, since POSIX reserves that
    /// prefix for implementation-defined (in practice, file-name) forms and
    /// glibc still accepts `:EST5EDT`.
    #[must_use]
    pub fn parse(s: &[u8]) -> Option<Self> {
        let s = s.strip_prefix(b":").unwrap_or(s);
        if s.is_empty() {
            return None;
        }
        let mut p = Cursor::new(s);

        let std_name = p.name()?;
        // POSIX requires an offset after the standard name.  (glibc treats a
        // bare name as an error too, falling back to UTC.)
        // POSIX's sign is west-positive; ours is east-positive, so invert
        // once here and never again.
        let std_gmtoff = p.offset()?.checked_neg()?;

        let dst = if p.at_end() {
            None
        } else {
            let name = p.name()?;
            // An omitted DST offset means one hour ahead of standard time.
            let gmtoff = if p
                .peek()
                .is_some_and(|c| c == b'+' || c == b'-' || c.is_ascii_digit())
            {
                p.offset()?.checked_neg()?
            } else {
                std_gmtoff.checked_add(3600)?
            };
            let (start, end) = if p.eat(b',') {
                let start = p.transition()?;
                if !p.eat(b',') {
                    return None;
                }
                let end = p.transition()?;
                (start, end)
            } else {
                // Rules omitted: the US rules in force since 2007, which is
                // what glibc and musl also substitute.
                (
                    TzTransition {
                        date: TzDate::MonthWeekDay {
                            month: 3,
                            week: 2,
                            day: 0,
                        },
                        time: DEFAULT_TRANSITION_TIME,
                    },
                    TzTransition {
                        date: TzDate::MonthWeekDay {
                            month: 11,
                            week: 1,
                            day: 0,
                        },
                        time: DEFAULT_TRANSITION_TIME,
                    },
                )
            };
            Some(TzDst {
                name,
                gmtoff,
                start,
                end,
            })
        };

        // Trailing junk means this was not a POSIX string after all; better to
        // fall back to UTC than to honour half of it.
        if !p.at_end() {
            return None;
        }
        Some(Self {
            std_name,
            std_gmtoff,
            dst,
        })
    }

    /// Whether this zone ever observes daylight saving (the POSIX `daylight`
    /// global).
    #[must_use]
    pub fn has_dst(&self) -> bool {
        self.dst.is_some()
    }

    /// The zone state at UTC instant `t` (seconds since the epoch).
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn lookup(&self, t: i64) -> TzInfo {
        let Some(dst) = self.dst else {
            return TzInfo {
                gmtoff: self.std_gmtoff,
                is_dst: false,
                name: self.std_name,
            };
        };
        if self.is_dst_at(t, &dst) {
            TzInfo {
                gmtoff: dst.gmtoff,
                is_dst: true,
                name: dst.name,
            }
        } else {
            TzInfo {
                gmtoff: self.std_gmtoff,
                is_dst: false,
                name: self.std_name,
            }
        }
    }

    /// Whether DST is in effect at UTC instant `t`.
    #[allow(clippy::arithmetic_side_effects)]
    fn is_dst_at(&self, t: i64, dst: &TzDst) -> bool {
        // Pick the year by local standard time.  A transition is at most a
        // day from where this lands, and both boundaries are recomputed from
        // that year, so the answer is stable across the year boundary: for a
        // southern-hemisphere zone the DST window wraps and the comparison
        // below handles it explicitly.
        let year = year_of_day(
            t.saturating_add(i64::from(self.std_gmtoff))
                .div_euclid(SECS_PER_DAY),
        );
        // Each boundary is expressed in the wall clock in force *just before*
        // it: DST begins at standard time and ends at daylight time.  Getting
        // this backwards shifts every transition by an hour.
        let start = transition_utc(&dst.start, year, self.std_gmtoff);
        let end = transition_utc(&dst.end, year, dst.gmtoff);
        if start <= end {
            t >= start && t < end
        } else {
            // Southern hemisphere: the DST window straddles New Year.
            t >= start || t < end
        }
    }

    /// Convert a local wall-clock time to UTC.
    ///
    /// `local` is seconds since the epoch *as if* the broken-down local time
    /// were UTC — i.e. what `timegm` returns for the same `struct tm`.
    /// `isdst_hint` follows `tm_isdst`: negative means "work it out", zero
    /// means "standard time", positive means "daylight time".
    ///
    /// Returns the UTC instant and the offset that was applied.  Local time is
    /// not a bijection — an hour vanishes each spring and repeats each
    /// autumn — so for the ambiguous hour we resolve to the offset in force
    /// *before* the transition (the earlier of the two instants, matching
    /// glibc), and for the nonexistent hour we take the standard-time reading,
    /// which lands just after the jump.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn local_to_utc(&self, local: i64, isdst_hint: i32) -> (i64, TzInfo) {
        let Some(dst) = self.dst else {
            let info = TzInfo {
                gmtoff: self.std_gmtoff,
                is_dst: false,
                name: self.std_name,
            };
            return (local.saturating_sub(i64::from(self.std_gmtoff)), info);
        };

        let std_guess = local.saturating_sub(i64::from(self.std_gmtoff));
        let dst_guess = local.saturating_sub(i64::from(dst.gmtoff));
        let std_ok = !self.is_dst_at(std_guess, &dst);
        let dst_ok = self.is_dst_at(dst_guess, &dst);

        // An explicit `tm_isdst` wins whenever the requested branch is
        // self-consistent; POSIX lets us disregard it otherwise.
        let use_dst = match (isdst_hint.signum(), std_ok, dst_ok) {
            (1, _, true) => true,
            (0, true, _) => false,
            // Exactly one reading is consistent — the unambiguous case.
            (_, true, false) => false,
            (_, false, true) => true,
            // Both consistent: the repeated autumn hour.  Take the earlier
            // instant, which is the one still on the pre-transition offset.
            (_, true, true) => std_guess > dst_guess,
            // Neither consistent: the vanished spring hour.  Standard time
            // places it just past the jump, as glibc does.
            (_, false, false) => false,
        };

        if use_dst {
            (
                dst_guess,
                TzInfo {
                    gmtoff: dst.gmtoff,
                    is_dst: true,
                    name: dst.name,
                },
            )
        } else {
            (
                std_guess,
                TzInfo {
                    gmtoff: self.std_gmtoff,
                    is_dst: false,
                    name: self.std_name,
                },
            )
        }
    }
}

/// The UTC instant of a transition in `year`, given the offset in force just
/// before it.
#[allow(clippy::arithmetic_side_effects)]
fn transition_utc(t: &TzTransition, year: i64, gmtoff_before: i32) -> i64 {
    let day = days_from_civil(year, 1, 1).saturating_add(t.date.day_of_year(year));
    day.saturating_mul(SECS_PER_DAY)
        .saturating_add(i64::from(t.time))
        .saturating_sub(i64::from(gmtoff_before))
}

// ---------------------------------------------------------------------------
// Grammar cursor
// ---------------------------------------------------------------------------

/// A byte cursor over a `TZ` string.
struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a [u8]) -> Self {
        Self { s, i: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }

    fn bump(&mut self) {
        self.i = self.i.saturating_add(1);
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// A zone abbreviation: three or more alphabetic bytes, or a `<…>`-quoted
    /// run of alphanumerics, `+` and `-`.
    fn name(&mut self) -> Option<TzName> {
        if self.eat(b'<') {
            let start = self.i;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'-')
            {
                self.bump();
            }
            let end = self.i;
            if !self.eat(b'>') {
                return None;
            }
            let raw = self.s.get(start..end)?;
            if raw.len() < 3 {
                return None;
            }
            TzName::new(raw)
        } else {
            let start = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                self.bump();
            }
            let raw = self.s.get(start..self.i)?;
            if raw.len() < 3 {
                return None;
            }
            TzName::new(raw)
        }
    }

    /// An unsigned decimal run, bounded so a pathological string cannot
    /// overflow.  Returns `None` if there is no digit at all.
    #[allow(clippy::arithmetic_side_effects)]
    fn number(&mut self) -> Option<i32> {
        let mut any = false;
        let mut n: i32 = 0;
        while let Some(c) = self.peek() {
            if !c.is_ascii_digit() {
                break;
            }
            any = true;
            n = n.checked_mul(10)?.checked_add(i32::from(c - b'0'))?;
            self.bump();
        }
        if any { Some(n) } else { None }
    }

    /// `[+|-]hh[:mm[:ss]]`, returned in seconds with POSIX's own sign (positive
    /// = west of Greenwich).  Callers negate it to get `gmtoff`.
    #[allow(clippy::arithmetic_side_effects)]
    fn offset(&mut self) -> Option<i32> {
        let neg = match self.peek() {
            Some(b'-') => {
                self.bump();
                true
            }
            Some(b'+') => {
                self.bump();
                false
            }
            _ => false,
        };
        // POSIX bounds the hour at 24 for an offset; glibc allows up to 167 in
        // a transition *time*, which `transition_time` handles separately.
        let hh = self.number()?;
        if hh > 24 {
            return None;
        }
        let mut secs = hh.checked_mul(3600)?;
        if self.eat(b':') {
            let mm = self.number()?;
            if mm > 59 {
                return None;
            }
            secs = secs.checked_add(mm.checked_mul(60)?)?;
            if self.eat(b':') {
                let ss = self.number()?;
                if ss > 59 {
                    return None;
                }
                secs = secs.checked_add(ss)?;
            }
        }
        Some(if neg { -secs } else { secs })
    }

    /// The `/time` half of a transition rule: like an offset but with the
    /// ordinary sign (seconds after local midnight) and a wider hour range,
    /// because real zones say `/24` for "midnight of the next day".
    #[allow(clippy::arithmetic_side_effects)]
    fn transition_time(&mut self) -> Option<i32> {
        let neg = match self.peek() {
            Some(b'-') => {
                self.bump();
                true
            }
            Some(b'+') => {
                self.bump();
                false
            }
            _ => false,
        };
        let hh = self.number()?;
        if hh > 167 {
            return None;
        }
        let mut secs = hh.checked_mul(3600)?;
        if self.eat(b':') {
            let mm = self.number()?;
            if mm > 59 {
                return None;
            }
            secs = secs.checked_add(mm.checked_mul(60)?)?;
            if self.eat(b':') {
                let ss = self.number()?;
                if ss > 59 {
                    return None;
                }
                secs = secs.checked_add(ss)?;
            }
        }
        Some(if neg { -secs } else { secs })
    }

    /// A transition rule: `Jn`, `n`, or `Mm.w.d`, with an optional `/time`.
    fn transition(&mut self) -> Option<TzTransition> {
        let date = match self.peek() {
            Some(b'J') => {
                self.bump();
                let n = self.number()?;
                if !(1..=365).contains(&n) {
                    return None;
                }
                TzDate::JulianNoLeap(n as u16)
            }
            Some(b'M') => {
                self.bump();
                let month = self.number()?;
                if !(1..=12).contains(&month) || !self.eat(b'.') {
                    return None;
                }
                let week = self.number()?;
                if !(1..=5).contains(&week) || !self.eat(b'.') {
                    return None;
                }
                let day = self.number()?;
                if !(0..=6).contains(&day) {
                    return None;
                }
                TzDate::MonthWeekDay {
                    month: month as u8,
                    week: week as u8,
                    day: day as u8,
                }
            }
            _ => {
                let n = self.number()?;
                if !(0..=365).contains(&n) {
                    return None;
                }
                TzDate::ZeroBased(n as u16)
            }
        };
        let time = if self.eat(b'/') {
            self.transition_time()?
        } else {
            DEFAULT_TRANSITION_TIME
        };
        Some(TzTransition { date, time })
    }
}

// ---------------------------------------------------------------------------
// Calendar helpers
// ---------------------------------------------------------------------------

/// Whether `year` is a Gregorian leap year.
#[must_use]
pub fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1‥12) of `year`.
#[must_use]
pub fn days_in_month(month: u32, year: i64) -> u32 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && is_leap(year) {
        29
    } else {
        DAYS.get((month.max(1) as usize).saturating_sub(1))
            .copied()
            .unwrap_or(30)
    }
}

/// Days since 1970-01-01 for a Gregorian date.
///
/// Howard Hinnant's `days_from_civil`, which is branch-free and correct for
/// the whole `i64` range rather than just post-1970.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let m = i64::from(month.clamp(1, 12));
    let d = i64::from(day.max(1));
    let y = if m <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The Gregorian `(year, month, day)` containing `days` (days since
/// 1970-01-01), with `month` and `day` 1-based.
///
/// Howard Hinnant's `civil_from_days`, and the exact inverse of
/// [`days_from_civil`] for every date that function can produce — including
/// dates before 1970, which is where hand-rolled versions of this go wrong.
///
/// # Why this is public
///
/// It is the other half of a bijection whose forward direction
/// ([`days_from_civil`]) was already exported. Exporting only one direction
/// does not stop callers needing the other; it only stops them sharing it. By
/// 2026-08-20 the tree held **six** independent transcriptions of this
/// function, and one of them — the file manager's — estimated the month as
/// `day_of_year / 30 + 1` and so reported a wrong date for every timestamp
/// before 2000-03-01. Every value it returned was in range, so no clamp and no
/// assertion could have caught it (see
/// `requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`).
///
/// [`year_of_day`] is defined in terms of this function, so the two cannot
/// disagree about which calendar year a date falls in.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
// The two casts at the end are exact: `m` is 1‥=12 and `d` is 1‥=31, and those
// ranges are enforced by the arithmetic above rather than by convention. The
// allow is on the function because attributes on a tail expression are not
// stable Rust.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so that the leap day lands at the end of
    // the year and the era arithmetic below has no special case for February.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // 0‥=146_096
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // 0‥=399
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 0‥=365, from March 1
    let mp = (5 * doy + 2) / 153; // March-based month index, 0‥=11
    let d = doy - (153 * mp + 2) / 5 + 1; // 1‥=31
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // 1‥=12
    // `mp >= 10` is January or February, which belong to the *next* calendar
    // year under a March-based count.
    let year = if mp >= 10 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

/// The Gregorian year containing `days` (days since 1970-01-01).
///
/// A thin projection of [`civil_from_days`]; see there for why the whole date
/// is available and why this is not a second copy of the algorithm.
#[must_use]
pub fn year_of_day(days: i64) -> i64 {
    civil_from_days(days).0
}

/// The smallest instant an MS-DOS date/time pair can represent:
/// 1980-01-01 00:00:00, as seconds since the Unix epoch.
pub const DOS_EPOCH_UNIX: i64 = 315_532_800;

/// The largest Unix second that still lands inside the MS-DOS range:
/// 2107-12-31 23:59:59.  The year field is 7 bits counting from 1980, so 2107
/// is the last year it can name.
///
/// This is an **inclusive** bound and is one second later than the last
/// *distinct* pair, which is 2107-12-31 23:59:58: seconds are stored halved, so
/// :59 rounds down into the :58 bucket rather than falling outside the format.
/// Treating :59 as out of range would discard a second that the format does in
/// fact accept.
pub const DOS_END_UNIX: i64 = 4_354_819_199;

/// Pack a Unix timestamp into the MS-DOS date/time pair used by ZIP and FAT,
/// as `(date << 16) | time`.
///
/// Returns **`0`** for any instant the format cannot represent — before
/// 1980-01-01 or after 2107-12-31. Zero is the agreed encoding for "no
/// modification time recorded": it is day 0 of month 0, which is not a
/// representable date, so a reader cannot mistake it for one.
///
/// # Why it refuses instead of clamping
///
/// Clamping an out-of-range time to the DOS minimum would stamp
/// `1980-01-01` on it — a real, plausible date that no reader can distinguish
/// from a file genuinely last written that day. That is the exact fabrication
/// `A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01` was filed for and
/// design-decisions.md §618 decided against. An unknown time must not be
/// rendered as a known one, so the failure is visible (`-` in a Date column)
/// rather than invisible.
///
/// A Unix timestamp of `0` is 1970, which is before the DOS epoch, so the
/// common "time not available" sentinel maps to "not recorded" on its own.
///
/// # Seconds are stored halved
///
/// The DOS time field gives seconds 5 bits, holding 0‥=29, which is the second
/// divided by two. This rounds **down**, so a recorded time is never later than
/// the real one — a file's mtime that reads as earlier than it was is a
/// resolution limit, whereas one that reads later can put a file "ahead of" an
/// event that actually preceded it.
///
/// # Timezone
///
/// DOS timestamps are *local* time with no zone recorded — the format has
/// nowhere to put one. This function does no zone conversion, so it produces
/// local time only if handed local time. Kernel callers have no user timezone
/// available and therefore write UTC into a slot readers will interpret as
/// local; that is a limitation of the format rather than of this function, and
/// every OS writing ZIPs has it.
///
/// # Why this lives here
///
/// `ziparchive` is the obvious-looking home and is the wrong one: it is
/// `no_std` and linked into the kernel, and encoding a DOS pair needs a
/// calendar, which would drag one into kernel space or force a second to exist.
/// `kernel/src/fs/zip.rs` is worse — a module of a *binary* crate cannot be
/// depended on, which is what stranded the ZIP parser in the kernel binary and
/// forced its promotion to a root crate. This crate is already `no_std`,
/// dependency-free, and already a dependency of both the kernel and `guitk`.
/// See design-decisions.md §621.
///
/// The inverse is deliberately absent: the only decoder today is
/// `apps/archivemanager`'s, which also range-checks the pair as part of
/// deciding whether to show it at all. Add one here when a second caller wants
/// it, rather than shipping API nothing calls.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
// Every cast below is exact and the ranges are enforced by the arithmetic, not
// by convention: `year - 1980` is 0‥=127 because of the bounds check, `month`
// is 1‥=12 and `day` 1‥=31 from `civil_from_days`, and `rem` is 0‥=86_399
// because `rem_euclid` cannot be negative. The allow is on the function because
// attributes on a tail expression are not stable Rust.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn dos_datetime_from_unix(secs: i64) -> u32 {
    // Checked before any arithmetic, so the shifts below cannot overflow their
    // fields: outside this window `year - 1980` does not fit in 7 bits.
    if secs < DOS_EPOCH_UNIX || secs > DOS_END_UNIX {
        return 0;
    }

    // `div_euclid`/`rem_euclid` rather than `/` and `%` so that the remainder
    // is never negative. It cannot be, given the bounds check above, but the
    // truncating forms would be wrong if this function were ever given a wider
    // range and the bug would be a silently shifted time rather than a panic.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);

    let date = (((year - 1980) as u32) << 9) | (month << 5) | day;
    let time = (((rem / 3600) as u32) << 11)
        | ((((rem % 3600) / 60) as u32) << 5)
        | (((rem % 60) / 2) as u32);

    (date << 16) | time
}

/// Decode a packed MS-DOS date/time pair back into whole seconds since the
/// Unix epoch. The inverse of [`dos_datetime_from_unix`].
///
/// Returns `None` when the pair does not name a real instant. That covers both
/// `0` — the agreed "no modification time recorded" encoding — and any pair
/// whose fields are individually in range but jointly impossible.
///
/// # Why the return is an `Option` and not a clamped `i64`
///
/// The DOS fields are wider than the values they may hold. Month gets 4 bits
/// (0‥=15) but only 1‥=12 are dates; day gets 5 (0‥=31) but no month has 0
/// days and only seven have 31; hours get 5 bits (0‥=31) against 24 valid, and
/// minutes 6 (0‥=63) against 60. So a malformed or hostile archive can name
/// "month 15", "February 30" or "25:61:58", and every one of those is a bit
/// pattern the format physically permits.
///
/// A decoder that normalises them — the usual shortcut of feeding the raw
/// numbers to a days-since-epoch formula — turns nonsense into a *plausible*
/// date silently: `days_from_civil` is happy to accept month 15 and roll it
/// into the following year, so `2020-15-01` decodes as `2021-03-01` and is
/// then indistinguishable from an archive that really said so. Refusing keeps
/// a corrupt field visible as a corrupt field, which is the same reason the
/// encoder refuses out-of-range instants rather than clamping them
/// (design-decisions.md §618).
///
/// # Seconds
///
/// The stored second is halved, so decoding multiplies by two and the result
/// is always even. A pair that was encoded from an odd second decodes one
/// second earlier — the resolution loss is in the format, and it errs in the
/// direction the encoder documents: never later than the real instant.
///
/// # Timezone
///
/// DOS pairs carry local time with no zone recorded, and this does no zone
/// conversion — it returns the instant that the fields name read as UTC. The
/// caveat is the same one on the encoder and is a property of the format.
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
// The arithmetic below cannot overflow: every field is masked to at most 7
// bits before use, `year` is 1980‥=2107, and `days_from_civil` of a date in
// that range is ~3_650‥=50_400, so the seconds total is far inside `i64`. The
// allow is on the function because attributes on expressions are not stable.
pub fn unix_from_dos_datetime(packed: u32) -> Option<i64> {
    // `0` is the sentinel, not a date. Checking it first means the field
    // decoding below never has to special-case an all-zero pair, and it is
    // also caught by the month/day validation that follows — belt and braces,
    // because this is the case callers actually hit.
    if packed == 0 {
        return None;
    }

    let date = packed >> 16;
    let time = packed & 0xFFFF;

    let year = i64::from(date >> 9) + 1980;
    let month = (date >> 5) & 0x0F;
    let day = date & 0x1F;

    let hour = time >> 11;
    let minute = (time >> 5) & 0x3F;
    // Stored halved; 5 bits hold 0‥=31, so this is 0‥=62 and an encoder that
    // wrote 30 or 31 would name second 60 or 62, which do not exist.
    let second = (time & 0x1F) * 2;

    // Reject every field the format can hold but the calendar cannot name.
    // `days_in_month` is what makes 2020-02-30 fail while 2020-02-29 passes,
    // which a fixed 1..=31 check would not.
    if month == 0 || month > 12 || day == 0 || day > days_in_month(month, year) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `TzName` in a test without the `Option` dance.
    fn name(s: &[u8]) -> TzName {
        TzName::new(s).expect("test name fits")
    }

    #[test]
    fn parses_a_bare_standard_zone() {
        let tz = Tz::parse(b"EST5").expect("valid");
        assert_eq!(tz.std_name, name(b"EST"));
        // POSIX's sign is west-positive; `gmtoff` is east-positive.
        assert_eq!(tz.std_gmtoff, -5 * 3600);
        assert!(tz.dst.is_none());
    }

    #[test]
    fn parses_minutes_and_seconds_in_an_offset() {
        let tz = Tz::parse(b"NPT-5:45").expect("valid");
        assert_eq!(tz.std_gmtoff, 5 * 3600 + 45 * 60);
        let tz = Tz::parse(b"XXX1:02:03").expect("valid");
        assert_eq!(tz.std_gmtoff, -(3600 + 2 * 60 + 3));
    }

    #[test]
    fn parses_an_angle_quoted_numeric_name() {
        let tz = Tz::parse(b"<-04>4").expect("valid");
        assert_eq!(tz.std_name, name(b"-04"));
        assert_eq!(tz.std_gmtoff, -4 * 3600);
    }

    #[test]
    fn a_dst_name_without_an_offset_means_one_hour_ahead() {
        let tz = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("valid");
        let dst = tz.dst.expect("has dst");
        assert_eq!(dst.gmtoff, -4 * 3600);
        assert_eq!(dst.name, name(b"EDT"));
    }

    #[test]
    fn omitted_rules_default_to_the_us_rules() {
        let with = Tz::parse(b"EST5EDT,M3.2.0/2,M11.1.0/2").expect("valid");
        let without = Tz::parse(b"EST5EDT").expect("valid");
        assert_eq!(with, without);
    }

    #[test]
    fn rejects_a_zoneinfo_name() {
        // The whole point of returning `None` here: `America/New_York` needs a
        // database we do not ship, and honouring it half-way would be worse
        // than falling back to UTC.
        assert!(Tz::parse(b"America/New_York").is_none());
        assert!(Tz::parse(b"").is_none());
        assert!(
            Tz::parse(b"AB1").is_none(),
            "a two-letter name is not a zone"
        );
        assert!(Tz::parse(b"EST").is_none(), "an offset is mandatory");
        assert!(
            Tz::parse(b"EST5!!!").is_none(),
            "trailing junk is not honoured"
        );
    }

    #[test]
    fn an_alphabetic_tail_is_a_dst_name_not_junk() {
        // `EST5junk` looks like garbage but is a well-formed POSIX string: a
        // standard zone at −5 whose daylight abbreviation happens to be
        // spelled `junk`, one hour ahead on the default US rules.  glibc reads
        // it the same way, so rejecting it would be the bug.
        let tz = Tz::parse(b"EST5junk").expect("valid POSIX string");
        let dst = tz.dst.expect("has dst");
        assert_eq!(dst.name, name(b"junk"));
        assert_eq!(dst.gmtoff, -4 * 3600);
    }

    #[test]
    fn strips_the_reserved_leading_colon() {
        assert_eq!(Tz::parse(b":EST5"), Tz::parse(b"EST5"));
    }

    /// 2021-03-14 07:00:00Z is the instant US Eastern springs forward.
    const US_SPRING_2021: i64 = 1_615_705_200;
    /// 2021-11-07 06:00:00Z is the instant it falls back.
    const US_FALL_2021: i64 = 1_636_264_800;

    #[test]
    fn us_eastern_transitions_at_the_right_instants() {
        let tz = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("valid");
        assert!(
            !tz.lookup(US_SPRING_2021 - 1).is_dst,
            "one second before the spring jump"
        );
        assert!(tz.lookup(US_SPRING_2021).is_dst, "at the spring jump");
        assert!(
            tz.lookup(US_FALL_2021 - 1).is_dst,
            "one second before the autumn jump"
        );
        assert!(!tz.lookup(US_FALL_2021).is_dst, "at the autumn jump");
        assert_eq!(tz.lookup(US_SPRING_2021).gmtoff, -4 * 3600);
        assert_eq!(tz.lookup(US_SPRING_2021).name, name(b"EDT"));
        assert_eq!(tz.lookup(US_FALL_2021).gmtoff, -5 * 3600);
        assert_eq!(tz.lookup(US_FALL_2021).name, name(b"EST"));
    }

    #[test]
    fn the_southern_hemisphere_window_wraps_the_new_year() {
        // Chile: DST from the first Sunday of September to the first Sunday of
        // April, so January is inside the window and June is outside it.
        let tz = Tz::parse(b"<-04>4<-03>,M9.1.6/24,M4.1.6/24").expect("valid");
        // 2021-01-15T12:00:00Z
        assert!(
            tz.lookup(1_610_712_000).is_dst,
            "January is southern summer"
        );
        // 2021-06-15T12:00:00Z
        assert!(!tz.lookup(1_623_758_400).is_dst, "June is southern winter");
    }

    #[test]
    fn julian_days_skip_february_29() {
        // J60 is March 1 in every year; day 59 (zero-based) is February 29 in
        // a leap year and March 1 otherwise.
        assert_eq!(
            TzDate::JulianNoLeap(60).day_of_year(2020),
            days_from_civil(2020, 3, 1) - days_from_civil(2020, 1, 1)
        );
        assert_eq!(
            TzDate::JulianNoLeap(60).day_of_year(2021),
            days_from_civil(2021, 3, 1) - days_from_civil(2021, 1, 1)
        );
        assert_eq!(TzDate::ZeroBased(59).day_of_year(2020), 59, "2020-02-29");
        assert_eq!(
            TzDate::ZeroBased(59).day_of_year(2021),
            days_from_civil(2021, 3, 1) - days_from_civil(2021, 1, 1)
        );
    }

    #[test]
    fn week_five_means_the_last_such_weekday() {
        // 2021-03: Sundays fall on the 7th, 14th, 21st and 28th, so "week 5"
        // must clamp to the 28th rather than run off the end of the month.
        let last = TzDate::MonthWeekDay {
            month: 3,
            week: 5,
            day: 0,
        }
        .day_of_year(2021);
        assert_eq!(
            last,
            days_from_civil(2021, 3, 28) - days_from_civil(2021, 1, 1)
        );
        let second = TzDate::MonthWeekDay {
            month: 3,
            week: 2,
            day: 0,
        }
        .day_of_year(2021);
        assert_eq!(
            second,
            days_from_civil(2021, 3, 14) - days_from_civil(2021, 1, 1)
        );
    }

    #[test]
    fn a_transition_time_may_exceed_a_day() {
        let tz = Tz::parse(b"AAA3BBB,M10.1.0/24,M3.1.0/24").expect("valid");
        let dst = tz.dst.expect("has dst");
        assert_eq!(dst.start.time, 24 * 3600);
        let tz = Tz::parse(b"AAA3BBB,M10.1.0/-1,M3.1.0/2").expect("valid");
        assert_eq!(tz.dst.expect("has dst").start.time, -3600);
    }

    #[test]
    fn local_to_utc_round_trips_outside_the_transitions() {
        let tz = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("valid");
        for t in [
            US_SPRING_2021 - 86_400,
            US_SPRING_2021 + 86_400,
            US_FALL_2021 + 86_400,
        ] {
            let info = tz.lookup(t);
            let local = t + i64::from(info.gmtoff);
            let (back, back_info) = tz.local_to_utc(local, -1);
            assert_eq!(back, t, "round trip at {t}");
            assert_eq!(back_info.is_dst, info.is_dst);
        }
    }

    #[test]
    fn the_repeated_autumn_hour_resolves_to_the_earlier_instant() {
        let tz = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("valid");
        // 01:30 local occurs twice on the fall-back morning: once on EDT
        // (-4) and again an hour later on EST (-5).  glibc returns the first.
        let local = US_FALL_2021 - 4 * 3600 + 1800 - 3600;
        let (t, info) = tz.local_to_utc(local, -1);
        assert!(
            info.is_dst,
            "the earlier of the two readings is still on DST"
        );
        assert_eq!(t, local - i64::from(info.gmtoff));
        // An explicit tm_isdst = 0 asks for the later, standard-time reading.
        let (t_std, info_std) = tz.local_to_utc(local, 0);
        assert!(!info_std.is_dst);
        assert_eq!(t_std, t + 3600);
    }

    #[test]
    fn the_vanished_spring_hour_lands_just_past_the_jump() {
        let tz = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("valid");
        // 02:30 local never happens: the clock goes 01:59:59 → 03:00:00.
        let local = US_SPRING_2021 - 5 * 3600 + 1800;
        let (t, _) = tz.local_to_utc(local, -1);
        assert!(
            t >= US_SPRING_2021,
            "resolves after the jump, not before it"
        );
    }

    #[test]
    fn a_zone_without_dst_is_a_pure_offset() {
        let tz = Tz::parse(b"UTC0").expect("valid");
        assert_eq!(tz.lookup(0).gmtoff, 0);
        assert!(!tz.lookup(0).is_dst);
        let tz = Tz::parse(b"IST-5:30").expect("valid");
        assert_eq!(tz.lookup(0).gmtoff, 5 * 3600 + 1800);
        let (t, _) = tz.local_to_utc(19_800, -1);
        assert_eq!(t, 0);
    }

    #[test]
    fn year_of_day_agrees_with_days_from_civil() {
        for (y, m, d) in [
            (1969, 12, 31),
            (1970, 1, 1),
            (2000, 2, 29),
            (2021, 3, 14),
            (2100, 12, 31),
        ] {
            assert_eq!(year_of_day(days_from_civil(y, m, d)), y, "{y}-{m}-{d}");
        }
    }

    #[test]
    fn civil_from_days_is_the_exact_inverse_of_days_from_civil() {
        // A round trip over every day in a span that straddles 1970 in both
        // directions, so a version that is only correct forward of the epoch
        // — the failure mode that shipped in the file manager — cannot pass.
        // 1900-01-01 through 2100-12-31 inclusive.
        let first = days_from_civil(1900, 1, 1);
        let last = days_from_civil(2100, 12, 31);
        let mut days = first;
        while days <= last {
            let (y, m, d) = civil_from_days(days);
            assert!(
                (1..=12).contains(&m),
                "month {m} out of range at day {days}"
            );
            assert!(
                (1..=days_in_month(m, y)).contains(&d),
                "day {d} out of range for {y}-{m}"
            );
            assert_eq!(days_from_civil(y, m, d), days, "round trip at {y}-{m}-{d}");
            days += 1;
        }
    }

    #[test]
    fn civil_from_days_names_the_dates_a_month_estimate_gets_wrong() {
        // The three dates from
        // `requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`
        // that the file manager's `day_of_year / 30 + 1` estimate reported
        // wrongly, plus the epoch and the day before it.
        for (y, m, d) in [
            (1969, 12, 31),
            (1970, 1, 1),
            (1985, 7, 4),
            (1999, 6, 15),
            (2000, 2, 29), // a real leap day, which the estimate moved to March
            (2000, 3, 1),
        ] {
            assert_eq!(
                civil_from_days(days_from_civil(y, m, d)),
                (y, m, d),
                "{y}-{m}-{d}"
            );
        }
    }

    #[test]
    fn civil_from_days_handles_dates_far_outside_the_unix_era() {
        // Hinnant's algorithm is correct for the whole `i64` range, and
        // `days_from_civil`'s doc comment promises that; a version with a
        // `days < 0` special case is correct only where it was tested.
        for (y, m, d) in [
            (1, 1, 1),
            (1582, 10, 15), // the Gregorian calendar's first day, proleptic here
            (1600, 2, 29),  // a century leap year
            (1700, 2, 28),  // a century *non*-leap year
            (-1, 12, 31),   // 1 BC in the proleptic Gregorian numbering
            (-400, 2, 29),
            (9999, 12, 31),
        ] {
            assert_eq!(
                civil_from_days(days_from_civil(y, m, d)),
                (y, m, d),
                "{y}-{m}-{d}"
            );
        }
    }

    #[test]
    fn year_of_day_is_civil_from_days_and_cannot_drift_from_it() {
        // Every month boundary over four centuries, which is where a
        // separately-written year projection would disagree first: the
        // March-based count puts January and February in the following year.
        let mut days = days_from_civil(1800, 1, 1);
        let last = days_from_civil(2200, 1, 1);
        while days <= last {
            assert_eq!(year_of_day(days), civil_from_days(days).0);
            days += 1;
        }
    }

    #[test]
    fn rejects_out_of_range_rule_fields() {
        assert!(Tz::parse(b"EST5EDT,M13.1.0,M11.1.0").is_none(), "month 13");
        assert!(Tz::parse(b"EST5EDT,M3.6.0,M11.1.0").is_none(), "week 6");
        assert!(Tz::parse(b"EST5EDT,M3.1.7,M11.1.0").is_none(), "weekday 7");
        assert!(Tz::parse(b"EST5EDT,J0,M11.1.0").is_none(), "Julian day 0");
        assert!(
            Tz::parse(b"EST5EDT,J366,M11.1.0").is_none(),
            "Julian day 366"
        );
        assert!(
            Tz::parse(b"EST5EDT,M3.2.0").is_none(),
            "one rule is not two"
        );
        assert!(Tz::parse(b"EST25").is_none(), "an offset past 24 hours");
    }

    // -- dos_datetime_from_unix ------------------------------------------

    /// Unpack a DOS pair the way a reader does, so the tests below can state
    /// their expectations as dates rather than as hex.
    ///
    /// Deliberately a *test-local* transcription rather than a public inverse:
    /// a round-trip through two functions in the same file that were written
    /// together can agree while both being wrong, so the tests that matter
    /// check against hand-computed constants, and this only exists to make
    /// failure messages readable.
    fn unpack(pair: u32) -> (i64, u32, u32, u32, u32, u32) {
        let (date, time) = (pair >> 16, pair & 0xFFFF);
        (
            1980 + i64::from(date >> 9),
            (date >> 5) & 0x0F,
            date & 0x1F,
            time >> 11,
            (time >> 5) & 0x3F,
            (time & 0x1F) * 2,
        )
    }

    #[test]
    fn packs_a_known_instant_into_the_documented_layout() {
        // 2026-08-26 14:30:00 UTC.
        let pair = dos_datetime_from_unix(1_787_754_600);
        assert_eq!(unpack(pair), (2026, 8, 26, 14, 30, 0));
        // Stated as bits too: the layout is the contract, and `unpack` above
        // would agree with a consistently-wrong packing.
        assert_eq!(
            pair >> 16,
            ((2026 - 1980) << 9) | (8 << 5) | 26,
            "date half"
        );
        assert_eq!(pair & 0xFFFF, (14 << 11) | (30 << 5), "time half");
    }

    #[test]
    fn the_dos_epoch_is_representable_and_the_instant_before_it_is_not() {
        // The boundary is the whole point of the range check, so both sides of
        // it are named rather than assumed.
        assert_eq!(
            unpack(dos_datetime_from_unix(DOS_EPOCH_UNIX)),
            (1980, 1, 1, 0, 0, 0),
            "the DOS epoch itself is a representable date"
        );
        assert_eq!(
            dos_datetime_from_unix(DOS_EPOCH_UNIX - 1),
            0,
            "one second earlier cannot be represented, so it is 'not recorded'"
        );
    }

    #[test]
    fn out_of_range_is_refused_rather_than_clamped() {
        // This is the property `A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01`
        // is about: a clamping implementation returns the DOS minimum here,
        // which reads as a real date of 1980-01-01 and is indistinguishable
        // from a file genuinely written that day.
        let dos_min = dos_datetime_from_unix(DOS_EPOCH_UNIX);
        for (secs, what) in [
            (0_i64, "the Unix epoch, and the 'time unavailable' sentinel"),
            (-1, "one second before the Unix epoch"),
            (i64::MIN, "the far past"),
            (i64::MAX, "the far future"),
            (DOS_END_UNIX + 1, "one second past the last DOS instant"),
        ] {
            let got = dos_datetime_from_unix(secs);
            assert_eq!(got, 0, "{what} must be 'not recorded'");
            assert_ne!(got, dos_min, "{what} must not be clamped to 1980-01-01");
        }
    }

    #[test]
    fn the_last_representable_instant_is_the_end_of_2107() {
        assert_eq!(
            unpack(dos_datetime_from_unix(DOS_END_UNIX)),
            (2107, 12, 31, 23, 59, 58),
            "the year field is 7 bits from 1980, so 2107 is the last year"
        );
        // 127 is the largest value the 7-bit year field holds; a bound that
        // was off by one would either lose 2107 or overflow into the month.
        assert_eq!(dos_datetime_from_unix(DOS_END_UNIX) >> 25, 127);
    }

    #[test]
    fn odd_seconds_round_down_so_a_time_is_never_later_than_it_was() {
        // 2026-08-26 14:30:00 UTC, then the following second.
        let even = dos_datetime_from_unix(1_787_754_600);
        let odd = dos_datetime_from_unix(1_787_754_601);
        assert_eq!(unpack(even).5, 0);
        assert_eq!(unpack(odd).5, 0, "the odd second rounds down, not up");
        assert_eq!(even, odd, "both land in the same two-second bucket");

        // And the next even second is a different bucket, so the rounding is
        // losing one bit of resolution rather than losing the seconds field.
        assert_eq!(unpack(dos_datetime_from_unix(1_787_754_602)).5, 2);
    }

    #[test]
    fn a_leap_day_is_packed_as_itself() {
        // 2024-02-29 12:00:00 UTC. A packing that derived the month from
        // day-of-year over a 365-day year -- the bug that produced six wrong
        // date columns before `civil_from_days` was shared -- reports 03-01
        // here, which is in range and so cannot be caught by a bounds check.
        assert_eq!(
            unpack(dos_datetime_from_unix(1_709_208_000)),
            (2024, 2, 29, 12, 0, 0)
        );
    }

    #[test]
    fn every_representable_day_packs_into_a_valid_in_range_field() {
        // Walks all ~46_750 representable days at noon.  The cheap property --
        // that no field ever overflows into its neighbour -- is exactly the one
        // a hand-rolled packing gets wrong for a handful of dates scattered
        // across a century, which spot checks miss.
        let mut secs = DOS_EPOCH_UNIX + 43_200;
        let mut checked = 0_u32;
        while secs <= DOS_END_UNIX {
            let pair = dos_datetime_from_unix(secs);
            let (year, month, day, hour, minute, second) = unpack(pair);
            assert_ne!(pair, 0, "in-range instant reported as unrecorded");
            assert!((1980..=2107).contains(&year), "year {year} out of range");
            assert!((1..=12).contains(&month), "month {month} out of range");
            assert!(
                day >= 1 && day <= days_in_month(month, year),
                "{year}-{month:02}-{day:02} is not a day that exists"
            );
            assert_eq!((hour, minute, second), (12, 0, 0));
            // And it agrees with the calendar the rest of the crate uses.
            assert_eq!(
                (year, month, day),
                civil_from_days(secs.div_euclid(86_400)),
                "packing disagrees with civil_from_days"
            );
            secs += 86_400;
            checked += 1;
        }
        assert!(checked > 46_000, "expected ~46_750 days, walked {checked}");
    }

    // -- unix_from_dos_datetime ------------------------------------------

    #[test]
    fn decodes_a_known_pair_without_consulting_the_encoder() {
        // 2020-09-13 12:26:40 UTC == 1_600_000_000, packed by hand from the
        // documented layout so this does not merely assert that two functions
        // agree with each other:
        //   date = ((2020-1980) << 9) | (9 << 5) | 13 = 20_781
        //   time = (12 << 11) | (26 << 5) | (40 / 2)  = 25_428
        let packed = (20_781_u32 << 16) | 25_428;
        assert_eq!(unix_from_dos_datetime(packed), Some(1_600_000_000));
    }

    #[test]
    fn the_unrecorded_sentinel_decodes_to_nothing_rather_than_to_1980() {
        // The whole point of the `0` encoding: it must not come back as the
        // DOS epoch, or a blank Date column silently becomes "1980-01-01".
        assert_eq!(unix_from_dos_datetime(0), None);
        assert_ne!(
            unix_from_dos_datetime(dos_datetime_from_unix(DOS_EPOCH_UNIX)),
            None,
            "the real DOS epoch must still decode -- only 0 is the sentinel"
        );
    }

    #[test]
    fn fields_the_format_can_hold_but_the_calendar_cannot_name_are_refused() {
        // Each of these is a bit pattern a malformed or hostile archive can
        // legally contain. A decoder that normalises instead of refusing turns
        // every one of them into a plausible, wrong date.
        let cases: [(u32, u32, &str); 7] = [
            ((2020 - 1980) << 9 | (15 << 5) | 1, 0, "month 15"),
            ((2020 - 1980) << 9 | (0 << 5) | 1, 0, "month 0"),
            ((2020 - 1980) << 9 | (9 << 5) | 0, 0, "day 0"),
            ((2020 - 1980) << 9 | (2 << 5) | 30, 0, "February 30"),
            (
                (2021 - 1980) << 9 | (2 << 5) | 29,
                0,
                "Feb 29 of a common year",
            ),
            ((2020 - 1980) << 9 | (9 << 5) | 13, 25 << 11, "hour 25"),
            ((2020 - 1980) << 9 | (9 << 5) | 13, 61 << 5, "minute 61"),
        ];
        for (date, time, what) in cases {
            assert_eq!(
                unix_from_dos_datetime((date << 16) | time),
                None,
                "{what} must be refused, not normalised into a real date"
            );
        }

        // The near-miss that proves the check is a calendar and not a constant:
        // the same day-29 pattern in a leap year is a date and must decode.
        let leap = ((2020 - 1980) << 9 | (2 << 5) | 29) << 16;
        assert!(
            unix_from_dos_datetime(leap).is_some(),
            "2020-02-29 exists and must decode"
        );
    }

    #[test]
    fn decoding_is_the_exact_inverse_of_encoding_over_the_whole_range() {
        // Walks every representable day, at a time of day whose seconds are
        // even so the halving is lossless and the round-trip must be exact.
        // A swapped shift, an off-by-one epoch, or a mask one bit too narrow
        // survives spot checks and dies here.
        let mut secs = DOS_EPOCH_UNIX + 12 * 3_600 + 26 * 60 + 40;
        let mut checked = 0_u32;
        while secs <= DOS_END_UNIX {
            let pair = dos_datetime_from_unix(secs);
            assert_eq!(
                unix_from_dos_datetime(pair),
                Some(secs),
                "round trip lost {secs}"
            );
            secs += 86_400;
            checked += 1;
        }
        assert!(checked > 46_000, "expected ~46_750 days, walked {checked}");
    }

    #[test]
    fn an_odd_second_round_trips_one_second_early_never_late() {
        // The format stores seconds halved, so odd seconds cannot survive.
        // The direction of the loss is the contract: a decoded time may be
        // earlier than the original, never later, or a file can be ordered
        // ahead of an event that actually preceded it.
        for offset in 0..120_i64 {
            let secs = 1_600_000_000 + offset;
            let back = unix_from_dos_datetime(dos_datetime_from_unix(secs))
                .expect("in-range instant must decode");
            assert!(
                back <= secs && secs - back <= 1,
                "{secs} round-tripped to {back}; must lose at most 1s, downward"
            );
            assert_eq!(back % 2, 0, "decoded seconds are always even");
        }
    }

    #[test]
    fn both_range_edges_survive_a_round_trip() {
        // DOS_END_UNIX is 23:59:59, one second past the last *distinct* pair
        // (23:59:58) because seconds are halved. It must round-trip to :58
        // rather than being refused as out of range.
        assert_eq!(
            unix_from_dos_datetime(dos_datetime_from_unix(DOS_END_UNIX)),
            Some(DOS_END_UNIX - 1),
        );
        assert_eq!(
            unix_from_dos_datetime(dos_datetime_from_unix(DOS_EPOCH_UNIX)),
            Some(DOS_EPOCH_UNIX),
        );
    }
}
