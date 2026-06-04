//! OurOS `diff` Utility -- Compare Files Line by Line
//!
//! Compares two files (or directories with `-r`) and reports differences.
//! Supports normal (ed-style), unified, context, and side-by-side output
//! formats, with optional color, JSON output, and whitespace-handling flags.
//!
//! # Usage
//!
//! ```text
//! diff [OPTION]... FILE1 FILE2
//!
//! Compare files line by line.
//!
//!   -u, --unified[=N]           Unified diff format (default N=3 context lines)
//!   -c, --context[=N]           Context diff format (default N=3 context lines)
//!   -y, --side-by-side          Side-by-side comparison
//!   -W <cols>, --width=<cols>   Output width for side-by-side (default: 130)
//!   -q, --brief                 Only report whether files differ
//!   -s, --report-identical-files Report when files are identical
//!   -i, --ignore-case           Case-insensitive comparison
//!   -b, --ignore-space-change   Ignore changes in amount of whitespace
//!   -w, --ignore-all-space      Ignore all whitespace
//!   -B, --ignore-blank-lines    Ignore blank line insertions/deletions
//!       --color                 Force color output
//!       --no-color              Force no color
//!   -r, --recursive             Recursively compare directories
//!       --json                  JSON output
//!   -N, --new-file              Treat absent files as empty
//!       --help                  Display this help and exit
//!       --version               Output version information and exit
//! ```
//!
//! # Exit codes
//!
//! - 0: files are identical
//! - 1: files differ
//! - 2: error occurred

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Maximum file size (in bytes) to attempt reading into memory. Files larger
/// than this are rejected to avoid unbounded memory use.
const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB

/// Number of bytes to sample for binary file detection.
const BINARY_DETECT_LEN: usize = 8192;

// ============================================================================
// Output format
// ============================================================================

/// The diff output format to produce.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    /// Traditional ed-style (`NaNM`, `NcNM`, `NdN`).
    Normal,
    /// Unified diff (`-u`).
    Unified,
    /// Context diff (`-c`).
    Context,
    /// Side-by-side (`-y`).
    SideBySide,
}

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    path1: String,
    path2: String,
    format: Format,
    context_lines: usize,
    width: usize,
    brief: bool,
    report_identical: bool,
    ignore_case: bool,
    ignore_space_change: bool,
    ignore_all_space: bool,
    ignore_blank_lines: bool,
    color: bool,
    recursive: bool,
    json: bool,
    new_file: bool,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Diff operations
// ============================================================================

/// A single edit operation between two sequences.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Insert,
    Delete,
}

