//! cmp — compare two files byte by byte.
//!
//! Usage: cmp [-l] [-s] FILE1 FILE2
//!   -l  print byte number and differing bytes for each difference
//!   -s  silent: only return exit status
//!   Exit 0 if identical, 1 if different, 2 on error.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut verbose = false;
    let mut silent = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in &args {
        match arg.as_str() {
            "-l" => verbose = true,
            "-s" => silent = true,
            _ => files.push(arg),
        }
    }

    if files.len() != 2 {
        eprintln!("cmp: requires exactly two files");
        process::exit(2);
    }

    let mut f1 = match File::open(files[0]) {
        Ok(f) => f,
        Err(e) => {
            if !silent {
                eprintln!("cmp: {}: {e}", files[0]);
            }
            process::exit(2);
        }
    };

    let mut f2 = match File::open(files[1]) {
        Ok(f) => f,
        Err(e) => {
            if !silent {
                eprintln!("cmp: {}: {e}", files[1]);
            }
            process::exit(2);
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut buf1 = [0u8; 4096];
    let mut buf2 = [0u8; 4096];
    let mut byte_num: u64 = 1;
    let mut line_num: u64 = 1;
    let mut found_diff = false;

    loop {
        let n1 = match f1.read(&mut buf1) {
            Ok(n) => n,
            Err(e) => {
                if !silent {
                    eprintln!("cmp: {}: {e}", files[0]);
                }
                process::exit(2);
            }
        };
        let n2 = match f2.read(&mut buf2) {
            Ok(n) => n,
            Err(e) => {
                if !silent {
                    eprintln!("cmp: {}: {e}", files[1]);
                }
                process::exit(2);
            }
        };

        let slice1 = buf1.get(..n1).unwrap_or(&[]);
        let slice2 = buf2.get(..n2).unwrap_or(&[]);
        let min_n = n1.min(n2);

        if verbose {
            // -l mode: report every difference in the chunk.
            for diff in find_all_diffs(slice1, slice2, byte_num, line_num) {
                found_diff = true;
                let _ = writeln!(out, "{}", format_diff_line(diff.byte_pos, diff.a, diff.b));
            }
        } else {
            // Default / -s mode: stop at first difference.
            match compare_bytes(slice1, slice2, byte_num, line_num) {
                CmpOutcome::Equal => {}
                CmpOutcome::Diff {
                    byte_pos,
                    line_num: ln,
                    ..
                } => {
                    if !silent {
                        println!(
                            "{} {} differ: byte {}, line {}",
                            files[0], files[1], byte_pos, ln
                        );
                    }
                    process::exit(1);
                }
                CmpOutcome::Eof { a_shorter } => {
                    if !silent {
                        let shorter = if a_shorter { files[0] } else { files[1] };
                        eprintln!("cmp: EOF on {shorter}");
                    }
                    process::exit(1);
                }
            }
        }

        // For verbose mode we also need to handle EOF if lengths differ.
        if verbose && n1 != n2 {
            if !silent {
                let shorter = if n1 < n2 { files[0] } else { files[1] };
                eprintln!("cmp: EOF on {shorter}");
            }
            process::exit(1);
        }

        // Advance running counters for the next chunk.
        for &byte in slice1.get(..min_n).unwrap_or(&[]) {
            if byte == b'\n' {
                line_num = line_num.saturating_add(1);
            }
        }
        byte_num = byte_num.saturating_add(min_n as u64);

        if n1 == 0 {
            break;
        }
    }

    process::exit(if found_diff { 1 } else { 0 });
}

/// Outcome of comparing two byte buffers.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum CmpOutcome {
    /// Buffers are identical up to the shorter length, and lengths are equal.
    Equal,
    /// Buffers differ at `byte_pos` (1-based), on line `line_num` (1-based),
    /// with values `a` and `b`. The byte values are reported for callers
    /// (and tests) even though the default-mode caller does not currently
    /// print them; `find_all_diffs` is used for the verbose `-l` path.
    #[allow(dead_code)] // `a` and `b` consumed only in tests / future callers.
    Diff {
        byte_pos: u64,
        line_num: u64,
        a: u8,
        b: u8,
    },
    /// Buffers are identical up to the shorter length, but lengths differ.
    /// `a_shorter` is true if `a` is the shorter buffer.
    Eof { a_shorter: bool },
}

