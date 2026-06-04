//! patch — apply a diff file to originals.
//!
//! Usage: patch [-pNUM] [-i PATCHFILE] [-R] [--dry-run] [-s] [-b] [ORIGFILE]
//!   -pNUM       strip NUM leading path components from file names
//!   -p NUM      same as -pNUM, with space
//!   -i FILE     read patch from FILE instead of stdin
//!   -R          reverse: swap old and new files in the patch
//!   --dry-run   print what would be done without modifying files
//!   -s          silent mode (suppress informational output)
//!   -b          create a .orig backup before modifying
//!   ORIGFILE    apply all hunks to this file (overrides filename in patch)
//!
//! Supports unified diff format (output of diff -u / git diff).
//! Handles multiple files in a single patch.
//!
//! Exit codes:
//!   0  all hunks applied successfully
//!   1  some hunks failed
//!   2  error (cannot read patch, etc.)

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process;

/// A single hunk from a unified diff.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Hunk {
    old_start: usize,  // 1-based line number in original file
    old_count: usize,
    new_start: usize,  // 1-based line number in new file
    new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

/// A patch for a single file, consisting of one or more hunks.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct FilePatch {
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    strip: Option<usize>,
    patch_file: Option<String>,
    reverse: bool,
    dry_run: bool,
    silent: bool,
    backup: bool,
    target_file: Option<String>,
}

/// Parse patch's argv into an `Options`.  Recognised flags:
///   -i FILE / -p NUM / -pNUM / -R / --reverse / --dry-run / -s /
///   --silent / --quiet / -b / --backup.
/// Anything else not starting with `-` (and not the bare string "-")
/// is the target file.  Unknown flags return an error.
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut i: usize = 0;

    while let Some(arg) = args.get(i) {
        let a = arg.as_str();
        if a == "-i" {
            i = i.saturating_add(1);
            let v = args
                .get(i)
                .ok_or_else(|| "option -i requires an argument".to_string())?;
            opts.patch_file = Some(v.clone());
        } else if a == "-p" {
            i = i.saturating_add(1);
            let v = args
                .get(i)
                .ok_or_else(|| "option -p requires an argument".to_string())?;
            let n: usize = v
                .parse()
                .map_err(|_| format!("invalid strip count: {v}"))?;
            opts.strip = Some(n);
        } else if let Some(rest) = a.strip_prefix("-p") {
            if !rest.is_empty() {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("invalid strip count: {rest}"))?;
                opts.strip = Some(n);
            }
        } else if a == "-R" || a == "--reverse" {
            opts.reverse = true;
        } else if a == "--dry-run" {
            opts.dry_run = true;
        } else if a == "-s" || a == "--silent" || a == "--quiet" {
            opts.silent = true;
        } else if a == "-b" || a == "--backup" {
            opts.backup = true;
        } else if a.starts_with('-') && a.len() > 1 && a != "-" {
            return Err(format!("unknown option: {a}"));
        } else {
            opts.target_file = Some(arg.clone());
        }
        i = i.saturating_add(1);
    }

    Ok(opts)
}

/// Strip NUM leading path components from a file path.
fn strip_path(path: &str, num: usize) -> String {
    if num == 0 {
        return path.to_string();
    }
    let parts: Vec<&str> = path.splitn(num.saturating_add(1), '/').collect();
    if let Some(tail) = parts.get(num) {
        (*tail).to_string()
    } else {
        // If there aren't enough components, return the basename.
        path.rsplit('/').next().unwrap_or(path).to_string()
    }
}

/// Parse the @@ -old_start,old_count +new_start,new_count @@ line.
fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize)> {
    // Format: @@ -A,B +C,D @@ optional text
    let line = line.trim();
    let after_at = line.strip_prefix("@@")?;
    let end_at = after_at.find("@@")?;
    let range_part = after_at.get(..end_at)?.trim();

    let mut parts = range_part.split_whitespace();
    let old_range = parts.next()?;
    let new_range = parts.next()?;

    let old_range = old_range.strip_prefix('-')?;
    let new_range = new_range.strip_prefix('+')?;

    let (old_start, old_count) = parse_range(old_range)?;
    let (new_start, new_count) = parse_range(new_range)?;

    Some((old_start, old_count, new_start, new_count))
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        Some((start_s.parse().ok()?, count_s.parse().ok()?))
    } else {
        // Single number means count=1.
        Some((s.parse().ok()?, 1))
    }
}

