//! sort -- sort lines of text.
//!
//! Usage: sort [-r] [-n] [-u] [FILE...]
//!   -r  reverse the result of comparisons
//!   -n  compare according to numeric value
//!   -u  output only unique lines
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let SortFlags {
        reverse,
        numeric,
        unique,
        mut files,
    } = match parse_flags(&args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("sort: {e}");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        files.push("-".to_string());
    }

    // Collect all lines from all input files.
    let mut lines: Vec<String> = Vec::new();
    for path in &files {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("sort: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for line in buf.lines() {
            match line {
                Ok(l) => lines.push(l),
                Err(e) => {
                    eprintln!("sort: {path}: {e}");
                    break;
                }
            }
        }
    }

    sort_lines(&mut lines, numeric, reverse);
    let output = dedup_if(lines, unique);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in &output {
        let _ = writeln!(out, "{line}");
    }
}

/// Parsed flag state for `sort`.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct SortFlags {
    reverse: bool,
    numeric: bool,
    unique: bool,
    files: Vec<String>,
}

/// Parse `sort`'s argv. Accepts bundled short flags (`-rn`, `-ru`, etc.).
/// `-` alone is a file (stdin marker), not a flag.
fn parse_flags(args: &[String]) -> Result<SortFlags, String> {
    let mut reverse = false;
    let mut numeric = false;
    let mut unique = false;
    let mut files: Vec<String> = Vec::new();

    for arg in args {
        if arg.len() > 1
            && arg != "-"
            && let Some(rest) = arg.strip_prefix('-')
        {
            for c in rest.chars() {
                match c {
                    'r' => reverse = true,
                    'n' => numeric = true,
                    'u' => unique = true,
                    _ => return Err(format!("unknown option: -{c}")),
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    Ok(SortFlags {
        reverse,
        numeric,
        unique,
        files,
    })
}

/// Sort `lines` in place using either lexicographic or `-n` numeric order,
/// reversing the result if `reverse` is set.
fn sort_lines(lines: &mut [String], numeric: bool, reverse: bool) {
    if numeric {
        lines.sort_by(|a, b| {
            let na = parse_leading_number(a);
            let nb = parse_leading_number(b);
            let cmp = na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal);
            if reverse { cmp.reverse() } else { cmp }
        });
    } else {
        lines.sort();
        if reverse {
            lines.reverse();
        }
    }
}

/// If `unique` is set, drop adjacent duplicates (POSIX `sort -u` semantics
/// — must follow a sort pass). Otherwise return unchanged.
fn dedup_if(mut lines: Vec<String>, unique: bool) -> Vec<String> {
    if unique {
        lines.dedup();
    }
    lines
}

/// Parse the leading numeric value from a string for -n comparison.
/// Returns 0.0 for non-numeric strings (matching GNU sort behavior).
fn parse_leading_number(s: &str) -> f64 {
    let trimmed = s.trim_start();
    if trimmed.is_empty() {
        return 0.0;
    }

    // Find the longest prefix that looks like a number.
    let mut end = 0;
    let bytes = trimmed.as_bytes();
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }
    let mut saw_dot = false;
    while end < bytes.len() {
        if bytes[end].is_ascii_digit() {
            end += 1;
        } else if bytes[end] == b'.' && !saw_dot {
            saw_dot = true;
            end += 1;
        } else {
            break;
        }
    }

    if end == 0 {
        return 0.0;
    }

    trimmed.get(..end).unwrap_or("").parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_flags ----------------

    #[test]
    fn flags_default_off() {
        let f = parse_flags(&s(&[])).unwrap();
        assert!(!f.reverse && !f.numeric && !f.unique);
        assert!(f.files.is_empty());
    }

    #[test]
    fn flags_r_n_u_separate() {
        let f = parse_flags(&s(&["-r", "-n", "-u"])).unwrap();
        assert!(f.reverse && f.numeric && f.unique);
    }

    #[test]
    fn flags_bundled() {
        let f = parse_flags(&s(&["-rnu"])).unwrap();
        assert!(f.reverse && f.numeric && f.unique);
    }

    #[test]
    fn flags_unknown_returns_error() {
        let err = parse_flags(&s(&["-z"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn flags_dash_alone_is_file() {
        let f = parse_flags(&s(&["-"])).unwrap();
        assert!(!f.reverse && !f.numeric && !f.unique);
        assert_eq!(f.files, vec!["-"]);
    }

    #[test]
    fn flags_collects_files() {
        let f = parse_flags(&s(&["-r", "a.txt", "b.txt"])).unwrap();
        assert!(f.reverse);
        assert_eq!(f.files, vec!["a.txt", "b.txt"]);
    }

    // ---------------- sort_lines ----------------

    #[test]
    fn sort_lexicographic_ascending() {
        let mut v = s(&["b", "a", "c"]);
        sort_lines(&mut v, false, false);
        assert_eq!(v, s(&["a", "b", "c"]));
    }

    #[test]
    fn sort_lexicographic_reverse() {
        let mut v = s(&["b", "a", "c"]);
        sort_lines(&mut v, false, true);
        assert_eq!(v, s(&["c", "b", "a"]));
    }

    #[test]
    fn sort_numeric_ascending() {
        // Lexicographic would sort "10" < "2", numeric should give 2 < 10.
        let mut v = s(&["10", "2", "1"]);
        sort_lines(&mut v, true, false);
        assert_eq!(v, s(&["1", "2", "10"]));
    }

    #[test]
    fn sort_numeric_reverse() {
        let mut v = s(&["1", "10", "2"]);
        sort_lines(&mut v, true, true);
        assert_eq!(v, s(&["10", "2", "1"]));
    }

    #[test]
    fn sort_numeric_non_numeric_lines_compare_as_zero() {
        let mut v = s(&["foo", "5", "-3", "bar"]);
        sort_lines(&mut v, true, false);
        // -3 < 0 (foo, bar) < 5; foo and bar both tie at 0 (stable sort
        // preserves insertion order: foo before bar).
        assert_eq!(v.first(), Some(&"-3".to_string()));
        assert_eq!(v.last(), Some(&"5".to_string()));
    }

    #[test]
    fn sort_empty_no_panic() {
        let mut v: Vec<String> = Vec::new();
        sort_lines(&mut v, false, false);
        sort_lines(&mut v, true, true);
        assert!(v.is_empty());
    }

    // ---------------- dedup_if ----------------

    #[test]
    fn dedup_off_keeps_duplicates() {
        let v = s(&["a", "a", "b"]);
        assert_eq!(dedup_if(v, false), s(&["a", "a", "b"]));
    }

    #[test]
    fn dedup_on_removes_adjacent() {
        let v = s(&["a", "a", "b", "b", "b", "c"]);
        assert_eq!(dedup_if(v, true), s(&["a", "b", "c"]));
    }

    #[test]
    fn dedup_does_not_remove_non_adjacent() {
        // dedup is adjacent-only — a non-adjacent duplicate stays.
        let v = s(&["a", "b", "a"]);
        assert_eq!(dedup_if(v, true), s(&["a", "b", "a"]));
    }

    // ---------------- parse_leading_number ----------------

    #[test]
    fn parse_num_simple_integer() {
        assert_eq!(parse_leading_number("123"), 123.0);
    }

    #[test]
    fn parse_num_negative() {
        assert_eq!(parse_leading_number("-42"), -42.0);
    }

    #[test]
    fn parse_num_decimal() {
        assert_eq!(parse_leading_number("3.14"), 3.14);
    }

    #[test]
    fn parse_num_leading_whitespace_stripped() {
        assert_eq!(parse_leading_number("   42"), 42.0);
    }

    #[test]
    fn parse_num_stops_at_non_digit() {
        assert_eq!(parse_leading_number("42abc"), 42.0);
    }

    #[test]
    fn parse_num_no_digits_returns_zero() {
        assert_eq!(parse_leading_number("abc"), 0.0);
    }

    #[test]
    fn parse_num_empty_returns_zero() {
        assert_eq!(parse_leading_number(""), 0.0);
        assert_eq!(parse_leading_number("   "), 0.0);
    }

    #[test]
    fn parse_num_only_sign_returns_zero() {
        // "-" with no digits -> parse fails -> 0.0.
        assert_eq!(parse_leading_number("-"), 0.0);
    }

    #[test]
    fn parse_num_two_dots_stop_at_second() {
        // "3.14.15" -> "3.14" -> 3.14
        assert_eq!(parse_leading_number("3.14.15"), 3.14);
    }
}
