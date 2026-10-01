//! gnulib's `posixtm`: the digit-string timestamps POSIX specifies.
//!
//! Three utilities read one of these, each under its own syntax, and the
//! syntax is all that differs -- upstream's comment, verbatim in substance:
//!
//! | caller | digits | [`Syntax`] |
//! |---|---|---|
//! | `touch -t [[CC]YY]MMDDhhmm[.ss]` | 8, 10 or 12, optional `.ss` | `CENTURY \| SECONDS` |
//! | `touch MMDDhhmm[YY]`, withdrawn by POSIX 1003.1-2001 | 8 or 10, a `YY` in 69-99 | `TRAILING_YEAR \| PRE_2000` |
//! | `date MMDDhhmm[[CC]YY]` | 8, 10 or 12 | `TRAILING_YEAR \| CENTURY` |
//!
//! A two-digit year is POSIX's: 69-99 are 1969-1999 and 00-68 are 2000-2068
//! (the second range refused outright under `PRE_2000`). No year at all is the
//! current year in the local zone.
//!
//! The string is read as a *local* time and turned into an instant through
//! [`Zone::mktime`] -- glibc's, with `tm_isdst` -1 as upstream sets it -- and
//! then the answer is checked against what was asked for: `mktime` carries
//! "September 31" into October and a spring-forward gap into the next hour,
//! and upstream refuses both by comparing the normalised fields with the
//! parsed ones. The one mismatch it forgives is a seconds field of 60, which
//! POSIX requires be accepted and which, with no leap second to land on, means
//! the second after 59.
//!
//! glibc's `mktime` rather than [`Zone::epoch`] because the repeated hour at a
//! fall-back has two answers, and which one `touch -t` stamps is the one
//! glibc's search finds -- from the offset the previous `mktime` in the
//! process left behind, which only the port reproduces.

use localtime::{StructTm, Zone};

/// Upstream's `PDS_*` bits. `LEADING_YEAR` is the absence of `TRAILING_YEAR`,
/// kept as a name because upstream's header keeps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Syntax(u8);

impl Syntax {
    /// `PDS_LEADING_YEAR`: `[[CC]YY]MMDDhhmm`, which is no bit at all.
    pub const LEADING_YEAR: Syntax = Syntax(0);
    /// `PDS_TRAILING_YEAR`: `MMDDhhmm[[CC]YY]`.
    pub const TRAILING_YEAR: Syntax = Syntax(1);
    /// `PDS_CENTURY`: a four-digit year is allowed.
    pub const CENTURY: Syntax = Syntax(2);
    /// `PDS_SECONDS`: a `.ss` suffix is allowed.
    pub const SECONDS: Syntax = Syntax(4);
    /// `PDS_PRE_2000`: a two-digit year must be 69-99.
    pub const PRE_2000: Syntax = Syntax(8);

    /// Both sets of bits.
    #[must_use]
    pub const fn with(self, other: Syntax) -> Syntax {
        Syntax(self.0 | other.0)
    }

    const fn has(self, bit: Syntax) -> bool {
        self.0 & bit.0 != 0
    }
}

/// The fields a string names, before `mktime`: upstream's `struct tm` as
/// `posix_time_parse` fills it, but with the full year and a 1-based month.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fields {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

/// Upstream's `year`: the year from `pairs` -- none, one pair or two -- under
/// `syntax`, `current` supplying the year when there are none.
///
/// `current` is called only then, because upstream's `localtime (&now)` is
/// made only then -- and that call is a `tzset`, which under a `TZ` built
/// from `posixrules` changes how the `mktime` after it anchors the zone.
fn year(pairs: &[i64], syntax: Syntax, current: &dyn Fn() -> i64) -> Option<i64> {
    match *pairs {
        [] => Some(current()),
        [yy] => {
            // POSIX: 00-68 are 2000-2068, 69-99 are 1969-1999.
            if yy <= 68 {
                if syntax.has(Syntax::PRE_2000) {
                    return None;
                }
                Some(yy.saturating_add(2000))
            } else {
                Some(yy.saturating_add(1900))
            }
        }
        [cc, yy] if syntax.has(Syntax::CENTURY) => Some(cc.saturating_mul(100).saturating_add(yy)),
        _ => None,
    }
}

