//! uptime — tell how long the system has been running.
//!
//! ```text
//! uptime [-p|--pretty] [-s|--since]
//! ```
//!
//! # It used to ignore its command line entirely
//!
//! This program read `/proc/uptime`, printed `up HH:MM`, and did that whatever
//! it was asked for. `uptime -s` asks when the machine booted and got how long
//! it had been up — not a worse format, a different question — and
//! `uptime --nosuchoption` exited 0. A program that does not look at `argv`
//! cannot honour an option and does not refuse one either, which is worse than
//! an unimplemented option and worse than the refusing stub §1006 forbids:
//! a refusal at least tells the caller it did not get what it asked for.
//!
//! Measured against procps-ng 4.0.4 before this was written:
//!
//! | | procps | here, before |
//! |---|---|---|
//! | `uptime -s` | `2026-09-11 19:27:28` | `up 00:26` |
//! | `uptime -p` | `up 1 hour, 21 minutes` | `up 00:26` |
//! | `uptime --nosuchoption` | usage, exit 1 | `up 00:26`, exit 0 |
//!
//! `scripts/check-argv-ignored.py` is the gate that now refuses the next one.
//!
//! # What is implemented, and what is still missing
//!
//! `-p` and `-s` are exact. The **default line is not**: procps prints
//! ` 20:48:41 up  1:21,  1 user,  load average: 0.10, 0.10, 0.09` and this
//! prints only the `up …` part.
//!
//! The missing pieces are the time of day, the load averages and the user
//! count. The first two are cheap — the clock and `/proc/loadavg` — but the
//! user count is read from `utmp`, and **this tree has no utmp reader**: there
//! is no `who`, and no shared module for it. Printing a plausible number
//! instead of a measured one is exactly the defect this file was just repaired
//! for, so the field is absent rather than invented. Recorded in
//! `known-issues.md` as the remaining half of
//! `B-COREUTILS-UPTIME-SILENTLY-IGNORES-EVERY-ARGUMENT`.
//!
//! # Why procps and not GNU
//!
//! `uptime` is procps-ng, not coreutils, so `DIFF_GNU_SOURCE` cannot supply a
//! reference and the comparison is against the installed `4.0.4`.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::stdfd;
use localtime::{Zone, strftime};
use std::fs;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

/// procps exits 1 on a bad option; measured, `uptime -Q; echo $?` prints 1.
const UPTIME: Program = Program::new("uptime", 1);

/// procps-ng 4.0.4's `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "phsV";

/// procps-ng 4.0.4's `longopts[]`, in its declaration order. Read from
/// `uptime --help` rather than guessed: it carries **four** options and none of
/// the `--raw`, `--json` or `-r` that the retired standalone invented.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("pretty", Takes::Nothing),
    ("help", Takes::Nothing),
    ("since", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// The default `up …` line.
    Plain,
    /// `-p`: `up 1 hour, 21 minutes`.
    Pretty,
    /// `-s`: the wall-clock time the machine booted.
    Since,
    Help,
    Version,
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

/// Parse `uptime`'s argv.
///
/// # Errors
///
/// An unknown option, an ambiguous abbreviation, or any operand: procps takes
/// none.
fn parse_args(args: &[std::ffi::OsString]) -> Result<Request, String> {
    let mut req = Request::Plain;
    for item in UPTIME.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item.map_err(|e| e.message())? {
            Opt::Long("pretty", _) | Opt::Short(b'p', _) => req = Request::Pretty,
            Opt::Long("since", _) | Opt::Short(b's', _) => req = Request::Since,
            Opt::Long("help", _) | Opt::Short(b'h', _) => return Ok(Request::Help),
            Opt::Long("version", _) | Opt::Short(b'V', _) => return Ok(Request::Version),
            Opt::Long(other, _) => {
                return Err(UPTIME
                    .usage_referring(format!("option '--{other}' is unhandled"))
                    .message());
            }
            Opt::Short(other, _) => return Err(UPTIME.invalid_option(other).message()),
            // procps takes no operands and rejects the first one it meets.
            Opt::Operand(_) => {
                return Err(UPTIME
                    .usage_referring("extra operand".to_string())
                    .message());
            }
        }
    }
    Ok(req)
}

