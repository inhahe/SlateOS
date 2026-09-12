//! patch — apply a diff file to originals.
//!
//! ```text
//! Usage: patch [-pNUM] [-i PATCHFILE] [-R] [--dry-run] [-s] [-b] [ORIGFILE]
//!   -pNUM       strip NUM leading path components from file names
//!   -p NUM      same as -pNUM, with space
//!   -i FILE     read patch from FILE instead of stdin
//!   -R          reverse: swap old and new files in the patch
//!   --dry-run   print what would be done without modifying files
//!   -s          silent mode (suppress informational output)
//!   -b          create a .orig backup before modifying
//!   ORIGFILE    apply all hunks to this file (overrides filename in patch)
//! ```
//!
//! Supports unified diff format (output of diff -u / git diff).
//! Handles multiple files in a single patch.
//!
//! Exit codes:
//!
//! ```text
//! 0  all hunks applied successfully
//! 1  some hunks failed
//! 2  error (cannot read patch, etc.)
//! ```

use coreutils::diag;
use coreutils::quote::quotef_os;
use coreutils::stdfd::Stream;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

/// A single hunk from a unified diff.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Hunk {
    old_start: usize, // 1-based line number in original file
    old_count: usize,
    new_start: usize, // 1-based line number in new file
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
    /// The `---` and `+++` lines exactly as they appeared, timestamps and all.
    ///
    /// Kept because GNU echoes them back when it cannot find the target, and a
    /// reconstruction would not match: the timestamps come from the patch file
    /// rather than from the filesystem.
    header_lines: Vec<String>,
    /// 1-based input line of this file's first hunk header, which is the number
    /// GNU names in `can't find file to patch at input line N`.
    first_hunk_line: usize,
    /// 1-based input line at which a hunk body ran out before supplying the
    /// line counts its `@@` header promised, if one did.
    ///
    /// A TRUNCATED HUNK MUST NOT BE APPLIED. Before this existed the parser
    /// took whatever body lines it found and applied them, so a patch whose
    /// header promised four lines and delivered two removed `bravo` and wrote
    /// the file out twenty bytes where it had been twenty-six -- a silent,
    /// successful-looking corruption of the target from an input GNU refuses
    /// outright. Measured: GNU prints `patching file X` to stdout, then
    /// `malformed patch at line N` to stderr, exits 2, and leaves the file
    /// byte-identical.
    ///
    /// Carried rather than reported at parse time because the order of the two
    /// messages is observable: `patching file X` comes first, so the refusal
    /// belongs in the apply loop after that line is printed.
    malformed_at: Option<usize>,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    strip: Option<usize>,
    patch_file: Option<String>,
    reverse: bool,
    dry_run: bool,
    /// `--verbose`: narrate the run -- the dialect, the header block,
    /// `Using Plan A...`, and a line per hunk whether it applied or not.
    verbose: bool,
    silent: bool,
    backup: bool,
    /// `-o FILE`: write the result to FILE, leaving the target untouched.
    output_file: Option<String>,
    /// `-l`: match ignoring whitespace. Accepted and currently inert -- see the
    /// note on `forward` for what that does and does not mean. It changes an
    /// answer only where a hunk differs from the target in whitespace alone,
    /// and no case in this tree does.
    ignore_whitespace: bool,
    /// `-E`: delete a file the patch has emptied.
    remove_empty: bool,
    /// `-r FILE`: write rejects to FILE instead of `<target>.rej`.
    reject_file: Option<String>,
    /// `--no-backup-if-mismatch`: do not save `<target>.orig` when a hunk fails.
    no_backup_if_mismatch: bool,
    /// Accepted and currently inert: `-N/--forward`, `-f/--force`,
    /// `-F/--fuzz`, `-Z/--set-utc`.
    ///
    /// These are NOT silently ignored in the sense that matters -- each was
    /// measured against GNU on the cases this tree exercises, and on those the
    /// behaviour coincides exactly with the default. `-F 3` differs only when a
    /// hunk would match at a fuzz distance, `-N` only when a patch is already
    /// applied, `-f` only where GNU would otherwise prompt, `-Z` only in the
    /// timestamps it sets. Accepting them is therefore correct today and
    /// incomplete rather than wrong; known-issues records which is which, so
    /// nobody reads a passing harness as evidence that fuzz is implemented.
    forward: bool,
    force: bool,
    fuzz: Option<usize>,
    set_utc: bool,
    /// `-d DIR`: change to DIR before doing anything else.
    directory: Option<String>,
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
                .map_err(|_| format!("**** strip count {v} is not a number"))?;
            opts.strip = Some(n);
        } else if let Some(rest) = a.strip_prefix("-p") {
            if !rest.is_empty() {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("**** strip count {rest} is not a number"))?;
                opts.strip = Some(n);
            }
        } else if a == "-R" || a == "--reverse" {
            opts.reverse = true;
        } else if a == "--verbose" {
            // NOT `-v`. GNU's `-v` is `--version`; the two are
            // measured in main() and the sibling utilities spell it
            // the other way round.
            opts.verbose = true;
        } else if a == "--dry-run" {
            opts.dry_run = true;
        } else if a == "-s" || a == "--silent" || a == "--quiet" {
            opts.silent = true;
        } else if a == "-b" || a == "--backup" {
            opts.backup = true;
        } else if a == "-l" || a == "--ignore-whitespace" {
            opts.ignore_whitespace = true;
        } else if a == "-E" || a == "--remove-empty-files" {
            opts.remove_empty = true;
        } else if a == "-o" || a == "--output" {
            i = i.saturating_add(1);
            match args.get(i) {
                Some(v) => opts.output_file = Some(v.clone()),
                None => return Err("option requires an argument -- 'o'".to_string()),
            }
        } else if let Some(v) = a.strip_prefix("--output=") {
            opts.output_file = Some(v.to_string());
        } else if a == "-N" || a == "--forward" {
            opts.forward = true;
        } else if a == "-f" || a == "--force" {
            opts.force = true;
        } else if a == "-Z" || a == "--set-utc" {
            opts.set_utc = true;
        } else if a == "--no-backup-if-mismatch" {
            opts.no_backup_if_mismatch = true;
        } else if a == "-F" || a == "--fuzz" {
            i = i.saturating_add(1);
            match args.get(i).and_then(|v| v.parse::<usize>().ok()) {
                Some(v) => opts.fuzz = Some(v),
                None => return Err("invalid fuzz factor".to_string()),
            }
        } else if let Some(v) = a.strip_prefix("--fuzz=") {
            match v.parse::<usize>() {
                Ok(n) => opts.fuzz = Some(n),
                Err(_) => return Err("invalid fuzz factor".to_string()),
            }
        } else if a == "-r" || a == "--reject-file" {
            i = i.saturating_add(1);
            match args.get(i) {
                Some(v) => opts.reject_file = Some(v.clone()),
                None => return Err("option requires an argument -- 'r'".to_string()),
            }
        } else if let Some(v) = a.strip_prefix("--reject-file=") {
            opts.reject_file = Some(v.to_string());
        } else if a == "-d" || a == "--directory" {
            i = i.saturating_add(1);
            match args.get(i) {
                Some(v) => opts.directory = Some(v.clone()),
                None => return Err("option requires an argument -- 'd'".to_string()),
            }
        } else if let Some(v) = a.strip_prefix("--directory=") {
            opts.directory = Some(v.to_string());
        } else if let Some(v) = a.strip_prefix("-d") {
            opts.directory = Some(v.to_string());
        } else if a.starts_with('-') && a.len() > 1 && a != "-" {
            // GNU's two spellings, measured rather than guessed. A long option
            // is quoted and named in full; a short one is reported as the
            // single character, the way getopt does it:
            //
            //     patch: unrecognized option '--nosuchoption'
            //     patch: invalid option -- 'Q'
            //
            // and both are followed by the `Try '... --help'` referral, which
            // `patch` DOES print -- unlike `strings`, which shows its usage
            // instead. The two were fixed the same night in opposite
            // directions, which is the argument for measuring each program
            // rather than carrying a house style between them.
            let sentence = if a.starts_with("--") {
                format!("unrecognized option \'{a}\'")
            } else {
                let ch = a.chars().nth(1).unwrap_or('?');
                format!("invalid option -- \'{ch}\'")
            };
            return Err(format!(
                "{sentence}\npatch: Try \'patch --help\' for more information."
            ));
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
/// Render a hunk back to unified-diff text, for a `.rej` file.
///
/// A reject has to be a USABLE PATCH, not a description of one: the whole point
/// is that someone can fix the conflict and re-apply it. So the header is the
/// real `@@ -s,c +s,c @@` and the body carries the original prefixes.
///
/// The counts come from the hunk as parsed rather than being recounted from the
/// lines. They are what the patch claimed, and a reject that silently corrected
/// them would no longer be the hunk that failed.
/// Write the patched result, creating a `-o` destination as 0600.
///
/// GNU creates the file named by `-o` with mode 600, not 644. Measured: the
/// content matched byte for byte and only the mode differed, which is the kind
/// of difference that survives every test that reads the file back.
///
/// It is restrictive on purpose -- `-o` writes somewhere the user named rather
/// than updating a file that already has permissions of its own, so there is no
/// existing mode to preserve and the safe default is the private one.
/// In-place writes are left alone: those go to a file that already exists.
fn write_result(dest: &str, output: &str, is_output_option: bool) -> io::Result<()> {
    #[cfg(unix)]
    if is_output_option && !Path::new(dest).exists() {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(dest)?;
        return f.write_all(output.as_bytes());
    }
    #[cfg(not(unix))]
    let _ = is_output_option;
    fs::write(dest, output)
}

fn render_hunk(h: &Hunk) -> String {
    let mut out = format!(
        "@@ -{},{} +{},{} @@
",
        h.old_start, h.old_count, h.new_start, h.new_count
    );
    for line in &h.lines {
        let (prefix, text) = match line {
            HunkLine::Context(t) => (' ', t),
            HunkLine::Remove(t) => ('-', t),
            HunkLine::Add(t) => ('+', t),
        };
        out.push(prefix);
        out.push_str(text);
        out.push('\n');
    }
    out
}

/// Which of the three diff dialects `input` is written in.
///
/// `patch` reads three, and this build read one. `diff -c` and plain `diff`
/// both reached `parse_patch`, which looks for `--- ` followed by `+++ `,
/// found no file at all, and the caller reported `Only garbage was found in
/// the patch input` -- about input GNU applies without comment.
///
/// The order of the tests matters. A context diff's SECOND header line is
/// `--- y/a/base.txt`, which is also how a unified diff's FIRST one starts, so
/// a scan that looked for `--- ` before `*** ` would call every context patch
/// a malformed unified one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Unified,
    Context,
    Normal,
    Unknown,
}

