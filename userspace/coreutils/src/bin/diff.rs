//! diff — compare files line by line.
//!
//! Usage: diff [-u] [-q] FILE1 FILE2
//!   -u  unified output format (3 lines of context)
//!   -q  only report if files differ (no detail)
//!
//! Uses a simple longest-common-subsequence algorithm.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DiffArgs {
    unified: bool,
    brief: bool,
    files: Vec<String>,
}

/// Parse diff's argv into a `DiffArgs`. Unknown flags are treated as files,
/// matching the existing implementation.
fn parse_args(args: &[String]) -> DiffArgs {
    let mut out = DiffArgs::default();
    for arg in args {
        match arg.as_str() {
            "-u" => out.unified = true,
            "-q" | "--brief" => out.brief = true,
            _ => out.files.push(arg.clone()),
        }
    }
    out
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = parse_args(&args);

    if parsed.files.len() != 2 {
        eprintln!("diff: requires exactly two files");
        process::exit(2);
    }
    let unified = parsed.unified;
    let brief = parsed.brief;
    let files: Vec<&str> = parsed.files.iter().map(String::as_str).collect();

    let content1 = match fs::read_to_string(files[0]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("diff: {}: {e}", files[0]);
            process::exit(2);
        }
    };

    let content2 = match fs::read_to_string(files[1]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("diff: {}: {e}", files[1]);
            process::exit(2);
        }
    };

    let lines1: Vec<&str> = content1.lines().collect();
    let lines2: Vec<&str> = content2.lines().collect();

    if lines1 == lines2 {
        process::exit(0);
    }

    if brief {
        println!("Files {} and {} differ", files[0], files[1]);
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if unified {
        let _ = writeln!(out, "--- {}", files[0]);
        let _ = writeln!(out, "+++ {}", files[1]);
        output_unified(&mut out, &lines1, &lines2, 3);
    } else {
        output_normal(&mut out, &lines1, &lines2);
    }

    process::exit(1);
}

/// Simple LCS-based diff — computes edit script.
fn lcs_table(a: &[&str], b: &[&str]) -> Vec<Vec<u32>> {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0u32; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    dp
}

#[derive(Debug)]
enum Edit {
    Keep(usize, ()), // line from a[i], b[j]
    Delete(usize),       // line from a[i]
    Insert(usize),       // line from b[j]
}

fn compute_edits<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Edit> {
    let dp = lcs_table(a, b);
    let mut edits = Vec::new();
    let mut i = a.len();
    let mut j = b.len();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            edits.push(Edit::Keep(i - 1, ()));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            edits.push(Edit::Insert(j - 1));
            j -= 1;
        } else {
            edits.push(Edit::Delete(i - 1));
            i -= 1;
        }
    }

    edits.reverse();
    edits
}

fn output_normal(out: &mut impl Write, a: &[&str], b: &[&str]) {
    let edits = compute_edits(a, b);

    let mut i = 0;
    while i < edits.len() {
        match &edits[i] {
            Edit::Keep(_, _) => {
                i += 1;
            }
            Edit::Delete(line_idx) => {
                // Collect consecutive deletes
                let start = *line_idx;
                let mut end = start;
                let mut j = i + 1;
                while j < edits.len() {
                    if let Edit::Delete(next) = &edits[j] {
                        end = *next;
                        j += 1;
                    } else {
                        break;
                    }
                }

                // Check if followed by inserts (change) or standalone (delete)
                let mut ins_start = None;
                let mut ins_end = 0;
                let mut k = j;
                while k < edits.len() {
                    if let Edit::Insert(idx) = &edits[k] {
                        if ins_start.is_none() {
                            ins_start = Some(*idx);
                        }
                        ins_end = *idx;
                        k += 1;
                    } else {
                        break;
                    }
                }

                if let Some(is) = ins_start {
                    // Change
                    let _ = writeln!(out, "{}c{}", range_str(start + 1, end + 1), range_str(is + 1, ins_end + 1));
                    for line in a.iter().take(end + 1).skip(start) {
                        let _ = writeln!(out, "< {line}");
                    }
                    let _ = writeln!(out, "---");
                    for line in b.iter().take(ins_end + 1).skip(is) {
                        let _ = writeln!(out, "> {line}");
                    }
                    i = k;
                } else {
                    // Pure delete
                    let _ = writeln!(out, "{}d{}", range_str(start + 1, end + 1), start);
                    for line in a.iter().take(end + 1).skip(start) {
                        let _ = writeln!(out, "< {line}");
                    }
                    i = j;
                }
            }
            Edit::Insert(line_idx) => {
                let start = *line_idx;
                let mut end = start;
                let mut j = i + 1;
                while j < edits.len() {
                    if let Edit::Insert(next) = &edits[j] {
                        end = *next;
                        j += 1;
                    } else {
                        break;
                    }
                }

                let _ = writeln!(out, "{}a{}", start, range_str(start + 1, end + 1));
                for line in b.iter().take(end + 1).skip(start) {
                    let _ = writeln!(out, "> {line}");
                }
                i = j;
            }
        }
    }
}

