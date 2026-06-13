//! patch -- apply a diff file to originals.
//!
//! A complete POSIX-compatible patch utility for SlateOS that supports
//! unified, context, and normal diff formats.
//!
//! Exit codes:
//!   0  all hunks applied successfully
//!   1  some hunks failed
//!   2  error (cannot read patch, bad usage, etc.)

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A single line within a hunk, tagged by role.
#[derive(Debug, Clone, PartialEq)]
enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

/// A parsed hunk with line-number metadata.
#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<HunkLine>,
}

/// Represents one file's worth of hunks from a patch.
#[derive(Debug)]
struct FilePatch {
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

/// Which diff format was detected.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DiffFormat {
    Unified,
    Context,
    Normal,
}

/// Command-line options.
struct Options {
    strip: Option<usize>,
    directory: Option<String>,
    patch_file: Option<String>,
    output_file: Option<String>,
    reverse: bool,
    forward: bool,
    backup: bool,
    backup_suffix: String,
    force: bool,
    silent: bool,
    verbose: bool,
    dry_run: bool,
    ignore_whitespace: bool,
    remove_empty_files: bool,
    no_backup_if_mismatch: bool,
    fuzz: usize,
    target_file: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            strip: None,
            directory: None,
            patch_file: None,
            output_file: None,
            reverse: false,
            forward: false,
            backup: false,
            backup_suffix: String::from(".orig"),
            force: false,
            silent: false,
            verbose: false,
            dry_run: false,
            ignore_whitespace: false,
            remove_empty_files: false,
            no_backup_if_mismatch: false,
            fuzz: 2,
            target_file: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

fn parse_args() -> Options {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-i" | "--input" => {
                i += 1;
                if i >= args.len() {
                    die("option -i/--input requires an argument");
                }
                opts.patch_file = Some(args[i].clone());
            }
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    die("option -o/--output requires an argument");
                }
                opts.output_file = Some(args[i].clone());
            }
            "-p" => {
                i += 1;
                if i >= args.len() {
                    die("option -p requires an argument");
                }
                match args[i].parse::<usize>() {
                    Ok(n) => opts.strip = Some(n),
                    Err(_) => die(&format!("invalid strip count: {}", args[i])),
                }
            }
            "-d" | "--directory" => {
                i += 1;
                if i >= args.len() {
                    die("option -d/--directory requires an argument");
                }
                opts.directory = Some(args[i].clone());
            }
            "-F" | "--fuzz" => {
                i += 1;
                if i >= args.len() {
                    die("option -F/--fuzz requires an argument");
                }
                match args[i].parse::<usize>() {
                    Ok(n) => opts.fuzz = n,
                    Err(_) => die(&format!("invalid fuzz factor: {}", args[i])),
                }
            }
            "-R" | "--reverse" => opts.reverse = true,
            "-N" | "--forward" => opts.forward = true,
            "-b" | "--backup" => opts.backup = true,
            "-f" | "--force" => opts.force = true,
            "-s" | "--silent" | "--quiet" => opts.silent = true,
            "-v" | "--verbose" => opts.verbose = true,
            "--dry-run" => opts.dry_run = true,
            "-l" | "--ignore-whitespace" => opts.ignore_whitespace = true,
            "-E" | "--remove-empty-files" => opts.remove_empty_files = true,
            "--no-backup-if-mismatch" => opts.no_backup_if_mismatch = true,
            _ if arg.starts_with("--strip=") => {
                let val = &arg["--strip=".len()..];
                match val.parse::<usize>() {
                    Ok(n) => opts.strip = Some(n),
                    Err(_) => die(&format!("invalid strip count: {val}")),
                }
            }
            _ if arg.starts_with("--directory=") => {
                opts.directory = Some(arg["--directory=".len()..].to_string());
            }
            _ if arg.starts_with("--input=") => {
                opts.patch_file = Some(arg["--input=".len()..].to_string());
            }
            _ if arg.starts_with("--output=") => {
                opts.output_file = Some(arg["--output=".len()..].to_string());
            }
            _ if arg.starts_with("--backup-suffix=") => {
                opts.backup_suffix = arg["--backup-suffix=".len()..].to_string();
            }
            _ if arg.starts_with("--fuzz=") => {
                let val = &arg["--fuzz=".len()..];
                match val.parse::<usize>() {
                    Ok(n) => opts.fuzz = n,
                    Err(_) => die(&format!("invalid fuzz factor: {val}")),
                }
            }
            _ if arg.starts_with("-p") && arg.len() > 2 => {
                let val = &arg[2..];
                match val.parse::<usize>() {
                    Ok(n) => opts.strip = Some(n),
                    Err(_) => die(&format!("invalid strip count: {val}")),
                }
            }
            _ if arg.starts_with("-F") && arg.len() > 2 => {
                let val = &arg[2..];
                match val.parse::<usize>() {
                    Ok(n) => opts.fuzz = n,
                    Err(_) => die(&format!("invalid fuzz factor: {val}")),
                }
            }
            _ if arg.starts_with('-') && arg.len() > 1 && arg != "-" => {
                die(&format!("unknown option: {arg}"));
            }
            _ => {
                opts.target_file = Some(arg.clone());
            }
        }
        i += 1;
    }

    opts
}

