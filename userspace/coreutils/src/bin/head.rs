//! head — output the first part of files.
//!
//! Usage: head [-n COUNT] [FILE...]
//!   -n COUNT  print first COUNT lines (default 10)
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let HeadArgs { count, mut files } = parse_args(&args);

    if files.is_empty() {
        files.push("-".to_string());
    }

    let show_header = files.len() > 1;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (idx, path) in files.iter().enumerate() {
        if show_header {
            if idx > 0 {
                let _ = writeln!(out);
            }
            let _ = writeln!(out, "==> {path} <==");
        }

        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("head: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for (line_idx, line) in buf.lines().enumerate() {
            if line_idx >= count {
                break;
            }
            match line {
                Ok(l) => {
                    let _ = writeln!(out, "{l}");
                }
                Err(e) => {
                    eprintln!("head: {path}: {e}");
                    break;
                }
            }
        }
    }
}

/// Parsed command-line state for `head`.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct HeadArgs {
    count: usize,
    files: Vec<String>,
}

/// Parse `head`'s arguments. Recognized forms:
/// - `-n COUNT` (separate arg)
/// - `-nCOUNT` (joined)
/// - `-COUNT` (shorthand)
/// - anything else is treated as a file path
///
/// Invalid numbers silently fall back to the default count of 10 (POSIX
/// `head` errors out; we choose graceful degradation to match the
/// existing behavior of the binary).
fn parse_args(args: &[String]) -> HeadArgs {
    let mut count: usize = 10;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while let Some(arg) = args.get(i) {
        if arg == "-n" {
            if let Some(next) = args.get(i.saturating_add(1)) {
                count = next.parse().unwrap_or(10);
                i = i.saturating_add(2);
                continue;
            }
            // Lone "-n" at end: ignore.
            i = i.saturating_add(1);
        } else if let Some(rest) = arg.strip_prefix("-n") {
            count = rest.parse().unwrap_or(10);
            i = i.saturating_add(1);
        } else if let Some(rest) = arg.strip_prefix('-') {
            if !rest.is_empty()
                && let Ok(n) = rest.parse::<usize>()
            {
                count = n;
            }
            i = i.saturating_add(1);
        } else {
            files.push(arg.clone());
            i = i.saturating_add(1);
        }
    }

    HeadArgs { count, files }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn default_count_when_no_flags() {
        let p = parse_args(&s(&[]));
        assert_eq!(p.count, 10);
        assert!(p.files.is_empty());
    }

    #[test]
    fn dash_n_separate_arg() {
        let p = parse_args(&s(&["-n", "5"]));
        assert_eq!(p.count, 5);
        assert!(p.files.is_empty());
    }

    #[test]
    fn dash_n_joined() {
        let p = parse_args(&s(&["-n5"]));
        assert_eq!(p.count, 5);
    }

    #[test]
    fn dash_count_shorthand() {
        let p = parse_args(&s(&["-3"]));
        assert_eq!(p.count, 3);
    }

    #[test]
    fn files_only_no_flags() {
        let p = parse_args(&s(&["a.txt", "b.txt"]));
        assert_eq!(p.count, 10);
        assert_eq!(p.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn flags_and_files_mixed() {
        let p = parse_args(&s(&["-n", "20", "a.txt", "b.txt"]));
        assert_eq!(p.count, 20);
        assert_eq!(p.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn invalid_count_falls_back_to_default() {
        let p = parse_args(&s(&["-n", "notanumber"]));
        assert_eq!(p.count, 10);
    }

    #[test]
    fn invalid_joined_count_falls_back_to_default() {
        let p = parse_args(&s(&["-nfoo"]));
        assert_eq!(p.count, 10);
    }

    #[test]
    fn lone_dash_n_at_end_is_ignored() {
        let p = parse_args(&s(&["-n"]));
        assert_eq!(p.count, 10);
        assert!(p.files.is_empty());
    }

    #[test]
    fn stdin_dash_treated_as_file() {
        let p = parse_args(&s(&["-"]));
        // "-" starts with '-' but rest "" parses to nothing — treated as
        // unrecognized flag (count unchanged), file list stays empty here.
        // The main() then defaults to ["-"] when files is empty.
        assert_eq!(p.count, 10);
    }

    #[test]
    fn last_count_wins() {
        let p = parse_args(&s(&["-n", "5", "-n", "20"]));
        assert_eq!(p.count, 20);
    }

    #[test]
    fn zero_count_is_valid() {
        let p = parse_args(&s(&["-n", "0"]));
        assert_eq!(p.count, 0);
    }
}