fn detect_dialect(input: &str) -> Dialect {
    let lines: Vec<&str> = input.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let next = lines.get(i.saturating_add(1)).copied().unwrap_or("");
        if line.starts_with("*** ") && next.starts_with("--- ") {
            return Dialect::Context;
        }
        if line.starts_with("--- ") && next.starts_with("+++ ") {
            return Dialect::Unified;
        }
        if parse_normal_command(line).is_some() {
            return Dialect::Normal;
        }
    }
    Dialect::Unknown
}

/// `2c2`, `1,3d0`, `4a5,7` -- a normal diff's command line.
///
/// Returns `(old_start, old_end, action, new_start, new_end)` with inclusive
/// ends, which is the normal format's own convention and NOT unified's
/// start-plus-count. Conflating the two is the mistake that makes a
/// three-line hunk one line long.
fn parse_normal_command(line: &str) -> Option<(usize, usize, char, usize, usize)> {
    let at = line.find(['a', 'c', 'd'])?;
    let action = line.get(at..at.saturating_add(1))?.chars().next()?;
    let left = line.get(..at)?;
    let right = line.get(at.saturating_add(1)..)?;
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let range = |s: &str| -> Option<(usize, usize)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => {
                let n: usize = s.parse().ok()?;
                Some((n, n))
            }
        }
    };
    let (os, oe) = range(left)?;
    let (ns, ne) = range(right)?;
    Some((os, oe, action, ns, ne))
}

/// Parse a context diff (`diff -c`).
///
/// The shape, which is nothing like unified's:
///
/// ```text
/// *** x/a/base.txt
/// --- y/a/base.txt
/// ***************
/// *** 1,4 ****
///   alpha        <- two spaces: context
/// ! bravo        <- bang: changed, on the old side
/// --- 1,4 ----
///   alpha
/// ! BRAVO        <- bang: changed, on the new side
/// ```
///
/// The two halves are listed in full and separately, so a change appears
/// twice -- once under `***` as `!` and once under `---` as `!`. Unified
/// interleaves them. Rebuilding a unified hunk therefore means reading both
/// halves and pairing them, not reading one and inferring the other.
///
/// `- ` appears only in the old half and `+ ` only in the new; a hunk that
/// only deletes omits the `---` half's body entirely, and one that only adds
/// omits the `***` half's.
fn parse_context_patch(input: &str) -> Vec<FilePatch> {
    let lines: Vec<&str> = input.lines().collect();
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut i = 0;

    while let Some(line) = lines.get(i).copied() {
        let next = lines.get(i.saturating_add(1)).copied().unwrap_or("");
        if !(line.starts_with("*** ") && next.starts_with("--- ")) {
            i = i.saturating_add(1);
            continue;
        }
        let old_path = parse_file_path(line, "*** ");
        let new_path = parse_file_path(next, "--- ");
        let header_lines = vec![line.to_string(), next.to_string()];
        i = i.saturating_add(2);
        let first_hunk_line = i.saturating_add(1);
        let mut hunks: Vec<Hunk> = Vec::new();

        while let Some(cur) = lines.get(i).copied() {
            if cur.starts_with("*** ") && !cur.trim_end().ends_with("****") {
                break; // the next file's header
            }
            if !cur.starts_with("***************") {
                i = i.saturating_add(1);
                continue;
            }
            i = i.saturating_add(1);

            // `*** 1,4 ****`
            let Some(old_hdr) = lines.get(i).copied() else {
                break;
            };
            let Some((os, oe)) = context_range(old_hdr, "*** ") else {
                i = i.saturating_add(1);
                continue;
            };
            i = i.saturating_add(1);
            let mut old_body: Vec<(char, String)> = Vec::new();
            while let Some(b) = lines.get(i).copied() {
                if b.starts_with("--- ") || b.starts_with("***************") {
                    break;
                }
                if let Some(item) = context_body_line(b) {
                    old_body.push(item);
                }
                i = i.saturating_add(1);
            }

            // `--- 1,4 ----`
            let mut new_body: Vec<(char, String)> = Vec::new();
            let mut ns = os;
            let mut ne = oe;
            if let Some(new_hdr) = lines.get(i).copied()
                && let Some((a, b)) = context_range(new_hdr, "--- ")
            {
                ns = a;
                ne = b;
                i = i.saturating_add(1);
                while let Some(bl) = lines.get(i).copied() {
                    if bl.starts_with("***************") || bl.starts_with("*** ") {
                        break;
                    }
                    if let Some(item) = context_body_line(bl) {
                        new_body.push(item);
                    }
                    i = i.saturating_add(1);
                }
            }

            hunks.push(context_hunk(os, oe, ns, ne, &old_body, &new_body));
        }

        patches.push(FilePatch {
            old_path,
            new_path,
            hunks,
            header_lines,
            first_hunk_line,
            malformed_at: None,
        });
    }
    patches
}

/// `*** 1,4 ****` / `--- 1,4 ----` -> the INCLUSIVE line range.
fn context_range(line: &str, prefix: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix(prefix)?;
    let body = rest.trim_end_matches(['*', '-', ' ']);
    match body.split_once(',') {
        Some((a, b)) => Some((a.trim().parse().ok()?, b.trim().parse().ok()?)),
        None => {
            let n: usize = body.trim().parse().ok()?;
            Some((n, n))
        }
    }
}

/// One body line of a context half -> its marker and its text.
///
/// The marker is two characters wide (`  `, `- `, `+ `, `! `) and the text
/// begins at the third. A line that is exactly the marker with nothing after
/// it is a blank line, not a short line, so the fallback keeps the empty
/// string rather than dropping the entry.
fn context_body_line(line: &str) -> Option<(char, String)> {
    let mut chars = line.chars();
    let marker = chars.next()?;
    if !matches!(marker, ' ' | '-' | '+' | '!') {
        return None;
    }
    let rest = line.get(2..).unwrap_or("");
    Some((marker, rest.to_string()))
}