/// A contiguous group of changes with surrounding context.
struct Hunk {
    /// Starting line in file 1 (0-based).
    start1: usize,
    /// Number of lines from file 1 in this hunk.
    count1: usize,
    /// Starting line in file 2 (0-based).
    start2: usize,
    /// Number of lines from file 2 in this hunk.
    count2: usize,
    /// Operations and their associated line text.
    lines: Vec<(Op, String)>,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> ParseResult {
    let mut format = Format::Normal;
    let mut context_lines: Option<usize> = None;
    let mut width: usize = 130;
    let mut brief = false;
    let mut report_identical = false;
    let mut ignore_case = false;
    let mut ignore_space_change = false;
    let mut ignore_all_space = false;
    let mut ignore_blank_lines = false;
    let mut color: Option<bool> = None;
    let mut recursive = false;
    let mut json = false;
    let mut new_file = false;
    let mut positional: Vec<String> = Vec::new();

    let mut end_of_opts = false;
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') {
            positional.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            if arg == "--unified" {
                format = Format::Unified;
            } else if let Some(n_str) = arg.strip_prefix("--unified=") {
                format = Format::Unified;
                match n_str.parse::<usize>() {
                    Ok(n) => context_lines = Some(n),
                    Err(_) => {
                        eprintln!("diff: invalid context length '{n_str}'");
                        process::exit(2);
                    }
                }
            } else if arg == "--context" {
                format = Format::Context;
            } else if let Some(n_str) = arg.strip_prefix("--context=") {
                format = Format::Context;
                match n_str.parse::<usize>() {
                    Ok(n) => context_lines = Some(n),
                    Err(_) => {
                        eprintln!("diff: invalid context length '{n_str}'");
                        process::exit(2);
                    }
                }
            } else if arg == "--side-by-side" {
                format = Format::SideBySide;
            } else if arg == "--width" {
                i += 1;
                if i >= args.len() {
                    eprintln!("diff: option '--width' requires an argument");
                    process::exit(2);
                }
                match args[i].parse::<usize>() {
                    Ok(w) => width = w,
                    Err(_) => {
                        eprintln!("diff: invalid width '{}'", args[i]);
                        process::exit(2);
                    }
                }
            } else if let Some(w_str) = arg.strip_prefix("--width=") {
                match w_str.parse::<usize>() {
                    Ok(w) => width = w,
                    Err(_) => {
                        eprintln!("diff: invalid width '{w_str}'");
                        process::exit(2);
                    }
                }
            } else if arg == "--brief" {
                brief = true;
            } else if arg == "--report-identical-files" {
                report_identical = true;
            } else if arg == "--ignore-case" {
                ignore_case = true;
            } else if arg == "--ignore-space-change" {
                ignore_space_change = true;
            } else if arg == "--ignore-all-space" {
                ignore_all_space = true;
            } else if arg == "--ignore-blank-lines" {
                ignore_blank_lines = true;
            } else if arg == "--color" {
                color = Some(true);
            } else if arg == "--no-color" {
                color = Some(false);
            } else if arg == "--recursive" {
                recursive = true;
            } else if arg == "--json" {
                json = true;
            } else if arg == "--new-file" {
                new_file = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else {
                eprintln!("diff: unrecognized option '{arg}'");
                eprintln!("Try 'diff --help' for more information.");
                process::exit(2);
            }

            i += 1;
            continue;
        }

        // Short options. Some accept an optional or required value.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'u' => {
                    format = Format::Unified;
                    // Check for optional inline number: `-u3`
                    let rest: String = chars[j + 1..].iter().collect();
                    if !rest.is_empty()
                        && let Ok(n) = rest.parse::<usize>() {
                            context_lines = Some(n);
                            // Consumed rest of this arg group.
                            j = chars.len();
                            continue;
                        }
                }
                'c' => {
                    format = Format::Context;
                    let rest: String = chars[j + 1..].iter().collect();
                    if !rest.is_empty()
                        && let Ok(n) = rest.parse::<usize>() {
                            context_lines = Some(n);
                            j = chars.len();
                            continue;
                        }
                }
                'y' => format = Format::SideBySide,
                'W' => {
                    // -W may have value glued on or as next arg.
                    let rest: String = chars[j + 1..].iter().collect();
                    if !rest.is_empty() {
                        match rest.parse::<usize>() {
                            Ok(w) => width = w,
                            Err(_) => {
                                eprintln!("diff: invalid width '{rest}'");
                                process::exit(2);
                            }
                        }
                    } else {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("diff: option '-W' requires an argument");
                            process::exit(2);
                        }
                        match args[i].parse::<usize>() {
                            Ok(w) => width = w,
                            Err(_) => {
                                eprintln!("diff: invalid width '{}'", args[i]);
                                process::exit(2);
                            }
                        }
                    }
                    j = chars.len();
                    continue;
                }
                'q' => brief = true,
                's' => report_identical = true,
                'i' => ignore_case = true,
                'b' => ignore_space_change = true,
                'w' => ignore_all_space = true,
                'B' => ignore_blank_lines = true,
                'r' => recursive = true,
                'N' => new_file = true,
                other => {
                    eprintln!("diff: invalid option -- '{other}'");
                    eprintln!("Try 'diff --help' for more information.");
                    process::exit(2);
                }
            }
            j += 1;
        }

        i += 1;
    }

    if positional.len() != 2 {
        eprintln!("diff: requires exactly two file arguments");
        eprintln!("Try 'diff --help' for more information.");
        process::exit(2);
    }

    // Default color: false (OurOS does not have reliable isatty yet).
    let use_color = color.unwrap_or(false);
    let ctx = context_lines.unwrap_or(3);

    ParseResult::Run(Config {
        path1: positional[0].clone(),
        path2: positional[1].clone(),
        format,
        context_lines: ctx,
        width,
        brief,
        report_identical,
        ignore_case,
        ignore_space_change,
        ignore_all_space,
        ignore_blank_lines,
        color: use_color,
        recursive,
        json,
        new_file,
    })
}

// ============================================================================
// Line normalization for comparison
// ============================================================================

/// Normalize a line for comparison purposes based on the current flags.
fn normalize_line(line: &str, config: &Config) -> String {
    let mut s = line.to_string();

    if config.ignore_all_space {
        s.retain(|c| !c.is_whitespace());
    } else if config.ignore_space_change {
        // Collapse runs of whitespace into a single space; trim trailing.
        let mut result = String::with_capacity(s.len());
        let mut in_space = false;
        for ch in s.chars() {
            if ch.is_whitespace() {
                if !in_space {
                    result.push(' ');
                    in_space = true;
                }
            } else {
                result.push(ch);
                in_space = false;
            }
        }
        // Trim trailing single space that might result from trailing whitespace.
        if result.ends_with(' ') {
            result.pop();
        }
        s = result;
    }

    if config.ignore_case {
        s = s.to_lowercase();
    }

    s
}

/// Returns true if a line is considered blank for `--ignore-blank-lines`.
fn is_blank(line: &str) -> bool {
    line.chars().all(|c| c.is_whitespace())
}

// ============================================================================
// File reading
// ============================================================================

