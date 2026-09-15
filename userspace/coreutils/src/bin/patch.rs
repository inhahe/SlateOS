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
use coreutils::quote;
use coreutils::quote::quotef_os;
use coreutils::stdfd::Stream;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

/// The path `diff` writes for "this side of the patch has no file" --
/// a creation names it as the old path, a deletion as the new one.
///
/// A constant because it is compared against `FilePatch`'s byte paths in
/// six places, and `b"/dev/null"` is an array rather than a slice, so each
/// comparison would otherwise need its own `.as_slice()`.
const DEV_NULL: &[u8] = b"/dev/null";

/// The byte-slice operations `str` gives away for free.
///
/// `patch` carries file content, and content is bytes: a source file with one
/// Latin-1 byte in a comment is a file GNU patches and this build used to
/// refuse outright (`known-issues.md` ->
/// B-PATCH-REFUSES-EVERY-FILE-THAT-IS-NOT-VALID-UTF-8). The structure a patch
/// is made of -- `@@`, `---`, `+++`, and the `+`/`-`/space in column one -- is
/// ASCII and stays comparable as bytes; only the line *contents* have to
/// survive untouched, and they only survive if nothing decodes them.
///
/// These are kept private rather than pushed into `coreutils::quote` or a
/// crate of their own because `patch` is the first bin to need them. `diff` is
/// the expected second; that is the point to extract, not this one.
mod bytes {
    /// The first index at which `needle` occurs in `hay`.
    ///
    /// `str::find`'s counterpart. Naive, and deliberately so: the needles here
    /// are two or three ASCII bytes (`@@`, `--- `) against a single line.
    pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        hay.windows(needle.len()).position(|w| w == needle)
    }

    /// Split on ASCII whitespace, dropping empty runs -- `str::split_whitespace`.
    pub fn split_whitespace(s: &[u8]) -> impl Iterator<Item = &[u8]> {
        s.split(|b: &u8| b.is_ascii_whitespace())
            .filter(|part| !part.is_empty())
    }

    /// Split at the first `sep` -- `str::split_once`.
    pub fn split_once(s: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
        let at = s.iter().position(|&b| b == sep)?;
        Some((s.get(..at)?, s.get(at.saturating_add(1)..)?))
    }

    /// Parse a decimal count.
    ///
    /// Via `from_utf8` rather than a hand-rolled digit loop: a line number in a
    /// patch is ASCII digits by definition, so the check is free, and anything
    /// that is not digits must fail rather than be salvaged. This is the one
    /// place decoding is right -- it decodes a *number*, never a name or a line.
    pub fn parse_usize(s: &[u8]) -> Option<usize> {
        std::str::from_utf8(s).ok()?.parse().ok()
    }

    /// `s` with any trailing byte that appears in `set` removed --
    /// `str::trim_end_matches` over a set of ASCII characters.
    pub fn trim_end_matches<'a>(s: &'a [u8], set: &[u8]) -> &'a [u8] {
        let mut out = s;
        while let Some((last, head)) = out.split_last() {
            if set.contains(last) {
                out = head;
            } else {
                break;
            }
        }
        out
    }

    /// The lines of `text`, without their terminators -- `str::lines`.
    ///
    /// Matches `str::lines` on the case that matters here: a trailing newline
    /// does NOT produce a final empty line, because a patch body's last line is
    /// terminated like every other one and an extra empty line would be an
    /// extra hunk line.
    pub fn lines(text: &[u8]) -> Vec<&[u8]> {
        let mut out: Vec<&[u8]> = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            match rest.iter().position(|&b| b == b'\n') {
                Some(at) => {
                    if let (Some(line), Some(tail)) =
                        (rest.get(..at), rest.get(at.saturating_add(1)..))
                    {
                        out.push(strip_cr(line));
                        rest = tail;
                    } else {
                        break;
                    }
                }
                None => {
                    out.push(strip_cr(rest));
                    break;
                }
            }
        }
        out
    }

    /// A line without its CR, for a patch written with DOS terminators.
    ///
    /// `str::lines` does this and the byte version must too, or every context
    /// line of a CRLF patch fails to match a target read with LF.
    fn strip_cr(line: &[u8]) -> &[u8] {
        match line.split_last() {
            Some((b'\r', head)) => head,
            _ => line,
        }
    }
}

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
    Context(Vec<u8>),
    Remove(Vec<u8>),
    Add(Vec<u8>),
}

/// A patch for a single file, consisting of one or more hunks.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct FilePatch {
    old_path: Vec<u8>,
    new_path: Vec<u8>,
    hunks: Vec<Hunk>,
    /// The `---` and `+++` lines exactly as they appeared, timestamps and all.
    ///
    /// Kept because GNU echoes them back when it cannot find the target, and a
    /// reconstruction would not match: the timestamps come from the patch file
    /// rather than from the filesystem.
    header_lines: Vec<Vec<u8>>,
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
    patch_file: Option<OsString>,
    reverse: bool,
    dry_run: bool,
    /// `--verbose`: narrate the run -- the dialect, the header block,
    /// `Using Plan A...`, and a line per hunk whether it applied or not.
    verbose: bool,
    silent: bool,
    backup: bool,
    /// `-o FILE`: write the result to FILE, leaving the target untouched.
    output_file: Option<OsString>,
    /// `-l`: match context ignoring whitespace differences. IMPLEMENTED.
    ///
    /// It was accepted and inert for a long time, on the argument that it
    /// changes an answer only where a hunk differs from the target in
    /// whitespace alone, and no case in this tree does. That argument does not
    /// survive the way it was found: `--help` advertises "Match ignoring
    /// whitespace." with no hint that nothing reads the flag, so a user is
    /// TOLD the option works. An inert option is defensible only while nothing
    /// promises otherwise -- and of the four places this one was written down,
    /// the only one a user reads was the one that lied.
    ///
    /// See `loose_eq` for the matching rule, which is measured, not guessed.
    ignore_whitespace: bool,
    /// `-E`: delete a file the patch has emptied.
    remove_empty: bool,
    /// `-r FILE`: write rejects to FILE instead of `<target>.rej`.
    reject_file: Option<OsString>,
    /// `--no-backup-if-mismatch`: do not save `<target>.orig` when a hunk fails.
    no_backup_if_mismatch: bool,
    /// `-N/--forward`: do not ask about a reversed or already-applied patch,
    /// just skip it. IMPLEMENTED.
    ///
    /// The difference is the two prompt lines and nothing else -- measured.
    /// Without it GNU writes the detection, `Assume -R? [n] `, `Apply anyway?
    /// [n] ` and `Skipping patch.` on four lines; with it, the detection and
    /// `Skipping patch.` share ONE line and the questions are not asked. The
    /// reject file, the `ignored` count and the exit status are identical
    /// either way, which is why an inert `-N` agreed with GNU on every case
    /// this tree had.
    forward: bool,
    /// Accepted and currently inert: `-f/--force`, `-Z/--set-utc`.
    ///
    /// Each was measured against GNU on the cases this tree exercises, and on
    /// those the behaviour coincides with the default: `-f` differs only where
    /// GNU would otherwise prompt, `-Z` only in the timestamps it sets.
    ///
    /// That argument is weaker than it reads, and `-l` is why. An option the
    /// `--help` text ADVERTISES is one the user has been told works, so
    /// "coincides on the cases we exercise" stops being a defence the moment
    /// the case is a user's rather than a harness's. These two are next.
    force: bool,
    fuzz: Option<usize>,
    set_utc: bool,
    /// `-d DIR`: change to DIR before doing anything else.
    directory: Option<OsString>,
    target_file: Option<OsString>,
}

