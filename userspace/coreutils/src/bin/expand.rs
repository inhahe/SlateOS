//! expand — convert tabs to spaces.
//!
//! Usage: expand [-t N] [FILE...]
//!   -t N   set tab width (default: 8)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut tab_width: usize = 8;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-t" && i + 1 < args.len() {
            tab_width = args[i + 1].parse().unwrap_or(8);
            i += 2;
        } else {
            files.push(args[i].clone());
            i += 1;
        }
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &files {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("expand: {path}: {e}");
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
            let _ = writeln!(out, "{}", expand_line(&line, tab_width));
        }
    }
}

/// Replace tab characters with the appropriate number of spaces so that
/// columns align on multiples of `tab_width`. Pure helper — unit-testable
/// without I/O.
///
/// Returns the empty string when `tab_width` is zero, to avoid division
/// by zero or an infinite loop.
fn expand_line(line: &str, tab_width: usize) -> String {
    if tab_width == 0 {
        // POSIX expand rejects width 0 — we just drop tabs to avoid an
        // infinite loop. Callers should validate the argument.
        return line.replace('\t', "");
    }

    let mut out = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = tab_width - (col % tab_width);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn no_tabs_unchanged() {
        assert_eq!(expand_line("hello world", 8), "hello world");
    }

    #[test]
    fn empty_line() {
        assert_eq!(expand_line("", 8), "");
    }

    #[test]
    fn single_tab_at_start_pads_to_width() {
        assert_eq!(expand_line("\tx", 8), "        x");
    }

    #[test]
    fn tab_after_text_aligns_to_next_column() {
        // After "ab" col=2, next tab stop at 8 -> 6 spaces.
        assert_eq!(expand_line("ab\tx", 8), "ab      x");
    }

    #[test]
    fn tab_exactly_at_tab_stop_adds_full_width() {
        // After 8 chars col=8, multiple of 8 -> tab adds full 8 spaces.
        assert_eq!(expand_line("12345678\tX", 8), "12345678        X");
    }

    #[test]
    fn multiple_tabs() {
        // "a\t" -> "a       " (col 8), "b\t" -> "b       " (col 16).
        assert_eq!(expand_line("a\tb\tc", 8), "a       b       c");
    }

    #[test]
    fn tab_width_one_expands_to_single_space() {
        // tab_width=1 always rolls to next column, so each tab -> 1 space.
        assert_eq!(expand_line("a\tb\tc", 1), "a b c");
    }

    #[test]
    fn tab_width_four() {
        // After "ab" col=2, next stop at 4 -> 2 spaces.
        assert_eq!(expand_line("ab\tc", 4), "ab  c");
    }

    #[test]
    fn unicode_chars_count_as_one_column() {
        // The implementation counts chars, not display width.
        // "ñ" is 1 char -> col=1, next tab stop at 8 -> 7 spaces.
        let r = expand_line("ñ\tX", 8);
        assert_eq!(r, "ñ       X");
    }

    #[test]
    fn tab_width_zero_drops_tabs() {
        // Guard branch: avoid divide-by-zero by stripping tabs entirely.
        assert_eq!(expand_line("a\tb\tc", 0), "abc");
    }
}