/// Result of reading a file for diffing.
enum FileContent {
    /// Text file, split into lines.
    Text(Vec<String>),
    /// Binary file detected (contains NUL bytes).
    Binary,
}

/// Read a file into lines. Returns `Err` on I/O errors, `Ok(Binary)` if the
/// file contains NUL bytes, or `Ok(Text(lines))` for normal text files.
fn read_file(path: &Path) -> Result<FileContent, String> {
    let metadata = fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "{}: file too large ({} bytes, max {})",
            path.display(),
            metadata.len(),
            MAX_FILE_SIZE,
        ));
    }

    let data = fs::read(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    // Check for binary content in the first BINARY_DETECT_LEN bytes.
    let check_len = data.len().min(BINARY_DETECT_LEN);
    if data[..check_len].contains(&0u8) {
        return Ok(FileContent::Binary);
    }

    // Convert to string. We use lossy conversion here only for the purpose of
    // displaying diff output; the comparison is byte-accurate via the line
    // strings.
    let text = String::from_utf8(data)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());

    // Split into lines, preserving the line content without trailing newlines.
    let lines: Vec<String> = text.lines().map(String::from).collect();

    Ok(FileContent::Text(lines))
}

// ============================================================================
// Myers diff algorithm (O(ND) shortest edit script)
// ============================================================================

/// Compute the longest common subsequence edit script between two line
/// sequences using the Myers O(ND) algorithm.
///
/// Returns a vector of `(Op, line_text)` pairs describing the full edit
/// sequence from `a` to `b`.
fn compute_diff(a: &[String], b: &[String], config: &Config) -> Vec<(Op, String)> {
    let n = a.len();
    let m = b.len();

    // Build normalized comparison keys.
    let norm_a: Vec<String> = a.iter().map(|l| normalize_line(l, config)).collect();
    let norm_b: Vec<String> = b.iter().map(|l| normalize_line(l, config)).collect();

    // For very large inputs, fall back to a simpler LCS DP when both files are
    // small enough that the O(NM) table fits in memory (< ~10K lines each).
    // For larger inputs, use the Myers algorithm which is O(ND) where D is the
    // edit distance.
    if n <= 10000 && m <= 10000 {
        lcs_diff(&norm_a, &norm_b, a, b)
    } else {
        myers_diff(&norm_a, &norm_b, a, b)
    }
}

/// LCS-based diff using dynamic programming. O(NM) time and space.
/// Suitable for files up to ~10K lines.
fn lcs_diff(
    norm_a: &[String],
    norm_b: &[String],
    orig_a: &[String],
    orig_b: &[String],
) -> Vec<(Op, String)> {
    let n = norm_a.len();
    let m = norm_b.len();

    // Build LCS length table.
    // dp[i][j] = length of LCS of norm_a[0..i] and norm_b[0..j].
    let mut dp = vec![vec![0u32; m + 1]; n + 1];

    for i in 1..=n {
        for j in 1..=m {
            if norm_a[i - 1] == norm_b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else if dp[i - 1][j] >= dp[i][j - 1] {
                dp[i][j] = dp[i - 1][j];
            } else {
                dp[i][j] = dp[i][j - 1];
            }
        }
    }

    // Trace back to build edit script.
    let mut ops: Vec<(Op, String)> = Vec::new();
    let mut i = n;
    let mut j = m;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && norm_a[i - 1] == norm_b[j - 1] {
            ops.push((Op::Equal, orig_a[i - 1].clone()));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push((Op::Insert, orig_b[j - 1].clone()));
            j -= 1;
        } else {
            ops.push((Op::Delete, orig_a[i - 1].clone()));
            i -= 1;
        }
    }

    ops.reverse();
    ops
}