/// Parse unified diff input into a list of file patches.
fn parse_patch(input: &str) -> Vec<FilePatch> {
    let lines: Vec<&str> = input.lines().collect();
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut i = 0;

    while let Some(line_i) = lines.get(i).copied() {
        // Look for --- line followed by +++ line.
        let next_starts_with_plus = lines
            .get(i.saturating_add(1))
            .is_some_and(|l| l.starts_with("+++ "));
        if line_i.starts_with("--- ") && next_starts_with_plus {
            let old_path = parse_file_path(line_i, "--- ");
            let new_path =
                parse_file_path(lines.get(i.saturating_add(1)).copied().unwrap_or(""), "+++ ");
            i = i.saturating_add(2);

            let mut hunks: Vec<Hunk> = Vec::new();

            // Parse hunks for this file.
            while let Some(cur) = lines.get(i).copied() {
                if cur.starts_with("@@ ") {
                    if let Some((os, oc, ns, nc)) = parse_hunk_header(cur) {
                        i = i.saturating_add(1);
                        let mut hunk_lines: Vec<HunkLine> = Vec::new();

                        while let Some(line) = lines.get(i).copied() {
                            if line.starts_with("@@ ")
                                || line.starts_with("--- ")
                                || line.starts_with("diff ")
                            {
                                break;
                            }

                            if let Some(rest) = line.strip_prefix('+') {
                                hunk_lines.push(HunkLine::Add(rest.to_string()));
                            } else if let Some(rest) = line.strip_prefix('-') {
                                hunk_lines.push(HunkLine::Remove(rest.to_string()));
                            } else if let Some(rest) = line.strip_prefix(' ') {
                                hunk_lines.push(HunkLine::Context(rest.to_string()));
                            } else if line == "\\ No newline at end of file" {
                                // Informational line from diff, skip.
                            } else {
                                // Treat lines without prefix as context
                                // (some patches have bare context lines).
                                hunk_lines.push(HunkLine::Context(line.to_string()));
                            }
                            i = i.saturating_add(1);
                        }

                        hunks.push(Hunk {
                            old_start: os,
                            old_count: oc,
                            new_start: ns,
                            new_count: nc,
                            lines: hunk_lines,
                        });
                    } else {
                        i = i.saturating_add(1);
                    }
                } else if cur.starts_with("--- ") || cur.starts_with("diff ") {
                    // Next file patch starts here.
                    break;
                } else {
                    i = i.saturating_add(1);
                }
            }

            patches.push(FilePatch {
                old_path,
                new_path,
                hunks,
            });
        } else {
            i = i.saturating_add(1);
        }
    }

    patches
}

fn parse_file_path(line: &str, prefix: &str) -> String {
    let rest = line.strip_prefix(prefix).unwrap_or(line);
    // Remove timestamp suffix if present (e.g., "file.c\t2024-01-01 ...")
    match rest.find('\t') {
        Some(tab_pos) => rest.get(..tab_pos).unwrap_or(rest).to_string(),
        None => rest.to_string(),
    }
}

