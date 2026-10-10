//! util-linux's `lib/timeutils.c`, ported once: `parse_sec` and
//! `parse_timestamp` -- the time arguments of `cal` and of `dmesg --since`
//! and `--until`: `now`, `today`, `yesterday`, `tomorrow`, `@SECONDS`,
//! `+5min` and `-1 week ago`-style offsets, and the dates and times of its
//! format list -- with C's arithmetic, `usec_t`'s unsigned wrapping included.
//!
//! This was `cal`'s, transcribed from util-linux 2.39.3 for its timestamp
//! operand, until 2026-10-08, when `dmesg` needed the same parser.
//!
//! Time zones are `localtime`'s, which resolves `TZ` as glibc does; `mktime`
//! is glibc's normalisation through it.

use ulstrutils::{c_isspace, scan_integer};

/// A broken-down local time, in the shape `strptime` fills in and `mktime`
/// reads. Fields are allowed out of range — `mktime` normalises them, which is
/// how `cal 2024-02-31` resolves to March.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrokenDown {
    year: i64,
    /// 1..=12 nominally, but `tomorrow`/`yesterday` and `strptime` may leave it
    /// outside that range for `mktime` to normalise.
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    /// Filled in by [`mktime`]; `-1` until then.
    wday: i32,
}

/// glibc's `mktime` ([`localtime::Zone::mktime`]) on `cal`'s own
/// [`BrokenDown`], with `tm_isdst` -1 as util-linux's `parse_timestamp` sets
/// it -- so a skipped or repeated hour resolves as it does upstream, from the
/// same process-wide offset guess. `None` where `mktime` fails, and where a
/// field does not fit the `int` it is in C.
///
/// The normalised fields are written back, as `mktime` writes them, so the
/// weekday check in `parse_timestamp` sees the resolved date rather than the
/// one that was typed.
pub fn mktime(zone: &localtime::Zone, tm: &mut BrokenDown) -> Option<i64> {
    let mut stm = localtime::StructTm {
        tm_sec: i32::try_from(tm.second).ok()?,
        tm_min: i32::try_from(tm.minute).ok()?,
        tm_hour: i32::try_from(tm.hour).ok()?,
        tm_mday: i32::try_from(tm.day).ok()?,
        tm_mon: i32::try_from(tm.month.checked_sub(1)?).ok()?,
        tm_year: i32::try_from(tm.year.checked_sub(1900)?).ok()?,
        tm_wday: -1,
        tm_isdst: -1,
        ..localtime::StructTm::default()
    };
    let t = zone.mktime(&mut stm)?;
    tm.year = i64::from(stm.tm_year).saturating_add(1900);
    tm.month = i64::from(stm.tm_mon).saturating_add(1);
    tm.day = i64::from(stm.tm_mday);
    tm.hour = i64::from(stm.tm_hour);
    tm.minute = i64::from(stm.tm_min);
    tm.second = i64::from(stm.tm_sec);
    tm.wday = stm.tm_wday;
    Some(t)
}

pub const USEC_PER_SEC: u64 = 1_000_000;
pub const USEC_PER_MSEC: u64 = 1_000;
pub const USEC_PER_MINUTE: u64 = 60 * USEC_PER_SEC;
pub const USEC_PER_HOUR: u64 = 60 * USEC_PER_MINUTE;
pub const USEC_PER_DAY: u64 = 24 * USEC_PER_HOUR;
pub const USEC_PER_WEEK: u64 = 7 * USEC_PER_DAY;
/// A "month" is a *mean* month — 30.4375 days — not a calendar one, so
/// `cal +1month` in a 31-day month can land in the month after next.
pub const USEC_PER_MONTH: u64 = 2_629_800 * USEC_PER_SEC;
/// Likewise a mean Julian year, 365.25 days.
pub const USEC_PER_YEAR: u64 = 31_557_600 * USEC_PER_SEC;

/// `lib/timeutils.c`'s `WHITESPACE`, which is **not** `isspace` — no vertical
/// tab and no form feed.
const TIMEUTILS_WHITESPACE: &[u8] = b" \t\n\r";