fn die(msg: &str) -> ! {
    eprintln!("patch: {msg}");
    process::exit(2);
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Strip NUM leading path components from a path string.
fn strip_path(path: &str, num: usize) -> String {
    if num == 0 {
        return path.to_string();
    }
    let parts: Vec<&str> = path.splitn(num + 1, '/').collect();
    if parts.len() > num {
        parts[num].to_string()
    } else {
        // Fewer components than strip count: return the last component.
        path.rsplit('/').next().unwrap_or(path).to_string()
    }
}

/// Resolve the target file path for a file patch, accounting for -p, -d,
/// -R, /dev/null handling, and explicit target file override.
fn resolve_target_path(fp: &FilePatch, opts: &Options) -> PathBuf {
    let raw = if let Some(ref target) = opts.target_file {
        target.clone()
    } else if opts.reverse {
        // When reversing, new_path becomes the source and old_path the target.
        choose_best_path(&fp.new_path, &fp.old_path)
    } else {
        choose_best_path(&fp.old_path, &fp.new_path)
    };

    let stripped = match opts.strip {
        Some(n) => strip_path(&raw, n),
        None => raw,
    };

    let mut path = PathBuf::from(&stripped);
    if let Some(ref dir) = opts.directory
        && !path.is_absolute()
    {
        path = PathBuf::from(dir).join(path);
    }
    path
}

/// Pick the best path between primary and fallback.
/// Prefer primary unless it is /dev/null, in which case use fallback.
fn choose_best_path(primary: &str, fallback: &str) -> String {
    if primary == "/dev/null" {
        fallback.to_string()
    } else {
        primary.to_string()
    }
}

// ---------------------------------------------------------------------------
// Line-ending helpers
// ---------------------------------------------------------------------------

/// Strip trailing carriage returns for DOS compatibility.
fn strip_cr(s: &str) -> String {
    s.strip_suffix('\r').unwrap_or(s).to_string()
}

/// Normalize a line for whitespace-insensitive comparison.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// Compare two lines, optionally ignoring whitespace differences.
fn lines_match(a: &str, b: &str, ignore_ws: bool) -> bool {
    if ignore_ws {
        normalize_ws(a) == normalize_ws(b)
    } else {
        a == b
    }
}

// ---------------------------------------------------------------------------
// Format detection
// ---------------------------------------------------------------------------

/// Detect whether the input is unified, context, or normal diff format.
fn detect_format(input: &str) -> DiffFormat {
    for line in input.lines() {
        if line.starts_with("@@") || line.starts_with("--- ") {
            return DiffFormat::Unified;
        }
        if line.starts_with("*** ") || line.starts_with("***************") {
            return DiffFormat::Context;
        }
        // Normal diff: lines like "1,3c4,6" or "5a6" or "3d2"
        if is_normal_diff_command(line) {
            return DiffFormat::Normal;
        }
    }
    // Default to unified if we cannot tell.
    DiffFormat::Unified
}

/// Check if a line looks like a normal diff command (e.g. "1,3c4,6").
fn is_normal_diff_command(line: &str) -> bool {
    // Pattern: <range><cmd><range> where cmd is a/d/c and ranges are
    // digit[,digit].
    let bytes = line.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return false;
    }
    // Find the command letter.
    let cmd_pos = bytes
        .iter()
        .position(|&b| b == b'a' || b == b'd' || b == b'c');
    let cmd_pos = match cmd_pos {
        Some(p) => p,
        None => return false,
    };
    if cmd_pos == 0 {
        return false;
    }
    // Everything before cmd must be digits and commas.
    let before = &line[..cmd_pos];
    if !before.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return false;
    }
    // Everything after cmd must be digits and commas.
    let after = &line[cmd_pos + 1..];
    if after.is_empty() {
        return false;
    }
    after.chars().all(|c| c.is_ascii_digit() || c == ',')
}

// ---------------------------------------------------------------------------
// Unified diff parser
// ---------------------------------------------------------------------------