/// Myers diff algorithm for large files. O(ND) time where D is edit distance.
fn myers_diff(
    norm_a: &[String],
    norm_b: &[String],
    orig_a: &[String],
    orig_b: &[String],
) -> Vec<(Op, String)> {
    let n = norm_a.len();
    let m = norm_b.len();

    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return orig_b.iter().map(|l| (Op::Insert, l.clone())).collect();
    }
    if m == 0 {
        return orig_a.iter().map(|l| (Op::Delete, l.clone())).collect();
    }

    // Myers shortest edit script. We store the V array for each iteration of d
    // so we can trace back the path.
    let max_d = n + m;
    let v_size = 2 * max_d + 1;

    // v_history[d] stores the V array snapshot after processing edit distance d.
    let mut v_history: Vec<Vec<isize>> = Vec::new();
    let mut v = vec![0isize; v_size];

    let offset = max_d as isize;

    // Helper to index into v with potentially negative k.
    let idx = |k: isize| -> usize { (k + offset) as usize };

    let mut found_d: Option<usize> = None;

    for d in 0..=max_d {
        let old_v = v.clone();
        let d_signed = d as isize;

        let mut k = -d_signed;
        while k <= d_signed {
            let x: isize;
            if k == -d_signed
                || (k != d_signed && old_v[idx(k - 1)] < old_v[idx(k + 1)])
            {
                x = old_v[idx(k + 1)];
            } else {
                x = old_v[idx(k - 1)] + 1;
            }

            let mut x_curr = x;
            let mut y_curr = x_curr - k;

            // Follow diagonal (matching lines).
            while (x_curr as usize) < n
                && (y_curr as usize) < m
                && norm_a[x_curr as usize] == norm_b[y_curr as usize]
            {
                x_curr += 1;
                y_curr += 1;
            }

            v[idx(k)] = x_curr;

            if x_curr as usize >= n && y_curr as usize >= m {
                v_history.push(v.clone());
                found_d = Some(d);
                break;
            }

            k += 2;
        }

        if found_d.is_some() {
            break;
        }
        v_history.push(v.clone());
    }

    // Trace back the path.
    let total_d = found_d.unwrap_or(max_d);
    let mut path: Vec<(usize, usize)> = Vec::new();

    let mut cx = n as isize;
    let mut cy = m as isize;

    for d in (0..=total_d).rev() {
        let d_signed = d as isize;
        let k = cx - cy;
        let vd = &v_history[d];

        let prev_k: isize;
        if k == -d_signed
            || (k != d_signed && vd[idx(k - 1)] < vd[idx(k + 1)])
        {
            prev_k = k + 1;
        } else {
            prev_k = k - 1;
        }

        let prev_x = if d > 0 { v_history[d - 1][idx(prev_k)] } else { 0 };
        let prev_y = prev_x - prev_k;

        // Record diagonal moves first (equal lines), walking backward.
        while cx > prev_x && cy > prev_y {
            cx -= 1;
            cy -= 1;
            path.push((cx as usize, cy as usize));
        }

        if d > 0 {
            if prev_k < k {
                // Deletion from a (move right in the grid).
                path.push((prev_x as usize, prev_y as usize));
            } else {
                // Insertion from b (move down).
                path.push((prev_x as usize, prev_y as usize));
            }
        }

        cx = prev_x;
        cy = prev_y;
    }

    path.reverse();

    // Convert path into edit operations.
    let mut ops: Vec<(Op, String)> = Vec::new();
    let mut ai: usize = 0;
    let mut bi: usize = 0;

    for &(px, py) in &path {
        // If we need to skip to (px, py), emit deletes/inserts.
        while ai < px && bi < py {
            ops.push((Op::Equal, orig_a[ai].clone()));
            ai += 1;
            bi += 1;
        }
        while ai < px {
            ops.push((Op::Delete, orig_a[ai].clone()));
            ai += 1;
        }
        while bi < py {
            ops.push((Op::Insert, orig_b[bi].clone()));
            bi += 1;
        }
        // The point itself.
        if ai == px && bi == py && ai < n && bi < m {
            if norm_a[ai] == norm_b[bi] {
                ops.push((Op::Equal, orig_a[ai].clone()));
                ai += 1;
                bi += 1;
            } else if ai < n {
                ops.push((Op::Delete, orig_a[ai].clone()));
                ai += 1;
            }
        }
    }

    // Flush remaining.
    while ai < n {
        ops.push((Op::Delete, orig_a[ai].clone()));
        ai += 1;
    }
    while bi < m {
        ops.push((Op::Insert, orig_b[bi].clone()));
        bi += 1;
    }

    ops
}

// ============================================================================
// Blank-line filtering
// ============================================================================

/// If `--ignore-blank-lines` is set, reclassify insertions and deletions of
/// blank lines as equal (keeping the line from whichever side is available).
fn filter_blank_lines(ops: &mut Vec<(Op, String)>) {
    for entry in ops.iter_mut() {
        if entry.0 != Op::Equal && is_blank(&entry.1) {
            entry.0 = Op::Equal;
        }
    }
}

// ============================================================================
// Hunk construction
// ============================================================================

