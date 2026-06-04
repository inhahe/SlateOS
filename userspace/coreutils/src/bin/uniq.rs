//! uniq — report or filter out repeated lines.
//!
//! Usage: uniq [-c] [-d] [-u] [INPUT [OUTPUT]]
//!   -c  prefix lines by the number of occurrences
//!   -d  only print duplicate lines
//!   -u  only print unique lines

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut count_mode = false;
    let mut duplicates_only = false;
    let mut unique_only = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'c' => count_mode = true,
                    'd' => duplicates_only = true,
                    'u' => unique_only = true,
                    _ => {
                        eprintln!("uniq: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    let reader: Box<dyn Read> = if files.is_empty() || files[0] == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(&files[0]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {}: {e}", files[0]);
                process::exit(1);
            }
        }
    };

    let mut writer: Box<dyn Write> = if files.len() >= 2 {
        match File::create(&files[1]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {}: {e}", files[1]);
                process::exit(1);
            }
        }
    } else {
        Box::new(io::stdout())
    };

    let buf = BufReader::new(reader);
    let mut prev_line: Option<String> = None;
    let mut prev_count: usize = 0;

    for line_result in buf.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("uniq: read error: {e}");
                break;
            }
        };

        match &prev_line {
            Some(prev) if *prev == line => {
                prev_count = prev_count.saturating_add(1);
            }
            _ => {
                if let Some(prev) = &prev_line
                    && should_emit(prev_count, duplicates_only, unique_only)
                {
                    let _ = writer.write_all(
                        format_uniq_line(prev, prev_count, count_mode).as_bytes(),
                    );
                }
                prev_line = Some(line);
                prev_count = 1;
            }
        }
    }

    if let Some(prev) = &prev_line
        && should_emit(prev_count, duplicates_only, unique_only)
    {
        let _ = writer
            .write_all(format_uniq_line(prev, prev_count, count_mode).as_bytes());
    }
}

/// Decide whether a line group should be emitted given the mode flags.
///
/// - `duplicates_only` and `unique_only` together suppress all output (POSIX
///   treats the combination as a contradiction).
/// - `duplicates_only`: only groups with count > 1.
/// - `unique_only`: only groups with count == 1.
/// - Neither: emit every group.
fn should_emit(count: usize, duplicates_only: bool, unique_only: bool) -> bool {
    if duplicates_only && unique_only {
        false
    } else if duplicates_only {
        count > 1
    } else if unique_only {
        count == 1
    } else {
        true
    }
}

/// Format a uniq output line. If `count_mode` is true, the line is prefixed
/// with the count, right-aligned in a 7-character field, then a space, then
/// the text. Always includes a trailing newline.
fn format_uniq_line(line: &str, count: usize, count_mode: bool) -> String {
    if count_mode {
        format!("{count:>7} {line}\n")
    } else {
        format!("{line}\n")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- should_emit ----------------

    #[test]
    fn emit_default_shows_everything() {
        assert!(should_emit(1, false, false));
        assert!(should_emit(2, false, false));
        assert!(should_emit(99, false, false));
    }

    #[test]
    fn emit_duplicates_only_filters_singletons() {
        assert!(!should_emit(1, true, false));
        assert!(should_emit(2, true, false));
        assert!(should_emit(10, true, false));
    }

    #[test]
    fn emit_unique_only_filters_repeats() {
        assert!(should_emit(1, false, true));
        assert!(!should_emit(2, false, true));
        assert!(!should_emit(99, false, true));
    }

    #[test]
    fn emit_both_flags_suppress_all() {
        assert!(!should_emit(1, true, true));
        assert!(!should_emit(2, true, true));
    }

    #[test]
    fn emit_zero_count_under_duplicates_is_false() {
        assert!(!should_emit(0, true, false));
    }

    // ---------------- format_uniq_line ----------------

    #[test]
    fn format_plain_appends_newline() {
        assert_eq!(format_uniq_line("hello", 1, false), "hello\n");
    }

    #[test]
    fn format_plain_ignores_count() {
        // count is irrelevant when count_mode is false.
        assert_eq!(format_uniq_line("hello", 42, false), "hello\n");
    }

    #[test]
    fn format_count_mode_prefixes_count() {
        assert_eq!(format_uniq_line("hello", 1, true), "      1 hello\n");
    }

    #[test]
    fn format_count_mode_right_aligns_in_field() {
        assert_eq!(format_uniq_line("x", 12, true), "     12 x\n");
        assert_eq!(format_uniq_line("x", 1234567, true), "1234567 x\n");
    }

    #[test]
    fn format_count_mode_large_count_overflows_width() {
        // Counts wider than 7 are not truncated; natural width wins.
        assert_eq!(format_uniq_line("x", 12345678, true), "12345678 x\n");
    }

    #[test]
    fn format_empty_line() {
        assert_eq!(format_uniq_line("", 1, false), "\n");
        assert_eq!(format_uniq_line("", 3, true), "      3 \n");
    }
}