fn output_unified(out: &mut impl Write, a: &[&str], b: &[&str], _context: usize) {
    let edits = compute_edits(a, b);

    // Find hunks (groups of changes with context lines)
    let changes: Vec<(usize, &Edit)> = edits
        .iter()
        .enumerate()
        .filter(|(_, e)| !matches!(e, Edit::Keep(_, _)))
        .collect();

    if changes.is_empty() {
        return;
    }

    // Output all changes as a single hunk for simplicity
    let _ = writeln!(out, "@@ -{},{} +{},{} @@", 1, a.len(), 1, b.len());
    for edit in &edits {
        match edit {
            Edit::Keep(ai, _) => {
                let _ = writeln!(out, " {}", a[*ai]);
            }
            Edit::Delete(ai) => {
                let _ = writeln!(out, "-{}", a[*ai]);
            }
            Edit::Insert(bi) => {
                let _ = writeln!(out, "+{}", b[*bi]);
            }
        }
    }
}

fn range_str(start: usize, end: usize) -> String {
    if start == end {
        format!("{start}")
    } else {
        format!("{start},{end}")
    }
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
        let a = parse_args(&s(&[]));
        assert!(!a.unified && !a.brief);
        assert!(a.files.is_empty());
    }

    #[test]
    fn parse_unified() {
        let a = parse_args(&s(&["-u", "a.txt", "b.txt"]));
        assert!(a.unified);
        assert_eq!(a.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn parse_brief_short() {
        let a = parse_args(&s(&["-q", "a", "b"]));
        assert!(a.brief);
    }

    #[test]
    fn parse_brief_long() {
        let a = parse_args(&s(&["--brief", "a", "b"]));
        assert!(a.brief);
    }

    #[test]
    fn parse_combined() {
        let a = parse_args(&s(&["-u", "-q", "a", "b"]));
        assert!(a.unified && a.brief);
        assert_eq!(a.files, vec!["a", "b"]);
    }

    #[test]
    fn parse_files_only() {
        let a = parse_args(&s(&["a", "b"]));
        assert_eq!(a.files, vec!["a", "b"]);
    }

    // ---------------- range_str ----------------

    #[test]
    fn range_single() {
        assert_eq!(range_str(5, 5), "5");
    }

    #[test]
    fn range_inclusive() {
        assert_eq!(range_str(2, 7), "2,7");
    }

    // ---------------- lcs_table ----------------

    #[test]
    fn lcs_table_empty() {
        let dp = lcs_table(&[], &[]);
        assert_eq!(dp, vec![vec![0u32]]);
    }

    #[test]
    fn lcs_table_identical() {
        let a = vec!["x", "y", "z"];
        let b = vec!["x", "y", "z"];
        let dp = lcs_table(&a, &b);
        assert_eq!(dp[3][3], 3);
    }

    #[test]
    fn lcs_table_disjoint() {
        let a = vec!["a", "b", "c"];
        let b = vec!["x", "y", "z"];
        let dp = lcs_table(&a, &b);
        assert_eq!(dp[3][3], 0);
    }

    #[test]
    fn lcs_table_partial() {
        // a = [a, b, c, d], b = [a, c, d, e]; LCS = [a, c, d] -> length 3.
        let a = vec!["a", "b", "c", "d"];
        let b = vec!["a", "c", "d", "e"];
        let dp = lcs_table(&a, &b);
        assert_eq!(dp[4][4], 3);
    }

    // ---------------- compute_edits ----------------

    #[test]
    fn edits_identical_all_keeps() {
        let a = vec!["x", "y"];
        let b = vec!["x", "y"];
        let edits = compute_edits(&a, &b);
        assert_eq!(edits.len(), 2);
        assert!(matches!(edits.first(), Some(Edit::Keep(_, _))));
        assert!(matches!(edits.get(1), Some(Edit::Keep(_, _))));
    }

    #[test]
    fn edits_pure_insert() {
        let a: Vec<&str> = vec![];
        let b = vec!["x"];
        let edits = compute_edits(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits.first(), Some(Edit::Insert(0))));
    }

    #[test]
    fn edits_pure_delete() {
        let a = vec!["x"];
        let b: Vec<&str> = vec![];
        let edits = compute_edits(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits.first(), Some(Edit::Delete(0))));
    }

    #[test]
    fn edits_replacement() {
        let a = vec!["old"];
        let b = vec!["new"];
        let edits = compute_edits(&a, &b);
        // No common line, so we expect one Insert and one Delete (order may vary).
        assert_eq!(edits.len(), 2);
        let has_delete = edits.iter().any(|e| matches!(e, Edit::Delete(_)));
        let has_insert = edits.iter().any(|e| matches!(e, Edit::Insert(_)));
        assert!(has_delete && has_insert);
    }

    #[test]
    fn edits_partial_keeps_middle() {
        // a = [keep1, del, keep2], b = [keep1, ins, keep2].
        let a = vec!["keep1", "del", "keep2"];
        let b = vec!["keep1", "ins", "keep2"];
        let edits = compute_edits(&a, &b);
        // First and last should be Keep.
        assert!(matches!(edits.first(), Some(Edit::Keep(0, _))));
        assert!(matches!(edits.last(), Some(Edit::Keep(2, _))));
    }
}
