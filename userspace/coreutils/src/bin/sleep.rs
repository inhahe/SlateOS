//! sleep -- suspend execution for a specified duration.
//!
//! Usage: sleep SECONDS
//!   SECONDS may be an integer or a decimal number.

use std::env;
use std::process;
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    match parse_seconds(&args) {
        Ok(secs) => thread::sleep(Duration::from_secs_f64(secs)),
        Err(msg) => {
            eprintln!("sleep: {msg}");
            process::exit(1);
        }
    }
}

/// Parse sleep's command-line arguments into a non-negative duration in
/// seconds. Pure helper — unit-testable without I/O.
fn parse_seconds(args: &[String]) -> Result<f64, String> {
    let Some(first) = args.first() else {
        return Err("missing operand".to_string());
    };

    match first.parse::<f64>() {
        Ok(v) if v >= 0.0 && v.is_finite() => Ok(v),
        Ok(_) => Err(format!("invalid time interval '{first}'")),
        Err(_) => Err(format!("invalid time interval '{first}'")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn integer_seconds() {
        assert_eq!(parse_seconds(&args(&["5"])).unwrap(), 5.0);
    }

    #[test]
    fn fractional_seconds() {
        assert_eq!(parse_seconds(&args(&["0.25"])).unwrap(), 0.25);
    }

    #[test]
    fn zero_is_allowed() {
        assert_eq!(parse_seconds(&args(&["0"])).unwrap(), 0.0);
    }

    #[test]
    fn missing_operand_errors() {
        let err = parse_seconds(&args(&[])).unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn negative_value_rejected() {
        let err = parse_seconds(&args(&["-1"])).unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn non_numeric_rejected() {
        let err = parse_seconds(&args(&["abc"])).unwrap_err();
        assert!(err.contains("invalid"));
        assert!(err.contains("abc"));
    }

    #[test]
    fn infinity_rejected() {
        let err = parse_seconds(&args(&["inf"])).unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn nan_rejected() {
        let err = parse_seconds(&args(&["NaN"])).unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn extra_args_ignored_takes_first() {
        // Only the first argument is consulted.
        assert_eq!(parse_seconds(&args(&["3", "ignored"])).unwrap(), 3.0);
    }
}