/// Apply a single hunk to the file lines. Returns the new lines if successful,
/// or None if the hunk doesn't match the expected context.
/// `offset` is the cumulative line offset from previous hunks.
fn apply_hunk(lines: &[String], hunk: &Hunk, offset: i64) -> Option<(Vec<String>, i64)> {
    let target_start_signed = i64::try_from(hunk.old_start)
        .unwrap_or(i64::MAX)
        .saturating_add(offset)
        .saturating_sub(1)
        .max(0);
    let target_start = usize::try_from(target_start_signed).unwrap_or(0);

    // Try exact position first, then search outward (fuzz).
    let max_fuzz: usize = 50;
    let mut best_pos: Option<usize> = None;

    'outer: for fuzz in 0..=max_fuzz {
        if fuzz == 0 {
            if try_hunk_at(lines, hunk, target_start) {
                best_pos = Some(target_start);
                break;
            }
            continue;
        }
        // Try -fuzz, then +fuzz.
        if let Some(pos) = target_start.checked_sub(fuzz)
            && try_hunk_at(lines, hunk, pos)
        {
            best_pos = Some(pos);
            break 'outer;
        }
        let pos = target_start.saturating_add(fuzz);
        if try_hunk_at(lines, hunk, pos) {
            best_pos = Some(pos);
            break;
        }
    }

    let pos = best_pos?;

    // Build the new file content.
    let mut result = Vec::new();
    if let Some(head) = lines.get(..pos) {
        result.extend_from_slice(head);
    }

    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(s) | HunkLine::Add(s) => result.push(s.clone()),
            HunkLine::Remove(_) => {} // skip removed lines
        }
    }

    // Count how many old lines the hunk consumed.
    let old_consumed = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_) | HunkLine::Remove(_)))
        .count();

    let tail_start = pos.saturating_add(old_consumed);
    if let Some(tail) = lines.get(tail_start..) {
        result.extend_from_slice(tail);
    }

    // The offset adjustment is new_count - old_count.
    let new_offset = offset
        .saturating_add(i64::try_from(hunk.new_count).unwrap_or(i64::MAX))
        .saturating_sub(i64::try_from(hunk.old_count).unwrap_or(i64::MAX));

    Some((result, new_offset))
}

/// Check if a hunk's context/remove lines match at the given position.
fn try_hunk_at(lines: &[String], hunk: &Hunk, pos: usize) -> bool {
    let mut line_idx = pos;
    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(expected) | HunkLine::Remove(expected) => {
                let Some(actual) = lines.get(line_idx) else {
                    return false;
                };
                if actual != expected {
                    return false;
                }
                line_idx = line_idx.saturating_add(1);
            }
            HunkLine::Add(_) => {
                // Added lines don't consume original lines.
            }
        }
    }
    true
}

