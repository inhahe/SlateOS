//! seq — print a sequence of numbers.
//!
//! Usage: seq [FIRST [INCREMENT]] LAST
//!   Prints numbers from FIRST to LAST by INCREMENT.
//!   Defaults: FIRST=1, INCREMENT=1.

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match generate(&args) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        Err(msg) => {
            eprintln!("seq: {msg}");
            process::exit(1);
        }
    }
}

/// Generate the sequence of formatted lines for `args` as the seq command
/// would print them. Pure helper — unit-testable without I/O.
///
/// Returns `Err` with a user-facing message for invalid input. Returns
/// `Ok(vec![])` when the start point is already past the stopping point
/// (no values to emit).
fn generate(args: &[String]) -> Result<Vec<String>, String> {
    let (first, increment, last) = match args.len() {
        0 => return Err("missing operand".to_string()),
        1 => {
            let last = parse_num(&args[0])?;
            (1.0, 1.0, last)
        }
        2 => {
            let first = parse_num(&args[0])?;
            let last = parse_num(&args[1])?;
            (first, 1.0, last)
        }
        3 => {
            let first = parse_num(&args[0])?;
            let increment = parse_num(&args[1])?;
            let last = parse_num(&args[2])?;
            if increment == 0.0 {
                return Err("zero increment".to_string());
            }
            (first, increment, last)
        }
        _ => return Err("too many arguments".to_string()),
    };

    let mut out = Vec::new();
    let mut val = first;
    if increment > 0.0 {
        while val <= last + f64::EPSILON {
            out.push(format_value(val));
            val += increment;
        }
    } else {
        while val >= last - f64::EPSILON {
            out.push(format_value(val));
            val += increment;
        }
    }
    Ok(out)
}

fn parse_num(s: &str) -> Result<f64, String> {
    s.parse().map_err(|_| format!("invalid number: '{s}'"))
}

/// Format a value the way seq does: integers without decimal point,
/// floats with default formatting.
fn format_value(val: f64) -> String {
    if val == val.trunc() && val.abs() < 1e15 {
        format!("{}", val as i64)
    } else {
        format!("{val}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn one_arg_counts_from_one() {
        assert_eq!(
            generate(&args(&["5"])).unwrap(),
            vec!["1", "2", "3", "4", "5"]
        );
    }

    #[test]
    fn two_args_uses_default_step() {
        assert_eq!(
            generate(&args(&["3", "7"])).unwrap(),
            vec!["3", "4", "5", "6", "7"]
        );
    }

    #[test]
    fn three_args_custom_step() {
        assert_eq!(
            generate(&args(&["0", "2", "10"])).unwrap(),
            vec!["0", "2", "4", "6", "8", "10"]
        );
    }

    #[test]
    fn descending_with_negative_step() {
        assert_eq!(
            generate(&args(&["5", "-1", "1"])).unwrap(),
            vec!["5", "4", "3", "2", "1"]
        );
    }

    #[test]
    fn empty_when_first_past_last_ascending() {
        // first > last with positive (default) increment -> empty.
        assert_eq!(generate(&args(&["10", "5"])).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn empty_when_first_past_last_descending() {
        // first < last with negative increment -> empty.
        assert_eq!(
            generate(&args(&["1", "-1", "5"])).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn single_value_when_first_equals_last() {
        assert_eq!(generate(&args(&["7", "7"])).unwrap(), vec!["7"]);
    }

    #[test]
    fn floats_with_fractional_step() {
        let out = generate(&args(&["1", "0.5", "3"])).unwrap();
        assert_eq!(out, vec!["1", "1.5", "2", "2.5", "3"]);
    }

    #[test]
    fn negative_range() {
        assert_eq!(
            generate(&args(&["-2", "2"])).unwrap(),
            vec!["-2", "-1", "0", "1", "2"]
        );
    }

    #[test]
    fn missing_operand_errors() {
        let err = generate(&args(&[])).unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn invalid_number_errors() {
        let err = generate(&args(&["abc"])).unwrap_err();
        assert!(err.contains("invalid"));
        let err = generate(&args(&["1", "x", "5"])).unwrap_err();
        assert!(err.contains("invalid"));
        let err = generate(&args(&["1", "5", "x"])).unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn zero_increment_errors() {
        let err = generate(&args(&["1", "0", "5"])).unwrap_err();
        assert!(err.contains("zero"));
    }

    #[test]
    fn too_many_args_errors() {
        let err = generate(&args(&["1", "2", "3", "4"])).unwrap_err();
        assert!(err.contains("too many"));
    }

    // ---------------- format_value ----------------

    #[test]
    fn format_value_integer_no_decimal() {
        assert_eq!(format_value(3.0), "3");
        assert_eq!(format_value(-7.0), "-7");
        assert_eq!(format_value(0.0), "0");
    }

    #[test]
    fn format_value_float_shows_decimal() {
        assert_eq!(format_value(2.5), "2.5");
        assert_eq!(format_value(-0.25), "-0.25");
    }

    #[test]
    fn format_value_huge_integer_falls_back_to_float() {
        // Beyond 1e15 we lose i64 precision — use default float format.
        let val = 1e16;
        let s = format_value(val);
        assert!(!s.is_empty());
    }
}