/// Group edit operations into hunks with the given number of context lines.
fn build_hunks(ops: &[(Op, String)], context: usize) -> Vec<Hunk> {
    if ops.is_empty() {
        return Vec::new();
    }

    // Find indices of all non-equal operations.
    let change_indices: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (op, _))| *op != Op::Equal)
        .map(|(i, _)| i)
        .collect();

    if change_indices.is_empty() {
        return Vec::new();
    }

    // Group changes that are within `2 * context` lines of each other.
    let mut groups: Vec<(usize, usize)> = Vec::new(); // (first_change_idx, last_change_idx)
    let mut group_start = change_indices[0];
    let mut group_end = change_indices[0];

    for &ci in &change_indices[1..] {
        // Count equal lines between group_end and ci.
        let gap = ops[group_end + 1..ci]
            .iter()
            .filter(|(op, _)| *op == Op::Equal)
            .count();

        if gap <= 2 * context {
            group_end = ci;
        } else {
            groups.push((group_start, group_end));
            group_start = ci;
            group_end = ci;
        }
    }
    groups.push((group_start, group_end));

    // Build hunks with context around each group.
    let mut hunks = Vec::new();

    for (gs, ge) in groups {
        let hunk_start = gs.saturating_sub(context);
        let hunk_end = (ge + context + 1).min(ops.len());

        let hunk_ops = &ops[hunk_start..hunk_end];

        // Count lines from file1 and file2 within this hunk, and track start
        // positions.
        let mut line1: usize = 0;
        let mut line2: usize = 0;

        // Count lines before hunk_start to determine the starting line numbers.
        for (op, _) in &ops[..hunk_start] {
            match op {
                Op::Equal => {
                    line1 += 1;
                    line2 += 1;
                }
                Op::Delete => line1 += 1,
                Op::Insert => line2 += 1,
            }
        }

        let start1 = line1;
        let start2 = line2;

        let mut count1 = 0usize;
        let mut count2 = 0usize;
        let mut lines: Vec<(Op, String)> = Vec::new();

        for (op, text) in hunk_ops {
            match op {
                Op::Equal => {
                    count1 += 1;
                    count2 += 1;
                }
                Op::Delete => count1 += 1,
                Op::Insert => count2 += 1,
            }
            lines.push((*op, text.clone()));
        }

        hunks.push(Hunk {
            start1,
            count1,
            start2,
            count2,
            lines,
        });
    }

    hunks
}

// ============================================================================
// Color helpers
// ============================================================================

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

fn color_red(s: &str, color: bool) -> String {
    if color {
        format!("{RED}{s}{RESET}")
    } else {
        s.to_string()
    }
}

fn color_green(s: &str, color: bool) -> String {
    if color {
        format!("{GREEN}{s}{RESET}")
    } else {
        s.to_string()
    }
}

fn color_cyan(s: &str, color: bool) -> String {
    if color {
        format!("{CYAN}{s}{RESET}")
    } else {
        s.to_string()
    }
}

// ============================================================================
// Normal diff output
// ============================================================================

/// Format a 1-based line range for normal diff headers.
fn range_str(start: usize, count: usize) -> String {
    if count == 0 {
        format!("{}", start)
    } else if count == 1 {
        format!("{}", start + 1)
    } else {
        format!("{},{}", start + 1, start + count)
    }
}

fn print_normal(hunks: &[Hunk], config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    for hunk in hunks {
        // Determine the operation type for this hunk: all deletes, all inserts,
        // or a change (mix).
        let has_del = hunk.lines.iter().any(|(op, _)| *op == Op::Delete);
        let has_ins = hunk.lines.iter().any(|(op, _)| *op == Op::Insert);

        let del_count = hunk.lines.iter().filter(|(op, _)| *op == Op::Delete).count();
        let ins_count = hunk.lines.iter().filter(|(op, _)| *op == Op::Insert).count();

        // Compute file-1 and file-2 ranges for the changed lines only (not context).
        // We need start positions relative to the hunk's changes.
        let mut line1_pos = hunk.start1;
        let mut line2_pos = hunk.start2;

        // Skip leading context to find where changes begin.
        for (op, _) in &hunk.lines {
            if *op == Op::Equal {
                line1_pos += 1;
                line2_pos += 1;
            } else {
                break;
            }
        }

        let r1 = range_str(
            if del_count > 0 { line1_pos } else { line1_pos },
            del_count,
        );
        let r2 = range_str(
            if ins_count > 0 { line2_pos } else { line2_pos },
            ins_count,
        );

        let op_char = match (has_del, has_ins) {
            (true, true) => 'c',
            (true, false) => 'd',
            (false, true) => 'a',
            (false, false) => continue, // all equal, skip
        };

        let header = match op_char {
            'a' => format!("{r1}{op_char}{r2}"),
            'd' => format!("{r1}{op_char}{r2}"),
            _ => format!("{r1}{op_char}{r2}"),
        };

        let _ = writeln!(w, "{}", color_cyan(&header, config.color));

        // Print deleted lines.
        for (op, text) in &hunk.lines {
            if *op == Op::Delete {
                let line = format!("< {text}");
                let _ = writeln!(w, "{}", color_red(&line, config.color));
            }
        }

        // Separator between deletes and inserts for 'c' operations.
        if has_del && has_ins {
            let _ = writeln!(w, "---");
        }

        // Print inserted lines.
        for (op, text) in &hunk.lines {
            if *op == Op::Insert {
                let line = format!("> {text}");
                let _ = writeln!(w, "{}", color_green(&line, config.color));
            }
        }
    }
}

// ============================================================================
// Unified diff output
// ============================================================================