/// `parse_sec`'s suffix table, in its order, which is what makes `m` a minute
/// and `ms` a millisecond: `months` and `month` are tried before `msec`, `ms`
/// and `m`, and `min`/`minute` before all of them.
const SEC_TABLE: &[(&str, u64)] = &[
    ("seconds", USEC_PER_SEC),
    ("second", USEC_PER_SEC),
    ("sec", USEC_PER_SEC),
    ("s", USEC_PER_SEC),
    ("minutes", USEC_PER_MINUTE),
    ("minute", USEC_PER_MINUTE),
    ("min", USEC_PER_MINUTE),
    ("months", USEC_PER_MONTH),
    ("month", USEC_PER_MONTH),
    ("msec", USEC_PER_MSEC),
    ("ms", USEC_PER_MSEC),
    ("m", USEC_PER_MINUTE),
    ("hours", USEC_PER_HOUR),
    ("hour", USEC_PER_HOUR),
    ("hr", USEC_PER_HOUR),
    ("h", USEC_PER_HOUR),
    ("days", USEC_PER_DAY),
    ("day", USEC_PER_DAY),
    ("d", USEC_PER_DAY),
    ("weeks", USEC_PER_WEEK),
    ("week", USEC_PER_WEEK),
    ("w", USEC_PER_WEEK),
    ("years", USEC_PER_YEAR),
    ("year", USEC_PER_YEAR),
    ("y", USEC_PER_YEAR),
    ("usec", 1),
    ("us", 1),
    // The empty suffix is last, so a bare number is seconds.
    ("", USEC_PER_SEC),
];

/// `parse_sec`: `+90min`, `2 days`, `1h30m`, `1.5w`.
///
/// Several terms may be concatenated and are summed. Returns micro-seconds, or
/// `None` for anything the C returns a negative errno for — `cal` treats every
/// failure the same way, so the distinction between `EINVAL` and `ERANGE` is
/// not carried.
pub fn parse_sec(t: &[u8]) -> Option<u64> {
    let mut r: u64 = 0;
    let mut something = false;
    let mut p = 0usize;

    loop {
        while t.get(p).is_some_and(|c| TIMEUTILS_WHITESPACE.contains(c)) {
            p = p.saturating_add(1);
        }
        if p >= t.len() {
            return if something { Some(r) } else { None };
        }

        // `strtoll(p, &e, 10)`, whose sign is accepted and then refused.
        let scanned = scan_integer(t.get(p..).unwrap_or_default(), 10);
        let (l, mut e) = match &scanned {
            Some(sc) => {
                if sc.saturated {
                    return None; // ERANGE
                }
                if sc.negative && sc.magnitude != 0 {
                    return None; // ERANGE
                }
                (u64::try_from(sc.magnitude).ok()?, p.saturating_add(sc.end))
            }
            None => (0u64, p),
        };

        let mut z: u64 = 0;
        let mut n = 0usize;
        if t.get(e) == Some(&b'.') {
            let b = e.saturating_add(1);
            let frac = scan_integer(t.get(b..).unwrap_or_default(), 10)?;
            if frac.saturated || frac.negative {
                return None;
            }
            z = u64::try_from(frac.magnitude).ok()?;
            e = b.saturating_add(frac.end);
            n = e.saturating_sub(b);
        } else if e == p {
            return None; // no digits and no decimal point
        }

        while t.get(e).is_some_and(|c| TIMEUTILS_WHITESPACE.contains(c)) {
            e = e.saturating_add(1);
        }

        let mut matched = false;
        for (suffix, usec) in SEC_TABLE {
            if !t
                .get(e..)
                .unwrap_or_default()
                .starts_with(suffix.as_bytes())
            {
                continue;
            }
            let mut k = z.saturating_mul(*usec);
            for _ in 0..n {
                k /= 10;
            }
            r = r.saturating_add(l.saturating_mul(*usec)).saturating_add(k);
            p = e.saturating_add(suffix.len());
            something = true;
            matched = true;
            break;
        }
        if !matched {
            return None;
        }
    }
}

/// `parse_subseconds`: the `.12` of `2012-09-22 16:34:22.12`, or the `,5` of an
/// ISO-8601 one. A bare `.` is accepted and worth nothing.
fn parse_subseconds(t: &[u8]) -> Option<u64> {
    if t.first() != Some(&b'.') && t.first() != Some(&b',') {
        return None;
    }
    let mut ret: u64 = 0;
    let mut factor: u64 = USEC_PER_SEC / 10;
    for &c in t.get(1..).unwrap_or_default() {
        if !c.is_ascii_digit() || factor < 1 {
            return None;
        }
        ret = ret.saturating_add(u64::from(c.saturating_sub(b'0')).saturating_mul(factor));
        factor /= 10;
    }
    Some(ret)
}