fn parse_unified(input: &str) -> Vec<FilePatch> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut i = 0;

    while i < raw_lines.len() {
        // Look for --- line followed by +++ line.
        if raw_lines[i].starts_with("--- ")
            && i + 1 < raw_lines.len()
            && raw_lines[i + 1].starts_with("+++ ")
        {
            let old_path = parse_file_header(raw_lines[i], "--- ");
            let new_path = parse_file_header(raw_lines[i + 1], "+++ ");
            i += 2;

            let mut hunks: Vec<Hunk> = Vec::new();

            while i < raw_lines.len() {
                if raw_lines[i].starts_with("@@ ") || raw_lines[i].starts_with("@@\t") {
                    if let Some((os, oc, ns, nc)) = parse_unified_hunk_header(raw_lines[i]) {
                        i += 1;
                        let mut hunk_lines: Vec<HunkLine> = Vec::new();

                        while i < raw_lines.len() {
                            let line = strip_cr(raw_lines[i]);
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
                                // Informational, skip.
                            } else {
                                // Bare context line (some patches omit the leading space).
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
                } else if raw_lines[i].starts_with("--- ") || raw_lines[i].starts_with("diff ") {
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

/// Parse `@@ -A,B +C,D @@` header.
fn parse_unified_hunk_header(line: &str) -> Option<(usize, usize, usize, usize)> {
    let line = strip_cr(line);
    let line = line.trim();
    if !line.starts_with("@@") {
        return None;
    }
    let after_at = &line[2..];
    let end_at = after_at.find("@@")?;
    let range_part = after_at[..end_at].trim();

    let mut parts = range_part.split_whitespace();
    let old_range = parts.next()?.strip_prefix('-')?;
    let new_range = parts.next()?.strip_prefix('+')?;

    let (os, oc) = parse_range(old_range)?;
    let (ns, nc) = parse_range(new_range)?;
    Some((os, oc, ns, nc))
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        Some((start_s.parse().ok()?, count_s.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

/// Extract the file path from a `--- ` or `+++ ` header line, stripping
/// any trailing timestamp.
fn parse_file_header(line: &str, prefix: &str) -> String {
    let rest = &line[prefix.len()..];
    let rest = strip_cr(rest);
    // Remove timestamp if present (tab-separated).
    if let Some(tab_pos) = rest.find('\t') {
        rest[..tab_pos].to_string()
    } else {
        rest.to_string()
    }
}

// ---------------------------------------------------------------------------
// Context diff parser
// ---------------------------------------------------------------------------

fn parse_context(input: &str) -> Vec<FilePatch> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut i = 0;

    while i < raw_lines.len() {
        // Context format: *** old_file  date
        //                 --- new_file  date
        if raw_lines[i].starts_with("*** ")
            && !raw_lines[i].starts_with("***************")
            && i + 1 < raw_lines.len()
            && raw_lines[i + 1].starts_with("--- ")
        {
            let old_path = parse_file_header(raw_lines[i], "*** ");
            let new_path = parse_file_header(raw_lines[i + 1], "--- ");
            i += 2;

            let mut hunks: Vec<Hunk> = Vec::new();

            while i < raw_lines.len() {
                let line = strip_cr(raw_lines[i]);
                if line.starts_with("***************") {
                    i += 1;

                    // Parse the "old" section: *** start,end ****
                    if i >= raw_lines.len() {
                        break;
                    }
                    let (old_start, old_end, old_section_lines) =
                        parse_context_section(&raw_lines, &mut i, "***");

                    // Parse the "new" section: --- start,end ----
                    let (new_start, new_end, new_section_lines) =
                        parse_context_section(&raw_lines, &mut i, "---");

                    let old_count = if old_end >= old_start {
                        old_end - old_start + 1
                    } else {
                        0
                    };
                    let new_count = if new_end >= new_start {
                        new_end - new_start + 1
                    } else {
                        0
                    };

                    let hunk_lines = merge_context_sections(&old_section_lines, &new_section_lines);

                    hunks.push(Hunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        lines: hunk_lines,
                    });
                } else if raw_lines[i].starts_with("*** ")
                    && !raw_lines[i].starts_with("***************")
                {
                    // Next file header.
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

/// Parse one section (old or new) of a context diff hunk.
/// `marker` is "***" for old or "---" for new.
/// Returns (start_line, end_line, tagged_lines).
fn parse_context_section(
    raw_lines: &[&str],
    i: &mut usize,
    marker: &str,
) -> (usize, usize, Vec<(char, String)>) {
    let mut start = 1usize;
    let mut end = 0usize;
    let mut section_lines: Vec<(char, String)> = Vec::new();

    if *i < raw_lines.len() {
        let header = strip_cr(raw_lines[*i]);
        let (s, e) = parse_context_range_header(&header, marker);
        start = s;
        end = e;
        *i += 1;
    }

    while *i < raw_lines.len() {
        let line = strip_cr(raw_lines[*i]);
        // Stop if we hit the other section header or a separator.
        if line.starts_with("***") || line.starts_with("--- ") {
            break;
        }

        if line.len() >= 2 {
            let prefix = line.chars().next().unwrap_or(' ');
            let rest = &line[2..];
            match prefix {
                '!' | '+' | '-' | ' ' => {
                    section_lines.push((prefix, rest.to_string()));
                }
                _ => {
                    // Treat as context.
                    section_lines.push((' ', line.to_string()));
                }
            }
        } else if line.is_empty() {
            section_lines.push((' ', String::new()));
        } else {
            section_lines.push((' ', line.to_string()));
        }
        *i += 1;
    }

    (start, end, section_lines)
}

/// Parse a context range header like `*** 1,5 ****` or `--- 3,7 ----`.
fn parse_context_range_header(line: &str, marker: &str) -> (usize, usize) {
    // Strip the leading marker and trailing stars/dashes.
    let trimmed = line.trim();
    let body = if marker == "***" {
        trimmed
            .strip_prefix("*** ")
            .unwrap_or(trimmed)
            .strip_suffix(" ****")
            .unwrap_or("")
    } else {
        trimmed
            .strip_prefix("--- ")
            .unwrap_or(trimmed)
            .strip_suffix(" ----")
            .unwrap_or("")
    };

    if let Some((s, e)) = body.split_once(',') {
        let start = s.trim().parse::<usize>().unwrap_or(1);
        let end = e.trim().parse::<usize>().unwrap_or(start);
        (start, end)
    } else {
        let n = body.trim().parse::<usize>().unwrap_or(1);
        (n, n)
    }
}

/// Merge old and new context-diff sections into a unified HunkLine sequence.
///
/// In context format, changed lines are marked '!' in both sections.
/// We need to pair them up: old '!' lines become Remove, new '!' lines
/// become Add, old ' ' lines become Context.
fn merge_context_sections(
    old_lines: &[(char, String)],
    new_lines: &[(char, String)],
) -> Vec<HunkLine> {
    let mut result = Vec::new();

    // Strategy: walk through old_lines. Context lines appear in both.
    // '!' lines in old are removes; '!' lines in new are adds.
    // '-' lines in old are removes; '+' lines in new are adds.
    let mut new_idx = 0;

    for (tag, content) in old_lines {
        match *tag {
            ' ' => {
                result.push(HunkLine::Context(content.clone()));
                // Skip the matching context line in new.
                if new_idx < new_lines.len() && new_lines[new_idx].0 == ' ' {
                    new_idx += 1;
                }
            }
            '!' => {
                result.push(HunkLine::Remove(content.clone()));
            }
            '-' => {
                result.push(HunkLine::Remove(content.clone()));
            }
            _ => {
                result.push(HunkLine::Context(content.clone()));
                if new_idx < new_lines.len() && new_lines[new_idx].0 == ' ' {
                    new_idx += 1;
                }
            }
        }
    }

    // Now emit the changed/added lines from new section.
    for (tag, content) in new_lines {
        match *tag {
            '!' | '+' => {
                result.push(HunkLine::Add(content.clone()));
            }
            ' ' => {
                // Already emitted as context from old side.
            }
            _ => {}
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Normal diff parser
// ---------------------------------------------------------------------------

fn parse_normal(input: &str) -> Vec<FilePatch> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut i = 0;

    while i < raw_lines.len() {
        let line = strip_cr(raw_lines[i]);
        if let Some(cmd) = parse_normal_command(&line) {
            i += 1;
            let mut hunk_lines: Vec<HunkLine> = Vec::new();

            // Collect lines for this command.
            while i < raw_lines.len() {
                let hline = strip_cr(raw_lines[i]);
                if hline == "---" {
                    // Separator between old and new in 'c' commands.
                    i += 1;
                    continue;
                }
                if let Some(rest) = hline.strip_prefix("< ") {
                    hunk_lines.push(HunkLine::Remove(rest.to_string()));
                } else if let Some(rest) = hline.strip_prefix("> ") {
                    hunk_lines.push(HunkLine::Add(rest.to_string()));
                } else if hline.starts_with('<') {
                    // "< " with empty content.
                    hunk_lines.push(HunkLine::Remove(
                        hline
                            .strip_prefix('<')
                            .unwrap_or("")
                            .trim_start()
                            .to_string(),
                    ));
                } else if hline.starts_with('>') {
                    hunk_lines.push(HunkLine::Add(
                        hline
                            .strip_prefix('>')
                            .unwrap_or("")
                            .trim_start()
                            .to_string(),
                    ));
                } else {
                    // Not part of this hunk.
                    break;
                }
                i += 1;
            }

            let old_count = match cmd.kind {
                NormalCmdKind::Add => 0,
                NormalCmdKind::Delete | NormalCmdKind::Change => cmd.old_end - cmd.old_start + 1,
            };
            let new_count = match cmd.kind {
                NormalCmdKind::Delete => 0,
                NormalCmdKind::Add | NormalCmdKind::Change => cmd.new_end - cmd.new_start + 1,
            };

            hunks.push(Hunk {
                old_start: cmd.old_start,
                old_count,
                new_start: cmd.new_start,
                new_count,
                lines: hunk_lines,
            });
        } else {
            i += 1;
        }
    }

    if hunks.is_empty() {
        return Vec::new();
    }

    // Normal diff doesn't name files, so we use placeholder paths.
    vec![FilePatch {
        old_path: String::new(),
        new_path: String::new(),
        hunks,
    }]
}

#[derive(Debug, Clone, Copy)]
enum NormalCmdKind {
    Add,
    Delete,
    Change,
}

struct NormalCmd {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
    kind: NormalCmdKind,
}

/// Parse a normal diff command line like "1,3c4,6", "5a6,8", "3d2".
fn parse_normal_command(line: &str) -> Option<NormalCmd> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let cmd_pos = line
        .bytes()
        .position(|b| b == b'a' || b == b'd' || b == b'c')?;

    if cmd_pos == 0 || cmd_pos >= line.len() - 1 {
        return None;
    }

    let kind = match line.as_bytes()[cmd_pos] {
        b'a' => NormalCmdKind::Add,
        b'd' => NormalCmdKind::Delete,
        b'c' => NormalCmdKind::Change,
        _ => return None,
    };

    let left = &line[..cmd_pos];
    let right = &line[cmd_pos + 1..];

    let (old_start, old_end) = parse_normal_range(left)?;
    let (new_start, new_end) = parse_normal_range(right)?;

    Some(NormalCmd {
        old_start,
        old_end,
        new_start,
        new_end,
        kind,
    })
}

fn parse_normal_range(s: &str) -> Option<(usize, usize)> {
    if let Some((a, b)) = s.split_once(',') {
        Some((a.parse().ok()?, b.parse().ok()?))
    } else {
        let n = s.parse().ok()?;
        Some((n, n))
    }
}

// ---------------------------------------------------------------------------
// Master parser dispatcher
// ---------------------------------------------------------------------------

fn parse_patch(input: &str) -> Vec<FilePatch> {
    let format = detect_format(input);
    match format {
        DiffFormat::Unified => parse_unified(input),
        DiffFormat::Context => parse_context(input),
        DiffFormat::Normal => parse_normal(input),
    }
}

// ---------------------------------------------------------------------------
// Hunk application
// ---------------------------------------------------------------------------

/// Try to match a hunk's context/remove lines at position `pos`.
fn try_hunk_at(lines: &[String], hunk: &Hunk, pos: usize, ignore_ws: bool) -> bool {
    let mut line_idx = pos;
    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(expected) | HunkLine::Remove(expected) => {
                if line_idx >= lines.len() {
                    return false;
                }
                if !lines_match(&lines[line_idx], expected, ignore_ws) {
                    return false;
                }
                line_idx += 1;
            }
            HunkLine::Add(_) => {}
        }
    }
    true
}

/// Apply a single hunk, searching with fuzz if the exact position fails.
/// Returns (new_lines, new_offset) on success, or None on failure.
fn apply_hunk(
    lines: &[String],
    hunk: &Hunk,
    offset: i64,
    fuzz: usize,
    ignore_ws: bool,
) -> Option<(Vec<String>, i64)> {
    // A pure insertion (old_count == 0, e.g. the normal-diff "1a2" command)
    // names in old_start the line to insert *after*, so 1-indexed line N maps
    // to insertion index N (old_start == 0 means prepend at the top). For
    // delete/change hunks old_start is the first affected line, whose 0-indexed
    // position is old_start - 1.
    let target_start = if hunk.old_count == 0 {
        (hunk.old_start as i64 + offset).max(0) as usize
    } else {
        (hunk.old_start as i64 + offset - 1).max(0) as usize
    };

    // Search with increasing fuzz distance.
    let max_search = fuzz.max(50);
    let mut best_pos = None;

    for dist in 0..=max_search {
        if dist == 0 {
            if try_hunk_at(lines, hunk, target_start, ignore_ws) {
                best_pos = Some(target_start);
                break;
            }
        } else {
            // Try below.
            let below = target_start + dist;
            if below <= lines.len() && try_hunk_at(lines, hunk, below, ignore_ws) {
                best_pos = Some(below);
                break;
            }
            // Try above.
            if target_start >= dist {
                let above = target_start - dist;
                if try_hunk_at(lines, hunk, above, ignore_ws) {
                    best_pos = Some(above);
                    break;
                }
            }
        }
    }

    let pos = best_pos?;

    // Build result.
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    result.extend_from_slice(&lines[..pos]);

    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(s) | HunkLine::Add(s) => result.push(s.clone()),
            HunkLine::Remove(_) => {}
        }
    }

    let old_consumed = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_) | HunkLine::Remove(_)))
        .count();

    if pos + old_consumed <= lines.len() {
        result.extend_from_slice(&lines[pos + old_consumed..]);
    }

    let new_offset = offset + hunk.new_count as i64 - hunk.old_count as i64;
    Some((result, new_offset))
}

// ---------------------------------------------------------------------------
// Reverse support
// ---------------------------------------------------------------------------

/// Reverse a hunk: swap Add <-> Remove, swap old/new metadata.
fn reverse_hunk(hunk: &Hunk) -> Hunk {
    let lines = hunk
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
        lines,
    }
}

/// Check if a patch appears to already be applied.
fn patch_already_applied(lines: &[String], hunks: &[Hunk], ignore_ws: bool) -> bool {
    // A patch is "already applied" if the reversed hunks match at their
    // expected positions.
    let reversed: Vec<Hunk> = hunks.iter().map(reverse_hunk).collect();
    let mut offset: i64 = 0;
    for h in &reversed {
        let target = (h.old_start as i64 + offset - 1).max(0) as usize;
        if !try_hunk_at(lines, h, target, ignore_ws) {
            return false;
        }
        offset += h.new_count as i64 - h.old_count as i64;
    }
    true
}

// ---------------------------------------------------------------------------
// Reject file writer
// ---------------------------------------------------------------------------

/// Write a reject file containing the failed hunks.
fn write_rejects(path: &Path, failed_hunks: &[&Hunk]) {
    if failed_hunks.is_empty() {
        return;
    }
    let mut reject_path = path.as_os_str().to_os_string();
    reject_path.push(".rej");
    let reject_path = PathBuf::from(reject_path);

    let mut content = String::new();
    for hunk in failed_hunks {
        content.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ));
        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    content.push(' ');
                    content.push_str(s);
                    content.push('\n');
                }
                HunkLine::Remove(s) => {
                    content.push('-');
                    content.push_str(s);
                    content.push('\n');
                }
                HunkLine::Add(s) => {
                    content.push('+');
                    content.push_str(s);
                    content.push('\n');
                }
            }
        }
    }

    // Best effort; if this fails we still report the error on stderr.
    if let Err(e) = fs::write(&reject_path, &content) {
        eprintln!(
            "patch: failed to write rejects to {}: {e}",
            reject_path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    let opts = parse_args();

    // Change directory if -d was given.
    if let Some(ref dir) = opts.directory
        && let Err(e) = env::set_current_dir(dir)
    {
        die(&format!("cannot change to directory {dir}: {e}"));
    }

    // Read patch input.
    let patch_input = if let Some(ref path) = opts.patch_file {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => die(&format!("{path}: {e}")),
        }
    } else {
        let mut buf = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut buf) {
            die(&format!("error reading stdin: {e}"));
        }
        buf
    };

    let file_patches = parse_patch(&patch_input);
    if file_patches.is_empty() {
        die("no valid patches found in input");
    }

    let mut any_failed = false;

    for fp in &file_patches {
        let file_path = resolve_target_path(fp, &opts);
        let file_path_str = file_path.display().to_string();

        if opts.verbose {
            eprintln!("patch: processing {file_path_str}");
        }

        if !opts.silent {
            if opts.dry_run {
                eprintln!("checking file {file_path_str}...");
            } else {
                eprintln!("patching file {file_path_str}");
            }
        }

        // Determine if the file is new.
        let is_new_file = (fp.old_path == "/dev/null" && !opts.reverse)
            || (fp.new_path == "/dev/null" && opts.reverse);

        // Read original file.
        let original = if is_new_file {
            String::new()
        } else {
            match fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("patch: can't open file {file_path_str}: {e}");
                    any_failed = true;
                    continue;
                }
            }
        };

        let mut lines: Vec<String> = original.lines().map(strip_cr).collect();

        // Prepare hunks, applying reverse if needed.
        let hunks: Vec<Hunk> = if opts.reverse {
            fp.hunks.iter().map(reverse_hunk).collect()
        } else {
            fp.hunks.clone()
        };

        // Forward mode: skip if already applied.
        if opts.forward && patch_already_applied(&lines, &hunks, opts.ignore_whitespace) {
            if !opts.silent {
                eprintln!("patch: {file_path_str} already patched, skipping");
            }
            continue;
        }

        let mut offset: i64 = 0;
        let mut hunks_applied = 0usize;
        let mut hunks_failed = 0usize;
        let mut failed_hunks: Vec<&Hunk> = Vec::new();

        for (idx, hunk) in hunks.iter().enumerate() {
            match apply_hunk(&lines, hunk, offset, opts.fuzz, opts.ignore_whitespace) {
                Some((new_lines, new_offset)) => {
                    lines = new_lines;
                    offset = new_offset;
                    hunks_applied += 1;
                    if opts.verbose {
                        eprintln!(
                            "Hunk #{} succeeded at offset {}",
                            idx + 1,
                            new_offset - (hunk.new_count as i64 - hunk.old_count as i64)
                        );
                    }
                }
                None => {
                    hunks_failed += 1;
                    failed_hunks.push(hunk);
                    if !opts.silent {
                        eprintln!("patch: Hunk #{} FAILED at line {}", idx + 1, hunk.old_start);
                    }
                }
            }
        }

        if hunks_failed > 0 {
            any_failed = true;
            if !opts.silent {
                eprintln!(
                    "patch: {hunks_failed} out of {} hunks FAILED for {file_path_str}",
                    hunks_applied + hunks_failed
                );
            }
        }

        // Write results.
        if !opts.dry_run && hunks_applied > 0 {
            let dest = if let Some(ref out) = opts.output_file {
                PathBuf::from(out)
            } else {
                file_path.clone()
            };

            // Backup if requested (and the file exists).
            let should_backup = opts.backup && !(opts.no_backup_if_mismatch && hunks_failed > 0);
            if should_backup && dest.exists() && opts.output_file.is_none() {
                let mut backup = dest.as_os_str().to_os_string();
                backup.push(&opts.backup_suffix);
                let backup = PathBuf::from(backup);
                if let Err(e) = fs::copy(&dest, &backup) {
                    eprintln!("patch: cannot create backup {}: {e}", backup.display());
                }
            }

            // Ensure parent directory exists.
            if let Some(parent) = dest.parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = fs::create_dir_all(parent);
            }

            // Reconstruct file content.
            let mut output = lines.join("\n");
            if original.ends_with('\n') || is_new_file {
                output.push('\n');
            }

            if let Err(e) = fs::write(&dest, &output) {
                eprintln!("patch: cannot write {}: {e}", dest.display());
                any_failed = true;
            }

            // Remove empty files if requested.
            if opts.remove_empty_files
                && dest.exists()
                && let Ok(meta) = fs::metadata(&dest)
                && meta.len() == 0
            {
                let _ = fs::remove_file(&dest);
                if opts.verbose {
                    eprintln!("patch: removed empty file {}", dest.display());
                }
            }
        }

        // Write rejects for failed hunks.
        if hunks_failed > 0 && !opts.dry_run {
            write_rejects(&file_path, &failed_hunks);
        }
    }

    if any_failed {
        process::exit(1);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Unified diff parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_unified_single_hunk() {
        let input = "\
--- a/foo.txt
+++ b/foo.txt
@@ -1,3 +1,4 @@
 line1
+inserted
 line2
 line3
";
        let patches = parse_unified(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].old_path, "a/foo.txt");
        assert_eq!(patches[0].new_path, "b/foo.txt");
        assert_eq!(patches[0].hunks.len(), 1);
        let h = &patches[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 4);
        assert_eq!(h.lines.len(), 4);
    }

    #[test]
    fn test_parse_unified_multiple_hunks() {
        let input = "\
--- a/foo.txt
+++ b/foo.txt
@@ -1,3 +1,3 @@
 line1
-old
+new
 line3
@@ -10,3 +10,4 @@
 line10
+added
 line11
 line12
";
        let patches = parse_unified(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].hunks.len(), 2);
        assert_eq!(patches[0].hunks[1].old_start, 10);
    }

    #[test]
    fn test_parse_unified_multiple_files() {
        let input = "\
--- a/one.txt
+++ b/one.txt
@@ -1,2 +1,3 @@
 a
+b
 c
--- a/two.txt
+++ b/two.txt
@@ -1 +1 @@
-old
+new
";
        let patches = parse_unified(input);
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].old_path, "a/one.txt");
        assert_eq!(patches[1].old_path, "a/two.txt");
    }

    #[test]
    fn test_parse_unified_new_file() {
        let input = "\
--- /dev/null
+++ b/newfile.txt
@@ -0,0 +1,2 @@
+hello
+world
";
        let patches = parse_unified(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].old_path, "/dev/null");
        assert_eq!(patches[0].hunks[0].lines.len(), 2);
    }

    #[test]
    fn test_parse_unified_delete_file() {
        let input = "\
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-goodbye
-world
";
        let patches = parse_unified(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].new_path, "/dev/null");
    }

    #[test]
    fn test_parse_unified_with_timestamps() {
        let input = "\
--- a/foo.txt\t2024-01-01 00:00:00.000000000 +0000
+++ b/foo.txt\t2024-01-02 00:00:00.000000000 +0000
@@ -1 +1 @@
-old
+new
";
        let patches = parse_unified(input);
        assert_eq!(patches[0].old_path, "a/foo.txt");
        assert_eq!(patches[0].new_path, "b/foo.txt");
    }

    #[test]
    fn test_parse_unified_no_newline_marker() {
        let input = "\
--- a/foo.txt
+++ b/foo.txt
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
        let patches = parse_unified(input);
        assert_eq!(patches[0].hunks[0].lines.len(), 2);
    }

    #[test]
    fn test_parse_hunk_header_basic() {
        assert_eq!(
            parse_unified_hunk_header("@@ -1,3 +1,4 @@"),
            Some((1, 3, 1, 4))
        );
    }

    #[test]
    fn test_parse_hunk_header_single_line() {
        assert_eq!(parse_unified_hunk_header("@@ -5 +5 @@"), Some((5, 1, 5, 1)));
    }

    #[test]
    fn test_parse_hunk_header_with_context() {
        assert_eq!(
            parse_unified_hunk_header("@@ -10,6 +10,8 @@ fn main() {"),
            Some((10, 6, 10, 8))
        );
    }

    // -----------------------------------------------------------------------
    // Context diff parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_context_single_change() {
        let input = "\
*** foo.txt.orig
--- foo.txt
***************
*** 1,3 ****
  line1
