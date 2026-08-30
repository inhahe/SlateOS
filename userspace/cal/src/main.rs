//! Slate OS calendar display utility.
//!
//! Multi-personality binary providing:
//! - **cal** — display a calendar
//! - **ncal** — display a calendar (weeks start on Monday)
//!
//! Shows monthly or yearly calendars with highlighting of the current day.

#![deny(clippy::all)]

use std::env;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Date calculations
// ============================================================================

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 => 31,
        2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 0,
    }
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    }
}

/// Day of week using Zeller's congruence.
/// Returns 0=Sunday, 1=Monday, ..., 6=Saturday.
fn day_of_week(year: i32, month: u32, day: u32) -> u32 {
    let mut y = year;
    let mut m = month as i32;
    if m < 3 {
        m += 12;
        y -= 1;
    }
    let q = day as i32;
    let k = y % 100;
    let j = y / 100;
    let h = (q + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 - 2 * j) % 7;
    let h = ((h + 7) % 7) as u32;
    // h: 0=Saturday, 1=Sunday, 2=Monday, ...
    // Convert to 0=Sunday.
    (h + 6) % 7
}

/// Get current date from the system (year, month, day).
fn current_date() -> (i32, u32, u32) {
    // Try reading from system.
    // On Slate OS this would use the kernel clock; for now use a reasonable default.
    // We'll try to parse /proc/driver/rtc or use std::time.
    use std::time::{SystemTime, UNIX_EPOCH};
    if let Ok(dur) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let secs = dur.as_secs() as i64;
        let (year, month, day) = unix_to_date(secs);
        return (year, month, day);
    }
    (2025, 1, 1) // fallback
}

fn unix_to_date(timestamp: i64) -> (i32, u32, u32) {
    // Days since 1970-01-01.
    let mut days = (timestamp / 86400) as i32;
    if timestamp < 0 {
        days -= 1;
    }

    // Compute year.
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Compute month.
    let mut month = 1u32;
    loop {
        let dim = days_in_month(year, month) as i32;
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
        if month > 12 {
            break;
        }
    }

    let day = (days + 1) as u32;
    (year, month, day)
}

fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    let mut doy = 0;
    for m in 1..month {
        doy += days_in_month(year, m);
    }
    doy + day
}

fn iso_week_number(year: i32, month: u32, day: u32) -> u32 {
    let doy = day_of_year(year, month, day);
    let dow = day_of_week(year, month, day);
    // ISO: Monday=1, Sunday=7.
    let iso_dow = if dow == 0 { 7 } else { dow };
    let w = (doy + 10 - iso_dow) / 7;
    if w < 1 { 52 } else if w > 52 { 1 } else { w }
}

// ============================================================================
// Calendar rendering
// ============================================================================

struct CalOpts {
    year: Option<i32>,
    month: Option<u32>,
    three_month: bool,
    full_year: bool,
    monday_first: bool,
    week_numbers: bool,
    julian: bool,
    highlight_today: bool,
    columns: u32,
}

fn render_month(year: i32, month: u32, monday_first: bool, highlight_day: Option<u32>, julian: bool, week_numbers: bool) -> Vec<String> {
    let mut lines = Vec::new();

    // Header.
    let title = format!("{} {year}", month_name(month));
    let width = if julian { 27 } else { 20 };
    let padding = if title.len() < width {
        (width - title.len()) / 2
    } else {
        0
    };
    lines.push(format!("{:>pad$}{title}", "", pad = padding));

    // Day headers.
    let day_header = if monday_first {
        if julian { "Mon Tue Wed Thu Fri Sat Sun" } else { "Mo Tu We Th Fr Sa Su" }
    } else if julian { "Sun Mon Tue Wed Thu Fri Sat" } else { "Su Mo Tu We Th Fr Sa" };
    let wk_prefix = if week_numbers { "Wk " } else { "" };
    lines.push(format!("{wk_prefix}{day_header}"));

    let first_dow = day_of_week(year, month, 1);
    let start_col = if monday_first {
        if first_dow == 0 { 6 } else { first_dow - 1 }
    } else {
        first_dow
    };

    let dim = days_in_month(year, month);
    let cell_width = if julian { 4 } else { 3 };

    let mut line = String::new();

    // Week number for first line.
    if week_numbers {
        let wk = iso_week_number(year, month, 1);
        line.push_str(&format!("{wk:>2} "));
    }

    // Leading blanks.
    for _ in 0..start_col {
        for _ in 0..cell_width {
            line.push(' ');
        }
    }

    let mut col = start_col;
    for day in 1..=dim {
        let day_str = if julian {
            let doy = day_of_year(year, month, day);
            format!("{doy:>3}")
        } else {
            format!("{day:>2}")
        };

        if Some(day) == highlight_day {
            // Highlight with reverse video.
            line.push_str(&format!("\x1b[7m{day_str}\x1b[0m "));
        } else {
            line.push_str(&day_str);
            line.push(' ');
        }

        col += 1;
        if col >= 7 {
            lines.push(line.trim_end().to_string());
            line = String::new();
            col = 0;
            // Week number for next line if there are more days.
            if day < dim && week_numbers {
                let wk = iso_week_number(year, month, day + 1);
                line.push_str(&format!("{wk:>2} "));
            }
        }
    }

    if !line.trim().is_empty() {
        lines.push(line.trim_end().to_string());
    }

    // Pad to 8 lines for consistent height.
    while lines.len() < 8 {
        lines.push(String::new());
    }

    lines
}

