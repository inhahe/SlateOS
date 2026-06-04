//! nl — number lines of files.
//!
//! Usage: nl [-b a|t|n] [-w WIDTH] [FILE...]
//!   -b a   number all lines (default)
//!   -b t   number only non-empty lines
//!   -b n   no numbering
//!   -w N   width of line number field (default: 6)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut body_num = 'a'; // a=all, t=non-empty, n=none
    let mut width: usize = 6;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-b" => {
                i += 1;
                if i < args.len() {
                    body_num = args[i].chars().next().unwrap_or('a');
                }
            }
            "-w" => {
                i += 1;
                if i < args.len() {
                    width = args[i].parse().unwrap_or(6);
                }
            }
            arg if !arg.starts_with('-') || arg == "-" => {
                files.push(arg.to_string());
            }
            _ => {
                // Ignore unknown flags gracefully
            }
        }
        i += 1;
    }

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
                    eprintln!("nl: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for line_result in buf.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let (formatted, advanced) = format_line(&line, line_num, width, body_num);
            let _ = writeln!(out, "{formatted}");
            if advanced {
                line_num += 1;
            }
        }
    }
}

/// Decide whether to number `line` based on `body_num` (a/t/n) and emit
/// the formatted line. Returns the formatted string and a flag indicating
/// whether the line counter should advance.
///
/// Pure helper — unit-testable without I/O.
fn format_line(line: &str, line_num: usize, width: usize, body_num: char) -> (String, bool) {
    let should_number = match body_num {
        'a' => true,
        't' => !line.is_empty(),
        'n' => false,
        _ => true,
    };

    if should_number {
        (format!("{line_num:>width$}\t{line}"), true)
    } else {
        (format!("{:>width$}\t{line}", ""), false)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn body_a_numbers_all_lines_including_empty() {
        let (s, adv) = format_line("hello", 1, 6, 'a');
        assert_eq!(s, "     1\thello");
        assert!(adv);

        let (s, adv) = format_line("", 5, 6, 'a');
        assert_eq!(s, "     5\t");
        assert!(adv);
    }

    #[test]
    fn body_t_skips_empty_lines() {
        let (s, adv) = format_line("hello", 1, 6, 't');
        assert_eq!(s, "     1\thello");
        assert!(adv);

        let (s, adv) = format_line("", 2, 6, 't');
        // Empty line gets blank number padding, counter does NOT advance.
        assert_eq!(s, "      \t");
        assert!(!adv);
    }

    #[test]
    fn body_n_never_numbers() {
        let (s, adv) = format_line("hello", 1, 6, 'n');
        assert_eq!(s, "      \thello");
        assert!(!adv);

        let (s, adv) = format_line("", 1, 6, 'n');
        assert_eq!(s, "      \t");
        assert!(!adv);
    }

    #[test]
    fn unknown_body_mode_defaults_to_numbering_all() {
        let (s, adv) = format_line("hello", 42, 6, '?');
        assert_eq!(s, "    42\thello");
        assert!(adv);
    }

    #[test]
    fn width_padding_right_aligned() {
        let (s, _) = format_line("x", 3, 4, 'a');
        assert_eq!(s, "   3\tx");
    }

    #[test]
    fn width_wider_than_number() {
        let (s, _) = format_line("x", 1, 10, 'a');
        assert_eq!(s, "         1\tx");
    }

    #[test]
    fn large_line_number_overflows_width() {
        // Numbers wider than `width` are not truncated; they take their
        // natural width.
        let (s, _) = format_line("x", 12345, 3, 'a');
        assert_eq!(s, "12345\tx");
    }
}