/// Upstream's `posix_time_parse`: the digits of `s` as fields, or `None` if
/// `s` is not in `syntax`. `current_year` gives the local year now, for a
/// string that names none, and is asked only for one of those.
fn parse(s: &[u8], syntax: Syntax, current_year: &dyn Fn() -> i64) -> Option<Fields> {
    // A `.` is only the seconds separator when seconds are allowed; otherwise
    // it is just a byte that is not a digit.
    let (digits, seconds) = match s.iter().position(|&b| b == b'.') {
        Some(dot) if syntax.has(Syntax::SECONDS) => {
            let (d, rest) = s.split_at(dot);
            // Exactly two bytes after the dot: upstream's `s_len - len != 3`.
            if rest.len() != 3 {
                return None;
            }
            (d, rest.get(1..))
        }
        _ => (s, None),
    };
    if !(8..=12).contains(&digits.len()) || !digits.len().is_multiple_of(2) {
        return None;
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // Whole pairs only; the length was checked even above, so nothing is left
    // over in the remainder `as_chunks` also returns.
    let pairs: Vec<i64> = digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[hi, lo]| {
            i64::from(hi.wrapping_sub(b'0'))
                .saturating_mul(10)
                .saturating_add(i64::from(lo.wrapping_sub(b'0')))
        })
        .collect();

    // The year is either the pairs before `MMDDhhmm` or the ones after it.
    let year_pairs = pairs.len().saturating_sub(4);
    let (year_at, mdhm_at): (usize, usize) = if syntax.has(Syntax::TRAILING_YEAR) {
        (4, 0)
    } else {
        (0, year_pairs)
    };
    let y = year(
        pairs.get(year_at..year_at.saturating_add(year_pairs))?,
        syntax,
        current_year,
    )?;
    let mdhm = pairs.get(mdhm_at..mdhm_at.saturating_add(4))?;
    let &[month, day, hour, minute] = mdhm else {
        return None;
    };

    let second = match seconds {
        None => 0,
        Some(&[a, b]) if a.is_ascii_digit() && b.is_ascii_digit() => {
            i64::from(a.wrapping_sub(b'0'))
                .saturating_mul(10)
                .saturating_add(i64::from(b.wrapping_sub(b'0')))
        }
        Some(_) => return None,
    };
    Some(Fields {
        year: y,
        month,
        day,
        hour,
        minute,
        second,
    })
}

