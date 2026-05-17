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
struct Hunk {
    old_start: usize,  // 1-based line number in original file
    old_count: usize,
    new_start: usize,  // 1-based line number in new file
    new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

/// A patch for a single file, consisting of one or more hunks.
#[derive(Debug)]
struct FilePatch {
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

struct Options {
    strip: Option<usize>,
    patch_file: Option<String>,
    reverse: bool,
    dry_run: bool,
    silent: bool,
    backup: bool,
    target_file: Option<String>,
}

/// Strip NUM leading path components from a file path.
fn strip_path(path: &str, num: usize) -> String {
    if num == 0 {
        return path.to_string();
    }
    let parts: Vec<&str> = path.splitn(num + 1, '/').collect();
    if parts.len() > num {
        parts[num].to_string()
    } else {
        // If there aren't enough components, return the basename.
        path.rsplit('/').next().unwrap_or(path).to_string()
    }
}

/// Parse the @@ -old_start,old_count +new_start,new_count @@ line.
fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize)> {
    // Format: @@ -A,B +C,D @@ optional text
    let line = line.trim();
    if !line.starts_with("@@") {
        return None;
    }

    let after_at = &line[2..];
    let end_at = after_at.find("@@")?;
    let range_part = after_at[..end_at].trim();

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

    while i < lines.len() {
        // Look for --- line followed by +++ line.
        if lines[i].starts_with("--- ") && i + 1 < lines.len() && lines[i + 1].starts_with("+++ ")
        {
            let old_path = parse_file_path(lines[i], "--- ");
            let new_path = parse_file_path(lines[i + 1], "+++ ");
            i += 2;

            let mut hunks: Vec<Hunk> = Vec::new();

            // Parse hunks for this file.
            while i < lines.len() {
                if lines[i].starts_with("@@ ") {
                    if let Some((os, oc, ns, nc)) = parse_hunk_header(lines[i]) {
                        i += 1;
                        let mut hunk_lines: Vec<HunkLine> = Vec::new();

                        while i < lines.len() {
                            let line = lines[i];
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
                            i += 1;
                        }

                        hunks.push(Hunk {
                            old_start: os,
                            old_count: oc,
                            new_start: ns,
                            new_count: nc,
                            lines: hunk_lines,
                        });
                    } else {
                        i += 1;
                    }
                } else if lines[i].starts_with("--- ") || lines[i].starts_with("diff ") {
                    // Next file patch starts here.
                    break;
                } else {
                    i += 1;
                }
            }

            patches.push(FilePatch {
                old_path,
                new_path,
                hunks,
            });
        } else {
            i += 1;
        }
    }

    patches
}

fn parse_file_path(line: &str, prefix: &str) -> String {
    let rest = &line[prefix.len()..];
    // Remove timestamp suffix if present (e.g., "file.c\t2024-01-01 ...")
    if let Some(tab_pos) = rest.find('\t') {
        rest[..tab_pos].to_string()
    } else {
        rest.to_string()
    }
}

/// Apply a single hunk to the file lines. Returns the new lines if successful,
/// or None if the hunk doesn't match the expected context.
/// `offset` is the cumulative line offset from previous hunks.
fn apply_hunk(lines: &[String], hunk: &Hunk, offset: i64) -> Option<(Vec<String>, i64)> {
    let target_start = (hunk.old_start as i64 + offset - 1).max(0) as usize;

    // Try exact position first, then search nearby (fuzz).
    let max_fuzz = 50;
    let mut best_pos = None;

    for fuzz in 0..=max_fuzz {
        for direction in &[0i64, -1, 1] {
            let try_pos = if *direction == 0 && fuzz == 0 {
                target_start
            } else if *direction < 0 {
                if target_start >= fuzz {
                    target_start - fuzz
                } else {
                    continue;
                }
            } else if *direction > 0 {
                target_start + fuzz
            } else {
                continue;
            };

            if fuzz == 0 && *direction != 0 {
                continue;
            }

            if try_hunk_at(lines, hunk, try_pos) {
                best_pos = Some(try_pos);
                break;
            }
        }
        if best_pos.is_some() {
            break;
        }
    }

    let pos = best_pos?;

    // Build the new file content.
    let mut result = Vec::new();
    result.extend_from_slice(&lines[..pos]);

    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(s) => result.push(s.clone()),
            HunkLine::Add(s) => result.push(s.clone()),
            HunkLine::Remove(_) => {} // skip removed lines
        }
    }

    // Count how many old lines the hunk consumed.
    let old_consumed = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_) | HunkLine::Remove(_)))
        .count();

    if pos + old_consumed <= lines.len() {
        result.extend_from_slice(&lines[pos + old_consumed..]);
    }

    // The offset adjustment is new_count - old_count.
    let new_offset = offset + hunk.new_count as i64 - hunk.old_count as i64;

    Some((result, new_offset))
}

/// Check if a hunk's context/remove lines match at the given position.
fn try_hunk_at(lines: &[String], hunk: &Hunk, pos: usize) -> bool {
    let mut line_idx = pos;
    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(expected) => {
                if line_idx >= lines.len() {
                    return false;
                }
                if lines[line_idx] != *expected {
                    return false;
                }
                line_idx += 1;
            }
            HunkLine::Remove(expected) => {
                if line_idx >= lines.len() {
                    return false;
                }
                if lines[line_idx] != *expected {
                    return false;
                }
                line_idx += 1;
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
    let mut opts = Options {
        strip: None,
        patch_file: None,
        reverse: false,
        dry_run: false,
        silent: false,
        backup: false,
        target_file: None,
    };
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-i" {
            i += 1;
            if i >= args.len() {
                eprintln!("patch: option -i requires an argument");
                process::exit(2);
            }
            opts.patch_file = Some(args[i].clone());
        } else if arg == "-p" {
            i += 1;
            if i >= args.len() {
                eprintln!("patch: option -p requires an argument");
                process::exit(2);
            }
            match args[i].parse::<usize>() {
                Ok(n) => opts.strip = Some(n),
                Err(_) => {
                    eprintln!("patch: invalid strip count: {}", args[i]);
                    process::exit(2);
                }
            }
        } else if arg.starts_with("-p") && arg.len() > 2 {
            match arg[2..].parse::<usize>() {
                Ok(n) => opts.strip = Some(n),
                Err(_) => {
                    eprintln!("patch: invalid strip count: {}", &arg[2..]);
                    process::exit(2);
                }
            }
        } else if arg == "-R" || arg == "--reverse" {
            opts.reverse = true;
        } else if arg == "--dry-run" {
            opts.dry_run = true;
        } else if arg == "-s" || arg == "--silent" || arg == "--quiet" {
            opts.silent = true;
        } else if arg == "-b" || arg == "--backup" {
            opts.backup = true;
        } else if arg.starts_with('-') && arg.len() > 1 && arg != "-" {
            eprintln!("patch: unknown option: {arg}");
            process::exit(2);
        } else {
            opts.target_file = Some(arg.clone());
        }
        i += 1;
    }

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
            if let Some(parent) = Path::new(&file_path).parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = fs::create_dir_all(parent);
                }
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