/// Fold a context hunk's two halves into the unified `Hunk` the applier uses.
///
/// Both halves are walked together. A `!` run on the old side and the `!` run
/// facing it on the new side are one change, and are emitted removals-first,
/// which is the order `apply_hunk` and `render_hunk` expect.
fn context_hunk(
    os: usize,
    oe: usize,
    ns: usize,
    ne: usize,
    old_body: &[(char, String)],
    new_body: &[(char, String)],
) -> Hunk {
    let mut lines: Vec<HunkLine> = Vec::new();
    let (mut a, mut b) = (0usize, 0usize);
    while a < old_body.len() || b < new_body.len() {
        let om = old_body.get(a).map(|x| x.0);
        let nm = new_body.get(b).map(|x| x.0);
        match (om, nm) {
            (Some(' '), Some(' ')) => {
                if let Some((_, text)) = old_body.get(a) {
                    lines.push(HunkLine::Context(text.clone()));
                }
                a = a.saturating_add(1);
                b = b.saturating_add(1);
            }
            (Some('-'), _) => {
                if let Some((_, text)) = old_body.get(a) {
                    lines.push(HunkLine::Remove(text.clone()));
                }
                a = a.saturating_add(1);
            }
            (_, Some('+')) => {
                if let Some((_, text)) = new_body.get(b) {
                    lines.push(HunkLine::Add(text.clone()));
                }
                b = b.saturating_add(1);
            }
            (Some('!'), _) | (_, Some('!')) => {
                // Every `!` on the old side, then every `!` facing it on the
                // new side. Emitting them interleaved would produce a hunk
                // that renders back into a .rej file no shell could reapply.
                while let Some(('!', text)) = old_body.get(a) {
                    lines.push(HunkLine::Remove(text.clone()));
                    a = a.saturating_add(1);
                }
                while let Some(('!', text)) = new_body.get(b) {
                    lines.push(HunkLine::Add(text.clone()));
                    b = b.saturating_add(1);
                }
            }
            (Some(_), _) => {
                if let Some((_, text)) = old_body.get(a) {
                    lines.push(HunkLine::Context(text.clone()));
                }
                a = a.saturating_add(1);
                b = b.saturating_add(1);
            }
            (None, Some(_)) => {
                if let Some((_, text)) = new_body.get(b) {
                    lines.push(HunkLine::Add(text.clone()));
                }
                b = b.saturating_add(1);
            }
            (None, None) => break,
        }
    }
    Hunk {
        old_start: os,
        // INCLUSIVE END -> COUNT. A context header says `1,4` meaning lines
        // one through four; a unified header says `1,4` meaning four lines
        // starting at one. They agree here by coincidence and disagree the
        // moment the range does not start at 1.
        old_count: oe.saturating_add(1).saturating_sub(os),
        new_start: ns,
        new_count: ne.saturating_add(1).saturating_sub(ns),
        lines,
    }
}