/// Compare two byte buffers and report the first point of difference, or
/// note that one ends before the other.
///
/// Line counting starts at `start_line` (the running line counter from
/// previous chunks) and counts each `\n` byte in `a` up to the diff point.
/// Byte numbering starts at `start_byte` (1-based for POSIX cmp).
fn compare_bytes(a: &[u8], b: &[u8], start_byte: u64, start_line: u64) -> CmpOutcome {
    let min_n = a.len().min(b.len());
    let mut line_num = start_line;

    for i in 0..min_n {
        // Safe: i < min_n <= a.len() and i < min_n <= b.len().
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        if av != bv {
            return CmpOutcome::Diff {
                byte_pos: start_byte.saturating_add(i as u64),
                line_num,
                a: av,
                b: bv,
            };
        }
        if av == b'\n' {
            line_num = line_num.saturating_add(1);
        }
    }

    if a.len() == b.len() {
        CmpOutcome::Equal
    } else {
        CmpOutcome::Eof {
            a_shorter: a.len() < b.len(),
        }
    }
}

/// Format the `-l` verbose-mode line for a single byte difference. POSIX
/// format: byte number in a 6-wide field, then 3-digit octals of each byte.
fn format_diff_line(byte_pos: u64, a: u8, b: u8) -> String {
    format!("{byte_pos:>6} {a:3o} {b:3o}")
}

/// A single byte difference record produced by `find_all_diffs`.
///
/// `line_num` is tracked for callers that want to surface line context, but
/// the current `-l` formatter (`format_diff_line`) only uses byte_pos/a/b.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DiffRecord {
    byte_pos: u64,
    #[allow(dead_code)] // Surfaced to tests; reserved for future formatters.
    line_num: u64,
    a: u8,
    b: u8,
}

