//! `cal`, transcribed from util-linux 2.39.3 rather than remembered.
//!
//! The calendar is the one utility where "looks about right" is indistinguishable
//! from correct at a glance and wrong in every column that matters. Its output is
//! a fixed-width grid whose geometry is not derivable from the dates — the month
//! block is 20 columns wide and **always six rows tall**, the gutter is two
//! spaces (three under `-y`), the header is centred with the *odd* space on the
//! left, and every line is padded to full width with trailing blanks. A
//! reimplementation that computes the right days and lays them out by eye differs
//! from upstream on nearly every byte.
//!
//! So this one is a transcription. Every function below names the `cal.c`
//! function it came from, and the layout rules were checked against byte-exact
//! captures (`od -c`) of util-linux 2.39.3 built from source, not against
//! recollection of what a calendar looks like.
//!
//! # What was wrong with the previous implementation
//!
//! 1. **argv was read as `Vec<String>`**, so a non-UTF-8 argument aborted the
//!    process before `cal` could reject it with a sentence. This is the finding
//!    `scripts/argv-utf8.py` recorded as `cal.rs:argv-as-string`.
//! 2. **`args[0].parse().unwrap_or(2024)`** — a year that did not parse silently
//!    became 2024. `cal xyz` printed the 2024 calendar; upstream says
//!    `cal: failed to parse timestamp or unknown month name: xyz` and exits 1.
//! 3. **`cal 5` showed May of the current year.** A single operand is a *year*
//!    upstream: `cal 5` is the calendar for AD 5. The old code guessed with a
//!    `y > 12` heuristic, so every year from 1 to 12 was unreachable and every
//!    month was reachable by an argument that upstream reads as a year.
//! 4. **No options at all**, and no `--`. `-1 -3 -n -S -s -m -j -y -Y -w -v -c`,
//!    `--reform`, `--iso`, `--color`, `--help` and `--version` were all absent,
//!    and a leading `-` was just a parse failure.
//! 5. **No month names and no timestamp operand.** `cal february`, `cal now`,
//!    `cal 2024-02-15`, `cal @1700000000`, `cal +1month` and `cal '2 days ago'`
//!    are all accepted upstream.
//! 6. **No day operand**, so the three-argument form `cal 15 2 2024` — which
//!    highlights the day — did not exist, and neither did the highlight.
//! 7. **No Gregorian reform.** September 1752 was printed with all 30 days
//!    instead of the eleven-day hole, and the weekday was computed with Zeller's
//!    congruence run proleptically back to year 1, so every date before the
//!    reform fell on the wrong day of the week.
//! 8. **Year 0 and negative years were accepted.** Upstream's range is 1 to
//!    `INT32_MAX - 1`.
//! 9. **The geometry was wrong in four separate ways**: a trailing space after
//!    every day, no leading pad on the first column, weeks emitted ragged
//!    instead of a fixed six-row 20-column block, and no padding of short lines.
//! 10. **The header was centred with a floor `(20 - len) / 2`**, putting the odd
//!     space on the right where upstream puts it on the left.
//! 11. **`cal <year>` stacked twelve months vertically** under a hardcoded
//!     28-space indent, rather than the three-across grid with a centred year.
//! 12. **`println!` on a closed pipe** panicked, so `cal 2024 | head -1` exited
//!     134 where upstream exits 0.
//! 13. **`days_in_month` returned 30 for month 0 and month 13**, so a bad month
//!     produced a plausible-looking calendar rather than a diagnostic.
//! 14. **No `guard_std_fds!()`**, so `cal 2>&-` could write a diagnostic into
//!     whatever file later took fd 2.
//! 15. **No today highlight and no colour**, so `--color` had nothing to colour
//!     and the current day was invisible.
//!
//! # Measured against util-linux 2.39.3
//!
//! Captured on a WSL Ubuntu host from a locally built `/usr/local/bin/cal`
//! (`LC_ALL=C.UTF-8`, `TZ=UTC`, wall clock 2026-08-27). The full captures are
//! the expectations in the test module.
//!
//! | Command | Upstream |
//! |---|---|
//! | `cal 2 2024` | 20-column block, six rows, `Su Mo Tu We Th Fr Sa` |
//! | `cal -j 2 2024` | day-of-year, 27 columns, `Sun Mon Tue …` |
//! | `cal --week 2 2024` | 23 columns, US week numbers in a 2-wide left column |
//! | `cal -m --week 1 2010` | ISO weeks: 1 Jan 2010 is in week 53 of 2009 |
//! | `cal 9 1752` | 2 Sep is followed by 14 Sep |
//! | `cal --reform=julian 9 1752` | all 30 days, Julian weekday |
//! | `cal -3 1 2024` | Dec 2023, Jan 2024, Feb 2024 |
//! | `cal -n 13 2024` | rows of 3, 3, 3, 3, then 1 |
//! | `cal -v 2 2024` | days down the column, header gutter one wider than body |
//! | `cal -y 2024` | `2024` centred in 66 columns, then a blank line |
//! | `cal 20240215` | the year 20240215, not a date |
//! | `cal sept` | `failed to parse timestamp or unknown month name: sept` |
//! | `cal -w5` | `invalid option -- '5'` (`-w`'s value is long-only) |
//! | `cal -y -Y` | `mutually exclusive arguments: --twelve --months --year` |
//! | `cal 1 99999999999` | `illegal year value: '…': Numerical result out of range` |
//! | `cal -c 1.5K` | 1536 columns of months, accepted |
//!
//! # Where this deliberately diverges
//!
//! 1. **Two upstream misalignment bugs are reproduced, not fixed.** In vertical
//!    mode the header appends its gutter after the *last* month as well as
//!    between months, so `cal -v 2 2024`'s header line is three bytes wider than
//!    its body lines; and a highlighted week number in vertical mode is printed
//!    at width `day_width - narrow` rather than `day_width`, so it sits one
//!    column left of the unhighlighted ones. Both are visible in the `od` dumps
//!    of the reference build. Matching upstream byte-for-byte is the point of
//!    the exercise; a "fix" here would be a silent divergence that only shows up
//!    when someone diffs against a real `cal`.
//! 2. **`--version` keeps util-linux's *shape*, not the coreutils one.** Every
//!    other binary here says `NAME (SlateOS coreutils) 0.1.0`; `cal` says
//!    `cal from SlateOS coreutils 0.1.0`, because that is the shape scripts
//!    parse for util-linux tools.
//! 3. **The terminal width comes from `COLUMNS`, defaulting to 80.** There is no
//!    `TIOCGWINSZ` in this build (the same limitation `ls` documents), so `-c
//!    auto` and the automatic three-across fit use the environment.
//! 4. **The calendar is rendered into a `String` and written once.** Upstream
//!    streams to `stdout`. The difference is observable only in how a
//!    mid-calendar `EPIPE` is reported, and writing once is what lets
//!    `stdfd::close_stdout` report it the way every other binary here does.
//! 5. **Operands echoed in diagnostics go through `quote::escape_unprintable`**,
//!    so a month name containing a newline cannot forge a second diagnostic
//!    line. Upstream prints the bytes.
//! 6. **`_NL_TIME_WEEK_1STDAY` is not consulted.** Upstream asks the locale
//!    whether the week starts on Sunday or Monday; the C locale answers Sunday,
//!    which is what this hardcodes. `-m` and `--iso` are the way to change it.
//! 7. **The `workday` and `weekend` colour sequences are omitted.** Upstream
//!    defines the names but their built-in values are empty strings, so only
//!    `today` and `weeknumber` (both reverse video) ever emit anything.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::Write as _;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use coreutils::getopt::{Error, Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd::{self, Stream};

// ------------------------------------------------------------- the tables ---

const CAL: Program = Program::new("cal", 1);

/// `cal.c`'s own short-option string, colons and all.
///
/// Note what is missing: **`w` has no colon**. `--week=5` takes a value but
/// `-w5` does not, and answers `invalid option -- '5'` — the `5` is read as the
/// next option in the bundle. That asymmetry is the single most surprising thing
/// in `cal`'s command line and it is not a typo here.
const SHORT_OPTIONS: &str = "13mjn:sSywYvc:Vh";

/// The long options, **in `cal.c`'s order**, which is observable: an ambiguous
/// prefix is answered with its candidates listed in table order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("one", Takes::Nothing),
    ("three", Takes::Nothing),
    ("sunday", Takes::Nothing),
    ("monday", Takes::Nothing),
    ("julian", Takes::Nothing),
    ("months", Takes::Required),
    ("span", Takes::Nothing),
    ("year", Takes::Nothing),
    ("week", Takes::Optional),
    ("color", Takes::Optional),
    ("reform", Takes::Required),
    ("iso", Takes::Nothing),
    ("version", Takes::Nothing),
    ("twelve", Takes::Nothing),
    ("help", Takes::Nothing),
    ("vertical", Takes::Nothing),
    ("columns", Takes::Required),
];

/// The three named reform points, plus the numeric one.
///
/// `GREGORIAN` and `ISO_REFORM` are the same value: both mean "the Gregorian
/// rules have always applied", and they differ only in that `--iso` *also* sets
/// the week numbering to ISO-8601. `JULIAN` means "they never did".
const GREGORIAN: i32 = i32::MIN;
const ISO_REFORM: i32 = i32::MIN;
const GB1752: i32 = 1752;
const JULIAN: i32 = i32::MAX;
const DEFAULT_REFORM_YEAR: i32 = 1752;

const SUNDAY: i32 = 0;
const MONDAY: i32 = 1;
const WEDNESDAY: i32 = 3;
const FRIDAY: i32 = 5;
const DAYS_IN_WEEK: usize = 7;
/// `day_in_week`'s "this date does not exist" answer — the eleven days the
/// reform deleted fall in neither the Julian nor the Gregorian branch.
const NONEDAY: i32 = 8;

/// The marker for a cell of the 6x7 grid that holds no day.
const SPACE: i32 = -1;
const MAXDAYS: usize = 42;
const MONTHS_IN_YEAR: usize = 12;
const SMALLEST_YEAR: i32 = 1;

const REFORMATION_MONTH: i32 = 9;
/// 2 September 1752 was followed by 14 September 1752.
const NUMBER_MISSING_DAYS: i32 = 11;
/// The day-of-year of 14 September 1752, counted as if no days were missing.
const YDAY_AFTER_MISSING: i32 = 258;

const MONTHS_IN_YEAR_ROW: i32 = 3;
const DOY_MONTH_WIDTH: i32 = 27;
const DOM_MONTH_WIDTH: i32 = 20;

const WEEK_NUM_DISABLED: u32 = 0;
const WEEK_NUM_MASK: u32 = 0xff;
const WEEK_NUM_ISO: u32 = 0x100;
const WEEK_NUM_US: u32 = 0x200;

/// `-c` unset: use three months per row, narrowing only if the terminal cannot
/// hold three.
const COLUMNS_MAX_THREE: i64 = -1;
/// `-c auto`: use as many months per row as the terminal can hold.
const COLUMNS_AUTO: i64 = -2;

/// Reverse video, which is `cal.c`'s built-in value for both `today` and
/// `weeknumber`. The other three colour names it defines default to `""`.
const HIGHLIGHT: &str = "\x1b[7m";
const RESET: &str = "\x1b[0m";

