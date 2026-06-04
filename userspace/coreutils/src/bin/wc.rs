//! wc — word, line, character, and byte count.
//!
//! Usage: wc [-l] [-w] [-c] [-m] [FILE...]
//!   -l  count lines
//!   -w  count words
//!   -c  count bytes
//!   -m  count characters
//!   If no flags, show all of: lines words bytes.
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Counts {
    lines: usize,
    words: usize,
    bytes: usize,
    chars: usize,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut show_lines = false;
    let mut show_words = false;
    let mut show_bytes = false;
    let mut show_chars = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'l' => show_lines = true,
                    'w' => show_words = true,
                    'c' => show_bytes = true,
                    'm' => show_chars = true,
                    _ => eprintln!("wc: unknown option: -{c}"),
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    // Default: show everything except chars
    if !show_lines && !show_words && !show_bytes && !show_chars {
        show_lines = true;
        show_words = true;
        show_bytes = true;
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut total = Counts { lines: 0, words: 0, bytes: 0, chars: 0 };

    for path in &files {
        let mut reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("wc: {path}: {e}");
                    continue;
                }
            }
        };

        let mut data = Vec::new();
        if let Err(e) = reader.read_to_end(&mut data) {
            eprintln!("wc: {path}: {e}");
            continue;
        }

        let counts = count_data(&data);
        total.lines += counts.lines;
        total.words += counts.words;
        total.bytes += counts.bytes;
        total.chars += counts.chars;

        print_counts(&mut out, &counts, path, show_lines, show_words, show_bytes, show_chars);
    }

    if files.len() > 1 {
        print_counts(&mut out, &total, "total", show_lines, show_words, show_bytes, show_chars);
    }
}

fn count_data(data: &[u8]) -> Counts {
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    let bytes = data.len();

    // Count words (runs of non-whitespace)
    let mut words = 0;
    let mut in_word = false;
    for &b in data {
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            in_word = false;
        } else if !in_word {
            in_word = true;
            words += 1;
        }
    }

    // Count UTF-8 characters
    let text = String::from_utf8_lossy(data);
    let chars = text.chars().count();

    Counts { lines, words, bytes, chars }
}

fn print_counts(
    out: &mut impl Write,
    c: &Counts,
    name: &str,
    lines: bool,
    words: bool,
    bytes: bool,
    chars: bool,
) {
    if lines { let _ = write!(out, "{:>7} ", c.lines); }
    if words { let _ = write!(out, "{:>7} ", c.words); }
    if bytes { let _ = write!(out, "{:>7} ", c.bytes); }
    if chars { let _ = write!(out, "{:>7} ", c.chars); }
    let _ = writeln!(out, "{name}");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- count_data ----------------

    #[test]
    fn count_empty_data() {
        let c = count_data(b"");
        assert_eq!(
            c,
            Counts {
                lines: 0,
                words: 0,
                bytes: 0,
                chars: 0,
            }
        );
    }

    #[test]
    fn count_single_line_no_newline() {
        let c = count_data(b"hello");
        assert_eq!(c.lines, 0); // lines counts newline chars
        assert_eq!(c.words, 1);
        assert_eq!(c.bytes, 5);
        assert_eq!(c.chars, 5);
    }

    #[test]
    fn count_single_line_with_newline() {
        let c = count_data(b"hello\n");
        assert_eq!(c.lines, 1);
        assert_eq!(c.words, 1);
        assert_eq!(c.bytes, 6);
    }

    #[test]
    fn count_multiple_lines() {
        let c = count_data(b"one\ntwo\nthree\n");
        assert_eq!(c.lines, 3);
        assert_eq!(c.words, 3);
        assert_eq!(c.bytes, 14);
    }

    #[test]
    fn count_words_multiple_per_line() {
        let c = count_data(b"the quick brown fox\n");
        assert_eq!(c.lines, 1);
        assert_eq!(c.words, 4);
    }

    #[test]
    fn count_words_runs_of_whitespace() {
        // Multiple spaces between words count as a single separator.
        let c = count_data(b"a   b\tc\n");
        assert_eq!(c.words, 3);
    }

    #[test]
    fn count_words_leading_and_trailing_whitespace() {
        let c = count_data(b"   hello world   ");
        assert_eq!(c.words, 2);
    }

    #[test]
    fn count_chars_utf8_vs_bytes() {
        // "héllo" — 'é' is 2 bytes in UTF-8.
        let data = "héllo".as_bytes();
        let c = count_data(data);
        assert_eq!(c.bytes, 6);
        assert_eq!(c.chars, 5);
    }

    #[test]
    fn count_chars_emoji() {
        // "🌍" is 4 bytes, 1 char.
        let data = "🌍".as_bytes();
        let c = count_data(data);
        assert_eq!(c.bytes, 4);
        assert_eq!(c.chars, 1);
    }

    #[test]
    fn count_only_whitespace() {
        let c = count_data(b"   \t\n  \n");
        assert_eq!(c.lines, 2);
        assert_eq!(c.words, 0);
        assert_eq!(c.bytes, 8);
    }

    #[test]
    fn count_carriage_return_treated_as_separator() {
        // wc treats \r as whitespace for word counting.
        let c = count_data(b"a\rb\rc\n");
        assert_eq!(c.words, 3);
    }

    // ---------------- print_counts ----------------

    #[test]
    fn print_counts_all_columns() {
        let c = Counts { lines: 3, words: 12, bytes: 50, chars: 50 };
        let mut buf = Vec::new();
        print_counts(&mut buf, &c, "file.txt", true, true, true, true);
        let s = String::from_utf8(buf).unwrap();
        // Right-aligned width 7 columns followed by name + newline.
        assert!(s.contains("3"));
        assert!(s.contains("12"));
        assert!(s.contains("50"));
        assert!(s.ends_with("file.txt\n"));
    }

    #[test]
    fn print_counts_only_lines() {
        let c = Counts { lines: 42, words: 0, bytes: 0, chars: 0 };
        let mut buf = Vec::new();
        print_counts(&mut buf, &c, "x", true, false, false, false);
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "     42 x\n");
    }

    #[test]
    fn print_counts_total_label() {
        let c = Counts { lines: 100, words: 200, bytes: 300, chars: 300 };
        let mut buf = Vec::new();
        print_counts(&mut buf, &c, "total", true, true, true, false);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with("total\n"));
        assert!(s.contains("100"));
        assert!(s.contains("200"));
        assert!(s.contains("300"));
    }
}
