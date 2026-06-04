//! comm — compare two sorted files line by line.
//!
//! Usage: comm [-1] [-2] [-3] FILE1 FILE2
//!   -1  suppress lines unique to FILE1
//!   -2  suppress lines unique to FILE2
//!   -3  suppress lines common to both

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut suppress1 = false;
    let mut suppress2 = false;
    let mut suppress3 = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 && arg.chars().skip(1).all(|c| c.is_ascii_digit())
        {
            for c in arg[1..].chars() {
                match c {
                    '1' => suppress1 = true,
                    '2' => suppress2 = true,
                    '3' => suppress3 = true,
                    _ => {}
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    if files.len() != 2 {
        eprintln!("comm: requires exactly two files");
        process::exit(1);
    }

    let lines1 = read_lines(&files[0]);
    let lines2 = read_lines(&files[1]);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in compare_lines(&lines1, &lines2, suppress1, suppress2, suppress3) {
        let _ = writeln!(out, "{line}");
    }
}

/// Three-way merge of two pre-sorted slices, formatted as `comm` output.
/// Returns the lines that would be printed (without trailing newlines).
///
/// Output columns:
/// - Column 1: lines unique to `lines1` (no prefix)
/// - Column 2: lines unique to `lines2` (prefix `\t` when column 1 is shown)
/// - Column 3: lines common to both (prefix `\t\t` when columns 1 and 2 are shown)
///
/// The `suppress*` flags drop the corresponding column and shrink the prefix
/// of remaining columns by one tab, matching POSIX.
fn compare_lines(
    lines1: &[String],
    lines2: &[String],
    suppress1: bool,
    suppress2: bool,
    suppress3: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < lines1.len() || j < lines2.len() {
        // Safe indexing: each branch below checks bounds before accessing.
        let l1 = lines1.get(i);
        let l2 = lines2.get(j);

        match (l1, l2) {
            (None, Some(b)) => {
                if !suppress2 {
                    let prefix = if suppress1 { "" } else { "\t" };
                    out.push(format!("{prefix}{b}"));
                }
                j = j.saturating_add(1);
            }
            (Some(a), None) => {
                if !suppress1 {
                    out.push(a.clone());
                }
                i = i.saturating_add(1);
            }
            (Some(a), Some(b)) => {
                if a < b {
                    if !suppress1 {
                        out.push(a.clone());
                    }
                    i = i.saturating_add(1);
                } else if a > b {
                    if !suppress2 {
                        let prefix = if suppress1 { "" } else { "\t" };
                        out.push(format!("{prefix}{b}"));
                    }
                    j = j.saturating_add(1);
                } else {
                    if !suppress3 {
                        let prefix = match (suppress1, suppress2) {
                            (true, true) => "",
                            (true, false) | (false, true) => "\t",
                            (false, false) => "\t\t",
                        };
                        out.push(format!("{prefix}{a}"));
                    }
                    i = i.saturating_add(1);
                    j = j.saturating_add(1);
                }
            }
            (None, None) => break, // Loop condition prevents this, but be explicit.
        }
    }

    out
}

fn read_lines(path: &str) -> Vec<String> {
    let reader: Box<dyn Read> = if path == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("comm: {path}: {e}");
                process::exit(1);
            }
        }
    };

    BufReader::new(reader).lines().map_while(Result::ok).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn no_suppress_three_columns() {
        // file1 = [a, b, d], file2 = [b, c, d]
        // a -> col1, b -> col3, c -> col2, d -> col3
        let out = compare_lines(
            &s(&["a", "b", "d"]),
            &s(&["b", "c", "d"]),
            false,
            false,
            false,
        );
        assert_eq!(out, vec!["a", "\t\tb", "\tc", "\t\td"]);
    }

    #[test]
    fn empty_inputs() {
        let empty: Vec<String> = Vec::new();
        assert!(compare_lines(&empty, &empty, false, false, false).is_empty());
    }

    #[test]
    fn only_first_empty_emits_column_two() {
        let out = compare_lines(&s(&[]), &s(&["a", "b"]), false, false, false);
        assert_eq!(out, vec!["\ta", "\tb"]);
    }

    #[test]
    fn only_second_empty_emits_column_one() {
        let out = compare_lines(&s(&["a", "b"]), &s(&[]), false, false, false);
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn identical_inputs_all_common() {
        let out = compare_lines(&s(&["a", "b"]), &s(&["a", "b"]), false, false, false);
        assert_eq!(out, vec!["\t\ta", "\t\tb"]);
    }

    #[test]
    fn disjoint_inputs_no_common() {
        // file1=[a, c], file2=[b, d] -> a col1, b col2, c col1, d col2
        let out = compare_lines(&s(&["a", "c"]), &s(&["b", "d"]), false, false, false);
        assert_eq!(out, vec!["a", "\tb", "c", "\td"]);
    }

    #[test]
    fn suppress1_drops_unique_to_first() {
        // file1=[a, b], file2=[b, c]
        // Without -1: a, \t\tb, \tc
        // With    -1: drop a, common still printed but shifted to col2 (one tab),
        //             col2 (\tc) shifts to col1 (no tab).
        let out = compare_lines(&s(&["a", "b"]), &s(&["b", "c"]), true, false, false);
        assert_eq!(out, vec!["\tb", "c"]);
    }

    #[test]
    fn suppress2_drops_unique_to_second() {
        let out = compare_lines(&s(&["a", "b"]), &s(&["b", "c"]), false, true, false);
        // a unique col1 kept; b common shifts (one suppressed -> \t); c dropped.
        assert_eq!(out, vec!["a", "\tb"]);
    }

    #[test]
    fn suppress3_drops_common() {
        let out = compare_lines(&s(&["a", "b"]), &s(&["b", "c"]), false, false, true);
        assert_eq!(out, vec!["a", "\tc"]);
    }

    #[test]
    fn suppress12_only_common_shown_without_prefix() {
        let out = compare_lines(&s(&["a", "b"]), &s(&["b", "c"]), true, true, false);
        assert_eq!(out, vec!["b"]);
    }

    #[test]
    fn suppress_all_emits_nothing() {
        let out = compare_lines(&s(&["a", "b"]), &s(&["b", "c"]), true, true, true);
        assert!(out.is_empty());
    }

    #[test]
    fn lexicographic_ordering() {
        // Single-char comparison is fine; comm relies on the inputs being sorted.
        let out = compare_lines(
            &s(&["apple", "banana"]),
            &s(&["banana", "cherry"]),
            false,
            false,
            false,
        );
        assert_eq!(out, vec!["apple", "\t\tbanana", "\tcherry"]);
    }
}
