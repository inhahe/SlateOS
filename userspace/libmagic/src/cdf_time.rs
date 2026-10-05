//! libmagic's `cdf_time.c`: Windows `FILETIME`s -- hundreds of nanoseconds
//! since 1601 -- as Unix times, the way upstream converts them.
//!
//! Which is approximately: the year is the day count over 365, and the
//! calendar fields are then handed to `mktime` -- in the *local* time zone --
//! so the answer moves with `TZ` and can be a day off near a year's end. Both
//! are upstream's, and a `qwdate` rule prints what they give.

use localtime::Zone;
use localtime::StructTm;

/// `CDF_BASE_YEAR`.
pub const CDF_BASE_YEAR: i32 = 1601;
/// `CDF_TIME_PREC`: ticks per second.
const CDF_TIME_PREC: i64 = 10_000_000;

/// `MAX_CTIME`: 9999-12-31 23:59:59, the last instant `ctime` is asked about.
pub const MAX_CTIME: i64 = 0x3a_fff4_87cf;

const MDAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn isleap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// `cdf_getdays`: days from 1601-01-01 to January 1st of `year`.
fn cdf_getdays(year: i32) -> i32 {
    let mut days: i32 = 0;
    let mut y = CDF_BASE_YEAR;
    while y < year {
        days = days.wrapping_add(i32::from(isleap(y)) + 365);
        y += 1;
    }
    days
}

/// `cdf_getday`: the day within its month.
fn cdf_getday(year: i32, mut days: i32) -> i32 {
    for (m, &md) in MDAYS.iter().enumerate() {
        let sub = md + i32::from(m == 1 && isleap(year));
        if days < sub {
            return days;
        }
        days = days.wrapping_sub(sub);
    }
    days
}

/// `cdf_getmonth`: the month, 0 to 11 -- or 12 past December.
fn cdf_getmonth(year: i32, mut days: i32) -> i32 {
    for (m, &md) in MDAYS.iter().enumerate() {
        days = days.wrapping_sub(md);
        if m == 1 && isleap(year) {
            days = days.wrapping_sub(1);
        }
        if days <= 0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            return m as i32;
        }
    }
    12
}

thread_local! {
    /// The local zone, resolved once, as glibc's `tzset` is.
    static LOCAL: Zone = Zone::from_env();
}

/// Run `f` with the process's local zone.
pub fn with_local<R>(f: impl FnOnce(&Zone) -> R) -> R {
    LOCAL.with(f)
}

/// `cdf_timestamp_to_timespec`: the seconds of a `FILETIME`, or `None` where
/// `mktime` fails (C's -1, `EINVAL`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn cdf_timestamp_to_timespec(t: i64) -> Option<i64> {
    let mut t = t / CDF_TIME_PREC;
    let mut tm = StructTm {
        tm_sec: (t % 60) as i32,
        ..StructTm::default()
    };
    t /= 60;
    tm.tm_min = (t % 60) as i32;
    t /= 60;
    tm.tm_hour = (t % 24) as i32;
    t /= 24;
    // "XXX: Approx".
    tm.tm_year = (i64::from(CDF_BASE_YEAR) + t / 365) as i32;
    let rdays = cdf_getdays(tm.tm_year);
    t -= i64::from(rdays) - 1;
    tm.tm_mday = cdf_getday(tm.tm_year, t as i32);
    tm.tm_mon = cdf_getmonth(tm.tm_year, t as i32);
    tm.tm_year = tm.tm_year.wrapping_sub(1900);
    with_local(|z| z.mktime(&mut tm))
}

/// `cdf_ctime`: `ctime_r`, or `*Bad* 0x...` for an instant past `MAX_CTIME`
/// or one `ctime_r` refuses.
#[must_use]
pub fn cdf_ctime(sec: i64) -> Vec<u8> {
    if sec <= MAX_CTIME {
        if let Some(tm) = with_local(|z| z.localtime_r(sec)) {
            if let Some(s) = asctime(&tm) {
                return s;
            }
        }
    }
    // `snprintf(buf, 26, "*Bad* %#16.16llx\n", sec)`.
    #[allow(clippy::cast_sign_loss)]
    let hex = crate::printf::format(b"*Bad* %#16.16llx\n", &[crate::printf::Arg::I64(sec as u64)])
        .unwrap_or_default();
    hex.into_iter().take(25).collect()
}

/// `asctime_r`: `Sun Sep 16 01:03:52 1973\n`, or `None` where glibc refuses --
/// a year past `INT_MAX - 1900`, or a line that would not fit its 26 bytes.
#[must_use]
pub fn asctime(tm: &StructTm) -> Option<Vec<u8>> {
    if tm.tm_year > i32::MAX - 1900 {
        return None;
    }
    let wday: &[u8] = usize::try_from(tm.tm_wday)
        .ok()
        .and_then(|w| localtime::WDAY_ABBR.get(w).copied())
        .unwrap_or(b"???");
    let mon: &[u8] = usize::try_from(tm.tm_mon)
        .ok()
        .and_then(|m| localtime::MON_ABBR.get(m).copied())
        .unwrap_or(b"???");
    let mut s = Vec::with_capacity(26);
    s.extend_from_slice(wday);
    s.push(b' ');
    s.extend_from_slice(mon);
    s.extend_from_slice(
        format!(
            "{:3} {:02}:{:02}:{:02} {}\n",
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            i64::from(tm.tm_year) + 1900
        )
        .as_bytes(),
    );
    if s.len() >= 26 {
        return None;
    }
    Some(s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_helpers_are_upstreams() {
        assert_eq!(cdf_getdays(1601), 0);
        assert_eq!(cdf_getdays(1602), 365);
        assert_eq!(cdf_getdays(1605), 365 * 4 + 1);
        assert_eq!(cdf_getday(1977, 40), 9);
        assert_eq!(cdf_getmonth(1977, 40), 1);
    }

    #[test]
    fn asctime_writes_and_refuses_as_glibc_does() {
        let tm = StructTm {
            tm_sec: 52,
            tm_min: 3,
            tm_hour: 1,
            tm_mday: 16,
            tm_mon: 8,
            tm_year: 73,
            tm_wday: 0,
            ..StructTm::default()
        };
        assert_eq!(asctime(&tm).unwrap(), b"Sun Sep 16 01:03:52 1973\n");
        let far = StructTm { tm_year: 10000 - 1900, ..tm };
        assert_eq!(asctime(&far), None);
        let odd = StructTm { tm_mon: 12, tm_wday: -1, ..tm };
        assert_eq!(asctime(&odd).unwrap(), b"??? ??? 16 01:03:52 1973\n");
    }
}
