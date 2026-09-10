//! Slate OS calendar display utility.
//!
//! Multi-personality binary providing:
//! - **cal** — display a calendar
//! - **ncal** — display a calendar (weeks start on Monday)
//!
//! Shows monthly or yearly calendars with highlighting of the current day.

#![deny(clippy::all)]

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

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
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
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

/// Today, as the system reckons it, or `None` if the clock cannot be read.
///
/// `SystemTime::now()` *is* the kernel clock on this target -- there is no
/// second, more direct route to try, and the comment here used to say
/// otherwise ("on Slate OS this would use the kernel clock; for now use a
/// reasonable default", plus a note about parsing `/proc/driver/rtc").
///
/// The `None` is the point. This returned `(2025, 1, 1)` when the clock could
/// not be read, so `cal` printed a calendar for January 2025 with the 1st
/// highlighted as today and exited 0. No caller could tell that from a correct
/// answer. A wrong date stated confidently is the same defect as the 2,285
/// commands `design-decisions.md` 1006 deleted, in a program worth keeping.
fn current_date() -> Option<(i32, u32, u32)> {
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let secs = i64::try_from(dur.as_secs()).ok()?;
    Some(unix_to_date(secs))
}

/// Which month `cal` should print, given its positional arguments.
///
/// Takes `OsStr` rather than `str` because argv is OS-boundary data and this
/// OS allows any byte but `/` and NUL in it. Nothing here needs the argument
/// to *be* text -- it needs it to be a number -- so a value that does not
/// decode simply fails to parse, which is already an error this function
/// reports. `cal` used to read argv as `String` and panicked with a Rust
/// backtrace before its first statement on anything else.
///
/// Split out of `main` so the clock-less paths can be tested. The rule is that
/// **only the arguments you did not supply need a clock**: `cal 9 2026` names
/// a month outright and works with no clock at all, while bare `cal` and
/// `cal 9` mean "of this year", which is a question only the system can
/// answer.
///
/// Returns the year, the month, and whether a full year was asked for.
fn resolve_period(
    positional: &[OsString],
    full_year: bool,
    today: Option<(i32, u32, u32)>,
) -> Result<(i32, u32, bool), NoClock> {
    let year_of = |t: Option<(i32, u32, u32)>| t.map(|(y, _, _)| y).ok_or(NoClock);
    let month_of = |t: Option<(i32, u32, u32)>| t.map(|(_, m, _)| m).ok_or(NoClock);

    match positional.len() {
        0 => Ok((year_of(today)?, month_of(today)?, full_year)),
        1 => {
            let Ok(val) = positional[0].to_str().unwrap_or("").parse::<i32>() else {
                // Not a number at all: the old code fell back to the current
                // year and printed *something*. Refusing is the honest answer,
                // and it does not depend on the clock.
                return Err(NoClock);
            };
            if (1..=12).contains(&val) && !full_year {
                // A month of the current year -- which needs the clock.
                Ok((
                    year_of(today)?,
                    u32::try_from(val).map_err(|_| NoClock)?,
                    false,
                ))
            } else {
                Ok((val, 1, true))
            }
        }
        _ => {
            let month = positional[0]
                .to_str()
                .unwrap_or("")
                .parse::<u32>()
                .map_err(|_| NoClock)?;
            let year = positional[1]
                .to_str()
                .unwrap_or("")
                .parse::<i32>()
                .map_err(|_| NoClock)?;
            Ok((year, month, full_year))
        }
    }
}

/// The clock could not be read, or an argument could not be parsed -- either
/// way `cal` does not know which month was meant.
#[derive(Debug, PartialEq, Eq)]
struct NoClock;

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
    if w < 1 {
        52
    } else if w > 52 {
        1
    } else {
        w
    }
}

// ============================================================================
// Calendar rendering
// ============================================================================

struct CalOpts {
    three_month: bool,
    full_year: bool,
    monday_first: bool,
    week_numbers: bool,
    julian: bool,
    highlight_today: bool,
    columns: u32,
}

fn render_month(
    year: i32,
    month: u32,
    monday_first: bool,
    highlight_day: Option<u32>,
    julian: bool,
    week_numbers: bool,
) -> Vec<String> {
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
        if julian {
            "Mon Tue Wed Thu Fri Sat Sun"
        } else {
            "Mo Tu We Th Fr Sa Su"
        }
    } else if julian {
        "Sun Mon Tue Wed Thu Fri Sat"
    } else {
        "Su Mo Tu We Th Fr Sa"
    };
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

