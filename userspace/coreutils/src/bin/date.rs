//! date — display the current date and time.
//!
//! Usage: date
//!   Prints the current UTC date and time in a simple format.
//!   (No timezone support yet — always UTC.)

use std::time::SystemTime;

/// A broken-down date/time in UTC.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DateTime {
    year: u64,
    month: usize, // 0..=11
    day: u64,     // 1..=31
    hour: u64,
    minute: u64,
    second: u64,
    dow: usize, // 0=Sun..6=Sat
}

fn main() {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let dt = unix_secs_to_datetime(dur.as_secs());
            println!("{}", format_datetime(&dt));
        }
        Err(_) => {
            println!("date: unable to determine current time");
        }
    }
}

/// Compute a broken-down UTC date/time from seconds since the Unix epoch.
fn unix_secs_to_datetime(total_secs: u64) -> DateTime {
    let second = total_secs % 60;
    let minute = (total_secs / 60) % 60;
    let hour = (total_secs / 3600) % 24;
    let mut days = total_secs / 86400;

    let mut year: u64 = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days = days.saturating_sub(days_in_year);
        year = year.saturating_add(1);
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i;
            break;
        }
        days = days.saturating_sub(md);
    }
    let day = days.saturating_add(1);

    // Jan 1 1970 was Thursday (4 in Sun=0..Sat=6).
    let dow = ((total_secs / 86400).saturating_add(4) % 7) as usize;

    DateTime { year, month, day, hour, minute, second, dow }
}

/// Format a `DateTime` like `Thu Jan  1 00:00:00 UTC 1970`.
fn format_datetime(dt: &DateTime) -> String {
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const DOW_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let dow = DOW_NAMES.get(dt.dow).copied().unwrap_or("???");
    let mon = MONTH_NAMES.get(dt.month).copied().unwrap_or("???");
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} UTC {}",
        dow, mon, dt.day, dt.hour, dt.minute, dt.second, dt.year
    )
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn leap_year_basic() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
        assert!(!is_leap(2100));
        assert!(is_leap(2400));
    }

    #[test]
    fn epoch_is_thursday_jan_1_1970() {
        let dt = unix_secs_to_datetime(0);
        assert_eq!(dt.year, 1970);
        assert_eq!(dt.month, 0);
        assert_eq!(dt.day, 1);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.second, 0);
        assert_eq!(dt.dow, 4); // Thursday
    }

    #[test]
    fn one_second_after_epoch() {
        let dt = unix_secs_to_datetime(1);
        assert_eq!(dt.second, 1);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.day, 1);
    }

    #[test]
    fn one_minute_after_epoch() {
        let dt = unix_secs_to_datetime(60);
        assert_eq!(dt.second, 0);
        assert_eq!(dt.minute, 1);
    }

    #[test]
    fn one_hour_after_epoch() {
        let dt = unix_secs_to_datetime(3600);
        assert_eq!(dt.hour, 1);
        assert_eq!(dt.minute, 0);
    }

    #[test]
    fn one_day_after_epoch_is_friday() {
        let dt = unix_secs_to_datetime(86400);
        assert_eq!(dt.year, 1970);
        assert_eq!(dt.month, 0);
        assert_eq!(dt.day, 2);
        assert_eq!(dt.dow, 5); // Friday
    }

    #[test]
    fn end_of_january_1970() {
        // Jan 31, 1970 00:00:00 UTC = 30 days after epoch.
        let dt = unix_secs_to_datetime(30 * 86400);
        assert_eq!(dt.month, 0);
        assert_eq!(dt.day, 31);
    }

    #[test]
    fn start_of_february_1970() {
        let dt = unix_secs_to_datetime(31 * 86400);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 1);
    }

    #[test]
    fn leap_day_2000() {
        // Feb 29, 2000 00:00:00 UTC = 951782400.
        let dt = unix_secs_to_datetime(951_782_400);
        assert_eq!(dt.year, 2000);
        assert_eq!(dt.month, 1); // Feb
        assert_eq!(dt.day, 29);
    }

    #[test]
    fn mar_1_2000() {
        // Mar 1, 2000 00:00:00 UTC = 951868800.
        let dt = unix_secs_to_datetime(951_868_800);
        assert_eq!(dt.year, 2000);
        assert_eq!(dt.month, 2); // Mar
        assert_eq!(dt.day, 1);
    }

    #[test]
    fn non_leap_century_1900_not_applicable_but_2100_via_arith() {
        // 1900 is before epoch; just sanity-check is_leap.
        assert!(!is_leap(2100));
    }

    #[test]
    fn known_datetime_2024_06_15_12_30_45() {
        // 2024-06-15 12:30:45 UTC = 1718454645.
        let dt = unix_secs_to_datetime(1_718_454_645);
        assert_eq!(dt.year, 2024);
        assert_eq!(dt.month, 5); // June (0-indexed)
        assert_eq!(dt.day, 15);
        assert_eq!(dt.hour, 12);
        assert_eq!(dt.minute, 30);
        assert_eq!(dt.second, 45);
    }

    #[test]
    fn dow_cycles_correctly() {
        // Day 0: Thu, Day 1: Fri, Day 2: Sat, Day 3: Sun, Day 4: Mon, ...
        assert_eq!(unix_secs_to_datetime(0).dow, 4);
        assert_eq!(unix_secs_to_datetime(86_400).dow, 5);
        assert_eq!(unix_secs_to_datetime(2 * 86_400).dow, 6);
        assert_eq!(unix_secs_to_datetime(3 * 86_400).dow, 0);
        assert_eq!(unix_secs_to_datetime(4 * 86_400).dow, 1);
        assert_eq!(unix_secs_to_datetime(7 * 86_400).dow, 4); // back to Thu
    }

    #[test]
    fn format_epoch() {
        let dt = unix_secs_to_datetime(0);
        assert_eq!(format_datetime(&dt), "Thu Jan  1 00:00:00 UTC 1970");
    }

    #[test]
    fn format_known_2024() {
        // 2024-06-15 12:30:45 UTC — June 15, 2024 was a Saturday.
        let dt = unix_secs_to_datetime(1_718_454_645);
        assert_eq!(format_datetime(&dt), "Sat Jun 15 12:30:45 UTC 2024");
    }

    #[test]
    fn format_pads_single_digit_day_with_space() {
        // Jan 2 1970.
        let dt = unix_secs_to_datetime(86_400);
        assert!(format_datetime(&dt).contains(" 2 "));
    }

    #[test]
    fn format_pads_time_with_zeros() {
        // 1 second after epoch -> 00:00:01.
        let dt = unix_secs_to_datetime(1);
        assert!(format_datetime(&dt).contains("00:00:01"));
    }
}