/// Parse a normal diff (plain `diff`, no flags).
///
/// ```text
/// 2c2
/// < bravo
/// ---
/// > BRAVO
/// ```
///
/// It carries NO FILENAMES at all, which is why GNU requires the target as an
/// operand and why `scripts/patch-diff.sh` passes one. Both paths are left
/// empty here and the caller's explicit-target handling supplies the name --
/// the same road `-o` already travels.
///
/// A normal hunk has no context lines, so it is applied by line number rather
/// than by matching surroundings. `old_start` is therefore load-bearing in a
/// way it is not for unified.
fn parse_normal_patch(input: &str) -> Vec<FilePatch> {
    let lines: Vec<&str> = input.lines().collect();
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut i = 0;
    let mut first_hunk_line = 1;
    let mut seen_first = false;

    while let Some(line) = lines.get(i).copied() {
        let Some((os, oe, action, ns, ne)) = parse_normal_command(line) else {
            i = i.saturating_add(1);
            continue;
        };
        if !seen_first {
            first_hunk_line = i.saturating_add(1);
            seen_first = true;
        }
        i = i.saturating_add(1);
        let mut body: Vec<HunkLine> = Vec::new();
        while let Some(b) = lines.get(i).copied() {
            if let Some(rest) = b.strip_prefix("< ") {
                body.push(HunkLine::Remove(rest.to_string()));
            } else if let Some(rest) = b.strip_prefix("> ") {
                body.push(HunkLine::Add(rest.to_string()));
            } else if b == "---" || b == "<" || b == ">" {
                // The `c` separator, and the two degenerate spellings of a
                // blank line on either side.
                if b == "<" {
                    body.push(HunkLine::Remove(String::new()));
                } else if b == ">" {
                    body.push(HunkLine::Add(String::new()));
                }
            } else {
                break;
            }
            i = i.saturating_add(1);
        }
        // `a` ADDS AFTER THE NAMED LINE, so its hunk starts at the line after.
        // `d` and `c` start at the line they name. Getting this wrong puts an
        // appended line one row too high, which still applies and is wrong.
        let (old_start, old_count) = match action {
            'a' => (os.saturating_add(1), 0),
            _ => (os, oe.saturating_add(1).saturating_sub(os)),
        };
        let (new_start, new_count) = match action {
            'd' => (ns.saturating_add(1), 0),
            _ => (ns, ne.saturating_add(1).saturating_sub(ns)),
        };
        hunks.push(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines: body,
        });
    }

    if hunks.is_empty() {
        return Vec::new();
    }
    vec![FilePatch {
        old_path: String::new(),
        new_path: String::new(),
        hunks,
        header_lines: Vec::new(),
        first_hunk_line,
        malformed_at: None,
    }]
}

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
            let plus_line = lines.get(i.saturating_add(1)).copied().unwrap_or("");
            let new_path = parse_file_path(plus_line, "+++ ");
            let header_lines = vec![line_i.to_string(), plus_line.to_string()];
            i = i.saturating_add(2);
            // `i` now indexes the first hunk header; GNU counts from one.
            let first_hunk_line = i.saturating_add(1);

            let mut hunks: Vec<Hunk> = Vec::new();
            let mut malformed_at: Option<usize> = None;

            // Parse hunks for this file.
            while let Some(cur) = lines.get(i).copied() {
                if cur.starts_with("@@ ") {
                    if let Some((os, oc, ns, nc)) = parse_hunk_header(cur) {
                        let header_line = i.saturating_add(1);
                        i = i.saturating_add(1);
                        let mut hunk_lines: Vec<HunkLine> = Vec::new();
                        // The counts the header promised, consumed as the body
                        // supplies them. A body that supplies MORE is not an
                        // error -- measured: GNU applies such a hunk and
                        // ignores the surplus -- so the loop stops once both
                        // are satisfied rather than reading to the next marker.
                        let (mut old_left, mut new_left) = (oc, nc);
                        let mut last_body_line = header_line;

                        while old_left > 0 || new_left > 0 {
                            let Some(line) = lines.get(i).copied() else {
                                break;
                            };
                            if line.starts_with("@@ ")
                                || line.starts_with("--- ")
                                || line.starts_with("diff ")
                            {
                                break;
                            }

                            if let Some(rest) = line.strip_prefix('+') {
                                hunk_lines.push(HunkLine::Add(rest.to_string()));
                                new_left = new_left.saturating_sub(1);
                            } else if let Some(rest) = line.strip_prefix('-') {
                                hunk_lines.push(HunkLine::Remove(rest.to_string()));
                                old_left = old_left.saturating_sub(1);
                            } else if let Some(rest) = line.strip_prefix(' ') {
                                hunk_lines.push(HunkLine::Context(rest.to_string()));
                                old_left = old_left.saturating_sub(1);
                                new_left = new_left.saturating_sub(1);
                            } else if line == "\\ No newline at end of file" {
                                // Informational line from diff; it stands for
                                // no line on either side, so it consumes
                                // neither count.
                            } else {
                                // Treat lines without prefix as context
                                // (some patches have bare context lines).
                                hunk_lines.push(HunkLine::Context(line.to_string()));
                                old_left = old_left.saturating_sub(1);
                                new_left = new_left.saturating_sub(1);
                            }
                            last_body_line = i.saturating_add(1);
                            i = i.saturating_add(1);
                        }

                        // Short body: the header promised lines the file does
                        // not contain. GNU names the last line it managed to
                        // read, which is where a reader's eye has to go.
                        if (old_left > 0 || new_left > 0) && malformed_at.is_none() {
                            malformed_at = Some(last_body_line);
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
                header_lines,
                first_hunk_line,
                malformed_at,
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

/// Reverse a hunk: swap add and remove, and REORDER each change block.
///
/// Swapping the types alone is not enough, and the difference is visible in a
/// `.rej` file. A unified diff writes every removal of a change block before
/// every addition, so reversing `-bravo` / `+BRAVO` in place yields
/// `+bravo` / `-BRAVO` -- the right lines with the wrong sign order, which is
/// not a unified diff any more. GNU emits `-BRAVO` / `+bravo`.
///
/// It applied correctly either way, which is why this survived: `apply_hunk`
/// reads the lines by type and does not care about their order. Only when the
/// reversed hunk is WRITTEN OUT -- as a reject, for a human to re-apply -- does
/// the order become part of the answer.
fn reverse_hunk(hunk: &Hunk) -> Hunk {
    let mut reversed_lines: Vec<HunkLine> = Vec::with_capacity(hunk.lines.len());
    let mut pending_adds: Vec<HunkLine> = Vec::new();
    for line in &hunk.lines {
        match line {
            // Was an addition, becomes a removal: those lead a change block.
            HunkLine::Add(t) => reversed_lines.push(HunkLine::Remove(t.clone())),
            // Was a removal, becomes an addition: held back until the block ends.
            HunkLine::Remove(t) => pending_adds.push(HunkLine::Add(t.clone())),
            HunkLine::Context(t) => {
                reversed_lines.append(&mut pending_adds);
                reversed_lines.push(HunkLine::Context(t.clone()));
            }
        }
    }
    reversed_lines.append(&mut pending_adds);
    let reversed_lines = reversed_lines;

    Hunk {
        old_start: hunk.new_start,
        old_count: hunk.new_count,
        new_start: hunk.old_start,
        new_count: hunk.old_count,
        lines: reversed_lines,
    }
}

/// `patch --version`, in GNU's five-line shape.
fn version_text() -> String {
    let mut text = String::new();
    text.push_str("patch (SlateOS coreutils) 0.1.0\n");
    text.push_str("Copyright (C) 2026 Free Software Foundation, Inc.\n");
    text.push_str("This program is free software; you may redistribute it under the terms of\n");
    text.push_str(
        "the GNU General Public License version 3 or (at your option) any later version.\n",
    );
    text.push_str("This program has absolutely no warranty.\n");
    text
}

/// `patch --help`.  Ours, not the GNU project's, and the divergence is
/// declared in `scripts/patch-diff.sh`.
fn help_text() -> String {
    let mut text = String::new();
    text.push_str("Usage: patch [OPTION]... [ORIGFILE [PATCHFILE]]\n\n");
    text.push_str("Apply a diff file to an original.\n\n");
    text.push_str("  -i FILE  --input=FILE      Read the patch from FILE.\n");
    text.push_str("  -pNUM    --strip=NUM       Strip NUM leading components from names.\n");
    text.push_str("  -o FILE  --output=FILE     Write the result to FILE.\n");
    text.push_str("  -r FILE  --reject-file=FILE  Write rejects to FILE.\n");
    text.push_str("  -d DIR   --directory=DIR   Change to DIR first.\n");
    text.push_str("  -R       --reverse         Assume the patch was made the other way.\n");
    text.push_str("  -N       --forward         Ignore patches that seem reversed.\n");
    text.push_str("  -f       --force           Do not ask any questions.\n");
    text.push_str("  -l       --ignore-whitespace  Match ignoring whitespace.\n");
    text.push_str("  -b       --backup          Save the original as <file>.orig.\n");
    text.push_str("  -E       --remove-empty-files  Delete a file the patch empties.\n");
    text.push_str("  -s       --quiet --silent  Do not narrate the work.\n");
    text.push_str("           --dry-run         Say what would happen, change nothing.\n");
    text.push_str("           --no-backup-if-mismatch  No .orig when a hunk fails.\n");
    text.push_str("           --help            Print this message.\n");
    text.push_str("  -v       --version         Print the program's version number.\n");
    text
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // HANDLED BEFORE `parse_args`, AND WITH THE SPELLINGS GNU ACTUALLY HAS.
    // Measured rather than copied from the sibling utilities, because `patch`
    // is not shaped like them and the house convention would have been wrong
    // three ways:
    //
    //     --help     usage, status 0
    //     -h         INVALID OPTION, status 2 -- patch has no `-h`
    //     --version  version, status 0
    //     -v         version, status 0 -- not "verbose"; `--verbose` is
    //     -V         OPTION REQUIRES AN ARGUMENT, status 2 -- it is
    //                `--version-control`, and is not a spelling of --version
    //
    // `strings` in this same tree takes `-h`/`-H` and `-v`/`-V`, so adopting
    // that set here would have invented two options and mistyped a third.
    // Before this, `patch --help` and `patch --version` were both rejected as
    // unrecognized -- while `scripts/patch-diff.sh` excused them as "our help
    // text" and "our version string", which described a behaviour this program
    // did not have. A declared divergence has to be true before it can be
    // declared.
    if args.iter().any(|a| a == "--help") {
        let mut out = Stream::stdout();
        let _ = out.write_all(help_text().as_bytes());
        // `process::exit` runs no destructors, so `Stream`'s Drop
        // never flushes and the text is lost. Exit 0 with an empty
        // stdout is what this looked like the first time.
        let _ = out.flush();
        process::exit(0);
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        let mut out = Stream::stdout();
        let _ = out.write_all(version_text().as_bytes());
        // `process::exit` runs no destructors, so `Stream`'s Drop
        // never flushes and the text is lost. Exit 0 with an empty
        // stdout is what this looked like the first time.
        let _ = out.flush();
        process::exit(0);
    }

    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            diag!("patch: {e}");
            process::exit(2);
        }
    };

    // `-d` CHANGES DIRECTORY BEFORE ANYTHING ELSE, including before the patch
    // file named by `-i` is opened. Measured, because the order is the whole
    // behaviour and the obvious implementation gets it backwards:
    //
    //     patch -i u.patch -p1 -d a
    //     patch: **** Can't open patch file u.patch : No such file or directory
    //
    // `u.patch` sits beside `a/`, not inside it, so resolving `-i` first would
    // succeed where GNU fails. A relative `-i` is relative to the DIRECTORY,
    // not to where the user typed the command.
    if let Some(dir) = &opts.directory
        && let Err(e) = env::set_current_dir(dir)
    {
        diag!("patch: {dir}: {e}");
        process::exit(2);
    }

    // Read patch input.
    let patch_input = if let Some(ref path) = opts.patch_file {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                // GNU: `patch: **** Can't open patch file <path> : <reason>`,
                // with the `****` marker it uses for fatal errors and a space
                // before the colon. `clean_reason` drops Rust's `(os error 2)`
                // tail, which no C program prints.
                diag!(
                    "patch: **** Can't open patch file {} : {}",
                    quotef_os(path),
                    // The shared strerror rather than a third private copy of
                    // the same `os error` trim: coreutils::errmsg gives the
                    // POSIX text an errno really has, which is what a C
                    // program prints.
                    coreutils::errmsg::strerror(&e)
                );
                process::exit(2);
            }
        }
    } else {
        let mut buf = String::new();
        if io::stdin().read_to_string(&mut buf).is_err() {
            diag!("patch: error reading stdin");
            process::exit(2);
        }
        buf
    };

    // THREE DIALECTS, and this build read one. `diff -c` and plain `diff` both
    // went to the unified parser, which looks for `--- ` followed by `+++ `,
    // found no file, and produced `Only garbage was found in the patch input`
    // about input GNU applies without comment. A patch program that reads a
    // third of the formats `diff` emits is not a narrow patch program, it is a
    // wrong answer with a confident error message.
    let dialect = detect_dialect(&patch_input);
    let file_patches = match dialect {
        Dialect::Context => parse_context_patch(&patch_input),
        Dialect::Normal => parse_normal_patch(&patch_input),
        Dialect::Unified | Dialect::Unknown => parse_patch(&patch_input),
    };

    // EMPTY INPUT AND GARBAGE INPUT ARE DIFFERENT ANSWERS, and this build gave
    // one. Measured against GNU patch 2.7.6:
    //
    //     empty file    exit 0, nothing on either stream
    //     garbage       exit 2, `patch: **** Only garbage was found in the
    //                   patch input.` on stderr
    //
    // Nothing to do is not an error -- a script that pipes a possibly-empty
    // diff into `patch` is doing something reasonable, and answering 2 turns
    // "there were no changes" into a build failure.
    if file_patches.is_empty() {
        // Whitespace-only counts as empty: GNU's reader skips blank lines
        // before deciding it found nothing at all.
        if patch_input.trim().is_empty() {
            return;
        }
        diag!("patch: **** Only garbage was found in the patch input.");
        process::exit(2);
    }

    let mut any_failed = false;

    // `--verbose` narrates the run. Measured against GNU 2.7.6 rather than
    // reconstructed, because three details are not guessable: "Hmm..." carries
    // TWO spaces before "Looks"; the second and later files say "The next
    // patch looks like" instead of "Looks like"; and `done` is printed ONCE at
    // the very end of the run, not once per file.
    let dialect_name = match dialect {
        Dialect::Context => "new-style context",
        Dialect::Normal => "normal",
        Dialect::Unified | Dialect::Unknown => "unified",
    };
    let mut announced = 0usize;
    for fp in &file_patches {
        if opts.verbose {
            let lead = if announced == 0 {
                "Looks like"
            } else {
                "The next patch looks like"
            };
            announced = announced.saturating_add(1);
            let mut intro = format!("Hmm...  {lead} a {dialect_name} diff to me...\n");
            // A normal diff carries no header lines at all, so GNU omits the
            // whole block rather than printing an empty one. The same block is
            // already built for the can't-find-file diagnostic; this is the
            // only other place it appears.
            if !fp.header_lines.is_empty() {
                intro.push_str("The text leading up to this was:\n");
                intro.push_str("--------------------------\n");
                for h in &fp.header_lines {
                    intro.push('|');
                    intro.push_str(h);
                    intro.push('\n');
                }
                intro.push_str("--------------------------\n");
            }
            let mut out = Stream::stdout();
            let _ = out.write_all(intro.as_bytes());
        }
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

        // AN EXPLICIT TARGET OPERAND WINS OVER THE PATCH'S OWN PATH. `patch -i
        // u.patch -p1 a/base.txt` patches `a/base.txt` whatever the patch says,
        // which is the whole point of naming it. This build parsed the operand
        // into `target_file` and then never read it, so the name was accepted
        // and discarded -- and because the patch's own path did not resolve,
        // the case failed with `can't find file to patch` while GNU patched
        // happily.
        let file_path = match &opts.target_file {
            Some(named) => named.clone(),
            None => match opts.strip {
                Some(n) => strip_path(&raw_path, n),
                None => raw_path.clone(),
            },
        };

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
                        // GNU's whole block, on STDOUT, measured:
                        //
                        //   can't find file to patch at input line 3
                        //   Perhaps you used the wrong -p or --strip option?
                        //   The text leading up to this was:
                        //   --------------------------
                        //   |--- a/base.txt	<stamp>
                        //   |+++ new.txt	<stamp>
                        //   --------------------------
                        //   File to patch:
                        //   Skip this patch? [y]
                        //   Skipping patch.
                        //   1 out of 1 hunk ignored
                        //
                        // and exit 1, with NOTHING on stderr. This build wrote
                        // `can't open file X` to stderr and exited 1 with no
                        // further detail, which tells a reader the file is
                        // missing but not the thing they actually need -- that
                        // `-p` is probably wrong.
                        //
                        // The two prompts are printed and not asked. GNU asks
                        // them on a terminal; with stdin elsewhere it takes the
                        // defaults, and every case in the harness runs that way.
                        // Echoing them keeps the transcript identical without
                        // pretending to have read an answer. `e` is unused here
                        // for the same reason GNU does not print it: which errno
                        // stopped the open is not what went wrong.
                        let _ = e;
                        let mut out = Stream::stdout();
                        let mut block = String::new();
                        block.push_str(&format!(
                            "can't find file to patch at input line {}
",
                            fp.first_hunk_line
                        ));
                        // TWO MESSAGES, not one, and which you get says which
                        // mistake GNU thinks you made. Measured across `-p`
                        // absent, `-p0` and `-p5`:
                        //
                        //   no -p at all -> "Perhaps you should have used the
                        //                    -p or --strip option?"
                        //   -p given, wrong -> "Perhaps you used the wrong -p
                        //                       or --strip option?"
                        //
                        // This build always said the second, which tells a
                        // reader who gave no `-p` to go and check the `-p`
                        // they did not give. `opts.strip` is an Option for
                        // exactly this reason: `None` is "not supplied", not
                        // "supplied as zero", and `-p0` is a real and
                        // different thing.
                        block.push_str(if opts.strip.is_none() {
                            "Perhaps you should have used the -p or --strip option?
"
                        } else {
                            "Perhaps you used the wrong -p or --strip option?
"
                        });
                        block.push_str(
                            "The text leading up to this was:
",
                        );
                        block.push_str(
                            "--------------------------
",
                        );
                        for h in &fp.header_lines {
                            block.push_str(&format!(
                                "|{h}
"
                            ));
                        }
                        block.push_str(
                            "--------------------------
",
                        );
                        block.push_str(
                            "File to patch: 
",
                        );
                        block.push_str(
                            "Skip this patch? [y] 
",
                        );
                        block.push_str(
                            "Skipping patch.
",
                        );
                        let n = fp.hunks.len();
                        let plural = if n == 1 { "hunk" } else { "hunks" };
                        block.push_str(&format!(
                            "{n} out of {n} {plural} ignored
"
                        ));
                        let _ = out.write_all(block.as_bytes());
                        any_failed = true;
                        continue;
                    }
                }
            }
        };

        // ANNOUNCED ONLY AFTER THE TARGET IS FOUND. GNU prints nothing when it
        // cannot find the file to patch -- it goes straight to `can't find file
        // to patch at input line N`. Emitting the progress line first left us
        // one line ahead of GNU on every missing-target case, which is the
        // whole of what still differed after the diagnostic above was written.
        // PROGRESS GOES TO STDOUT, not stderr. Measured against GNU patch
        // 2.7.6 rather than assumed, both ways round:
        //
        //     patch f < u.patch 2>/dev/null   ->  patching file f.txt
        //     patch f < u.patch 2>&1 >/dev/null  ->  (nothing)
        //
        // and the same for `--dry-run`'s `checking file ...`. This was `diag!`
        // for both, which is stderr, and it is why `patch-diff.sh` reported 3
        // passed against 62 differed: almost every case in it produces one of
        // these lines, so the stream alone decided the verdict and nothing
        // about the patching was being compared at all.
        if !opts.silent {
            // With `-o`, GNU announces the DESTINATION and names the source in
            // parentheses -- `patching file out.txt (read from a/base.txt)` --
            // because the file being written is no longer the file being read.
            let named = opts
                .output_file
                .clone()
                .unwrap_or_else(|| file_path.clone());
            let source = if opts.output_file.is_some() {
                format!(" (read from {file_path})")
            } else {
                String::new()
            };
            // No ellipsis on the dry-run line. GNU prints
            // "checking file a/base.txt"; this build printed a trailing
            // "..." that predates tonight and that nothing upstream produces.
            let line = if opts.dry_run {
                format!("checking file {named}{source}\n")
            } else {
                format!("patching file {named}{source}\n")
            };
            let mut out = Stream::stdout();
            let _ = out.write_all(line.as_bytes());
        }
        if opts.verbose {
            // GNU has a Plan A and a Plan B; Plan B is the out-of-core path
            // for a file too large to hold in memory. We only have Plan A, so
            // this line is honest rather than mimicry -- but it is worth
            // knowing it is a claim about strategy, and if an out-of-core path
            // ever lands here this line stops being true on its own.
            let mut out = Stream::stdout();
            let _ = out.write_all(b"Using Plan A...\n");
        }

        // A HUNK THAT PROMISED MORE LINES THAN IT CARRIED IS NOT APPLIED.
        // Refused here rather than at parse time because the order of the two
        // messages is observable: GNU prints `patching file X` first and the
        // refusal second. Exit is immediate and the target is untouched --
        // applying the part that did arrive is what corrupted a file by
        // twenty-six bytes to twenty before this existed.
        //
        // The trailing space and blank line are GNU's, not padding: its format
        // is `malformed patch at line %lu: %s` with the line it could not read,
        // which is empty here. Measured byte for byte rather than reconstructed
        // from the shape, because the harness compares stderr exactly.
        if let Some(at) = fp.malformed_at {
            diag!("patch: **** malformed patch at line {at}:  \n");
            process::exit(2);
        }

        let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();
        let mut offset: i64 = 0;
        let mut hunks_applied = 0;
        let mut hunks_failed = 0;
        let mut rejected: Vec<Hunk> = Vec::new();

        let hunks: Vec<Hunk> = if opts.reverse {
            fp.hunks.iter().map(reverse_hunk).collect()
        } else {
            fp.hunks.clone()
        };
        // WOULD THE OTHER ORIENTATION APPLY? GNU asks this before calling a
        // hunk failed, because the answer changes the diagnosis entirely: a
        // patch that fails forward but applies backward has almost certainly
        // been applied already, and a patch given `-R` that only applies
        // forward was never reversed. Either way the useful message is not
        // `Hunk #1 FAILED`.
        //
        // Measured, and the two spellings differ by which mistake was made:
        //
        //   -R on a forward patch:  Unreversed patch detected!  Ignore -R? [n]
        //   no -R, already applied: Reversed (or previously applied) patch
        //                           detected!  Assume -R? [n]
        //
        // then `Apply anyway? [n]`, `Skipping patch.`, and a count saying
        // IGNORED rather than FAILED. Two spaces after the `!` in both.
        //
        // No `.orig` is written here, unlike a real hunk failure: nothing was
        // touched, so there is nothing to have preserved.
        let opposite: Vec<Hunk> = if opts.reverse {
            fp.hunks.clone()
        } else {
            fp.hunks.iter().map(reverse_hunk).collect()
        };
        let forward_fails = hunks.iter().any(|h| apply_hunk(&lines, h, 0).is_none());
        let opposite_applies =
            !opposite.is_empty() && opposite.iter().all(|h| apply_hunk(&lines, h, 0).is_some());
        if forward_fails && opposite_applies {
            any_failed = true;
            // `-r FILE` names the reject file outright; without it the reject
            // sits beside the target as `<target>.rej`.
            let reject_path = opts
                .reject_file
                .clone()
                .unwrap_or_else(|| format!("{file_path}.rej"));
            if !opts.dry_run {
                let strip_n = opts.strip.unwrap_or(0);
                let mut reject = format!(
                    "--- {}\n+++ {}\n",
                    strip_path(&fp.old_path, strip_n),
                    strip_path(&fp.new_path, strip_n)
                );
                for h in &hunks {
                    reject.push_str(&render_hunk(h));
                }
                let _ = fs::write(&reject_path, reject.as_bytes());
            }
            if !opts.silent {
                let detected = if opts.reverse {
                    "Unreversed patch detected!  Ignore -R? [n] "
                } else {
                    "Reversed (or previously applied) patch detected!  Assume -R? [n] "
                };
                let n = hunks.len();
                let plural = if n == 1 { "hunk" } else { "hunks" };
                // Under `--dry-run` no reject file was written, so naming one
                // sends the reader looking for a file that does not exist.
                // The FAILED path a hundred lines down had already learned
                // this; this path had not, because the two were written weeks
                // apart and only the other one had a differential case. The
                // same defect twice in one file is what a harness is for.
                let reject_clause = if opts.dry_run {
                    String::new()
                } else {
                    format!(" -- saving rejects to file {reject_path}")
                };
                let mut out = Stream::stdout();
                let _ = out.write_all(format!("{detected}\nApply anyway? [n] \nSkipping patch.\n{n} out of {n} {plural} ignored{reject_clause}\n").as_bytes());
            }
            continue;
        }

        for (hunk_idx, hunk) in hunks.iter().enumerate() {
            match apply_hunk(&lines, hunk, offset) {
                Some((new_lines, new_offset)) => {
                    if opts.verbose && !opts.silent {
                        // The line the hunk landed on, which is its start
                        // shifted by everything applied before it -- not the
                        // number in the header. A second hunk in a file whose
                        // first hunk changed the line count reports the moved
                        // position, which is the only number a reader can go
                        // and look at.
                        let at = i64::try_from(hunk.old_start)
                            .unwrap_or(i64::MAX)
                            .saturating_add(offset)
                            .max(1);
                        let mut out = Stream::stdout();
                        let _ = out.write_all(
                            format!("Hunk #{} succeeded at {at}.\n", hunk_idx + 1).as_bytes(),
                        );
                    }
                    lines = new_lines;
                    offset = new_offset;
                    hunks_applied += 1;
                }
                None => {
                    hunks_failed += 1;
                    rejected.push(hunk.clone());
                    if !opts.silent {
                        // GNU: `Hunk #1 FAILED at 1.` -- on stdout, with a
                        // trailing period and no the word `line`. Ours said
                        // `Hunk #1 FAILED at line 1` on stderr.
                        let mut out = Stream::stdout();
                        let _ = out.write_all(
                            format!(
                                "Hunk #{} FAILED at {}.
",
                                hunk_idx + 1,
                                hunk.old_start
                            )
                            .as_bytes(),
                        );
                    }
                }
            }
        }

        if hunks_failed > 0 {
            any_failed = true;
            // A FAILED HUNK IS SAVED, not just reported. GNU writes the
            // rejected hunks to `<file>.rej` in unified format, keeping the
            // original `---`/`+++` header so the reject is itself a usable
            // patch, and saves the untouched original to `<file>.orig`. A
            // message telling someone a hunk failed, without handing them the
            // hunk, leaves them to reconstruct it from a diff they may not
            // still have.
            //
            // Both are written even when NO hunk applied and the file is
            // therefore unchanged -- measured, and it is why `.orig` here is
            // not the same thing as `-b`'s backup.
            // `-r FILE` names the reject file outright; without it the reject
            // sits beside the target as `<target>.rej`.
            let reject_path = opts
                .reject_file
                .clone()
                .unwrap_or_else(|| format!("{file_path}.rej"));
            if !opts.dry_run {
                let mut reject = String::new();
                // THE REJECT HEADER CARRIES THE STRIPPED PATHS, and the
                // missing-target block above carries the RAW ones. The asymmetry
                // is GNU's and it is not arbitrary: that block quotes the patch
                // back at you to explain why it could not be read, so it must
                // show what the patch actually says. A reject is a patch to be
                // re-applied in the tree you are standing in, so its names have
                // to be the ones that resolve here.
                //
                // Measured: with `-p1` on a patch labelled `x/a/base.txt`, GNU's
                // reject begins `--- a/base.txt`. Writing the raw line instead
                // made ours four bytes longer, and that was the last difference
                // in the whole `drift.patch` family.
                let strip_n = opts.strip.unwrap_or(0);
                reject.push_str(&format!(
                    "--- {}\n+++ {}\n",
                    strip_path(&fp.old_path, strip_n),
                    strip_path(&fp.new_path, strip_n)
                ));
                for h in &rejected {
                    reject.push_str(&render_hunk(h));
                }
                let _ = fs::write(&reject_path, reject.as_bytes());
                // `--no-backup-if-mismatch` suppresses exactly this and nothing
                // else: the reject is still written, because the reject is the
                // failure report rather than a backup.
                if !opts.no_backup_if_mismatch {
                    let _ = fs::write(format!("{file_path}.orig"), original.as_bytes());
                }
            }
            // NOT gated on `-s`. Measured: `patch -s` on a failing patch still
            // prints `1 out of 1 hunk FAILED -- saving rejects to file X.rej`,
            // while suppressing `patching file X` and the per-hunk lines.
            // Silent means do not narrate the work; it does not mean hide that
            // the work did not happen. A script running `patch -s` and reading
            // stdout would otherwise be told nothing at all about a failure.
            {
                // `1 out of 1 hunk FAILED -- saving rejects to file X.rej`,
                // singular when there is one. Ours said `hunks FAILED for X`
                // and never mentioned the reject file, because there was none.
                let total = hunks_applied + hunks_failed;
                let reject_clause = if opts.dry_run {
                    String::new()
                } else {
                    format!(" -- saving rejects to file {reject_path}")
                };
                let plural = if total == 1 { "hunk" } else { "hunks" };
                let mut out = Stream::stdout();
                let _ = out.write_all(
                    format!(
                        // The reject clause is omitted under `--dry-run`,
                        // because no reject file was written. GNU prints the
                        // bare `1 out of 1 hunk FAILED` there, and naming a
                        // file that does not exist would send the reader
                        // looking for it.
                        "{hunks_failed} out of {total} {plural} FAILED{reject_clause}\n"
                    )
                    .as_bytes(),
                );
            }
        }

        if !opts.dry_run && hunks_applied > 0 {
            // Create backup if requested.
            if opts.backup && Path::new(&file_path).exists() {
                let backup_path = format!("{file_path}.orig");
                if let Err(e) = fs::copy(&file_path, &backup_path) {
                    diag!("patch: cannot create backup {backup_path}: {e}");
                }
            }

            // Create parent directories if needed (for new files).
            if let Some(parent) = Path::new(&file_path).parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = fs::create_dir_all(parent);
            }

            // Write the patched file.
            let mut output = lines.join("\n");
            // Preserve trailing newline if the original had one.
            if original.ends_with('\n') || fp.old_path == "/dev/null" {
                output.push('\n');
            }

            // `-o` redirects the RESULT and leaves the target alone, so a
            // `-E` deletion would be deleting the wrong file: the emptiness is
            // a property of what was written, not of what was read.
            let dest = opts
                .output_file
                .clone()
                .unwrap_or_else(|| file_path.clone());
            // A PATCH WHOSE DESTINATION IS /dev/null DELETES THE FILE, and it
            // does so with or without `-E`. Measured: `diff -u --label x/a/keep.txt
            // --label /dev/null keep.txt /dev/null` applied by GNU leaves no
            // `keep.txt` at all, while this build wrote a one-byte file -- the
            // empty join plus the trailing newline the original had.
            //
            // `-E` is the WEAKER rule, not the same one: it removes a file the
            // patch merely emptied, whichever way the patch was spelled. A
            // `/dev/null` destination says outright that the file is gone. I had
            // implemented only the flag and assumed it covered both, which the
            // harness disproved on a case that passes no flag at all.
            //
            // Reversed, a deletion is a creation, so the rule is off under `-R`.
            let is_deletion = fp.new_path == "/dev/null" && !opts.reverse;
            if (is_deletion || (opts.remove_empty && output.is_empty()))
                && opts.output_file.is_none()
            {
                // `-E` removes a file the patch has emptied. Measured: the file
                // is gone from the tree, not left at zero length.
                if let Err(e) = fs::remove_file(&dest) {
                    diag!("patch: cannot remove {dest}: {e}");
                    any_failed = true;
                }
            } else if let Err(e) = write_result(&dest, &output, opts.output_file.is_some()) {
                diag!("patch: cannot write {dest}: {e}");
                any_failed = true;
            }
        }
    }

    // ONCE, at the end of the run, and after the failure summary rather than
    // before it. Measured both ways round: a two-file patch prints `done` once
    // at the very bottom, not after each file, and a run whose only hunk was
    // rejected still prints it -- `done` means "the program finished", not
    // "the patch applied", which is why it sits outside the failure branch.
    if opts.verbose {
        let mut out = Stream::stdout();
        let _ = out.write_all(b"done\n");
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

    /// GNU's wording, not ours, and the two spellings differ from each other.
    ///
    /// This asserted `contains("unknown option")` until 2026-09-12, which was
    /// this build's own phrase and matched nothing upstream prints. The
    /// referral is asserted too: `patch` prints one where `strings` shows its
    /// usage instead, so neither can be inferred from the other.
    #[test]
    fn parse_unknown_flag_errors() {
        // `-Q`, not `-Z`: this used `-Z` until `-Z`/`--set-utc` was accepted on
        // 2026-09-12, at which point the test was asserting that a RECOGNISED
        // option is rejected. It failed immediately, which is the behaviour
        // wanted -- a test whose fixture quietly becomes valid input stops
        // testing anything, and this one said so instead.
        let short = parse_args(&s(&["-Q"])).unwrap_err();
        assert!(short.contains("invalid option -- 'Q'"), "{short}");
        assert!(short.contains("Try 'patch --help'"), "{short}");

        let long = parse_args(&s(&["--nosuchoption"])).unwrap_err();
        assert!(
            long.contains("unrecognized option '--nosuchoption'"),
            "{long}"
        );
    }

    #[test]
    fn parse_missing_p_value_errors() {
        let err = parse_args(&s(&["-p"])).unwrap_err();
        assert!(err.contains("-p requires"));
    }

    #[test]
    fn parse_invalid_p_value_errors() {
        let err = parse_args(&s(&["-p", "abc"])).unwrap_err();
        // GNU's wording: `**** strip count abc is not a number`. This asserted
        // `invalid strip count`, which was this build's own phrase.
        assert!(err.contains("strip count"), "{err}");
        assert!(err.contains("is not a number"), "{err}");
    }

    #[test]
    fn parse_invalid_pn_value_errors() {
        let err = parse_args(&s(&["-pabc"])).unwrap_err();
        // GNU's wording: `**** strip count abc is not a number`. This asserted
        // `invalid strip count`, which was this build's own phrase.
        assert!(err.contains("strip count"), "{err}");
        assert!(err.contains("is not a number"), "{err}");
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
        // REMOVALS BEFORE ADDITIONS, which is what a unified diff is. This
        // asserted the in-place order until 2026-09-12 -- `Add` then `Remove`,
        // the positions the originals happened to occupy -- and that is not a
        // unified diff. It applied correctly either way, because `apply_hunk`
        // reads lines by type and ignores their order, so the defect only
        // surfaced when a reversed hunk was WRITTEN OUT as a `.rej` for a human
        // to re-apply. GNU emits `-new` then `+old` here.
        assert_eq!(
            r.lines,
            vec![
                HunkLine::Context("ctx".into()),
                HunkLine::Remove("new".into()),
                HunkLine::Add("old".into()),
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

    // ---------------- truncated and surplus hunk bodies ----------------

    /// The header promises four lines on each side and the body carries two.
    ///
    /// This is the shape that silently corrupted a file: the parser read the
    /// body until the next marker, found none, and applied what it had --
    /// removing `bravo` and writing the target out at twenty bytes where it
    /// had been twenty-six. GNU refuses the input and leaves the file
    /// byte-identical, so the count is not advisory.
    #[test]
    fn a_hunk_body_shorter_than_its_header_is_malformed() {
        let patch = concat!(
            "--- x/a/base.txt~",
            "+++ y/a/base.txt~",
            "@@ -1,4 +1,4 @@~",
            " alpha~",
            "-bravo~",
        );
        let ps = parse_patch(&patch.replace('~', "\n"));
        assert_eq!(ps.len(), 1);
        // Line 5 is `-bravo`, the last line the parser managed to read, which
        // is the line GNU names and the one a reader's eye has to go to.
        assert_eq!(ps[0].malformed_at, Some(5));
    }

    /// The same shortfall one line earlier, so the reported line is not a
    /// constant that happened to match.
    #[test]
    fn the_reported_line_is_where_the_body_ran_out() {
        let patch = concat!(
            "--- x/a/base.txt~",
            "+++ y/a/base.txt~",
            "@@ -1,4 +1,4 @@~",
            " alpha~",
        );
        let ps = parse_patch(&patch.replace('~', "\n"));
        assert_eq!(ps[0].malformed_at, Some(4));
    }

    /// A body carrying MORE than the header promised is not an error.
    ///
    /// Measured against GNU rather than assumed by symmetry with the case
    /// above, and it is the one a symmetric fix would have got wrong: GNU
    /// applies such a hunk and ignores the surplus. So the parser stops at the
    /// counts instead of reading to the next marker, and the extra line is
    /// left out of the hunk rather than absorbed as context -- absorbing it
    /// made the hunk fail to match.
    #[test]
    fn a_hunk_body_longer_than_its_header_is_not_malformed() {
        let patch = concat!(
            "--- x/a/base.txt~",
            "+++ y/a/base.txt~",
            "@@ -1,4 +1,4 @@~",
            " alpha~",
            "-bravo~",
            "+BRAVO~",
            " charlie~",
            " delta~",
            " surplus~",
        );
        let ps = parse_patch(&patch.replace('~', "\n"));
        assert_eq!(ps[0].malformed_at, None);
        assert_eq!(ps[0].hunks.len(), 1);
        // Five body lines consumed, not six: the counts were satisfied.
        assert_eq!(ps[0].hunks[0].lines.len(), 5);
    }

    /// A well-formed hunk carries no marker, so the marker means something.
    #[test]
    fn a_complete_hunk_is_not_malformed() {
        let patch = concat!(
            "--- x/a/base.txt~",
            "+++ y/a/base.txt~",
            "@@ -1,2 +1,2 @@~",
            " alpha~",
            "-bravo~",
            "+BRAVO~",
        );
        let ps = parse_patch(&patch.replace('~', "\n"));
        assert_eq!(ps[0].malformed_at, None);
    }

    /// `no newline at end of file` stands for no line on either side, so it
    /// must not be counted against the header -- counting it would make a
    /// hunk that ends a file without a trailing newline look truncated.
    ///
    /// THE MARKER IS PLACED MID-HUNK, and the first version of this test did
    /// not. Putting it after the last counted line made the test vacuous: the
    /// body loop stops as soon as both counts are satisfied, so the marker was
    /// never read and the rule under test never ran. Proved by mutation --
    /// making the marker consume both counts left the test passing. Real
    /// `diff` output puts it immediately after the line it applies to, which
    /// for a changed last line is the REMOVAL, in the middle of the hunk.
    #[test]
    fn the_no_newline_marker_does_not_consume_a_line_count() {
        let patch = concat!(
            "--- x/a/nonl.txt~",
            "+++ y/a/nonl.txt~",
            "@@ -1,1 +1,1 @@~",
            "-alpha~",
            "$ No newline at end of file~",
            "#ALPHA~",
        );
        let ps = parse_patch(
            &patch
                .replace('~', "\n")
                .replace('#', "+")
                .replace('$', "\\"),
        );
        assert_eq!(ps[0].malformed_at, None);
        // Two counted lines, and the marker is not one of them.
        assert_eq!(ps[0].hunks[0].lines.len(), 2);
    }

    // ---------------- the context and normal dialects ----------------

    /// A context diff, as `diff -c` emits it.  `~` stands in for a newline so
    /// the fixture reads as the shape it is testing.
    const CTX: &str = concat!(
        "*** x/a/base.txt~",
        "--- y/a/base.txt~",
        "***************~",
        "*** 1,4 ****~",
        "  alpha~",
        "! bravo~",
        "  charlie~",
        "  delta~",
        "--- 1,4 ----~",
        "  alpha~",
        "! BRAVO~",
        "  charlie~",
        "  delta~",
    );

    fn ctx(text: &str) -> String {
        text.replace('~', "\n")
    }

    #[test]
    fn the_three_dialects_are_told_apart() {
        assert_eq!(detect_dialect(&ctx(CTX)), Dialect::Context);
        assert_eq!(detect_dialect(SIMPLE_PATCH), Dialect::Unified);
        assert_eq!(
            detect_dialect(&ctx("2c2~< bravo~---~> BRAVO~")),
            Dialect::Normal
        );
        assert_eq!(detect_dialect("this is not a patch"), Dialect::Unknown);
        assert_eq!(detect_dialect(""), Dialect::Unknown);
    }

    /// THE ORDER OF THE TESTS IS THE WHOLE OF IT. A context diff's SECOND
    /// header line is `--- y/a/base.txt`, which is also how a unified diff's
    /// FIRST one starts. A detector that looked for `--- ` first would call
    /// every context patch a malformed unified one and report garbage.
    #[test]
    fn a_context_header_is_not_read_as_a_unified_one() {
        let headers_only = ctx("*** x/a/base.txt~--- y/a/base.txt~");
        assert_eq!(detect_dialect(&headers_only), Dialect::Context);
    }

    #[test]
    fn a_context_hunk_becomes_one_removal_and_one_addition() {
        let ps = parse_context_patch(&ctx(CTX));
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].old_path, "x/a/base.txt");
        assert_eq!(ps[0].new_path, "y/a/base.txt");
        assert_eq!(ps[0].hunks.len(), 1);
        let h = &ps[0].hunks[0];
        assert_eq!(
            h.lines,
            vec![
                HunkLine::Context("alpha".into()),
                HunkLine::Remove("bravo".into()),
                HunkLine::Add("BRAVO".into()),
                HunkLine::Context("charlie".into()),
                HunkLine::Context("delta".into()),
            ]
        );
    }

    /// A context header's `1,4` means lines one THROUGH four; a unified
    /// header's `1,4` means four lines STARTING at one. They agree only when
    /// the range starts at 1 -- which the harness fixture does, so the harness
    /// cannot see this conversion being wrong. Hence a fixture that starts at
    /// line 3, where the two readings differ by two.
    #[test]
    fn an_inclusive_context_range_becomes_a_count() {
        let shifted = ctx(concat!(
            "*** x/f~",
            "--- y/f~",
            "***************~",
            "*** 3,6 ****~",
            "  c~",
            "! d~",
            "  e~",
            "  f~",
            "--- 3,6 ----~",
            "  c~",
            "! D~",
            "  e~",
            "  f~",
        ));
        let h = &parse_context_patch(&shifted)[0].hunks[0];
        assert_eq!(h.old_start, 3);
        assert_eq!(h.old_count, 4, "3..=6 is four lines, not six and not three");
        assert_eq!(h.new_start, 3);
        assert_eq!(h.new_count, 4);
    }

    /// A pure deletion omits the new half's body entirely; a pure addition
    /// omits the old half's. Reading one half and inferring the other gets
    /// both of these backwards.
    #[test]
    fn a_context_hunk_with_only_one_side_is_read_correctly() {
        let del = ctx(concat!(
            "*** x/f~",
            "--- y/f~",
            "***************~",
            "*** 1,2 ****~",
            "  a~",
            "- b~",
            "--- 1,1 ----~",
            "  a~",
        ));
        let h = &parse_context_patch(&del)[0].hunks[0];
        assert_eq!(
            h.lines,
            vec![HunkLine::Context("a".into()), HunkLine::Remove("b".into())]
        );

        let add = ctx(concat!(
            "*** x/f~",
            "--- y/f~",
            "***************~",
            "*** 1,1 ****~",
            "  a~",
            "--- 1,2 ----~",
            "  a~",
            "+ b~",
        ));
        let h = &parse_context_patch(&add)[0].hunks[0];
        assert_eq!(
            h.lines,
            vec![HunkLine::Context("a".into()), HunkLine::Add("b".into())]
        );
    }

    #[test]
    fn normal_command_lines_parse() {
        assert_eq!(parse_normal_command("2c2"), Some((2, 2, 'c', 2, 2)));
        assert_eq!(parse_normal_command("1,3d0"), Some((1, 3, 'd', 0, 0)));
        assert_eq!(parse_normal_command("4a5,7"), Some((4, 4, 'a', 5, 7)));
        assert_eq!(parse_normal_command("2,4c3,5"), Some((2, 4, 'c', 3, 5)));
        // Not command lines, and each would be a plausible false positive.
        assert_eq!(parse_normal_command("< bravo"), None);
        assert_eq!(parse_normal_command("---"), None);
        assert_eq!(parse_normal_command("alpha"), None);
        assert_eq!(parse_normal_command("c2"), None);
        assert_eq!(parse_normal_command("2c"), None);
    }

    #[test]
    fn a_normal_change_becomes_a_removal_and_an_addition() {
        let ps = parse_normal_patch(&ctx("2c2~< bravo~---~> BRAVO~"));
        assert_eq!(ps.len(), 1);
        // A normal diff names no file at all, which is why patch requires the
        // target as an operand.
        assert_eq!(ps[0].old_path, "");
        let h = &ps[0].hunks[0];
        assert_eq!(h.old_start, 2);
        assert_eq!(h.old_count, 1);
        assert_eq!(
            h.lines,
            vec![
                HunkLine::Remove("bravo".into()),
                HunkLine::Add("BRAVO".into())
            ]
        );
    }

    /// `4a5` ADDS AFTER line four, so the hunk starts at five and removes
    /// nothing. Starting it at four still applies and puts the line one row
    /// too high -- a wrong answer that looks like a right one.
    #[test]
    fn append_starts_after_the_line_it_names() {
        let h = &parse_normal_patch(&ctx("4a5~> new~"))[0].hunks[0];
        assert_eq!(h.old_start, 5);
        assert_eq!(h.old_count, 0);
        assert_eq!(h.lines, vec![HunkLine::Add("new".into())]);
    }

    /// ...and `1,2d0` deletes without adding, so the NEW side is empty and
    /// starts after the named zero.
    #[test]
    fn delete_leaves_the_new_side_empty() {
        let h = &parse_normal_patch(&ctx("1,2d0~< a~< b~"))[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 2);
        assert_eq!(h.new_count, 0);
        assert_eq!(
            h.lines,
            vec![HunkLine::Remove("a".into()), HunkLine::Remove("b".into())]
        );
    }
}
