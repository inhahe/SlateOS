//! strings — find printable strings in binary files.
//!
//! Usage: strings [-n MIN] [FILE...]
//!   -n MIN   minimum string length (default: 4)
//!   Prints all sequences of printable characters of length >= MIN.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct StringsArgs {
    min_len: usize,
    files: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut parsed = parse_args(&args);

    if parsed.files.is_empty() {
        parsed.files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &parsed.files {
        let mut reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("strings: {path}: {e}");
                    continue;
                }
            }
        };

        let mut data = Vec::new();
        if reader.read_to_end(&mut data).is_err() {
            eprintln!("strings: {path}: read error");
            continue;
        }

        for run in extract_strings(&data, parsed.min_len) {
            let _ = out.write_all(&run);
            let _ = writeln!(out);
        }
    }
}

/// Parse strings's argv. Supports `-n N` and the shorthand `-N`.
fn parse_args(args: &[String]) -> StringsArgs {
    let mut min_len: usize = 4;
    let mut files: Vec<String> = Vec::new();
    let mut i: usize = 0;

    while i < args.len() {
        let arg = args.get(i).map(String::as_str).unwrap_or("");
        if arg == "-n" && i.saturating_add(1) < args.len() {
            if let Some(v) = args.get(i.saturating_add(1)) {
                min_len = v.parse().unwrap_or(4);
            }
            i = i.saturating_add(2);
        } else if arg.starts_with('-') && arg.len() > 1 {
            if let Ok(n) = arg.get(1..).unwrap_or("").parse::<usize>() {
                min_len = n;
            }
            i = i.saturating_add(1);
        } else {
            files.push(arg.to_string());
            i = i.saturating_add(1);
        }
    }

    StringsArgs { min_len, files }
}

/// Walk `data` and return every run of printable bytes of length >= `min_len`.
fn extract_strings(data: &[u8], min_len: usize) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for &b in data {
        if is_printable(b) {
            current.push(b);
        } else {
            if current.len() >= min_len {
                out.push(current.clone());
            }
            current.clear();
        }
    }
    if current.len() >= min_len {
        out.push(current);
    }
    out
}

/// Printable ASCII (space through tilde) plus tab.
fn is_printable(b: u8) -> bool {
    (0x20..=0x7e).contains(&b) || b == b'\t'
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    fn b(items: &[&[u8]]) -> Vec<Vec<u8>> {
        items.iter().map(|x| x.to_vec()).collect()
    }

    #[test]
    fn parse_defaults() {
        let a = parse_args(&s(&[]));
        assert_eq!(a.min_len, 4);
        assert!(a.files.is_empty());
    }

    #[test]
    fn parse_dash_n_value() {
        let a = parse_args(&s(&["-n", "8", "file"]));
        assert_eq!(a.min_len, 8);
        assert_eq!(a.files, vec!["file"]);
    }

    #[test]
    fn parse_dash_number_shorthand() {
        let a = parse_args(&s(&["-6", "file"]));
        assert_eq!(a.min_len, 6);
        assert_eq!(a.files, vec!["file"]);
    }

    #[test]
    fn parse_dash_n_invalid_keeps_default() {
        let a = parse_args(&s(&["-n", "abc"]));
        assert_eq!(a.min_len, 4);
    }

    #[test]
    fn parse_dash_n_at_end_dropped() {
        // "-n" with no following value is treated as a no-op.
        let a = parse_args(&s(&["-n"]));
        assert_eq!(a.min_len, 4);
    }

    #[test]
    fn parse_files_only() {
        let a = parse_args(&s(&["a", "b"]));
        assert_eq!(a.files, vec!["a", "b"]);
    }

    #[test]
    fn parse_mixed_order() {
        let a = parse_args(&s(&["file1", "-n", "5", "file2"]));
        assert_eq!(a.min_len, 5);
        assert_eq!(a.files, vec!["file1", "file2"]);
    }

    #[test]
    fn printable_basic() {
        assert!(is_printable(b'A'));
        assert!(is_printable(b' '));
        assert!(is_printable(b'~'));
        assert!(is_printable(b'\t'));
    }

    #[test]
    fn unprintable_basic() {
        assert!(!is_printable(0));
        assert!(!is_printable(b'\n'));
        assert!(!is_printable(0x1f));
        assert!(!is_printable(0x7f));
        assert!(!is_printable(0xff));
    }

    #[test]
    fn extract_empty_returns_empty() {
        assert!(extract_strings(&[], 4).is_empty());
    }

    #[test]
    fn extract_all_printable_single_run() {
        let data = b"hello world";
        let out = extract_strings(data, 4);
        assert_eq!(out, b(&[b"hello world"]));
    }

    #[test]
    fn extract_drops_short_runs() {
        let data = b"hi\0there\0";
        let out = extract_strings(data, 4);
        assert_eq!(out, b(&[b"there"]));
    }

    #[test]
    fn extract_multiple_runs() {
        let data = b"foo\0bar\0baz\0";
        let out = extract_strings(data, 3);
        assert_eq!(out, b(&[b"foo", b"bar", b"baz"]));
    }

    #[test]
    fn extract_min_len_threshold() {
        let data = b"abc\0abcd\0abcde";
        // min 4 -> "abcd" and "abcde".
        let out = extract_strings(data, 4);
        assert_eq!(out, b(&[b"abcd", b"abcde"]));
    }

    #[test]
    fn extract_trailing_run_no_terminator() {
        let data = b"\0\0hello";
        let out = extract_strings(data, 4);
        assert_eq!(out, b(&[b"hello"]));
    }

    #[test]
    fn extract_min_zero_keeps_everything() {
        // min 0 should not skip single-byte runs. Note: empty current at the
        // delimiter is also flushed (len 0 >= 0), but consecutive nulls within
        // the data would be needed to see that. With single nulls between bytes,
        // every printable byte becomes its own run.
        let data = b"a\0b\0c";
        let out = extract_strings(data, 0);
        assert_eq!(out, b(&[b"a", b"b", b"c"]));
    }

    #[test]
    fn extract_min_zero_emits_empty_between_consecutive_nulls() {
        // Two consecutive nulls: the second one flushes an empty current.
        let data = b"a\0\0b";
        let out = extract_strings(data, 0);
        assert_eq!(out, b(&[b"a", b"", b"b"]));
    }

    #[test]
    fn extract_tab_counts_as_printable() {
        let data = b"a\tb\tc\0";
        let out = extract_strings(data, 5);
        assert_eq!(out, b(&[b"a\tb\tc"]));
    }

    #[test]
    fn extract_short_under_threshold_not_emitted() {
        let data = b"abc";
        let out = extract_strings(data, 4);
        assert!(out.is_empty());
    }
}
