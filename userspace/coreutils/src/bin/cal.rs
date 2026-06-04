//! cal — display a calendar.
//!
//! Usage: cal [MONTH YEAR]
//!        cal [YEAR]
//!   Without arguments: show current month.
//!   With YEAR only: show all 12 months.

use std::env;
use std::time::SystemTime;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let (month, year, show_all) = if args.is_empty() {
        // Current month/year
        let now = current_date();
        (now.0, now.1, false)
    } else if args.len() == 1 {
        // Might be just a year
        let y: u32 = args[0].parse().unwrap_or(2024);
        if y > 12 {
            (1, y, true) // Show whole year
        } else {
            let now = current_date();
            (y, now.1, false) // Month of current year
        }
    } else {
        let m: u32 = args[0].parse().unwrap_or(1);
        let y: u32 = args[1].parse().unwrap_or(2024);
        (m, y, false)
    };

    if show_all {
        println!("                            {year}");
        println!();
        for m in 1..=12 {
            print_month(m, year);
            println!();
        }
    } else {
        print_month(month, year);
    }
}

fn print_month(month: u32, year: u32) {
    let month_names = [
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

    let name = if (1..=12).contains(&month) {
        month_names[(month - 1) as usize]
    } else {
        "Unknown"
    };

    let header = format!("{name} {year}");
    let pad = (20usize.saturating_sub(header.len())) / 2;
    println!("{:>pad$}{header}", "");
    println!("Su Mo Tu We Th Fr Sa");

    let days = days_in_month(month, year);
    let start_dow = day_of_week(year, month, 1); // 0=Sun

    // Print leading spaces
    for _ in 0..start_dow {
        print!("   ");
    }

    for day in 1..=days {
        print!("{day:>2} ");
        if (start_dow + day).is_multiple_of(7) {
            println!();
        }
    }

    if !(start_dow + days).is_multiple_of(7) {
        println!();
    }
}

fn days_in_month(month: u32, year: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn is_leap(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

/// Zeller's congruence — returns day of week (0=Sunday).
fn day_of_week(year: u32, month: u32, day: u32) -> u32 {
    let (y, m) = if month <= 2 {
        (year as i32 - 1, month as i32 + 12)
    } else {
        (year as i32, month as i32)
    };

    let q = day as i32;
    let k = y % 100;
    let j = y / 100;

    let h = (q + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 - 2 * j) % 7;
    // h: 0=Sat, 1=Sun, ... 6=Fri → convert to 0=Sun
    ((h + 6) % 7) as u32
}

fn current_date() -> (u32, u32) {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let total_secs = dur.as_secs();
            let days = total_secs / 86400;
            unix_days_to_month_year(days)
        }
        Err(_) => (1, 2024),
    }
}

/// Convert a count of days since the Unix epoch (1970-01-01) to a
/// (month, year) pair. Pure helper — unit-testable independently of
/// `SystemTime::now`.
fn unix_days_to_month_year(mut days: u64) -> (u32, u32) {
    let mut year: u32 = 1970;
    loop {
        let diy = if is_leap(year) { 366u64 } else { 365 };
        if days < diy {
            break;
        }
        days -= diy;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month: u32 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (month, year)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- is_leap ----------------

    #[test]
    fn is_leap_div_by_4_not_100() {
        assert!(is_leap(2024));
        assert!(is_leap(2020));
        assert!(is_leap(1996));
    }

    #[test]
    fn is_leap_div_by_400() {
        assert!(is_leap(2000));
        assert!(is_leap(1600));
        assert!(is_leap(2400));
    }

    #[test]
    fn is_leap_div_by_100_not_400() {
        assert!(!is_leap(1900));
        assert!(!is_leap(2100));
        assert!(!is_leap(2200));
        assert!(!is_leap(2300));
    }

    #[test]
    fn is_leap_not_div_by_4() {
        assert!(!is_leap(2023));
        assert!(!is_leap(2021));
        assert!(!is_leap(2025));
    }

    // ---------------- days_in_month ----------------

    #[test]
    fn days_in_31_day_months() {
        for &m in &[1u32, 3, 5, 7, 8, 10, 12] {
            assert_eq!(days_in_month(m, 2024), 31, "month {m}");
        }
    }

    #[test]
    fn days_in_30_day_months() {
        for &m in &[4u32, 6, 9, 11] {
            assert_eq!(days_in_month(m, 2024), 30, "month {m}");
        }
    }

    #[test]
    fn days_in_february_leap() {
        assert_eq!(days_in_month(2, 2024), 29);
        assert_eq!(days_in_month(2, 2000), 29);
    }

    #[test]
    fn days_in_february_non_leap() {
        assert_eq!(days_in_month(2, 2023), 28);
        assert_eq!(days_in_month(2, 1900), 28);
    }

    #[test]
    fn days_in_invalid_month_returns_fallback() {
        // Implementation returns 30 for unknown months.
        assert_eq!(days_in_month(0, 2024), 30);
        assert_eq!(days_in_month(13, 2024), 30);
    }

    // ---------------- day_of_week (Zeller's congruence, 0=Sunday) ----------------

    #[test]
    fn day_of_week_known_dates() {
        // 1970-01-01 was a Thursday (4 in 0=Sun..6=Sat).
        assert_eq!(day_of_week(1970, 1, 1), 4);
        // 2000-01-01 was a Saturday (6).
        assert_eq!(day_of_week(2000, 1, 1), 6);
        // 2024-01-01 was a Monday (1).
        assert_eq!(day_of_week(2024, 1, 1), 1);
        // 2024-02-29 (leap) was a Thursday (4).
        assert_eq!(day_of_week(2024, 2, 29), 4);
        // 2024-12-25 was a Wednesday (3).
        assert_eq!(day_of_week(2024, 12, 25), 3);
    }

    #[test]
    fn day_of_week_january_and_february_use_previous_year() {
        // Zeller's adjusts Jan/Feb as months 13/14 of the previous year.
        // Verify Feb 1 2024 was a Thursday (4).
        assert_eq!(day_of_week(2024, 2, 1), 4);
    }

    // ---------------- unix_days_to_month_year ----------------

    #[test]
    fn unix_day_zero_is_jan_1970() {
        assert_eq!(unix_days_to_month_year(0), (1, 1970));
    }

    #[test]
    fn unix_day_31_is_feb_1970() {
        // Day 0..30 = Jan 1970; day 31 = Feb 1, 1970.
        assert_eq!(unix_days_to_month_year(31), (2, 1970));
    }

    #[test]
    fn unix_day_365_is_jan_1971() {
        // 1970 has 365 days; day 365 is Jan 1, 1971.
        assert_eq!(unix_days_to_month_year(365), (1, 1971));
    }

    #[test]
    fn unix_day_late_feb_2024_handles_leap() {
        // Days from 1970-01-01 to 2024-02-29:
        // 54 years from 1970 to 2024 — count leap years in [1970, 2024):
        // 1972, 76, 80, 84, 88, 92, 96, 2000, 04, 08, 12, 16, 20 = 13 leap.
        // Days = 54*365 + 13 = 19723. Jan = 31 -> Feb 1 = day 19754. Feb 29
        // = day 19754 + 28 = 19782.
        assert_eq!(unix_days_to_month_year(19782), (2, 2024));
    }
}
