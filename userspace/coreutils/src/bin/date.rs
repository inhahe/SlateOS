//! date — display the current date and time.
//!
//! Usage: date
//!   Prints the current UTC date and time in a simple format.
//!   (No timezone support yet — always UTC.)

use std::time::SystemTime;

fn main() {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let total_secs = dur.as_secs();
            let secs = total_secs % 60;
            let mins = (total_secs / 60) % 60;
            let hours = (total_secs / 3600) % 24;
            let mut days = total_secs / 86400;

            // Calculate year/month/day from days since epoch
            let mut year: u64 = 1970;
            loop {
                let days_in_year = if is_leap(year) { 366 } else { 365 };
                if days < days_in_year {
                    break;
                }
                days -= days_in_year;
                year += 1;
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
                days -= md;
            }
            let day = days + 1;

            let month_names = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun",
                "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];

            let dow = (total_secs / 86400 + 4) % 7; // Jan 1 1970 was Thursday
            let dow_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

            println!(
                "{} {} {:>2} {:02}:{:02}:{:02} UTC {}",
                dow_names[dow as usize],
                month_names[month],
                day, hours, mins, secs, year
            );
        }
        Err(_) => {
            println!("date: unable to determine current time");
        }
    }
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