/// `cal.c`'s `days_in_month[2][13]`, Julian and Gregorian sharing one table
/// because they differ only in which years are leap.
const DAYS_IN_MONTH: [[i32; 13]; 2] = [
    [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

const FULL_MONTH: [&str; MONTHS_IN_YEAR] = [
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

/// The C locale's `ABMON_1..12`. Note `Sep`, not `Sept` — which is why
/// `cal sept` is a parse error and `cal sep` is not.
const ABBR_MONTH: [&str; MONTHS_IN_YEAR] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The C locale's `ABDAY_1..7`. The two-letter headings of a normal calendar are
/// these truncated to width 2 by [`center`], not a separate table.
const ABDAY: [&str; DAYS_IN_WEEK] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColorMode {
    Undef,
    Auto,
    Never,
    Always,
}

/// What the operands asked for. `year` is `i32` because upstream's is, and the
/// `INT32_MAX` rejection below is only meaningful at that width.
#[derive(Clone, Copy, Debug, Default)]
struct Request {
    /// Day *of year*, 1-based — not day of month. `cal_output_months` converts
    /// back per month. Zero means "no day is highlighted".
    day: i32,
    month: i32,
    year: i32,
    week: i32,
    start_month: i32,
}

/// `cal.c`'s `struct cal_control`, after `main` has finished filling it in.
#[derive(Clone, Debug)]
struct Ctl {
    reform_year: i32,
    colormode: ColorMode,
    weekstart: i32,
    weektype: u32,
    day_width: usize,
    week_width: usize,
    gutter_width: usize,
    num_months: i32,
    months_in_row: i32,
    span_months: bool,
    req: Request,
    julian: bool,
    header_hint: bool,
    header_year: bool,
    vertical: bool,
    colors: bool,
    /// `day_headings`, built once by `headers_init`.
    day_headings: String,
}

impl Default for Ctl {
    fn default() -> Self {
        Ctl {
            reform_year: DEFAULT_REFORM_YEAR,
            colormode: ColorMode::Undef,
            weekstart: SUNDAY,
            weektype: WEEK_NUM_DISABLED,
            day_width: 3,
            week_width: 0,
            gutter_width: 2,
            num_months: 0,
            months_in_row: 0,
            span_months: false,
            req: Request::default(),
            julian: false,
            header_hint: false,
            header_year: false,
            vertical: false,
            colors: false,
            day_headings: String::new(),
        }
    }
}

/// One month's filled-in 6x7 grid, as `cal.c`'s `struct cal_month`.
#[derive(Clone, Debug)]
struct CalMonth {
    days: [i32; MAXDAYS],
    weeks: [i32; 6],
    month: i32,
    year: i32,
}

/// What `cal` needs to know about the terminal, hoisted out so that everything
/// below `run_main` is a pure function of its arguments and therefore testable.
#[derive(Clone, Copy, Debug)]
struct Terminal {
    is_tty: bool,
    width: i32,
}

// ---------------------------------------------------- the calendar itself ---

/// `cal.c`'s `leap_year`.
///
/// The reform year is the hinge: on or before it the Julian rule applies (every
/// fourth year), after it the Gregorian one. With `--reform=julian` the reform
/// year is `INT32_MAX`, so the Julian rule applies forever; with
/// `--reform=gregorian` it is `INT32_MIN`, so the Gregorian rule does.
fn leap_year(reform_year: i32, year: i32) -> bool {
    if year <= reform_year {
        year % 4 == 0
    } else {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }
}

/// The largest day number any month has — `cal.c`'s `DAYS_IN_MONTH` scalar, kept
/// distinct from the `days_in_month[][]` table above.
const MAX_DAYS_IN_MONTH: i32 = 31;

/// Length of `month` in `year`, or 0 if `month` is outside 1..=12.
///
/// The old implementation answered 30 for month 0 and month 13, which turned a
/// bad operand into a plausible calendar. Every caller here has already checked
/// the range; returning 0 rather than indexing out of bounds is what keeps that
/// true even if one day a caller does not.
fn month_length(reform_year: i32, month: i32, year: i32) -> i32 {
    let leap = usize::from(leap_year(reform_year, year));
    usize::try_from(month)
        .ok()
        .and_then(|m| DAYS_IN_MONTH[leap].get(m).copied())
        .unwrap_or(0)
}

/// `cal.c`'s `day_in_year`: the 1-based day of the year of `day`/`month`.
///
/// It counts the reformation year's September as a full 30 days, which is what
/// makes `YDAY_AFTER_MISSING` the right correction everywhere it is applied.
fn day_in_year(reform_year: i32, day: i32, month: i32, year: i32) -> i32 {
    let leap = usize::from(leap_year(reform_year, year));
    let mut total = day;
    let mut m = 1i32;
    while m < month {
        if let Ok(i) = usize::try_from(m) {
            total += DAYS_IN_MONTH[leap].get(i).copied().unwrap_or(0);
        }
        m += 1;
    }
    total
}

/// `cal.c`'s `day_in_week`, transcribed with the arithmetic promoted to `i64`.
///
/// The promotion is not cosmetic. `--reform=julian` sets `reform_year` to
/// `INT32_MAX`, and the very first line computes `reform_year + 1`, which in C
/// is signed overflow — undefined, and in practice wrapping to `INT32_MIN`. At
/// `i64` the sum is representable and no valid year (1..`INT32_MAX-1`) equals
/// it, which is the same answer the wrap happens to give, arrived at honestly.
///
/// Returns 0..6 with Sunday 0, or [`NONEDAY`] for the eleven days the reform
/// deleted — 3 to 13 September of the reform year, which belong to neither
/// branch.
fn day_in_week(reform_year: i32, day: i32, month: i32, year: i32) -> i32 {
    /// Days-since-a-known-Sunday, per month, under the Gregorian rules.
    const REFORM: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    /// The same, under the Julian ones.
    const OLD: [i64; 12] = [5, 1, 0, 3, 5, 1, 3, 6, 2, 4, 0, 2];

    let reform = i64::from(reform_year);
    let day = i64::from(day);
    let month = i64::from(month);
    let mut year = i64::from(year);

    if year != reform + 1 {
        year -= i64::from(month < 3);
    } else {
        year -= i64::from(month < 3) + 14;
    }

    let Ok(idx) = usize::try_from(month - 1) else {
        return NONEDAY;
    };
    let (Some(&reform_off), Some(&old_off)) = (REFORM.get(idx), OLD.get(idx)) else {
        return NONEDAY;
    };

    let reformation_month = i64::from(REFORMATION_MONTH);
    if reform < year
        || (year == reform && reformation_month < month)
        || (year == reform && month == reformation_month && 13 < day)
    {
        // Truncating division and remainder, exactly as C's `/` and `%` are and
        // Rust's are; `div_euclid` here would be a different function.
        let n = year + year / 4 - year / 100 + year / 400 + reform_off + day;
        return i32::try_from(n % 7).unwrap_or(NONEDAY);
    }
    if year < reform
        || (year == reform && month < reformation_month)
        || (year == reform && month == reformation_month && day < 3)
    {
        let n = year + year / 4 + old_off + day;
        return i32::try_from(n % 7).unwrap_or(NONEDAY);
    }
    NONEDAY
}

/// `cal.c`'s `week_number`.
///
/// The recursion is bounded at depth two and the bound is worth stating, since
/// nothing in the shape of the function says so: the "last year is last year"
/// arm always recurses on 31 December, whose `yday` is at least 355, so
/// `yday + fday` cannot be under seven a second time; and the "part of the next
/// year" arm always recurses on 1 January, whose `yday` is 1, far below 363.
/// Neither arm can re-enter the other.
fn week_number(day: i32, mut month: i32, year: i32, ctl: &Ctl) -> i32 {
    let wday = day_in_week(ctl.reform_year, 1, 1, year);

    let mut fday = if ctl.weektype & WEEK_NUM_ISO != 0 {
        wday + if wday >= FRIDAY { -2 } else { 5 }
    } else {
        // WEEK_NUM_US: 1 January is always in the first week, which may begin in
        // the previous year — so there is very seldom a week 53.
        wday + 6
    };

    // For Julian dates the month can be set to January; the caller's `julian`
    // flag cannot be relied on here because the 31 December recursion below
    // would then be misread.
    if day > MAX_DAYS_IN_MONTH {
        month = 1;
    }

    let yday = day_in_year(ctl.reform_year, day, month, year);
    if year == ctl.reform_year && yday >= YDAY_AFTER_MISSING {
        fday -= NUMBER_MISSING_DAYS;
    }

    if yday + fday < i32::try_from(DAYS_IN_WEEK).unwrap_or(7) {
        return week_number(31, 12, year - 1, ctl);
    }

    // The equality is exact, not a mask test: a `--week=N` request ORs N into
    // the low byte, and upstream's check only fires when nothing was ORed in.
    if ctl.weektype == WEEK_NUM_ISO
        && yday >= 363
        && day_in_week(ctl.reform_year, day, month, year) >= MONDAY
        && day_in_week(ctl.reform_year, day, month, year) <= WEDNESDAY
        && day_in_week(ctl.reform_year, 31, 12, year) >= MONDAY
        && day_in_week(ctl.reform_year, 31, 12, year) <= WEDNESDAY
    {
        return week_number(1, 1, year + 1, ctl);
    }

    (yday + fday) / 7
}

/// `cal.c`'s `week_to_day`: the day-of-year `--week=N` starts at, or 1 if that
/// day falls in the previous year.
fn week_to_day(ctl: &Ctl) -> i32 {
    let wday = day_in_week(ctl.reform_year, 1, 1, ctl.req.year);
    let mut yday = ctl.req.week * 7 - wday;

    if ctl.req.year == ctl.reform_year && yday >= YDAY_AFTER_MISSING {
        yday += NUMBER_MISSING_DAYS;
    }

    if ctl.weektype & WEEK_NUM_ISO != 0 {
        yday -= if wday >= FRIDAY { -2 } else { 5 };
    } else {
        yday -= 6;
    }
    if yday <= 0 { 1 } else { yday }
}

/// Days since 1970-01-01 for a proleptic-Gregorian civil date.
///
/// Howard Hinnant's `days_from_civil`, which is what the C library's `mktime`
/// computes by a longer road. It is exact for every year in `i64` and has no
/// table, which matters here because the calendar's *own* date arithmetic is
/// deliberately not proleptic — this is only used to turn a parsed timestamp
/// into an epoch second, where the C library's rules, not `cal`'s, apply.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A broken-down local time, in the shape `strptime` fills in and `mktime`
/// reads. Fields are allowed out of range — `mktime` normalises them, which is
/// how `cal 2024-02-31` resolves to March.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BrokenDown {
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

/// `mktime` for a [`Zone`](localtime::Zone): civil local time to epoch seconds.
///
/// There is no inverse of `Zone::local` in the `localtime` crate, so this is it.
/// The offset depends on the instant and the instant depends on the offset, so
/// it is solved by iteration from a UTC guess; three rounds converge for every
/// real zone, since an offset change is never larger than a day and never
/// happens twice within one.
///
/// A local time that a spring-forward skipped does not exist, and this resolves
/// it to a nearby instant rather than failing — which is also what glibc does
/// with `tm_isdst = -1`.
fn mktime(zone: &localtime::Zone, tm: &mut BrokenDown) -> i64 {
    // Normalise the month first, so that `days_from_civil` sees 1..=12 and the
    // day-of-month overflow (31 February) is left for it to carry.
    let mut year = tm.year;
    let mut month = tm.month;
    year += (month - 1).div_euclid(12);
    month = (month - 1).rem_euclid(12) + 1;

    let days = days_from_civil(year, month, tm.day);
    let local_secs = days
        .saturating_mul(86_400)
        .saturating_add(tm.hour * 3_600)
        .saturating_add(tm.minute * 60)
        .saturating_add(tm.second);

    let mut t = local_secs;
    for _ in 0..3 {
        let off = i64::from(zone.lookup(t).gmtoff);
        let next = local_secs - off;
        if next == t {
            break;
        }
        t = next;
    }

    // Write the normalised civil fields back, the way `mktime` does, so that the
    // weekday check in `parse_timestamp` sees the resolved date rather than the
    // one that was typed.
    let resolved = zone.local(t, 0);
    tm.year = resolved.year;
    tm.month = i64::from(resolved.month);
    tm.day = i64::from(resolved.day);
    tm.hour = i64::from(resolved.hour);
    tm.minute = i64::from(resolved.minute);
    tm.second = i64::from(resolved.second);
    tm.wday = i32::try_from(resolved.wday).unwrap_or(0);
    t
}

// ------------------------------------------------------- numbers, as C's ---

/// Every diagnostic here is `errx`/`err` with `EXIT_FAILURE` and no referral to
/// `--help`, which is what makes them different from the getopt ones.
fn fail(message: String) -> Error {
    CAL.usage(message)
}

/// An operand as it should appear inside `'…'` in a diagnostic.
///
/// Upstream writes the bytes; this escapes the unprintable ones, for the reason
/// [`coreutils::getopt`] gives at length — a newline in an operand would
/// otherwise let it forge a second diagnostic line.
fn shown(s: &OsStr) -> String {
    escape_unprintable(&os_bytes(s))
}

/// What went wrong in a C string-to-number conversion, which is `errno` and is
/// observable: `ERANGE` reaches the user through `err()` and so carries
/// `: Numerical result out of range`, while `EINVAL` reaches it through
/// `errx()` and carries nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NumErr {
    Invalid,
    Range,
}

impl NumErr {
    /// `strerror` for the two values that get here.
    fn strerror(self) -> &'static str {
        match self {
            NumErr::Invalid => "Invalid argument",
            NumErr::Range => "Numerical result out of range",
        }
    }
}

fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The sign, magnitude and end index of a C integer conversion.
///
/// `magnitude` saturates rather than wrapping, and `saturated` says which
/// happened, because that is the difference between a value and `ERANGE`.
struct Scanned {
    negative: bool,
    magnitude: u128,
    saturated: bool,
    /// Index one past the last digit — C's `endptr`.
    end: usize,
}

/// The digit-scanning half of `strtoimax`/`strtoumax`, shared by both.
///
/// `base` is 10 or 0; 0 means C's "guess from the prefix" rule, which
/// [`parse_size`] relies on and which is why `cal -c 010` is eight columns.
/// Returns `None` for "no conversion performed", where C leaves `endptr` equal
/// to the input.
fn scan_integer(s: &[u8], base: u32) -> Option<Scanned> {
    let mut i = 0usize;
    while i < s.len() && c_isspace(s[i]) {
        i += 1;
    }
    let mut negative = false;
    if let Some(&c) = s.get(i)
        && (c == b'+' || c == b'-')
    {
        negative = c == b'-';
        i += 1;
    }

    let mut radix = base;
    if radix == 0 {
        if s.get(i) == Some(&b'0') {
            match s.get(i + 1) {
                Some(&b'x' | &b'X') if s.get(i + 2).is_some_and(u8::is_ascii_hexdigit) => {
                    radix = 16;
                    i += 2;
                }
                _ => radix = 8,
            }
        } else {
            radix = 10;
        }
    } else if radix == 16 && s.get(i) == Some(&b'0') && matches!(s.get(i + 1), Some(&b'x' | &b'X'))
    {
        i += 2;
    }

    let digits_start = i;
    // Above `SATURATE` the value cannot be represented in any type this program
    // converts to, so accumulation stops and only the flag matters.
    const SATURATE: u128 = 1 << 100;
    let mut magnitude: u128 = 0;
    let mut saturated = false;
    while let Some(&c) = s.get(i) {
        let Some(d) = (c as char).to_digit(radix) else {
            break;
        };
        if !saturated {
            magnitude = magnitude * u128::from(radix) + u128::from(d);
            if magnitude > SATURATE {
                saturated = true;
            }
        }
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    Some(Scanned {
        negative,
        magnitude,
        saturated,
        end: i,
    })
}

/// `ul_strtos64(str, &num, 10)` from `lib/strutils.c`.
///
/// The three refusals are C's, in C's order: an empty string is `EINVAL`;
/// `strtoimax` overflowing is `ERANGE`; and anything left over after the digits
/// — including nothing having been converted at all — is `EINVAL`.
fn ul_strtos64(s: &[u8]) -> Result<i64, NumErr> {
    if s.is_empty() {
        return Err(NumErr::Invalid);
    }
    let Some(sc) = scan_integer(s, 10) else {
        return Err(NumErr::Invalid);
    };
    let limit = if sc.negative {
        u128::from(i64::MAX.unsigned_abs()) + 1
    } else {
        u128::from(i64::MAX.unsigned_abs())
    };
    if sc.saturated || sc.magnitude > limit {
        return Err(NumErr::Range);
    }
    if sc.end != s.len() {
        return Err(NumErr::Invalid);
    }
    // `magnitude` is at most `i64::MAX + 1`, so both branches are exact in
    // `i128` and only the negative one can reach `i64::MIN`.
    let magnitude = i128::try_from(sc.magnitude).map_err(|_| NumErr::Range)?;
    let value = if sc.negative { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| NumErr::Range)
}

/// `ul_strtou64(str, &num, 10)`.
///
/// The odd shape is upstream's: it runs `strtoimax` first purely to reject a
/// leading `-`, then *clears* `errno` and re-runs `strtoumax`, which is why
/// `-n 10000000000000000000` is a range error from the bound check rather than
/// from the first conversion.
fn ul_strtou64(s: &[u8]) -> Result<u64, NumErr> {
    if s.is_empty() {
        return Err(NumErr::Invalid);
    }
    let Some(sc) = scan_integer(s, 10) else {
        return Err(NumErr::Invalid);
    };
    if sc.negative && (sc.saturated || sc.magnitude != 0) {
        return Err(NumErr::Range);
    }
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return Err(NumErr::Range);
    }
    if sc.end != s.len() {
        return Err(NumErr::Invalid);
    }
    u64::try_from(sc.magnitude).map_err(|_| NumErr::Range)
}

/// The message `str2num_or_err` produces for a failed conversion.
///
/// It ends `goto err`, and `err:` chooses between `err()` and `errx()` on
/// `errno == ERANGE` — so only the range case carries a `strerror`. Compare
/// [`size_error`], which is the same sentence with the opposite rule.
fn num_error(msg: &str, arg: &OsStr, e: NumErr) -> Error {
    match e {
        NumErr::Invalid => fail(format!("{msg}: '{}'", shown(arg))),
        NumErr::Range => fail(format!(
            "{msg}: '{}': {}",
            shown(arg),
            NumErr::Range.strerror()
        )),
    }
}

/// The message `strtosize_or_err` produces for a failed conversion.
///
/// Its test is `if (errno) err(...)` rather than `if (errno == ERANGE)`, and
/// `parse_size` always sets one, so **both** failures carry a `strerror` here:
/// `cal -c abc` says `: Invalid argument` where `cal -n abc` says nothing. The
/// two sentences are otherwise identical, which is why the difference is worth
/// a function of its own rather than a flag.
fn size_error(msg: &str, arg: &OsStr, e: NumErr) -> Error {
    fail(format!("{msg}: '{}': {}", shown(arg), e.strerror()))
}

/// `str2num_or_err`, of which `strtos32_or_err` is the `INT32_MIN..=INT32_MAX`
/// case.
///
/// A bound violation sets `errno = ERANGE` just as an overflow does, which is
/// why `cal 1 99999999999` — a number that fits in `int64_t` perfectly well —
/// still says `Numerical result out of range`.
fn strtos32_or_err(arg: &OsStr, msg: &str) -> Result<i32, Error> {
    let bytes = os_bytes(arg);
    match ul_strtos64(&bytes) {
        Ok(n) => i32::try_from(n).map_err(|_| num_error(msg, arg, NumErr::Range)),
        Err(e) => Err(num_error(msg, arg, e)),
    }
}

/// `strtou32_or_err`. Its `uint32_t` result is stored into an `int` by `cal`'s
/// `-n` handler, so `-n 4294967295` becomes `-1` months and prints nothing.
fn strtou32_or_err(arg: &OsStr, msg: &str) -> Result<u32, Error> {
    let bytes = os_bytes(arg);
    match ul_strtou64(&bytes) {
        Ok(n) => u32::try_from(n).map_err(|_| num_error(msg, arg, NumErr::Range)),
        Err(e) => Err(num_error(msg, arg, e)),
    }
}

/// `do_scale_by_power`: multiply by `base` `power` times, refusing to wrap.
fn do_scale_by_power(x: &mut u64, base: u64, power: i32) -> Result<(), NumErr> {
    for _ in 0..power {
        if u64::MAX / base < *x {
            return Err(NumErr::Range);
        }
        *x *= base;
    }
    Ok(())
}

/// `parse_size` from `lib/strutils.c`, fractions and all.
///
/// `cal` uses it for one thing — `-c` — and could have used a plain integer
/// parse, but then `cal -c 1.5K` would be an error where upstream accepts 1536.
/// The decimal-point branch is the whole reason this is 60 lines rather than 6.
///
/// Note the base: the leading conversion is `strtoumax(str, &end, 0)`, so `010`
/// is eight and `0x10` is sixteen.
fn parse_size(s: &[u8]) -> Result<u64, NumErr> {
    if s.is_empty() {
        return Err(NumErr::Invalid);
    }

    // Only positive numbers are acceptable. The check is on the first
    // non-blank byte, while the conversion below still starts at the front.
    let mut lead = 0usize;
    while lead < s.len() && c_isspace(s[lead]) {
        lead += 1;
    }
    if s.get(lead) == Some(&b'-') {
        return Err(NumErr::Invalid);
    }

    let Some(sc) = scan_integer(s, 0) else {
        return Err(NumErr::Invalid);
    };
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return Err(NumErr::Range);
    }
    let mut x = u64::try_from(sc.magnitude).map_err(|_| NumErr::Range)?;
    let mut p = sc.end;
    if p >= s.len() {
        return Ok(x); // without suffix
    }

    let mut base: u64 = 1024;
    let mut frac: u64 = 0;
    let mut frac_zeros = 0i32;

    // `check_suffix:`, which the decimal-point branch jumps back to.
    let at = |i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    loop {
        if at(p + 1) == b'i' && (at(p + 2) == b'B' || at(p + 2) == b'b') && at(p + 3) == 0 {
            base = 1024; // XiB, 2^N
        } else if (at(p + 1) == b'B' || at(p + 1) == b'b') && at(p + 2) == 0 {
            base = 1000; // XB, 10^N
        } else if at(p + 1) != 0 {
            // The C locale's decimal point is `.` and is one byte long.
            if frac != 0 || at(p) != b'.' {
                return Err(NumErr::Invalid); // unexpected suffix
            }
            let mut fstr = p + 1;
            while at(fstr) == b'0' {
                frac_zeros += 1;
                fstr += 1;
            }
            let end = if at(fstr).is_ascii_digit() {
                let Some(fsc) = scan_integer(&s[fstr..], 0) else {
                    return Err(NumErr::Invalid);
                };
                if fsc.saturated || fsc.magnitude > u128::from(u64::MAX) {
                    return Err(NumErr::Range);
                }
                frac = u64::try_from(fsc.magnitude).map_err(|_| NumErr::Range)?;
                fstr + fsc.end
            } else {
                fstr
            };
            if frac != 0 && end >= s.len() {
                return Err(NumErr::Invalid); // a fraction with no suffix
            }
            p = end;
            continue;
        }
        break;
    }

    const SUF: &[u8] = b"KMGTPEZY";
    const SUF2: &[u8] = b"kmgtpezy";
    let here = at(p);
    let pwr = if let Some(i) = SUF.iter().position(|&c| c == here && c != 0) {
        i32::try_from(i).unwrap_or(0) + 1
    } else if let Some(i) = SUF2.iter().position(|&c| c == here && c != 0) {
        i32::try_from(i).unwrap_or(0) + 1
    } else {
        return Err(NumErr::Invalid);
    };

    let scaled = do_scale_by_power(&mut x, base, pwr);

    if frac != 0 && pwr != 0 {
        let mut frac_div: u64 = 10;
        let mut frac_poz: u64 = 1;
        let mut frac_base: u64 = 1;
        // Its overflow is discarded upstream, and so is it here.
        let _ = do_scale_by_power(&mut frac_base, base, pwr);

        // The divisor for the last digit: 100 for 0.05, 1000 for 0.054.
        while frac_div < frac {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }
        for _ in 0..frac_zeros {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }

        // Walk the fraction backwards from its last digit, adding what each
        // digit is worth in `frac_base`.
        loop {
            let seg = frac % 10;
            let seg_div = frac_div / frac_poz;
            frac /= 10;
            frac_poz = frac_poz.saturating_mul(10);
            if seg != 0 && seg_div / seg != 0 {
                x = x.saturating_add(frac_base / (seg_div / seg));
            }
            if frac == 0 {
                break;
            }
        }
    }

    // `parse_size` writes the (possibly overflowed) result out and *then*
    // returns the error, so a caller that ignored the status would still see a
    // number. `strtosize_or_err` does not ignore it.
    scaled.map(|()| x)
}

/// `parse_reform_year`, whose table is matched case-insensitively.
fn parse_reform_year(arg: &OsStr) -> Result<i32, Error> {
    const TABLE: [(&str, i32); 4] = [
        ("gregorian", GREGORIAN),
        ("iso", ISO_REFORM),
        ("1752", GB1752),
        ("julian", JULIAN),
    ];
    let bytes = os_bytes(arg);
    for (name, val) in TABLE {
        if bytes.eq_ignore_ascii_case(name.as_bytes()) {
            return Ok(val);
        }
    }
    Err(fail(format!("invalid --reform value: '{}'", shown(arg))))
}

/// `colormode_or_err`. An absent value means `auto`; an empty one is an error,
/// because `colormode_from_string` rejects `""` before it reaches the table.
fn colormode_or_err(arg: &OsStr) -> Result<ColorMode, Error> {
    let bytes = os_bytes(arg);
    if bytes.eq_ignore_ascii_case(b"auto") {
        Ok(ColorMode::Auto)
    } else if bytes.eq_ignore_ascii_case(b"never") {
        Ok(ColorMode::Never)
    } else if bytes.eq_ignore_ascii_case(b"always") {
        Ok(ColorMode::Always)
    } else {
        Err(fail(format!("unsupported color mode: '{}'", shown(arg))))
    }
}

/// `isdigit_string`: non-empty, and every byte an ASCII digit.
///
/// This is what decides whether a lone operand is read as a year or handed to
/// the timestamp parser, and it is why `cal 20240215` is the year 20240215
/// while `cal 2024-02-15` is a date. Note that `" 5"` is *not* a digit string,
/// so it goes to the timestamp parser and is refused there.
fn isdigit_string(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}