/// glibc's `get_number` macro: skip *spaces* (not all whitespace), then read at
/// most `n` digits, stopping early once another digit could not fit under `to`.
fn get_number(s: &[u8], rp: &mut usize, from: i64, to: i64, n: u32) -> Option<i64> {
    while s.get(*rp) == Some(&b' ') {
        *rp = (*rp).saturating_add(1);
    }
    if !s.get(*rp).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut val: i64 = 0;
    let mut left = n;
    loop {
        let d = i64::from(s.get(*rp)?.wrapping_sub(b'0'));
        val = val.checked_mul(10)?.checked_add(d)?;
        *rp = (*rp).saturating_add(1);
        left = left.saturating_sub(1);
        if !(left > 0
            && val.checked_mul(10).is_some_and(|v| v <= to)
            && s.get(*rp).is_some_and(u8::is_ascii_digit))
        {
            break;
        }
    }
    if val < from || val > to {
        return None;
    }
    Some(val)
}

/// The subset of `strptime` `parse_timestamp` uses: `%y %Y %m %d %H %M %S %s`,
/// literal bytes, and whitespace in the format matching any run of whitespace
/// in the input — including none.
///
/// Returns the index one past the last byte consumed, which is what C's
/// `endptr` return says, and `None` where C returns `NULL`.
fn strptime(s: &[u8], fmt: &str, tm: &mut BrokenDown, zone: &localtime::Zone) -> Option<usize> {
    let fmt = fmt.as_bytes();
    let mut rp = 0usize;
    let mut fi = 0usize;

    while fi < fmt.len() {
        let Some(&fc) = fmt.get(fi) else {
            break;
        };
        if c_isspace(fc) {
            while s.get(rp).is_some_and(|c| c_isspace(*c)) {
                rp = rp.saturating_add(1);
            }
            fi = fi.saturating_add(1);
            continue;
        }
        if fc != b'%' {
            if s.get(rp) != Some(&fc) {
                return None;
            }
            rp = rp.saturating_add(1);
            fi = fi.saturating_add(1);
            continue;
        }
        fi = fi.saturating_add(1);
        let spec = *fmt.get(fi)?;
        fi = fi.saturating_add(1);

        if spec == b's' {
            // Seconds since the epoch. Deliberately *not* `get_number`: the
            // value may be far larger than any field, and no sign is accepted,
            // which is why `cal @-1` is refused.
            if !s.get(rp).is_some_and(u8::is_ascii_digit) {
                return None;
            }
            let mut secs: i64 = 0;
            while let Some(&c) = s.get(rp) {
                if !c.is_ascii_digit() {
                    break;
                }
                secs = secs
                    .saturating_mul(10)
                    .saturating_add(i64::from(c.saturating_sub(b'0')));
                rp = rp.saturating_add(1);
            }
            *tm = broken_down(zone, secs);
            continue;
        }

        let (from, to, digits) = match spec {
            b'Y' => (0i64, 9999i64, 4u32),
            b'y' => (0, 99, 2),
            b'm' => (1, 12, 2),
            b'd' => (1, 31, 2),
            b'H' => (0, 23, 2),
            b'M' => (0, 59, 2),
            // 61, for the two leap seconds POSIX once allowed.
            b'S' => (0, 61, 2),
            _ => return None,
        };
        let val = get_number(s, &mut rp, from, to, digits)?;
        match spec {
            b'Y' => tm.year = val,
            // "The Year 2000: The Millennium Rollover" paper's rule, which glibc
            // follows: 69..99 is the twentieth century, 00..68 the twenty-first.
            b'y' => {
                tm.year = if val >= 69 {
                    val.saturating_add(1900)
                } else {
                    val.saturating_add(2000)
                }
            }
            b'm' => tm.month = val,
            b'd' => tm.day = val,
            b'H' => tm.hour = val,
            b'M' => tm.minute = val,
            b'S' => tm.second = val,
            _ => return None,
        }
    }
    Some(rp)
}

/// `localtime_r` into the shape [`strptime`] and [`mktime`] share.
pub fn broken_down(zone: &localtime::Zone, t: i64) -> BrokenDown {
    let tm = zone.local(t, 0);
    BrokenDown {
        year: tm.year,
        month: i64::from(tm.month),
        day: i64::from(tm.day),
        hour: i64::from(tm.hour),
        minute: i64::from(tm.minute),
        second: i64::from(tm.second),
        wday: i32::try_from(tm.wday).unwrap_or(0),
    }
}

/// What a format does to the fields it did not set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum After {
    /// Leave them — the format set everything it was going to.
    Keep,
    /// A date and an hour and minute: seconds become zero.
    ZeroSec,
    /// A bare date: the whole time becomes midnight.
    ZeroTime,
}