/// Upstream's `posixtime`: `s` as a local time in `zone`, as an instant, or
/// `None` if it is not in `syntax` or names a time that does not exist.
///
/// `now` is passed in only to supply the current year to a string that names
/// none, so a test can pin it.
#[must_use]
pub fn posixtime(s: &[u8], syntax: Syntax, zone: &Zone, now: i64) -> Option<i64> {
    // Upstream's `year` reads it with `localtime`, which runs `tzset` -- and
    // only for a string with no year in it (see [`year`]).
    let current_year = || zone.localtime(now, 0).year;
    let mut want = parse(s, syntax, &current_year)?;
    let mut leapsec = false;
    loop {
        let mut tm1 = StructTm {
            tm_sec: i32::try_from(want.second).ok()?,
            tm_min: i32::try_from(want.minute).ok()?,
            tm_hour: i32::try_from(want.hour).ok()?,
            tm_mday: i32::try_from(want.day).ok()?,
            tm_mon: i32::try_from(want.month.checked_sub(1)?).ok()?,
            tm_year: i32::try_from(want.year.checked_sub(1900)?).ok()?,
            tm_wday: -1,
            tm_isdst: -1,
            ..StructTm::default()
        };
        // `if (tm1.tm_wday < 0) return false;`: `mktime` failed, and left
        // the fields as they were.
        let t = zone.mktime(&mut tm1)?;
        let same = i64::from(tm1.tm_year).checked_add(1900) == Some(want.year)
            && i64::from(tm1.tm_mon).checked_add(1) == Some(want.month)
            && i64::from(tm1.tm_mday) == want.day
            && i64::from(tm1.tm_hour) == want.hour
            && i64::from(tm1.tm_min) == want.minute
            && i64::from(tm1.tm_sec) == want.second;
        if same {
            return t.checked_add(i64::from(leapsec));
        }
        // Any mismatch without 60 in the seconds field is invalid; with it,
        // POSIX wants 59 and the second after it.
        if want.second != 60 {
            return None;
        }
        want.second = 59;
        leapsec = true;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::{Syntax, posixtime};
    use localtime::Zone;

    /// 2026-09-25 12:00:00 UTC, so "the current year" is 2026.
    const NOW: i64 = 1_790_337_600;

    fn at(s: &str, syntax: Syntax) -> Option<i64> {
        posixtime(s.as_bytes(), syntax, &Zone::utc(), NOW)
    }

    const TOUCH_T: Syntax = Syntax::CENTURY.with(Syntax::SECONDS);
    const OBSOLETE: Syntax = Syntax::TRAILING_YEAR.with(Syntax::PRE_2000);
    const DATE: Syntax = Syntax::TRAILING_YEAR.with(Syntax::CENTURY);

    #[test]
    fn touch_t_takes_eight_ten_or_twelve_digits_and_seconds() {
        // 2020-01-02 03:04:05 UTC.
        let t = 1_577_934_245;
        assert_eq!(at("202001020304.05", TOUCH_T), Some(t));
        assert_eq!(at("2001020304.05", TOUCH_T), Some(t));
        assert_eq!(at("202001020304", TOUCH_T), Some(t - 5));
        // No year: this year.
        assert_eq!(at("01020304", TOUCH_T), Some(1_767_323_040));
        // Odd lengths, too short, too long, stray bytes.
        for bad in [
            "2020010203",
            "0102030",
            "20200102030405",
            "2020010203x4",
            "",
        ] {
            assert_eq!(at(bad, TOUCH_T), None, "{bad:?}");
        }
        // The seconds suffix is exactly two digits.
        for bad in ["202001020304.5", "202001020304.055", "202001020304.x5"] {
            assert_eq!(at(bad, TOUCH_T), None, "{bad:?}");
        }
    }

    #[test]
    fn two_digit_years_split_at_sixty_nine() {
        assert_eq!(at("6901010000", TOUCH_T), Some(-31_536_000));
        assert_eq!(at("6801010000", TOUCH_T), Some(3_092_601_600));
        // The obsolete form refuses 00-68 and takes 69-99 after the rest.
        assert_eq!(at("0101000068", OBSOLETE), None);
        assert_eq!(at("0101000069", OBSOLETE), Some(-31_536_000));
        // ...and has no century and no seconds.
        assert_eq!(at("010100001969", OBSOLETE), None);
        assert_eq!(at("0101000069.00", OBSOLETE), None);
    }

    #[test]
    fn date_puts_a_century_year_last() {
        assert_eq!(at("010203042020", DATE), Some(1_577_934_240));
        assert_eq!(at("0102030420", DATE), Some(1_577_934_240));
    }

    #[test]
    fn a_time_that_does_not_exist_is_refused() {
        assert_eq!(at("202009310000", TOUCH_T), None, "September 31st");
        assert_eq!(at("202013010000", TOUCH_T), None, "month 13");
        assert_eq!(at("202001012500", TOUCH_T), None, "hour 25");
        assert_eq!(at("202001010061", TOUCH_T), None, "minute 61");
        assert_eq!(
            at("202002290000", TOUCH_T),
            Some(1_582_934_400),
            "a leap day"
        );
        assert_eq!(at("202102290000", TOUCH_T), None, "not a leap year");
    }

    #[test]
    fn a_time_in_a_spring_forward_gap_is_refused_and_a_repeated_one_is_either() {
        // New York's rules, so no zoneinfo tree is needed.
        let ny = Zone::resolve(
            Some(b"EST5EDT,M3.2.0,M11.1.0"),
            std::path::Path::new("/nonexistent"),
            std::path::Path::new("/nonexistent"),
        );
        let in_ny = |s: &str| posixtime(s.as_bytes(), TOUCH_T, &ny, NOW);
        // 02:30 on 2020-03-08 did not happen: `mktime` moves it and the
        // fields no longer match.
        assert_eq!(in_ny("202003080230"), None);
        // 01:30 on 2020-11-01 happened twice. Which one is glibc's search's
        // choice, from the process-wide offset -- either is a valid answer.
        let t = in_ny("202011010130").unwrap();
        assert!(t == 1_604_208_600 || t == 1_604_212_200, "{t}");
    }

    #[test]
    fn a_sixtieth_second_is_the_one_after_the_fifty_ninth() {
        let t = at("202001010000.59", TOUCH_T).unwrap();
        assert_eq!(at("202001010000.60", TOUCH_T), Some(t + 1));
        assert_eq!(at("202001010000.61", TOUCH_T), None);
    }
}