fn print_unified(hunks: &[Hunk], path1: &str, path2: &str, config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    // File headers.
    let hdr1 = format!("--- {path1}");
    let hdr2 = format!("+++ {path2}");
    let _ = writeln!(w, "{}", color_red(&hdr1, config.color));
    let _ = writeln!(w, "{}", color_green(&hdr2, config.color));

    for hunk in hunks {
        // Hunk header: @@ -start,count +start,count @@
        let h1_start = hunk.start1 + 1;
        let h2_start = hunk.start2 + 1;
        let header = format!(
            "@@ -{},{} +{},{} @@",
            if hunk.count1 == 0 { 0 } else { h1_start },
            hunk.count1,
            if hunk.count2 == 0 { 0 } else { h2_start },
            hunk.count2,
        );
        let _ = writeln!(w, "{}", color_cyan(&header, config.color));

        for (op, text) in &hunk.lines {
            match op {
                Op::Equal => {
                    let _ = writeln!(w, " {text}");
                }
                Op::Delete => {
                    let line = format!("-{text}");
                    let _ = writeln!(w, "{}", color_red(&line, config.color));
                }
                Op::Insert => {
                    let line = format!("+{text}");
                    let _ = writeln!(w, "{}", color_green(&line, config.color));
                }
            }
        }
    }
}

// ============================================================================
// Context diff output
// ============================================================================

fn print_context(hunks: &[Hunk], path1: &str, path2: &str, config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    let _ = writeln!(w, "{}", color_red(&format!("*** {path1}"), config.color));
    let _ = writeln!(
        w,
        "{}",
        color_green(&format!("--- {path2}"), config.color)
    );

    for hunk in hunks {
        let _ = writeln!(w, "***************");

        // File 1 section.
        let f1_start = hunk.start1 + 1;
        let f1_end = hunk.start1 + hunk.count1;
        let _ = writeln!(
            w,
            "{}",
            color_cyan(
                &format!("*** {},{} ****", f1_start, if f1_end == 0 { f1_start } else { f1_end }),
                config.color,
            )
        );

        for (op, text) in &hunk.lines {
            match op {
                Op::Equal => {
                    let _ = writeln!(w, "  {text}");
                }
                Op::Delete => {
                    let line = format!("- {text}");
                    let _ = writeln!(w, "{}", color_red(&line, config.color));
                }
                Op::Insert => {
                    // Inserts are not shown in the file-1 section.
                }
            }
        }

        // File 2 section.
        let f2_start = hunk.start2 + 1;
        let f2_end = hunk.start2 + hunk.count2;
        let _ = writeln!(
            w,
            "{}",
            color_cyan(
                &format!("--- {},{} ----", f2_start, if f2_end == 0 { f2_start } else { f2_end }),
                config.color,
            )
        );

        for (op, text) in &hunk.lines {
            match op {
                Op::Equal => {
                    let _ = writeln!(w, "  {text}");
                }
                Op::Insert => {
                    let line = format!("+ {text}");
                    let _ = writeln!(w, "{}", color_green(&line, config.color));
                }
                Op::Delete => {
                    // Deletes are not shown in the file-2 section.
                }
            }
        }
    }
}

// ============================================================================
// Side-by-side output
// ============================================================================

fn print_side_by_side(ops: &[(Op, String)], config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    // Reserve 3 chars for the separator column (" | ", " < ", " > ", "   ").
    let col_width = if config.width > 3 {
        (config.width - 3) / 2
    } else {
        30
    };

    for (op, text) in ops {
        match op {
            Op::Equal => {
                let left = truncate_or_pad(text, col_width);
                let right = truncate_or_pad(text, col_width);
                let _ = writeln!(w, "{left}   {right}");
            }
            Op::Delete => {
                let left = truncate_or_pad(text, col_width);
                let right = " ".repeat(col_width);
                let line = format!("{left} < {right}");
                let _ = writeln!(w, "{}", color_red(&line, config.color));
            }
            Op::Insert => {
                let left = " ".repeat(col_width);
                let right = truncate_or_pad(text, col_width);
                let line = format!("{left} > {right}");
                let _ = writeln!(w, "{}", color_green(&line, config.color));
            }
        }
    }
}

/// Truncate or pad a string to exactly `width` display characters.
fn truncate_or_pad(s: &str, width: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= width {
        format!("{s}{}", " ".repeat(width - char_count))
    } else {
        let truncated: String = s.chars().take(width) .collect();
        truncated
    }
}

// ============================================================================
// JSON output
// ============================================================================

