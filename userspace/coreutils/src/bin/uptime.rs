//! uptime — tell how long the system has been running.
//!
//! Usage: uptime
//!   Reads /proc/uptime for system uptime information.

use std::fs;

fn main() {
    match fs::read_to_string("/proc/uptime") {
        Ok(content) => println!("{}", format_uptime_line(&content)),
        Err(_) => println!("uptime: cannot read /proc/uptime"),
    }
}

/// Format `/proc/uptime` content into the human-readable line we print.
fn format_uptime_line(content: &str) -> String {
    let parts: Vec<&str> = content.split_whitespace().collect();
    let Some(secs_str) = parts.first() else {
        return "up (unknown)".to_string();
    };
    let total_secs: f64 = secs_str.parse().unwrap_or(0.0);
    let (days, hours, mins) = split_uptime(total_secs);
    let mut out = String::from("up ");
    if days > 0 {
        let suffix = if days == 1 { "" } else { "s" };
        out.push_str(&format!("{days} day{suffix}, "));
    }
    out.push_str(&format!("{hours:02}:{mins:02}"));
    out
}

/// Break a total-seconds count into `(days, hours, mins)` for display.
fn split_uptime(total_secs: f64) -> (u64, u64, u64) {
    let secs = if total_secs.is_finite() && total_secs >= 0.0 {
        total_secs
    } else {
        0.0
    };
    let days = (secs / 86400.0) as u64;
    let hours = ((secs % 86400.0) / 3600.0) as u64;
    let mins = ((secs % 3600.0) / 60.0) as u64;
    (days, hours, mins)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn split_zero() {
        assert_eq!(split_uptime(0.0), (0, 0, 0));
    }

    #[test]
    fn split_one_minute() {
        assert_eq!(split_uptime(60.0), (0, 0, 1));
    }

    #[test]
    fn split_one_hour() {
        assert_eq!(split_uptime(3600.0), (0, 1, 0));
    }

    #[test]
    fn split_one_hour_one_min() {
        assert_eq!(split_uptime(3660.0), (0, 1, 1));
    }

    #[test]
    fn split_one_day() {
        assert_eq!(split_uptime(86400.0), (1, 0, 0));
    }

    #[test]
    fn split_mixed() {
        // 2 days, 3 hours, 4 minutes = 2*86400 + 3*3600 + 4*60.
        let s = 2.0 * 86400.0 + 3.0 * 3600.0 + 4.0 * 60.0;
        assert_eq!(split_uptime(s), (2, 3, 4));
    }

    #[test]
    fn split_fractional_truncates() {
        // 59.999 seconds -> 0 mins.
        assert_eq!(split_uptime(59.999), (0, 0, 0));
        // 60.5 -> 0:01.
        assert_eq!(split_uptime(60.5), (0, 0, 1));
    }

    #[test]
    fn split_negative_clamped_to_zero() {
        assert_eq!(split_uptime(-100.0), (0, 0, 0));
    }

    #[test]
    fn split_nan_clamped_to_zero() {
        assert_eq!(split_uptime(f64::NAN), (0, 0, 0));
    }

    #[test]
    fn split_infinity_clamped_to_zero() {
        assert_eq!(split_uptime(f64::INFINITY), (0, 0, 0));
    }

    #[test]
    fn format_basic_seconds() {
        // 60 seconds -> "up 00:01".
        assert_eq!(format_uptime_line("60.0 30.0"), "up 00:01");
    }

    #[test]
    fn format_one_day_singular() {
        let s = format!("{} 0", 86400);
        assert_eq!(format_uptime_line(&s), "up 1 day, 00:00");
    }

    #[test]
    fn format_two_days_plural() {
        let total = 2 * 86400 + 3 * 3600 + 5 * 60;
        let s = format!("{total} 0");
        assert_eq!(format_uptime_line(&s), "up 2 days, 03:05");
    }

    #[test]
    fn format_empty_returns_unknown() {
        assert_eq!(format_uptime_line(""), "up (unknown)");
    }

    #[test]
    fn format_garbage_first_field_is_zero() {
        assert_eq!(format_uptime_line("garbage 0"), "up 00:00");
    }

    #[test]
    fn format_just_seconds_no_idle() {
        assert_eq!(format_uptime_line("120"), "up 00:02");
    }

    #[test]
    fn format_multiple_whitespace() {
        assert_eq!(format_uptime_line("60.0   30.0"), "up 00:01");
    }

    #[test]
    fn format_zero_seconds() {
        assert_eq!(format_uptime_line("0 0"), "up 00:00");
    }

    #[test]
    fn format_almost_one_day_no_days_prefix() {
        let total = 86399; // 23:59:59
        let s = format!("{total} 0");
        assert_eq!(format_uptime_line(&s), "up 23:59");
    }
}