/// Parse patch's argv into an `Options`.  Recognised flags:
///   -i FILE / -p NUM / -pNUM / -R / --reverse / --dry-run / -s /
///   --silent / --quiet / -b / --backup.
/// Anything else not starting with `-` (and not the bare string "-")
/// is the target file.  Unknown flags return an error.
fn parse_args(args: &[OsString]) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut i: usize = 0;

    while let Some(arg) = args.get(i) {
        // Decoded for MATCHING ONLY, and `""` when the argument is not
        // Unicode. Every option `patch` accepts is ASCII, so a non-Unicode
        // argument cannot be one; `""` matches nothing here and falls through
        // to the operand arm, which keeps the original `OsString`. Option
        // VALUES are never decoded -- they are the paths, and the whole point
        // of taking `OsString` is that they reach the syscall unaltered.
        let a = arg.to_str().unwrap_or("");
        let raw = quote::os_bytes(arg);
        if a == "-i" || a == "--input" {
            i = i.saturating_add(1);
            let v = args
                .get(i)
                .ok_or_else(|| "option -i requires an argument".to_string())?;
            opts.patch_file = Some(v.clone());
        } else if let Some(v) = raw.strip_prefix(b"--input=") {
            // `--input` is the one thing the retired `userspace/patch` crate
            // accepted that this did not. It is carried over rather than lost:
            // deleting the worse half of a duplicate pair means the better half
            // has to end up with everything, and a survey column reading
            // "1 only in the standalone" is a list of one to go and check, not
            // a rounding error.
            opts.patch_file = Some(quote::os_from_bytes(v));
        } else if a == "-p" {
            i = i.saturating_add(1);
            let v = args
                .get(i)
                .ok_or_else(|| "option -p requires an argument".to_string())?;
            let n: usize = v.to_str().and_then(|s| s.parse().ok()).ok_or_else(|| {
                // `quotef`, NOT `quote_glibc`. Measured: GNU prints
                // `**** strip count abc is not a number` with no quotes at
                // all, where it DOES quote an option name
                // (`invalid option -- 'Q'`). `quotef` leaves plain text alone
                // and escapes only a value that could forge a line, so it
                // matches GNU on every input GNU is defined on. The harness
                // caught this: `quote_glibc` here printed `'abc'`.
                format!(
                    "**** strip count {} is not a number",
                    quote::quotef(&quote::os_bytes(v))
                )
            })?;
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
        } else if let Some(v) = raw.strip_prefix(b"--output=") {
            opts.output_file = Some(quote::os_from_bytes(v));
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
            match args
                .get(i)
                .and_then(|v| v.to_str())
                .and_then(|v| v.parse::<usize>().ok())
            {
                Some(v) => opts.fuzz = Some(v),
                None => return Err("invalid fuzz factor".to_string()),
            }
        } else if let Some(v) = a.strip_prefix("--fuzz=") {
            match v.parse::<usize>() {
                Ok(n) => opts.fuzz = Some(n),
                Err(_) => return Err("invalid fuzz factor".to_string()),
            }
        } else if let Some(v) = a.strip_prefix("-F")
            && !v.is_empty()
        {
            // `-F1` with the number GLUED ON. Measured: GNU takes all four of
            // `-F1`, `-F 1`, `--fuzz=1` and `--fuzz 1`; this build took the
            // last three, so `patch -F1` answered `invalid option -- 'F'`.
            // Must sit after the bare `-F` arm above, which claims the
            // separate-word spelling.
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
        } else if let Some(v) = raw.strip_prefix(b"--reject-file=") {
            opts.reject_file = Some(quote::os_from_bytes(v));
        } else if a == "-d" || a == "--directory" {
            i = i.saturating_add(1);
            match args.get(i) {
                Some(v) => opts.directory = Some(v.clone()),
                None => return Err("option requires an argument -- 'd'".to_string()),
            }
        } else if let Some(v) = raw.strip_prefix(b"--directory=") {
            opts.directory = Some(quote::os_from_bytes(v));
        } else if let Some(v) = raw.strip_prefix(b"-d") {
            opts.directory = Some(quote::os_from_bytes(v));
        } else if raw.starts_with(b"-") && raw.len() > 1 && &*raw != b"-".as_slice() {
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
            // The RAW bytes, and `quote_glibc` rather than a bare `'{a}'`, for
            // two reasons the decoded form gets wrong. An argument that is not
            // Unicode decodes to the empty string, so `-<0x80>` would be
            // reported as an empty option -- or, before this line existed,
            // would not be reported at all, because it never reached this arm
            // and was silently taken as the target file. And a name holding a
            // newline, printed raw, lets whoever chose it forge a second line
            // of our error stream. Both are what `coreutils::getopt` already
            // does, down to the octal spelling of a non-ASCII byte: glibc
            // answers `invalid option -- '\\303'`, never the character
            // that byte might begin.
            let sentence = if raw.starts_with(b"--") {
                format!("unrecognized option {}", quote::quote_glibc(&raw))
            } else {
                let flag = raw.get(1).copied().unwrap_or(b'?');
                format!("invalid option -- {}", quote::quote_glibc(&[flag]))
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
fn strip_path(path: &[u8], num: usize) -> Vec<u8> {
    if num == 0 {
        return path.to_vec();
    }
    let parts: Vec<&[u8]> = path.splitn(num.saturating_add(1), |&b| b == b'/').collect();
    if let Some(tail) = parts.get(num) {
        (*tail).to_vec()
    } else {
        // If there aren't enough components, return the basename.
        path.rsplit(|&b| b == b'/').next().unwrap_or(path).to_vec()
    }
}

/// Parse the @@ -old_start,old_count +new_start,new_count @@ line.
fn parse_hunk_header(line: &[u8]) -> Option<(usize, usize, usize, usize)> {
    // Format: @@ -A,B +C,D @@ optional text
    let line = line.trim_ascii();
    let after_at = line.strip_prefix(b"@@")?;
    let end_at = bytes::find(after_at, b"@@")?;
    let range_part = after_at.get(..end_at)?.trim_ascii();

    let mut parts = bytes::split_whitespace(range_part);
    let old_range = parts.next()?;
    let new_range = parts.next()?;

    let old_range = old_range.strip_prefix(b"-")?;
    let new_range = new_range.strip_prefix(b"+")?;

    let (old_start, old_count) = parse_range(old_range)?;
    let (new_start, new_count) = parse_range(new_range)?;

    Some((old_start, old_count, new_start, new_count))
}

fn parse_range(s: &[u8]) -> Option<(usize, usize)> {
    if let Some((start_s, count_s)) = bytes::split_once(s, b',') {
        Some((bytes::parse_usize(start_s)?, bytes::parse_usize(count_s)?))
    } else {
        // Single number means count=1.
        Some((bytes::parse_usize(s)?, 1))
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
fn write_result(dest: &OsStr, output: &[u8], is_output_option: bool) -> io::Result<()> {
    #[cfg(unix)]
    if is_output_option && !Path::new(dest).exists() {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(dest)?;
        return f.write_all(output);
    }
    #[cfg(not(unix))]
    let _ = is_output_option;
    fs::write(dest, output)
}

fn render_hunk(h: &Hunk) -> Vec<u8> {
    let mut out: Vec<u8> = format!(
        "@@ -{},{} +{},{} @@\n",
        h.old_start, h.old_count, h.new_start, h.new_count
    )
    .into_bytes();
    for line in &h.lines {
        let (prefix, text) = match line {
            HunkLine::Context(t) => (b' ', t),
            HunkLine::Remove(t) => (b'-', t),
            HunkLine::Add(t) => (b'+', t),
        };
        out.push(prefix);
        out.extend_from_slice(text);
        out.push(b'\n');
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

fn detect_dialect(input: &[u8]) -> Dialect {
    let lines: Vec<&[u8]> = bytes::lines(input);
    for (i, line) in lines.iter().enumerate() {
        let next = lines.get(i.saturating_add(1)).copied().unwrap_or(&[]);
        if line.starts_with(b"*** ") && next.starts_with(b"--- ") {
            return Dialect::Context;
        }
        if line.starts_with(b"--- ") && next.starts_with(b"+++ ") {
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
fn parse_normal_command(line: &[u8]) -> Option<(usize, usize, u8, usize, usize)> {
    let at = line.iter().position(|&b| matches!(b, b'a' | b'c' | b'd'))?;
    let action = line.get(at)?.to_owned();
    let left = line.get(..at)?;
    let right = line.get(at.saturating_add(1)..)?;
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let range = |s: &[u8]| -> Option<(usize, usize)> {
        match bytes::split_once(s, b',') {
            Some((a, b)) => Some((bytes::parse_usize(a)?, bytes::parse_usize(b)?)),
            None => {
                let n: usize = bytes::parse_usize(s)?;
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
fn parse_context_patch(input: &[u8]) -> Vec<FilePatch> {
    let lines: Vec<&[u8]> = bytes::lines(input);
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut i = 0;

    while let Some(line) = lines.get(i).copied() {
        let next = lines.get(i.saturating_add(1)).copied().unwrap_or(&[]);
        if !(line.starts_with(b"*** ") && next.starts_with(b"--- ")) {
            i = i.saturating_add(1);
            continue;
        }
        let old_path = parse_file_path(line, b"*** ");
        let new_path = parse_file_path(next, b"--- ");
        let header_lines = vec![line.to_vec(), next.to_vec()];
        i = i.saturating_add(2);
        let first_hunk_line = i.saturating_add(1);
        let mut hunks: Vec<Hunk> = Vec::new();

        while let Some(cur) = lines.get(i).copied() {
            if cur.starts_with(b"*** ") && !cur.trim_ascii_end().ends_with(b"****") {
                break; // the next file's header
            }
            if !cur.starts_with(b"***************") {
                i = i.saturating_add(1);
                continue;
            }
            i = i.saturating_add(1);

            // `*** 1,4 ****`
            let Some(old_hdr) = lines.get(i).copied() else {
                break;
            };
            let Some((os, oe)) = context_range(old_hdr, b"*** ") else {
                i = i.saturating_add(1);
                continue;
            };
            i = i.saturating_add(1);
            let mut old_body: Vec<(u8, Vec<u8>)> = Vec::new();
            while let Some(b) = lines.get(i).copied() {
                if b.starts_with(b"--- ") || b.starts_with(b"***************") {
                    break;
                }
                if let Some(item) = context_body_line(b) {
                    old_body.push(item);
                }
                i = i.saturating_add(1);
            }

            // `--- 1,4 ----`
            let mut new_body: Vec<(u8, Vec<u8>)> = Vec::new();
            let mut ns = os;
            let mut ne = oe;
            if let Some(new_hdr) = lines.get(i).copied()
                && let Some((a, b)) = context_range(new_hdr, b"--- ")
            {
                ns = a;
                ne = b;
                i = i.saturating_add(1);
                while let Some(bl) = lines.get(i).copied() {
                    if bl.starts_with(b"***************") || bl.starts_with(b"*** ") {
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
fn context_range(line: &[u8], prefix: &[u8]) -> Option<(usize, usize)> {
    let rest = line.strip_prefix(prefix)?;
    let body = bytes::trim_end_matches(rest, b"*- ");
    match bytes::split_once(body, b',') {
        Some((a, b)) => Some((
            bytes::parse_usize(a.trim_ascii())?,
            bytes::parse_usize(b.trim_ascii())?,
        )),
        None => {
            let n: usize = bytes::parse_usize(body.trim_ascii())?;
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
fn context_body_line(line: &[u8]) -> Option<(u8, Vec<u8>)> {
    let marker = line.first().copied()?;
    if !matches!(marker, b' ' | b'-' | b'+' | b'!') {
        return None;
    }
    let rest = line.get(2..).unwrap_or(&[]);
    Some((marker, rest.to_vec()))
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
    old_body: &[(u8, Vec<u8>)],
    new_body: &[(u8, Vec<u8>)],
) -> Hunk {
    let mut lines: Vec<HunkLine> = Vec::new();
    let (mut a, mut b) = (0usize, 0usize);
    while a < old_body.len() || b < new_body.len() {
        let om = old_body.get(a).map(|x| x.0);
        let nm = new_body.get(b).map(|x| x.0);
        match (om, nm) {
            (Some(b' '), Some(b' ')) => {
                if let Some((_, text)) = old_body.get(a) {
                    lines.push(HunkLine::Context(text.clone()));
                }
                a = a.saturating_add(1);
                b = b.saturating_add(1);
            }
            (Some(b'-'), _) => {
                if let Some((_, text)) = old_body.get(a) {
                    lines.push(HunkLine::Remove(text.clone()));
                }
                a = a.saturating_add(1);
            }
            (_, Some(b'+')) => {
                if let Some((_, text)) = new_body.get(b) {
                    lines.push(HunkLine::Add(text.clone()));
                }
                b = b.saturating_add(1);
            }
            (Some(b'!'), _) | (_, Some(b'!')) => {
                // Every `!` on the old side, then every `!` facing it on the
                // new side. Emitting them interleaved would produce a hunk
                // that renders back into a .rej file no shell could reapply.
                while let Some((b'!', text)) = old_body.get(a) {
                    lines.push(HunkLine::Remove(text.clone()));
                    a = a.saturating_add(1);
                }
                while let Some((b'!', text)) = new_body.get(b) {
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
fn parse_normal_patch(input: &[u8]) -> Vec<FilePatch> {
    let lines: Vec<&[u8]> = bytes::lines(input);
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
            if let Some(rest) = b.strip_prefix(b"< ") {
                body.push(HunkLine::Remove(rest.to_vec()));
            } else if let Some(rest) = b.strip_prefix(b"> ") {
                body.push(HunkLine::Add(rest.to_vec()));
            } else if b == b"---".as_slice() || b == b"<".as_slice() || b == b">".as_slice() {
                // The `c` separator, and the two degenerate spellings of a
                // blank line on either side.
                if b == b"<".as_slice() {
                    body.push(HunkLine::Remove(Vec::new()));
                } else if b == b">".as_slice() {
                    body.push(HunkLine::Add(Vec::new()));
                }
            } else {
                break;
            }
            i = i.saturating_add(1);
        }
        // `a` ADDS AFTER THE NAMED LINE, so its hunk starts at the line after.
        // `d` and `c` start at the line they name. Getting this wrong puts an
        // appended line one row too high, which still applies and is wrong.
        // `a` adds AFTER the named line and removes nothing, which
        // `apply_hunk` now handles uniformly for every dialect: an
        // `old_count == 0` hunk inserts at `old_start` rather than at
        // `old_start - 1`. This used to add one here to compensate for
        // `apply_hunk` being wrong, which was right for normal diffs and left
        // zero-context UNIFIED insertions broken -- the same fix in one
        // dialect's parser instead of in the shared applier.
        let (old_start, old_count) = match action {
            b'a' => (os, 0),
            _ => (os, oe.saturating_add(1).saturating_sub(os)),
        };
        let (new_start, new_count) = match action {
            b'd' => (ns.saturating_add(1), 0),
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
        old_path: Vec::new(),
        new_path: Vec::new(),
        hunks,
        header_lines: Vec::new(),
        first_hunk_line,
        malformed_at: None,
    }]
}

fn parse_patch(input: &[u8]) -> Vec<FilePatch> {
    let lines: Vec<&[u8]> = bytes::lines(input);
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut i = 0;

    while let Some(line_i) = lines.get(i).copied() {
        // Look for --- line followed by +++ line.
        let next_starts_with_plus = lines
            .get(i.saturating_add(1))
            .is_some_and(|l| l.starts_with(b"+++ "));
        if line_i.starts_with(b"--- ") && next_starts_with_plus {
            let old_path = parse_file_path(line_i, b"--- ");
            let plus_line = lines.get(i.saturating_add(1)).copied().unwrap_or(&[]);
            let new_path = parse_file_path(plus_line, b"+++ ");
            let header_lines = vec![line_i.to_vec(), plus_line.to_vec()];
            i = i.saturating_add(2);
            // `i` now indexes the first hunk header; GNU counts from one.
            let first_hunk_line = i.saturating_add(1);

            let mut hunks: Vec<Hunk> = Vec::new();
            let mut malformed_at: Option<usize> = None;

            // Parse hunks for this file.
            while let Some(cur) = lines.get(i).copied() {
                if cur.starts_with(b"@@ ") {
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
                            if line.starts_with(b"@@ ")
                                || line.starts_with(b"--- ")
                                || line.starts_with(b"diff ")
                            {
                                break;
                            }

                            if let Some(rest) = line.strip_prefix(b"+") {
                                hunk_lines.push(HunkLine::Add(rest.to_vec()));
                                new_left = new_left.saturating_sub(1);
                            } else if let Some(rest) = line.strip_prefix(b"-") {
                                hunk_lines.push(HunkLine::Remove(rest.to_vec()));
                                old_left = old_left.saturating_sub(1);
                            } else if let Some(rest) = line.strip_prefix(b" ") {
                                hunk_lines.push(HunkLine::Context(rest.to_vec()));
                                old_left = old_left.saturating_sub(1);
                                new_left = new_left.saturating_sub(1);
                            } else if line == b"\\ No newline at end of file".as_slice() {
                                // Informational line from diff; it stands for
                                // no line on either side, so it consumes
                                // neither count.
                            } else {
                                // Treat lines without prefix as context
                                // (some patches have bare context lines).
                                hunk_lines.push(HunkLine::Context(line.to_vec()));
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
                } else if cur.starts_with(b"--- ") || cur.starts_with(b"diff ") {
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

fn parse_file_path(line: &[u8], prefix: &[u8]) -> Vec<u8> {
    let rest = line.strip_prefix(prefix).unwrap_or(line);
    // Remove timestamp suffix if present (e.g., "file.c\t2024-01-01 ...")
    match rest.iter().position(|&b| b == b'\t') {
        Some(tab_pos) => rest.get(..tab_pos).unwrap_or(rest).to_vec(),
        None => rest.to_vec(),
    }
}

/// Apply a single hunk to the file lines. Returns the new lines if successful,
/// or None if the hunk doesn't match the expected context.
/// `offset` is the cumulative line offset from previous hunks.
/// What applying a hunk did, beyond producing the new content.
///
/// A struct rather than a wider tuple because three of these four are numbers
/// the caller has to tell apart -- `offset` is the running size change from
/// earlier hunks, `slide` is how far THIS hunk moved from where its header
/// said it would be, and they are different things that GNU reports
/// differently.
struct Applied {
    lines: Vec<Vec<u8>>,
    /// Cumulative size change, carried to the next hunk.
    offset: i64,
    /// Context lines ignored at each end to make it match.
    fuzz: usize,
    /// Distance from the header's position to the one it landed on. GNU
    /// reports this as `(offset N lines)`, which is an unrelated use of the
    /// word to the field above.
    slide: i64,
    /// 0-based line the hunk's first old line landed on.
    pos: usize,
}

fn apply_hunk(
    lines: &[Vec<u8>],
    hunk: &Hunk,
    offset: i64,
    loose: bool,
    max_fuzz: usize,
) -> Option<Applied> {
    // A HUNK THAT REMOVES NOTHING NAMES THE LINE TO INSERT *AFTER*, so its
    // 0-based insertion point is `old_start` itself and the usual `- 1` is
    // wrong. For delete and change, `old_start` is the first affected line and
    // the `- 1` is right.
    //
    // Measured against GNU on a zero-context insertion, `diff -U0` of one added
    // line, `@@ -1,0 +2 @@`:
    //
    //     GNU : a X b c
    //     ours: X a b c        <- before this
    //
    // The retired `userspace/patch` crate had this fix and this one did not.
    // It surfaced only because retiring the duplicate meant reading what the
    // loser knew that the winner did not -- a `todo.txt` note from 2026-05-31
    // recording the identical off-by-one, in the half that was about to be
    // deleted. Nothing in `patch-diff.sh` used `-U0`, so 64 of 64 passing said
    // nothing about it.
    let insertion = hunk.old_count == 0;
    let target_start_signed = i64::try_from(hunk.old_start)
        .unwrap_or(i64::MAX)
        .saturating_add(offset)
        .saturating_sub(if insertion { 0 } else { 1 })
        .max(0);
    let target_start = usize::try_from(target_start_signed).unwrap_or(0);

    // SLIDE and FUZZ are two different things, and this constant used to be
    // called `max_fuzz` while meaning the first. Sliding is how far the hunk
    // may move from the line its header names, looking for an exact match --
    // what GNU reports as `(offset N lines)`. Fuzz is how many CONTEXT lines
    // may be ignored at each end of the hunk -- what GNU reports as `with
    // fuzz N`. One name over two mechanisms, and the name belonged to the one
    // that was not implemented.
    const MAX_SLIDE: usize = 50;

    // Exact first, then looser. GNU tries every position at fuzz 0 before it
    // tries any position at fuzz 1, so a clean match further away beats a
    // fuzzy one nearby -- which is the order that keeps a hunk from silently
    // eating a neighbouring block that merely resembles it.
    let mut best: Option<(usize, usize)> = None;

    'outer: for fz in 0..=max_fuzz {
        for slide in 0..=MAX_SLIDE {
            if slide == 0 {
                if try_hunk_at(lines, hunk, target_start, loose, fz) {
                    best = Some((target_start, fz));
                    break 'outer;
                }
                continue;
            }
            if let Some(pos) = target_start.checked_sub(slide)
                && try_hunk_at(lines, hunk, pos, loose, fz)
            {
                best = Some((pos, fz));
                break 'outer;
            }
            let pos = target_start.saturating_add(slide);
            if try_hunk_at(lines, hunk, pos, loose, fz) {
                best = Some((pos, fz));
                break 'outer;
            }
        }
    }

    let (pos, fuzz_used) = best?;

    // Build the new file content.
    let mut result = Vec::new();
    if let Some(head) = lines.get(..pos) {
        result.extend_from_slice(head);
    }

    // A CONTEXT line is emitted as the FILE has it, NOT as the patch spells
    // it. Under exact matching the two are the same bytes, which is why this
    // went unnoticed for as long as exact matching was all there was. Under
    // `-l` they are not: the patch may use four spaces where the file uses a
    // tab, and both are "the same line". GNU keeps the file's -- measured, a
    // tab-indented file patched with a space-indented patch comes back still
    // indented with tabs, and only the line the hunk genuinely changes is
    // rewritten.
    //
    // Emitting the patch's text instead silently reindents every context line
    // the hunk covers, which is a whitespace rewrite nobody asked for and the
    // diff would not show as a change of content.
    let mut src = pos;
    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(s) => {
                // `unwrap_or_else` cannot normally fire: the hunk matched
                // here, so the line exists. It is the patch's text rather
                // than a panic if a future caller ever matches past the end.
                result.push(lines.get(src).cloned().unwrap_or_else(|| s.clone()));
                src = src.saturating_add(1);
            }
            HunkLine::Add(s) => result.push(s.clone()),
            // Skipped from the output, but it still consumed a file line.
            HunkLine::Remove(_) => src = src.saturating_add(1),
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

    let slide = i64::try_from(pos)
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(target_start).unwrap_or(i64::MAX));

    Some(Applied {
        lines: result,
        offset: new_offset,
        fuzz: fuzz_used,
        slide,
        pos,
    })
}

/// Compare one hunk line against one file line, honouring `-l`.
fn lines_match(actual: &[u8], expected: &[u8], loose: bool) -> bool {
    if loose {
        loose_eq(actual, expected)
    } else {
        actual == expected
    }
}

/// `-l/--ignore-whitespace`: a run of whitespace is interchangeable with any
/// other run, but is never interchangeable with nothing.
///
/// Measured against GNU patch, seven cases against one patch whose context
/// line is `    indented a b`:
///
/// | target line | GNU |
/// |---|---|
/// | `\tindented a b` (tab for four spaces) | match |
/// | `        indented a b` (eight for four) | match |
/// | `    indented  a  b` (internal run longer) | match |
/// | `    indented a b    ` (trailing added) | match |
/// | `indented a b` (leading run REMOVED) | fail |
/// | `    indenteda b` (internal run REMOVED) | fail |
/// | `    indented a c` (a real byte differs) | fail |
///
/// One rule explains all seven: strip TRAILING whitespace, then treat every
/// remaining run as equal to any other run. The asymmetry is the part worth
/// keeping -- lengthening a run matches, removing it does not.
///
/// A line that is ENTIRELY whitespace matches an empty line, which looks like
/// a contradiction of that and is not: the trailing strip consumes the whole
/// line, so there is no run left to match against nothing. Measured both ways
/// round -- four spaces against a tab, against two spaces, and against an
/// empty line all match, while four spaces against `x` fails. Thirteen cases
/// in total, one rule, no special case.
///
/// Whitespace here is SPACE and TAB only, and that is measured rather than
/// assumed: `\v`, `\f` and `\r` each FAIL against a space. So
/// `is_ascii_whitespace()` would have been wrong in three ways, and wrong
/// invisibly -- no fixture in this tree holds those bytes, so every harness
/// case would still have passed. An instrument that cannot fail is not
/// evidence.
fn loose_eq(a: &[u8], b: &[u8]) -> bool {
    // GNU's set here is `" \t"` -- space and tab -- not `isspace`.
    const WS: &[u8] = b" \t";

    fn is_ws(byte: u8) -> bool {
        byte == b' ' || byte == b'\t'
    }

    // Trailing whitespace is not part of the comparison at all, on either side.
    let a = bytes::trim_end_matches(a, WS);
    let b = bytes::trim_end_matches(b, WS);

    let (mut i, mut j) = (0usize, 0usize);
    loop {
        match (a.get(i).copied(), b.get(j).copied()) {
            (None, None) => return true,
            // Both sit on a run: consume each run WHOLE. The two runs may be
            // different lengths and made of different bytes; that is the point.
            (Some(x), Some(y)) if is_ws(x) && is_ws(y) => {
                while a.get(i).copied().is_some_and(is_ws) {
                    i = i.saturating_add(1);
                }
                while b.get(j).copied().is_some_and(is_ws) {
                    j = j.saturating_add(1);
                }
            }
            (Some(x), Some(y)) if x == y && !is_ws(x) => {
                i = i.saturating_add(1);
                j = j.saturating_add(1);
            }
            // Anything else -- one side ended, or a run faces a non-run, or two
            // ordinary bytes differ -- is a mismatch.
            _ => return false,
        }
    }
}

/// How many CONTEXT lines a hunk opens and closes with.
///
/// Fuzz is spent at the ENDS, so these two counts are its budget. A hunk that
/// begins with a removal has no leading context and no leading fuzz is
/// possible -- measured: GNU refuses a perturbed removal even at `-F3`.
fn leading_context(hunk: &Hunk) -> usize {
    hunk.lines
        .iter()
        .take_while(|l| matches!(l, HunkLine::Context(_)))
        .count()
}

fn trailing_context(hunk: &Hunk) -> usize {
    hunk.lines
        .iter()
        .rev()
        .take_while(|l| matches!(l, HunkLine::Context(_)))
        .count()
}

/// Does the hunk's context match the file at `pos`, allowing `fuzz`?
///
/// `fuzz` is GNU's `-F`: ignore up to that many CONTEXT lines at EACH end of
/// the hunk. Measured, with a hunk carrying three context lines either side:
///
/// | perturbed | GNU applies at |
/// |---|---|
/// | the outermost leading context line | fuzz 1 |
/// | outermost leading AND outermost trailing | fuzz 1 |
/// | two leading and one trailing | fuzz 2 |
/// | a REMOVED line, at any `-F` | never |
///
/// The second row is what makes "at each end" the right reading rather than
/// "in total": two wrong lines still cost only fuzz 1, because they sit at
/// opposite ends. The default is 2, also measured -- three wrong leading
/// context lines fail without an explicit `-F 3`.
///
/// An ignored context line is still CONSUMED: it occupies its file line, it
/// is simply not compared. What the file holds there is kept, which
/// `apply_hunk` handles by emitting context from the file rather than the
/// patch.
fn try_hunk_at(lines: &[Vec<u8>], hunk: &Hunk, pos: usize, loose: bool, fuzz: usize) -> bool {
    // Fuzz can only ever spend itself on context, so it is capped by how much
    // context there is. `-F 100` on a hunk with two leading context lines
    // ignores two, not a hundred, and still compares every removal.
    let skip_lead = fuzz.min(leading_context(hunk));
    let skip_trail = fuzz.min(trailing_context(hunk));
    let old_total = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_) | HunkLine::Remove(_)))
        .count();
    let trail_from = old_total.saturating_sub(skip_trail);

    let mut line_idx = pos;
    let mut old_seen: usize = 0;
    for hl in &hunk.lines {
        match hl {
            HunkLine::Context(expected) | HunkLine::Remove(expected) => {
                let Some(actual) = lines.get(line_idx) else {
                    return false;
                };
                // A skipped line still has to EXIST -- the hunk occupies it
                // either way. Only the comparison is waived, and only for
                // context: a removal is a line this hunk is about to delete,
                // so agreeing about it is not optional.
                let skipped = matches!(hl, HunkLine::Context(_))
                    && (old_seen < skip_lead || old_seen >= trail_from);
                if !skipped && !lines_match(actual, expected, loose) {
                    return false;
                }
                line_idx = line_idx.saturating_add(1);
                old_seen = old_seen.saturating_add(1);
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
    // `args_os`, not `args`: the latter's iterator unwraps, so a filename
    // holding a byte that is not valid Unicode aborts the process before
    // `patch` runs a line of its own. Our paths allow every byte but `/`
    // and NUL (design.txt), so that is a legal name, not a malformed one.
    let args: Vec<OsString> = env::args_os().skip(1).collect();

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
        diag!("patch: {}: {e}", quotef_os(dir));
        process::exit(2);
    }

    // Read patch input.
    let patch_input = if let Some(ref path) = opts.patch_file {
        match fs::read(path) {
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
        let mut buf: Vec<u8> = Vec::new();
        if io::stdin().read_to_end(&mut buf).is_err() {
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
        if patch_input.trim_ascii().is_empty() {
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
            let mut intro: Vec<u8> =
                format!("Hmm...  {lead} a {dialect_name} diff to me...\n").into_bytes();
            // A normal diff carries no header lines at all, so GNU omits the
            // whole block rather than printing an empty one. The same block is
            // already built for the can't-find-file diagnostic; this is the
            // only other place it appears.
            if !fp.header_lines.is_empty() {
                intro.extend_from_slice(b"The text leading up to this was:\n");
                intro.extend_from_slice(b"--------------------------\n");
                for h in &fp.header_lines {
                    intro.push(b'|');
                    // The header line as it arrived. It is echoed back, so
                    // it has to be the bytes the patch held, not a render.
                    intro.extend_from_slice(h);
                    intro.push(b'\n');
                }
                intro.extend_from_slice(b"--------------------------\n");
            }
            let mut out = Stream::stdout();
            let _ = out.write_all(&intro);
        }
        // Determine the target file path.
        // Bytes, not `OsString`: nearly everything done to this value is
        // byte arithmetic -- `strip_path` cuts it at a `/`, and the reject and
        // backup names are it with a suffix stuck on. It becomes something a
        // syscall takes, via `os_from_bytes`, only where a syscall takes it.
        let raw_path: Vec<u8> = if let Some(ref target) = opts.target_file {
            quote::os_bytes(target).into_owned()
        } else if opts.reverse {
            fp.new_path.clone()
        } else {
            // Prefer new_path if old_path is /dev/null (new file).
            if fp.old_path == DEV_NULL {
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
            Some(named) => quote::os_bytes(named).into_owned(),
            None => match opts.strip {
                Some(n) => strip_path(&raw_path, n),
                None => raw_path.clone(),
            },
        };

        // The same name as something a syscall takes, converted once. On the
        // target this is free -- an `OsStr` there IS its bytes.
        let file_path_os = quote::os_from_bytes(&file_path);

        // Read the original file (or start empty for new files).
        let original = if fp.old_path == DEV_NULL && !opts.reverse {
            Vec::new()
        } else {
            // `fs::read`, not `read_to_string`. The file being patched is the
            // user's, and one Latin-1 byte in a comment is enough to make it
            // not valid UTF-8 -- which GNU patches and this used to refuse.
            match fs::read(quote::os_from_bytes(&file_path)) {
                Ok(s) => s,
                Err(e) => {
                    if fp.old_path == DEV_NULL || opts.target_file.is_some() {
                        // A NAMED TARGET IS NOT A SEARCH, so it cannot fail as
                        // one. The block below is what GNU prints when it
                        // *looked* for a file and could not work out which one
                        // the patch meant -- which is why it suggests `-p`. If
                        // the operand named the file, there was nothing to work
                        // out, and a missing one is simply empty.
                        //
                        // This is not a cosmetic difference. Measured, and it
                        // is the same answer in every dialect: `patch -i
                        // create.patch newfile.txt` where `create.patch` only
                        // ADDS lines produces the file and exits 0. Refusing
                        // with `can't find file to patch` meant this build
                        // could not create a file from a patch at all when the
                        // target was named -- and a normal diff names no file,
                        // so for that dialect it could not create one ever.
                        Vec::new()
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
                        // Bytes: the header lines this echoes back are the patch's own,
                        // and a patch may name a file this OS allows and
                        // Unicode does not.
                        let mut block: Vec<u8> = Vec::new();
                        block.extend_from_slice(
                            format!(
                                "can't find file to patch at input line {}
",
                                fp.first_hunk_line
                            )
                            .as_bytes(),
                        );
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
                        block.extend_from_slice(if opts.strip.is_none() {
                            b"Perhaps you should have used the -p or --strip option?
"
                        } else {
                            b"Perhaps you used the wrong -p or --strip option?
"
                        });
                        block.extend_from_slice(
                            b"The text leading up to this was:
",
                        );
                        block.extend_from_slice(
                            b"--------------------------
",
                        );
                        for h in &fp.header_lines {
                            block.push(b'|');
                            // Verbatim. GNU echoes back the header line it
                            // read, and a render of it would not be that line.
                            block.extend_from_slice(h);
                            block.push(b'\n');
                        }
                        block.extend_from_slice(
                            b"--------------------------
",
                        );
                        block.extend_from_slice(
                            b"File to patch: 
",
                        );
                        block.extend_from_slice(
                            b"Skip this patch? [y] 
",
                        );
                        block.extend_from_slice(
                            b"Skipping patch.
",
                        );
                        let n = fp.hunks.len();
                        let plural = if n == 1 { "hunk" } else { "hunks" };
                        block.extend_from_slice(
                            format!(
                                "{n} out of {n} {plural} ignored
"
                            )
                            .as_bytes(),
                        );
                        let _ = out.write_all(&block);
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
            let named: Vec<u8> = opts
                .output_file
                .as_ref()
                .map_or_else(|| file_path.clone(), |p| quote::os_bytes(p).into_owned());
            let source: Vec<u8> = if opts.output_file.is_some() {
                [b" (read from ".as_slice(), &file_path, b")"].concat()
            } else {
                Vec::new()
            };
            // No ellipsis on the dry-run line. GNU prints
            // "checking file a/base.txt"; this build printed a trailing
            // "..." that predates tonight and that nothing upstream produces.
            // Assembled as bytes rather than `format!`ed. GNU writes the
            // name into this line RAW -- it is stdout, not a diagnostic -- so
            // a name holding a byte that is not valid Unicode has to reach the
            // line unaltered. `quotef` would be the wrong answer here for the
            // same reason it is the right one on stderr.
            let lead: &[u8] = if opts.dry_run {
                b"checking file "
            } else {
                b"patching file "
            };
            let line: Vec<u8> = [lead, &named, &source, b"\n"].concat();
            let mut out = Stream::stdout();
            let _ = out.write_all(&line);
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

        let mut lines: Vec<Vec<u8>> = bytes::lines(&original)
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        let mut offset: i64 = 0;
        let mut hunks_applied = 0;
        let mut hunks_failed = 0;
        // Did any hunk land INEXACTLY -- needing fuzz, or having to slide
        // off the line its header named? GNU treats either as the patch not
        // matching the file, which decides whether a `.orig` is written.
        let mut any_mismatch = false;
        let mut rejected: Vec<Hunk> = Vec::new();

        // GNU's default fuzz is 2, measured: three wrong leading context
        // lines are refused without an explicit `-F 3`, two are not.
        let max_fuzz = opts.fuzz.unwrap_or(2);

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
        let forward_fails = hunks
            .iter()
            .any(|h| apply_hunk(&lines, h, 0, opts.ignore_whitespace, max_fuzz).is_none());
        let opposite_applies = !opposite.is_empty()
            && opposite
                .iter()
                .all(|h| apply_hunk(&lines, h, 0, opts.ignore_whitespace, max_fuzz).is_some());
        if forward_fails && opposite_applies {
            any_failed = true;
            // `-r FILE` names the reject file outright; without it the reject
            // sits beside the target as `<target>.rej`.
            let reject_path: Vec<u8> = opts.reject_file.as_ref().map_or_else(
                || [file_path.as_slice(), b".rej"].concat(),
                |p| quote::os_bytes(p).into_owned(),
            );
            if !opts.dry_run {
                let strip_n = opts.strip.unwrap_or(0);
                // The paths go in RAW, as GNU writes them: a reject has to be
                // a patch someone can re-apply, and a quoted path would not
                // name the file.
                let mut reject: Vec<u8> = [
                    b"--- ".as_slice(),
                    &strip_path(&fp.old_path, strip_n),
                    b"\n+++ ",
                    &strip_path(&fp.new_path, strip_n),
                    b"\n",
                ]
                .concat();
                for h in &hunks {
                    reject.extend_from_slice(&render_hunk(h));
                }
                let _ = fs::write(quote::os_from_bytes(&reject_path), &reject);
            }
            if !opts.silent {
                let detected = if opts.reverse {
                    "Unreversed patch detected!  "
                } else {
                    "Reversed (or previously applied) patch detected!  "
                };
                // `-N/--forward` asks nothing. Measured: GNU drops BOTH prompt
                // lines and puts `Skipping patch.` on the same line as the
                // detection, so the question text and the newline after it go
                // together -- printing one without the other would leave a
                // line break GNU does not write.
                let prompts = if opts.forward {
                    ""
                } else if opts.reverse {
                    "Ignore -R? [n] \nApply anyway? [n] \n"
                } else {
                    "Assume -R? [n] \nApply anyway? [n] \n"
                };
                let n = hunks.len();
                let plural = if n == 1 { "hunk" } else { "hunks" };
                // Under `--dry-run` no reject file was written, so naming one
                // sends the reader looking for a file that does not exist.
                // The FAILED path a hundred lines down had already learned
                // this; this path had not, because the two were written weeks
                // apart and only the other one had a differential case. The
                // same defect twice in one file is what a harness is for.
                // Assembled as bytes: the reject path goes in RAW, the way
                // GNU writes it on stdout, so a name this OS allows and
                // Unicode does not still names the file it wrote.
                let mut msg: Vec<u8> =
                    format!("{detected}{prompts}Skipping patch.\n{n} out of {n} {plural} ignored")
                        .into_bytes();
                if !opts.dry_run {
                    msg.extend_from_slice(b" -- saving rejects to file ");
                    msg.extend_from_slice(&reject_path);
                }
                msg.push(b'\n');
                let mut out = Stream::stdout();
                let _ = out.write_all(&msg);
            }
            continue;
        }

        for (hunk_idx, hunk) in hunks.iter().enumerate() {
            match apply_hunk(&lines, hunk, offset, opts.ignore_whitespace, max_fuzz) {
                Some(applied) => {
                    let fuzz_used = applied.fuzz;
                    let slide = applied.slide;
                    any_mismatch = any_mismatch || fuzz_used > 0 || slide != 0;
                    // GNU reports an INEXACT hunk without `-v`: a clean apply
                    // is silent, one that needed fuzz or had to move says so.
                    // Measured -- plain `patch` prints `Hunk #1 succeeded at 2
                    // with fuzz 1.` and `Hunk #1 succeeded at 3 (offset 1
                    // line).` with no verbosity asked for.
                    if (opts.verbose || fuzz_used > 0 || slide != 0) && !opts.silent {
                        // The line the hunk landed on, which is its start
                        // shifted by everything applied before it -- not the
                        // number in the header. A second hunk in a file whose
                        // first hunk changed the line count reports the moved
                        // position, which is the only number a reader can go
                        // and look at.
                        // The line it ACTUALLY landed on, taken from the
                        // match position rather than recomputed from the
                        // header, so the number in the message and the bytes
                        // on disk cannot disagree.
                        //
                        // This also FIXES a case that was already wrong. The
                        // old formula was `old_start + offset`, and for a
                        // zero-context INSERTION that is one too low: measured
                        // under `--verbose`, a `@@ -1,0 +2 @@` hunk reports
                        // `Hunk #1 succeeded at 2.` and the old formula said
                        // 1. `pos + 1` is right for all three shapes -- exact,
                        // slid, and insertion -- which the old one was not.
                        let at = i64::try_from(applied.pos)
                            .unwrap_or(i64::MAX)
                            .saturating_add(1)
                            .max(1);
                        let fuzz_note = if fuzz_used > 0 {
                            format!(" with fuzz {fuzz_used}")
                        } else {
                            String::new()
                        };
                        // `1 line`, `2 lines` -- and `-1 lines`, PLURAL.
                        // Measured, and not what anyone would write: GNU's
                        // test is `n == 1`, not `n.abs() == 1`, so a hunk that
                        // moved one line EARLIER reads `(offset -1 lines)`.
                        let offset_note = if slide == 0 {
                            String::new()
                        } else {
                            let unit = if slide == 1 { "line" } else { "lines" };
                            format!(" (offset {slide} {unit})")
                        };
                        let mut out = Stream::stdout();
                        let _ = out.write_all(
                            format!(
                                "Hunk #{} succeeded at {at}{fuzz_note}{offset_note}.\n",
                                hunk_idx + 1
                            )
                            .as_bytes(),
                        );
                    }
                    lines = applied.lines;
                    offset = applied.offset;
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
            let reject_path: Vec<u8> = opts.reject_file.as_ref().map_or_else(
                || [file_path.as_slice(), b".rej"].concat(),
                |p| quote::os_bytes(p).into_owned(),
            );
            // A NAMED TARGET THAT NEVER EXISTED, whose hunks then failed.
            //
            // GNU's shape is odd and is reproduced deliberately, ORDER
            // INCLUDED: it writes the (empty) `.orig`, then tries to reopen the
            // original to restore it, cannot, and dies -- before the reject is
            // written. So the run leaves an empty `<target>.orig`, NO `.rej`,
            // no output file, and exit 2.
            //
            // The order is the whole of the remaining difference. Placing this
            // after the reject write matched GNU's stdout, stderr and exit code
            // exactly and still left a 44-byte `a/nosuch.txt.rej` on disk that
            // GNU does not create -- which only the harness's directory
            // snapshot could see, since all three streams agreed.
            //
            // Bug-for-bug on purpose; design-decisions.md 371.
            if opts.target_file.is_some() && !Path::new(&file_path_os).exists() && !opts.dry_run {
                if !opts.no_backup_if_mismatch {
                    let orig_path = [file_path.as_slice(), b".orig"].concat();
                    let _ = fs::write(quote::os_from_bytes(&orig_path), &original);
                }
                diag!(
                    "patch: **** Can't reopen file {} : No such file or directory",
                    quote::quotef(&file_path)
                );
                process::exit(2);
            }
            if !opts.dry_run {
                let mut reject: Vec<u8> = Vec::new();
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
                reject.extend_from_slice(
                    &[
                        b"--- ".as_slice(),
                        &strip_path(&fp.old_path, strip_n),
                        b"\n+++ ",
                        &strip_path(&fp.new_path, strip_n),
                        b"\n",
                    ]
                    .concat(),
                );
                for h in &rejected {
                    reject.extend_from_slice(&render_hunk(h));
                }
                let _ = fs::write(quote::os_from_bytes(&reject_path), &reject);
                // `--no-backup-if-mismatch` suppresses exactly this and nothing
                // else: the reject is still written, because the reject is the
                // failure report rather than a backup.
                if !opts.no_backup_if_mismatch {
                    let orig_path = [file_path.as_slice(), b".orig"].concat();
                    let _ = fs::write(quote::os_from_bytes(&orig_path), &original);
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
                let plural = if total == 1 { "hunk" } else { "hunks" };
                // Bytes, so the reject path goes in raw -- see the sibling
                // summary above. The reject clause is omitted under
                // `--dry-run`, because no reject file was written: GNU
                // prints the bare `1 out of 1 hunk FAILED` there, and
                // naming a file that does not exist would send the reader
                // looking for it.
                let mut msg: Vec<u8> =
                    format!("{hunks_failed} out of {total} {plural} FAILED").into_bytes();
                if !opts.dry_run {
                    msg.extend_from_slice(b" -- saving rejects to file ");
                    msg.extend_from_slice(&reject_path);
                }
                msg.push(b'\n');
                let mut out = Stream::stdout();
                let _ = out.write_all(&msg);
            }
        }

        if !opts.dry_run && hunks_applied > 0 {
            // `-b` asks for a backup outright. GNU ALSO writes one when the
            // patch did not apply EXACTLY -- `--backup-if-mismatch` is its
            // default -- and a hunk that needed fuzz is exactly that.
            //
            // Measured, and the last row is the one worth keeping:
            //
            // | apply | `.orig` |
            // |---|---|
            // | exact | no |
            // | fuzz 1 | YES |
            // | offset 1, no fuzz | YES |
            // | fuzz 1 with `--no-backup-if-mismatch` | no |
            // | `-l`, tab against spaces | NO |
            //
            // A loose match is not a mismatch. That is not obvious -- it is
            // every bit as inexact as a fuzzy one -- but `-l` says those lines
            // ARE equal, so nothing about the match was approximate once the
            // flag was given. The harness found this the honest way: the
            // `-l` case passed while the fuzz case differed by one file.
            let backup_for_mismatch = any_mismatch && !opts.no_backup_if_mismatch;
            if (opts.backup || backup_for_mismatch) && Path::new(&file_path_os).exists() {
                let backup_path = [file_path.as_slice(), b".orig"].concat();
                if let Err(e) = fs::copy(&file_path_os, quote::os_from_bytes(&backup_path)) {
                    diag!(
                        "patch: cannot create backup {}: {e}",
                        quote::quotef(&backup_path)
                    );
                }
            }

            // Create parent directories if needed (for new files).
            if let Some(parent) = Path::new(&file_path_os).parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = fs::create_dir_all(parent);
            }

            // Write the patched file.
            let mut output = lines.join(&b'\n');
            // Preserve trailing newline if the original had one.
            if original.ends_with(b"\n") || fp.old_path == DEV_NULL {
                output.push(b'\n');
            }

            // `-o` redirects the RESULT and leaves the target alone, so a
            // `-E` deletion would be deleting the wrong file: the emptiness is
            // a property of what was written, not of what was read.
            let dest: Vec<u8> = opts
                .output_file
                .as_ref()
                .map_or_else(|| file_path.clone(), |p| quote::os_bytes(p).into_owned());
            let dest_os = quote::os_from_bytes(&dest);
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
            let is_deletion = fp.new_path == DEV_NULL && !opts.reverse;
            if (is_deletion || (opts.remove_empty && output.is_empty()))
                && opts.output_file.is_none()
            {
                // `-E` removes a file the patch has emptied. Measured: the file
                // is gone from the tree, not left at zero length.
                if let Err(e) = fs::remove_file(&dest_os) {
                    diag!("patch: cannot remove {}: {e}", quote::quotef(&dest));
                    any_failed = true;
                }
            } else if let Err(e) = write_result(&dest_os, &output, opts.output_file.is_some()) {
                diag!("patch: cannot write {}: {e}", quote::quotef(&dest));
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

    // ---------------- bytes, not UTF-8 ----------------
    //
    // Measured against GNU patch 2.7.6 before any of this was written:
    //
    //     $ patch -i u.diff orig.txt        # a context line holds byte 0xE9
    //     patching file orig.txt            # GNU:  exit 0, byte preserved
    //     patch: **** Can't open patch file u.diff : stream did not contain
    //           valid UTF-8                 # ours: exit 2, nothing patched
    //
    // The refusal came from `fs::read_to_string`, so it happened before any
    // target was opened -- one Latin-1 byte in a comment was enough to make a
    // file unpatchable. See known-issues.md ->
    // B-PATCH-REFUSES-EVERY-FILE-THAT-IS-NOT-VALID-UTF-8.

    /// The byte that is not valid UTF-8 on its own: Latin-1 `é`.
    const HIGH: u8 = 0xE9;

    #[test]
    fn a_context_line_that_is_not_utf8_survives_parsing() {
        let mut input: Vec<u8> = b"--- a.txt~+++ a.txt~@@ -1,2 +1,2 @@~ caf".to_vec();
        input.push(HIGH);
        input.extend_from_slice(b"~-old~+new~");
        let input: Vec<u8> = input
            .iter()
            .map(|&b| if b == b'~' { b'\n' } else { b })
            .collect();

        let ps = parse_patch(&input);
        assert_eq!(ps.len(), 1, "the patch should parse");
        let ctxt = &ps[0].hunks[0].lines[0];
        // The byte is carried through untouched -- not replaced, not dropped,
        // and above all not a reason to refuse the file.
        assert_eq!(*ctxt, HunkLine::Context(b"caf\xe9".to_vec()));
    }

    #[test]
    fn a_path_that_is_not_utf8_survives_parsing() {
        // A name this OS allows -- design.txt: every byte but `/` and NUL.
        let mut input: Vec<u8> = b"--- caf".to_vec();
        input.push(HIGH);
        input.extend_from_slice(b".txt~+++ caf");
        input.push(HIGH);
        input.extend_from_slice(b".txt~@@ -1 +1 @@~-a~+b~");
        let input: Vec<u8> = input
            .iter()
            .map(|&b| if b == b'~' { b'\n' } else { b })
            .collect();

        let ps = parse_patch(&input);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].old_path, b"caf\xe9.txt");
    }

    #[test]
    fn strip_path_cuts_a_name_that_is_not_utf8() {
        // `-p1` over a directory whose name is not Unicode. Cutting at a `/`
        // is a byte operation; decoding first would fail on the whole path.
        let mut p: Vec<u8> = b"caf".to_vec();
        p.push(HIGH);
        p.extend_from_slice(b"/inner.txt");
        assert_eq!(strip_path(&p, 1), b"inner.txt");
    }

    #[test]
    fn an_option_value_that_is_not_utf8_is_split_at_the_byte_level() {
        // `--input=<name>` where the NAME is not Unicode. Splitting a decoded
        // `&str` cannot do this: the whole argument fails to decode, the
        // option stops matching, and the path is taken as an operand -- a
        // wrong file rather than an error.
        let name = coreutils::quote::os_from_bytes(&[b'c', b'a', b'f', HIGH]);
        let mut arg = OsString::from("--input=");
        arg.push(&name);

        let o = parse_args(&[arg]).expect("the option should still match");
        assert_eq!(o.patch_file.as_deref(), Some(name.as_os_str()));
        assert!(o.target_file.is_none(), "it is an option, not an operand");
    }

    #[test]
    fn a_line_keeps_its_bytes_through_the_splitter() {
        // `bytes::lines` is the one place every line passes through, so this
        // is the narrowest place to pin that nothing decodes.
        let mut text: Vec<u8> = b"one~".to_vec();
        text.push(HIGH);
        text.extend_from_slice(b"~three");
        let text: Vec<u8> = text
            .iter()
            .map(|&b| if b == b'~' { b'\n' } else { b })
            .collect();
        assert_eq!(
            bytes::lines(&text),
            vec![b"one".as_slice(), &[HIGH], b"three"]
        );
    }

    #[test]
    fn a_crlf_patch_line_loses_only_its_terminator() {
        // `str::lines` drops a trailing CR and the byte version must too, or
        // every context line of a DOS patch fails to match an LF target.
        assert_eq!(bytes::lines(b"a\r\nb\n"), vec![b"a".as_slice(), b"b"]);
    }

    /// argv for a test, as `parse_args` now takes it.
    ///
    /// Only the element type changed when `patch` moved to bytes; every call
    /// site and every assertion below is the one that was there before, which
    /// is what makes them a check on the conversion rather than a restatement
    /// of it.
    fn s(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
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
        assert_eq!(o.patch_file.as_deref(), Some(OsStr::new("x.patch")));
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
        assert_eq!(o.target_file.as_deref(), Some(OsStr::new("foo.txt")));
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
        assert_eq!(o.target_file.as_deref(), Some(OsStr::new("-")));
    }

    // ---------------- strip_path ----------------

    #[test]
    fn strip_zero_keeps_path() {
        assert_eq!(strip_path(b"a/b/c", 0), b"a/b/c");
    }

    #[test]
    fn strip_one() {
        assert_eq!(strip_path(b"a/b/c", 1), b"b/c");
    }

    #[test]
    fn strip_two() {
        assert_eq!(strip_path(b"a/b/c", 2), b"c");
    }

    #[test]
    fn strip_too_many_falls_back_to_basename() {
        assert_eq!(strip_path(b"a/b/c", 5), b"c");
    }

    #[test]
    fn strip_no_slashes_basename() {
        assert_eq!(strip_path(b"file.c", 1), b"file.c");
    }

    // ---------------- parse_range ----------------

    #[test]
    fn parse_range_with_count() {
        assert_eq!(parse_range(b"10,5"), Some((10, 5)));
    }

    #[test]
    fn parse_range_single_number_implies_one() {
        assert_eq!(parse_range(b"7"), Some((7, 1)));
    }

    #[test]
    fn parse_range_garbage_is_none() {
        assert!(parse_range(b"x").is_none());
        assert!(parse_range(b"1,x").is_none());
    }

    // ---------------- parse_hunk_header ----------------

    #[test]
    fn parse_hunk_header_basic() {
        let h = parse_hunk_header(b"@@ -1,3 +1,4 @@").unwrap();
        assert_eq!(h, (1, 3, 1, 4));
    }

    #[test]
    fn parse_hunk_header_with_trailing_context() {
        let h = parse_hunk_header(b"@@ -10,5 +20,7 @@ fn foo()").unwrap();
        assert_eq!(h, (10, 5, 20, 7));
    }

    #[test]
    fn parse_hunk_header_single_line_count_one() {
        let h = parse_hunk_header(b"@@ -5 +5 @@").unwrap();
        assert_eq!(h, (5, 1, 5, 1));
    }

    #[test]
    fn parse_hunk_header_no_at_markers_is_none() {
        assert!(parse_hunk_header(b"nope").is_none());
        assert!(parse_hunk_header(b"@@ no end").is_none());
    }

    // ---------------- parse_file_path ----------------

    #[test]
    fn parse_file_path_plain() {
        assert_eq!(parse_file_path(b"--- foo.c", b"--- "), b"foo.c");
    }

    #[test]
    fn parse_file_path_strips_timestamp() {
        assert_eq!(
            parse_file_path(b"+++ bar.c\t2024-01-01 12:00", b"+++ "),
            b"bar.c"
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
        let ps = parse_patch(SIMPLE_PATCH.as_bytes());
        assert_eq!(ps.len(), 1);
        let fp = &ps[0];
        assert_eq!(fp.old_path, b"old.txt");
        assert_eq!(fp.new_path, b"new.txt");
        assert_eq!(fp.hunks.len(), 1);
        let h = &fp.hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
        assert_eq!(
            h.lines,
            vec![
                HunkLine::Context(b"line1".to_vec()),
                HunkLine::Remove(b"line2".to_vec()),
                HunkLine::Add(b"line2 modified".to_vec()),
                HunkLine::Context(b"line3".to_vec()),
            ]
        );
    }

    #[test]
    fn parse_patch_no_diff_returns_empty() {
        assert!(parse_patch(b"no diff here").is_empty());
        assert!(parse_patch(b"").is_empty());
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
        let ps = parse_patch(input.as_bytes());
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].old_path, b"a.c");
        assert_eq!(ps[1].old_path, b"b.c");
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

    /// File content as `apply_hunk` now takes it: a line is bytes.
    fn lines(items: &[&str]) -> Vec<Vec<u8>> {
        items.iter().map(|x| x.as_bytes().to_vec()).collect()
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
        assert!(try_hunk_at(&l, &modify_hunk(), 0, false, 0));
    }

    #[test]
    fn try_hunk_at_fails_on_mismatch() {
        let l = lines(&["lineA", "lineB", "lineC"]);
        assert!(!try_hunk_at(&l, &modify_hunk(), 0, false, 0));
    }

    #[test]
    fn try_hunk_at_fails_past_end() {
        let l = lines(&["line1"]);
        assert!(!try_hunk_at(&l, &modify_hunk(), 0, false, 0));
    }

    // ---------------- -F / fuzz ----------------

    /// A hunk with `n` context lines either side of a one-line change.
    fn fuzz_hunk() -> Hunk {
        Hunk {
            old_start: 1,
            old_count: 7,
            new_start: 1,
            new_count: 7,
            lines: vec![
                HunkLine::Context("c1".into()),
                HunkLine::Context("c2".into()),
                HunkLine::Context("c3".into()),
                HunkLine::Remove("mid".into()),
                HunkLine::Add("MID".into()),
                HunkLine::Context("c4".into()),
                HunkLine::Context("c5".into()),
                HunkLine::Context("c6".into()),
            ],
        }
    }

    /// The fuzz budget is PER END, not per hunk. Two wrong context lines at
    /// opposite ends still cost only fuzz 1 -- measured against GNU, and the
    /// row that makes "at each end" the right reading.
    #[test]
    fn fuzz_is_budgeted_at_each_end_not_across_the_hunk() {
        let h = fuzz_hunk();
        // Outermost leading wrong.
        let one = lines(&["XX", "c2", "c3", "mid", "c4", "c5", "c6"]);
        assert!(!try_hunk_at(&one, &h, 0, false, 0), "fuzz 0 must refuse");
        assert!(try_hunk_at(&one, &h, 0, false, 1), "fuzz 1 must accept");

        // Outermost wrong at BOTH ends: still fuzz 1, because each end has
        // its own budget. Under a per-hunk budget this would need fuzz 2.
        let both = lines(&["XX", "c2", "c3", "mid", "c4", "c5", "YY"]);
        assert!(!try_hunk_at(&both, &h, 0, false, 0));
        assert!(
            try_hunk_at(&both, &h, 0, false, 1),
            "one per end, not one in total"
        );

        // Two deep on the leading side needs 2.
        let two = lines(&["XX", "ZZ", "c3", "mid", "c4", "c5", "YY"]);
        assert!(!try_hunk_at(&two, &h, 0, false, 1));
        assert!(try_hunk_at(&two, &h, 0, false, 2));
    }

    /// Fuzz never excuses a REMOVED line. Measured: GNU refuses a perturbed
    /// removal at `-F3`, and it must, because that is a line the hunk is
    /// about to delete -- applying anyway would destroy content the patch
    /// never matched.
    #[test]
    fn fuzz_never_excuses_a_removed_line() {
        let h = fuzz_hunk();
        let bad = lines(&["c1", "c2", "c3", "WRONG", "c4", "c5", "c6"]);
        for fuzz in 0..=9 {
            assert!(
                !try_hunk_at(&bad, &h, 0, false, fuzz),
                "fuzz {fuzz} must not excuse a removal"
            );
        }
    }

    /// Fuzz is capped by how much context there actually is, so a hunk that
    /// opens with a removal gets no leading fuzz at all however large `-F` is.
    #[test]
    fn fuzz_is_capped_by_the_context_a_hunk_actually_has() {
        let h = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Remove("first".into()),
                HunkLine::Add("FIRST".into()),
                HunkLine::Context("c1".into()),
                HunkLine::Context("c2".into()),
            ],
        };
        assert_eq!(leading_context(&h), 0, "it opens with a removal");
        assert_eq!(trailing_context(&h), 2);

        let bad = lines(&["WRONG", "c1", "c2"]);
        assert!(!try_hunk_at(&bad, &h, 0, false, 99));

        // The trailing end still fuzzes normally.
        let tail_wrong = lines(&["first", "c1", "YY"]);
        assert!(!try_hunk_at(&tail_wrong, &h, 0, false, 0));
        assert!(try_hunk_at(&tail_wrong, &h, 0, false, 1));
    }

    /// A fuzzed context line keeps what the FILE holds there, and the fuzz
    /// level is reported back so the caller can say `with fuzz N` and decide
    /// whether to write a `.orig`.
    #[test]
    fn a_fuzzed_context_line_keeps_the_files_text_and_reports_its_level() {
        let h = fuzz_hunk();
        let file = lines(&["XX", "c2", "c3", "mid", "c4", "c5", "c6"]);
        let a = apply_hunk(&file, &h, 0, false, 2).expect("fuzz 1 applies");
        let (out, fuzz_used) = (a.lines, a.fuzz);
        assert_eq!(fuzz_used, 1);
        // `XX` survives: the hunk did not match it and does not get to
        // replace it with the `c1` the patch spells.
        assert_eq!(out, lines(&["XX", "c2", "c3", "MID", "c4", "c5", "c6"]));
    }

    /// Both spellings of the flag, including the glued one this build used to
    /// answer `invalid option -- 'F'` to.
    #[test]
    fn fuzz_parses_in_every_spelling_gnu_takes() {
        assert_eq!(parse_args(&s(&["-F1"])).unwrap().fuzz, Some(1));
        assert_eq!(parse_args(&s(&["-F", "1"])).unwrap().fuzz, Some(1));
        assert_eq!(parse_args(&s(&["--fuzz=1"])).unwrap().fuzz, Some(1));
        assert_eq!(parse_args(&s(&["--fuzz", "1"])).unwrap().fuzz, Some(1));
        assert_eq!(parse_args(&s(&["-F0"])).unwrap().fuzz, Some(0));
        // Unasked-for, it stays None so the default of 2 applies.
        assert_eq!(parse_args(&s(&[])).unwrap().fuzz, None);
    }

    // ---------------- -l / --ignore-whitespace ----------------

    /// The seven cases measured against GNU with a context line of
    /// `    indented a b`. The table in `loose_eq`'s doc comment is this list,
    /// and this test is what keeps the two honest.
    #[test]
    fn loose_eq_matches_gnu_on_whitespace_runs() {
        let base = &b"    indented a b"[..];
        for (target, want, label) in [
            (&b"\tindented a b"[..], true, "tab for four spaces"),
            (&b"        indented a b"[..], true, "eight spaces for four"),
            (&b"    indented  a  b"[..], true, "internal run lengthened"),
            (
                &b"    indented a b    "[..],
                true,
                "trailing whitespace added",
            ),
            (&b"indented a b"[..], false, "leading run REMOVED"),
            (&b"    indenteda b"[..], false, "internal run REMOVED"),
            (&b"    indented a c"[..], false, "a real byte differs"),
        ] {
            assert_eq!(loose_eq(target, base), want, "{label}");
            // Matching must not depend on which side came from the patch.
            assert_eq!(loose_eq(base, target), want, "{label}, reversed");
        }
    }

    /// Whitespace for `-l` is SPACE and TAB only. Vertical tab, form feed and
    /// carriage return are NOT whitespace here -- measured against GNU, not
    /// assumed. `is_ascii_whitespace()` would have been wrong in three ways,
    /// and wrong invisibly: no fixture in this tree contains those bytes.
    #[test]
    fn loose_eq_counts_only_space_and_tab_as_whitespace() {
        assert!(loose_eq(b"A\tB", b"A B"), "tab IS whitespace");
        for (target, label) in [
            (&b"A\x0bB"[..], "vertical tab"),
            (&b"A\x0cB"[..], "form feed"),
            (&b"A\rB"[..], "carriage return"),
            (&b"A\xa0B"[..], "nbsp, which is not an ASCII space"),
        ] {
            assert!(!loose_eq(target, b"A B"), "{label} must NOT be whitespace");
        }
    }

    /// A line that is ENTIRELY whitespace matches an empty one. That looks
    /// like a counterexample to "a run never matches nothing" and is not: the
    /// trailing strip eats the whole line, so no run is left to match.
    #[test]
    fn loose_eq_treats_an_all_whitespace_line_as_empty() {
        for (a, b, want) in [
            (&b"    "[..], &b"\t"[..], true),
            (&b"    "[..], &b""[..], true),
            (&b"    "[..], &b"  "[..], true),
            (&b"    "[..], &b"x"[..], false),
            (&b""[..], &b"    "[..], true),
            (&b""[..], &b""[..], true),
        ] {
            assert_eq!(loose_eq(a, b), want, "{a:?} vs {b:?}");
        }
    }

    /// End to end: a hunk whose context differs from the file in whitespace
    /// alone applies under `-l` and is refused without it. This is the
    /// behaviour `--help` has been advertising all along.
    #[test]
    fn a_hunk_differing_only_in_whitespace_applies_under_l_and_not_without() {
        // The file is indented with a TAB; the patch was made against a copy
        // indented with four SPACES.
        let file = lines(&["start", "\tindented a b", "end"]);
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("start".into()),
                HunkLine::Remove("    indented a b".into()),
                HunkLine::Add("CHANGED".into()),
                HunkLine::Context("end".into()),
            ],
        };

        assert!(
            apply_hunk(&file, &hunk, 0, false, 0).is_none(),
            "exact matching must still refuse a whitespace difference"
        );
        let out = apply_hunk(&file, &hunk, 0, true, 0)
            .expect("-l must apply it")
            .lines;
        // GNU writes the PATCH's replacement text, not the file's -- measured.
        assert_eq!(out, lines(&["start", "CHANGED", "end"]));
    }

    /// Both spellings reach the flag, and it is off unless asked for.
    #[test]
    fn l_and_ignore_whitespace_both_set_the_flag() {
        assert!(parse_args(&s(&["-l"])).unwrap().ignore_whitespace);
        assert!(
            parse_args(&s(&["--ignore-whitespace"]))
                .unwrap()
                .ignore_whitespace
        );
        assert!(!parse_args(&s(&[])).unwrap().ignore_whitespace);
    }

    #[test]
    fn apply_hunk_modifies_buffer() {
        let l = lines(&["line1", "line2", "line3"]);
        let a = apply_hunk(&l, &modify_hunk(), 0, false, 0).unwrap();
        let (new_lines, new_offset) = (a.lines, a.offset);
        assert_eq!(new_lines, lines(&["line1", "line2 modified", "line3"]));
        assert_eq!(new_offset, 0); // new_count(3) - old_count(3) = 0
    }

    #[test]
    fn apply_hunk_returns_none_on_mismatch() {
        let l = lines(&["nope", "nope", "nope"]);
        assert!(apply_hunk(&l, &modify_hunk(), 0, false, 0).is_none());
    }

    #[test]
    fn apply_hunk_slides_to_find_its_match() {
        // A blank prefix line: the hunk header says 1 and the match is at 2.
        // This is SLIDING, not fuzz -- every line still matches exactly, the
        // hunk just sits elsewhere -- which is why it passes fuzz 0. The test
        // was called `apply_hunk_finds_via_fuzz` while the constant it
        // exercised was called `max_fuzz`, and both names belonged to the
        // other mechanism.
        let l = lines(&["blank", "line1", "line2", "line3"]);
        let new_lines = apply_hunk(&l, &modify_hunk(), 0, false, 0).unwrap().lines;
        assert_eq!(
            new_lines,
            lines(&["blank", "line1", "line2 modified", "line3"])
        );
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
        let a = apply_hunk(&l, &h, 0, false, 0).unwrap();
        let (new_lines, offset) = (a.lines, a.offset);
        assert_eq!(new_lines, lines(&["a", "b", "c"]));
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
        let ps = parse_patch(patch.replace('~', "\n").as_bytes());
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
        let ps = parse_patch(patch.replace('~', "\n").as_bytes());
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
        let ps = parse_patch(patch.replace('~', "\n").as_bytes());
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
        let ps = parse_patch(patch.replace('~', "\n").as_bytes());
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
            patch
                .replace('~', "\n")
                .replace('#', "+")
                .replace('$', "\\")
                .as_bytes(),
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
        assert_eq!(detect_dialect(ctx(CTX).as_bytes()), Dialect::Context);
        assert_eq!(detect_dialect(SIMPLE_PATCH.as_bytes()), Dialect::Unified);
        assert_eq!(
            detect_dialect(ctx("2c2~< bravo~---~> BRAVO~").as_bytes()),
            Dialect::Normal
        );
        assert_eq!(detect_dialect(b"this is not a patch"), Dialect::Unknown);
        assert_eq!(detect_dialect(b""), Dialect::Unknown);
    }

    /// THE ORDER OF THE TESTS IS THE WHOLE OF IT. A context diff's SECOND
    /// header line is `--- y/a/base.txt`, which is also how a unified diff's
    /// FIRST one starts. A detector that looked for `--- ` first would call
    /// every context patch a malformed unified one and report garbage.
    #[test]
    fn a_context_header_is_not_read_as_a_unified_one() {
        let headers_only = ctx("*** x/a/base.txt~--- y/a/base.txt~");
        assert_eq!(detect_dialect(headers_only.as_bytes()), Dialect::Context);
    }

    #[test]
    fn a_context_hunk_becomes_one_removal_and_one_addition() {
        let ps = parse_context_patch(ctx(CTX).as_bytes());
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].old_path, b"x/a/base.txt");
        assert_eq!(ps[0].new_path, b"y/a/base.txt");
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
        let h = &parse_context_patch(shifted.as_bytes())[0].hunks[0];
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
        let h = &parse_context_patch(del.as_bytes())[0].hunks[0];
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
        let h = &parse_context_patch(add.as_bytes())[0].hunks[0];
        assert_eq!(
            h.lines,
            vec![HunkLine::Context("a".into()), HunkLine::Add("b".into())]
        );
    }

    #[test]
    fn normal_command_lines_parse() {
        assert_eq!(parse_normal_command(b"2c2"), Some((2, 2, b'c', 2, 2)));
        assert_eq!(parse_normal_command(b"1,3d0"), Some((1, 3, b'd', 0, 0)));
        assert_eq!(parse_normal_command(b"4a5,7"), Some((4, 4, b'a', 5, 7)));
        assert_eq!(parse_normal_command(b"2,4c3,5"), Some((2, 4, b'c', 3, 5)));
        // Not command lines, and each would be a plausible false positive.
        assert_eq!(parse_normal_command(b"< bravo"), None);
        assert_eq!(parse_normal_command(b"---"), None);
        assert_eq!(parse_normal_command(b"alpha"), None);
        assert_eq!(parse_normal_command(b"c2"), None);
        assert_eq!(parse_normal_command(b"2c"), None);
    }

    #[test]
    fn a_normal_change_becomes_a_removal_and_an_addition() {
        let ps = parse_normal_patch(ctx("2c2~< bravo~---~> BRAVO~").as_bytes());
        assert_eq!(ps.len(), 1);
        // A normal diff names no file at all, which is why patch requires the
        // target as an operand.
        assert_eq!(ps[0].old_path, b"");
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

    /// `4a5` ADDS AFTER line four and removes nothing, so the hunk NAMES four
    /// and `apply_hunk` inserts at four rather than at three.
    ///
    /// THIS TEST PINNED THE BUG, not the contract. It used to assert
    /// `old_start == 5`, because the parser added one here to compensate for
    /// `apply_hunk` subtracting one from every hunk including insertions. That
    /// was right for normal diffs and left zero-context UNIFIED insertions
    /// broken -- `@@ -1,0 +2 @@` put the line one row too high, which GNU does
    /// not. The compensation is gone and the applier handles `old_count == 0`
    /// for every dialect.
    ///
    /// So it now asserts the parse AND the applied result. Asserting the
    /// internal offset alone is what let a compensating pair of errors look
    /// correct from inside: the two halves agreed with each other and with
    /// nothing else.
    #[test]
    fn append_inserts_after_the_line_it_names() {
        let h = &parse_normal_patch(ctx("4a5~> new~").as_bytes())[0].hunks[0];
        assert_eq!(h.old_start, 4);
        assert_eq!(h.old_count, 0);
        assert_eq!(h.lines, vec![HunkLine::Add("new".into())]);

        // The observable half: applied to five lines, `new` lands fifth.
        let original = lines(&["1", "2", "3", "4", "5"]);
        let out = apply_hunk(&original, h, 0, false, 0)
            .expect("a pure insertion applies")
            .lines;
        assert_eq!(out, lines(&["1", "2", "3", "4", "new", "5"]));
    }

    /// The same rule through the UNIFIED door, which is the one that was
    /// wrong: `diff -U0` of a single added line after line one.
    #[test]
    fn a_zero_context_unified_insertion_lands_where_gnu_puts_it() {
        let ps = parse_patch(ctx("--- x/f.txt~+++ y/f.txt~@@ -1,0 +2 @@~+X~").as_bytes());
        let h = &ps[0].hunks[0];
        assert_eq!(h.old_count, 0, "a -U0 insertion removes nothing");
        let out = apply_hunk(&lines(&["a", "b", "c"]), h, 0, false, 0)
            .expect("a pure insertion applies")
            .lines;
        // Measured against GNU patch 2.7.6: `a X b c`, not `X a b c`.
        assert_eq!(out, lines(&["a", "X", "b", "c"]));
    }

    /// ...and `1,2d0` deletes without adding, so the NEW side is empty and
    /// starts after the named zero.
    #[test]
    fn delete_leaves_the_new_side_empty() {
        let h = &parse_normal_patch(ctx("1,2d0~< a~< b~").as_bytes())[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 2);
        assert_eq!(h.new_count, 0);
        assert_eq!(
            h.lines,
            vec![HunkLine::Remove("a".into()), HunkLine::Remove("b".into())]
        );
    }
}
