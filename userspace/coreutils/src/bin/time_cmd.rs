//! time — run a command and report its execution time.
//!
//! Usage: time COMMAND [ARGS...]
//!   Runs COMMAND and prints elapsed wall-clock time to stderr.
//!
//! Note: This is named time_cmd.rs to avoid conflict with Rust's
//! std::time module. The binary is installed as "time".

use std::env;
use std::process::{self, Command};
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("time: missing command");
        process::exit(1);
    }

    let cmd = &args[0];
    let cmd_args = &args[1..];

    let start = Instant::now();

    let status = match Command::new(cmd).args(cmd_args).status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("time: {cmd}: {e}");
            process::exit(127);
        }
    };

    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs_f64();

    eprintln!();
    eprintln!("real\t{}", format_real_line(total_secs));
    // We can't distinguish user/sys time without kernel support,
    // so just show real time.

    process::exit(status.code().unwrap_or(126));
}

/// Format the `Mm.SSSs` real-time line.  Negative or NaN/Infinity values are
/// clamped to zero seconds.
fn format_real_line(total_secs: f64) -> String {
    let total = if total_secs.is_finite() && total_secs > 0.0 {
        total_secs
    } else {
        0.0
    };
    let mins = (total / 60.0) as u64;
    let secs = total % 60.0;
    format!("{mins}m{secs:.3}s")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn zero_seconds() {
        assert_eq!(format_real_line(0.0), "0m0.000s");
    }

    #[test]
    fn negative_clamped_to_zero() {
        assert_eq!(format_real_line(-5.0), "0m0.000s");
    }

    #[test]
    fn nan_clamped_to_zero() {
        assert_eq!(format_real_line(f64::NAN), "0m0.000s");
    }

    #[test]
    fn infinity_clamped_to_zero() {
        assert_eq!(format_real_line(f64::INFINITY), "0m0.000s");
    }

    #[test]
    fn one_second() {
        assert_eq!(format_real_line(1.0), "0m1.000s");
    }

    #[test]
    fn fractional_seconds() {
        assert_eq!(format_real_line(0.123), "0m0.123s");
    }

    #[test]
    fn exactly_one_minute() {
        assert_eq!(format_real_line(60.0), "1m0.000s");
    }

    #[test]
    fn one_minute_plus_change() {
        assert_eq!(format_real_line(75.5), "1m15.500s");
    }

    #[test]
    fn many_minutes() {
        // 7 minutes 30.25 seconds.
        assert_eq!(format_real_line(450.25), "7m30.250s");
    }

    #[test]
    fn just_under_minute() {
        assert_eq!(format_real_line(59.999), "0m59.999s");
    }

    #[test]
    fn rounds_to_thousandths() {
        // 0.1234 rounds to 0.123 at thousandths precision.
        let s = format_real_line(0.1234);
        assert!(s == "0m0.123s" || s == "0m0.124s"); // banker's or half-up
    }
}