/// The `-p` rendering, which is procps' own decomposition.
///
/// Transcribed from procps-ng's `uptime.c` rather than invented, because the
/// units are not the obvious ones: weeks are taken modulo 52 and days modulo 7,
/// so 400 days is "1 year, 5 weeks, 0 days" and never "400 days". Components
/// that are zero are omitted entirely, except that an uptime under a minute
/// still prints `up 0 minutes`.
fn pretty(total_secs: f64) -> String {
    let secs = total_secs.max(0.0) as u64;
    let decades = secs / (60 * 60 * 24 * 365 * 10);
    let years = (secs / (60 * 60 * 24 * 365)) % 10;
    let weeks = (secs / (60 * 60 * 24 * 7)) % 52;
    let days = (secs / (60 * 60 * 24)) % 7;
    let hours = (secs / (60 * 60)) % 24;
    let minutes = (secs / 60) % 60;

    let mut parts: Vec<String> = Vec::new();
    let mut push = |n: u64, unit: &str| {
        if n > 0 {
            let s = if n == 1 { "" } else { "s" };
            parts.push(format!("{n} {unit}{s}"));
        }
    };
    push(decades, "decade");
    push(years, "year");
    push(weeks, "week");
    push(days, "day");
    push(hours, "hour");
    if parts.is_empty() || minutes > 0 {
        let s = if minutes == 1 { "" } else { "s" };
        parts.push(format!("{minutes} minute{s}"));
    }
    format!("up {}", parts.join(", "))
}

/// The `-s` rendering: the wall clock when the machine booted.
///
/// `now - uptime`, truncated to the second exactly as procps does, formatted in
/// the local zone. Returns `None` only when the clock is before the epoch,
/// which is the one case where there is no answer rather than a wrong one.
fn since(total_secs: f64) -> Option<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let boot = i64::try_from(now.as_secs())
        .ok()?
        .checked_sub(total_secs.max(0.0) as i64)?;
    let zone = Zone::from_env();
    let tm = zone.local(boot, 0);
    String::from_utf8(strftime(b"%Y-%m-%d %H:%M:%S", &tm)).ok()
}

fn help_text() -> String {
    "\
Usage:
 uptime [options]

Options:
 -p, --pretty   show uptime in pretty format
 -h, --help     display this help and exit
 -s, --since    system up since
 -V, --version  output version information and exit
"
    .to_string()
}

