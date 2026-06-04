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
    // month, year from system time
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let total_secs = dur.as_secs();
            let mut days = total_secs / 86400;
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
        Err(_) => (1, 2024),
    }
}
