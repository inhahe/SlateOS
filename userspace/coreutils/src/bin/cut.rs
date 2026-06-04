//! cut — remove sections from each line of files.
//!
//! Usage: cut -d DELIM -f FIELDS [FILE...]
//!        cut -c CHARS [FILE...]
//!   -d DELIM   use DELIM as field delimiter (default TAB)
//!   -f FIELDS  select fields (comma-separated, e.g. 1,3 or 1-3)
//!   -c CHARS   select characters (comma-separated ranges)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut delim = '\t';
    let mut field_spec: Option<String> = None;
    let mut char_spec: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-d" && i + 1 < args.len() {
            let d = &args[i + 1];
            delim = d.chars().next().unwrap_or('\t');
            i += 2;
        } else if args[i].starts_with("-d") && args[i].len() > 2 {
            delim = args[i][2..].chars().next().unwrap_or('\t');
            i += 1;
        } else if args[i] == "-f" && i + 1 < args.len() {
            field_spec = Some(args[i + 1].clone());
            i += 2;
        } else if args[i].starts_with("-f") && args[i].len() > 2 {
            field_spec = Some(args[i][2..].to_string());
            i += 1;
        } else if args[i] == "-c" && i + 1 < args.len() {
            char_spec = Some(args[i + 1].clone());
            i += 2;
        } else if args[i].starts_with("-c") && args[i].len() > 2 {
            char_spec = Some(args[i][2..].to_string());
            i += 1;
        } else {
            files.push(args[i].clone());
            i += 1;
        }
    }

    if field_spec.is_none() && char_spec.is_none() {
        eprintln!("cut: you must specify -f or -c");
        process::exit(1);
    }

    let indices = parse_ranges(field_spec.as_deref().or(char_spec.as_deref()).unwrap_or(""));

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
                    eprintln!("cut: {path}: {e}");
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

            let cut = if field_spec.is_some() {
                cut_line_fields(&line, delim, &indices)
            } else {
                cut_line_chars(&line, &indices)
            };
            let _ = writeln!(out, "{cut}");
        }
    }
}

/// Parse range specs like "1,3,5-7" into a sorted list of 1-based indices.
fn parse_ranges(spec: &str) -> Vec<usize> {
    let mut result = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let start: usize = a.parse().unwrap_or(1);
            let end: usize = b.parse().unwrap_or(start);
            for i in start..=end {
                result.push(i);
            }
        } else if let Ok(n) = part.parse::<usize>() {
            result.push(n);
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

/// Cut a single line by field, returning the joined selected fields.
/// Pure helper — unit-testable without I/O.
fn cut_line_fields(line: &str, delim: char, indices: &[usize]) -> String {
    let fields: Vec<&str> = line.split(delim).collect();
    let mut out = String::new();
    let mut first = true;
    for &idx in indices {
        if idx > 0 && idx <= fields.len() {
            if !first {
                out.push(delim);
            }
            // 1-based -> 0-based index. Bounds checked above.
            if let Some(field) = fields.get(idx - 1) {
                out.push_str(field);
            }
            first = false;
        }
    }
    out
}

/// Cut a single line by character (1-based), returning the selected
/// characters in order.
fn cut_line_chars(line: &str, indices: &[usize]) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    for &idx in indices {
        if idx > 0
            && idx <= chars.len()
            && let Some(&c) = chars.get(idx - 1)
        {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- parse_ranges ----------------

    #[test]
    fn parse_ranges_single() {
        assert_eq!(parse_ranges("3"), vec![3]);
    }

    #[test]
    fn parse_ranges_comma_list() {
        assert_eq!(parse_ranges("1,3,5"), vec![1, 3, 5]);
    }

    #[test]
    fn parse_ranges_range() {
        assert_eq!(parse_ranges("2-5"), vec![2, 3, 4, 5]);
    }

    #[test]
    fn parse_ranges_mixed() {
        assert_eq!(parse_ranges("1,3-5,7"), vec![1, 3, 4, 5, 7]);
    }

    #[test]
    fn parse_ranges_sorted_and_deduped() {
        assert_eq!(parse_ranges("5,1,3,1,5"), vec![1, 3, 5]);
    }

    #[test]
    fn parse_ranges_overlapping_ranges_dedup() {
        assert_eq!(parse_ranges("1-3,2-4"), vec![1, 2, 3, 4]);
    }

    #[test]
    fn parse_ranges_whitespace_trimmed() {
        assert_eq!(parse_ranges(" 1 , 2 , 3 "), vec![1, 2, 3]);
    }

    #[test]
    fn parse_ranges_empty_returns_empty() {
        assert!(parse_ranges("").is_empty());
    }

    #[test]
    fn parse_ranges_invalid_entries_skipped() {
        // Non-numeric isolated entries are skipped; the range fallback
        // applies defaults for missing endpoints.
        let r = parse_ranges("abc,2,xyz,4");
        assert_eq!(r, vec![2, 4]);
    }

    // ---------------- cut_line_fields ----------------

    #[test]
    fn cut_fields_single() {
        assert_eq!(cut_line_fields("a:b:c", ':', &[2]), "b");
    }

    #[test]
    fn cut_fields_multiple() {
        assert_eq!(cut_line_fields("a:b:c:d", ':', &[1, 3]), "a:c");
    }

    #[test]
    fn cut_fields_preserves_order_in_indices() {
        // Indices come in sorted from parse_ranges, so output is in field
        // order (not the user's requested order — matching coreutils).
        assert_eq!(cut_line_fields("a:b:c:d", ':', &[1, 2, 4]), "a:b:d");
    }

    #[test]
    fn cut_fields_out_of_bounds_skipped() {
        // Field 5 doesn't exist in "a:b:c" — skip silently.
        assert_eq!(cut_line_fields("a:b:c", ':', &[2, 5]), "b");
    }

    #[test]
    fn cut_fields_no_delim_in_line_treated_as_single_field() {
        assert_eq!(cut_line_fields("nodelim", ':', &[1]), "nodelim");
        assert_eq!(cut_line_fields("nodelim", ':', &[2]), "");
    }

    #[test]
    fn cut_fields_tab_delim() {
        assert_eq!(cut_line_fields("a\tb\tc", '\t', &[2]), "b");
    }

    #[test]
    fn cut_fields_empty_line() {
        assert_eq!(cut_line_fields("", ':', &[1]), "");
    }

    #[test]
    fn cut_fields_zero_index_ignored() {
        assert_eq!(cut_line_fields("a:b:c", ':', &[0, 2]), "b");
    }

    // ---------------- cut_line_chars ----------------

    #[test]
    fn cut_chars_single() {
        assert_eq!(cut_line_chars("hello", &[1]), "h");
        assert_eq!(cut_line_chars("hello", &[5]), "o");
    }

    #[test]
    fn cut_chars_multiple() {
        assert_eq!(cut_line_chars("hello", &[1, 3, 5]), "hlo");
    }

    #[test]
    fn cut_chars_out_of_bounds_skipped() {
        assert_eq!(cut_line_chars("ab", &[1, 5]), "a");
    }

    #[test]
    fn cut_chars_unicode() {
        // "héllo" — characters, not bytes.
        assert_eq!(cut_line_chars("héllo", &[2]), "é");
        assert_eq!(cut_line_chars("héllo", &[1, 2, 3]), "hél");
    }

    #[test]
    fn cut_chars_empty_indices() {
        assert_eq!(cut_line_chars("hello", &[]), "");
    }

    #[test]
    fn cut_chars_empty_line() {
        assert_eq!(cut_line_chars("", &[1, 2]), "");
    }
}