fn print_json(ops: &[(Op, String)], path1: &str, path2: &str, has_diff: bool) {
    let out = io::stdout();
    let mut w = out.lock();

    let _ = write!(w, "{{");
    let _ = write!(w, "\"file1\":{},", json_escape(path1));
    let _ = write!(w, "\"file2\":{},", json_escape(path2));
    let _ = write!(w, "\"identical\":{},", !has_diff);
    let _ = write!(w, "\"changes\":[");

    let mut first = true;
    let mut line1: usize = 0;
    let mut line2: usize = 0;

    for (op, text) in ops {
        if *op == Op::Equal {
            line1 += 1;
            line2 += 1;
            continue;
        }

        if !first {
            let _ = write!(w, ",");
        }
        first = false;

        let op_str = match op {
            Op::Delete => "delete",
            Op::Insert => "insert",
            Op::Equal => "equal",
        };

        let line_num = match op {
            Op::Delete => {
                line1 += 1;
                line1
            }
            Op::Insert => {
                line2 += 1;
                line2
            }
            Op::Equal => 0,
        };

        let _ = write!(
            w,
            "{{\"op\":{},\"line\":{},\"text\":{}}}",
            json_escape(op_str),
            line_num,
            json_escape(text),
        );
    }

    let _ = writeln!(w, "]}}");
}

/// Escape a string for JSON output. Produces a quoted JSON string.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                // Control character: encode as \u00XX.
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Directory comparison
// ============================================================================

/// Recursively compare two directories. Returns the worst exit code seen.
fn diff_dirs(path1: &Path, path2: &Path, config: &Config) -> i32 {
    let mut entries1 = match list_dir(path1) {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("diff: {msg}");
            return 2;
        }
    };
    let mut entries2 = match list_dir(path2) {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("diff: {msg}");
            return 2;
        }
    };

    entries1.sort();
    entries2.sort();

    // Merge the two sorted lists.
    let mut all_names: Vec<String> = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < entries1.len() && j < entries2.len() {
        match entries1[i].cmp(&entries2[j]) {
            std::cmp::Ordering::Less => {
                all_names.push(entries1[i].clone());
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                all_names.push(entries2[j].clone());
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                all_names.push(entries1[i].clone());
                i += 1;
                j += 1;
            }
        }
    }
    while i < entries1.len() {
        all_names.push(entries1[i].clone());
        i += 1;
    }
    while j < entries2.len() {
        all_names.push(entries2[j].clone());
        j += 1;
    }

    let mut worst_exit = 0;

    for name in &all_names {
        let p1 = path1.join(name);
        let p2 = path2.join(name);
        let e1 = p1.exists();
        let e2 = p2.exists();

        if !e1 && !config.new_file {
            eprintln!(
                "Only in {}: {name}",
                path2.display()
            );
            if worst_exit < 1 {
                worst_exit = 1;
            }
            continue;
        }
        if !e2 && !config.new_file {
            eprintln!(
                "Only in {}: {name}",
                path1.display()
            );
            if worst_exit < 1 {
                worst_exit = 1;
            }
            continue;
        }

        let is_dir1 = e1 && p1.is_dir();
        let is_dir2 = e2 && p2.is_dir();

        if is_dir1 && is_dir2 {
            let code = diff_dirs(&p1, &p2, config);
            if code > worst_exit {
                worst_exit = code;
            }
        } else if is_dir1 || is_dir2 {
            eprintln!(
                "diff: {} is a directory while {} is not",
                if is_dir1 { p1.display() } else { p2.display() },
                if is_dir1 { p2.display() } else { p1.display() },
            );
            if worst_exit < 1 {
                worst_exit = 1;
            }
        } else {
            let code = diff_files(
                &p1.to_string_lossy(),
                &p2.to_string_lossy(),
                config,
            );
            if code > worst_exit {
                worst_exit = code;
            }
        }
    }

    worst_exit
}

/// List entries in a directory, returning just the file/dir names.
fn list_dir(path: &Path) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

// ============================================================================
// Core diff driver
// ============================================================================