/// Reverse a hunk: swap add and remove.
fn reverse_hunk(hunk: &Hunk) -> Hunk {
    let reversed_lines = hunk
        .lines
        .iter()
        .map(|l| match l {
            HunkLine::Context(s) => HunkLine::Context(s.clone()),
            HunkLine::Add(s) => HunkLine::Remove(s.clone()),
            HunkLine::Remove(s) => HunkLine::Add(s.clone()),
        })
        .collect();

    Hunk {
        old_start: hunk.new_start,
        old_count: hunk.new_count,
        new_start: hunk.old_start,
        new_count: hunk.old_count,
        lines: reversed_lines,
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("patch: {e}");
            process::exit(2);
        }
    };

    // Read patch input.
    let patch_input = if let Some(ref path) = opts.patch_file {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("patch: {path}: {e}");
                process::exit(2);
            }
        }
    } else {
        let mut buf = String::new();
        if io::stdin().read_to_string(&mut buf).is_err() {
            eprintln!("patch: error reading stdin");
            process::exit(2);
        }
        buf
    };

    let file_patches = parse_patch(&patch_input);

    if file_patches.is_empty() {
        eprintln!("patch: no valid patches found in input");
        process::exit(2);
    }

    let mut any_failed = false;

    for fp in &file_patches {
        // Determine the target file path.
        let raw_path = if let Some(ref target) = opts.target_file {
            target.clone()
        } else if opts.reverse {
            fp.new_path.clone()
        } else {
            // Prefer new_path if old_path is /dev/null (new file).
            if fp.old_path == "/dev/null" {
                fp.new_path.clone()
            } else {
                fp.old_path.clone()
            }
        };

        let file_path = match opts.strip {
            Some(n) => strip_path(&raw_path, n),
            None => raw_path.clone(),
        };

        if !opts.silent {
            if opts.dry_run {
                eprintln!("checking file {file_path}...");
            } else {
                eprintln!("patching file {file_path}");
            }
        }

        // Read the original file (or start empty for new files).
        let original = if fp.old_path == "/dev/null" && !opts.reverse {
            String::new()
        } else {
            match fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    if fp.old_path == "/dev/null" {
                        String::new()
                    } else {
                        eprintln!("patch: can't open file {file_path}: {e}");
                        any_failed = true;
                        continue;
                    }
                }
            }
        };

        let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();
        let mut offset: i64 = 0;
        let mut hunks_applied = 0;
        let mut hunks_failed = 0;

        let hunks: Vec<Hunk> = if opts.reverse {
            fp.hunks.iter().map(reverse_hunk).collect()
        } else {
            fp.hunks.clone()
        };

        for (hunk_idx, hunk) in hunks.iter().enumerate() {
            match apply_hunk(&lines, hunk, offset) {
                Some((new_lines, new_offset)) => {
                    lines = new_lines;
                    offset = new_offset;
                    hunks_applied += 1;
                }
                None => {
                    hunks_failed += 1;
                    if !opts.silent {
                        eprintln!(
                            "patch: Hunk #{} FAILED at line {}",
                            hunk_idx + 1,
                            hunk.old_start
                        );
                    }
                }
            }
        }

        if hunks_failed > 0 {
            any_failed = true;
            if !opts.silent {
                eprintln!(
                    "patch: {hunks_failed} out of {} hunks FAILED for {file_path}",
                    hunks_applied + hunks_failed
                );
            }
        }

        if !opts.dry_run && hunks_applied > 0 {
            // Create backup if requested.
            if opts.backup && Path::new(&file_path).exists() {
                let backup_path = format!("{file_path}.orig");
                if let Err(e) = fs::copy(&file_path, &backup_path) {
                    eprintln!("patch: cannot create backup {backup_path}: {e}");
                }
            }

            // Create parent directories if needed (for new files).
            if let Some(parent) = Path::new(&file_path).parent()
                && !parent.as_os_str().is_empty() {
                    let _ = fs::create_dir_all(parent);
                }

            // Write the patched file.
            let mut output = lines.join("\n");
            // Preserve trailing newline if the original had one.
            if original.ends_with('\n') || fp.old_path == "/dev/null" {
                output.push('\n');
            }

            if let Err(e) = fs::write(&file_path, &output) {
                eprintln!("patch: cannot write {file_path}: {e}");
                any_failed = true;
            }
        }
    }

    if any_failed {
        process::exit(1);
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
    fn parse_empty() {
        let o = parse_args(&s(&[])).unwrap();
        assert_eq!(o, Options::default());
    }

    #[test]
    fn parse_input_file() {
        let o = parse_args(&s(&["-i", "x.patch"])).unwrap();
        assert_eq!(o.patch_file.as_deref(), Some("x.patch"));
    }

    #[test]
    fn parse_strip_count_separate_arg() {
        let o = parse_args(&s(&["-p", "1"])).unwrap();
        assert_eq!(o.strip, Some(1));
    }

    #[test]
    fn parse_strip_count_joined() {
        let o = parse_args(&s(&["-p1"])).unwrap();
        assert_eq!(o.strip, Some(1));
    }

    #[test]
    fn parse_reverse_flag() {
        let o = parse_args(&s(&["-R"])).unwrap();
        assert!(o.reverse);
        let o = parse_args(&s(&["--reverse"])).unwrap();
        assert!(o.reverse);
    }

    #[test]
    fn parse_dry_run_flag() {
        let o = parse_args(&s(&["--dry-run"])).unwrap();
        assert!(o.dry_run);
    }

    #[test]
    fn parse_silent_alias() {
        for alias in ["-s", "--silent", "--quiet"] {
            let o = parse_args(&s(&[alias])).unwrap();
            assert!(o.silent, "expected silent for {alias}");
        }
    }

    #[test]
    fn parse_backup_alias() {
        for alias in ["-b", "--backup"] {
            let o = parse_args(&s(&[alias])).unwrap();
            assert!(o.backup, "expected backup for {alias}");
        }
    }

    #[test]
    fn parse_target_file() {
        let o = parse_args(&s(&["foo.txt"])).unwrap();
        assert_eq!(o.target_file.as_deref(), Some("foo.txt"));
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse_args(&s(&["-Z"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_missing_p_value_errors() {
        let err = parse_args(&s(&["-p"])).unwrap_err();
        assert!(err.contains("-p requires"));
    }

    #[test]
    fn parse_invalid_p_value_errors() {
        let err = parse_args(&s(&["-p", "abc"])).unwrap_err();
        assert!(err.contains("invalid strip count"));
    }

    #[test]
    fn parse_invalid_pn_value_errors() {
        let err = parse_args(&s(&["-pabc"])).unwrap_err();
        assert!(err.contains("invalid strip count"));
    }

    #[test]
    fn parse_bare_dash_is_target() {
        let o = parse_args(&s(&["-"])).unwrap();
        assert_eq!(o.target_file.as_deref(), Some("-"));
    }

    // ---------------- strip_path ----------------

    #[test]
    fn strip_zero_keeps_path() {
        assert_eq!(strip_path("a/b/c", 0), "a/b/c");
    }

    #[test]
    fn strip_one() {
        assert_eq!(strip_path("a/b/c", 1), "b/c");
    }

    #[test]
    fn strip_two() {
        assert_eq!(strip_path("a/b/c", 2), "c");
    }

    #[test]
    fn strip_too_many_falls_back_to_basename() {
        assert_eq!(strip_path("a/b/c", 5), "c");
    }

    #[test]
    fn strip_no_slashes_basename() {
        assert_eq!(strip_path("file.c", 1), "file.c");
    }

    // ---------------- parse_range ----------------

    #[test]
    fn parse_range_with_count() {
        assert_eq!(parse_range("10,5"), Some((10, 5)));
    }

    #[test]
    fn parse_range_single_number_implies_one() {
        assert_eq!(parse_range("7"), Some((7, 1)));
    }

    #[test]
    fn parse_range_garbage_is_none() {
        assert!(parse_range("x").is_none());
        assert!(parse_range("1,x").is_none());
    }

    // ---------------- parse_hunk_header ----------------

    #[test]
    fn parse_hunk_header_basic() {
        let h = parse_hunk_header("@@ -1,3 +1,4 @@").unwrap();
        assert_eq!(h, (1, 3, 1, 4));
    }

    #[test]
    fn parse_hunk_header_with_trailing_context() {
        let h = parse_hunk_header("@@ -10,5 +20,7 @@ fn foo()").unwrap();
        assert_eq!(h, (10, 5, 20, 7));
    }

    #[test]
    fn parse_hunk_header_single_line_count_one() {
        let h = parse_hunk_header("@@ -5 +5 @@").unwrap();
        assert_eq!(h, (5, 1, 5, 1));
    }

    #[test]
    fn parse_hunk_header_no_at_markers_is_none() {
        assert!(parse_hunk_header("nope").is_none());
        assert!(parse_hunk_header("@@ no end").is_none());
    }

    // ---------------- parse_file_path ----------------

    #[test]
    fn parse_file_path_plain() {
        assert_eq!(parse_file_path("--- foo.c", "--- "), "foo.c");
    }

    #[test]
    fn parse_file_path_strips_timestamp() {
        assert_eq!(
            parse_file_path("+++ bar.c\t2024-01-01 12:00", "+++ "),
            "bar.c"
        );
    }

    // ---------------- parse_patch ----------------

    const SIMPLE_PATCH: &str = "\
--- old.txt
+++ new.txt
@@ -1,3 +1,3 @@
 line1
-line2
+line2 modified
 line3
";

    #[test]
    fn parse_patch_simple() {
        let ps = parse_patch(SIMPLE_PATCH);
        assert_eq!(ps.len(), 1);
        let fp = &ps[0];
        assert_eq!(fp.old_path, "old.txt");
        assert_eq!(fp.new_path, "new.txt");
        assert_eq!(fp.hunks.len(), 1);
        let h = &fp.hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
        assert_eq!(
            h.lines,
            vec![
                HunkLine::Context("line1".to_string()),
                HunkLine::Remove("line2".to_string()),
                HunkLine::Add("line2 modified".to_string()),
                HunkLine::Context("line3".to_string()),
            ]
        );
    }

    #[test]
    fn parse_patch_no_diff_returns_empty() {
        assert!(parse_patch("no diff here").is_empty());
        assert!(parse_patch("").is_empty());
    }

    #[test]
    fn parse_patch_two_files() {
        let input = "\
--- a.c
+++ a.c
@@ -1,1 +1,1 @@
-old
+new
--- b.c
+++ b.c
@@ -2,1 +2,1 @@
-x
+y
";
        let ps = parse_patch(input);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].old_path, "a.c");
        assert_eq!(ps[1].old_path, "b.c");
    }

    // ---------------- reverse_hunk ----------------

    #[test]
    fn reverse_hunk_swaps_add_remove_and_ranges() {
        let h = Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 3,
            new_count: 4,
            lines: vec![
                HunkLine::Context("ctx".into()),
                HunkLine::Remove("old".into()),
                HunkLine::Add("new".into()),
            ],
        };
        let r = reverse_hunk(&h);
        assert_eq!(r.old_start, 3);
        assert_eq!(r.old_count, 4);
        assert_eq!(r.new_start, 1);
        assert_eq!(r.new_count, 2);
        assert_eq!(
            r.lines,
            vec![
                HunkLine::Context("ctx".into()),
                HunkLine::Add("old".into()),
                HunkLine::Remove("new".into()),
            ]
        );
    }

    // ---------------- try_hunk_at / apply_hunk ----------------

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    fn modify_hunk() -> Hunk {
        // Corresponds to SIMPLE_PATCH's only hunk.
        Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("line1".into()),
                HunkLine::Remove("line2".into()),
                HunkLine::Add("line2 modified".into()),
                HunkLine::Context("line3".into()),
            ],
        }
    }

    #[test]
    fn try_hunk_at_matches_correct_position() {
        let l = lines(&["line1", "line2", "line3"]);
        assert!(try_hunk_at(&l, &modify_hunk(), 0));
    }

    #[test]
    fn try_hunk_at_fails_on_mismatch() {
        let l = lines(&["lineA", "lineB", "lineC"]);
        assert!(!try_hunk_at(&l, &modify_hunk(), 0));
    }

    #[test]
    fn try_hunk_at_fails_past_end() {
        let l = lines(&["line1"]);
        assert!(!try_hunk_at(&l, &modify_hunk(), 0));
    }

    #[test]
    fn apply_hunk_modifies_buffer() {
        let l = lines(&["line1", "line2", "line3"]);
        let (new_lines, new_offset) = apply_hunk(&l, &modify_hunk(), 0).unwrap();
        assert_eq!(new_lines, vec!["line1", "line2 modified", "line3"]);
        assert_eq!(new_offset, 0); // new_count(3) - old_count(3) = 0
    }

    #[test]
    fn apply_hunk_returns_none_on_mismatch() {
        let l = lines(&["nope", "nope", "nope"]);
        assert!(apply_hunk(&l, &modify_hunk(), 0).is_none());
    }

    #[test]
    fn apply_hunk_finds_via_fuzz() {
        // Add a blank prefix line — hunk says start=1 but actual match is at line 2.
        let l = lines(&["blank", "line1", "line2", "line3"]);
        let (new_lines, _) = apply_hunk(&l, &modify_hunk(), 0).unwrap();
        assert_eq!(new_lines, vec!["blank", "line1", "line2 modified", "line3"]);
    }

    #[test]
    fn apply_hunk_offset_for_size_change() {
        let h = Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Remove("x".into()),
                HunkLine::Add("a".into()),
                HunkLine::Add("b".into()),
                HunkLine::Add("c".into()),
            ],
        };
        let l = lines(&["x"]);
        let (new_lines, offset) = apply_hunk(&l, &h, 0).unwrap();
        assert_eq!(new_lines, vec!["a", "b", "c"]);
        assert_eq!(offset, 2); // 3 - 1
    }
}
