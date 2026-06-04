//! cat — concatenate and print files.
//!
//! Usage: cat [-n] [FILE...]
//!   -n  number all output lines
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (number_lines, mut files) = parse_args(&args);

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut line_num: usize = 1;

    for path in &files {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("cat: {path}: {e}");
                    continue;
                }
            }
        };

        if number_lines {
            let buf = BufReader::new(reader);
            for line in buf.lines() {
                match line {
                    Ok(l) => {
                        let _ = writeln!(out, "{}", format_numbered_line(line_num, &l));
                        line_num = line_num.saturating_add(1);
                    }
                    Err(e) => {
                        eprintln!("cat: {path}: {e}");
                        break;
                    }
                }
            }
        } else {
            let mut buf = BufReader::new(reader);
            let mut chunk = [0u8; 4096];
            loop {
                match buf.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = out.write_all(chunk.get(..n).unwrap_or(&[]));
                    }
                    Err(e) => {
                        eprintln!("cat: {path}: {e}");
                        break;
                    }
                }
            }
        }
    }
}

/// Parse cat's argv into `(number_lines, files)`.
fn parse_args(args: &[String]) -> (bool, Vec<String>) {
    let mut number_lines = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        if arg == "-n" {
            number_lines = true;
        } else {
            files.push(arg.clone());
        }
    }
    (number_lines, files)
}

/// Format a `-n`-numbered output line: 6-wide right-aligned number,
/// tab, then the line text. Does NOT include the trailing newline
/// (caller uses `writeln!` to add it).
fn format_numbered_line(line_num: usize, line: &str) -> String {
    format!("{line_num:6}\t{line}")
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
    fn parse_no_args() {
        let (n, f) = parse_args(&s(&[]));
        assert!(!n);
        assert!(f.is_empty());
    }

    #[test]
    fn parse_dash_n_only() {
        let (n, f) = parse_args(&s(&["-n"]));
        assert!(n);
        assert!(f.is_empty());
    }

    #[test]
    fn parse_files_only() {
        let (n, f) = parse_args(&s(&["a.txt", "b.txt"]));
        assert!(!n);
        assert_eq!(f, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn parse_n_and_files() {
        let (n, f) = parse_args(&s(&["-n", "a.txt"]));
        assert!(n);
        assert_eq!(f, vec!["a.txt"]);
    }

    #[test]
    fn parse_dash_treated_as_file() {
        // "-" means stdin but it's not "-n", so it goes in files.
        let (n, f) = parse_args(&s(&["-"]));
        assert!(!n);
        assert_eq!(f, vec!["-"]);
    }

    #[test]
    fn parse_duplicate_n_idempotent() {
        let (n, f) = parse_args(&s(&["-n", "-n"]));
        assert!(n);
        assert!(f.is_empty());
    }

    // ---------------- format_numbered_line ----------------

    #[test]
    fn numbered_basic() {
        assert_eq!(format_numbered_line(1, "hello"), "     1\thello");
    }

    #[test]
    fn numbered_empty_line() {
        assert_eq!(format_numbered_line(1, ""), "     1\t");
    }

    #[test]
    fn numbered_padded_to_six() {
        assert_eq!(format_numbered_line(42, "x"), "    42\tx");
        assert_eq!(format_numbered_line(123456, "x"), "123456\tx");
    }

    #[test]
    fn numbered_overflow_width_natural_size() {
        // Number wider than 6 columns: not truncated, takes natural width.
        assert_eq!(format_numbered_line(1234567, "x"), "1234567\tx");
    }
}