/// Compare two files and print the diff. Returns exit code (0/1/2).
fn diff_files(path1_str: &str, path2_str: &str, config: &Config) -> i32 {
    let p1 = Path::new(path1_str);
    let p2 = Path::new(path2_str);

    let e1 = p1.exists();
    let e2 = p2.exists();

    // Handle absent files with --new-file.
    if !e1 && !config.new_file {
        eprintln!("diff: {path1_str}: No such file or directory");
        return 2;
    }
    if !e2 && !config.new_file {
        eprintln!("diff: {path2_str}: No such file or directory");
        return 2;
    }

    // Read file contents (absent files treated as empty when -N is set).
    let content1 = if e1 {
        match read_file(p1) {
            Ok(c) => c,
            Err(msg) => {
                eprintln!("diff: {msg}");
                return 2;
            }
        }
    } else {
        FileContent::Text(Vec::new())
    };

    let content2 = if e2 {
        match read_file(p2) {
            Ok(c) => c,
            Err(msg) => {
                eprintln!("diff: {msg}");
                return 2;
            }
        }
    } else {
        FileContent::Text(Vec::new())
    };

    // Handle binary files.
    match (&content1, &content2) {
        (FileContent::Binary, _) | (_, FileContent::Binary) => {
            // For binary files, just report whether they differ.
            if config.brief || config.json {
                println!("Binary files {path1_str} and {path2_str} differ");
            } else {
                println!("Binary files {path1_str} and {path2_str} differ");
            }
            return 1;
        }
        _ => {}
    }

    let lines1 = match &content1 {
        FileContent::Text(l) => l,
        FileContent::Binary => return 2, // unreachable due to match above
    };
    let lines2 = match &content2 {
        FileContent::Text(l) => l,
        FileContent::Binary => return 2,
    };

    // Compute the diff.
    let mut ops = compute_diff(lines1, lines2, config);

    // Apply blank-line filtering if requested.
    if config.ignore_blank_lines {
        filter_blank_lines(&mut ops);
    }

    // Check if there are any differences.
    let has_diff = ops.iter().any(|(op, _)| *op != Op::Equal);

    if !has_diff {
        if config.report_identical {
            println!("Files {path1_str} and {path2_str} are identical");
        }
        return 0;
    }

    if config.brief {
        println!("Files {path1_str} and {path2_str} differ");
        return 1;
    }

    if config.json {
        print_json(&ops, path1_str, path2_str, has_diff);
        return 1;
    }

    // Format the output.
    match config.format {
        Format::SideBySide => {
            print_side_by_side(&ops, config);
        }
        Format::Normal => {
            let hunks = build_hunks(&ops, 0);
            print_normal(&hunks, config);
        }
        Format::Unified => {
            let hunks = build_hunks(&ops, config.context_lines);
            print_unified(&hunks, path1_str, path2_str, config);
        }
        Format::Context => {
            let hunks = build_hunks(&ops, config.context_lines);
            print_context(&hunks, path1_str, path2_str, config);
        }
    }

    1
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS diff v{VERSION}");
    println!();
    println!("Compare files line by line.");
    println!();
    println!("USAGE:");
    println!("  diff [OPTION]... FILE1 FILE2");
    println!();
    println!("OUTPUT FORMATS:");
    println!("  (default)                 Normal diff (ed-style commands)");
    println!("  -u, --unified[=N]         Unified format with N context lines (default 3)");
    println!("  -c, --context[=N]         Context format with N context lines (default 3)");
    println!("  -y, --side-by-side        Side-by-side comparison");
    println!("  -W <cols>, --width=<cols>  Output width for side-by-side (default 130)");
    println!();
    println!("FILTERING:");
    println!("  -q, --brief                 Only report whether files differ");
    println!("  -s, --report-identical-files Report identical files");
    println!("  -i, --ignore-case           Case-insensitive comparison");
    println!("  -b, --ignore-space-change   Ignore changes in whitespace amount");
    println!("  -w, --ignore-all-space      Ignore all whitespace");
    println!("  -B, --ignore-blank-lines    Ignore blank line changes");
    println!();
    println!("OUTPUT:");
    println!("      --color               Force color output");
    println!("      --no-color            Force no color");
    println!("      --json                JSON output");
    println!();
    println!("DIRECTORY:");
    println!("  -r, --recursive           Recursively compare directories");
    println!("  -N, --new-file            Treat absent files as empty");
    println!();
    println!("MISC:");
    println!("      --help                Display this help and exit");
    println!("      --version             Output version information and exit");
    println!();
    println!("EXIT STATUS:");
    println!("  0  Files are identical");
    println!("  1  Files differ");
    println!("  2  Error occurred");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    match parse_args(&args) {
        ParseResult::Help => {
            print_help();
            process::exit(0);
        }
        ParseResult::Version => {
            println!("diff (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let p1 = PathBuf::from(&config.path1);
            let p2 = PathBuf::from(&config.path2);

            let is_dir1 = p1.is_dir();
            let is_dir2 = p2.is_dir();

            let exit_code = if is_dir1 && is_dir2 {
                if config.recursive {
                    diff_dirs(&p1, &p2, &config)
                } else {
                    eprintln!(
                        "diff: {} and {} are directories (use -r to compare recursively)",
                        config.path1, config.path2,
                    );
                    2
                }
            } else if is_dir1 || is_dir2 {
                // One is a directory, one is a file -- diff the file against
                // the same-named file in the directory.
                let (dir, file) = if is_dir1 {
                    (&p1, &p2)
                } else {
                    (&p2, &p1)
                };

                if let Some(fname) = file.file_name() {
                    let target = dir.join(fname);
                    let t_str = target.to_string_lossy();
                    let f_str = file.to_string_lossy();

                    if is_dir1 {
                        diff_files(&t_str, &f_str, &config)
                    } else {
                        diff_files(&f_str, &t_str, &config)
                    }
                } else {
                    eprintln!("diff: cannot determine filename from path");
                    2
                }
            } else {
                diff_files(&config.path1, &config.path2, &config)
            };

            process::exit(exit_code);
        }
    }
}