fn print_single_month(out: &mut io::StdoutLock<'_>, year: i32, month: u32, opts: &CalOpts, today: (i32, u32, u32)) {
    let highlight = if opts.highlight_today && year == today.0 && month == today.1 {
        Some(today.2)
    } else {
        None
    };
    let lines = render_month(year, month, opts.monday_first, highlight, opts.julian, opts.week_numbers);
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
}

fn print_three_months(out: &mut io::StdoutLock<'_>, year: i32, center_month: u32, opts: &CalOpts, today: (i32, u32, u32)) {
    let mut months = Vec::new();
    for delta in [-1i32, 0, 1] {
        let mut m = center_month as i32 + delta;
        let mut y = year;
        if m < 1 { m += 12; y -= 1; }
        if m > 12 { m -= 12; y += 1; }
        let highlight = if opts.highlight_today && y == today.0 && m as u32 == today.1 {
            Some(today.2)
        } else {
            None
        };
        months.push(render_month(y, m as u32, opts.monday_first, highlight, opts.julian, opts.week_numbers));
    }

    let width = if opts.julian { 27 } else { 22 };
    let max_lines = months.iter().map(|m| m.len()).max().unwrap_or(0);

    for line_idx in 0..max_lines {
        for (col, month_lines) in months.iter().enumerate() {
            let line = month_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
            if col + 1 < months.len() {
                let _ = write!(out, "{:<width$}  ", line);
            } else {
                let _ = write!(out, "{line}");
            }
        }
        let _ = writeln!(out);
    }
}