/// `parse_timestamp`'s format list, in its order, with the two things the C
/// expresses by repetition: what to zero afterwards, and whether the format is
/// one of the four marked `!` in `timeutils.c`'s comment — the ones that also
/// accept up to six digits of subsecond granularity.
///
/// The order matters at one point in particular: `%y-%m-%d …` is tried *before*
/// `%Y-%m-%d …`, so `24-02-15` is 2024 rather than the year 24.
const TIMESTAMP_FORMATS: &[(&str, After, bool)] = &[
    ("%y-%m-%d %H:%M:%S", After::Keep, true),
    ("%Y-%m-%d %H:%M:%S", After::Keep, true),
    ("%Y-%m-%dT%H:%M:%S", After::Keep, true),
    ("%y-%m-%d %H:%M", After::ZeroSec, false),
    ("%Y-%m-%d %H:%M", After::ZeroSec, false),
    ("%y-%m-%d", After::ZeroTime, false),
    ("%Y-%m-%d", After::ZeroTime, false),
    ("%H:%M:%S", After::Keep, true),
    ("%H:%M", After::ZeroSec, false),
    ("%Y%m%d%H%M%S", After::Keep, true),
];

/// `parse_timestamp`'s weekday table, in its order: each full name immediately
/// before its abbreviation, so `Sunday 2024-02-18` and `Sun 2024-02-18` both
/// resolve and `Sundae` resolves to neither.
const DAY_NR: &[(&str, i32)] = &[
    ("Sunday", 0),
    ("Sun", 0),
    ("Monday", 1),
    ("Mon", 1),
    ("Tuesday", 2),
    ("Tue", 2),
    ("Wednesday", 3),
    ("Wed", 3),
    ("Thursday", 4),
    ("Thu", 4),
    ("Friday", 5),
    ("Fri", 5),
    ("Saturday", 6),
    ("Sat", 6),
];

fn starts_with_no_case(s: &[u8], prefix: &str) -> bool {
    let p = prefix.as_bytes();
    s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p))
}