// ------------------------------------------------ timestamps, as C's again ---

const USEC_PER_SEC: u64 = 1_000_000;
const USEC_PER_MSEC: u64 = 1_000;
const USEC_PER_MINUTE: u64 = 60 * USEC_PER_SEC;
const USEC_PER_HOUR: u64 = 60 * USEC_PER_MINUTE;
const USEC_PER_DAY: u64 = 24 * USEC_PER_HOUR;
const USEC_PER_WEEK: u64 = 7 * USEC_PER_DAY;
/// A "month" is a *mean* month — 30.4375 days — not a calendar one, so
/// `cal +1month` in a 31-day month can land in the month after next.
const USEC_PER_MONTH: u64 = 2_629_800 * USEC_PER_SEC;
/// Likewise a mean Julian year, 365.25 days.
const USEC_PER_YEAR: u64 = 31_557_600 * USEC_PER_SEC;

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
fn parse_sec(t: &[u8]) -> Option<u64> {
    let mut r: u64 = 0;
    let mut something = false;
    let mut p = 0usize;

    loop {
        while t.get(p).is_some_and(|c| TIMEUTILS_WHITESPACE.contains(c)) {
            p += 1;
        }
        if p >= t.len() {
            return if something { Some(r) } else { None };
        }

        // `strtoll(p, &e, 10)`, whose sign is accepted and then refused.
        let scanned = scan_integer(&t[p..], 10);
        let (l, mut e) = match &scanned {
            Some(sc) => {
                if sc.saturated {
                    return None; // ERANGE
                }
                if sc.negative && sc.magnitude != 0 {
                    return None; // ERANGE
                }
                (u64::try_from(sc.magnitude).ok()?, p + sc.end)
            }
            None => (0u64, p),
        };

        let mut z: u64 = 0;
        let mut n = 0usize;
        if t.get(e) == Some(&b'.') {
            let b = e + 1;
            let frac = scan_integer(&t[b..], 10)?;
            if frac.saturated || frac.negative {
                return None;
            }
            z = u64::try_from(frac.magnitude).ok()?;
            e = b + frac.end;
            n = e - b;
        } else if e == p {
            return None; // no digits and no decimal point
        }

        while t.get(e).is_some_and(|c| TIMEUTILS_WHITESPACE.contains(c)) {
            e += 1;
        }

        let mut matched = false;
        for (suffix, usec) in SEC_TABLE {
            if !t[e..].starts_with(suffix.as_bytes()) {
                continue;
            }
            let mut k = z.saturating_mul(*usec);
            for _ in 0..n {
                k /= 10;
            }
            r = r.saturating_add(l.saturating_mul(*usec)).saturating_add(k);
            p = e + suffix.len();
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
    for &c in &t[1..] {
        if !c.is_ascii_digit() || factor < 1 {
            return None;
        }
        ret += u64::from(c - b'0') * factor;
        factor /= 10;
    }
    Some(ret)
}

/// glibc's `get_number` macro: skip *spaces* (not all whitespace), then read at
/// most `n` digits, stopping early once another digit could not fit under `to`.
fn get_number(s: &[u8], rp: &mut usize, from: i64, to: i64, n: u32) -> Option<i64> {
    while s.get(*rp) == Some(&b' ') {
        *rp += 1;
    }
    if !s.get(*rp).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut val: i64 = 0;
    let mut left = n;
    loop {
        let d = i64::from(s.get(*rp)?.wrapping_sub(b'0'));
        val = val.checked_mul(10)?.checked_add(d)?;
        *rp += 1;
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
        let fc = fmt[fi];
        if c_isspace(fc) {
            while s.get(rp).is_some_and(|c| c_isspace(*c)) {
                rp += 1;
            }
            fi += 1;
            continue;
        }
        if fc != b'%' {
            if s.get(rp) != Some(&fc) {
                return None;
            }
            rp += 1;
            fi += 1;
            continue;
        }
        fi += 1;
        let spec = *fmt.get(fi)?;
        fi += 1;

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
                secs = secs.saturating_mul(10).saturating_add(i64::from(c - b'0'));
                rp += 1;
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
            b'y' => tm.year = if val >= 69 { 1900 + val } else { 2000 + val },
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
fn broken_down(zone: &localtime::Zone, t: i64) -> BrokenDown {
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
    s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p)
}

/// `parse_timestamp_reference`: everything `cal <timestamp>` accepts.
///
/// The result is micro-seconds in util-linux's `usec_t`, which is **unsigned**.
/// A date before 1970 therefore comes back as an enormous positive number
/// rather than a negative one, and `cal` divides it by a million and asks
/// `localtime` about the result — so `cal 1960-01-01` names a year in the far
/// future. That is upstream's arithmetic, wrapping and all, and it is
/// reproduced rather than corrected.
fn parse_timestamp(zone: &localtime::Zone, reference: i64, t: &[u8]) -> Option<u64> {
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
        tm.day -= 1;
        tm.second = 0;
        tm.minute = 0;
        tm.hour = 0;
    } else if t == b"tomorrow" {
        tm.day += 1;
        tm.second = 0;
        tm.minute = 0;
        tm.hour = 0;
    } else if t.first() == Some(&b'+') {
        plus = parse_sec(&t[1..])?;
    } else if t.first() == Some(&b'-') {
        minus = parse_sec(&t[1..])?;
    } else if t.first() == Some(&b'@') {
        let k = strptime(&t[1..], "%s", &mut tm, zone)?;
        let rest = &t[1 + k..];
        if !rest.is_empty() {
            ret = parse_subseconds(rest)?;
        }
    } else if t.ends_with(b" ago") {
        minus = parse_sec(&t[..t.len() - 4])?;
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
            s = &s[name.len() + 1..];
            break;
        }

        let copy = tm;
        let mut matched = false;
        for (fmt, after, subsec) in TIMESTAMP_FORMATS {
            tm = copy;
            let Some(k) = strptime(s, fmt, &mut tm, zone) else {
                continue;
            };
            let rest = &s[k..];
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

    let x = mktime(zone, &mut tm);
    if weekday >= 0 && tm.wday != weekday {
        return None;
    }
    ret = ret.wrapping_add((x as u64).wrapping_mul(USEC_PER_SEC));
    ret = ret.wrapping_add(plus);
    Some(ret.saturating_sub(minus))
}

/// `monthname_to_number`: full names first, then abbreviations, both matched
/// case-insensitively. `-EINVAL` becomes `None`.
fn monthname_to_number(name: &[u8]) -> Option<i32> {
    for (i, m) in FULL_MONTH.iter().enumerate() {
        if name.eq_ignore_ascii_case(m.as_bytes()) {
            return i32::try_from(i + 1).ok();
        }
    }
    for (i, m) in ABBR_MONTH.iter().enumerate() {
        if name.eq_ignore_ascii_case(m.as_bytes()) {
            return i32::try_from(i + 1).ok();
        }
    }
    None
}

// ---------------------------------------------------------- the rendering ---

/// `MONTHS_IN_YEAR` at `i32` width. Upstream spells the same constant
/// `DECEMBER` where it is a month number and `MONTHS_IN_YEAR` where it is a
/// count; the two are the same 12 and it uses them interchangeably.
const DECEMBER: i32 = 12;

/// Rows in a month grid — `MAXDAYS / DAYS_IN_WEEK`.
const WEEK_LINES: usize = MAXDAYS / DAYS_IN_WEEK;

/// `printf("%*s", n, "")`.
fn pad(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

/// `printf("%*d", width, value)`.
///
/// The `Result` is dropped because `impl fmt::Write for String` has no failure
/// mode: its only `Err` path is an allocation failure, which aborts instead.
fn num(out: &mut String, value: i32, width: usize) {
    let _ = write!(out, "{value:>width$}");
}

/// A colour escape, or nothing when colouring is off.
///
/// Upstream's `cal_get_color_sequence` returns `""` when `colors_init` decided
/// against colour, and the surrounding `printf` widths are chosen so that the
/// layout does not depend on the sequence being present. That holds here too,
/// so this only ever adds or removes zero-width bytes.
fn seq(ctl: &Ctl, escape: &'static str) -> &'static str {
    if ctl.colors { escape } else { "" }
}

#[derive(Clone, Copy, Debug)]
enum Align {
    Center,
    Left,
}

/// `sizeof(lineout)` in `center` and `left` — `cal.c`'s `FMT_ST_CHARS`.
const FMT_ST_CHARS: usize = 300;

/// `mbsalign` reduced to the C locale, where one byte is one column.
///
/// Everything `cal` aligns is ASCII — month names, weekday abbreviations and
/// `%04d` years — so the multibyte half of the original is unreachable and the
/// `MBA_UNIBYTE_FALLBACK` path is the only one that runs. `str::get` rather
/// than a slice keeps that assumption from becoming a panic if it is ever
/// false: a cut that would land inside a character declines to cut at all.
///
/// `dest_size` is the C buffer's size and it **is** observable, so it is
/// reproduced rather than assumed away. `mbsalign` writes at most
/// `dest_size - 1` bytes and NUL-terminates, and the caller then `fputs`es
/// whatever survived. `cal -c 1K -y 2024` asks for a year heading 23549 columns
/// wide and gets a line of 299 spaces: the "2024" it was centring sits far
/// past the end of the buffer and is never written at all. Measured.
fn mbsalign(out: &mut String, s: &str, width: usize, align: Align, dest_size: usize) {
    let shown = if s.len() > width {
        s.get(..width).unwrap_or(s)
    } else {
        s
    };
    let n_spaces = width.saturating_sub(shown.len());
    let (start, end) = match align {
        // The odd space goes on the left, which is why "February 2024" sits
        // four columns in from a 20-wide month and three from its right edge.
        Align::Center => (n_spaces / 2 + n_spaces % 2, n_spaces / 2),
        Align::Left => (0, n_spaces),
    };

    // One byte of the buffer belongs to the terminator.
    let mut room = dest_size.saturating_sub(1);

    let n = start.min(room);
    pad(out, n);
    room -= n;

    let n = shown.len().min(room);
    // Unreachable on a non-ASCII cut, as above; declining to write is the safe
    // answer if the assumption is ever broken.
    out.push_str(shown.get(..n).unwrap_or(""));
    room -= n;

    pad(out, end.min(room));
}

/// `cal.c`'s `center`: centre `s` in `width`, then `separate` spaces if
/// `separate` is non-zero.
///
/// The `separate` spaces come from a second `printf` and so are *not* subject
/// to the buffer limit above.
fn center(out: &mut String, s: &str, width: usize, separate: usize) {
    mbsalign(out, s, width, Align::Center, FMT_ST_CHARS);
    if separate != 0 {
        pad(out, separate);
    }
}

/// `cal.c`'s `left`, the same with the padding all on the right.
fn left(out: &mut String, s: &str, width: usize, separate: usize) {
    mbsalign(out, s, width, Align::Left, FMT_ST_CHARS);
    if separate != 0 {
        pad(out, separate);
    }
}

/// `FULL_MONTH` indexed by a 1-based month number.
fn month_name(month: i32) -> &'static str {
    usize::try_from(month)
        .ok()
        .and_then(|m| m.checked_sub(1))
        .and_then(|m| FULL_MONTH.get(m).copied())
        .unwrap_or("")
}

/// `cal.c`'s `weekdays_init`: the abbreviated day names rotated so that index 0
/// is the first day of the week.
fn weekdays_init(ctl: &Ctl) -> [&'static str; DAYS_IN_WEEK] {
    let start = usize::try_from(ctl.weekstart).unwrap_or(0);
    let mut wd = [""; DAYS_IN_WEEK];
    for (i, slot) in wd.iter_mut().enumerate() {
        *slot = ABDAY.get((i + start) % DAYS_IN_WEEK).copied().unwrap_or("");
    }
    wd
}

/// `cal.c`'s `headers_init`: build the day-of-week heading row, and decide
/// whether the month name and the year fit on one line.
fn headers_init(ctl: &mut Ctl, weekdays: &[&str; DAYS_IN_WEEK]) {
    /// `sizeof(day_headings)` — `(WEEK_LEN + 1) * 6 + 1`, with `WEEK_LEN` being
    /// `DAYS_IN_WEEK * DAY_LEN`.
    const DAY_HEADINGS_SIZE: usize = (DAYS_IN_WEEK * 3 + 1) * 6 + 1;

    let year_len = format!("{:04}", ctl.req.year).len();

    let mut dh = String::new();
    for (i, name) in weekdays.iter().enumerate() {
        if i != 0 {
            dh.push(' ');
        }
        // Upstream's guard against overrunning a 133-byte buffer. The widest
        // heading row `cal` can build is 4*7 - 1 = 27 bytes, so this never
        // fires; it is kept because dropping it would be a silent divergence.
        let space_left = DAY_HEADINGS_SIZE - dh.len();
        // Upstream spells this `space_left <= ctl->day_width - 1`; the two are
        // the same test for every `day_width` cal can produce (3 or 4).
        if space_left < ctl.day_width {
            break;
        }
        mbsalign(&mut dh, name, ctl.day_width - 1, Align::Center, space_left);
    }
    ctl.day_headings = dh;

    // The `+ 1` for the space between name and year that upstream's comment
    // promises is not in upstream's code; this reproduces the code.
    for m in FULL_MONTH {
        if ctl.week_width < m.len() + year_len {
            ctl.header_hint = true;
        }
    }
}

/// `cal.c`'s `cal_fill_month`: lay one month out into a 6x7 grid.
fn cal_fill_month(month: &mut CalMonth, ctl: &Ctl) {
    let mut first_week_day = day_in_week(ctl.reform_year, 1, month.month, month.year);

    let mut j = if ctl.julian {
        day_in_year(ctl.reform_year, 1, month.month, month.year)
    } else {
        1
    };
    let mut month_days = j + month_length(ctl.reform_year, month.month, month.year);

    // True when Sunday is not the first day in the output week.
    if ctl.weekstart != 0 {
        first_week_day -= ctl.weekstart;
        if first_week_day < 0 {
            first_week_day = 7 - ctl.weekstart;
        }
        month_days += ctl.weekstart - 1;
    }

    let mut blank_lines = 0i32;
    for slot in &mut month.days {
        if 0 < first_week_day {
            *slot = SPACE;
            first_week_day -= 1;
            continue;
        }
        if j < month_days {
            // The reform's eleven missing days, skipped in whichever numbering
            // is in force: 3 September by day-of-month, 247 by day-of-year.
            if month.year == ctl.reform_year
                && month.month == REFORMATION_MONTH
                && (j == 3 || j == 247)
            {
                j += NUMBER_MISSING_DAYS;
            }
            *slot = j;
            j += 1;
            continue;
        }
        *slot = SPACE;
        blank_lines += 1;
    }

    if ctl.weektype != WEEK_NUM_DISABLED {
        let mut weeknum = week_number(1, month.month, month.year, ctl);
        // How many of the six rows hold at least one day.
        let mut weeklines = 6 - blank_lines / 7;
        for i in 0..WEEK_LINES {
            if 0 < weeklines {
                if 52 < weeknum {
                    // A December that spills into week 1 of the next year. The
                    // day handed over may be SPACE (-1) when the row starts in
                    // the previous month; upstream passes it unchanged and so
                    // does this, because `week_number` treats it as a
                    // day-of-year offset and the answer still lands in range.
                    let d = month.days.get(i * DAYS_IN_WEEK).copied().unwrap_or(SPACE);
                    weeknum = week_number(d, month.month, month.year, ctl);
                }
                if let Some(w) = month.weeks.get_mut(i) {
                    *w = weeknum;
                }
                weeknum += 1;
            } else if let Some(w) = month.weeks.get_mut(i) {
                *w = SPACE;
            }
            weeklines -= 1;
        }
    }
}

/// `cal.c`'s `cal_output_header`: one or two title lines, then the day-of-week
/// headings.
fn cal_output_header(out: &mut String, months: &[CalMonth], ctl: &Ctl) {
    let last = months.len().saturating_sub(1);
    let gutter = |k: usize| if k == last { 0 } else { ctl.gutter_width };

    if ctl.header_hint || ctl.header_year {
        for (k, m) in months.iter().enumerate() {
            center(out, month_name(m.month), ctl.week_width, gutter(k));
        }
        if !ctl.header_year {
            out.push('\n');
            for (k, m) in months.iter().enumerate() {
                center(out, &format!("{:04}", m.year), ctl.week_width, gutter(k));
            }
        }
    } else {
        for (k, m) in months.iter().enumerate() {
            let title = format!("{} {:04}", month_name(m.month), m.year);
            center(out, &title, ctl.week_width, gutter(k));
        }
    }
    out.push('\n');

    for k in 0..months.len() {
        if ctl.weektype != WEEK_NUM_DISABLED {
            // Room for the week-number column. One narrower under -j, because
            // the julian day column is itself one wider.
            pad(
                out,
                if ctl.julian {
                    ctl.day_width - 1
                } else {
                    ctl.day_width
                },
            );
        }
        out.push_str(&ctl.day_headings);
        if k != last {
            pad(out, ctl.gutter_width);
        }
    }
    out.push('\n');
}

/// The day the request asked to highlight, expressed as this month's own day
/// number — `req.day` is a day of the *year*.
fn highlighted_day(m: &CalMonth, ctl: &Ctl) -> i32 {
    if m.month != ctl.req.month || m.year != ctl.req.year {
        return 0;
    }
    if ctl.julian {
        ctl.req.day
    } else {
        ctl.req.day + 1 - day_in_year(ctl.reform_year, 1, m.month, m.year)
    }
}

/// `cal.c`'s `cal_output_months`: six rows of days, however few of them are
/// occupied.
fn cal_output_months(out: &mut String, months: &[CalMonth], ctl: &Ctl) {
    let narrow = if ctl.julian { 3 } else { 2 };
    let last = months.len().saturating_sub(1);

    for week_line in 0..WEEK_LINES {
        for (k, m) in months.iter().enumerate() {
            let reqday = highlighted_day(m, ctl);

            let mut skip = if ctl.weektype != WEEK_NUM_DISABLED {
                let w = m.weeks.get(week_line).copied().unwrap_or(SPACE);
                if 0 < w {
                    if u32::try_from(w).is_ok_and(|w| ctl.weektype & WEEK_NUM_MASK == w) {
                        out.push_str(seq(ctl, HIGHLIGHT));
                        num(out, w, 2);
                        out.push_str(seq(ctl, RESET));
                    } else {
                        num(out, w, 2);
                    }
                } else {
                    pad(out, 2);
                }
                ctl.day_width
            } else {
                // With no week-number column there is no leading space, so the
                // first day of the row is one column narrower than the rest.
                ctl.day_width - 1
            };

            for d in DAYS_IN_WEEK * week_line..DAYS_IN_WEEK * week_line + DAYS_IN_WEEK {
                let day = m.days.get(d).copied().unwrap_or(SPACE);
                if 0 < day {
                    if reqday == day {
                        // The escape goes between the padding and the number so
                        // that only the number is reversed.
                        pad(out, skip.saturating_sub(narrow));
                        out.push_str(seq(ctl, HIGHLIGHT));
                        num(out, day, narrow);
                        out.push_str(seq(ctl, RESET));
                    } else {
                        num(out, day, skip);
                    }
                } else {
                    pad(out, skip);
                }
                if skip < ctl.day_width {
                    skip += 1;
                }
            }
            if k != last {
                pad(out, ctl.gutter_width);
            }
        }
        out.push('\n');
    }
}

/// `cal.c`'s `cal_vert_output_header`.
///
/// The gutter is appended after *every* month including the last, which leaves
/// trailing spaces the horizontal header does not. That is upstream's
/// behaviour, byte for byte, and is reproduced deliberately.
fn cal_vert_output_header(out: &mut String, months: &[CalMonth], ctl: &Ctl) {
    let month_width = ctl.day_width * WEEK_LINES;

    // Room for the weekday labels down the left edge.
    pad(out, ctl.day_width + 1);

    if ctl.header_hint || ctl.header_year {
        for m in months {
            left(out, month_name(m.month), month_width, ctl.gutter_width);
        }
        if !ctl.header_year {
            out.push('\n');
            pad(out, ctl.day_width + 1);
            for m in months {
                left(
                    out,
                    &format!("{:04}", m.year),
                    month_width,
                    ctl.gutter_width,
                );
            }
        }
    } else {
        for m in months {
            let title = format!("{} {:04}", month_name(m.month), m.year);
            left(out, &title, month_width, ctl.gutter_width);
        }
    }
    out.push('\n');
}

/// `cal.c`'s `cal_vert_output_months`: seven rows, one per weekday.
///
/// Upstream's `skip` is assigned `day_width` before the loop and again at the
/// end of every innermost iteration, so it is constant; it is a variable there
/// only because the horizontal renderer's is. Keeping it constant here is not a
/// simplification — it is what the original computes.
///
/// The highlighted week number is printed at width `skip - (julian ? 3 : 2)`
/// rather than `skip`, which puts it one column left of its unhighlighted
/// neighbours. That is an upstream bug; it is reproduced so that the two
/// programs agree byte for byte.
fn cal_vert_output_months(
    out: &mut String,
    months: &[CalMonth],
    ctl: &Ctl,
    weekdays: &[&str; DAYS_IN_WEEK],
) {
    let narrow = if ctl.julian { 3 } else { 2 };
    let skip = ctl.day_width;
    let last = months.len().saturating_sub(1);

    for (i, wd) in weekdays.iter().enumerate() {
        left(out, wd, ctl.day_width - 1, 0);
        for (k, m) in months.iter().enumerate() {
            let reqday = highlighted_day(m, ctl);
            for week in 0..WEEK_LINES {
                let day = m
                    .days
                    .get(i + DAYS_IN_WEEK * week)
                    .copied()
                    .unwrap_or(SPACE);
                if 0 < day {
                    if reqday == day {
                        pad(out, skip.saturating_sub(narrow));
                        out.push_str(seq(ctl, HIGHLIGHT));
                        num(out, day, narrow);
                        out.push_str(seq(ctl, RESET));
                    } else {
                        num(out, day, skip);
                    }
                } else {
                    pad(out, skip);
                }
            }
            if k != last {
                pad(out, ctl.gutter_width);
            }
        }
        out.push('\n');
    }

    if ctl.weektype == WEEK_NUM_DISABLED {
        return;
    }

    pad(out, ctl.day_width - 1);
    for (k, m) in months.iter().enumerate() {
        for week in 0..WEEK_LINES {
            let w = m.weeks.get(week).copied().unwrap_or(SPACE);
            if 0 < w {
                if u32::try_from(w).is_ok_and(|w| ctl.weektype & WEEK_NUM_MASK == w) {
                    out.push_str(seq(ctl, HIGHLIGHT));
                    num(out, w, skip.saturating_sub(narrow));
                    out.push_str(seq(ctl, RESET));
                } else {
                    num(out, w, skip);
                }
            } else {
                pad(out, skip);
            }
        }
        if k != last {
            pad(out, ctl.gutter_width);
        }
    }
    out.push('\n');
}

/// `cal.c`'s `monthly`: emit `num_months` months, `months_in_row` at a time.
fn monthly(out: &mut String, ctl: &Ctl, weekdays: &[&str; DAYS_IN_WEEK]) {
    let mut month = if ctl.req.start_month != 0 {
        ctl.req.start_month
    } else {
        ctl.req.month
    };
    let mut year = ctl.req.year;

    // `cal -3`, `cal -Y --span`: centre the run on the requested month.
    if ctl.span_months {
        let mut new_month = month - ctl.num_months / 2;
        if new_month < 1 {
            new_month = -new_month;
            year -= new_month / DECEMBER + 1;
            if new_month > DECEMBER {
                new_month %= DECEMBER;
            }
            month = DECEMBER - new_month;
        } else {
            month = new_month;
        }
    }

    // `main` guarantees at least one; a zero would make `rows` a division by
    // zero rather than an empty calendar.
    if ctl.months_in_row < 1 {
        return;
    }

    let rows = (ctl.num_months - 1) / ctl.months_in_row;
    let remainder = ctl.num_months % ctl.months_in_row;
    let mut i = 0;
    while i <= rows {
        // Upstream shortens its fixed-size month list in place for the last
        // row; building each row to length is the same calendar without the
        // `xcalloc` that `--columns 1K` would otherwise size at 1024 months.
        let in_row = if i == rows && remainder > 0 {
            remainder
        } else {
            ctl.months_in_row
        };

        let mut ms = Vec::with_capacity(usize::try_from(in_row).unwrap_or(0));
        for _ in 0..in_row {
            let mut m = CalMonth {
                days: [SPACE; MAXDAYS],
                weeks: [SPACE; WEEK_LINES],
                month,
                year,
            };
            month += 1;
            if DECEMBER < month {
                year += 1;
                month = 1;
            }
            cal_fill_month(&mut m, ctl);
            ms.push(m);
        }

        if ctl.vertical {
            if i > 0 {
                // A blank line between rows of months.
                out.push('\n');
            }
            cal_vert_output_header(out, &ms, ctl);
            cal_vert_output_months(out, &ms, ctl, weekdays);
        } else {
            cal_output_header(out, &ms, ctl);
            cal_output_months(out, &ms, ctl);
        }
        i += 1;
    }
}

/// `cal.c`'s `yearly`: the year number over a centred block of months.
fn yearly(out: &mut String, ctl: &Ctl, weekdays: &[&str; DAYS_IN_WEEK]) {
    let in_row = usize::try_from(ctl.months_in_row).unwrap_or(0);
    // `saturating_sub` where upstream has `(size_t)months_in_row - 1`: at zero
    // months per row that expression is `SIZE_MAX`, which `main` prevents but
    // this function should not depend on.
    let year_width = in_row * ctl.week_width + in_row.saturating_sub(1) * ctl.gutter_width;

    if ctl.header_year {
        center(out, &format!("{:04}", ctl.req.year), year_width, 0);
        out.push_str("\n\n");
    }
    monthly(out, ctl, weekdays);
}

// --------------------------------------------------------------- the help ---

/// `usage()`, which upstream writes to **stdout** and exits 0.
///
/// The leading blank line, the blank line before `-h, --help`, and the blank
/// line before the closing sentence are all upstream's; `--help` is compared
/// byte for byte against it in the tests below.
fn help_text() -> String {
    let mut text = String::new();
    text.push('\n');
    text.push_str("Usage:\n");
    text.push_str(" cal [options] [[[day] month] year]\n");
    text.push_str(" cal [options] <timestamp|monthname>\n");
    text.push('\n');
    text.push_str("Display a calendar, or some part of it.\n");
    text.push_str("Without any arguments, display the current month.\n");
    text.push('\n');
    text.push_str("Options:\n");
    text.push_str(" -1, --one             show only a single month (default)\n");
    text.push_str(" -3, --three           show three months spanning the date\n");
    text.push_str(" -n, --months <num>    show num months starting with date's month\n");
    text.push_str(" -S, --span            span the date when displaying multiple months\n");
    text.push_str(" -s, --sunday          Sunday as first day of week\n");
    text.push_str(" -m, --monday          Monday as first day of week\n");
    text.push_str(" -j, --julian          use day-of-year for all calendars\n");
    text.push_str("     --reform <val>    Gregorian reform date (1752|gregorian|iso|julian)\n");
    text.push_str("     --iso             alias for --reform=iso\n");
    text.push_str(" -y, --year            show the whole year\n");
    text.push_str(" -Y, --twelve          show the next twelve months\n");
    text.push_str(" -w, --week[=<num>]    show US or ISO-8601 week numbers\n");
    text.push_str(" -v, --vertical        show day vertically instead of line\n");
    text.push_str(" -c, --columns <width> amount of columns to use\n");
    text.push_str("     --color[=<when>]  colorize messages (auto, always or never)\n");
    text.push_str("                         colors are enabled by default\n");
    text.push('\n');
    text.push_str(" -h, --help            display this help\n");
    text.push_str(" -V, --version         display version\n");
    text.push('\n');
    text.push_str("For more details see cal(1).\n");
    text
}

/// `print_version`, which for util-linux is one line naming the package.
///
/// Upstream says `cal from util-linux 2.39.3`. This keeps the *shape* — which
/// is the observable thing a script would match on — and names the package it
/// actually came from. It is deliberately not the coreutils house form
/// `cal (SlateOS coreutils) 0.1.0`, because `cal` is not a coreutil.
fn version_text() -> String {
    String::from("cal from SlateOS coreutils 0.1.0\n")
}

// ------------------------------------------------------------ the command ---

/// `cal.c`'s option codes. A short option is its own letter; `--color`,
/// `--iso` and `--reform` have no short form, and upstream numbers them past
/// `CHAR_MAX` so that the `switch` can still see them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Code {
    One,
    Three,
    Sunday,
    Monday,
    Julian,
    Months,
    Span,
    Year,
    Week,
    Color,
    Reform,
    Iso,
    Version,
    Twelve,
    Help,
    Vertical,
    Columns,
}

/// The letter-to-code half of the option table. `None` cannot happen —
/// [`SHORT_OPTIONS`] lists exactly these letters and the parser has already
/// refused anything else — and is answered as if the parser had.
fn short_code(flag: u8) -> Option<Code> {
    Some(match flag {
        b'1' => Code::One,
        b'3' => Code::Three,
        b's' => Code::Sunday,
        b'm' => Code::Monday,
        b'j' => Code::Julian,
        b'n' => Code::Months,
        b'S' => Code::Span,
        b'y' => Code::Year,
        b'w' => Code::Week,
        b'Y' => Code::Twelve,
        b'v' => Code::Vertical,
        b'c' => Code::Columns,
        b'V' => Code::Version,
        b'h' => Code::Help,
        _ => return None,
    })
}

/// The same for [`LONG_OPTIONS`], whose names the parser hands back as the
/// table spells them rather than as they were typed.
fn long_code(name: &str) -> Option<Code> {
    Some(match name {
        "one" => Code::One,
        "three" => Code::Three,
        "sunday" => Code::Sunday,
        "monday" => Code::Monday,
        "julian" => Code::Julian,
        "months" => Code::Months,
        "span" => Code::Span,
        "year" => Code::Year,
        "week" => Code::Week,
        "color" => Code::Color,
        "reform" => Code::Reform,
        "iso" => Code::Iso,
        "version" => Code::Version,
        "twelve" => Code::Twelve,
        "help" => Code::Help,
        "vertical" => Code::Vertical,
        "columns" => Code::Columns,
        _ => return None,
    })
}

/// `err_exclusive_options` for `cal`'s single group, `{ 'Y', 'n', 'y' }`.
///
/// Upstream scans the group only while the entry sorts at or before the option
/// it was handed. The group is in ASCII order — its own documentation requires
/// that — so the scan is exactly a membership test, and is written as one.
///
/// The message names **every** option in the group rather than the two that
/// collided, and carries no `Try 'cal --help'` referral, because
/// `err_exclusive_options` prints and `exit`s rather than calling `errtryhelp`.
#[derive(Default, Debug)]
struct Exclusive {
    seen: Option<Code>,
}

impl Exclusive {
    fn check(&mut self, code: Code) -> Result<(), Error> {
        if !matches!(code, Code::Twelve | Code::Months | Code::Year) {
            return Ok(());
        }
        match self.seen {
            None => {
                self.seen = Some(code);
                Ok(())
            }
            // Repeating the *same* option is not a collision: `cal -y -y`
            // works, `cal -y -Y` does not.
            Some(first) if first == code => Ok(()),
            Some(_) => Err(fail(
                "mutually exclusive arguments: --twelve --months --year".to_string(),
            )),
        }
    }
}

/// The value of an option the tables mark `Required`.
///
/// The parser refuses a required option with no value before this is reached,
/// so the empty fallback stands for a case that cannot arise.
fn arg_of(value: &Option<OsString>) -> &OsStr {
    value.as_deref().unwrap_or(OsStr::new(""))
}

/// `illegal week value: year Y doesn't have week W`, raised from two places.
fn no_such_week(ctl: &Ctl) -> Error {
    fail(format!(
        "illegal week value: year {} doesn't have week {}",
        ctl.req.year, ctl.req.week
    ))
}

/// What the command line asked for.
#[derive(Debug)]
enum Action {
    Help,
    Version,
    Calendar {
        ctl: Ctl,
        /// `yflag || Yflag`: whether `yearly` or `monthly` does the printing.
        whole_year: bool,
    },
}

/// Everything `cal.c`'s `main` does between reading `argv` and printing.
///
/// `wall` and `zone` stand in for `time(NULL)` and the process's time zone, and
/// `term` for `isatty` and `get_terminal_width`. They are parameters rather
/// than ambient state so that the whole of `cal`'s behaviour can be tested
/// without a clock, a terminal, or an environment.
///
/// # Errors
///
/// Every diagnostic `cal` can produce before it prints a calendar: getopt's,
/// the numeric ones, the mutually-exclusive group, and the operand checks.
#[expect(
    clippy::too_many_lines,
    reason = "one transcribed function, whose order is load-bearing"
)]
fn build(
    args: &[OsString],
    zone: &localtime::Zone,
    wall: i64,
    term: Terminal,
) -> Result<Action, Error> {
    let mut ctl = Ctl::default();
    let mut yflag = false;
    let mut twelve = false;
    let mut cols: i64 = COLUMNS_MAX_THREE;
    let mut excl = Exclusive::default();
    let mut operands: Vec<&OsString> = Vec::new();

    for item in CAL.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        let (code, value) = match item? {
            Opt::Short(flag, value) => (
                short_code(flag).ok_or_else(|| CAL.invalid_option(flag))?,
                value,
            ),
            Opt::Long(name, value) => (
                // Unreachable: the parser resolves against `LONG_OPTIONS`, and
                // every entry there has a `Code`. Spelled out rather than
                // `unwrap`ped so that adding a long option without a code is a
                // diagnostic instead of a panic.
                long_code(name)
                    .ok_or_else(|| CAL.unrecognized_option(format!("--{name}").as_bytes()))?,
                value,
            ),
            // glibc permutes, so an operand may appear before an option and
            // still be an operand. Collecting them in order is that, without
            // the reshuffling of argv.
            Opt::Operand(word) => {
                operands.push(word);
                continue;
            }
        };

        // Before the switch, exactly as upstream calls it.
        excl.check(code)?;

        match code {
            Code::One => ctl.num_months = 1,
            Code::Three => {
                ctl.num_months = 3;
                ctl.span_months = true;
            }
            Code::Sunday => ctl.weekstart = SUNDAY,
            Code::Monday => ctl.weekstart = MONDAY,
            Code::Julian => {
                ctl.julian = true;
                ctl.day_width = 4;
            }
            Code::Year => yflag = true,
            Code::Twelve => twelve = true,
            Code::Months => {
                // A `uint32_t` stored into an `int`. The narrowing is
                // upstream's, and it is why `-n 4294967295` asks for -1 months
                // and prints nothing instead of being rejected.
                let n = strtou32_or_err(arg_of(&value), "invalid month argument")?;
                ctl.num_months = i32::from_ne_bytes(n.to_ne_bytes());
            }
            Code::Span => ctl.span_months = true,
            Code::Week => {
                if let Some(v) = &value {
                    ctl.req.week = strtos32_or_err(v, "invalid week argument")?;
                    if ctl.req.week < 1 || 54 < ctl.req.week {
                        return Err(fail("illegal week value: use 1-54".to_string()));
                    }
                }
                // Set whether or not a number was given: `-w` alone means "show
                // week numbers", and `--week=N` means that *and* highlight N.
                ctl.weektype = WEEK_NUM_US;
            }
            Code::Color => {
                ctl.colormode = ColorMode::Auto;
                if let Some(v) = &value {
                    ctl.colormode = colormode_or_err(v)?;
                }
            }
            Code::Reform => ctl.reform_year = parse_reform_year(arg_of(&value))?,
            Code::Iso => ctl.reform_year = ISO_REFORM,
            Code::Vertical => ctl.vertical = true,
            Code::Columns => {
                let arg = arg_of(&value);
                cols = if os_bytes(arg).as_ref() == b"auto".as_slice() {
                    COLUMNS_AUTO
                } else {
                    let size = parse_size(&os_bytes(arg))
                        .map_err(|e| size_error("failed to parse columns", arg, e))?;
                    // A `uintmax_t` stored into an `int`, again upstream's.
                    let low = u32::try_from(size & 0xffff_ffff).unwrap_or(0);
                    i64::from(i32::from_ne_bytes(low.to_ne_bytes()))
                };
            }
            // Both exit from inside the switch upstream, before any operand is
            // looked at: `cal --help nonsense` prints the help.
            Code::Version => return Ok(Action::Version),
            Code::Help => return Ok(Action::Help),
        }
    }

    if ctl.weektype != WEEK_NUM_DISABLED {
        ctl.weektype = u32::try_from(ctl.req.week).unwrap_or(0) & WEEK_NUM_MASK;
        ctl.weektype |= if ctl.weekstart == MONDAY {
            WEEK_NUM_ISO
        } else {
            WEEK_NUM_US
        };
        ctl.week_width = ctl.day_width * DAYS_IN_WEEK + 3;
    } else {
        ctl.week_width = ctl.day_width * DAYS_IN_WEEK;
    }
    // `day_width` counts the space *between* days; there is none before the
    // first, so the row is one column narrower than seven of them.
    ctl.week_width -= 1;

    let sole = match operands.as_slice() {
        [word] if !isdigit_string(&os_bytes(word)) => Some(*word),
        _ => None,
    };
    let now = if let Some(word) = sole {
        let bytes = os_bytes(word);
        let now = if let Some(x) = parse_timestamp(zone, wall, &bytes) {
            // `usec_t` divided down to a `time_t`. The division cannot exceed
            // `i64::MAX`, so the cast upstream makes is always exact here.
            i64::try_from(x / USEC_PER_SEC).unwrap_or(0)
        } else if let Some(month) = monthname_to_number(&bytes) {
            ctl.req.month = month;
            wall
        } else {
            return Err(fail(format!(
                "failed to parse timestamp or unknown month name: {}",
                shown(word)
            )));
        };
        // `argc = 0`: the word was the date, not an operand.
        operands.clear();
        now
    } else {
        wall
    };

    let local = zone.local(now, 0);
    let local_month = i32::try_from(local.month).unwrap_or(1);
    let local_yday = i32::try_from(local.yday).unwrap_or(0);

    if operands.len() > 3 {
        return Err(CAL.usage_referring("bad usage".to_string()));
    }
    // Upstream's `switch (argc)` falls through from 3 to 2 to 1, so with three
    // operands the last is still the year and the middle still the month.
    let count = operands.len();
    let day_arg = if count == 3 {
        operands.first().copied()
    } else {
        None
    };
    let month_arg = if count >= 2 {
        operands.get(count.saturating_sub(2)).copied()
    } else {
        None
    };
    let year_arg = operands.last().copied();

    if let Some(word) = day_arg {
        ctl.req.day = strtos32_or_err(word, "illegal day value")?;
        if ctl.req.day < 1 || MAX_DAYS_IN_MONTH < ctl.req.day {
            return Err(fail(format!(
                "illegal day value: use 1-{MAX_DAYS_IN_MONTH}"
            )));
        }
    }

    if let Some(word) = month_arg {
        let bytes = os_bytes(word);
        if bytes.first().is_some_and(u8::is_ascii_digit) {
            ctl.req.month = strtos32_or_err(word, "illegal month value: use 1-12")?;
        } else {
            // An empty operand takes this path too: `**argv` is the terminator,
            // which `isdigit` says no to.
            match monthname_to_number(&bytes) {
                Some(month) => ctl.req.month = month,
                None => return Err(fail(format!("unknown month name: {}", shown(word)))),
            }
        }
        if ctl.req.month < 1 || DECEMBER < ctl.req.month {
            return Err(fail("illegal month value: use 1-12".to_string()));
        }
    }

    if let Some(word) = year_arg {
        ctl.req.year = strtos32_or_err(word, "illegal year value")?;
        if ctl.req.year < SMALLEST_YEAR {
            return Err(fail("illegal year value: use positive integer".to_string()));
        }
        // `INT32_MAX` is the sentinel `--reform=julian` uses, so no calendar can
        // be printed for it. The message has no "use …" tail, and that asymmetry
        // is upstream's.
        if ctl.req.year == JULIAN {
            return Err(fail("illegal year value".to_string()));
        }
        if ctl.req.day != 0 {
            let dm = month_length(ctl.reform_year, ctl.req.month, ctl.req.year);
            if ctl.req.day > dm {
                return Err(fail(format!("illegal day value: use 1-{dm}")));
            }
            // From here on `req.day` is a day of the *year*.
            ctl.req.day = day_in_year(ctl.reform_year, ctl.req.day, ctl.req.month, ctl.req.year);
        } else if local.year == i64::from(ctl.req.year) {
            ctl.req.day = local_yday + 1;
        }
        if ctl.req.month == 0 && ctl.req.week == 0 {
            ctl.req.month = local_month;
            // `cal 2024` is a whole year; `cal -3 2024` is not.
            if ctl.num_months == 0 {
                yflag = true;
            }
        }
    } else {
        ctl.req.day = local_yday + 1;
        ctl.req.year = i32::try_from(local.year).unwrap_or(SMALLEST_YEAR);
        if ctl.req.month == 0 {
            ctl.req.month = local_month;
        }
    }

    if 0 < ctl.req.week {
        let mut yday = week_to_day(&ctl);
        if yday < 1 {
            return Err(no_such_week(&ctl));
        }
        let mut m = 1;
        while m <= DECEMBER {
            let len = month_length(ctl.reform_year, m, ctl.req.year);
            if yday <= len {
                break;
            }
            yday -= len;
            m += 1;
        }
        // Some years (2010 in ISO mode) start with a remnant of the previous
        // year's week 53 yet end inside week 52. Asking for 53 then means that
        // remnant, and nothing else.
        if DECEMBER < m
            && ctl.weektype & WEEK_NUM_ISO != 0
            && ctl.req.week != week_number(31, DECEMBER, ctl.req.year - 1, &ctl)
        {
            return Err(no_such_week(&ctl));
        }
        if ctl.req.month == 0 {
            ctl.req.month = if DECEMBER < m { 1 } else { m };
        }
    }

    let weekdays = weekdays_init(&ctl);
    headers_init(&mut ctl, &weekdays);

    ctl.colors = match ctl.colormode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        // `UL_COLORMODE_UNDEF` behaves as `auto`: `colors_init` resolves it the
        // same way once there is no `terminal-colors.d` to consult.
        ColorMode::Auto | ColorMode::Undef => term.is_tty,
    };
    if !ctl.colors {
        // With nothing to highlight *with*, upstream removes the two things it
        // would have highlighted rather than emitting bare escapes.
        ctl.req.day = 0;
        ctl.weektype &= !WEEK_NUM_MASK;
    }

    if yflag || twelve {
        ctl.gutter_width = 3;
        if ctl.num_months == 0 {
            ctl.num_months = DECEMBER;
        }
        if yflag {
            ctl.req.start_month = 1;
            ctl.header_year = true;
        }
    }

    if ctl.vertical {
        ctl.gutter_width = 1;
    }

    if ctl.num_months > 1 && ctl.months_in_row == 0 {
        ctl.months_in_row = MONTHS_IN_YEAR_ROW;

        if cols > 0 {
            ctl.months_in_row = i32::try_from(cols).unwrap_or(MONTHS_IN_YEAR_ROW);
        } else if term.is_tty {
            let mw = if ctl.julian {
                DOY_MONTH_WIDTH
            } else {
                DOM_MONTH_WIDTH
            };
            let w = term.width.max(mw);
            let gutter = i32::try_from(ctl.gutter_width).unwrap_or(0);
            let extra = (w / mw - 1) * gutter;
            let new_n = (w - extra) / mw;

            match cols {
                // The default: three months per row unless the terminal is too
                // narrow for three, never more than three however wide it is.
                // The width test is a guard rather than an `if` inside the arm
                // so that "wide enough for three" falls to the `_` arm, which
                // is where "leave `months_in_row` at its default" already is.
                COLUMNS_MAX_THREE if new_n < MONTHS_IN_YEAR_ROW => {
                    ctl.months_in_row = new_n.max(1);
                }
                COLUMNS_AUTO => ctl.months_in_row = new_n.max(1),
                _ => {}
            }
        }
    } else if ctl.months_in_row == 0 {
        ctl.months_in_row = 1;
    }

    if ctl.num_months == 0 {
        ctl.num_months = 1;
    }

    Ok(Action::Calendar {
        ctl,
        whole_year: yflag || twelve,
    })
}