fn print_full_year(out: &mut io::StdoutLock<'_>, year: i32, opts: &CalOpts, today: (i32, u32, u32)) {
    // Year header.
    let title = format!("{year}");
    let total_width = if opts.julian { 27 * 3 + 4 } else { 22 * 3 + 4 };
    let padding = (total_width - title.len()) / 2;
    let _ = writeln!(out, "{:>pad$}{title}", "", pad = padding);
    let _ = writeln!(out);

    let cols = opts.columns.clamp(1, 4);
    let mut month = 1u32;

    while month <= 12 {
        let mut row_months = Vec::new();
        for c in 0..cols {
            let m = month + c;
            if m <= 12 {
                let highlight = if opts.highlight_today && year == today.0 && m == today.1 {
                    Some(today.2)
                } else {
                    None
                };
                row_months.push(render_month(year, m, opts.monday_first, highlight, opts.julian, opts.week_numbers));
            }
        }

        let width = if opts.julian { 27 } else { 22 };
        let max_lines = row_months.iter().map(|m| m.len()).max().unwrap_or(0);

        for line_idx in 0..max_lines {
            for (col, month_lines) in row_months.iter().enumerate() {
                let line = month_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
                if col + 1 < row_months.len() {
                    let _ = write!(out, "{:<width$}  ", line);
                } else {
                    let _ = write!(out, "{line}");
                }
            }
            let _ = writeln!(out);
        }

        month += cols;
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("cal");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let monday_default = prog_name == "ncal";

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let mut opts = CalOpts {
        year: None,
        month: None,
        three_month: false,
        full_year: false,
        monday_first: monday_default,
        week_numbers: false,
        julian: false,
        highlight_today: true,
        columns: 3,
    };

    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cal [options] [[month] year]");
                println!("       ncal [options] [[month] year]");
                println!();
                println!("Display a calendar.");
                println!();
                println!("Options:");
                println!("  -1                 Show current month only");
                println!("  -3                 Show prev/current/next month");
                println!("  -y, --year         Show entire year");
                println!("  -m, --monday       Start week on Monday");
                println!("  -s, --sunday       Start week on Sunday");
                println!("  -j, --julian       Julian day numbers");
                println!("  -w, --week         Show week numbers");
                println!("  --no-highlight     Don't highlight today");
                println!("  -c, --columns N    Columns for year view (1-4)");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cal {VERSION}");
                process::exit(0);
            }
            "-1" => { /* single month, default */ }
            "-3" => opts.three_month = true,
            "-y" | "--year" => opts.full_year = true,
            "-m" | "--monday" => opts.monday_first = true,
            "-s" | "--sunday" => opts.monday_first = false,
            "-j" | "--julian" => opts.julian = true,
            "-w" | "--week" => opts.week_numbers = true,
            "--no-highlight" => opts.highlight_today = false,
            "-c" | "--columns" => {
                i += 1;
                if i < rest.len() {
                    opts.columns = rest[i].parse().unwrap_or(3);
                }
            }
            s if !s.starts_with('-') => {
                positional.push(s.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    let today = current_date();

    // Parse positional arguments.
    match positional.len() {
        0 => {
            opts.year = Some(today.0);
            if !opts.full_year {
                opts.month = Some(today.1);
            }
        }
        1 => {
            let val: i32 = positional[0].parse().unwrap_or(today.0);
            if (1..=12).contains(&val) && !opts.full_year {
                opts.month = Some(val as u32);
                opts.year = Some(today.0);
            } else {
                opts.year = Some(val);
                opts.full_year = true;
            }
        }
        _ => {
            opts.month = positional[0].parse::<u32>().ok();
            opts.year = positional[1].parse::<i32>().ok();
        }
    }

    let year = opts.year.unwrap_or(today.0);
    let month = opts.month.unwrap_or(today.1);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.full_year {
        print_full_year(&mut out, year, &opts, today);
    } else if opts.three_month {
        print_three_months(&mut out, year, month, &opts, today);
    } else {
        print_single_month(&mut out, year, month, &opts, today);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
    }

    #[test]
    fn test_days_in_month_regular() {
        assert_eq!(days_in_month(2023, 1), 31);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 12), 31);
    }

    #[test]
    fn test_days_in_month_leap() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
    }

    #[test]
    fn test_month_name() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(6), "June");
        assert_eq!(month_name(12), "December");
    }

    #[test]
    fn test_day_of_week_known_dates() {
        // 2024-01-01 was Monday.
        assert_eq!(day_of_week(2024, 1, 1), 1);
        // 2024-07-04 was Thursday.
        assert_eq!(day_of_week(2024, 7, 4), 4);
        // 2000-01-01 was Saturday.
        assert_eq!(day_of_week(2000, 1, 1), 6);
        // 2023-12-25 was Monday.
        assert_eq!(day_of_week(2023, 12, 25), 1);
    }

    #[test]
    fn test_unix_to_date_epoch() {
        let (y, m, d) = unix_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_unix_to_date_known() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let (y, m, d) = unix_to_date(1704067200);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_unix_to_date_leap_day() {
        // 2024-02-29 = 1709164800
        let (y, m, d) = unix_to_date(1709164800);
        assert_eq!((y, m, d), (2024, 2, 29));
    }

    #[test]
    fn test_day_of_year() {
        assert_eq!(day_of_year(2024, 1, 1), 1);
        assert_eq!(day_of_year(2024, 2, 1), 32);
        assert_eq!(day_of_year(2024, 12, 31), 366);
        assert_eq!(day_of_year(2023, 12, 31), 365);
    }

    #[test]
    fn test_iso_week_number() {
        // 2024-01-01 is in week 1.
        assert_eq!(iso_week_number(2024, 1, 1), 1);
    }

    #[test]
    fn test_render_month_structure() {
        let lines = render_month(2024, 1, false, None, false, false);
        assert!(lines.len() >= 7);
        // First line should contain "January 2024".
        assert!(lines[0].contains("January"));
        assert!(lines[0].contains("2024"));
    }

    #[test]
    fn test_render_month_monday_first() {
        let lines = render_month(2024, 1, true, None, false, false);
        assert!(lines[1].starts_with("Mo"));
    }

    #[test]
    fn test_render_month_sunday_first() {
        let lines = render_month(2024, 1, false, None, false, false);
        assert!(lines[1].starts_with("Su"));
    }

    #[test]
    fn test_render_month_highlight() {
        let lines = render_month(2024, 1, false, Some(15), false, false);
        // Should contain escape codes for highlighting.
        let full = lines.join("\n");
        assert!(full.contains("\x1b[7m"));
    }

    #[test]
    fn test_render_month_no_highlight() {
        let lines = render_month(2024, 1, false, None, false, false);
        let full = lines.join("\n");
        assert!(!full.contains("\x1b[7m"));
    }

    #[test]
    fn test_render_month_julian() {
        let lines = render_month(2024, 1, false, None, true, false);
        // Julian mode uses 3-digit day-of-year numbers.
        let full = lines.join("\n");
        // Day 1 of year = "  1", day 31 = " 31"
        assert!(full.contains("  1"));
    }

    #[test]
    fn test_render_month_week_numbers() {
        let lines = render_month(2024, 1, true, None, false, true);
        // Should have "Wk" prefix.
        assert!(lines[1].starts_with("Wk"));
    }

    #[test]
    fn test_february_leap_year() {
        let lines = render_month(2024, 2, false, None, false, false);
        let full = lines.join("\n");
        assert!(full.contains("29"));
    }

    #[test]
    fn test_february_non_leap() {
        let lines = render_month(2023, 2, false, None, false, false);
        let full = lines.join("\n");
        assert!(!full.contains("29"));
    }

    #[test]
    fn test_current_date() {
        let (y, m, d) = current_date();
        assert!(y >= 2024);
        assert!((1..=12).contains(&m));
        assert!((1..=31).contains(&d));
    }

    #[test]
    fn test_invalid_month() {
        assert_eq!(days_in_month(2024, 0), 0);
        assert_eq!(days_in_month(2024, 13), 0);
    }

    #[test]
    fn test_days_in_month_all() {
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (i, &exp) in expected.iter().enumerate() {
            assert_eq!(days_in_month(2023, (i + 1) as u32), exp);
        }
    }
}