fn print_single_month(
    out: &mut io::StdoutLock<'_>,
    year: i32,
    month: u32,
    opts: &CalOpts,
    today: Option<(i32, u32, u32)>,
) {
    let highlight =
        if opts.highlight_today && today.is_some_and(|(ty, tm, _)| year == ty && month == tm) {
            today.map(|(_, _, td)| td)
        } else {
            None
        };
    let lines = render_month(
        year,
        month,
        opts.monday_first,
        highlight,
        opts.julian,
        opts.week_numbers,
    );
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
}

fn print_three_months(
    out: &mut io::StdoutLock<'_>,
    year: i32,
    center_month: u32,
    opts: &CalOpts,
    today: Option<(i32, u32, u32)>,
) {
    let mut months = Vec::new();
    for delta in [-1i32, 0, 1] {
        let mut m = center_month as i32 + delta;
        let mut y = year;
        if m < 1 {
            m += 12;
            y -= 1;
        }
        if m > 12 {
            m -= 12;
            y += 1;
        }
        let highlight =
            if opts.highlight_today && today.is_some_and(|(ty, tm, _)| y == ty && m as u32 == tm) {
                today.map(|(_, _, td)| td)
            } else {
                None
            };
        months.push(render_month(
            y,
            m as u32,
            opts.monday_first,
            highlight,
            opts.julian,
            opts.week_numbers,
        ));
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

fn print_full_year(
    out: &mut io::StdoutLock<'_>,
    year: i32,
    opts: &CalOpts,
    today: Option<(i32, u32, u32)>,
) {
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
                let highlight = if opts.highlight_today
                    && today.is_some_and(|(ty, tm, _)| year == ty && m == tm)
                {
                    today.map(|(_, _, td)| td)
                } else {
                    None
                };
                row_months.push(render_month(
                    year,
                    m,
                    opts.monday_first,
                    highlight,
                    opts.julian,
                    opts.week_numbers,
                ));
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
    let args: Vec<OsString> = env::args_os().collect();

    // argv[0] is only ever compared against "ncal", so a name that does not
    // decode is simply not that name. `to_str()` says so exactly; a lossy
    // conversion would answer the same question by corrupting the input first.
    let prog_name = args
        .first()
        .and_then(|s| s.to_str())
        .map(|s| {
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        })
        .unwrap_or_else(|| "cal".to_string());

    let monday_default = prog_name == "ncal";

    let rest: Vec<OsString> = args.into_iter().skip(1).collect();

    let mut opts = CalOpts {
        three_month: false,
        full_year: false,
        monday_first: monday_default,
        week_numbers: false,
        julian: false,
        highlight_today: true,
        columns: 3,
    };

    let mut positional: Vec<OsString> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].to_str().unwrap_or("\u{fffd}") {
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
                    opts.columns = rest[i].to_str().unwrap_or("").parse().unwrap_or(3);
                }
            }
            s if !s.starts_with('-') => {
                // Push the original OS string, not the decoded `s` -- `s` is a
                // replacement character when the argument did not decode, and
                // the point is to carry the caller's bytes through.
                positional.push(rest[i].clone());
                let _ = s;
            }
            _ => {}
        }
        i += 1;
    }

    let today = current_date();

    let (year, month, full_year) = match resolve_period(&positional, opts.full_year, today) {
        Ok(v) => v,
        Err(NoClock) => {
            // The one thing not to do here is print a calendar anyway.
            eprintln!("cal: cannot tell which month you mean");
            if today.is_none() {
                eprintln!("cal: the system clock could not be read, so there is no \"this month\"");
            }
            eprintln!("cal: name it explicitly, e.g. `cal 9 2026`, or `cal 2026` for the year");
            process::exit(1);
        }
    };
    opts.full_year = full_year;

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
        // The host has a clock, so this must succeed there. On a machine
        // without one it returns None, which is the whole point of the type.
        let (y, m, d) = current_date().expect("the host has a clock");
        assert!(y >= 2024);
        assert!((1..=12).contains(&m));
        assert!((1..=31).contains(&d));
    }

    /// `unix_to_date` against dates computed elsewhere.
    ///
    /// The guard came off this program on 2026-09-10, so its arithmetic is now
    /// load-bearing rather than decorative. Leap years and leap *days* are
    /// where a hand-rolled civil-date conversion goes wrong, so both centuries
    /// rules are here: 2000 was a leap year (divisible by 400) and 1900 was
    /// not, though only the first is reachable from a Unix timestamp.
    #[test]
    fn unix_to_date_agrees_with_known_dates() {
        assert_eq!(unix_to_date(0), (1970, 1, 1));
        assert_eq!(unix_to_date(86_399), (1970, 1, 1)); // one second to midnight
        assert_eq!(unix_to_date(86_400), (1970, 1, 2));
        assert_eq!(unix_to_date(951_782_400), (2000, 2, 29)); // the 400-year rule
        assert_eq!(unix_to_date(951_868_800), (2000, 3, 1));
        assert_eq!(unix_to_date(1_709_164_800), (2024, 2, 29));
        assert_eq!(unix_to_date(1_767_225_600), (2026, 1, 1));
        assert_eq!(unix_to_date(1_756_684_800), (2025, 9, 1));
    }

    /// A year and month given outright need no clock.
    #[test]
    fn an_explicit_month_works_without_a_clock() {
        let args = vec![OsString::from("9"), OsString::from("2026")];
        assert_eq!(resolve_period(&args, false, None), Ok((2026, 9, false)));
    }

    /// A year given outright needs no clock either.
    #[test]
    fn an_explicit_year_works_without_a_clock() {
        let args = vec![OsString::from("2026")];
        assert_eq!(resolve_period(&args, false, None), Ok((2026, 1, true)));
    }

    /// Everything that means "of this year" needs one, and says so.
    ///
    /// This is the case the change exists for. `current_date` used to answer
    /// `(2025, 1, 1)` when it could not read the clock, so bare `cal` printed
    /// January 2025 with the 1st highlighted as today and exited 0 -- a
    /// confident wrong answer, indistinguishable from a right one.
    #[test]
    fn a_month_of_this_year_refuses_without_a_clock() {
        assert_eq!(resolve_period(&[], false, None), Err(NoClock));
        assert_eq!(
            resolve_period(&[OsString::from("9")], false, None),
            Err(NoClock)
        );
    }

    /// ...and the same arguments succeed once there is one.
    #[test]
    fn the_same_arguments_succeed_with_a_clock() {
        let today = Some((2026, 9, 10));
        assert_eq!(resolve_period(&[], false, today), Ok((2026, 9, false)));
        assert_eq!(
            resolve_period(&[OsString::from("9")], false, today),
            Ok((2026, 9, false))
        );
        // A number outside 1..=12 is a year, not a month, clock or no clock.
        assert_eq!(
            resolve_period(&[OsString::from("2026")], false, today),
            Ok((2026, 1, true))
        );
    }

    /// An argument that is not valid text at all is refused, not fatal.
    ///
    /// This is what the `argv-utf8` gate refuses, and `cal` walked straight
    /// into it the moment the `notimpl` guard came off: reading argv as
    /// `String` panics with a Rust backtrace *before the program's first
    /// statement* on a byte sequence that is not UTF-8, and a path on this OS
    /// may contain any byte but `/` and NUL. Nothing in `cal` needs its
    /// argument to be text -- it needs it to be a number -- so the undecodable
    /// case is just another thing that does not parse.
    #[test]
    fn an_argument_that_is_not_utf8_is_refused_rather_than_fatal() {
        let bad = non_utf8_osstring();
        assert!(bad.to_str().is_none(), "fixture must not be valid UTF-8");
        let today = Some((2026, 9, 10));
        assert_eq!(
            resolve_period(std::slice::from_ref(&bad), false, today),
            Err(NoClock)
        );
        assert_eq!(
            resolve_period(&[bad, OsString::from("2026")], false, today),
            Err(NoClock)
        );
    }

    /// An `OsString` the platform accepts and `str` cannot represent.
    #[cfg(windows)]
    fn non_utf8_osstring() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        // A lone high surrogate: legal in a Windows path, not encodable as
        // UTF-8, so `to_str()` returns None.
        OsString::from_wide(&[0xD800])
    }

    #[cfg(unix)]
    fn non_utf8_osstring() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        // 0xFF cannot begin a UTF-8 sequence, and is a legal byte in a path
        // here -- which is the case this whole conversion exists for.
        OsString::from_vec(vec![0xFF])
    }

    /// An argument that is not a number is refused rather than guessed at.
    ///
    /// It used to `parse().unwrap_or(today.0)`, so `cal banana` printed the
    /// current year's calendar and exited 0.
    #[test]
    fn a_nonsense_argument_is_refused_not_guessed() {
        let today = Some((2026, 9, 10));
        assert_eq!(
            resolve_period(&[OsString::from("banana")], false, today),
            Err(NoClock)
        );
        assert_eq!(
            resolve_period(&[OsString::from("x"), OsString::from("2026")], false, today),
            Err(NoClock)
        );
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
