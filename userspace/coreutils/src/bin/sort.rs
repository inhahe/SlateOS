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
    let mut reverse = false;
    let mut numeric = false;
    let mut unique = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 && arg != "-" {
            for c in arg[1..].chars() {
                match c {
                    'r' => reverse = true,
                    'n' => numeric = true,
                    'u' => unique = true,
                    _ => {
                        eprintln!("sort: unknown option: -{c}");
                        std::process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

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

    // Sort.
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

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut prev: Option<&str> = None;
    for line in &lines {
        if unique {
            if let Some(p) = prev {
                if p == line {
                    continue;
                }
            }
        }
        let _ = writeln!(out, "{line}");
        prev = Some(line);
    }
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

    trimmed[..end].parse::<f64>().unwrap_or(0.0)
}