/// The calendar itself, as one string.
fn render(ctl: &Ctl, whole_year: bool) -> String {
    let weekdays = weekdays_init(ctl);
    let mut out = String::new();
    if whole_year {
        yearly(&mut out, ctl, &weekdays);
    } else {
        monthly(&mut out, ctl, &weekdays);
    }
    out
}

/// `time(NULL)`.
fn wall_clock() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        // A clock set before 1970. `duration_since` reports the distance rather
        // than a negative, so the sign has to be put back.
        Err(e) => i64::try_from(e.duration().as_secs())
            .unwrap_or(i64::MAX)
            .saturating_neg(),
    }
}

/// `get_terminal_width(80)`.
///
/// Upstream asks `TIOCGWINSZ` first and falls back to `COLUMNS`. This build has
/// no window-size ioctl — the same gap `ls` documents — so only the environment
/// variable is consulted, and only when it parses as a positive number.
fn terminal_width() -> i32 {
    let Some(value) = std::env::var_os("COLUMNS") else {
        return 80;
    };
    match ul_strtos64(&os_bytes(&value)) {
        Ok(w) if w > 0 => i32::try_from(w).unwrap_or(80),
        _ => 80,
    }
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let zone = localtime::Zone::from_env();
    let term = Terminal {
        is_tty: stdfd::is_tty(1),
        width: terminal_width(),
    };

    let action = match build(&args, &zone, wall_clock(), term) {
        Ok(action) => action,
        Err(e) => {
            CAL.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let mut out = Stream::stdout();
    let text = match action {
        Action::Help => help_text(),
        Action::Version => version_text(),
        Action::Calendar { ctl, whole_year } => render(&ctl, whole_year),
    };
    let _ = out.write_all(text.as_bytes());

    stdfd::close_stdout("cal", out, ExitCode::SUCCESS)
}

// ------------------------------------------------------------------ tests ---

/// Everything below `build` is a pure function of its arguments — the clock,
/// the zone and the terminal are parameters rather than globals — so the whole
/// program can be exercised without a process, a tty or a `TZ`.
///
/// The golden tables were captured from util-linux 2.39.3 on a WSL Ubuntu host
/// (`/usr/local/bin/cal`, built from the release tarball) rather than written
/// from the source, because the two disagreed four times while this file was
/// being written and the binary was right every time.
#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-08-27 00:00:00 UTC — the day the goldens were captured. The four
    /// commands with no year in them resolve against this.
    const NOW: i64 = 1_787_788_800;

    /// stdout is a pipe: no colour unless `--color=always` asks for it.
    const PIPE: Terminal = Terminal {
        is_tty: false,
        width: 80,
    };

    /// stdout is a terminal 80 columns wide.
    const TTY: Terminal = Terminal {
        is_tty: true,
        width: 80,
    };

    /// Every one of these was captured from util-linux 2.39.3 `cal` on 2026-08-27
    /// under `LC_ALL=C.UTF-8 TZ=UTC`, with stdout a pipe. The four that take no
    /// year (`[]`, `-3`, `--twelve`, `now`) are therefore anchored to [`NOW`].
    const GOLDEN: &[(&[&str], &str)] = &[
        (
            &[],
            "     August 2026    \nSu Mo Tu We Th Fr Sa\n                   1\n 2  3  4  5  6  7  8\n 9 10 11 12 13 14 15\n16 17 18 19 20 21 22\n23 24 25 26 27 28 29\n30 31               \n",
        ),
        (
            &["2", "2024"],
            "    February 2024   \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n",
        ),
        (
            &["-m", "2", "2024"],
            "    February 2024   \nMo Tu We Th Fr Sa Su\n          1  2  3  4\n 5  6  7  8  9 10 11\n12 13 14 15 16 17 18\n19 20 21 22 23 24 25\n26 27 28 29         \n                    \n",
        ),
        (
            &["-j", "2", "2024"],
            "       February 2024       \nSun Mon Tue Wed Thu Fri Sat\n                 32  33  34\n 35  36  37  38  39  40  41\n 42  43  44  45  46  47  48\n 49  50  51  52  53  54  55\n 56  57  58  59  60        \n                           \n",
        ),
        (
            &["-w", "12", "2010"],
            "     December 2010     \n   Su Mo Tu We Th Fr Sa\n49           1  2  3  4\n50  5  6  7  8  9 10 11\n51 12 13 14 15 16 17 18\n52 19 20 21 22 23 24 25\n53 26 27 28 29 30 31   \n                       \n",
        ),
        (
            &["-j", "-w", "2", "2024"],
            "         February 2024        \n   Sun Mon Tue Wed Thu Fri Sat\n 5                  32  33  34\n 6  35  36  37  38  39  40  41\n 7  42  43  44  45  46  47  48\n 8  49  50  51  52  53  54  55\n 9  56  57  58  59  60        \n                              \n",
        ),
        (
            &["-3", "1", "2024"],
            "    December 2023         January 2024          February 2024   \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n                1  2      1  2  3  4  5  6               1  2  3\n 3  4  5  6  7  8  9   7  8  9 10 11 12 13   4  5  6  7  8  9 10\n10 11 12 13 14 15 16  14 15 16 17 18 19 20  11 12 13 14 15 16 17\n17 18 19 20 21 22 23  21 22 23 24 25 26 27  18 19 20 21 22 23 24\n24 25 26 27 28 29 30  28 29 30 31           25 26 27 28 29      \n31                                                              \n",
        ),
        (
            &["-3", "12", "2024"],
            "    November 2024         December 2024         January 2025    \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n                1  2   1  2  3  4  5  6  7            1  2  3  4\n 3  4  5  6  7  8  9   8  9 10 11 12 13 14   5  6  7  8  9 10 11\n10 11 12 13 14 15 16  15 16 17 18 19 20 21  12 13 14 15 16 17 18\n17 18 19 20 21 22 23  22 23 24 25 26 27 28  19 20 21 22 23 24 25\n24 25 26 27 28 29 30  29 30 31              26 27 28 29 30 31   \n                                                                \n",
        ),
        (
            &["-3", "2", "2024"],
            "    January 2024          February 2024          March 2024     \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6               1  2  3                  1  2\n 7  8  9 10 11 12 13   4  5  6  7  8  9 10   3  4  5  6  7  8  9\n14 15 16 17 18 19 20  11 12 13 14 15 16 17  10 11 12 13 14 15 16\n21 22 23 24 25 26 27  18 19 20 21 22 23 24  17 18 19 20 21 22 23\n28 29 30 31           25 26 27 28 29        24 25 26 27 28 29 30\n                                            31                  \n",
        ),
        (
            &["-3"],
            "      July 2026            August 2026         September 2026   \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n          1  2  3  4                     1         1  2  3  4  5\n 5  6  7  8  9 10 11   2  3  4  5  6  7  8   6  7  8  9 10 11 12\n12 13 14 15 16 17 18   9 10 11 12 13 14 15  13 14 15 16 17 18 19\n19 20 21 22 23 24 25  16 17 18 19 20 21 22  20 21 22 23 24 25 26\n26 27 28 29 30 31     23 24 25 26 27 28 29  27 28 29 30         \n                      30 31                                     \n",
        ),
        (
            &["-y", "2024"],
            "                               2024                               \n\n       January               February                 March       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3                   1  2\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10    3  4  5  6  7  8  9\n14 15 16 17 18 19 20   11 12 13 14 15 16 17   10 11 12 13 14 15 16\n21 22 23 24 25 26 27   18 19 20 21 22 23 24   17 18 19 20 21 22 23\n28 29 30 31            25 26 27 28 29         24 25 26 27 28 29 30\n                                              31                  \n        April                   May                   June        \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6             1  2  3  4                      1\n 7  8  9 10 11 12 13    5  6  7  8  9 10 11    2  3  4  5  6  7  8\n14 15 16 17 18 19 20   12 13 14 15 16 17 18    9 10 11 12 13 14 15\n21 22 23 24 25 26 27   19 20 21 22 23 24 25   16 17 18 19 20 21 22\n28 29 30               26 27 28 29 30 31      23 24 25 26 27 28 29\n                                              30                  \n        July                  August                September     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3    1  2  3  4  5  6  7\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10    8  9 10 11 12 13 14\n14 15 16 17 18 19 20   11 12 13 14 15 16 17   15 16 17 18 19 20 21\n21 22 23 24 25 26 27   18 19 20 21 22 23 24   22 23 24 25 26 27 28\n28 29 30 31            25 26 27 28 29 30 31   29 30               \n                                                                  \n       October               November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n       1  2  3  4  5                   1  2    1  2  3  4  5  6  7\n 6  7  8  9 10 11 12    3  4  5  6  7  8  9    8  9 10 11 12 13 14\n13 14 15 16 17 18 19   10 11 12 13 14 15 16   15 16 17 18 19 20 21\n20 21 22 23 24 25 26   17 18 19 20 21 22 23   22 23 24 25 26 27 28\n27 28 29 30 31         24 25 26 27 28 29 30   29 30 31            \n                                                                  \n",
        ),
        (
            &["-Y", "2024"],
            "                               2024                               \n\n       January               February                 March       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3                   1  2\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10    3  4  5  6  7  8  9\n14 15 16 17 18 19 20   11 12 13 14 15 16 17   10 11 12 13 14 15 16\n21 22 23 24 25 26 27   18 19 20 21 22 23 24   17 18 19 20 21 22 23\n28 29 30 31            25 26 27 28 29         24 25 26 27 28 29 30\n                                              31                  \n        April                   May                   June        \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6             1  2  3  4                      1\n 7  8  9 10 11 12 13    5  6  7  8  9 10 11    2  3  4  5  6  7  8\n14 15 16 17 18 19 20   12 13 14 15 16 17 18    9 10 11 12 13 14 15\n21 22 23 24 25 26 27   19 20 21 22 23 24 25   16 17 18 19 20 21 22\n28 29 30               26 27 28 29 30 31      23 24 25 26 27 28 29\n                                              30                  \n        July                  August                September     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3    1  2  3  4  5  6  7\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10    8  9 10 11 12 13 14\n14 15 16 17 18 19 20   11 12 13 14 15 16 17   15 16 17 18 19 20 21\n21 22 23 24 25 26 27   18 19 20 21 22 23 24   22 23 24 25 26 27 28\n28 29 30 31            25 26 27 28 29 30 31   29 30               \n                                                                  \n       October               November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n       1  2  3  4  5                   1  2    1  2  3  4  5  6  7\n 6  7  8  9 10 11 12    3  4  5  6  7  8  9    8  9 10 11 12 13 14\n13 14 15 16 17 18 19   10 11 12 13 14 15 16   15 16 17 18 19 20 21\n20 21 22 23 24 25 26   17 18 19 20 21 22 23   22 23 24 25 26 27 28\n27 28 29 30 31         24 25 26 27 28 29 30   29 30 31            \n                                                                  \n",
        ),
        (
            &["-v", "8", "2026"],
            "    August 2026        \nSu     2  9 16 23 30\nMo     3 10 17 24 31\nTu     4 11 18 25   \nWe     5 12 19 26   \nTh     6 13 20 27   \nFr     7 14 21 28   \nSa  1  8 15 22 29   \n",
        ),
        (
            &["-v", "-w", "8", "2026"],
            "    August 2026        \nSu     2  9 16 23 30\nMo     3 10 17 24 31\nTu     4 11 18 25   \nWe     5 12 19 26   \nTh     6 13 20 27   \nFr     7 14 21 28   \nSa  1  8 15 22 29   \n   31 32 33 34 35 36\n",
        ),
        (
            &["-v", "-y", "2024"],
            "                             2024                             \n\n    January            February           March              \nSu     7 14 21 28         4 11 18 25         3 10 17 24 31\nMo  1  8 15 22 29         5 12 19 26         4 11 18 25   \nTu  2  9 16 23 30         6 13 20 27         5 12 19 26   \nWe  3 10 17 24 31         7 14 21 28         6 13 20 27   \nTh  4 11 18 25         1  8 15 22 29         7 14 21 28   \nFr  5 12 19 26         2  9 16 23         1  8 15 22 29   \nSa  6 13 20 27         3 10 17 24         2  9 16 23 30   \n\n    April              May                June               \nSu     7 14 21 28         5 12 19 26         2  9 16 23 30\nMo  1  8 15 22 29         6 13 20 27         3 10 17 24   \nTu  2  9 16 23 30         7 14 21 28         4 11 18 25   \nWe  3 10 17 24         1  8 15 22 29         5 12 19 26   \nTh  4 11 18 25         2  9 16 23 30         6 13 20 27   \nFr  5 12 19 26         3 10 17 24 31         7 14 21 28   \nSa  6 13 20 27         4 11 18 25         1  8 15 22 29   \n\n    July               August             September          \nSu     7 14 21 28         4 11 18 25      1  8 15 22 29   \nMo  1  8 15 22 29         5 12 19 26      2  9 16 23 30   \nTu  2  9 16 23 30         6 13 20 27      3 10 17 24      \nWe  3 10 17 24 31         7 14 21 28      4 11 18 25      \nTh  4 11 18 25         1  8 15 22 29      5 12 19 26      \nFr  5 12 19 26         2  9 16 23 30      6 13 20 27      \nSa  6 13 20 27         3 10 17 24 31      7 14 21 28      \n\n    October            November           December           \nSu     6 13 20 27         3 10 17 24      1  8 15 22 29   \nMo     7 14 21 28         4 11 18 25      2  9 16 23 30   \nTu  1  8 15 22 29         5 12 19 26      3 10 17 24 31   \nWe  2  9 16 23 30         6 13 20 27      4 11 18 25      \nTh  3 10 17 24 31         7 14 21 28      5 12 19 26      \nFr  4 11 18 25         1  8 15 22 29      6 13 20 27      \nSa  5 12 19 26         2  9 16 23 30      7 14 21 28      \n",
        ),
        (
            &["9", "1752"],
            "   September 1752   \nSu Mo Tu We Th Fr Sa\n       1  2 14 15 16\n17 18 19 20 21 22 23\n24 25 26 27 28 29 30\n                    \n                    \n                    \n",
        ),
        (
            &["-j", "9", "1752"],
            "       September 1752      \nSun Mon Tue Wed Thu Fri Sat\n        245 246 258 259 260\n261 262 263 264 265 266 267\n268 269 270 271 272 273 274\n                           \n                           \n                           \n",
        ),
        (
            &["-w", "9", "1752"],
            "     September 1752    \n   Su Mo Tu We Th Fr Sa\n36        1  2 14 15 16\n37 17 18 19 20 21 22 23\n38 24 25 26 27 28 29 30\n                       \n                       \n                       \n",
        ),
        (
            &["-w", "1", "1752"],
            "      January 1752     \n   Su Mo Tu We Th Fr Sa\n 1           1  2  3  4\n 2  5  6  7  8  9 10 11\n 3 12 13 14 15 16 17 18\n 4 19 20 21 22 23 24 25\n 5 26 27 28 29 30 31   \n                       \n",
        ),
        (
            &["-w", "12", "1752"],
            "     December 1752     \n   Su Mo Tu We Th Fr Sa\n47                 1  2\n48  3  4  5  6  7  8  9\n49 10 11 12 13 14 15 16\n50 17 18 19 20 21 22 23\n51 24 25 26 27 28 29 30\n52 31                  \n",
        ),
        (
            &["-m", "-w", "1", "2010"],
            "      January 2010     \n   Mo Tu We Th Fr Sa Su\n53              1  2  3\n 1  4  5  6  7  8  9 10\n 2 11 12 13 14 15 16 17\n 3 18 19 20 21 22 23 24\n 4 25 26 27 28 29 30 31\n                       \n",
        ),
        (
            &["-m", "-w", "1", "2011"],
            "      January 2011     \n   Mo Tu We Th Fr Sa Su\n52                 1  2\n 1  3  4  5  6  7  8  9\n 2 10 11 12 13 14 15 16\n 3 17 18 19 20 21 22 23\n 4 24 25 26 27 28 29 30\n 5 31                  \n",
        ),
        (
            &["-m", "-w", "12", "2010"],
            "     December 2010     \n   Mo Tu We Th Fr Sa Su\n48        1  2  3  4  5\n49  6  7  8  9 10 11 12\n50 13 14 15 16 17 18 19\n51 20 21 22 23 24 25 26\n52 27 28 29 30 31      \n                       \n",
        ),
        (
            &["-y", "1"],
            "                               0001                               \n\n       January               February                 March       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                   1          1  2  3  4  5          1  2  3  4  5\n 2  3  4  5  6  7  8    6  7  8  9 10 11 12    6  7  8  9 10 11 12\n 9 10 11 12 13 14 15   13 14 15 16 17 18 19   13 14 15 16 17 18 19\n16 17 18 19 20 21 22   20 21 22 23 24 25 26   20 21 22 23 24 25 26\n23 24 25 26 27 28 29   27 28                  27 28 29 30 31      \n30 31                                                             \n        April                   May                   June        \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                1  2    1  2  3  4  5  6  7             1  2  3  4\n 3  4  5  6  7  8  9    8  9 10 11 12 13 14    5  6  7  8  9 10 11\n10 11 12 13 14 15 16   15 16 17 18 19 20 21   12 13 14 15 16 17 18\n17 18 19 20 21 22 23   22 23 24 25 26 27 28   19 20 21 22 23 24 25\n24 25 26 27 28 29 30   29 30 31               26 27 28 29 30      \n                                                                  \n        July                  August                September     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                1  2       1  2  3  4  5  6                1  2  3\n 3  4  5  6  7  8  9    7  8  9 10 11 12 13    4  5  6  7  8  9 10\n10 11 12 13 14 15 16   14 15 16 17 18 19 20   11 12 13 14 15 16 17\n17 18 19 20 21 22 23   21 22 23 24 25 26 27   18 19 20 21 22 23 24\n24 25 26 27 28 29 30   28 29 30 31            25 26 27 28 29 30   \n31                                                                \n       October               November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                   1          1  2  3  4  5                1  2  3\n 2  3  4  5  6  7  8    6  7  8  9 10 11 12    4  5  6  7  8  9 10\n 9 10 11 12 13 14 15   13 14 15 16 17 18 19   11 12 13 14 15 16 17\n16 17 18 19 20 21 22   20 21 22 23 24 25 26   18 19 20 21 22 23 24\n23 24 25 26 27 28 29   27 28 29 30            25 26 27 28 29 30 31\n30 31                                                             \n",
        ),
        (
            &["-y", "13"],
            "                               0013                               \n\n       January               February                 March       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7             1  2  3  4             1  2  3  4\n 8  9 10 11 12 13 14    5  6  7  8  9 10 11    5  6  7  8  9 10 11\n15 16 17 18 19 20 21   12 13 14 15 16 17 18   12 13 14 15 16 17 18\n22 23 24 25 26 27 28   19 20 21 22 23 24 25   19 20 21 22 23 24 25\n29 30 31               26 27 28               26 27 28 29 30 31   \n                                                                  \n        April                   May                   June        \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                   1       1  2  3  4  5  6                1  2  3\n 2  3  4  5  6  7  8    7  8  9 10 11 12 13    4  5  6  7  8  9 10\n 9 10 11 12 13 14 15   14 15 16 17 18 19 20   11 12 13 14 15 16 17\n16 17 18 19 20 21 22   21 22 23 24 25 26 27   18 19 20 21 22 23 24\n23 24 25 26 27 28 29   28 29 30 31            25 26 27 28 29 30   \n30                                                                \n        July                  August                September     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                   1          1  2  3  4  5                   1  2\n 2  3  4  5  6  7  8    6  7  8  9 10 11 12    3  4  5  6  7  8  9\n 9 10 11 12 13 14 15   13 14 15 16 17 18 19   10 11 12 13 14 15 16\n16 17 18 19 20 21 22   20 21 22 23 24 25 26   17 18 19 20 21 22 23\n23 24 25 26 27 28 29   27 28 29 30 31         24 25 26 27 28 29 30\n30 31                                                             \n       October               November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7             1  2  3  4                   1  2\n 8  9 10 11 12 13 14    5  6  7  8  9 10 11    3  4  5  6  7  8  9\n15 16 17 18 19 20 21   12 13 14 15 16 17 18   10 11 12 13 14 15 16\n22 23 24 25 26 27 28   19 20 21 22 23 24 25   17 18 19 20 21 22 23\n29 30 31               26 27 28 29 30         24 25 26 27 28 29 30\n                                              31                  \n",
        ),
        (
            &["--week=13", "2024"],
            "       March 2024      \n   Su Mo Tu We Th Fr Sa\n 9                 1  2\n10  3  4  5  6  7  8  9\n11 10 11 12 13 14 15 16\n12 17 18 19 20 21 22 23\n13 24 25 26 27 28 29 30\n14 31                  \n",
        ),
        (
            &["--week=1", "2024"],
            "      January 2024     \n   Su Mo Tu We Th Fr Sa\n 1     1  2  3  4  5  6\n 2  7  8  9 10 11 12 13\n 3 14 15 16 17 18 19 20\n 4 21 22 23 24 25 26 27\n 5 28 29 30 31         \n                       \n",
        ),
        (
            &["--week=54", "2024"],
            "      January 2024     \n   Su Mo Tu We Th Fr Sa\n 1     1  2  3  4  5  6\n 2  7  8  9 10 11 12 13\n 3 14 15 16 17 18 19 20\n 4 21 22 23 24 25 26 27\n 5 28 29 30 31         \n                       \n",
        ),
        (
            &["-j", "2", "1900"],
            "       February 1900       \nSun Mon Tue Wed Thu Fri Sat\n                 32  33  34\n 35  36  37  38  39  40  41\n 42  43  44  45  46  47  48\n 49  50  51  52  53  54  55\n 56  57  58  59            \n                           \n",
        ),
        (
            &["-c", "1", "-y", "2024"],
            "        2024        \n\n       January      \nSu Mo Tu We Th Fr Sa\n    1  2  3  4  5  6\n 7  8  9 10 11 12 13\n14 15 16 17 18 19 20\n21 22 23 24 25 26 27\n28 29 30 31         \n                    \n      February      \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n        March       \nSu Mo Tu We Th Fr Sa\n                1  2\n 3  4  5  6  7  8  9\n10 11 12 13 14 15 16\n17 18 19 20 21 22 23\n24 25 26 27 28 29 30\n31                  \n        April       \nSu Mo Tu We Th Fr Sa\n    1  2  3  4  5  6\n 7  8  9 10 11 12 13\n14 15 16 17 18 19 20\n21 22 23 24 25 26 27\n28 29 30            \n                    \n         May        \nSu Mo Tu We Th Fr Sa\n          1  2  3  4\n 5  6  7  8  9 10 11\n12 13 14 15 16 17 18\n19 20 21 22 23 24 25\n26 27 28 29 30 31   \n                    \n        June        \nSu Mo Tu We Th Fr Sa\n                   1\n 2  3  4  5  6  7  8\n 9 10 11 12 13 14 15\n16 17 18 19 20 21 22\n23 24 25 26 27 28 29\n30                  \n        July        \nSu Mo Tu We Th Fr Sa\n    1  2  3  4  5  6\n 7  8  9 10 11 12 13\n14 15 16 17 18 19 20\n21 22 23 24 25 26 27\n28 29 30 31         \n                    \n       August       \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29 30 31\n                    \n      September     \nSu Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7\n 8  9 10 11 12 13 14\n15 16 17 18 19 20 21\n22 23 24 25 26 27 28\n29 30               \n                    \n       October      \nSu Mo Tu We Th Fr Sa\n       1  2  3  4  5\n 6  7  8  9 10 11 12\n13 14 15 16 17 18 19\n20 21 22 23 24 25 26\n27 28 29 30 31      \n                    \n      November      \nSu Mo Tu We Th Fr Sa\n                1  2\n 3  4  5  6  7  8  9\n10 11 12 13 14 15 16\n17 18 19 20 21 22 23\n24 25 26 27 28 29 30\n                    \n      December      \nSu Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7\n 8  9 10 11 12 13 14\n15 16 17 18 19 20 21\n22 23 24 25 26 27 28\n29 30 31            \n                    \n",
        ),
        (
            &["-c", "2", "-y", "2024"],
            "                    2024                   \n\n       January               February      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10\n14 15 16 17 18 19 20   11 12 13 14 15 16 17\n21 22 23 24 25 26 27   18 19 20 21 22 23 24\n28 29 30 31            25 26 27 28 29      \n                                           \n        March                  April       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                1  2       1  2  3  4  5  6\n 3  4  5  6  7  8  9    7  8  9 10 11 12 13\n10 11 12 13 14 15 16   14 15 16 17 18 19 20\n17 18 19 20 21 22 23   21 22 23 24 25 26 27\n24 25 26 27 28 29 30   28 29 30            \n31                                         \n         May                   June        \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n          1  2  3  4                      1\n 5  6  7  8  9 10 11    2  3  4  5  6  7  8\n12 13 14 15 16 17 18    9 10 11 12 13 14 15\n19 20 21 22 23 24 25   16 17 18 19 20 21 22\n26 27 28 29 30 31      23 24 25 26 27 28 29\n                       30                  \n        July                  August       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10\n14 15 16 17 18 19 20   11 12 13 14 15 16 17\n21 22 23 24 25 26 27   18 19 20 21 22 23 24\n28 29 30 31            25 26 27 28 29 30 31\n                                           \n      September               October      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7          1  2  3  4  5\n 8  9 10 11 12 13 14    6  7  8  9 10 11 12\n15 16 17 18 19 20 21   13 14 15 16 17 18 19\n22 23 24 25 26 27 28   20 21 22 23 24 25 26\n29 30                  27 28 29 30 31      \n                                           \n      November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                1  2    1  2  3  4  5  6  7\n 3  4  5  6  7  8  9    8  9 10 11 12 13 14\n10 11 12 13 14 15 16   15 16 17 18 19 20 21\n17 18 19 20 21 22 23   22 23 24 25 26 27 28\n24 25 26 27 28 29 30   29 30 31            \n                                           \n",
        ),
        (
            &["-c", "1K", "-y", "2024"],
            "                                                                                                                                                                                                                                                                                                           \n\n       January               February                 March                  April                   May                   June                   July                  August                September               October               November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6                1  2  3                   1  2       1  2  3  4  5  6             1  2  3  4                      1       1  2  3  4  5  6                1  2  3    1  2  3  4  5  6  7          1  2  3  4  5                   1  2    1  2  3  4  5  6  7\n 7  8  9 10 11 12 13    4  5  6  7  8  9 10    3  4  5  6  7  8  9    7  8  9 10 11 12 13    5  6  7  8  9 10 11    2  3  4  5  6  7  8    7  8  9 10 11 12 13    4  5  6  7  8  9 10    8  9 10 11 12 13 14    6  7  8  9 10 11 12    3  4  5  6  7  8  9    8  9 10 11 12 13 14\n14 15 16 17 18 19 20   11 12 13 14 15 16 17   10 11 12 13 14 15 16   14 15 16 17 18 19 20   12 13 14 15 16 17 18    9 10 11 12 13 14 15   14 15 16 17 18 19 20   11 12 13 14 15 16 17   15 16 17 18 19 20 21   13 14 15 16 17 18 19   10 11 12 13 14 15 16   15 16 17 18 19 20 21\n21 22 23 24 25 26 27   18 19 20 21 22 23 24   17 18 19 20 21 22 23   21 22 23 24 25 26 27   19 20 21 22 23 24 25   16 17 18 19 20 21 22   21 22 23 24 25 26 27   18 19 20 21 22 23 24   22 23 24 25 26 27 28   20 21 22 23 24 25 26   17 18 19 20 21 22 23   22 23 24 25 26 27 28\n28 29 30 31            25 26 27 28 29         24 25 26 27 28 29 30   28 29 30               26 27 28 29 30 31      23 24 25 26 27 28 29   28 29 30 31            25 26 27 28 29 30 31   29 30                  27 28 29 30 31         24 25 26 27 28 29 30   29 30 31            \n                                              31                                                                   30                                                                                                                                                            \n",
        ),
        (
            &["-n", "5", "2", "2024"],
            "    February 2024          March 2024            April 2024     \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n             1  2  3                  1  2      1  2  3  4  5  6\n 4  5  6  7  8  9 10   3  4  5  6  7  8  9   7  8  9 10 11 12 13\n11 12 13 14 15 16 17  10 11 12 13 14 15 16  14 15 16 17 18 19 20\n18 19 20 21 22 23 24  17 18 19 20 21 22 23  21 22 23 24 25 26 27\n25 26 27 28 29        24 25 26 27 28 29 30  28 29 30            \n                      31                                        \n      May 2024              June 2024     \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n          1  2  3  4                     1\n 5  6  7  8  9 10 11   2  3  4  5  6  7  8\n12 13 14 15 16 17 18   9 10 11 12 13 14 15\n19 20 21 22 23 24 25  16 17 18 19 20 21 22\n26 27 28 29 30 31     23 24 25 26 27 28 29\n                      30                  \n",
        ),
        (
            &["-n", "0", "2", "2024"],
            "    February 2024   \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n",
        ),
        (
            &["-n", "2", "2", "2024"],
            "    February 2024          March 2024     \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n             1  2  3                  1  2\n 4  5  6  7  8  9 10   3  4  5  6  7  8  9\n11 12 13 14 15 16 17  10 11 12 13 14 15 16\n18 19 20 21 22 23 24  17 18 19 20 21 22 23\n25 26 27 28 29        24 25 26 27 28 29 30\n                      31                  \n",
        ),
        (
            &["--reform=julian", "9", "1752"],
            "   September 1752   \nSu Mo Tu We Th Fr Sa\n       1  2  3  4  5\n 6  7  8  9 10 11 12\n13 14 15 16 17 18 19\n20 21 22 23 24 25 26\n27 28 29 30         \n                    \n",
        ),
        (
            &["--reform=gregorian", "9", "1752"],
            "   September 1752   \nSu Mo Tu We Th Fr Sa\n                1  2\n 3  4  5  6  7  8  9\n10 11 12 13 14 15 16\n17 18 19 20 21 22 23\n24 25 26 27 28 29 30\n                    \n",
        ),
        (
            &["-w", "--reform=gregorian", "1", "1"],
            "      January 0001     \n   Su Mo Tu We Th Fr Sa\n 1     1  2  3  4  5  6\n 2  7  8  9 10 11 12 13\n 3 14 15 16 17 18 19 20\n 4 21 22 23 24 25 26 27\n 5 28 29 30 31         \n                       \n",
        ),
        (
            &["1", "1"],
            "    January 0001    \nSu Mo Tu We Th Fr Sa\n                   1\n 2  3  4  5  6  7  8\n 9 10 11 12 13 14 15\n16 17 18 19 20 21 22\n23 24 25 26 27 28 29\n30 31               \n",
        ),
        (
            &["007", "2024"],
            "      July 2024     \nSu Mo Tu We Th Fr Sa\n    1  2  3  4  5  6\n 7  8  9 10 11 12 13\n14 15 16 17 18 19 20\n21 22 23 24 25 26 27\n28 29 30 31         \n                    \n",
        ),
        (
            &["-s", "2", "2024"],
            "    February 2024   \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n",
        ),
        (
            &["-S", "2", "2024"],
            "    February 2024   \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n",
        ),
        (
            &["-1", "-3", "2", "2024"],
            "    January 2024          February 2024          March 2024     \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6               1  2  3                  1  2\n 7  8  9 10 11 12 13   4  5  6  7  8  9 10   3  4  5  6  7  8  9\n14 15 16 17 18 19 20  11 12 13 14 15 16 17  10 11 12 13 14 15 16\n21 22 23 24 25 26 27  18 19 20 21 22 23 24  17 18 19 20 21 22 23\n28 29 30 31           25 26 27 28 29        24 25 26 27 28 29 30\n                                            31                  \n",
        ),
        (
            &["-3", "-1", "2", "2024"],
            "    February 2024   \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n",
        ),
        (
            &["--twelve"],
            "     August 2026          September 2026          October 2026    \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                   1          1  2  3  4  5                1  2  3\n 2  3  4  5  6  7  8    6  7  8  9 10 11 12    4  5  6  7  8  9 10\n 9 10 11 12 13 14 15   13 14 15 16 17 18 19   11 12 13 14 15 16 17\n16 17 18 19 20 21 22   20 21 22 23 24 25 26   18 19 20 21 22 23 24\n23 24 25 26 27 28 29   27 28 29 30            25 26 27 28 29 30 31\n30 31                                                             \n    November 2026          December 2026          January 2027    \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7          1  2  3  4  5                   1  2\n 8  9 10 11 12 13 14    6  7  8  9 10 11 12    3  4  5  6  7  8  9\n15 16 17 18 19 20 21   13 14 15 16 17 18 19   10 11 12 13 14 15 16\n22 23 24 25 26 27 28   20 21 22 23 24 25 26   17 18 19 20 21 22 23\n29 30                  27 28 29 30 31         24 25 26 27 28 29 30\n                                              31                  \n    February 2027           March 2027             April 2027     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n    1  2  3  4  5  6       1  2  3  4  5  6                1  2  3\n 7  8  9 10 11 12 13    7  8  9 10 11 12 13    4  5  6  7  8  9 10\n14 15 16 17 18 19 20   14 15 16 17 18 19 20   11 12 13 14 15 16 17\n21 22 23 24 25 26 27   21 22 23 24 25 26 27   18 19 20 21 22 23 24\n28                     28 29 30 31            25 26 27 28 29 30   \n                                                                  \n      May 2027               June 2027              July 2027     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n                   1          1  2  3  4  5                1  2  3\n 2  3  4  5  6  7  8    6  7  8  9 10 11 12    4  5  6  7  8  9 10\n 9 10 11 12 13 14 15   13 14 15 16 17 18 19   11 12 13 14 15 16 17\n16 17 18 19 20 21 22   20 21 22 23 24 25 26   18 19 20 21 22 23 24\n23 24 25 26 27 28 29   27 28 29 30            25 26 27 28 29 30 31\n30 31                                                             \n",
        ),
        (
            &["-3", "1", "1752"],
            "    December 1751         January 1752          February 1752   \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7            1  2  3  4                     1\n 8  9 10 11 12 13 14   5  6  7  8  9 10 11   2  3  4  5  6  7  8\n15 16 17 18 19 20 21  12 13 14 15 16 17 18   9 10 11 12 13 14 15\n22 23 24 25 26 27 28  19 20 21 22 23 24 25  16 17 18 19 20 21 22\n29 30 31              26 27 28 29 30 31     23 24 25 26 27 28 29\n                                                                \n",
        ),
        (
            &["-3", "9", "1752"],
            "     August 1752         September 1752         October 1752    \nSu Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa  Su Mo Tu We Th Fr Sa\n                   1         1  2 14 15 16   1  2  3  4  5  6  7\n 2  3  4  5  6  7  8  17 18 19 20 21 22 23   8  9 10 11 12 13 14\n 9 10 11 12 13 14 15  24 25 26 27 28 29 30  15 16 17 18 19 20 21\n16 17 18 19 20 21 22                        22 23 24 25 26 27 28\n23 24 25 26 27 28 29                        29 30 31            \n30 31                                                           \n",
        ),
        (
            &["-j", "-3", "9", "1752"],
            "        August 1752                 September 1752                October 1752       \nSun Mon Tue Wed Thu Fri Sat  Sun Mon Tue Wed Thu Fri Sat  Sun Mon Tue Wed Thu Fri Sat\n                        214          245 246 258 259 260  275 276 277 278 279 280 281\n215 216 217 218 219 220 221  261 262 263 264 265 266 267  282 283 284 285 286 287 288\n222 223 224 225 226 227 228  268 269 270 271 272 273 274  289 290 291 292 293 294 295\n229 230 231 232 233 234 235                               296 297 298 299 300 301 302\n236 237 238 239 240 241 242                               303 304 305                \n243 244                                                                              \n",
        ),
        (
            &["-w", "-3", "9", "1752"],
            "      August 1752             September 1752            October 1752     \n   Su Mo Tu We Th Fr Sa     Su Mo Tu We Th Fr Sa     Su Mo Tu We Th Fr Sa\n31                    1  36        1  2 14 15 16  39  1  2  3  4  5  6  7\n32  2  3  4  5  6  7  8  37 17 18 19 20 21 22 23  40  8  9 10 11 12 13 14\n33  9 10 11 12 13 14 15  38 24 25 26 27 28 29 30  41 15 16 17 18 19 20 21\n34 16 17 18 19 20 21 22                           42 22 23 24 25 26 27 28\n35 23 24 25 26 27 28 29                           43 29 30 31            \n36 30 31                                                                 \n",
        ),
        (
            &["-y", "1752"],
            "                               1752                               \n\n       January               February                 March       \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n          1  2  3  4                      1    1  2  3  4  5  6  7\n 5  6  7  8  9 10 11    2  3  4  5  6  7  8    8  9 10 11 12 13 14\n12 13 14 15 16 17 18    9 10 11 12 13 14 15   15 16 17 18 19 20 21\n19 20 21 22 23 24 25   16 17 18 19 20 21 22   22 23 24 25 26 27 28\n26 27 28 29 30 31      23 24 25 26 27 28 29   29 30 31            \n                                                                  \n        April                   May                   June        \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n          1  2  3  4                   1  2       1  2  3  4  5  6\n 5  6  7  8  9 10 11    3  4  5  6  7  8  9    7  8  9 10 11 12 13\n12 13 14 15 16 17 18   10 11 12 13 14 15 16   14 15 16 17 18 19 20\n19 20 21 22 23 24 25   17 18 19 20 21 22 23   21 22 23 24 25 26 27\n26 27 28 29 30         24 25 26 27 28 29 30   28 29 30            \n                       31                                         \n        July                  August                September     \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n          1  2  3  4                      1          1  2 14 15 16\n 5  6  7  8  9 10 11    2  3  4  5  6  7  8   17 18 19 20 21 22 23\n12 13 14 15 16 17 18    9 10 11 12 13 14 15   24 25 26 27 28 29 30\n19 20 21 22 23 24 25   16 17 18 19 20 21 22                       \n26 27 28 29 30 31      23 24 25 26 27 28 29                       \n                       30 31                                      \n       October               November               December      \nSu Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa   Su Mo Tu We Th Fr Sa\n 1  2  3  4  5  6  7             1  2  3  4                   1  2\n 8  9 10 11 12 13 14    5  6  7  8  9 10 11    3  4  5  6  7  8  9\n15 16 17 18 19 20 21   12 13 14 15 16 17 18   10 11 12 13 14 15 16\n22 23 24 25 26 27 28   19 20 21 22 23 24 25   17 18 19 20 21 22 23\n29 30 31               26 27 28 29 30         24 25 26 27 28 29 30\n                                              31                  \n",
        ),
        (
            &["-j", "-y", "1752"],
            "                                          1752                                         \n\n          January                       February                       March           \nSun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat\n              1   2   3   4                            32    61  62  63  64  65  66  67\n  5   6   7   8   9  10  11    33  34  35  36  37  38  39    68  69  70  71  72  73  74\n 12  13  14  15  16  17  18    40  41  42  43  44  45  46    75  76  77  78  79  80  81\n 19  20  21  22  23  24  25    47  48  49  50  51  52  53    82  83  84  85  86  87  88\n 26  27  28  29  30  31        54  55  56  57  58  59  60    89  90  91                \n                                                                                       \n           April                          May                           June           \nSun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat\n             92  93  94  95                       122 123       153 154 155 156 157 158\n 96  97  98  99 100 101 102   124 125 126 127 128 129 130   159 160 161 162 163 164 165\n103 104 105 106 107 108 109   131 132 133 134 135 136 137   166 167 168 169 170 171 172\n110 111 112 113 114 115 116   138 139 140 141 142 143 144   173 174 175 176 177 178 179\n117 118 119 120 121           145 146 147 148 149 150 151   180 181 182                \n                              152                                                      \n            July                         August                      September         \nSun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat\n            183 184 185 186                           214           245 246 258 259 260\n187 188 189 190 191 192 193   215 216 217 218 219 220 221   261 262 263 264 265 266 267\n194 195 196 197 198 199 200   222 223 224 225 226 227 228   268 269 270 271 272 273 274\n201 202 203 204 205 206 207   229 230 231 232 233 234 235                              \n208 209 210 211 212 213       236 237 238 239 240 241 242                              \n                              243 244                                                  \n          October                       November                      December         \nSun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat   Sun Mon Tue Wed Thu Fri Sat\n275 276 277 278 279 280 281               306 307 308 309                       336 337\n282 283 284 285 286 287 288   310 311 312 313 314 315 316   338 339 340 341 342 343 344\n289 290 291 292 293 294 295   317 318 319 320 321 322 323   345 346 347 348 349 350 351\n296 297 298 299 300 301 302   324 325 326 327 328 329 330   352 353 354 355 356 357 358\n303 304 305                   331 332 333 334 335           359 360 361 362 363 364 365\n                                                            366                        \n",
        ),
        (
            &["12", "9999"],
            "    December 9999   \nSu Mo Tu We Th Fr Sa\n          1  2  3  4\n 5  6  7  8  9 10 11\n12 13 14 15 16 17 18\n19 20 21 22 23 24 25\n26 27 28 29 30 31   \n                    \n",
        ),
        (
            &["2", "2100"],
            "    February 2100   \nSu Mo Tu We Th Fr Sa\n    1  2  3  4  5  6\n 7  8  9 10 11 12 13\n14 15 16 17 18 19 20\n21 22 23 24 25 26 27\n28                  \n                    \n",
        ),
        (
            &["2", "2000"],
            "    February 2000   \nSu Mo Tu We Th Fr Sa\n       1  2  3  4  5\n 6  7  8  9 10 11 12\n13 14 15 16 17 18 19\n20 21 22 23 24 25 26\n27 28 29            \n                    \n",
        ),
        (
            &["-w", "1", "2016"],
            "      January 2016     \n   Su Mo Tu We Th Fr Sa\n 1                 1  2\n 2  3  4  5  6  7  8  9\n 3 10 11 12 13 14 15 16\n 4 17 18 19 20 21 22 23\n 5 24 25 26 27 28 29 30\n 6 31                  \n",
        ),
        (
            &["-m", "-w", "1", "2016"],
            "      January 2016     \n   Mo Tu We Th Fr Sa Su\n53              1  2  3\n 1  4  5  6  7  8  9 10\n 2 11 12 13 14 15 16 17\n 3 18 19 20 21 22 23 24\n 4 25 26 27 28 29 30 31\n                       \n",
        ),
        (
            &["2", "1"],
            "    February 0001   \nSu Mo Tu We Th Fr Sa\n       1  2  3  4  5\n 6  7  8  9 10 11 12\n13 14 15 16 17 18 19\n20 21 22 23 24 25 26\n27 28               \n                    \n",
        ),
        (
            &["1", "2147483646"],
            " January 2147483646 \nSu Mo Tu We Th Fr Sa\n    1  2  3  4  5  6\n 7  8  9 10 11 12 13\n14 15 16 17 18 19 20\n21 22 23 24 25 26 27\n28 29 30 31         \n                    \n",
        ),
        (
            &["now"],
            "     August 2026    \nSu Mo Tu We Th Fr Sa\n                   1\n 2  3  4  5  6  7  8\n 9 10 11 12 13 14 15\n16 17 18 19 20 21 22\n23 24 25 26 27 28 29\n30 31               \n",
        ),
        (
            &["Sep"],
            "   September 2026   \nSu Mo Tu We Th Fr Sa\n       1  2  3  4  5\n 6  7  8  9 10 11 12\n13 14 15 16 17 18 19\n20 21 22 23 24 25 26\n27 28 29 30         \n                    \n",
        ),
        (
            &["SEPTEMBER"],
            "   September 2026   \nSu Mo Tu We Th Fr Sa\n       1  2  3  4  5\n 6  7  8  9 10 11 12\n13 14 15 16 17 18 19\n20 21 22 23 24 25 26\n27 28 29 30         \n                    \n",
        ),
        (
            &["2024-02-15"],
            "    February 2024   \nSu Mo Tu We Th Fr Sa\n             1  2  3\n 4  5  6  7  8  9 10\n11 12 13 14 15 16 17\n18 19 20 21 22 23 24\n25 26 27 28 29      \n                    \n",
        ),
        (
            &["--color=never", "8", "2026"],
            "     August 2026    \nSu Mo Tu We Th Fr Sa\n                   1\n 2  3  4  5  6  7  8\n 9 10 11 12 13 14 15\n16 17 18 19 20 21 22\n23 24 25 26 27 28 29\n30 31               \n",
        ),
    ];

    /// The same, with `--color=always`, which is the only way to see the two
    /// highlight bugs transcribed in [`cal_vert_output_months`] without a tty.
    const GOLDEN_COLOR: &[(&[&str], &str)] = &[
        (
            &["--color=always", "-w", "2", "2024"],
            "     February 2024     \n   Su Mo Tu We Th Fr Sa\n 5              1  2  3\n 6  4  5  6  7  8  9 10\n 7 11 12 13 14 15 16 17\n 8 18 19 20 21 22 23 24\n 9 25 26 27 28 29      \n                       \n",
        ),
        (
            &["--color=always", "-v", "8", "2026"],
            "    August 2026        \nSu     2  9 16 23 30\nMo     3 10 17 24 31\nTu     4 11 18 25   \nWe     5 12 19 26   \nTh     6 13 20 \x1b[7m27\x1b[0m   \nFr     7 14 21 28   \nSa  1  8 15 22 29   \n",
        ),
        (
            &["--color=always", "-v", "-w", "8", "2026"],
            "    August 2026        \nSu     2  9 16 23 30\nMo     3 10 17 24 31\nTu     4 11 18 25   \nWe     5 12 19 26   \nTh     6 13 20 \x1b[7m27\x1b[0m   \nFr     7 14 21 28   \nSa  1  8 15 22 29   \n   31 32 33 34 35 36\n",
        ),
        (
            &["--color=always", "8", "2026"],
            "     August 2026    \nSu Mo Tu We Th Fr Sa\n                   1\n 2  3  4  5  6  7  8\n 9 10 11 12 13 14 15\n16 17 18 19 20 21 22\n23 24 25 26 \x1b[7m27\x1b[0m 28 29\n30 31               \n",
        ),
        (
            &["--color=always", "-j", "8", "2026"],
            "        August 2026        \nSun Mon Tue Wed Thu Fri Sat\n                        213\n214 215 216 217 218 219 220\n221 222 223 224 225 226 227\n228 229 230 231 232 233 234\n235 236 237 238 \x1b[7m239\x1b[0m 240 241\n242 243                    \n",
        ),
    ];

    /// The sentence each rejection produces, with the `cal: ` prefix and the
    /// trailing newline removed and the referral kept, since [`Error::message`]
    /// produces exactly that. Upstream names the program by `argv[0]`, so its
    /// getopt sentences read `/usr/local/bin/cal:`; ours always says `cal`.
    const REJECTED: &[(&[&str], &str)] = &[
        (
            &["1", "2", "3", "4"],
            "bad usage\nTry 'cal --help' for more information.",
        ),
        (&["32", "2", "2024"], "illegal day value: use 1-31"),
        (&["30", "2", "2024"], "illegal day value: use 1-29"),
        (&["1", "2x", "2024"], "illegal month value: use 1-12: '2x'"),
        (
            &["1", "0x7", "2024"],
            "illegal month value: use 1-12: '0x7'",
        ),
        (&["1", "abc", "2024"], "unknown month name: abc"),
        (
            &["abc"],
            "failed to parse timestamp or unknown month name: abc",
        ),
        (
            &["99999999999"],
            "illegal year value: '99999999999': Numerical result out of range",
        ),
        (&["1", "2147483647"], "illegal year value"),
        (&["-n", "abc"], "invalid month argument: 'abc'"),
        (
            &["-n", "-1"],
            "invalid month argument: '-1': Numerical result out of range",
        ),
        (&["--week=55", "2024"], "illegal week value: use 1-54"),
        (&["--week=0", "2024"], "illegal week value: use 1-54"),
        (
            &["-c", "abc"],
            "failed to parse columns: 'abc': Invalid argument",
        ),
        (
            &["-c", "1x"],
            "failed to parse columns: '1x': Invalid argument",
        ),
        (&["-c", ""], "failed to parse columns: '': Invalid argument"),
        (
            &["-c", "1Y"],
            "failed to parse columns: '1Y': Numerical result out of range",
        ),
        (
            &["-c", "18446744073709551616"],
            "failed to parse columns: '18446744073709551616': Numerical result out of range",
        ),
        (&["--reform=nosuch"], "invalid --reform value: 'nosuch'"),
        (&["--reform=1918"], "invalid --reform value: '1918'"),
        (&["--color=nosuch"], "unsupported color mode: 'nosuch'"),
        (
            &["-Y", "-n", "3"],
            "mutually exclusive arguments: --twelve --months --year",
        ),
        (
            &["-Z"],
            "invalid option -- 'Z'\nTry 'cal --help' for more information.",
        ),
        (
            &["--nosuch"],
            "unrecognized option '--nosuch'\nTry 'cal --help' for more information.",
        ),
        (
            &["-w5", "2024"],
            "invalid option -- '5'\nTry 'cal --help' for more information.",
        ),
        (
            &["-1day"],
            "invalid option -- 'd'\nTry 'cal --help' for more information.",
        ),
        (&[""], "failed to parse timestamp or unknown month name: "),
        (
            &[" 5"],
            "failed to parse timestamp or unknown month name:  5",
        ),
        (
            &["sept"],
            "failed to parse timestamp or unknown month name: sept",
        ),
        (&["0", "2024"], "illegal month value: use 1-12"),
        (&["13", "2024"], "illegal month value: use 1-12"),
        (&["1", "13", "2024"], "illegal month value: use 1-12"),
        (&["0"], "illegal year value: use positive integer"),
        (&["-w", "53", "2024"], "illegal month value: use 1-12"),
    ];

    /// `cal --help`, byte for byte, from util-linux 2.39.3.
    const UPSTREAM_HELP: &str = "\nUsage:\n cal [options] [[[day] month] year]\n cal [options] <timestamp|monthname>\n\nDisplay a calendar, or some part of it.\nWithout any arguments, display the current month.\n\nOptions:\n -1, --one             show only a single month (default)\n -3, --three           show three months spanning the date\n -n, --months <num>    show num months starting with date's month\n -S, --span            span the date when displaying multiple months\n -s, --sunday          Sunday as first day of week\n -m, --monday          Monday as first day of week\n -j, --julian          use day-of-year for all calendars\n     --reform <val>    Gregorian reform date (1752|gregorian|iso|julian)\n     --iso             alias for --reform=iso\n -y, --year            show the whole year\n -Y, --twelve          show the next twelve months\n -w, --week[=<num>]    show US or ISO-8601 week numbers\n -v, --vertical        show day vertically instead of line\n -c, --columns <width> amount of columns to use\n     --color[=<when>]  colorize messages (auto, always or never)\n                         colors are enabled by default\n\n -h, --help            display this help\n -V, --version         display version\n\nFor more details see cal(1).\n";

    fn argv(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn act(list: &[&str], term: Terminal) -> Result<String, Error> {
        let zone = localtime::Zone::utc();
        Ok(match build(&argv(list), &zone, NOW, term)? {
            Action::Help => help_text(),
            Action::Version => version_text(),
            Action::Calendar { ctl, whole_year } => render(&ctl, whole_year),
        })
    }

    fn ok(list: &[&str]) -> String {
        match act(list, PIPE) {
            Ok(text) => text,
            Err(e) => panic!("cal {list:?} was rejected: {}", e.message()),
        }
    }

    fn rejected(list: &[&str]) -> Error {
        match act(list, PIPE) {
            Ok(text) => panic!("cal {list:?} was accepted, and printed {text:?}"),
            Err(e) => e,
        }
    }

    fn control(list: &[&str], term: Terminal) -> Ctl {
        let zone = localtime::Zone::utc();
        match build(&argv(list), &zone, NOW, term) {
            Ok(Action::Calendar { ctl, .. }) => ctl,
            Ok(_) => panic!("cal {list:?} asked for help, not a calendar"),
            Err(e) => panic!("cal {list:?} was rejected: {}", e.message()),
        }
    }

    #[test]
    fn every_calendar_is_upstreams_to_the_byte() {
        for (args, want) in GOLDEN {
            assert_eq!(&ok(args).as_str(), want, "cal {args:?}");
        }
    }

    #[test]
    fn every_highlighted_calendar_is_upstreams_to_the_byte() {
        for (args, want) in GOLDEN_COLOR {
            assert_eq!(&ok(args).as_str(), want, "cal {args:?}");
        }
    }

    #[test]
    fn every_rejection_says_what_upstream_says() {
        for (args, want) in REJECTED {
            let e = rejected(args);
            assert_eq!(&e.message().as_str(), want, "cal {args:?}");
            assert_eq!(e.status, 1, "cal {args:?}");
        }
    }

    #[test]
    fn a_terminal_turns_the_highlight_on_by_itself() {
        assert_eq!(
            act(&["8", "2026"], TTY).expect("cal 8 2026"),
            ok(&["--color=always", "8", "2026"]),
        );
        assert_eq!(
            act(&["--color=never", "8", "2026"], TTY).expect("cal --color=never 8 2026"),
            ok(&["8", "2026"]),
        );
    }

    #[test]
    fn a_pipe_removes_both_of_the_things_a_colour_would_have_marked() {
        // Not "prints them unmarked": with no escape to wrap them in, upstream
        // clears the requested day and the requested week number outright, and
        // `cal --week=35 8 2026` into a pipe therefore highlights neither.
        let piped = control(&["--week=35", "8", "2026"], PIPE);
        assert_eq!(piped.req.day, 0);
        assert_eq!(piped.weektype & WEEK_NUM_MASK, 0);

        let on_a_tty = control(&["--week=35", "8", "2026"], TTY);
        assert_eq!(on_a_tty.weektype & WEEK_NUM_MASK, 35);
        // Day of *year*, not of month: 2026-08-27 is the 239th day of 2026.
        assert_eq!(on_a_tty.req.day, 239);
    }

    #[test]
    fn help_beats_what_follows_it_and_loses_to_what_precedes_it() {
        // Measured on `readlink` when this parser was written, and true here:
        // `getopt_long` is called in a loop and the caller acts on each answer,
        // so a later bad option is never reached.
        assert_eq!(ok(&["--help", "--nosuch"]), help_text());
        assert_eq!(ok(&["-h", "13", "2024"]), help_text());
        assert_eq!(ok(&["--version", "13", "2024"]), version_text());
        assert_eq!(ok(&["-V"]), version_text());
        assert_eq!(
            rejected(&["--nosuch", "--help"]).sentence,
            "unrecognized option '--nosuch'"
        );
    }

    #[test]
    fn the_help_is_upstreams_to_the_byte() {
        assert_eq!(help_text(), UPSTREAM_HELP);
        assert_eq!(help_text().len(), 1200);
        // The one line that is deliberately ours rather than upstream's.
        assert_eq!(version_text(), "cal from SlateOS coreutils 0.1.0\n");
    }

    #[test]
    fn the_exclusive_group_complains_in_either_order_but_not_about_repeats() {
        for args in [
            ["-y", "-n", "3"].as_slice(),
            ["-n", "3", "-y"].as_slice(),
            ["-Y", "-y"].as_slice(),
            ["-y", "-Y"].as_slice(),
            ["-n", "3", "-Y"].as_slice(),
            ["-Y", "-n", "3"].as_slice(),
        ] {
            let e = rejected(args);
            assert_eq!(
                e.sentence, "mutually exclusive arguments: --twelve --months --year",
                "cal {args:?}"
            );
            // `errx`, not `usage(EXIT_FAILURE)`: no referral.
            assert!(e.referral.is_none(), "cal {args:?}");
        }
        // The same member twice is one member, so these are fine.
        assert_eq!(ok(&["-y", "-y", "2024"]), ok(&["-y", "2024"]));
        assert_eq!(ok(&["-3", "-3", "2", "2024"]), ok(&["-3", "2", "2024"]));
    }

    #[test]
    fn a_negative_month_count_prints_nothing_and_succeeds() {
        // `-n` is read as `uint32_t` and stored in an `int`, so 4294967295 asks
        // for -1 months. `rows` is then -2, the row loop never runs, and cal
        // exits 0 having printed nothing. Reproduced rather than tidied.
        assert_eq!(ok(&["-n", "4294967295", "2", "2024"]), "");
        assert_eq!(
            control(&["-n", "4294967295", "2", "2024"], PIPE).num_months,
            -1
        );
    }

    #[test]
    fn a_wide_terminal_never_widens_the_row_but_columns_auto_does() {
        // The default is COLUMNS_MAX_THREE: the terminal can only take months
        // away, never add them.
        let wide = Terminal {
            is_tty: true,
            width: 400,
        };
        let narrow = Terminal {
            is_tty: true,
            width: 40,
        };
        assert_eq!(control(&["-y", "2024"], wide).months_in_row, 3);
        assert_eq!(control(&["-y", "2024"], narrow).months_in_row, 1);
        assert_eq!(
            control(&["-c", "auto", "-y", "2024"], wide).months_in_row,
            17
        );
        assert_eq!(
            control(&["-c", "auto", "-y", "2024"], narrow).months_in_row,
            1
        );
        // A pipe is not a terminal, so neither form consults the width at all.
        assert_eq!(
            control(&["-c", "auto", "-y", "2024"], PIPE).months_in_row,
            3
        );
        // `-c 0` is accepted — zero is not `> 0`, so it changes nothing.
        assert_eq!(control(&["-c", "0", "-y", "2024"], PIPE).months_in_row, 3);
        // An explicit count wins over the terminal in both directions.
        assert_eq!(control(&["-c", "5", "-y", "2024"], narrow).months_in_row, 5);
    }

    #[test]
    fn julian_days_widen_every_column() {
        let plain = control(&["2", "2024"], PIPE);
        assert_eq!((plain.day_width, plain.week_width), (3, 20));
        let julian = control(&["-j", "2", "2024"], PIPE);
        assert_eq!((julian.day_width, julian.week_width), (4, 27));
        let weeks = control(&["-w", "2", "2024"], PIPE);
        assert_eq!((weeks.day_width, weeks.week_width), (3, 23));
        let both = control(&["-j", "-w", "2", "2024"], PIPE);
        assert_eq!((both.day_width, both.week_width), (4, 30));
    }

    #[test]
    fn leap_years_hinge_on_the_reform_year() {
        // Julian side of 1752: every fourth year, centuries included.
        assert!(leap_year(DEFAULT_REFORM_YEAR, 1700));
        assert!(leap_year(DEFAULT_REFORM_YEAR, 1752));
        // Gregorian side: centuries only when divisible by 400.
        assert!(!leap_year(DEFAULT_REFORM_YEAR, 1900));
        assert!(leap_year(DEFAULT_REFORM_YEAR, 2000));
        assert!(leap_year(DEFAULT_REFORM_YEAR, 2024));
        assert!(!leap_year(DEFAULT_REFORM_YEAR, 2023));
        // `--reform=julian` moves the hinge past every year there is.
        assert!(leap_year(JULIAN, 1900));
        // `--reform=gregorian` moves it before every year there is.
        assert!(!leap_year(GREGORIAN, 1700));
    }

    #[test]
    fn september_1752_still_has_thirty_days_in_the_table() {
        // The eleven missing days are a hole in the *grid*, not a shorter
        // month: `month_length` is the plain table lookup, which is why
        // `cal 30 9 1752` is accepted.
        assert_eq!(month_length(DEFAULT_REFORM_YEAR, 9, 1752), 30);
        assert_eq!(month_length(DEFAULT_REFORM_YEAR, 2, 2024), 29);
        assert_eq!(month_length(DEFAULT_REFORM_YEAR, 2, 2023), 28);
        assert_eq!(month_length(DEFAULT_REFORM_YEAR, 2, 1900), 28);
        assert_eq!(month_length(JULIAN, 2, 1900), 29);
        // Out of range is zero rather than a panic, because `req.month` is
        // validated after `month_length` is first consulted.
        assert_eq!(month_length(DEFAULT_REFORM_YEAR, 0, 2024), 0);
        assert_eq!(month_length(DEFAULT_REFORM_YEAR, 13, 2024), 0);
    }

    #[test]
    fn the_day_of_the_year_skips_the_reform() {
        assert_eq!(day_in_year(DEFAULT_REFORM_YEAR, 1, 1, 2024), 1);
        assert_eq!(day_in_year(DEFAULT_REFORM_YEAR, 1, 3, 2024), 61);
        assert_eq!(day_in_year(DEFAULT_REFORM_YEAR, 27, 8, 2026), 239);
        assert_eq!(day_in_year(DEFAULT_REFORM_YEAR, 31, 12, 2023), 365);
        // 1752-09-02 is day 246; the next day printed is the 14th, day 258.
        assert_eq!(day_in_year(DEFAULT_REFORM_YEAR, 2, 9, 1752), 246);
        assert_eq!(day_in_year(DEFAULT_REFORM_YEAR, 14, 9, 1752), 258);
    }

    #[test]
    fn the_eleven_removed_days_are_on_no_weekday_at_all() {
        assert_eq!(day_in_week(DEFAULT_REFORM_YEAR, 1, 2, 2024), 4);
        assert_eq!(day_in_week(DEFAULT_REFORM_YEAR, 27, 8, 2026), 4);
        assert_eq!(day_in_week(DEFAULT_REFORM_YEAR, 2, 9, 1752), 3);
        assert_eq!(day_in_week(DEFAULT_REFORM_YEAR, 14, 9, 1752), 4);
        for day in 3..=13 {
            assert_eq!(
                day_in_week(DEFAULT_REFORM_YEAR, day, 9, 1752),
                NONEDAY,
                "1752-09-{day}"
            );
        }
        // With the reform moved, those eleven days exist again.
        assert_eq!(day_in_week(JULIAN, 3, 9, 1752), 4);
        assert_eq!(day_in_week(GREGORIAN, 3, 9, 1752), 0);
    }

    #[test]
    fn iso_week_one_is_the_week_with_the_fourth_of_january_in_it() {
        let iso = Ctl {
            weekstart: MONDAY,
            weektype: WEEK_NUM_ISO,
            ..Ctl::default()
        };
        assert_eq!(week_number(4, 1, 2010, &iso), 1);
        assert_eq!(week_number(1, 1, 2010, &iso), 53);
        assert_eq!(week_number(1, 1, 2011, &iso), 52);
        assert_eq!(week_number(1, 1, 2016, &iso), 53);
        assert_eq!(week_number(4, 1, 2016, &iso), 1);
        assert_eq!(week_number(31, 12, 2010, &iso), 52);

        // The US numbering starts week 1 at the first Sunday-based week that
        // touches January at all, so no year ever begins in week 52 or 53.
        let us = Ctl {
            weektype: WEEK_NUM_US,
            ..Ctl::default()
        };
        assert_eq!(week_number(1, 1, 2010, &us), 1);
        assert_eq!(week_number(1, 1, 2016, &us), 1);
        assert_eq!(week_number(26, 12, 2010, &us), 53);
    }

    #[test]
    fn sizes_are_read_the_way_strtosize_reads_them() {
        assert_eq!(parse_size(b"3"), Ok(3));
        assert_eq!(parse_size(b"+3"), Ok(3));
        assert_eq!(parse_size(b" 5"), Ok(5));
        assert_eq!(parse_size(b"010"), Ok(8));
        assert_eq!(parse_size(b"0x10"), Ok(16));
        assert_eq!(parse_size(b"1.5K"), Ok(1536));
        assert_eq!(parse_size(b"0.5K"), Ok(512));
        assert_eq!(parse_size(b"1kiB"), Ok(1024));
        assert_eq!(parse_size(b"5KB"), Ok(5000));
        assert_eq!(parse_size(b"1EiB"), Ok(1 << 60));
        // A fraction with no suffix to divide into, and a bare `B`, are both
        // rejected — the fraction because there is nothing to scale.
        assert_eq!(parse_size(b"1.9"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"1."), Err(NumErr::Invalid));
        assert_eq!(parse_size(b".5K"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"5B"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"5b"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"1x"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"abc"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"-1"), Err(NumErr::Invalid));
        // `Y` overflows 64 bits, and so does one past `u64::MAX`.
        assert_eq!(parse_size(b"1Y"), Err(NumErr::Range));
        assert_eq!(parse_size(b"18446744073709551616"), Err(NumErr::Range));
    }

    #[test]
    fn signed_numbers_are_read_the_way_strtos64_reads_them() {
        assert_eq!(ul_strtos64(b"0"), Ok(0));
        assert_eq!(ul_strtos64(b"-1"), Ok(-1));
        assert_eq!(ul_strtos64(b"2147483647"), Ok(2_147_483_647));
        assert_eq!(ul_strtos64(b"007"), Ok(7));
        assert_eq!(ul_strtos64(b""), Err(NumErr::Invalid));
        assert_eq!(ul_strtos64(b"2x"), Err(NumErr::Invalid));
        assert_eq!(ul_strtos64(b"0x7"), Err(NumErr::Invalid));
        assert_eq!(ul_strtos64(b"99999999999999999999"), Err(NumErr::Range));
    }

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
    fn a_month_name_is_matched_in_full_or_at_its_c_locale_abbreviation() {
        assert_eq!(monthname_to_number(b"Sep"), Some(9));
        assert_eq!(monthname_to_number(b"SEPTEMBER"), Some(9));
        assert_eq!(monthname_to_number(b"september"), Some(9));
        assert_eq!(monthname_to_number(b"january"), Some(1));
        assert_eq!(monthname_to_number(b"Dec"), Some(12));
        // `ABMON_9` is `Sep`, not `Sept`, so this is not a prefix match and
        // `sept` is simply not a month name.
        assert_eq!(monthname_to_number(b"sept"), None);
        assert_eq!(monthname_to_number(b"Se"), None);
        assert_eq!(monthname_to_number(b""), None);
    }

    #[test]
    fn centring_puts_the_odd_space_on_the_left() {
        let mut s = String::new();
        center(&mut s, "ab", 7, 0);
        assert_eq!(s, "   ab  ");

        s.clear();
        center(&mut s, "ab", 6, 0);
        assert_eq!(s, "  ab  ");

        // The gutter comes from a second `printf` and is appended after.
        s.clear();
        center(&mut s, "ab", 6, 2);
        assert_eq!(s, "  ab    ");

        // Too long for the field: truncated, with no padding at all.
        s.clear();
        center(&mut s, "abcdef", 3, 0);
        assert_eq!(s, "abc");
    }

    #[test]
    fn left_alignment_puts_every_space_after() {
        let mut s = String::new();
        left(&mut s, "Su", 6, 1);
        assert_eq!(s, "Su     ");
    }

    #[test]
    fn a_field_wider_than_the_line_buffer_is_cut_at_299_bytes() {
        // `cal -c 1K -y 2024` centres "2024" in a field 23549 columns wide, and
        // upstream's `char lineout[FMT_ST_CHARS]` holds 299 of them plus a NUL.
        // All 299 are leading padding, so the year never appears — which is the
        // first line of that golden, and is why `mbsalign` takes a buffer size.
        let mut s = String::new();
        center(&mut s, "2024", 23_549, 0);
        assert_eq!(s.len(), FMT_ST_CHARS - 1);
        assert!(s.bytes().all(|b| b == b' '));
    }

    #[test]
    fn a_year_the_calendar_cannot_reach_is_the_reform_sentinel() {
        // `INT32_MAX` is `--reform=julian`'s marker, so no year may equal it.
        // The message has no "use …" tail while the too-small one does; that
        // asymmetry is upstream's.
        assert_eq!(
            rejected(&["1", "2147483647"]).sentence,
            "illegal year value"
        );
        assert_eq!(
            rejected(&["0"]).sentence,
            "illegal year value: use positive integer"
        );
        assert!(ok(&["1", "2147483646"]).starts_with(" January 2147483646 "));
    }
}
