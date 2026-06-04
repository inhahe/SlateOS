//! tail -- output the last part of files.
//!
//! Usage: tail [-n COUNT] [FILE...]
//!   -n COUNT  print last COUNT lines (default 10)
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let TailArgs { count, mut files } = parse_args(&args);

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
                    eprintln!("tail: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        let lines: Vec<String> = buf
            .lines()
            .map_while(|r| match r {
                Ok(l) => Some(l),
                Err(e) => {
                    eprintln!("tail: {path}: {e}");
                    None
                }
            })
            .collect();

        for line in last_n(&lines, count) {
            let _ = writeln!(out, "{line}");
        }
    }
}

/// Parsed command-line state for `tail`.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct TailArgs {
    count: usize,
    files: Vec<String>,
}

/// Parse `tail`'s arguments. Recognized forms:
/// - `-n COUNT` (separate arg)
/// - `-nCOUNT` (joined)
/// - `-COUNT` (shorthand)
/// - `-` (stdin marker, treated as file)
/// - anything else is treated as a file path
fn parse_args(args: &[String]) -> TailArgs {
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
            i = i.saturating_add(1);
        } else if let Some(rest) = arg.strip_prefix("-n") {
            count = rest.parse().unwrap_or(10);
            i = i.saturating_add(1);
        } else if arg.len() > 1
            && arg != "-"
            && let Some(rest) = arg.strip_prefix('-')
        {
            if let Ok(n) = rest.parse::<usize>() {
                count = n;
            }
            i = i.saturating_add(1);
        } else {
            files.push(arg.clone());
            i = i.saturating_add(1);
        }
    }

    TailArgs { count, files }
}

/// Return references to the last `count` lines in `lines`. If `count` is
/// zero, returns an empty slice. If `count >= lines.len()`, returns all
/// lines.
fn last_n(lines: &[String], count: usize) -> &[String] {
    if count == 0 {
        return &[];
    }
    let start = lines.len().saturating_sub(count);
    lines.get(start..).unwrap_or(&[])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

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
    }

    #[test]
    fn dash_n_joined() {
        let p = parse_args(&s(&["-n7"]));
        assert_eq!(p.count, 7);
    }

    #[test]
    fn dash_count_shorthand() {
        let p = parse_args(&s(&["-3"]));
        assert_eq!(p.count, 3);
    }

    #[test]
    fn files_collected() {
        let p = parse_args(&s(&["a.txt", "b.txt"]));
        assert_eq!(p.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn dash_alone_treated_as_file() {
        let p = parse_args(&s(&["-"]));
        assert_eq!(p.files, vec!["-"]);
    }

    #[test]
    fn invalid_count_falls_back() {
        let p = parse_args(&s(&["-n", "junk"]));
        assert_eq!(p.count, 10);
    }

    // ---------------- last_n ----------------

    #[test]
    fn last_n_fewer_lines_than_count() {
        let lines = s(&["a", "b"]);
        assert_eq!(last_n(&lines, 5), &lines[..]);
    }

    #[test]
    fn last_n_more_lines_than_count() {
        let lines = s(&["a", "b", "c", "d", "e"]);
        assert_eq!(last_n(&lines, 2), &lines[3..]);
    }

    #[test]
    fn last_n_exact_match() {
        let lines = s(&["a", "b", "c"]);
        assert_eq!(last_n(&lines, 3), &lines[..]);
    }

    #[test]
    fn last_n_zero_returns_empty() {
        let lines = s(&["a", "b", "c"]);
        assert!(last_n(&lines, 0).is_empty());
    }

    #[test]
    fn last_n_empty_input() {
        let lines: Vec<String> = Vec::new();
        assert!(last_n(&lines, 5).is_empty());
    }

    #[test]
    fn last_n_one_line() {
        let lines = s(&["a", "b", "c"]);
        assert_eq!(last_n(&lines, 1), &lines[2..]);
    }
}