fn run_main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let req = match parse_args(&args) {
        Ok(r) => r,
        Err(message) => {
            diag!("{}", message);
            return ExitCode::FAILURE;
        }
    };
    match req {
        Request::Help => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("uptime (SlateOS coreutils)");
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let content = match fs::read_to_string("/proc/uptime") {
        Ok(c) => c,
        Err(e) => {
            // This printed `uptime: cannot read /proc/uptime` with `println!`
            // and then returned SUCCESS. Three things wrong at once, and the
            // first two are the ones that reach a script:
            //
            //   * the diagnostic went to STDOUT, so `up=$(uptime)` captured
            //     the error message where the uptime should be;
            //   * the exit status was 0, so `uptime || echo failed` never
            //     printed anything; and
            //   * it named no reason, so a permission problem and a missing
            //     file read identically.
            //
            // All three in a crate whose `main` is a funnel for exactly this
            // discipline -- `stdfd::close_stderr` exists to turn a diagnostic
            // that could not be *written* into a failure status, while this
            // one was not written to stderr at all.
            diag!("uptime: cannot read /proc/uptime: {}", strerror(&e));
            return ExitCode::FAILURE;
        }
    };
    // The seconds count is parsed once, here, so that a malformed
    // `/proc/uptime` fails the same way for every form rather than only for the
    // default one. `-s` on a file that says nothing must not answer with a
    // plausible boot time.
    let Some(total_secs) = uptime_seconds(&content) else {
        diag!("uptime: /proc/uptime is not a number of seconds");
        return ExitCode::FAILURE;
    };

    match req {
        Request::Pretty => {
            println!("{}", pretty(total_secs));
            ExitCode::SUCCESS
        }
        Request::Since => match since(total_secs) {
            Some(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            None => {
                diag!("uptime: cannot determine the boot time from this clock");
                ExitCode::FAILURE
            }
        },
        _ => {
            println!("{}", format_uptime_line(total_secs));
            ExitCode::SUCCESS
        }
    }
}

/// The seconds count at the head of `/proc/uptime`, or `None` if the file does
/// not begin with one.
fn uptime_seconds(content: &str) -> Option<f64> {
    let secs: f64 = content.split_whitespace().next()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some(secs)
}

/// Format `/proc/uptime` content into the human-readable line we print, or
/// `None` if the file does not begin with a number of seconds.
///
/// # Why this is an `Option` now
///
/// It returned a `String` unconditionally: an empty file gave
/// `"up (unknown)"`, which is honest, but a *malformed* one went through
/// `secs_str.parse().unwrap_or(0.0)` and printed `up 00:00`. That is a
/// machine that booted this minute -- a plausible reading of a real system,
/// produced from a file that said nothing of the kind, and indistinguishable
/// from the true answer on a freshly-booted host.
///
/// The empty case is folded in with it: `up (unknown)` was the right shape but
/// still exited 0 and still printed to stdout, so a caller could not act on it
/// either.
fn format_uptime_line(total_secs: f64) -> String {
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

    /// The old `line(&str) -> Option<String>`, recomposed.
    ///
    /// Parsing and formatting were one function until `-p` and `-s` needed the
    /// seconds on their own. Splitting them would have invalidated every test
    /// below, each of which pins a real past defect -- a malformed file
    /// printing `up 00:00`, an empty one exiting 0. Recomposing here keeps the
    /// assertions and their reasons exactly as they were, and tests the two
    /// halves through the same seam the program uses.
    fn line(content: &str) -> Option<String> {
        uptime_seconds(content).map(format_uptime_line)
    }

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

    // --- `-p`, whose units are procps' and not the obvious ones ---------

    #[test]
    fn pretty_under_a_minute_still_says_minutes() {
        // The one case with no non-zero component. procps prints `up 0
        // minutes` rather than a bare `up`, so the string is never truncated
        // to something that reads as an error.
        assert_eq!(pretty(0.0), "up 0 minutes");
        assert_eq!(pretty(59.0), "up 0 minutes");
    }

    #[test]
    fn pretty_singular_and_plural() {
        assert_eq!(pretty(60.0), "up 1 minute");
        assert_eq!(pretty(120.0), "up 2 minutes");
        assert_eq!(pretty(3600.0), "up 1 hour");
        assert_eq!(pretty(7200.0), "up 2 hours");
    }

    #[test]
    fn pretty_omits_zero_components() {
        // 1 hour exactly: the minutes are zero and must not appear, but an
        // hour and one minute prints both.
        assert_eq!(pretty(3600.0), "up 1 hour");
        assert_eq!(pretty(3660.0), "up 1 hour, 1 minute");
    }

    #[test]
    fn pretty_matches_the_live_reading() {
        // 4993 s is what /proc/uptime said while this was being written, and
        // procps printed exactly this for it.
        assert_eq!(pretty(4993.0), "up 1 hour, 23 minutes");
    }

    #[test]
    fn pretty_weeks_are_modulo_52_and_days_modulo_7() {
        // The reason this function is transcribed rather than invented, and I
        // still got the expectation wrong on the first try: I wrote "up 1
        // year, 5 weeks" by subtracting 365 from 400 and dividing the
        // remainder by 7. procps does not subtract. EACH COMPONENT IS
        // COMPUTED INDEPENDENTLY FROM THE TOTAL with its own modulo, so they
        // do not sum back to it:
        //
        //     years = (total / 365d) % 10  = 1
        //     weeks = (total /   7d) % 52  = (400/7) % 52 = 57 % 52 = 5
        //     days  = (total /   1d) %  7  = 400 % 7       = 1
        //
        // 1 year + 5 weeks + 1 day is 401 days, not 400. That is upstream's
        // arithmetic and this reproduces it; the test is here because the
        // reasonable-looking answer is the wrong one.
        let four_hundred_days = 400.0 * 86400.0;
        assert_eq!(pretty(four_hundred_days), "up 1 year, 5 weeks, 1 day");
        // 8 days is one week and one day, never "8 days".
        assert_eq!(pretty(8.0 * 86400.0), "up 1 week, 1 day");
    }

    #[test]
    fn pretty_clamps_a_negative_rather_than_wrapping() {
        // `as u64` on a negative f64 saturates to 0 in Rust, but the clamp is
        // explicit so the behaviour does not depend on that.
        assert_eq!(pretty(-1.0), "up 0 minutes");
    }

    // --- parsing, now that it is its own function -----------------------

    #[test]
    fn uptime_seconds_rejects_what_it_should() {
        assert_eq!(uptime_seconds(""), None);
        assert_eq!(uptime_seconds("garbage 0"), None);
        assert_eq!(uptime_seconds("nan 0"), None);
        assert_eq!(uptime_seconds("-1 0"), None);
        assert_eq!(uptime_seconds("inf 0"), None);
    }

    #[test]
    fn uptime_seconds_takes_the_first_field_only() {
        assert_eq!(uptime_seconds("60.5 30.0"), Some(60.5));
        assert_eq!(uptime_seconds("120"), Some(120.0));
    }

    #[test]
    fn format_basic_seconds() {
        // 60 seconds -> "up 00:01".
        assert_eq!(line("60.0 30.0"), Some("up 00:01".to_string()));
    }

    #[test]
    fn format_one_day_singular() {
        let s = format!("{} 0", 86400);
        assert_eq!(line(&s), Some("up 1 day, 00:00".to_string()));
    }

    #[test]
    fn format_two_days_plural() {
        let total = 2 * 86400 + 3 * 3600 + 5 * 60;
        let s = format!("{total} 0");
        assert_eq!(line(&s), Some("up 2 days, 03:05".to_string()));
    }

    #[test]
    fn format_empty_returns_unknown() {
        assert_eq!(
            line(""),
            None,
            "an empty /proc/uptime is a failure to report, not an uptime to print"
        );
    }

    #[test]
    fn format_garbage_first_field_is_zero() {
        // This test used to assert `"up 00:00"` -- that garbage input
        // produces a machine which booted this minute. It certified the
        // defect: a plausible reading of a real system, generated from a
        // file that said nothing of the kind, and indistinguishable from
        // the true answer on a freshly-booted host.
        assert_eq!(
            line("garbage 0"),
            None,
            "a first field that is not a number is a failure, not zero seconds"
        );
        assert_eq!(line("nan 0"), None);
        assert_eq!(line("-1 0"), None);
    }

    #[test]
    fn format_just_seconds_no_idle() {
        assert_eq!(line("120"), Some("up 00:02".to_string()));
    }

    #[test]
    fn format_multiple_whitespace() {
        assert_eq!(line("60.0   30.0"), Some("up 00:01".to_string()));
    }

    #[test]
    fn format_zero_seconds() {
        assert_eq!(line("0 0"), Some("up 00:00".to_string()));
    }

    #[test]
    fn format_almost_one_day_no_days_prefix() {
        let total = 86399; // 23:59:59
        let s = format!("{total} 0");
        assert_eq!(line(&s), Some("up 23:59".to_string()));
    }
}