! old
  line3
--- 1,3 ----
  line1
! new
  line3
";
        let patches = parse_context(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].hunks.len(), 1);
        let h = &patches[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
    }

    #[test]
    fn test_parse_context_add_lines() {
        let input = "\
*** orig.txt
--- new.txt
***************
*** 1,2 ****
--- 1,4 ----
  line1
+ added1
+ added2
  line2
";
        let patches = parse_context(input);
        assert_eq!(patches.len(), 1);
        let h = &patches[0].hunks[0];
        let add_count = h
            .lines
            .iter()
            .filter(|l| matches!(l, HunkLine::Add(_)))
            .count();
        assert!(add_count >= 2);
    }

    #[test]
    fn test_parse_context_delete_lines() {
        let input = "\
*** orig.txt
--- new.txt
***************
*** 1,4 ****
  line1
- removed1
- removed2
  line2
--- 1,2 ----
  line1
  line2
";
        let patches = parse_context(input);
        assert_eq!(patches.len(), 1);
        let h = &patches[0].hunks[0];
        let rem_count = h
            .lines
            .iter()
            .filter(|l| matches!(l, HunkLine::Remove(_)))
            .count();
        assert!(rem_count >= 2);
    }

    // -----------------------------------------------------------------------
    // Normal diff parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_normal_change() {
        let input = "\
1,2c1,2
< old1
< old2
---
> new1
> new2
";
        let patches = parse_normal(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].hunks.len(), 1);
        let h = &patches[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_start + h.old_count - 1, 2);
        assert_eq!(h.new_start, 1);
    }

    #[test]
    fn test_parse_normal_add() {
        let input = "\
2a3
> inserted
";
        let patches = parse_normal(input);
        assert_eq!(patches.len(), 1);
        let h = &patches[0].hunks[0];
        assert_eq!(h.old_count, 0);
        assert_eq!(h.new_count, 1);
    }

    #[test]
    fn test_parse_normal_delete() {
        let input = "\
3d2
< removed
";
        let patches = parse_normal(input);
        assert_eq!(patches.len(), 1);
        let h = &patches[0].hunks[0];
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_count, 0);
    }

    #[test]
    fn test_parse_normal_multiple_commands() {
        let input = "\
1c1
< a
---
> b
3a4
> c
";
        let patches = parse_normal(input);
        assert_eq!(patches[0].hunks.len(), 2);
    }

    #[test]
    fn test_is_normal_diff_command() {
        assert!(is_normal_diff_command("1c1"));
        assert!(is_normal_diff_command("1,3c4,6"));
        assert!(is_normal_diff_command("5a6"));
        assert!(is_normal_diff_command("3d2"));
        assert!(!is_normal_diff_command("--- a/foo"));
        assert!(!is_normal_diff_command("+++ b/foo"));
        assert!(!is_normal_diff_command("normal text"));
        assert!(!is_normal_diff_command(""));
    }

    // -----------------------------------------------------------------------
    // Format detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_unified() {
        let input = "--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(detect_format(input), DiffFormat::Unified);
    }

    #[test]
    fn test_detect_context() {
        let input = "*** foo.orig\n--- foo\n***************\n*** 1,2 ****\n";
        assert_eq!(detect_format(input), DiffFormat::Context);
    }

    #[test]
    fn test_detect_normal() {
        let input = "1c1\n< old\n---\n> new\n";
        assert_eq!(detect_format(input), DiffFormat::Normal);
    }

    // -----------------------------------------------------------------------
    // Hunk application
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_simple_add() {
        let lines: Vec<String> = vec!["a", "b", "c"].into_iter().map(String::from).collect();
        let hunk = Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 2,
            lines: vec![
                HunkLine::Context("b".into()),
                HunkLine::Add("inserted".into()),
            ],
        };
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert_eq!(new_lines, vec!["a", "b", "inserted", "c"]);
    }

    #[test]
    fn test_apply_simple_remove() {
        let lines: Vec<String> = vec!["a", "b", "c"].into_iter().map(String::from).collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 2,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("b".into()),
                HunkLine::Context("c".into()),
            ],
        };
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert_eq!(new_lines, vec!["a", "c"]);
    }

    #[test]
    fn test_apply_change() {
        let lines: Vec<String> = vec!["a", "old", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("old".into()),
                HunkLine::Add("new".into()),
                HunkLine::Context("c".into()),
            ],
        };
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert_eq!(new_lines, vec!["a", "new", "c"]);
    }

    #[test]
    fn test_apply_hunk_mismatch() {
        let lines: Vec<String> = vec!["x", "y", "z"].into_iter().map(String::from).collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 2,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("b".into()),
                HunkLine::Add("c".into()),
            ],
        };
        // Nothing matches, so this should fail.
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Fuzz matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_fuzz_offset_positive() {
        // Hunk says line 2, but actual content is at line 4.
        let lines: Vec<String> = vec!["x", "y", "z", "target", "end"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunk = Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 2,
            lines: vec![
                HunkLine::Context("target".into()),
                HunkLine::Add("inserted".into()),
            ],
        };
        let result = apply_hunk(&lines, &hunk, 0, 10, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert!(new_lines.contains(&"inserted".to_string()));
    }

    #[test]
    fn test_fuzz_offset_negative() {
        // Hunk says line 5, but actual content is at line 1.
        let lines: Vec<String> = vec!["target", "end"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunk = Hunk {
            old_start: 5,
            old_count: 1,
            new_start: 5,
            new_count: 2,
            lines: vec![
                HunkLine::Context("target".into()),
                HunkLine::Add("inserted".into()),
            ],
        };
        let result = apply_hunk(&lines, &hunk, 0, 10, false);
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzz_zero_strict() {
        // With fuzz=0, still search nearby (max 50).
        let lines: Vec<String> = vec!["a", "b", "target", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
            lines: vec![
                HunkLine::Context("target".into()),
                HunkLine::Add("new".into()),
            ],
        };
        let result = apply_hunk(&lines, &hunk, 0, 0, false);
        assert!(result.is_some());
    }

    // -----------------------------------------------------------------------
    // Reverse
    // -----------------------------------------------------------------------

    #[test]
    fn test_reverse_hunk() {
        let hunk = Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("ctx".into()),
                HunkLine::Remove("old".into()),
                HunkLine::Add("new".into()),
            ],
        };
        let rev = reverse_hunk(&hunk);
        assert_eq!(rev.old_start, 1);
        assert_eq!(rev.old_count, 3);
        assert_eq!(rev.new_start, 1);
        assert_eq!(rev.new_count, 2);
        assert_eq!(rev.lines[0], HunkLine::Context("ctx".into()));
        assert_eq!(rev.lines[1], HunkLine::Add("old".into()));
        assert_eq!(rev.lines[2], HunkLine::Remove("new".into()));
    }

    #[test]
    fn test_reverse_apply() {
        // Apply a patch forward, then reverse it back.
        let original: Vec<String> = vec!["a", "old", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("old".into()),
                HunkLine::Add("new".into()),
                HunkLine::Context("c".into()),
            ],
        };

        let (patched, _) = apply_hunk(&original, &hunk, 0, 2, false).unwrap();
        assert_eq!(patched, vec!["a", "new", "c"]);

        let rev = reverse_hunk(&hunk);
        let (restored, _) = apply_hunk(&patched, &rev, 0, 2, false).unwrap();
        assert_eq!(restored, vec!["a", "old", "c"]);
    }

    // -----------------------------------------------------------------------
    // Strip path
    // -----------------------------------------------------------------------

    #[test]
    fn test_strip_path_0() {
        assert_eq!(strip_path("a/b/c.txt", 0), "a/b/c.txt");
    }

    #[test]
    fn test_strip_path_1() {
        assert_eq!(strip_path("a/b/c.txt", 1), "b/c.txt");
    }

    #[test]
    fn test_strip_path_2() {
        assert_eq!(strip_path("a/b/c.txt", 2), "c.txt");
    }

    #[test]
    fn test_strip_path_excess() {
        assert_eq!(strip_path("a/b", 5), "b");
    }

    #[test]
    fn test_strip_path_no_slash() {
        assert_eq!(strip_path("file.txt", 1), "file.txt");
    }

    // -----------------------------------------------------------------------
    // Whitespace matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_lines_match_exact() {
        assert!(lines_match("hello world", "hello world", false));
        assert!(!lines_match("hello world", "hello  world", false));
    }

    #[test]
    fn test_lines_match_ignore_ws() {
        assert!(lines_match("hello  world", "hello world", true));
        assert!(lines_match("  hello  ", "hello", true));
        assert!(lines_match("\thello\tworld", "hello world", true));
    }

    #[test]
    fn test_apply_with_whitespace_ignore() {
        let lines: Vec<String> = vec!["a", "  old  line  ", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("old line".into()),
                HunkLine::Add("new line".into()),
                HunkLine::Context("c".into()),
            ],
        };
        // Without ignore_whitespace: fails because "  old  line  " != "old line".
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_none());

        // With ignore_whitespace: succeeds.
        let result = apply_hunk(&lines, &hunk, 0, 2, true);
        assert!(result.is_some());
    }

    // -----------------------------------------------------------------------
    // CR stripping
    // -----------------------------------------------------------------------

    #[test]
    fn test_strip_cr() {
        assert_eq!(strip_cr("hello\r"), "hello");
        assert_eq!(strip_cr("hello"), "hello");
        assert_eq!(strip_cr(""), "");
        assert_eq!(strip_cr("\r"), "");
    }

    // -----------------------------------------------------------------------
    // Already-applied detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_patch_already_applied() {
        let lines: Vec<String> = vec!["a", "new", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("old".into()),
                HunkLine::Add("new".into()),
                HunkLine::Context("c".into()),
            ],
        }];
        assert!(patch_already_applied(&lines, &hunks, false));
    }

    #[test]
    fn test_patch_not_already_applied() {
        let lines: Vec<String> = vec!["a", "old", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("a".into()),
                HunkLine::Remove("old".into()),
                HunkLine::Add("new".into()),
                HunkLine::Context("c".into()),
            ],
        }];
        assert!(!patch_already_applied(&lines, &hunks, false));
    }

    // -----------------------------------------------------------------------
    // Multiple hunks with offset tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_multiple_hunks_offset() {
        let lines: Vec<String> = vec!["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();

        // First hunk: insert after line 1.
        let h1 = Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
            lines: vec![HunkLine::Context("a".into()), HunkLine::Add("x".into())],
        };
        let (lines2, offset) = apply_hunk(&lines, &h1, 0, 2, false).unwrap();
        assert_eq!(lines2, vec!["a", "x", "b", "c", "d", "e"]);
        assert_eq!(offset, 1);

        // Second hunk: modify at original line 4.
        let h2 = Hunk {
            old_start: 4,
            old_count: 1,
            new_start: 5,
            new_count: 1,
            lines: vec![HunkLine::Remove("d".into()), HunkLine::Add("D".into())],
        };
        let (lines3, _) = apply_hunk(&lines2, &h2, offset, 2, false).unwrap();
        assert_eq!(lines3, vec!["a", "x", "b", "c", "D", "e"]);
    }

    // -----------------------------------------------------------------------
    // End-to-end: parse + apply unified
    // -----------------------------------------------------------------------

    #[test]
    fn test_end_to_end_unified() {
        let original = "line1\nline2\nline3\n";
        let patch_text = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,4 @@
 line1
+inserted
 line2
 line3
";
        let patches = parse_unified(patch_text);
        assert_eq!(patches.len(), 1);

        let mut lines: Vec<String> = original.lines().map(String::from).collect();
        let mut offset = 0i64;
        for hunk in &patches[0].hunks {
            let (new_lines, new_offset) = apply_hunk(&lines, hunk, offset, 2, false).unwrap();
            lines = new_lines;
            offset = new_offset;
        }
        assert_eq!(lines, vec!["line1", "inserted", "line2", "line3"]);
    }

    // -----------------------------------------------------------------------
    // End-to-end: parse + apply normal diff
    // -----------------------------------------------------------------------

    #[test]
    fn test_end_to_end_normal_change() {
        let original = "a\nb\nc\n";
        let patch_text = "2c2\n< b\n---\n> B\n";

        let patches = parse_normal(patch_text);
        assert_eq!(patches.len(), 1);

        let mut lines: Vec<String> = original.lines().map(String::from).collect();
        let mut offset = 0i64;
        for hunk in &patches[0].hunks {
            let (new_lines, new_offset) = apply_hunk(&lines, hunk, offset, 2, false).unwrap();
            lines = new_lines;
            offset = new_offset;
        }
        assert_eq!(lines, vec!["a", "B", "c"]);
    }

    #[test]
    fn test_end_to_end_normal_add() {
        let original = "a\nc\n";
        let patch_text = "1a2\n> b\n";

        let patches = parse_normal(patch_text);
        let mut lines: Vec<String> = original.lines().map(String::from).collect();
        let mut offset = 0i64;
        for hunk in &patches[0].hunks {
            let (new_lines, new_offset) = apply_hunk(&lines, hunk, offset, 2, false).unwrap();
            lines = new_lines;
            offset = new_offset;
        }
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_end_to_end_normal_delete() {
        let original = "a\nb\nc\n";
        let patch_text = "2d1\n< b\n";

        let patches = parse_normal(patch_text);
        let mut lines: Vec<String> = original.lines().map(String::from).collect();
        let mut offset = 0i64;
        for hunk in &patches[0].hunks {
            let (new_lines, new_offset) = apply_hunk(&lines, hunk, offset, 2, false).unwrap();
            lines = new_lines;
            offset = new_offset;
        }
        assert_eq!(lines, vec!["a", "c"]);
    }

    // -----------------------------------------------------------------------
    // Normal diff command parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_normal_command_change() {
        let cmd = parse_normal_command("1,3c4,6").unwrap();
        assert_eq!(cmd.old_start, 1);
        assert_eq!(cmd.old_end, 3);
        assert_eq!(cmd.new_start, 4);
        assert_eq!(cmd.new_end, 6);
        assert!(matches!(cmd.kind, NormalCmdKind::Change));
    }

    #[test]
    fn test_parse_normal_command_add() {
        let cmd = parse_normal_command("5a6,8").unwrap();
        assert_eq!(cmd.old_start, 5);
        assert_eq!(cmd.new_start, 6);
        assert_eq!(cmd.new_end, 8);
        assert!(matches!(cmd.kind, NormalCmdKind::Add));
    }

    #[test]
    fn test_parse_normal_command_delete() {
        let cmd = parse_normal_command("3d2").unwrap();
        assert_eq!(cmd.old_start, 3);
        assert_eq!(cmd.new_start, 2);
        assert!(matches!(cmd.kind, NormalCmdKind::Delete));
    }

    #[test]
    fn test_parse_normal_command_invalid() {
        assert!(parse_normal_command("hello").is_none());
        assert!(parse_normal_command("").is_none());
    }

    // -----------------------------------------------------------------------
    // Resolve target path
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_path_with_strip() {
        let fp = FilePatch {
            old_path: "a/b/c.txt".into(),
            new_path: "a/b/c.txt".into(),
            hunks: vec![],
        };
        let opts = Options {
            strip: Some(1),
            ..Options::default()
        };
        let path = resolve_target_path(&fp, &opts);
        assert_eq!(path, PathBuf::from("b/c.txt"));
    }

    #[test]
    fn test_resolve_path_devnull() {
        let fp = FilePatch {
            old_path: "/dev/null".into(),
            new_path: "b/newfile.txt".into(),
            hunks: vec![],
        };
        let opts = Options::default();
        let path = resolve_target_path(&fp, &opts);
        assert_eq!(path, PathBuf::from("b/newfile.txt"));
    }

    #[test]
    fn test_resolve_path_reverse() {
        let fp = FilePatch {
            old_path: "a/old.txt".into(),
            new_path: "b/new.txt".into(),
            hunks: vec![],
        };
        let opts = Options {
            reverse: true,
            ..Options::default()
        };
        let path = resolve_target_path(&fp, &opts);
        assert_eq!(path, PathBuf::from("b/new.txt"));
    }

    // -----------------------------------------------------------------------
    // Normalize whitespace
    // -----------------------------------------------------------------------

    #[test]
    fn test_normalize_ws() {
        assert_eq!(normalize_ws("  hello   world  "), "hello world");
        assert_eq!(normalize_ws("a\tb"), "a b");
        assert_eq!(normalize_ws(""), "");
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_hunk() {
        let lines: Vec<String> = vec!["a", "b"].into_iter().map(String::from).collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 0,
            new_start: 1,
            new_count: 0,
            lines: vec![],
        };
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert_eq!(new_lines, vec!["a", "b"]);
    }

    #[test]
    fn test_apply_at_end_of_file() {
        let lines: Vec<String> = vec!["a", "b", "c"].into_iter().map(String::from).collect();
        let hunk = Hunk {
            old_start: 3,
            old_count: 1,
            new_start: 3,
            new_count: 2,
            lines: vec![HunkLine::Context("c".into()), HunkLine::Add("d".into())],
        };
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert_eq!(new_lines, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_apply_at_beginning() {
        let lines: Vec<String> = vec!["a", "b", "c"].into_iter().map(String::from).collect();
        let hunk = Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
            lines: vec![HunkLine::Add("z".into()), HunkLine::Context("a".into())],
        };
        let result = apply_hunk(&lines, &hunk, 0, 2, false);
        assert!(result.is_some());
        let (new_lines, _) = result.unwrap();
        assert_eq!(new_lines, vec!["z", "a", "b", "c"]);
    }

    #[test]
    fn test_dos_line_endings_in_patch() {
        let input = "--- a/foo.txt\r\n+++ b/foo.txt\r\n@@ -1 +1 @@\r\n-old\r\n+new\r\n";
        let patches = parse_unified(input);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].hunks[0].lines.len(), 2);
    }

    #[test]
    fn test_parse_range_single() {
        assert_eq!(parse_range("42"), Some((42, 1)));
    }

    #[test]
    fn test_parse_range_pair() {
        assert_eq!(parse_range("10,20"), Some((10, 20)));
    }

    #[test]
    fn test_choose_best_path() {
        assert_eq!(choose_best_path("/dev/null", "b/new.txt"), "b/new.txt");
        assert_eq!(choose_best_path("a/old.txt", "b/new.txt"), "a/old.txt");
    }
}