/// `parse_timestamp_reference`: everything `cal <timestamp>` accepts.
///
/// The result is micro-seconds in util-linux's `usec_t`, which is **unsigned**.
/// A date before 1970 therefore comes back as an enormous positive number
/// rather than a negative one, and `cal` divides it by a million and asks
/// `localtime` about the result — so `cal 1960-01-01` names a year in the far
/// future. That is upstream's arithmetic, wrapping and all, and it is
/// reproduced rather than corrected.
pub fn parse_timestamp(zone: &localtime::Zone, reference: i64, t: &[u8]) -> Option<u64> {
    let mut tm = broken_down(zone, reference);
    let mut plus: u64 = 0;
    let mut minus: u64 = 0;
    let mut ret: u64 = 0;
    let mut weekday: i32 = -1;

    if t == b"now" {
        // Nothing to adjust.
    } else if t == b"today" {
        tm.second = 0;
        tm.minute = 0;
        tm.hour = 0;
    } else if t == b"yesterday" {
        tm.day = tm.day.saturating_sub(1);
        tm.second = 0;
        tm.minute = 0;
        tm.hour = 0;
    } else if t == b"tomorrow" {
        tm.day = tm.day.saturating_add(1);
        tm.second = 0;
        tm.minute = 0;
        tm.hour = 0;
    } else if t.first() == Some(&b'+') {
        plus = parse_sec(t.get(1..)?)?;
    } else if t.first() == Some(&b'-') {
        minus = parse_sec(t.get(1..)?)?;
    } else if t.first() == Some(&b'@') {
        let k = strptime(t.get(1..)?, "%s", &mut tm, zone)?;
        let rest = t.get(1usize.saturating_add(k)..)?;
        if !rest.is_empty() {
            ret = parse_subseconds(rest)?;
        }
    } else if t.ends_with(b" ago") {
        minus = parse_sec(t.len().checked_sub(4).and_then(|n| t.get(..n))?)?;
    } else {
        let mut s = t;
        for (name, nr) in DAY_NR {
            if !starts_with_no_case(s, name) {
                continue;
            }
            if s.get(name.len()) != Some(&b' ') {
                continue;
            }
            weekday = *nr;
            s = s.get(name.len().saturating_add(1)..).unwrap_or_default();
            break;
        }

        let copy = tm;
        let mut matched = false;
        for (fmt, after, subsec) in TIMESTAMP_FORMATS {
            tm = copy;
            let Some(k) = strptime(s, fmt, &mut tm, zone) else {
                continue;
            };
            let Some(rest) = s.get(k..) else {
                continue;
            };
            if rest.is_empty() {
                match after {
                    After::Keep => {}
                    After::ZeroSec => tm.second = 0,
                    After::ZeroTime => {
                        tm.second = 0;
                        tm.minute = 0;
                        tm.hour = 0;
                    }
                }
                matched = true;
                break;
            }
            if *subsec && let Some(sub) = parse_subseconds(rest) {
                ret = sub;
                matched = true;
                break;
            }
        }
        if !matched {
            return None;
        }
    }

    // `if (x == (time_t) -1) return -EINVAL;` -- which refuses the second
    // before the epoch along with a failure, as upstream's does.
    let x = mktime(zone, &mut tm).filter(|&x| x != -1)?;
    if weekday >= 0 && tm.wday != weekday {
        return None;
    }
    ret = ret.wrapping_add((x as u64).wrapping_mul(USEC_PER_SEC));
    ret = ret.wrapping_add(plus);
    Some(ret.saturating_sub(minus))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// 2026-08-27 00:00:00 UTC, the reference the readings below are relative to.
    const NOW: i64 = 1_787_788_800;

    #[test]
    fn timestamps_are_read_the_way_parse_timestamp_reads_them() {
        let utc = localtime::Zone::utc();
        let at = |s: &[u8]| parse_timestamp(&utc, NOW, s);
        let secs = |t: i64| Some(u64::try_from(t).expect("positive") * USEC_PER_SEC);

        assert_eq!(at(b"2024-02-15"), secs(1_707_955_200));
        assert_eq!(at(b"24-02-15"), secs(1_707_955_200));
        // The subsecond part is kept, not rounded away: `parse_subseconds`
        // accumulates into the same `usec_t` the seconds land in.
        assert_eq!(
            at(b"2024-02-15 10:00:00.123"),
            Some(1_707_991_200 * USEC_PER_SEC + 123_000)
        );
        assert_eq!(
            at(b"2024-02-15T10:00:00,5"),
            Some(1_707_991_200 * USEC_PER_SEC + 500_000)
        );
        assert_eq!(at(b"Sat 2024-02-17"), secs(1_708_128_000));
        assert_eq!(at(b"@0"), Some(0));
        assert_eq!(at(b"@1"), Some(USEC_PER_SEC));
        assert_eq!(at(b"now"), secs(NOW));
        assert_eq!(at(b"+1day"), secs(NOW + 86_400));
        assert_eq!(at(b"2 days ago"), secs(NOW - 2 * 86_400));
        // `mktime` normalises, so the 31st of February is the 2nd of March.
        assert_eq!(at(b"2024-02-31"), secs(1_709_337_600));

        // Rejected: a negative epoch, a bare sign, the wrong case, a trailing
        // space, an impossible month, and a weekday that contradicts the date.
        assert_eq!(at(b"@-1"), None);
        assert_eq!(at(b"@"), None);
        assert_eq!(at(b"@abc"), None);
        assert_eq!(at(b"+"), None);
        assert_eq!(at(b"NOW"), None);
        assert_eq!(at(b"now "), None);
        assert_eq!(at(b"2024-13-01"), None);
        assert_eq!(at(b"99:99"), None);
        assert_eq!(at(b"Mon 2024-02-17"), None);
        assert_eq!(at(b"abc"), None);
    }

    #[test]
    fn durations_sum_their_terms_by_upstreams_suffix_table() {
        let s = |n: u64| Some(n * USEC_PER_SEC);
        assert_eq!(parse_sec(b"90"), s(90));
        assert_eq!(parse_sec(b"1h30m"), s(5400));
        assert_eq!(parse_sec(b"2 days"), s(2 * 86_400));
        // The table's order: `m` is a minute, `ms` a millisecond, and
        // `months` and `month` are tried before both.
        assert_eq!(parse_sec(b"1m"), s(60));
        assert_eq!(parse_sec(b"1ms"), Some(USEC_PER_MSEC));
        assert_eq!(parse_sec(b"1month"), Some(USEC_PER_MONTH));
        assert_eq!(parse_sec(b"1min"), s(60));
        assert_eq!(parse_sec(b"5us"), Some(5));
        // A fraction scales its own unit.
        assert_eq!(parse_sec(b"1.5h"), s(5400));
        assert_eq!(parse_sec(b"0.5s"), Some(USEC_PER_SEC / 2));
        // Nothing, or no number, is not a duration.
        assert_eq!(parse_sec(b""), None);
        assert_eq!(parse_sec(b"h"), None);
        assert_eq!(parse_sec(b"5 furlongs"), None);
        assert_eq!(parse_sec(b"-5"), None);
    }
}