/// Return every byte position where `a` and `b` differ, up to the shorter
/// length. Used by `-l` verbose mode. Line counting starts at `start_line`
/// and advances on each `\n` byte in `a` (matching POSIX `cmp -l` behavior).
fn find_all_diffs(a: &[u8], b: &[u8], start_byte: u64, start_line: u64) -> Vec<DiffRecord> {
    let mut out = Vec::new();
    let min_n = a.len().min(b.len());
    let mut line_num = start_line;
    for i in 0..min_n {
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        if av != bv {
            out.push(DiffRecord {
                byte_pos: start_byte.saturating_add(i as u64),
                line_num,
                a: av,
                b: bv,
            });
        }
        if av == b'\n' {
            line_num = line_num.saturating_add(1);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- compare_bytes ----------------

    #[test]
    fn equal_buffers_return_equal() {
        assert_eq!(compare_bytes(b"hello", b"hello", 1, 1), CmpOutcome::Equal);
    }

    #[test]
    fn empty_buffers_are_equal() {
        assert_eq!(compare_bytes(b"", b"", 1, 1), CmpOutcome::Equal);
    }

    #[test]
    fn first_byte_differs() {
        assert_eq!(
            compare_bytes(b"abc", b"xbc", 1, 1),
            CmpOutcome::Diff {
                byte_pos: 1,
                line_num: 1,
                a: b'a',
                b: b'x',
            }
        );
    }

    #[test]
    fn middle_byte_differs() {
        assert_eq!(
            compare_bytes(b"abc", b"aXc", 1, 1),
            CmpOutcome::Diff {
                byte_pos: 2,
                line_num: 1,
                a: b'b',
                b: b'X',
            }
        );
    }

    #[test]
    fn line_number_counts_preceding_newlines() {
        // Two newlines before the diff -> line 3 when we hit it.
        assert_eq!(
            compare_bytes(b"a\nb\nXc", b"a\nb\nYc", 1, 1),
            CmpOutcome::Diff {
                byte_pos: 5,
                line_num: 3,
                a: b'X',
                b: b'Y',
            }
        );
    }

    #[test]
    fn diff_at_newline_itself() {
        // The newline byte itself differs; line count for THAT diff is the
        // current line, not the next.
        assert_eq!(
            compare_bytes(b"a\nb", b"a b", 1, 1),
            CmpOutcome::Diff {
                byte_pos: 2,
                line_num: 1,
                a: b'\n',
                b: b' ',
            }
        );
    }

    #[test]
    fn shorter_a_returns_eof_a_shorter_true() {
        assert_eq!(
            compare_bytes(b"abc", b"abcd", 1, 1),
            CmpOutcome::Eof { a_shorter: true }
        );
    }

    #[test]
    fn shorter_b_returns_eof_a_shorter_false() {
        assert_eq!(
            compare_bytes(b"abcd", b"abc", 1, 1),
            CmpOutcome::Eof { a_shorter: false }
        );
    }

    #[test]
    fn start_byte_offset_applied() {
        // Reading the second chunk: start_byte=5 means the diff at index 0
        // of the new chunks reports byte_pos=5.
        assert_eq!(
            compare_bytes(b"X", b"Y", 5, 2),
            CmpOutcome::Diff {
                byte_pos: 5,
                line_num: 2,
                a: b'X',
                b: b'Y',
            }
        );
    }

    // ---------------- format_diff_line ----------------

    #[test]
    fn format_diff_basic() {
        // 'a' = 0o141, 'A' = 0o101
        assert_eq!(format_diff_line(1, b'a', b'A'), "     1 141 101");
    }

    #[test]
    fn format_diff_zero_bytes() {
        // 0 -> "  0"
        assert_eq!(format_diff_line(42, 0, 0), "    42   0   0");
    }

    #[test]
    fn format_diff_high_byte() {
        // 0xff = 0o377
        assert_eq!(format_diff_line(7, 0xff, 0), "     7 377   0");
    }

    #[test]
    fn format_diff_wide_byte_pos() {
        // Larger than 6 digits still works — width is min, not max.
        assert_eq!(format_diff_line(1234567, 1, 2), "1234567   1   2");
    }

    // ---------------- find_all_diffs ----------------

    #[test]
    fn find_all_diffs_equal_buffers_empty() {
        assert!(find_all_diffs(b"abc", b"abc", 1, 1).is_empty());
    }

    #[test]
    fn find_all_diffs_every_byte_differs() {
        let diffs = find_all_diffs(b"abc", b"xyz", 1, 1);
        assert_eq!(diffs.len(), 3);
        assert_eq!(diffs[0].byte_pos, 1);
        assert_eq!(diffs[1].byte_pos, 2);
        assert_eq!(diffs[2].byte_pos, 3);
    }

    #[test]
    fn find_all_diffs_some_bytes_differ() {
        let diffs = find_all_diffs(b"abcd", b"aXcY", 1, 1);
        assert_eq!(diffs.len(), 2);
        assert_eq!(
            diffs[0],
            DiffRecord {
                byte_pos: 2,
                line_num: 1,
                a: b'b',
                b: b'X',
            }
        );
        assert_eq!(
            diffs[1],
            DiffRecord {
                byte_pos: 4,
                line_num: 1,
                a: b'd',
                b: b'Y',
            }
        );
    }

    #[test]
    fn find_all_diffs_stops_at_shorter_length() {
        // Length comparison itself is not reported by this helper.
        let diffs = find_all_diffs(b"ab", b"abcd", 1, 1);
        assert!(diffs.is_empty());
    }

    #[test]
    fn find_all_diffs_line_numbers_track_newlines() {
        // 'X' on line 1, 'Y' on line 2 (after the '\n').
        let diffs = find_all_diffs(b"aX\nbY", b"a \nb ", 1, 1);
        assert_eq!(diffs[0].line_num, 1);
        assert_eq!(diffs[0].a, b'X');
        assert_eq!(diffs[1].line_num, 2);
        assert_eq!(diffs[1].a, b'Y');
    }
}
