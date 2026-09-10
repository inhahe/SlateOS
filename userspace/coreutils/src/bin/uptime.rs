//! uptime — tell how long the system has been running.
//!
//! Usage: uptime
//!   Reads /proc/uptime for system uptime information.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::stdfd;
use std::fs;
use std::process::ExitCode;

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
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
    match format_uptime_line(&content) {
        Some(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        None => {
            diag!("uptime: /proc/uptime is not a number of seconds");
            ExitCode::FAILURE
        }
    }
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
fn format_uptime_line(content: &str) -> Option<String> {
    let parts: Vec<&str> = content.split_whitespace().collect();
    let secs_str = parts.first()?;
    let total_secs: f64 = secs_str.parse().ok()?;
    if !total_secs.is_finite() || total_secs < 0.0 {
        return None;
    }
    let (days, hours, mins) = split_uptime(total_secs);
    let mut out = String::from("up ");
    if days > 0 {
        let suffix = if days == 1 { "" } else { "s" };
        out.push_str(&format!("{days} day{suffix}, "));
    }
    out.push_str(&format!("{hours:02}:{mins:02}"));
    Some(out)
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
        assert_eq!(
            format_uptime_line("60.0 30.0"),
            Some("up 00:01".to_string())
        );
    }

    #[test]
    fn format_one_day_singular() {
        let s = format!("{} 0", 86400);
        assert_eq!(format_uptime_line(&s), Some("up 1 day, 00:00".to_string()));
    }

    #[test]
    fn format_two_days_plural() {
        let total = 2 * 86400 + 3 * 3600 + 5 * 60;
        let s = format!("{total} 0");
        assert_eq!(format_uptime_line(&s), Some("up 2 days, 03:05".to_string()));
    }

    #[test]
    fn format_empty_returns_unknown() {
        assert_eq!(
            format_uptime_line(""),
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
            format_uptime_line("garbage 0"),
            None,
            "a first field that is not a number is a failure, not zero seconds"
        );
        assert_eq!(format_uptime_line("nan 0"), None);
        assert_eq!(format_uptime_line("-1 0"), None);
    }

    #[test]
    fn format_just_seconds_no_idle() {
        assert_eq!(format_uptime_line("120"), Some("up 00:02".to_string()));
    }

    #[test]
    fn format_multiple_whitespace() {
        assert_eq!(
            format_uptime_line("60.0   30.0"),
            Some("up 00:01".to_string())
        );
    }

    #[test]
    fn format_zero_seconds() {
        assert_eq!(format_uptime_line("0 0"), Some("up 00:00".to_string()));
    }

    #[test]
    fn format_almost_one_day_no_days_prefix() {
        let total = 86399; // 23:59:59
        let s = format!("{total} 0");
        assert_eq!(format_uptime_line(&s), Some("up 23:59".to_string()));
    }
}
