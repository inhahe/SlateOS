//! diff -- compare files line by line.
//!
//! PORTED FROM `userspace/diff` ON 2026-09-12, under design-decisions.md
//! 1005: coreutils is the one home, and the better half of each duplicate
//! pair survives inside it. This is the ONE pair of the eight where the
//! better half was the standalone, so it is a port in rather than a
//! deletion. Measured the same day on the same 107 cases against the same
//! GNU reference, in one job:
//!
//!     coreutils diff    21 passed, 86 differed
//!     userspace/diff    43 passed, 64 differed
//!
//! The survey ranks pairs by how many options each side MENTIONS, and on
//! that reading this one is 414 to 1541 -- the same direction, by luck.
//! Its own header records that the count has been wrong every time it was
//! checked, twice backwards, which is why the harness decided it.
//!
//! `--json` is deliberately NOT carried across. GNU diff has no such
//! option, so porting it would make coreutils GAIN an invention it did not
//! have -- and the half under differential test is the worst place to put
//! one, because every case that exercises it is a case the reference
//! cannot answer. Same call as `chown`'s `--json`, in the other direction:
//! there the invention died with the deleted crate, here it is dropped in
//! transit. Neither adds one to coreutils.
//!
//! WHAT THE OLD IMPLEMENTATION HAD THAT THIS DID NOT: tests. It carried
//! fourteen and the standalone carried none, so a straight copy would have
//! deleted every test this utility has. That is the `patch` lesson pointing
//! the other way -- there the losing half knew something the winner did
//! not, here the winner did. Both halves of a duplicate are witnesses.
//! See the test module at the bottom for which survived and which could
//! not.
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
//!   -Z, --ignore-trailing-space Ignore whitespace at line end
//!   -a, --text                  Treat all files as text
//!   -B, --ignore-blank-lines    Ignore blank line insertions/deletions
//!   -t, --expand-tabs           Expand tabs to spaces in the output
//!   -T, --initial-tab           Put a tab, not a space, after the marker
//!       --color[=WHEN]          Color the output: never, always or auto
//!   -r, --recursive             Recursively compare directories
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

use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::stdfd;
use quoting::{quoteaf_os, quotef_os};
use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
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
    /// An `ed` script (`-e`, `--ed`): commands that turn file 1 into file 2.
    Ed,
    /// RCS's own diff format (`-n`, `--rcs`).
    Rcs,
}

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    path1: OsString,
    path2: OsString,
    format: Format,
    context_lines: usize,
    width: usize,
    brief: bool,
    /// `-I RE`: a change whose lines ALL match one of these is not a change.
    ///
    /// Compiled once at parse time rather than per line, and held as BREs
    /// because that is the dialect GNU uses here -- measured: `-I 'o\|O'`
    /// ignores a hunk and `-I 'o|O'` does not, which is BRE alternation
    /// working and ERE alternation not.
    ignore_matching: Vec<ere::Regex>,
    report_identical: bool,
    ignore_case: bool,
    ignore_space_change: bool,
    ignore_all_space: bool,
    ignore_blank_lines: bool,
    color: bool,
    recursive: bool,
    new_file: bool,
    /// `-Z`: whitespace at the END of a line is not a difference.
    ///
    /// Narrower than `-b`, which collapses runs anywhere, and much narrower
    /// than `-w`. A file that differs only in trailing spaces is the common
    /// case an editor creates, and GNU exits 0 on it under `-Z`.
    ignore_trailing_space: bool,
    /// `-a`: compare even a file holding NUL bytes as text.
    text_mode: bool,
    /// `-t`: expand tabs in the OUTPUT to 8-column stops.
    ///
    /// The stops are counted from the start of the line's own text, NOT from
    /// the start of the output line -- the `< ` marker is not counted. So
    /// `a<TAB>b` comes back as `< a` plus seven spaces, exactly as it would
    /// without a marker at all.
    expand_tabs: bool,
    /// `-T`: separate the marker from the text with a tab rather than a space.
    ///
    /// The point is alignment: `<` plus a tab is eight columns wide, which is
    /// one tab stop, so tabs inside the text still land where they would in
    /// the file itself. Pairs with `-t`, but works on its own.
    initial_tab: bool,
    /// `--suppress-common-lines`: under `-y`, print only the lines that
    /// differ.
    suppress_common_lines: bool,
    /// `--left-column`: under `-y`, print a common line in the left column
    /// only, marked `(`.
    left_column: bool,
    /// `--tabsize`: tab stops every this many columns, for `-t` and `-y`.
    tabsize: usize,
    /// The option words exactly as the user typed them, for the `diff -r
    /// da/x.txt db/x.txt` line GNU prints ahead of each file in a directory
    /// walk.
    ///
    /// Kept verbatim rather than reconstructed from the parsed flags,
    /// because GNU echoes the spelling: measured, `-ru` comes back as `-ru`
    /// and `-r -u` as `-r -u`. Rebuilding the line from `Config` would
    /// print one canonical form for both.
    option_words: Vec<OsString>,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
    /// A command line that will not run: everything to print, each line
    /// already behind `diff: `, and the status is always 2.
    Fail(String),
}

// ============================================================================
// Diff operations
// ============================================================================

/// A single edit operation between two sequences.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Op {
    Equal,
    Insert,
    Delete,
}

/// A contiguous group of changes with surrounding context.
/// Whether a file's last line is terminated.
///
/// A `bool` would read as `true`/`false` at four call sites that each have to
/// remember which way round it is. The bug this exists for was two files, 26
/// bytes and 25, reported IDENTICAL -- so the cost of getting the polarity
/// backwards is another silent wrong answer, not a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalNewline {
    Present,
    Missing,
}

/// One line of the edit script, and whether GNU's no-newline marker follows it.
///
/// A tuple until 2026-09-14, when it grew the third field. `diff` reported a
/// file ending in a newline and one that does not as IDENTICAL -- exit 0 on
/// two files of 26 and 25 bytes -- because splitting into lines throws the
/// terminator away and both sides then yield the same four lines.
///
/// The flag rides on the emitted line rather than being recomputed by each
/// renderer because that is where the answer is needed: measured, GNU puts
/// `\ No newline at end of file` immediately after the line it belongs to, on
/// whichever side lacks the newline, and after BOTH when both lack it.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
struct Edit {
    op: Op,
    text: Vec<u8>,
    /// This is the final line of a file with no terminating newline.
    no_final_newline: bool,
    /// For a common line, file 2's text where it is not `text` -- which it
    /// can be only under `-i`, `-b`, `-w` or `-Z`. Only `-y` prints both
    /// files' copies of a common line; every other format prints file 1's.
    other: Option<Vec<u8>>,
}

impl Edit {
    fn new(op: Op, text: Vec<u8>) -> Self {
        Self {
            op,
            text,
            no_final_newline: false,
            other: None,
        }
    }

    /// A common line: `a` as file 1 has it and `b` as file 2 does.
    fn equal(a: &[u8], b: &[u8]) -> Self {
        Self {
            other: (a != b).then(|| b.to_vec()),
            ..Self::new(Op::Equal, a.to_vec())
        }
    }
}

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
    lines: Vec<Edit>,
}

// ============================================================================
// Argument parsing
// ============================================================================

/// GNU's default `--tabsize`.
const DEFAULT_TABSIZE: usize = 8;

/// The largest `--tabsize` upstream takes: `SIZE_MAX - GUTTER_WIDTH_MINIMUM`,
/// so that a tab and the `-y` gutter still fit a `size_t`.
const TABSIZE_MAX: usize = usize::MAX - GUTTER_WIDTH_MINIMUM;

/// Upstream's reading of a `-W` or `--tabsize` value: `strtoimax`, which
/// skips leading white space, takes a sign and saturates, then a whole
/// number in `1..=max` with nothing after it.
fn size_value(text: &[u8], max: usize) -> Option<usize> {
    let start = text
        .iter()
        .position(|&c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .unwrap_or(text.len());
    let body = text.get(start..).unwrap_or_default();
    let (negative, digits) = match body.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, body),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // Saturating at `INTMAX_MAX`, as `strtoimax` does on overflow -- and
    // upstream does not look at `errno`, so the saturated value is taken.
    let value = digits.iter().fold(0u64, |v, &d| {
        v.saturating_mul(10)
            .saturating_add(u64::from(d.wrapping_sub(b'0')))
            .min(i64::MAX.unsigned_abs())
    });
    if negative || value == 0 {
        return None;
    }
    usize::try_from(value).ok().filter(|&v| v <= max)
}

/// Set `slot` from a `-W` or `--tabsize` value, or exit as upstream does:
/// `invalid width 'x'` with the referral for a bad value, `conflicting width
/// options` without one for a second value that disagrees with the first.
fn set_size_option(
    slot: &mut Option<usize>,
    value: &OsString,
    max: usize,
    what: &str,
) -> Result<(), String> {
    let bytes = quoting::os_bytes(value);
    let Some(n) = size_value(&bytes, max) else {
        return Err(try_help(&format!("invalid {what} {}", quoteaf_os(value))));
    };
    match *slot {
        Some(old) if old != n => Err(fatal(&format!("conflicting {what} options"))),
        _ => {
            *slot = Some(n);
            Ok(())
        }
    }
}

/// `diff`'s name and the status of a command line it will not run: 2,
/// upstream's `EXIT_TROUBLE`, which `try_help` and `fatal` both exit with.
const DIFF: Program = Program::new("diff", 2);

/// diffutils 3.10's `shortopts`, verbatim.
const SHORT_OPTIONS: &str = "0123456789abBcC:dD:eEfF:hHiI:lL:nNpPqrsS:tTuU:vwW:x:X:yZ";

/// diffutils 3.10's `longopts`, **in declaration order**, which is observable:
/// an ambiguous abbreviation lists its candidates in table order. The last two
/// are upstream's undocumented ones, spelled with a third dash (`---no-directory`
/// for `diff3`, `---presume-output-tty` for its tests); the shared parser reads
/// them the same way glibc does, as a long option whose name begins with `-`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("binary", Takes::Nothing),
    ("brief", Takes::Nothing),
    ("changed-group-format", Takes::Required),
    ("color", Takes::Optional),
    ("context", Takes::Optional),
    ("ed", Takes::Nothing),
    ("exclude", Takes::Required),
    ("exclude-from", Takes::Required),
    ("expand-tabs", Takes::Nothing),
    ("forward-ed", Takes::Nothing),
    ("from-file", Takes::Required),
    ("help", Takes::Nothing),
    ("horizon-lines", Takes::Required),
    ("ifdef", Takes::Required),
    ("ignore-all-space", Takes::Nothing),
    ("ignore-blank-lines", Takes::Nothing),
    ("ignore-case", Takes::Nothing),
    ("ignore-file-name-case", Takes::Nothing),
    ("ignore-matching-lines", Takes::Required),
    ("ignore-space-change", Takes::Nothing),
    ("ignore-tab-expansion", Takes::Nothing),
    ("ignore-trailing-space", Takes::Nothing),
    ("inhibit-hunk-merge", Takes::Nothing),
    ("initial-tab", Takes::Nothing),
    ("label", Takes::Required),
    ("left-column", Takes::Nothing),
    ("line-format", Takes::Required),
    ("minimal", Takes::Nothing),
    ("new-file", Takes::Nothing),
    ("new-group-format", Takes::Required),
    ("new-line-format", Takes::Required),
    ("no-dereference", Takes::Nothing),
    ("no-ignore-file-name-case", Takes::Nothing),
    ("normal", Takes::Nothing),
    ("old-group-format", Takes::Required),
    ("old-line-format", Takes::Required),
    ("paginate", Takes::Nothing),
    ("palette", Takes::Required),
    ("rcs", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("report-identical-files", Takes::Nothing),
    ("sdiff-merge-assist", Takes::Nothing),
    ("show-c-function", Takes::Nothing),
    ("show-function-line", Takes::Required),
    ("side-by-side", Takes::Nothing),
    ("speed-large-files", Takes::Nothing),
    ("starting-file", Takes::Required),
    ("strip-trailing-cr", Takes::Nothing),
    ("suppress-blank-empty", Takes::Nothing),
    ("suppress-common-lines", Takes::Nothing),
    ("tabsize", Takes::Required),
    ("text", Takes::Nothing),
    ("to-file", Takes::Required),
    ("unchanged-group-format", Takes::Required),
    ("unchanged-line-format", Takes::Required),
    ("unidirectional-new-file", Takes::Nothing),
    ("unified", Takes::Optional),
    ("version", Takes::Nothing),
    ("width", Takes::Required),
    ("-no-directory", Takes::Nothing),
    ("-presume-output-tty", Takes::Nothing),
];

/// Upstream's `CONTEXT_MAX`, `(LIN_MAX - 1) / 2` with `lin` a `ptrdiff_t`: the
/// most context lines anything can ask for. Larger requests are clamped to it.
const CONTEXT_MAX: u64 = (i64::MAX.unsigned_abs() - 1) / 2;

/// `--color`'s three answers: upstream's `colors_style`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorStyle {
    Never,
    Auto,
    Always,
}

/// Upstream's `try_help`: the diagnostic, then the referral, each behind
/// `diff: `.
fn try_help(sentence: &str) -> String {
    format!("diff: {sentence}\ndiff: Try 'diff --help' for more information.")
}

/// Upstream's `fatal`: the diagnostic alone.
fn fatal(sentence: &str) -> String {
    format!("diff: {sentence}")
}

/// An option diffutils has and this `diff` does not.
///
/// Refused by name, never ignored: each changes the output -- `-D` writes
/// `#ifdef`s, the `--*-format`s rewrite every line, `-L` renames the headers,
/// `-x` skips files -- and a `diff` that ignored one would print something
/// other than what was asked for.
fn unimplemented(item: &Opt<'_>) -> String {
    match item {
        Opt::Short(flag, _) => try_help(&format!(
            "option -{} is not implemented by this diff",
            char::from(*flag)
        )),
        Opt::Long(name, _) => try_help(&format!(
            "option '--{name}' is not implemented by this diff"
        )),
        Opt::Operand(_) => try_help("internal error: an operand reached the option refusal"),
    }
}

/// Upstream's `strtoimax (optarg, &numend, 10)` followed by `*numend` being
/// the only test of the whole string: white space may lead, a sign may too,
/// and nothing may trail. `Some` is the value, saturated as `strtoimax`
/// saturates; `None` is a string with a trailing byte or no digits -- except
/// the *empty* string, which `strtoimax` answers with 0 and an `numend` that
/// already points at the terminating NUL, so it reads as zero.
fn strtoimax_whole(text: &[u8]) -> Option<i64> {
    if text.is_empty() {
        return Some(0);
    }
    let start = text
        .iter()
        .position(|&c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .unwrap_or(text.len());
    let body = text.get(start..).unwrap_or_default();
    let (negative, digits) = match body.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, body),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let magnitude = digits.iter().fold(0u64, |v, &d| {
        v.saturating_mul(10)
            .saturating_add(u64::from(d.wrapping_sub(b'0')))
            .min(i64::MAX.unsigned_abs())
    });
    let value = i64::try_from(magnitude).unwrap_or(i64::MAX);
    Some(if negative {
        value.saturating_neg()
    } else {
        value
    })
}

/// `-C`/`-U`/`--context`/`--unified`'s value: 3 when there is none, and
/// otherwise a whole non-negative number, clamped to [`CONTEXT_MAX`].
fn context_value(value: Option<&OsString>) -> Result<u64, String> {
    let Some(value) = value else {
        return Ok(3);
    };
    match strtoimax_whole(&quoting::os_bytes(value)) {
        Some(n) if n >= 0 => Ok(n.unsigned_abs().min(CONTEXT_MAX)),
        _ => Err(try_help(&format!(
            "invalid context length {}",
            quoteaf_os(value)
        ))),
    }
}

/// Upstream's `specify_style`: the first style asked for, and any different
/// one after it refused.
fn specify_style(style: &mut Option<Format>, wanted: Format) -> Result<(), String> {
    match *style {
        Some(current) if current != wanted => Err(try_help("conflicting output style options")),
        _ => {
            *style = Some(wanted);
            Ok(())
        }
    }
}

/// Parse argv -- `args[0]` is the program name -- the way upstream's `main`
/// does, on the shared parser.
///
/// So long options abbreviate to any unique prefix (`--unif`), `--` ends the
/// options, and `POSIXLY_CORRECT` stops them at the first operand, none of
/// which the ladder of exact spellings this replaces did. And upstream's rules
/// come with it: two different output styles are `conflicting output style
/// options` (the ladder kept the last), repeated context lengths keep the
/// largest, and the obsolete `-NUM` digits accumulate across words.
#[allow(clippy::too_many_lines)]
fn parse_args(args: &[OsString]) -> ParseResult {
    let words = args.get(1..).unwrap_or_default();

    let mut style: Option<Format> = None;
    // Upstream's `context` (the largest asked for), `explicit_context` (whether
    // `-C`/`-U` asked at all) and `ocontext` (the obsolete `-NUM` digits).
    let mut context: u64 = 0;
    let mut explicit_context = false;
    let mut ocontext: Option<u64> = None;
    // Upstream's `prev`: whether the previous *option* was a digit. An operand
    // is skipped by getopt, so it neither sets nor clears it.
    let mut prev_digit = false;

    let mut width: Option<usize> = None;
    let mut tabsize: Option<usize> = None;
    let mut color_style = ColorStyle::Never;
    let mut presume_output_tty = false;
    let mut suppress_common_lines = false;
    let mut left_column = false;
    let mut brief = false;
    let mut ignore_matching: Vec<ere::Regex> = Vec::new();
    let mut report_identical = false;
    let mut ignore_case = false;
    let mut ignore_space_change = false;
    let mut ignore_all_space = false;
    let mut ignore_blank_lines = false;
    let mut recursive = false;
    let mut new_file = false;
    let mut ignore_trailing_space = false;
    let mut text_mode = false;
    let mut expand_tabs = false;
    let mut initial_tab = false;
    // Every operand with its index in `words`, so that the option words can be
    // told from it by position rather than by content: `diff -U 5 5 other`
    // names a file called `5`.
    let mut operands: Vec<(usize, OsString)> = Vec::new();

    let mut parser = DIFF.parse(words, SHORT_OPTIONS, LONG_OPTIONS);
    while let Some(item) = parser.next() {
        let item = match item {
            Ok(item) => item,
            Err(e) => return ParseResult::Fail(try_help(&e.sentence)),
        };
        let mut this_is_a_digit = false;
        let step: Result<(), String> = match &item {
            Opt::Operand(word) => {
                operands.push((parser.optind().saturating_sub(1), (*word).clone()));
                continue;
            }
            Opt::Short(d @ b'0'..=b'9', _) => {
                // Upstream's arithmetic, including the clamp: a digit
                // continues the previous option's number only if that option
                // was a digit too, so `-1 -2` is twelve.
                let digit = u64::from(d.wrapping_sub(b'0'));
                ocontext = Some(match ocontext {
                    Some(oc) if prev_digit => {
                        if oc.saturating_sub(u64::from(digit <= CONTEXT_MAX % 10))
                            < CONTEXT_MAX / 10
                        {
                            oc.saturating_mul(10).saturating_add(digit)
                        } else {
                            CONTEXT_MAX
                        }
                    }
                    _ => digit,
                });
                this_is_a_digit = true;
                Ok(())
            }
            Opt::Short(b'a', _) | Opt::Long("text", _) => {
                text_mode = true;
                Ok(())
            }
            Opt::Short(b'b', _) | Opt::Long("ignore-space-change", _) => {
                ignore_space_change = true;
                Ok(())
            }
            Opt::Short(b'Z', _) | Opt::Long("ignore-trailing-space", _) => {
                ignore_trailing_space = true;
                Ok(())
            }
            Opt::Short(b'B', _) | Opt::Long("ignore-blank-lines", _) => {
                ignore_blank_lines = true;
                Ok(())
            }
            Opt::Short(b'C' | b'U', value) | Opt::Long("context" | "unified", value) => {
                let wanted = if matches!(item, Opt::Short(b'U', _) | Opt::Long("unified", _)) {
                    Format::Unified
                } else {
                    Format::Context
                };
                context_value(value.as_ref()).and_then(|n| {
                    specify_style(&mut style, wanted)?;
                    context = context.max(n);
                    explicit_context = true;
                    Ok(())
                })
            }
            Opt::Short(b'c', _) => specify_style(&mut style, Format::Context).map(|()| {
                context = context.max(3);
            }),
            Opt::Short(b'u', _) => specify_style(&mut style, Format::Unified).map(|()| {
                context = context.max(3);
            }),
            // Upstream's `-d` asks for a minimal diff, and this one always
            // computes one; `-h` "currently has no effect" upstream either;
            // `-H` and `--horizon-lines` tune upstream's heuristics, which
            // this build does not have; `--inhibit-hunk-merge` is obsolete
            // upstream and accepted for compatibility; `--binary` matters only
            // where `O_BINARY` does. Accepting them is the implementation.
            Opt::Short(b'd' | b'h' | b'H', _)
            | Opt::Long("minimal" | "speed-large-files" | "inhibit-hunk-merge" | "binary", _) => {
                Ok(())
            }
            Opt::Long("horizon-lines", value) => {
                let text = value.clone().unwrap_or_default();
                match strtoimax_whole(&quoting::os_bytes(&text)) {
                    Some(n) if n >= 0 => Ok(()),
                    _ => Err(try_help(&format!(
                        "invalid horizon length {}",
                        quoteaf_os(&text)
                    ))),
                }
            }
            Opt::Short(b'e', _) | Opt::Long("ed", _) => specify_style(&mut style, Format::Ed),
            Opt::Short(b'n', _) | Opt::Long("rcs", _) => specify_style(&mut style, Format::Rcs),
            Opt::Long("normal", _) => specify_style(&mut style, Format::Normal),
            Opt::Short(b'y', _) | Opt::Long("side-by-side", _) => {
                specify_style(&mut style, Format::SideBySide)
            }
            Opt::Short(b'i', _) | Opt::Long("ignore-case", _) => {
                ignore_case = true;
                Ok(())
            }
            Opt::Short(b'I', value) | Opt::Long("ignore-matching-lines", value) => {
                compile_ignore_pattern(&value.clone().unwrap_or_default())
                    .map(|re| ignore_matching.push(re))
            }
            Opt::Short(b'N', _) | Opt::Long("new-file", _) => {
                new_file = true;
                Ok(())
            }
            Opt::Short(b'q', _) | Opt::Long("brief", _) => {
                brief = true;
                Ok(())
            }
            Opt::Short(b'r', _) | Opt::Long("recursive", _) => {
                recursive = true;
                Ok(())
            }
            Opt::Short(b's', _) | Opt::Long("report-identical-files", _) => {
                report_identical = true;
                Ok(())
            }
            Opt::Short(b't', _) | Opt::Long("expand-tabs", _) => {
                expand_tabs = true;
                Ok(())
            }
            Opt::Short(b'T', _) | Opt::Long("initial-tab", _) => {
                initial_tab = true;
                Ok(())
            }
            Opt::Short(b'v', _) | Opt::Long("version", _) => return ParseResult::Version,
            Opt::Long("help", _) => return ParseResult::Help,
            Opt::Short(b'w', _) | Opt::Long("ignore-all-space", _) => {
                ignore_all_space = true;
                Ok(())
            }
            Opt::Short(b'W', value) | Opt::Long("width", value) => set_size_option(
                &mut width,
                &value.clone().unwrap_or_default(),
                usize::MAX,
                "width",
            ),
            Opt::Long("tabsize", value) => set_size_option(
                &mut tabsize,
                &value.clone().unwrap_or_default(),
                TABSIZE_MAX,
                "tabsize",
            ),
            Opt::Long("left-column", _) => {
                left_column = true;
                Ok(())
            }
            Opt::Long("suppress-common-lines", _) => {
                suppress_common_lines = true;
                Ok(())
            }
            Opt::Long("color", value) => {
                // Upstream's `specify_colors_style`: the three words exactly,
                // no abbreviation, and no value at all means `auto`.
                match value
                    .as_ref()
                    .map(|v| quoting::os_bytes(v).into_owned())
                    .as_deref()
                {
                    None | Some(b"auto") => {
                        color_style = ColorStyle::Auto;
                        Ok(())
                    }
                    Some(b"always") => {
                        color_style = ColorStyle::Always;
                        Ok(())
                    }
                    Some(b"never") => {
                        color_style = ColorStyle::Never;
                        Ok(())
                    }
                    Some(_) => Err(try_help(&format!(
                        "invalid color {}",
                        quoteaf_os(value.clone().unwrap_or_default())
                    ))),
                }
            }
            Opt::Long("-presume-output-tty", _) => {
                presume_output_tty = true;
                Ok(())
            }
            _ => Err(unimplemented(&item)),
        };
        if let Err(message) = step {
            return ParseResult::Fail(message);
        }
        prev_digit = this_is_a_digit;
    }

    // Upstream: `--color` alone is `auto`, and `auto` on a `dumb` terminal is
    // `never`; otherwise it colours only a terminal, or anything under the
    // testing option that says to presume one.
    let term_is_dumb = std::env::var_os("TERM").is_some_and(|t| t == "dumb");
    let color = match color_style {
        ColorStyle::Always => true,
        ColorStyle::Never => false,
        ColorStyle::Auto => !term_is_dumb && (presume_output_tty || stdfd::is_tty(1)),
    };

    let format = style.unwrap_or(Format::Normal);
    // Upstream's reconciliation of the obsolete `-NUM` with `-C`/`-U`: it
    // counts only for the two styles that have context, and it wins over a
    // context that `-c`/`-u` merely defaulted, but not over an explicit one
    // that is larger.
    if let Some(oc) = ocontext
        && matches!(format, Format::Context | Format::Unified)
        && (context < oc || (oc < context && !explicit_context))
    {
        context = oc;
    }

    // Two operands, exactly. Upstream names the *last word after getopt's
    // permutation* when one is missing: the last operand if there is one, and
    // otherwise the last word of all -- `argv[0]` itself when there is nothing
    // else.
    let [(_, operand1), (_, operand2)] = operands.as_slice() else {
        let message = if let Some((_, extra)) = operands.get(2) {
            format!("extra operand {}", quoteaf_os(extra))
        } else {
            let fallback = OsString::from("diff");
            let last = operands
                .last()
                .map(|(_, w)| w)
                .or_else(|| args.last())
                .unwrap_or(&fallback);
            format!("missing operand after {}", quoteaf_os(last))
        };
        return ParseResult::Fail(try_help(&message));
    };

    // Every word that was not an operand, in the order it was typed: upstream's
    // `argv[1 .. optind)` after its permutation, which `option_list` joins
    // into the header of each recursive comparison.
    let option_words: Vec<OsString> = words
        .iter()
        .enumerate()
        .filter(|(at, _)| !operands.iter().any(|(o, _)| o == at))
        .map(|(_, w)| w.clone())
        .collect();

    ParseResult::Run(Config {
        path1: operand1.clone(),
        path2: operand2.clone(),
        format,
        context_lines: usize::try_from(context).unwrap_or(usize::MAX),
        width: width.unwrap_or(130),
        brief,
        ignore_matching,
        report_identical,
        ignore_case,
        ignore_space_change,
        ignore_all_space,
        ignore_blank_lines,
        color,
        recursive,
        new_file,
        ignore_trailing_space,
        text_mode,
        expand_tabs,
        initial_tab,
        suppress_common_lines,
        left_column,
        tabsize: tabsize.unwrap_or(DEFAULT_TABSIZE),
        option_words,
    })
}

// ============================================================================
// Line normalization for comparison
// ============================================================================

/// Normalize a line for comparison purposes based on the current flags.
fn normalize_line(line: &[u8], config: &Config) -> Vec<u8> {
    // ASCII, not Unicode, and MEASURED rather than conceded. The `char`
    // versions this replaced were each a divergence from GNU:
    //
    //   diff -i  `cafE<U+00C9>` vs `caf<U+00E9>`  GNU: DIFFERENT (not folded)
    //            `ABC` vs `abc`                   GNU: SAME      (folded)
    //   diff -w  `a<U+00A0>b` vs `ab`             GNU: DIFFERENT (not space)
    //            `a b` / `a<TAB>b` vs `ab`        GNU: SAME      (folded)
    //
    // GNU's `tolower`/`isspace` are byte-wise, so it folds ASCII case and
    // ASCII space and nothing else. `str::to_lowercase` and
    // `char::is_whitespace` did more than that, which meant `-i` and `-w`
    // each answered a question GNU answers the other way. Both probes were
    // run with a control pair, so they are known to be sensitive.
    let mut s = line.to_vec();

    if config.ignore_all_space {
        s.retain(|b| !b.is_ascii_whitespace());
    } else if config.ignore_space_change {
        // Collapse runs of whitespace into a single space; trim trailing.
        let mut result: Vec<u8> = Vec::with_capacity(s.len());
        let mut in_space = false;
        for &b in &s {
            if b.is_ascii_whitespace() {
                if !in_space {
                    result.push(b' ');
                    in_space = true;
                }
            } else {
                result.push(b);
                in_space = false;
            }
        }
        // Trim trailing single space that might result from trailing whitespace.
        if result.last() == Some(&b' ') {
            result.pop();
        }
        s = result;
    }

    // `-Z` trims only the END of the line, and runs AFTER the collapsing
    // options above so that `-b -Z` sees what `-b` left. `-w` has already
    // removed every space, so `-Z` finds nothing to do there, which is
    // correct rather than a special case.
    if config.ignore_trailing_space {
        while s.last().is_some_and(u8::is_ascii_whitespace) {
            s.pop();
        }
    }

    if config.ignore_case {
        s.make_ascii_lowercase();
    }

    s
}

/// Whether `line` is blank for `-B`, as upstream's `analyze_hunk` has it:
/// empty -- or, when white space is being ignored too (`-Z`, `-b` or `-w`),
/// nothing but white space. Without one of those, a line of spaces is a
/// line like any other.
fn is_blank(line: &[u8], config: &Config) -> bool {
    if config.ignore_trailing_space || config.ignore_space_change || config.ignore_all_space {
        // C's `isspace`, vertical tab included.
        line.iter()
            .all(|&c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
    } else {
        line.is_empty()
    }
}

// ============================================================================
// File reading
// ============================================================================

/// The lines of `data`, without their terminators.
///
/// `str::lines` over bytes, and the reason `diff` has its own rather than
/// decoding first: a line is file content, and file content is bytes. A
/// trailing newline does NOT produce a final empty line, and a CR before the
/// newline goes with it, both matching `str::lines` -- a DOS file must not
/// report every line as changed against the same file with Unix endings.
fn split_lines(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (line, tail) = match rest.iter().position(|&b| b == b'\n') {
            Some(at) => (rest.get(..at), rest.get(at.saturating_add(1)..)),
            None => (Some(rest), None),
        };
        let Some(line) = line else { break };
        let line = match line.split_last() {
            Some((b'\r', head)) => head,
            _ => line,
        };
        out.push(line.to_vec());
        match tail {
            Some(t) => rest = t,
            None => break,
        }
    }
    out
}

/// Result of reading a file for diffing.
enum FileContent {
    /// Text file, split into lines.
    Text(Vec<Vec<u8>>, FinalNewline),
    /// Binary file detected (contains NUL bytes).
    Binary,
}

/// Read a file into lines. Returns `Err` on I/O errors, `Ok(Binary)` if the
/// file contains NUL bytes, or `Ok(Text(lines))` for normal text files.
fn read_file(path: &Path, text_mode: bool) -> Result<FileContent, String> {
    // `-` is stdin, which has no metadata to size-check and no mtime. It also
    // cannot be read twice, so `diff - -` reads it once and compares the result
    // with an empty second side -- the same thing GNU does with a pipe.
    if is_stdin(path) {
        let mut data = Vec::new();
        io::stdin()
            .read_to_end(&mut data)
            .map_err(|e| format!("-: {}", strerror(&e)))?;
        return Ok(classify(data, text_mode));
    }

    let metadata =
        fs::metadata(path).map_err(|e| format!("{}: {}", path.display(), strerror(&e)))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "{}: file too large ({} bytes, max {})",
            path.display(),
            metadata.len(),
            MAX_FILE_SIZE,
        ));
    }

    let data = fs::read(path).map_err(|e| format!("{}: {}", path.display(), strerror(&e)))?;
    Ok(classify(data, text_mode))
}

/// Whether this operand names standard input.
///
/// A bare `-` only; `./-` is a file called `-`, which is what GNU does and is
/// the reason this is a function rather than a `== "-"` at each site.
fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// A file's bytes, as either binary or a list of lines.
///
/// Shared by the path that reads a file and the one that reads stdin, so that
/// `diff base.txt -` classifies its two sides by the same rule. Before stdin
/// was supported this was read_file's tail and there was only one caller.
fn classify(data: Vec<u8>, text_mode: bool) -> FileContent {
    // Binary is decided on the first BINARY_DETECT_LEN bytes -- unless `-a`
    // says to treat everything as text, in which case the probe is SKIPPED
    // rather than its verdict overridden later. A NUL is then just another
    // byte in a line, which only works because lines are bytes now.
    let check_len = data.len().min(BINARY_DETECT_LEN);
    if !text_mode
        && data
            .get(..check_len)
            .is_some_and(|head| head.contains(&0u8))
    {
        return FileContent::Binary;
    }

    // NO DECODE. The comment that used to stand here said the lossy
    // conversion below was "only for the purpose of displaying diff output;
    // the comparison is byte-accurate via the line strings" -- and that was
    // false in the way that hid the bug: `lines` WAS the lossy text, so the
    // comparison ran on it. Every byte that is not valid UTF-8 became U+FFFD,
    // so two files differing only in distinct bad bytes compared EQUAL and
    // `diff` printed nothing and exited 0 where GNU printed `2c2`.
    //
    // A line is bytes. See known-issues.md ->
    // B-DIFF-SAYS-TWO-DIFFERENT-FILES-ARE-IDENTICAL.
    let lines: Vec<Vec<u8>> = split_lines(&data);
    // An EMPTY file is treated as ending with a newline: it has no last line
    // to mark, and GNU prints no marker for it.
    let final_newline = if data.is_empty() || data.last() == Some(&b'\n') {
        FinalNewline::Present
    } else {
        FinalNewline::Missing
    };

    FileContent::Text(lines, final_newline)
}

// ============================================================================
// Myers diff algorithm (O(ND) shortest edit script)
// ============================================================================

/// Compute the longest common subsequence edit script between two line
/// sequences using the Myers O(ND) algorithm.
///
/// Returns a vector of `(Op, line_text)` pairs describing the full edit
/// sequence from `a` to `b`.
/// Flag the emitted lines that GNU follows with `\\ No newline at end of file`.
///
/// Measured, in all three output formats: the marker goes immediately after
/// the line it belongs to, on whichever side lacks the newline, and after BOTH
/// lines when both files lack one.
///
/// The last line of file A is simply the last edit that came from A -- a
/// `Delete` or an `Equal` -- and the last line of B is the last `Insert` or
/// `Equal`. An `Equal` is the last line of both at once, which is the
/// single-file case and needs no special handling: the flag is set twice on
/// the same edit and the marker prints once.
fn mark_missing_newlines(ops: &mut [Edit], nl_a: FinalNewline, nl_b: FinalNewline) {
    if nl_a == FinalNewline::Missing
        && let Some(e) = ops
            .iter_mut()
            .rev()
            .find(|e| matches!(e.op, Op::Delete | Op::Equal))
    {
        e.no_final_newline = true;
    }
    if nl_b == FinalNewline::Missing
        && let Some(e) = ops
            .iter_mut()
            .rev()
            .find(|e| matches!(e.op, Op::Insert | Op::Equal))
    {
        e.no_final_newline = true;
    }
}

/// GNU's no-newline marker, written after the line it belongs to.
///
/// It carries NO diff prefix -- not `<`, not `-`, not two spaces -- which is
/// why it is written here rather than through `write_body_line`.
fn write_no_newline_marker(w: &mut impl Write) {
    let _ = w.write_all(b"\\ No newline at end of file\n");
}

fn compute_diff(
    a: &[Vec<u8>],
    b: &[Vec<u8>],
    nl_a: FinalNewline,
    nl_b: FinalNewline,
    config: &Config,
) -> Vec<Edit> {
    let n = a.len();
    let m = b.len();

    // Build normalized comparison keys.
    let mut norm_a: Vec<Vec<u8>> = a.iter().map(|l| normalize_line(l, config)).collect();
    let mut norm_b: Vec<Vec<u8>> = b.iter().map(|l| normalize_line(l, config)).collect();

    // A LAST LINE WITH NO NEWLINE IS NOT THE SAME LINE as the same text
    // terminated, and this is the only place that can be said: splitting a
    // file into lines throws the terminator away, so `delta` and `delta\n`
    // arrive here identical and two files of 26 and 25 bytes compared EQUAL.
    //
    // The marker is a newline, which is not a choice of sentinel but the
    // absence of one: a line produced by `split_lines` cannot contain a
    // newline by construction, so no real content can collide with it. It
    // goes on the NORMALISED key only -- `orig_a`/`orig_b` are what gets
    // emitted, so nothing printed is affected.
    //
    // When NEITHER file ends with a newline, both last lines are marked and
    // still compare equal, which is right.
    if nl_a == FinalNewline::Missing
        && let Some(last) = norm_a.last_mut()
    {
        last.push(b'\n');
    }
    if nl_b == FinalNewline::Missing
        && let Some(last) = norm_b.last_mut()
    {
        last.push(b'\n');
    }
    let (norm_a, norm_b) = (norm_a, norm_b);

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
// `indexing_slicing` is allowed on this function and on `myers_diff`, and
// nowhere else in this file.
//
// Every index is a cell in the `dp` table, which is built `(n + 1) x (m + 1)`
// immediately above and walked with `i in 1..=n` and `j in 1..=m`. The bounds
// are the algorithm's, not this code's.
//
// They cannot be made provable: `n` and `m` are run-time, so no slice type can
// carry "length n + 1". There is nowhere to report a violation to either --
// this returns `Vec<Edit>`.
//
// And a fallback would be worse than the panic. A wrong `dp` cell does not
// crash; it produces a WRONG DIFF, quietly, in a tool whose entire output is a
// claim about whether two files match. An out-of-range index is loud and
// immediate; a substituted zero is neither.
#[allow(clippy::indexing_slicing)]
fn lcs_diff(
    norm_a: &[Vec<u8>],
    norm_b: &[Vec<u8>],
    orig_a: &[Vec<u8>],
    orig_b: &[Vec<u8>],
) -> Vec<Edit> {
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
    let mut ops: Vec<Edit> = Vec::new();
    let mut i = n;
    let mut j = m;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && norm_a[i - 1] == norm_b[j - 1] {
            ops.push(Edit::equal(&orig_a[i - 1], &orig_b[j - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(Edit::new(Op::Insert, orig_b[j - 1].clone()));
            j -= 1;
        } else {
            ops.push(Edit::new(Op::Delete, orig_a[i - 1].clone()));
            i -= 1;
        }
    }

    ops.reverse();
    ops
}

/// Myers diff algorithm for large files. O(ND) time where D is edit distance.
// See `lcs_diff` above for the reasoning; the same three answers hold here,
// with one addition that is specific to this pair.
//
// `lcs_diff` is NOT a fallback for this function -- the two are alternatives
// chosen by size (`lcs_diff` at or below 10,000 lines a side, this one above),
// because `lcs_diff` is O(n*m). So failing over to it on an internal
// inconsistency would trade a wrong answer for a hang, on exactly the inputs
// that selected this algorithm.
#[allow(clippy::indexing_slicing, clippy::expect_used)]
fn myers_diff(
    norm_a: &[Vec<u8>],
    norm_b: &[Vec<u8>],
    orig_a: &[Vec<u8>],
    orig_b: &[Vec<u8>],
) -> Vec<Edit> {
    let n = norm_a.len();
    let m = norm_b.len();

    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return orig_b
            .iter()
            .map(|l| Edit::new(Op::Insert, l.clone()))
            .collect();
    }
    if m == 0 {
        return orig_a
            .iter()
            .map(|l| Edit::new(Op::Delete, l.clone()))
            .collect();
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
    //
    // `k + offset` lands in `0..v_size` for every k these loops visit: `k`
    // runs `-d..=d`, `d` runs `0..=max_d`, and `offset` is `max_d`, so the sum
    // is between 0 and `2 * max_d`, which is `v_size - 1`.
    //
    // `try_from` rather than `as`, and this is the one behavioural change in
    // this commit. An `as` cast turns a negative sum into a huge `usize`, so a
    // broken invariant would surface as an out-of-bounds panic somewhere
    // downstream, pointing at the array rather than at the k-line arithmetic
    // that actually went wrong. Same failure, named at its cause.
    let idx = |k: isize| -> usize {
        usize::try_from(k.saturating_add(offset))
            .expect("k-line index: k runs -d..=d and offset is max_d, so k + offset >= 0")
    };

    let mut found_d: Option<usize> = None;

    for d in 0..=max_d {
        let old_v = v.clone();
        let d_signed = d as isize;

        let mut k = -d_signed;
        while k <= d_signed {
            let x: isize =
                if k == -d_signed || (k != d_signed && old_v[idx(k - 1)] < old_v[idx(k + 1)]) {
                    old_v[idx(k + 1)]
                } else {
                    old_v[idx(k - 1)] + 1
                };

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

        let prev_k: isize = if k == -d_signed || (k != d_signed && vd[idx(k - 1)] < vd[idx(k + 1)])
        {
            k + 1
        } else {
            k - 1
        };

        let prev_x = if d > 0 {
            v_history[d - 1][idx(prev_k)]
        } else {
            0
        };
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
    let mut ops: Vec<Edit> = Vec::new();
    let mut ai: usize = 0;
    let mut bi: usize = 0;

    for &(px, py) in &path {
        // If we need to skip to (px, py), emit deletes/inserts.
        while ai < px && bi < py {
            ops.push(Edit::equal(&orig_a[ai], &orig_b[bi]));
            ai += 1;
            bi += 1;
        }
        while ai < px {
            ops.push(Edit::new(Op::Delete, orig_a[ai].clone()));
            ai += 1;
        }
        while bi < py {
            ops.push(Edit::new(Op::Insert, orig_b[bi].clone()));
            bi += 1;
        }
        // The point itself.
        if ai == px && bi == py && ai < n && bi < m {
            if norm_a[ai] == norm_b[bi] {
                ops.push(Edit::equal(&orig_a[ai], &orig_b[bi]));
                ai += 1;
                bi += 1;
            } else if ai < n {
                ops.push(Edit::new(Op::Delete, orig_a[ai].clone()));
                ai += 1;
            }
        }
    }

    // Flush remaining.
    while ai < n {
        ops.push(Edit::new(Op::Delete, orig_a[ai].clone()));
        ai += 1;
    }
    while bi < m {
        ops.push(Edit::new(Op::Insert, orig_b[bi].clone()));
        bi += 1;
    }

    ops
}

// ============================================================================
// Hunk construction
// ============================================================================

/// Group edit operations into hunks with the given number of context lines.
fn build_hunks(ops: &[Edit], context: usize) -> Vec<Hunk> {
    if ops.is_empty() {
        return Vec::new();
    }

    // Find indices of all non-equal operations.
    let change_indices: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, e)| e.op != Op::Equal)
        .map(|(i, _)| i)
        .collect();

    if change_indices.is_empty() {
        return Vec::new();
    }

    // Group changes that are within `2 * context` lines of each other.
    let mut groups: Vec<(usize, usize)> = Vec::new(); // (first_change_idx, last_change_idx)
    // `is_empty` was checked above, so `split_first` cannot fail -- and it
    // hands back the tail the loop wants in the same operation.
    let Some((&first_change, rest_changes)) = change_indices.split_first() else {
        return Vec::new();
    };
    let mut group_start = first_change;
    let mut group_end = first_change;

    for &ci in rest_changes {
        // Count equal lines between group_end and ci.
        let gap = ops
            .get(group_end.saturating_add(1)..ci)
            .unwrap_or_default()
            .iter()
            .filter(|e| e.op == Op::Equal)
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

        let hunk_ops = ops.get(hunk_start..hunk_end).unwrap_or_default();

        // Count lines from file1 and file2 within this hunk, and track start
        // positions.
        let mut line1: usize = 0;
        let mut line2: usize = 0;

        // Count lines before hunk_start to determine the starting line numbers.
        for Edit { op, .. } in ops.get(..hunk_start).unwrap_or_default() {
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
        let mut lines: Vec<Edit> = Vec::new();

        for e in hunk_ops {
            match e.op {
                Op::Equal => {
                    count1 += 1;
                    count2 += 1;
                }
                Op::Delete => count1 += 1,
                Op::Insert => count2 += 1,
            }
            lines.push(e.clone());
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

/// Write one body line of a diff: an ASCII marker, the line's own BYTES, and a
/// newline, optionally wrapped in a colour.
///
/// The line goes out RAW. Measured: GNU prints `< cafM-i` for a line holding
/// byte 0351, which is `cat -v` showing the byte itself, not an escape GNU
/// chose. Rendering it any other way would also break the pipe that matters --
/// `diff -u | patch` -- since `patch` reads these lines back and compares them
/// against the file.
fn write_body_line(w: &mut impl Write, marker: &[u8], text: &[u8], color: Option<&str>) {
    let mut line: Vec<u8> = Vec::with_capacity(marker.len().saturating_add(text.len()) + 16);
    if let Some(c) = color {
        line.extend_from_slice(c.as_bytes());
    }
    line.extend_from_slice(marker);
    line.extend_from_slice(text);
    if color.is_some() {
        line.extend_from_slice(RESET.as_bytes());
    }
    line.push(b'\n');
    let _ = w.write_all(&line);
}

/// Write one body line with `-T` and `-t` applied.
///
/// Separate from `write_body_line` because the `---`/`+++` and `*** `/`--- `
/// file headers go through that one too, and GNU applies NEITHER option to
/// them: measured, `-t -u` leaves the tab between the filename and its mtime
/// unexpanded. `-y` is excluded for a different reason -- it computes its own
/// column layout and pads the gutter itself.
fn write_text_line(
    w: &mut impl Write,
    config: &Config,
    marker: &[u8],
    text: &[u8],
    color: Option<&str>,
) {
    // An EMPTY marker stays empty. GNU emits the separator only when there is
    // a flag to separate it from (`if (line_flag && *line_flag)`), so `-T`
    // must not turn "no marker" into a line that begins with a lone tab.
    let marker: Cow<[u8]> = if config.initial_tab && !marker.is_empty() {
        let mut m = marker.to_vec();
        // The space the marker already carries is REPLACED, not appended to:
        // `< ` becomes `<\t`, and unified's `-`, which has no space, just
        // gains one. Both measured.
        while m.last() == Some(&b' ') {
            m.pop();
        }
        m.push(b'\t');
        Cow::Owned(m)
    } else {
        Cow::Borrowed(marker)
    };
    let text: Cow<[u8]> = if config.expand_tabs {
        Cow::Owned(expand_output_tabs(text, &marker, config.tabsize))
    } else {
        Cow::Borrowed(text)
    };
    write_body_line(w, &marker, &text, color);
}

/// Expand tabs in one output line to 8-column stops, the way the reference
/// does it.
///
/// **Columns are counted over BYTES, and a byte advances the column only when
/// it is printable ASCII.** That is not this implementation simplifying a
/// character-based rule -- it is what GNU does, and it is measured. A line
/// holding `e` with an acute accent (`0xC3 0xA9`) followed by a tab comes back
/// with a FULL eight spaces, identical to a line that begins with the tab:
/// both bytes count as zero columns, because neither is printable in the C
/// locale. A three-byte CJK character behaves the same, so the rule is not
/// "count characters" either, and the answer does not move under
/// `LC_ALL=C.UTF-8`.
///
/// This is one of the rare places where the byte-oriented reading is both the
/// faithful one and the one this project wants anyway.
///
/// Three bytes are special, all three measured rather than recalled:
///
/// | byte | effect |
/// |---|---|
/// | `\t` | pad with spaces to the next multiple of 8 |
/// | `\r` | print it, RE-EMIT the marker, and reset the column to 0 |
/// | `\b` | back up one column -- but at column 0 it is DROPPED, not printed |
///
/// The carriage-return rule is the surprising one, and it is deliberate: on a
/// file with CRLF endings the terminal would return to the left margin and the
/// text would overprint the `<`, so GNU writes the marker again behind it.
fn expand_output_tabs(text: &[u8], marker: &[u8], tab_stop: usize) -> Vec<u8> {
    // `--tabsize`, never zero: the parser refuses it.
    let tab_stop = tab_stop.max(1);
    let mut out: Vec<u8> = Vec::with_capacity(text.len().saturating_add(tab_stop));
    let mut column: usize = 0;
    let mut rest = text;

    while let Some((&b, tail)) = rest.split_first() {
        rest = tail;
        match b {
            b'\t' => {
                // Never zero: `column % tab_stop` is below the stop, so a tab
                // already sitting on one still advances a full stop.
                let pad = tab_stop.saturating_sub(column % tab_stop);
                out.resize(out.len().saturating_add(pad), b' ');
                column = column.saturating_add(pad);
            }
            b'\r' => {
                out.push(b);
                // Only when text follows. A line that ENDS in a carriage
                // return gets no second marker -- there is nothing left to
                // overprint. (`text` excludes the newline, so "bytes remain"
                // is the whole test.)
                if !rest.is_empty() {
                    out.extend_from_slice(marker);
                }
                column = 0;
            }
            // 0x08 is BS; Rust has no `\b` escape.
            b'\x08' => {
                // Dropped at column 0 rather than printed, so that a line
                // starting with a backspace cannot back over the marker.
                if column > 0 {
                    column = column.saturating_sub(1);
                    out.push(b);
                }
            }
            // `0x20..=0x7E` is C-locale `isprint` exactly. Everything else --
            // control bytes, DEL, and every byte of every multi-byte
            // character -- is printed but counts as no width at all.
            _ => {
                if (0x20..=0x7E).contains(&b) {
                    column = column.saturating_add(1);
                }
                out.push(b);
            }
        }
    }
    out
}

/// `Some(code)` when colour is on, so `write_body_line` takes one argument
/// rather than a colour and a flag that must agree.
fn when(color: bool, code: &str) -> Option<&str> {
    if color { Some(code) } else { None }
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

/// Compile one `-I` pattern, or exit 2 the way GNU does.
///
/// A **basic** regular expression, which was measured rather than assumed:
/// `diff -I 'o\|O'` ignores a hunk and `diff -I 'o|O'` does not, so the
/// alternation that works is BRE's escaped one. Compiling it as an ERE would
/// make `\|` a literal bar and quietly ignore nothing.
///
/// GNU's refusal is `diff: [: Invalid regular expression` with exit 2, the
/// pattern named bare rather than quoted -- measured, since this file quotes
/// file names and does not quote this.
fn compile_ignore_pattern(pattern: &OsString) -> Result<ere::Regex, String> {
    let bytes = quoting::os_bytes(pattern.as_os_str());
    // Upstream's `add_regexp`: `error (EXIT_TROUBLE, 0, "%s: %s", pattern, m)`,
    // `m` being the regex library's own sentence for what is wrong -- `Unmatched
    // ( or \(` for `a\(`, not one fixed phrase for every failure. The pattern is
    // printed as typed, except that a byte that could forge a line or garble the
    // terminal is spelled in octal.
    ere::bre::compile(&bytes, false).map_err(|e| {
        format!(
            "diff: {}: {}",
            quoting::escape_unprintable(&bytes),
            e.message()
        )
    })
}

/// Drop the hunks `-I` says are not changes.
///
/// Applied in every renderer arm rather than once over `ops`, because removing
/// operations would renumber every hunk after the one removed -- the line
/// numbers in the output are the whole point of a diff, and a hunk that is
/// ignored must not shift the ones that are printed.
fn unignored(hunks: Vec<Hunk>, config: &Config) -> Vec<Hunk> {
    if config.ignore_matching.is_empty() {
        return hunks;
    }
    hunks
        .into_iter()
        .filter(|h| !hunk_is_ignorable(h, config))
        .collect()
}

/// Does every CHANGED line in this hunk match one of the `-I` patterns?
///
/// "Every" spans both sides, which is the part the manual's phrasing hides and
/// the measurement settles: a hunk holding one matching and one non-matching
/// changed line is printed in full. Context lines are not consulted at all --
/// they did not change, so they are not part of the change being judged.
///
/// An empty pattern list answers `false`, so a hunk is never ignored when `-I`
/// was not given; a hunk with no changed lines answers `false` too, because
/// "all of nothing matches" would silently drop a hunk that has no business
/// being dropped.
fn hunk_is_ignorable(hunk: &Hunk, config: &Config) -> bool {
    edits_are_ignorable(&hunk.lines, config)
}

/// [`hunk_is_ignorable`] over a run of edits: upstream's `analyze_hunk`,
/// where a changed line is trivial if `-B` finds it blank or an `-I` pattern
/// matches it, and a run is ignored only when every changed line in it is.
/// `-B` used to be applied line by line instead, turning each blank changed
/// line into a common one -- which hid a blank line inside a hunk upstream
/// prints whole, and under `-y` put a line one file has into both columns.
fn edits_are_ignorable(edits: &[Edit], config: &Config) -> bool {
    if config.ignore_matching.is_empty() && !config.ignore_blank_lines {
        return false;
    }
    let mut saw_change = false;
    for Edit { op, text, .. } in edits {
        if *op == Op::Equal {
            continue;
        }
        saw_change = true;
        let trivial = (config.ignore_blank_lines && is_blank(text, config))
            || config
                .ignore_matching
                .iter()
                .any(|re| re.is_match(text).unwrap_or(false));
        if !trivial {
            return false;
        }
    }
    saw_change
}

/// Where a hunk's change begins in each file, and how many lines it touches.
///
/// The leading context is skipped first, which is why this is shared rather
/// than repeated: every renderer needs the position of the first CHANGED line,
/// and computing it from `start1` alone is wrong for any hunk with context.
fn hunk_change_span(hunk: &Hunk) -> (usize, usize, usize, usize) {
    let del_count = hunk.lines.iter().filter(|e| e.op == Op::Delete).count();
    let ins_count = hunk.lines.iter().filter(|e| e.op == Op::Insert).count();
    let mut line1_pos = hunk.start1;
    let mut line2_pos = hunk.start2;
    for Edit { op, .. } in &hunk.lines {
        if *op == Op::Equal {
            line1_pos += 1;
            line2_pos += 1;
        } else {
            break;
        }
    }
    (line1_pos, line2_pos, del_count, ins_count)
}

/// `-e`: an `ed` script that turns file 1 into file 2.
///
/// **Emitted in REVERSE order**, which is the whole subtlety. `ed` applies the
/// commands in the order given and each one renumbers the lines after it, so a
/// script written front-to-back would have every command after the first
/// aiming at the wrong line. Measured: GNU prints `4a` before `2c` for a file
/// whose change is at line 2 and whose append is at line 4.
///
/// A delete has no body and no terminator; an append and a change carry their
/// lines followed by a lone `.`.
fn print_ed(hunks: &[Hunk], config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    for hunk in hunks.iter().rev() {
        let (line1_pos, _, del_count, ins_count) = hunk_change_span(hunk);
        let (has_del, has_ins) = (del_count > 0, ins_count > 0);
        let op_char = match (has_del, has_ins) {
            (true, true) => 'c',
            (true, false) => 'd',
            (false, true) => 'a',
            (false, false) => continue,
        };
        let _ = writeln!(w, "{}{}", range_str(line1_pos, del_count), op_char);
        if !has_ins {
            continue;
        }
        for Edit { op, text, .. } in &hunk.lines {
            if *op == Op::Insert {
                write_text_line(&mut w, config, b"", text, None);
            }
        }
        // `ed` ends an insert with a line holding a single dot. A body line
        // that is itself a dot would end the insert early; GNU has the same
        // hole, and a diff that cannot be applied is upstream's behaviour
        // rather than something invented here.
        let _ = writeln!(w, ".");
    }
}

/// `-n`: RCS's diff format.
///
/// Forward order, unlike `-e`, because the commands carry explicit counts and
/// are defined against the ORIGINAL line numbering rather than being applied
/// to a file that shifts under them.
///
/// A change is a delete AND an append, and the append's position is measured
/// past the deleted lines: GNU answers a one-line change at line 2 with
/// `d2 1` then `a2 1`, not `a1 1`. That `+ del_count` is the one piece of this
/// that reasoning gets wrong.
fn print_rcs(hunks: &[Hunk], config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    for hunk in hunks {
        let (line1_pos, _, del_count, ins_count) = hunk_change_span(hunk);
        if del_count > 0 {
            let _ = writeln!(w, "d{} {}", line1_pos.saturating_add(1), del_count);
        }
        if ins_count > 0 {
            let _ = writeln!(w, "a{} {}", line1_pos.saturating_add(del_count), ins_count);
            for Edit { op, text, .. } in &hunk.lines {
                if *op == Op::Insert {
                    write_text_line(&mut w, config, b"", text, None);
                }
            }
        }
    }
}

fn print_normal(hunks: &[Hunk], config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    for hunk in hunks {
        // Determine the operation type for this hunk: all deletes, all inserts,
        // or a change (mix).
        let has_del = hunk.lines.iter().any(|e| e.op == Op::Delete);
        let has_ins = hunk.lines.iter().any(|e| e.op == Op::Insert);

        let del_count = hunk.lines.iter().filter(|e| e.op == Op::Delete).count();
        let ins_count = hunk.lines.iter().filter(|e| e.op == Op::Insert).count();

        // Compute file-1 and file-2 ranges for the changed lines only (not context).
        // We need start positions relative to the hunk's changes.
        let mut line1_pos = hunk.start1;
        let mut line2_pos = hunk.start2;

        // Skip leading context to find where changes begin.
        for Edit { op, .. } in &hunk.lines {
            if *op == Op::Equal {
                line1_pos += 1;
                line2_pos += 1;
            } else {
                break;
            }
        }

        // POSIX diff prints the starting line of each range whether or not
        // the count is zero; both arms return the same value.
        let r1 = range_str(line1_pos, del_count);
        let r2 = range_str(line2_pos, ins_count);

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
        for Edit {
            op,
            text,
            no_final_newline,
            ..
        } in &hunk.lines
        {
            if *op == Op::Delete {
                write_text_line(&mut w, config, b"< ", text, when(config.color, RED));
                if *no_final_newline {
                    write_no_newline_marker(&mut w);
                }
            }
        }

        // Separator between deletes and inserts for 'c' operations.
        if has_del && has_ins {
            let _ = writeln!(w, "---");
        }

        // Print inserted lines.
        for Edit {
            op,
            text,
            no_final_newline,
            ..
        } in &hunk.lines
        {
            if *op == Op::Insert {
                write_text_line(&mut w, config, b"> ", text, when(config.color, GREEN));
                if *no_final_newline {
                    write_no_newline_marker(&mut w);
                }
            }
        }
    }
}

// ============================================================================
// Unified diff output
// ============================================================================

/// The `<path>TAB<mtime>` field GNU puts on a `---`, `+++` or `***` header.
///
/// Measured against GNU diffutils rather than reconstructed, because three
/// details of it are not guessable:
///
/// ```text
/// --- x.txt<TAB>2020-01-02 08:04:05.000000000 +0000
/// ```
///
/// * the separator is a TAB, and `patch` relies on it -- everything after the
///   tab is the timestamp, which is how a path containing spaces stays
///   readable;
/// * the fraction is NINE digits, always, even for a whole second;
/// * the time is LOCAL, not UTC. `TZ=America/New_York` on the same file gives
///   `2020-01-02 03:04:05.000000000 -0500`, so the offset moves with it.
///
/// A file whose mtime cannot be read -- stdin, above all -- takes the current
/// time, which is also measured: GNU stamps `diff -u - y.txt` with now.
/// A path as `diff` writes it in a HEADER, C-quoted when it needs to be.
///
/// GNU runs the names in `--- `/`+++ ` and in the `diff -r A B` label through
/// `quotearg` in the C style, so a name holding a byte like `0xE9` comes out
/// as `"da/oddéname.txt"` -- double-quoted, with the byte escaped in
/// octal. Measured (`scripts/probe-diff-name-quoting.sh`), along with three
/// rules that are not guessable from "it quotes odd names":
///
/// * a space or a tab is enough to force the quotes: `"with space1"`,
///   `"tab\tone"`;
/// * a single quote is **not** -- `apo'st1` stays bare -- though a double
///   quote is, as `"quo\"te1"`;
/// * and `Only in DIR: NAME` does **not** go through this at all. It prints
///   the raw bytes. Two lines of the same program, two rules, and assuming
///   they agreed would have been wrong in whichever direction I guessed.
fn qname(p: &Path) -> Vec<u8> {
    quoting::quote_c_maybe(&pb(p)).into_bytes()
}

fn header_field(path: &Path) -> Vec<u8> {
    let (secs, nanos) = mtime_parts(path);
    let tm = localtime::Zone::from_env().local(secs, nanos);
    let mut out = qname(path);
    out.push(b'\t');
    out.extend_from_slice(&localtime::nstrftime(b"%Y-%m-%d %H:%M:%S.%N %z", &tm));
    out
}

/// `path`'s modification time as `(seconds, nanoseconds)` since the epoch,
/// falling back to now.
///
/// Split out so the fallback is one decision in one place: every way of
/// failing to learn a file's mtime -- it does not exist, it is a pipe, the
/// platform will not say -- produces the same answer GNU produces for stdin.
fn mtime_parts(path: &Path) -> (i64, u32) {
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or((0, 0), |d| {
                (i64::try_from(d.as_secs()).unwrap_or(0), d.subsec_nanos())
            })
    };
    let Ok(meta) = fs::metadata(path) else {
        return now();
    };
    let Ok(modified) = meta.modified() else {
        return now();
    };
    epoch_parts(modified)
}

/// A `SystemTime` as `(seconds, nanoseconds)` since the epoch, with the
/// nanoseconds always POSITIVE.
///
/// Split from [`mtime_parts`] because the pre-1970 branch is the only real
/// arithmetic here and a path-taking function cannot be given a 1969 file to
/// test with.
///
/// `duration_since` reports a pre-epoch instant as a positive gap the other
/// way round, so a naive negation puts the instant on the wrong side of the
/// second: 0.75 s before the epoch is `(-1, 250_000_000)`, not
/// `(0, -750_000_000)` -- which is not even representable -- nor `(-0, 750...)`,
/// which would name 1970-01-01T00:00:00.75, three quarters of a second in the
/// wrong direction. `strftime` renders the two fields independently, so an
/// error here is a timestamp that looks entirely plausible.
fn epoch_parts(t: std::time::SystemTime) -> (i64, u32) {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (i64::try_from(d.as_secs()).unwrap_or(0), d.subsec_nanos()),
        Err(e) => {
            let d = e.duration();
            let secs = i64::try_from(d.as_secs()).unwrap_or(0);
            if d.subsec_nanos() == 0 {
                (-secs, 0)
            } else {
                (
                    -secs.saturating_add(1),
                    1_000_000_000_u32.saturating_sub(d.subsec_nanos()),
                )
            }
        }
    }
}

/// One side of a `@@ -a,b +c,d @@` header.
///
/// A count of exactly ONE is written as the bare line number, with no comma
/// and no count: GNU answers `diff -U0` with `@@ -20 +20 @@`, not
/// `@@ -20,1 +20,1 @@`. That is the unified format's own rule rather than a
/// preference, and it shows up the moment a hunk has no context -- which is
/// why `-U0` was the case that exposed it here.
///
/// A count of ZERO keeps its `,0` and takes the line number GNU uses for an
/// insertion point, which the caller has already worked out.
fn range_field(start: usize, count: usize) -> String {
    if count == 1 {
        format!("{start}")
    } else {
        format!("{start},{count}")
    }
}

fn print_unified(hunks: &[Hunk], path1: &Path, path2: &Path, config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    // File headers, carrying the mtime GNU puts there. Written as bytes for
    // the same reason the body is: a path is bytes, and this one is read back
    // by `patch`.
    write_body_line(
        &mut w,
        b"--- ",
        &header_field(path1),
        when(config.color, RED),
    );
    write_body_line(
        &mut w,
        b"+++ ",
        &header_field(path2),
        when(config.color, GREEN),
    );

    for hunk in hunks {
        // Hunk header: @@ -start,count +start,count @@
        let h1_start = hunk.start1 + 1;
        let h2_start = hunk.start2 + 1;
        let header = format!(
            "@@ -{} +{} @@",
            range_field(if hunk.count1 == 0 { 0 } else { h1_start }, hunk.count1),
            range_field(if hunk.count2 == 0 { 0 } else { h2_start }, hunk.count2),
        );
        let _ = writeln!(w, "{}", color_cyan(&header, config.color));

        for Edit {
            op,
            text,
            no_final_newline,
            ..
        } in &hunk.lines
        {
            match op {
                Op::Equal => {
                    // The marker belongs here too. A unified hunk's TRAILING
                    // CONTEXT is where the last line of the file lands when
                    // the change is above it -- which is precisely the case
                    // where both files lack a final newline, since then the
                    // last line is unchanged and stays an `Equal`.
                    write_text_line(&mut w, config, b" ", text, None);
                    if *no_final_newline {
                        write_no_newline_marker(&mut w);
                    }
                }
                Op::Delete => {
                    write_text_line(&mut w, config, b"-", text, when(config.color, RED));
                    if *no_final_newline {
                        write_no_newline_marker(&mut w);
                    }
                }
                Op::Insert => {
                    write_text_line(&mut w, config, b"+", text, when(config.color, GREEN));
                    if *no_final_newline {
                        write_no_newline_marker(&mut w);
                    }
                }
            }
        }
    }
}

// ============================================================================
// Context diff output
// ============================================================================

/// The two-character marker a context diff puts in front of a body line.
///
/// `changed` is "this hunk has deletions AND insertions", which is what makes
/// a line a CHANGE rather than a removal or an addition. Measured, one hunk of
/// each shape:
///
/// ```text
/// change        ! charlie   /  ! CHANGED     -- both halves
/// pure delete   - charlie                    -- `---` half is header only
/// pure insert                 + extra        -- `***` half is header only
/// ```
///
/// A function rather than two inline `if`s because the rule is symmetric and
/// stating it once is what makes that visible: the same `changed` flag turns
/// both a `-` and a `+` into a `!`, and getting only one of them right would
/// produce a diff that still looks plausible.
fn context_marker(op: Op, changed: bool) -> &'static [u8] {
    match op {
        Op::Equal => b"  ",
        Op::Delete if changed => b"! ",
        Op::Delete => b"- ",
        Op::Insert if changed => b"! ",
        Op::Insert => b"+ ",
    }
}

fn print_context(hunks: &[Hunk], path1: &Path, path2: &Path, config: &Config) {
    let out = io::stdout();
    let mut w = out.lock();

    write_body_line(
        &mut w,
        b"*** ",
        &header_field(path1),
        when(config.color, RED),
    );
    write_body_line(
        &mut w,
        b"--- ",
        &header_field(path2),
        when(config.color, GREEN),
    );

    for hunk in hunks {
        let _ = writeln!(w, "***************");

        // WHICH MARKER, AND WHETHER THE HALF HAS A BODY AT ALL. Measured, on
        // one hunk of each shape:
        //
        //   change (deletes AND inserts)  `!` on BOTH halves, both bodies
        //   pure delete                   `-`, and the `---` half is HEADER
        //                                 ONLY -- no body, not even context
        //   pure insert                   `+`, and the `***` half is header
        //                                 only
        //
        // This build printed `-` and `+` where GNU prints `!`, and printed
        // both bodies always. A context diff is meant to be re-appliable, and
        // `!` is what says "these two lines are the same line, changed" rather
        // than "one line vanished and an unrelated one appeared".
        let has_del = hunk.lines.iter().any(|e| e.op == Op::Delete);
        let has_ins = hunk.lines.iter().any(|e| e.op == Op::Insert);
        let changed = has_del && has_ins;

        // File 1 section.
        let f1_start = hunk.start1 + 1;
        let f1_end = hunk.start1 + hunk.count1;
        let _ = writeln!(
            w,
            "{}",
            color_cyan(
                &format!(
                    "*** {},{} ****",
                    f1_start,
                    if f1_end == 0 { f1_start } else { f1_end }
                ),
                config.color,
            )
        );

        if has_del {
            for Edit {
                op,
                text,
                no_final_newline,
                ..
            } in &hunk.lines
            {
                match op {
                    Op::Equal => {
                        write_text_line(&mut w, config, b"  ", text, None);
                        if *no_final_newline {
                            write_no_newline_marker(&mut w);
                        }
                    }
                    Op::Delete => {
                        let marker = context_marker(Op::Delete, changed);
                        write_text_line(&mut w, config, marker, text, when(config.color, RED));
                        if *no_final_newline {
                            write_no_newline_marker(&mut w);
                        }
                    }
                    Op::Insert => {
                        // Inserts are not shown in the file-1 section.
                    }
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
                &format!(
                    "--- {},{} ----",
                    f2_start,
                    if f2_end == 0 { f2_start } else { f2_end }
                ),
                config.color,
            )
        );

        if has_ins {
            for Edit {
                op,
                text,
                no_final_newline,
                ..
            } in &hunk.lines
            {
                match op {
                    Op::Equal => {
                        write_text_line(&mut w, config, b"  ", text, None);
                        if *no_final_newline {
                            write_no_newline_marker(&mut w);
                        }
                    }
                    Op::Insert => {
                        let marker = context_marker(Op::Insert, changed);
                        write_text_line(&mut w, config, marker, text, when(config.color, GREEN));
                        if *no_final_newline {
                            write_no_newline_marker(&mut w);
                        }
                    }
                    Op::Delete => {
                        // Deletes are not shown in the file-2 section.
                    }
                }
            }
        }
    }
}

// ============================================================================
// Side-by-side output
// ============================================================================

/// `GUTTER_WIDTH_MINIMUM`: the narrowest gutter `-y` puts between its halves.
const GUTTER_WIDTH_MINIMUM: usize = 3;

/// `-y`'s two columns: upstream's `sdiff_half_width` and
/// `sdiff_column2_offset`, computed in `diff.c` as below.
///
/// "Maximize first the half line width, and then the gutter width": two
/// halves and a gutter fit the width, the gutter is at least three columns,
/// and -- unless tabs are being expanded -- a half and its gutter are a whole
/// number of tab stops, so that tabs in the right column land where they
/// would in the file. That last rule is why the gutter is not simply centred:
/// at the default width of 130 the right column starts at 64 and the gutter
/// character sits at 62, where `(130 - 1) / 2` would put it at 64. The rule
/// was tracked as TD-B-DIFF-SIDE-BY-SIDE-PADS-WITH-SPACES-WHERE-GNU-USES-TABS
/// until it was read from `diff.c` rather than fitted to samples.
fn sdiff_columns(width: usize, tabsize: usize, expand_tabs: bool) -> (usize, usize) {
    let t = if expand_tabs { 1 } else { tabsize.max(1) };
    let w = width;
    let t_plus_g = t.saturating_add(GUTTER_WIDTH_MINIMUM);
    let unaligned_off = (w >> 1)
        .saturating_add(t_plus_g >> 1)
        .saturating_add(w & t_plus_g & 1);
    let off = unaligned_off.saturating_sub(unaligned_off.checked_rem(t).unwrap_or(0));
    let half = if off <= GUTTER_WIDTH_MINIMUM || w <= off {
        0
    } else {
        off.saturating_sub(GUTTER_WIDTH_MINIMUM)
            .min(w.saturating_sub(off))
    };
    (half, if half == 0 { w } else { off })
}

/// One line as `-y` shows it: its text, and whether it ended in a newline.
#[derive(Clone, Copy)]
struct SideLine<'a> {
    text: &'a [u8],
    newline: bool,
}

impl<'a> SideLine<'a> {
    /// File 1's copy of `edit`.
    fn left(edit: &'a Edit) -> Self {
        SideLine {
            text: &edit.text,
            newline: !edit.no_final_newline,
        }
    }

    /// File 2's copy of `edit`: its own text for an insertion, and for a
    /// common line whatever file 2 had, which under `-i` or `-b` need not be
    /// file 1's.
    fn right(edit: &'a Edit) -> Self {
        SideLine {
            text: edit.other.as_deref().unwrap_or(&edit.text),
            newline: !edit.no_final_newline,
        }
    }
}

/// `side.c`'s printing state: the layout and the output so far.
struct SideBySide<'c> {
    config: &'c Config,
    /// `sdiff_half_width`.
    half: usize,
    /// `sdiff_column2_offset`.
    column2: usize,
    out: Vec<u8>,
}

impl SideBySide<'_> {
    /// `tab_from_to`: pad from column `from` to column `to` with tabs where a
    /// tab stop falls in between, then spaces; returns `to`.
    fn tab_from_to(&mut self, from: usize, to: usize) -> usize {
        let tabsize = self.config.tabsize.max(1);
        let mut from = from;
        if !self.config.expand_tabs {
            let mut tab =
                from.saturating_add(tabsize.saturating_sub(from.checked_rem(tabsize).unwrap_or(0)));
            while tab <= to {
                self.out.push(b'\t');
                from = tab;
                tab = tab.saturating_add(tabsize);
            }
        }
        while from < to {
            self.out.push(b' ');
            from = from.saturating_add(1);
        }
        to
    }

    /// `print_half_line`: `line` in a column `out_bound` wide -- cut there,
    /// its tabs laid out against the column, its newline dropped. `indent` is
    /// where the column starts, which a carriage return goes back to. Returns
    /// the last column written.
    fn print_half_line(&mut self, line: &[u8], indent: usize, out_bound: usize) -> usize {
        let tabsize = self.config.tabsize.max(1);
        let mut in_position = 0usize;
        let mut out_position = 0usize;
        let mut rest = line;
        while let Some(&c) = rest.first() {
            let mut step = 1;
            match c {
                b'\t' => {
                    let spaces =
                        tabsize.saturating_sub(in_position.checked_rem(tabsize).unwrap_or(0));
                    if in_position == out_position {
                        let mut tabstop = out_position.saturating_add(spaces);
                        if self.config.expand_tabs {
                            tabstop = tabstop.min(out_bound);
                            while out_position < tabstop {
                                self.out.push(b' ');
                                out_position = out_position.saturating_add(1);
                            }
                        } else if tabstop < out_bound {
                            out_position = tabstop;
                            self.out.push(c);
                        }
                    }
                    in_position = in_position.saturating_add(spaces);
                }
                b'\r' => {
                    self.out.push(c);
                    self.tab_from_to(0, indent);
                    in_position = 0;
                    out_position = 0;
                }
                0x08 => {
                    if in_position != 0 {
                        in_position = in_position.saturating_sub(1);
                        if in_position < out_bound {
                            if out_position <= in_position {
                                // Make up for a tab suppressed past the bound.
                                while out_position < in_position {
                                    self.out.push(b' ');
                                    out_position = out_position.saturating_add(1);
                                }
                            } else {
                                out_position = in_position;
                                self.out.push(c);
                            }
                        }
                    }
                }
                b'\n' => return out_position,
                0x0b | 0x0c => {
                    if in_position < out_bound {
                        self.out.push(c);
                    }
                }
                0x20..=0x7e => {
                    // Printable ASCII: one column.
                    let before = in_position;
                    in_position = in_position.saturating_add(1);
                    if before < out_bound {
                        out_position = in_position;
                        self.out.push(c);
                    }
                }
                _ => match quoting::next_mb(rest) {
                    // `mbrtowc` answered a character, NUL aside (it answers 0
                    // for that): as wide as `wcwidth` says, a control
                    // character none.
                    Some(quoting::Mb::Char(ch, n)) if ch != '\0' => {
                        if let Some(width) = charwidth::char_width(ch) {
                            in_position = in_position.saturating_add(width);
                        }
                        if in_position <= out_bound {
                            out_position = in_position;
                            self.out.extend_from_slice(rest.get(..n).unwrap_or(rest));
                        }
                        step = n;
                    }
                    // A byte that begins no character, or NUL: printed as it
                    // is while there is room, and no width.
                    _ => {
                        if in_position < out_bound {
                            self.out.push(c);
                        }
                    }
                },
            }
            rest = rest.get(step..).unwrap_or_default();
        }
        out_position
    }

    /// `print_1sdiff_line`: one row -- the left half, the gutter character
    /// `sep` unless it is a space, the right half, then the newline if either
    /// half ended in one.
    fn row(&mut self, left: Option<SideLine<'_>>, sep: u8, right: Option<SideLine<'_>>) {
        // Upstream colours the one-sided rows only, and resets after the
        // newline.
        let color = match sep {
            b'<' => when(self.config.color, RED),
            b'>' => when(self.config.color, GREEN),
            _ => None,
        };
        if let Some(code) = color {
            self.out.extend_from_slice(code.as_bytes());
        }
        let mut col = 0;
        let mut put_newline = false;
        if let Some(l) = left {
            put_newline |= l.newline;
            col = self.print_half_line(l.text, 0, self.half);
        }
        if sep != b' ' {
            let gutter = self.half.saturating_add(self.column2).saturating_sub(1) / 2;
            col = self.tab_from_to(col, gutter).saturating_add(1);
            // A changed pair where one line ended without a newline and the
            // other did not: `/` when only the left has one, `\` when only the
            // right does.
            let mut sep = sep;
            if sep == b'|' && put_newline != right.is_some_and(|r| r.newline) {
                sep = if put_newline { b'/' } else { b'\\' };
            }
            self.out.push(sep);
        }
        if let Some(r) = right {
            put_newline |= r.newline;
            if !r.text.is_empty() {
                col = self.tab_from_to(col, self.column2);
                self.print_half_line(r.text, col, self.half);
            }
        }
        if put_newline {
            self.out.push(b'\n');
        }
        if color.is_some() {
            self.out.extend_from_slice(RESET.as_bytes());
        }
    }

    /// `print_sdiff_common_lines`: the lines between two hunks -- common
    /// lines, and the lines of any hunk `-B` or `-I` ignored, which upstream
    /// pairs off in order with nothing to say they differ. A surplus on the
    /// right is marked `)`, on the left `(`; `--left-column` shows the left
    /// alone, every line marked `(`.
    fn common_lines(&mut self, a: &mut Vec<SideLine<'_>>, b: &mut Vec<SideLine<'_>>) {
        if !self.config.suppress_common_lines && (!a.is_empty() || !b.is_empty()) {
            let mut left = a.iter().copied();
            if !self.config.left_column {
                let mut right = b.iter().copied();
                for (l, r) in left.by_ref().zip(right.by_ref()) {
                    self.row(Some(l), b' ', Some(r));
                }
                for r in right {
                    self.row(None, b')', Some(r));
                }
            }
            for l in left {
                self.row(Some(l), b'(', None);
            }
        }
        a.clear();
        b.clear();
    }
}

/// `-y`: the two files in two columns, row by row. A port of diffutils'
/// `side.c`, down to its column arithmetic ([`sdiff_columns`]) and its tab
/// handling, so the output matches GNU's byte for byte rather than only
/// looking alike on a terminal.
///
/// A hunk pairs its deleted and inserted lines off in order, `|` between;
/// the surplus follows, inserted lines (`>`) before deleted ones (`<`), which
/// is upstream's order. No-final-newline markers are deliberately absent:
/// GNU's `-y` on a file lacking its final newline shows the line and nothing
/// else, and simply omits the newline from its own last line of output.
fn print_side_by_side(ops: &[Edit], config: &Config) {
    // Deliberately unread, as every other renderer here: the process's exit
    // status is the comparison's, and a failed write has nowhere better to go.
    let _ = io::stdout().lock().write_all(&side_by_side(ops, config));
}

/// [`print_side_by_side`]'s output, as bytes.
fn side_by_side(ops: &[Edit], config: &Config) -> Vec<u8> {
    let (half, column2) = sdiff_columns(config.width, config.tabsize, config.expand_tabs);
    let mut sdiff = SideBySide {
        config,
        half,
        column2,
        out: Vec::new(),
    };
    let mut common_a: Vec<SideLine<'_>> = Vec::new();
    let mut common_b: Vec<SideLine<'_>> = Vec::new();

    let mut i = 0;
    while let Some(cur) = ops.get(i) {
        if cur.op == Op::Equal {
            common_a.push(SideLine::left(cur));
            common_b.push(SideLine::right(cur));
            i = i.saturating_add(1);
            continue;
        }
        // A maximal run of changes: one of upstream's hunks.
        let start = i;
        while ops.get(i).is_some_and(|e| e.op != Op::Equal) {
            i = i.saturating_add(1);
        }
        let run = ops.get(start..i).unwrap_or_default();
        let deleted: Vec<SideLine<'_>> = run
            .iter()
            .filter(|e| e.op == Op::Delete)
            .map(SideLine::left)
            .collect();
        let inserted: Vec<SideLine<'_>> = run
            .iter()
            .filter(|e| e.op == Op::Insert)
            .map(SideLine::right)
            .collect();
        if edits_are_ignorable(run, config) {
            common_a.extend(deleted);
            common_b.extend(inserted);
            continue;
        }
        sdiff.common_lines(&mut common_a, &mut common_b);
        let pairs = deleted.len().min(inserted.len());
        for (d, n) in deleted.iter().zip(&inserted) {
            sdiff.row(Some(*d), b'|', Some(*n));
        }
        for n in inserted.iter().skip(pairs) {
            sdiff.row(None, b'>', Some(*n));
        }
        for d in deleted.iter().skip(pairs) {
            sdiff.row(Some(*d), b'<', None);
        }
    }
    sdiff.common_lines(&mut common_a, &mut common_b);
    sdiff.out
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
    let mut all_names: Vec<OsString> = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while let (Some(e1), Some(e2)) = (entries1.get(i), entries2.get(j)) {
        match e1.cmp(e2) {
            std::cmp::Ordering::Less => {
                all_names.push(e1.clone());
                i = i.saturating_add(1);
            }
            std::cmp::Ordering::Greater => {
                all_names.push(e2.clone());
                j = j.saturating_add(1);
            }
            std::cmp::Ordering::Equal => {
                all_names.push(e1.clone());
                i = i.saturating_add(1);
                j = j.saturating_add(1);
            }
        }
    }
    // Whichever list still has entries; the other loop above stopped because
    // one of them ran out.
    all_names.extend(entries1.get(i..).unwrap_or_default().iter().cloned());
    all_names.extend(entries2.get(j..).unwrap_or_default().iter().cloned());

    let mut worst_exit = 0;

    for name in &all_names {
        let p1 = path1.join(name);
        let p2 = path2.join(name);
        let e1 = p1.exists();
        let e2 = p2.exists();

        if !e1 && !config.new_file {
            // STDOUT. Measured: `diff -r da db 2>/dev/null` still shows both
            // `Only in` lines and `2>&1 >/dev/null` shows neither, so GNU
            // puts them on stdout. They are a RESULT -- part of the answer to
            // "how do these trees differ" -- not a diagnostic, and on stderr
            // they were lost by every caller that redirected the diff.
            // Bytes, not `Display`: `name` is a directory entry and may
            // hold any byte. `println!` would not compile against an
            // `OsString`, and the `to_str()` that used to make it
            // compile is what silently dropped such entries upstream.
            print_path_line(&[b"Only in ", &pb(path2), b": ", &pb(Path::new(name))]);
            if worst_exit < 1 {
                worst_exit = 1;
            }
            continue;
        }
        if !e2 && !config.new_file {
            print_path_line(&[b"Only in ", &pb(path1), b": ", &pb(Path::new(name))]);
            if worst_exit < 1 {
                worst_exit = 1;
            }
            continue;
        }

        let is_dir1 = e1 && p1.is_dir();
        let is_dir2 = e2 && p2.is_dir();

        if is_dir1 && is_dir2 {
            if config.recursive {
                let code = diff_dirs(&p1, &p2, config);
                if code > worst_exit {
                    worst_exit = code;
                }
            } else {
                // Measured: this is informational and goes to STDOUT with the
                // rest of the answer, in the same sorted pass as the file
                // comparisons rather than collected at the end.
                println!(
                    "Common subdirectories: {} and {}",
                    p1.display(),
                    p2.display()
                );
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
            let code = diff_files(&p1, &p2, config, true);
            if code > worst_exit {
                worst_exit = code;
            }
        }
    }

    worst_exit
}

/// List entries in a directory, returning just the file/dir names.
fn list_dir(path: &Path) -> Result<Vec<OsString>, String> {
    let entries =
        fs::read_dir(path).map_err(|e| format!("{}: {}", path.display(), strerror(&e)))?;

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {}", path.display(), strerror(&e)))?;
        // The name is taken as it is. This used to read
        //
        //     if let Some(name) = entry.file_name().to_str() { names.push(...) }
        //
        // which **silently dropped every entry whose name is not valid
        // Unicode** -- a legal filename on this OS. The effect was worse than
        // the panic it avoided: `diff -r a b` reported no difference for files
        // it had never looked at, and said nothing about having skipped them.
        // A comparison tool answering "these are the same" about a file it
        // declined to read is the one failure it must never have.
        names.push(entry.file_name());
    }
    Ok(names)
}

// ============================================================================
// Core diff driver
// ============================================================================

/// Compare two files and print the diff. Returns exit code (0/1/2).
/// A verdict line naming two paths, written as BYTES.
///
/// `println!("Files {p1} and {p2} differ")` cannot do this: a path may hold
/// bytes that are not valid Unicode -- on this OS every byte but `/` and NUL
/// is legal in a name -- and `Path::display()` replaces them, so the line
/// would name a file nobody can open.
fn print_path_line(parts: &[&[u8]]) {
    let out = io::stdout();
    let mut w = out.lock();
    let mut line: Vec<u8> = Vec::new();
    for p in parts {
        line.extend_from_slice(p);
    }
    line.push(b'\n');
    let _ = w.write_all(&line);
}

/// A path's bytes, for the lines above.
fn pb(p: &Path) -> Vec<u8> {
    quoting::os_bytes(p.as_os_str()).into_owned()
}

fn diff_files(p1: &Path, p2: &Path, config: &Config, in_dir_walk: bool) -> i32 {
    // `-` is stdin, which exists without being a path -- `Path::new("-")
    // .exists()` is false, so the two guards below turned every
    // `cmd | diff file -` into `No such file or directory` before `read_file`
    // was ever reached.
    let e1 = is_stdin(p1) || p1.exists();
    let e2 = is_stdin(p2) || p2.exists();

    // Handle absent files with --new-file.
    //
    // BOTH are reported before returning. Measured: `diff nosuch.txt
    // nosuch2.txt` gives GNU two lines, one per file, and this build gave one
    // and stopped -- so a user who mistyped both paths fixed the first, re-ran,
    // and only then learned about the second. Same shape as CLAUDE.md's rule
    // that a batch operation reports the worst error rather than the first one
    // it meets.
    if !config.new_file {
        let mut absent = false;
        if !e1 {
            eprintln!("diff: {}: No such file or directory", quotef_os(p1));
            absent = true;
        }
        if !e2 {
            eprintln!("diff: {}: No such file or directory", quotef_os(p2));
            absent = true;
        }
        if absent {
            return 2;
        }
    }

    // Read file contents (absent files treated as empty when -N is set).
    let content1 = if e1 {
        match read_file(p1, config.text_mode) {
            Ok(c) => c,
            Err(msg) => {
                eprintln!("diff: {msg}");
                return 2;
            }
        }
    } else {
        FileContent::Text(Vec::new(), FinalNewline::Present)
    };

    let content2 = if e2 {
        match read_file(p2, config.text_mode) {
            Ok(c) => c,
            Err(msg) => {
                eprintln!("diff: {msg}");
                return 2;
            }
        }
    } else {
        FileContent::Text(Vec::new(), FinalNewline::Present)
    };

    // Handle binary files.
    match (&content1, &content2) {
        (FileContent::Binary, _) | (_, FileContent::Binary) => {
            // The wording DOES depend on --brief, which the comment that
            // stood here denied. Measured:
            //
            //     diff  nul.txt nul2.txt   Binary files nul.txt and nul2.txt differ
            //     diff -q nul.txt nul2.txt Files nul.txt and nul2.txt differ
            //
            // `-q` asks only whether the files differ, and at that level a
            // binary file is not a special case -- so it gets the same
            // sentence a pair of text files gets.
            if config.brief {
                print_path_line(&[b"Files ", &pb(p1), b" and ", &pb(p2), b" differ"]);
            } else {
                print_path_line(&[b"Binary files ", &pb(p1), b" and ", &pb(p2), b" differ"]);
            }
            return 1;
        }
        _ => {}
    }

    let (lines1, nl1) = match &content1 {
        FileContent::Text(l, nl) => (l, *nl),
        FileContent::Binary => return 2, // unreachable due to match above
    };
    let (lines2, nl2) = match &content2 {
        FileContent::Text(l, nl) => (l, *nl),
        FileContent::Binary => return 2,
    };

    // Compute the diff.
    let mut ops = compute_diff(lines1, lines2, nl1, nl2, config);
    mark_missing_newlines(&mut ops, nl1, nl2);

    // Check if there are any differences.
    //
    // `-I` is applied HERE, above the `-q` branch, because it changes what
    // counts as a difference rather than what is printed about one: measured,
    // `diff -q -I '^#'` on files differing only in a `#` line prints nothing
    // and exits 0. Filtering later would have left `-q` saying they differ.
    let has_diff = ops.iter().any(|e| e.op != Op::Equal)
        && !build_hunks(&ops, 0)
            .iter()
            .all(|h| hunk_is_ignorable(h, config));

    // `-y` is the one format that prints something for files with no
    // difference: every line, as common lines -- upstream's
    // `no_diff_means_no_output` is false for it, unless
    // `--suppress-common-lines` leaves it nothing to show.
    let show_anyway =
        config.format == Format::SideBySide && !config.suppress_common_lines && !config.brief;
    if !has_diff && !show_anyway {
        if config.report_identical {
            print_path_line(&[b"Files ", &pb(p1), b" and ", &pb(p2), b" are identical"]);
        }
        return 0;
    }

    if config.brief {
        print_path_line(&[b"Files ", &pb(p1), b" and ", &pb(p2), b" differ"]);
        return 1;
    }

    // THE `diff -r da/x.txt db/x.txt` LINE, printed here rather than by the
    // directory walk, because the conditions for it are only known now.
    // Measured, three ways round:
    //
    //   * it appears only for a file that DIFFERS -- an identical pair in the
    //     same walk gets no line, and under `-s` its `are identical` line has
    //     no header either, which is why this sits below the `!has_diff`
    //     return rather than above it;
    //   * `--brief` prints none at all, which is why it sits below that return
    //     too;
    //   * the options are echoed as TYPED.
    if in_dir_walk {
        // Bytes: an option word is echoed exactly as typed, and `diff`
        // accepts words it does not recognise as operands rather than
        // rejecting them, so this must not assume Unicode.
        let mut line: Vec<u8> = b"diff".to_vec();
        for word in &config.option_words {
            line.push(b' ');
            line.extend_from_slice(&quoting::os_bytes(word));
        }
        print_path_line(&[&line, b" ", &qname(p1), b" ", &qname(p2)]);
    }

    // Format the output.
    match config.format {
        Format::SideBySide => {
            print_side_by_side(&ops, config);
        }
        Format::Normal => {
            let hunks = unignored(build_hunks(&ops, 0), config);
            print_normal(&hunks, config);
        }
        // Both take zero context: an `ed` script and an RCS delta are
        // instructions, not a readable rendering, so a surrounding line would
        // be applied as though it were a change.
        Format::Ed => {
            let hunks = unignored(build_hunks(&ops, 0), config);
            print_ed(&hunks, config);
        }
        Format::Rcs => {
            let hunks = unignored(build_hunks(&ops, 0), config);
            print_rcs(&hunks, config);
        }
        Format::Unified => {
            let hunks = unignored(build_hunks(&ops, config.context_lines), config);
            print_unified(&hunks, p1, p2, config);
        }
        Format::Context => {
            let hunks = unignored(build_hunks(&ops, config.context_lines), config);
            print_context(&hunks, p1, p2, config);
        }
    }

    if !has_diff {
        if config.report_identical {
            print_path_line(&[b"Files ", &pb(p1), b" and ", &pb(p2), b" are identical"]);
        }
        return 0;
    }
    1
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS diff v{VERSION}");
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
    println!("  --suppress-common-lines    With -y, print only the lines that differ");
    println!("  --left-column              With -y, print common lines in the left column only");
    println!("  --tabsize=NUM              Tab stops every NUM columns (default 8)");
    println!();
    println!("FILTERING:");
    println!("  -q, --brief                 Only report whether files differ");
    println!("  -s, --report-identical-files Report identical files");
    println!("  -i, --ignore-case           Case-insensitive comparison");
    println!("  -b, --ignore-space-change   Ignore changes in whitespace amount");
    println!("  -w, --ignore-all-space      Ignore all whitespace");
    println!("  -Z, --ignore-trailing-space Ignore whitespace at line end");
    println!("  -a, --text                  Treat all files as text");
    println!("  -B, --ignore-blank-lines    Ignore blank line changes");
    println!();
    println!("OUTPUT:");
    println!("      --color[=WHEN]        Color the output: never, always or auto");
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
    // `args_os`, not `args`: the latter's iterator unwraps, so a filename
    // holding a byte that is not valid Unicode killed the process here.
    let args: Vec<OsString> = env::args_os().collect();

    match parse_args(&args) {
        ParseResult::Help => {
            print_help();
            process::exit(0);
        }
        ParseResult::Version => {
            println!("diff (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Fail(message) => {
            stdfd::diag_line(&message);
            process::exit(2);
        }
        ParseResult::Run(config) => {
            let p1 = PathBuf::from(&config.path1);
            let p2 = PathBuf::from(&config.path2);

            let is_dir1 = p1.is_dir();
            let is_dir2 = p2.is_dir();

            // TWO DIRECTORIES ARE COMPARED WITH OR WITHOUT `-r`. This build
            // refused without it -- `diff: da and db are directories (use -r to
            // compare recursively)`, exit 2 -- where GNU compares the files
            // they hold and reports `Common subdirectories:` for the ones it
            // will not descend into. `-r` chooses whether to DESCEND, not
            // whether to compare at all, and the refusal made the common
            // `diff olddir newdir` fail outright.
            let exit_code = if is_dir1 && is_dir2 {
                diff_dirs(&p1, &p2, &config)
            } else if is_dir1 || is_dir2 {
                // One is a directory, one is a file -- diff the file against
                // the same-named file in the directory.
                let (dir, file) = if is_dir1 { (&p1, &p2) } else { (&p2, &p1) };

                if let Some(fname) = file.file_name() {
                    // Paths, not decoded strings. `to_string_lossy` here meant
                    // that `diff dir file` where either name held a byte that
                    // is not valid Unicode built a path with U+FFFD in it and
                    // then failed to open it.
                    let target = dir.join(fname);

                    if is_dir1 {
                        diff_files(&target, file, &config, false)
                    } else {
                        diff_files(file, &target, &config, false)
                    }
                } else {
                    eprintln!("diff: cannot determine filename from path");
                    2
                }
            } else {
                diff_files(
                    Path::new(&config.path1),
                    Path::new(&config.path2),
                    &config,
                    false,
                )
            };

            process::exit(exit_code);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

/// WHAT SURVIVED THE PORT AND WHAT COULD NOT.
///
/// The implementation this replaced carried fourteen tests; the implementation
/// that replaced it carried none. A straight copy would therefore have deleted
/// every test this utility has, which is why these exist.
///
/// **Retargeted, because the behaviour is the same:** the `parse_args` cases,
/// and the edit-script cases, which now drive `compute_diff` rather than a
/// helper called `edits`.
///
/// **Rewritten, because the contract changed under the same name:** `range_str`
/// took `(start, end)` with a 1-based inclusive end and now takes
/// `(start, count)` with a 0-based start. The old assertions -- `(5,5)` is
/// `"5"`, `(2,7)` is `"2,7"` -- are *wrong* against the new function, and
/// porting them verbatim would have pinned a contract nothing implements. Two
/// functions, one name, one arity, different meaning: the case where copying a
/// test across is most obviously safe and is not.
///
/// **Could not survive:** the four `lcs_table_*` cases. They asserted the shape
/// of a dynamic-programming table that this implementation does not build --
/// it uses Myers, which has no such table. A test of an internal representation
/// cannot outlive the representation, and there is no honest way to retarget
/// one. They are named here rather than deleted in silence, because a test
/// removed with a reason is a decision and a test removed quietly is an
/// accident that looks like a decision six months later.
///
/// **Not testable here at all, and this is a REGRESSION the port introduces:**
/// every refusal. `parse_args` calls `process::exit(2)` on a bad option, an
/// invalid context length and a missing `--width` argument, so none of them can
/// be driven from a unit test -- the process would leave. The implementation
/// this replaced returned a struct and could be asked. So the half that wins on
/// behaviour loses on testability, and the two-probe rule cannot be satisfied
/// here: these tests prove the parser accepts, and nothing proves it refuses.
/// The fix is for `parse_args` to return a `Result` and let `main` exit;
/// recorded in `todo.txt` rather than done in the same change as the port.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::arithmetic_side_effects)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A hunk of exactly one line drops the `,1` from its `@@` header.
    ///
    /// Measured: `diff -U0` on a one-line change gives `@@ -20 +20 @@`, and
    /// this build wrote `@@ -20,1 +20,1 @@`. `patch` reads both -- its
    /// `parse_range` already treats a bare number as a count of one -- so
    /// nothing downstream complained, which is exactly why the harness is the
    /// thing that found it.
    #[test]
    fn a_one_line_range_is_written_without_its_count() {
        assert_eq!(range_field(20, 1), "20");
        // Every other count keeps the comma, including the two that bracket 1.
        assert_eq!(range_field(20, 0), "20,0");
        assert_eq!(range_field(20, 2), "20,2");
        assert_eq!(range_field(1, 4), "1,4");
    }

    // ---------------- the header timestamp ----------------

    /// A pre-1970 mtime keeps its nanoseconds on the right side of the second.
    ///
    /// `duration_since` hands a pre-epoch instant back as a POSITIVE gap the
    /// other way round, so the obvious negation is wrong by up to a second in
    /// the wrong direction -- and `strftime` would render the result without
    /// complaint, because both fields are individually valid.
    #[test]
    fn an_mtime_before_the_epoch_keeps_positive_nanoseconds() {
        use std::time::{Duration, UNIX_EPOCH};
        // Exactly on the epoch.
        assert_eq!(epoch_parts(UNIX_EPOCH), (0, 0));
        // A whole second before: no remainder to carry.
        assert_eq!(epoch_parts(UNIX_EPOCH - Duration::from_secs(1)), (-1, 0));
        // 0.75 s before the epoch is one second before, plus a quarter.
        let t = UNIX_EPOCH - Duration::from_nanos(750_000_000);
        assert_eq!(epoch_parts(t), (-1, 250_000_000));
        // 1.75 s before.
        let t = UNIX_EPOCH - Duration::from_nanos(1_750_000_000);
        assert_eq!(epoch_parts(t), (-2, 250_000_000));
        // After the epoch is the plain case, and is the control: without it a
        // function that negated everything would pass the cases above.
        let t = UNIX_EPOCH + Duration::from_nanos(1_250_000_000);
        assert_eq!(epoch_parts(t), (1, 250_000_000));
    }

    /// The header field is `<path>TAB<stamp>`, and the fraction is nine digits.
    ///
    /// Measured shape, from GNU:
    ///   `--- x.txt<TAB>2020-01-02 08:04:05.000000000 +0000`
    /// The TAB is what `patch` splits on, and a fraction printed with fewer
    /// digits still parses -- so nothing downstream would notice the loss.
    #[test]
    fn the_header_field_is_a_tab_then_a_nine_digit_stamp() {
        let field = header_field(Path::new("nosuchfile.txt"));
        let at = field
            .iter()
            .position(|&b| b == b'\t')
            .expect("the separator must be a TAB");
        assert_eq!(&field[..at], b"nosuchfile.txt");
        let stamp = &field[at + 1..];
        // `YYYY-MM-DD HH:MM:SS.NNNNNNNNN +ZZZZ`
        let dot = stamp
            .iter()
            .position(|&b| b == b'.')
            .expect("a fractional second must be present");
        let frac: Vec<u8> = stamp[dot + 1..]
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .collect();
        assert_eq!(frac.len(), 9, "GNU always prints nine digits");
        let last = stamp.split(|&b| b == b' ').next_back().unwrap_or(b"");
        assert_eq!(last.len(), 5, "a +ZZZZ offset, got {last:?}");
    }

    /// A context diff marks a CHANGED line `!` on both sides, and a pure
    /// removal or addition `-` / `+`.
    ///
    /// The symmetry is the point: the same `changed` flag has to turn both
    /// markers into `!`, and a build that converted only one of them would
    /// still produce a diff that looked right.
    #[test]
    fn a_changed_context_line_is_marked_on_both_sides() {
        assert_eq!(context_marker(Op::Delete, true), b"! ");
        assert_eq!(context_marker(Op::Insert, true), b"! ");
        assert_eq!(context_marker(Op::Delete, false), b"- ");
        assert_eq!(context_marker(Op::Insert, false), b"+ ");
        // Context is two spaces either way -- `changed` does not reach it.
        assert_eq!(context_marker(Op::Equal, true), b"  ");
        assert_eq!(context_marker(Op::Equal, false), b"  ");
    }

    /// `-Z` ignores whitespace at the END of a line and nowhere else.
    ///
    /// Narrower than `-b`, which collapses runs anywhere. A file that differs
    /// only in trailing spaces is what an editor leaves behind, and GNU exits
    /// 0 on it under `-Z`.
    #[test]
    fn ignore_trailing_space_trims_only_the_end() {
        let mut z = cfg();
        z.ignore_trailing_space = true;
        assert_eq!(normalize_line(b"a b   ", &z), b"a b");
        assert_eq!(normalize_line(b"a b\t", &z), b"a b");
        // The controls: leading and interior space are untouched.
        assert_eq!(normalize_line(b"  a b", &z), b"  a b");
        assert_eq!(normalize_line(b"a   b", &z), b"a   b");
        // And without the flag nothing is trimmed at all.
        assert_eq!(normalize_line(b"a b   ", &cfg()), b"a b   ");
    }

    /// `-a` makes a file holding NUL bytes compare as text.
    #[test]
    fn text_mode_stops_a_nul_meaning_binary() {
        let data = b"a\0nul\n".to_vec();
        assert!(
            matches!(classify(data.clone(), false), FileContent::Binary),
            "a NUL means binary without -a"
        );
        match classify(data, true) {
            FileContent::Text(lines, _) => {
                assert_eq!(lines, vec![b"a\0nul".to_vec()]);
            }
            FileContent::Binary => panic!("-a should have made this text"),
        }
    }

    // ---------------- the missing final newline ----------------

    /// The same four lines, one file terminated and one not, are DIFFERENT.
    ///
    /// This build called them identical and exited 0 on files of 26 and 25
    /// bytes, because splitting into lines throws the terminator away and both
    /// sides then yield the same four lines. GNU prints `4c4`.
    ///
    /// See known-issues.md -> B-DIFF-CANNOT-SEE-A-MISSING-FINAL-NEWLINE.
    #[test]
    fn a_missing_final_newline_is_a_difference() {
        let a = s(&["alpha", "bravo", "charlie", "delta"]);
        let script = compute_diff(&a, &a.clone(), NL, FinalNewline::Missing, &cfg());
        assert!(
            script.iter().any(|e| e.op != Op::Equal),
            "the terminator is part of the file, got {script:?}"
        );
    }

    /// The control: when NEITHER file ends with a newline they are equal.
    ///
    /// Without this, an implementation that simply called the last lines
    /// different whenever it was asked would pass the case above.
    #[test]
    fn two_files_that_both_lack_a_final_newline_are_equal() {
        let a = s(&["alpha", "bravo", "charlie", "delta"]);
        let script = compute_diff(
            &a,
            &a.clone(),
            FinalNewline::Missing,
            FinalNewline::Missing,
            &cfg(),
        );
        assert!(
            script.iter().all(|e| e.op == Op::Equal),
            "both unterminated is still the same file, got {script:?}"
        );
    }

    /// The second control: two terminated files are equal, as always.
    #[test]
    fn two_terminated_files_are_equal() {
        let a = s(&["alpha", "delta"]);
        let script = compute_diff(&a, &a.clone(), NL, NL, &cfg());
        assert!(script.iter().all(|e| e.op == Op::Equal), "{script:?}");
    }

    /// The marker lands on the last line of the side that lacks the newline,
    /// and on that side only.
    ///
    /// Measured, both directions:
    ///   `diff withnl nonl`  -> marker after the `>` line
    ///   `diff nonl withnl`  -> marker after the `<` line
    #[test]
    fn the_marker_goes_on_the_side_that_lacks_the_newline() {
        let mut edits = vec![
            Edit::new(Op::Equal, b"alpha".to_vec()),
            Edit::new(Op::Delete, b"delta".to_vec()),
            Edit::new(Op::Insert, b"delta".to_vec()),
        ];
        mark_missing_newlines(&mut edits, NL, FinalNewline::Missing);
        assert!(!edits[1].no_final_newline, "the old side is terminated");
        assert!(edits[2].no_final_newline, "the new side is not");

        let mut edits = vec![
            Edit::new(Op::Equal, b"alpha".to_vec()),
            Edit::new(Op::Delete, b"delta".to_vec()),
            Edit::new(Op::Insert, b"delta".to_vec()),
        ];
        mark_missing_newlines(&mut edits, FinalNewline::Missing, NL);
        assert!(edits[1].no_final_newline, "the old side is not terminated");
        assert!(!edits[2].no_final_newline, "the new side is");
    }

    /// An `Equal` line is the last line of BOTH files at once, which is the
    /// single-hunk-at-the-end case and must be marked from either side.
    #[test]
    fn an_equal_line_can_be_the_last_line_of_both_files() {
        let mut edits = vec![Edit::new(Op::Equal, b"delta".to_vec())];
        mark_missing_newlines(&mut edits, FinalNewline::Missing, FinalNewline::Missing);
        assert!(edits[0].no_final_newline);
    }

    // ---------------- bytes, not UTF-8 ----------------

    /// Two lines that differ, in bytes that are not valid UTF-8, must differ.
    ///
    /// This is the bug `from_utf8_lossy` caused: both \xe9 and \xff decoded to
    /// U+FFFD, so the comparison saw one character twice and called the files
    /// identical -- no output, exit 0 -- where GNU prints `2c2`.
    /// `scripts/diff-diff.sh` had the fixtures for this all along
    /// (`bytes.txt` / `bytes2.txt`) and three of its cases were red because of
    /// it; they went green with this change and nothing else moved.
    ///
    /// See known-issues.md -> B-DIFF-SAYS-TWO-DIFFERENT-FILES-ARE-IDENTICAL.
    #[test]
    fn two_distinct_bytes_that_are_not_utf8_are_not_the_same_line() {
        let a = vec![b"caf\xe9".to_vec()];
        let b = vec![b"caf\xff".to_vec()];
        let script = compute_diff(&a, &b, NL, NL, &cfg());
        assert!(
            script.iter().any(|e| e.op != Op::Equal),
            "a difference must be reported, got {script:?}"
        );
    }

    /// The control for the case above: the SAME bad byte really is equal.
    ///
    /// Without this, a `compute_diff` that called everything different would
    /// pass the test above and be just as wrong.
    #[test]
    fn the_same_byte_that_is_not_utf8_is_still_equal() {
        let a = vec![b"caf\xe9".to_vec()];
        let script = compute_diff(&a, &a.clone(), NL, NL, &cfg());
        assert!(
            script.iter().all(|e| e.op == Op::Equal),
            "identical input must produce no edits, got {script:?}"
        );
    }

    /// `-i` folds ASCII case and NOT Unicode case, which is what GNU does.
    ///
    /// Measured, with the ASCII pair as the control:
    ///   `caf<U+00C9>` vs `caf<U+00E9>` -> GNU reports DIFFERENT
    ///   `ABC` vs `abc`                 -> GNU reports SAME
    #[test]
    fn ignore_case_folds_ascii_and_leaves_unicode_alone() {
        let mut ci = cfg();
        ci.ignore_case = true;
        assert_eq!(normalize_line(b"ABC", &ci), b"abc");
        assert_ne!(
            normalize_line("caf\u{c9}".as_bytes(), &ci),
            normalize_line("caf\u{e9}".as_bytes(), &ci)
        );
    }

    /// A line keeps its bytes through the splitter, and a CR goes with the LF.
    #[test]
    fn split_lines_keeps_bytes_and_drops_the_terminator() {
        assert_eq!(
            split_lines(b"one\ntwo"),
            vec![b"one".to_vec(), b"two".to_vec()]
        );
        assert_eq!(split_lines(b"a\r\nb\n"), vec![b"a".to_vec(), b"b".to_vec()]);
        // A trailing newline does not invent a final empty line.
        assert_eq!(split_lines(b"x\n").len(), 1);
    }

    /// The common case: both files end with a newline.
    const NL: FinalNewline = FinalNewline::Present;

    /// File content as the differ now takes it: a line is bytes.
    fn s(items: &[&str]) -> Vec<Vec<u8>> {
        items.iter().map(|x| x.as_bytes().to_vec()).collect()
    }

    /// argv, which is a separate question from content and still a `String`
    /// here. `diff` remains in `scripts/argv-utf8-baseline.txt`: it cannot yet
    /// take a filename that is not Unicode. That is the smaller of the two
    /// faults -- it fails loudly -- and it is deliberately not mixed into the
    /// change that fixes the one returning a wrong answer.
    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// A Config with every knob off, so a test names only what it changes.
    fn cfg() -> Config {
        Config {
            path1: OsString::new(),
            path2: OsString::new(),
            format: Format::Normal,
            context_lines: 3,
            width: 130,
            brief: false,
            ignore_matching: Vec::new(),
            report_identical: false,
            ignore_case: false,
            ignore_space_change: false,
            ignore_all_space: false,
            ignore_blank_lines: false,
            color: false,
            recursive: false,
            new_file: false,
            ignore_trailing_space: false,
            text_mode: false,
            expand_tabs: false,
            initial_tab: false,
            suppress_common_lines: false,
            left_column: false,
            tabsize: DEFAULT_TABSIZE,
            option_words: Vec::new(),
        }
    }

    fn run(args: &[&str]) -> Config {
        match parse_args(&argv(args)) {
            ParseResult::Run(c) => c,
            _ => panic!("expected Run"),
        }
    }

    /// The text of a refusal, without its referral line.
    fn fail(args: &[&str]) -> String {
        match parse_args(&argv(args)) {
            ParseResult::Fail(message) => message
                .strip_suffix("\ndiff: Try 'diff --help' for more information.")
                .unwrap_or(&message)
                .to_string(),
            _ => panic!("expected a refusal for {args:?}"),
        }
    }

    // ---------------- the command line, as diffutils 3.10 reads it ----------------

    /// Upstream's `specify_style`: the first style stands and a different one
    /// after it is refused. The ladder this replaced kept the last.
    #[test]
    fn two_output_styles_conflict() {
        assert_eq!(
            fail(&["diff", "-u", "-c", "a", "b"]),
            "diff: conflicting output style options"
        );
        assert_eq!(
            fail(&["diff", "-y", "--normal", "a", "b"]),
            "diff: conflicting output style options"
        );
        // The same style twice is not a conflict.
        assert!(matches!(
            run(&["diff", "-u", "--unified", "a", "b"]).format,
            Format::Unified
        ));
    }

    /// Long options abbreviate to a unique prefix; `--no-color` was never
    /// diffutils' and is refused as GNU refuses it.
    #[test]
    fn long_options_abbreviate() {
        assert!(matches!(
            run(&["diff", "--unif", "a", "b"]).format,
            Format::Unified
        ));
        assert!(matches!(
            run(&["diff", "--side", "a", "b"]).format,
            Format::SideBySide
        ));
        assert!(run(&["diff", "--ignore-c", "a", "b"]).ignore_case);
        assert_eq!(
            fail(&["diff", "--no-color", "a", "b"]),
            "diff: unrecognized option '--no-color'"
        );
    }

    /// Context lengths keep the largest asked for; `-u`/`-c` ask for three;
    /// the obsolete digits accumulate across words and win over a defaulted
    /// context but not over a larger explicit one.
    #[test]
    fn context_lengths_follow_upstream() {
        assert_eq!(
            run(&["diff", "-U", "5", "-U", "2", "a", "b"]).context_lines,
            5
        );
        assert_eq!(run(&["diff", "-u", "-U", "1", "a", "b"]).context_lines, 3);
        assert_eq!(run(&["diff", "-U", "0", "a", "b"]).context_lines, 0);
        assert_eq!(run(&["diff", "-u2", "a", "b"]).context_lines, 2);
        assert_eq!(run(&["diff", "-1", "-2", "-u", "a", "b"]).context_lines, 12);
        assert_eq!(run(&["diff", "-U", "5", "-2", "a", "b"]).context_lines, 5);
        assert_eq!(run(&["diff", "--context", "a", "b"]).context_lines, 3);
        assert_eq!(run(&["diff", "--context=7", "a", "b"]).context_lines, 7);
        assert_eq!(
            fail(&["diff", "-U", "x", "a", "b"]),
            "diff: invalid context length 'x'"
        );
        assert_eq!(
            fail(&["diff", "-U", "-1", "a", "b"]),
            "diff: invalid context length '-1'"
        );
    }

    /// Upstream names the last word after getopt's permutation: the last
    /// operand, so `diff x -u` is `after 'x'`, not `after '-u'`.
    #[test]
    fn operand_errors_name_upstreams_word() {
        assert_eq!(
            fail(&["diff", "x", "-u"]),
            "diff: missing operand after 'x'"
        );
        assert_eq!(fail(&["diff", "-u"]), "diff: missing operand after '-u'");
        assert_eq!(fail(&["diff"]), "diff: missing operand after 'diff'");
        assert_eq!(fail(&["diff", "a", "b", "c"]), "diff: extra operand 'c'");
        // `--` ends the options: a file called `-u`.
        let c = run(&["diff", "--", "-u", "b"]);
        assert_eq!(c.path1, OsString::from("-u"));
        assert!(matches!(c.format, Format::Normal));
    }

    #[test]
    fn color_takes_upstreams_three_words_exactly() {
        assert!(run(&["diff", "--color=always", "a", "b"]).color);
        assert!(!run(&["diff", "--color=never", "a", "b"]).color);
        assert_eq!(
            fail(&["diff", "--color=alw", "a", "b"]),
            "diff: invalid color 'alw'"
        );
    }

    /// An option diffutils has and this build does not is refused by name,
    /// never ignored; the ones whose effect this build already has are taken.
    #[test]
    fn unimplemented_options_are_refused_and_no_ops_accepted() {
        assert_eq!(
            fail(&["diff", "-D", "X", "a", "b"]),
            "diff: option -D is not implemented by this diff"
        );
        assert_eq!(
            fail(&["diff", "--label=x", "a", "b"]),
            "diff: option '--label' is not implemented by this diff"
        );
        let c = run(&[
            "diff",
            "-d",
            "-H",
            "--horizon-lines=5",
            "--inhibit-hunk-merge",
            "a",
            "b",
        ]);
        assert!(matches!(c.format, Format::Normal));
        assert_eq!(
            fail(&["diff", "--horizon-lines=x", "a", "b"]),
            "diff: invalid horizon length 'x'"
        );
    }

    // ---------------- -t / -T ----------------

    /// Render one body line through the real writer, so the marker rules are
    /// tested where they actually live rather than in a copy of them.
    fn marked(config: &Config, marker: &[u8], text: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        write_text_line(&mut out, config, marker, text, None);
        out
    }

    /// Tab stops are counted from the start of the TEXT, not of the output
    /// line -- the `< ` marker does not shift them. Measured against GNU.
    #[test]
    fn expand_tabs_counts_columns_from_the_text_not_the_marker() {
        assert_eq!(
            expand_output_tabs(b"a\tb", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"a       b"[..]
        );
        assert_eq!(
            expand_output_tabs(b"ab\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"ab      z"[..]
        );
        // A tab already sitting ON a stop still advances a full eight.
        assert_eq!(
            expand_output_tabs(b"abcdefgh\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"abcdefgh        z"[..]
        );
        // Control: a line with no tab comes back byte-identical.
        assert_eq!(
            expand_output_tabs(b"plain", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"plain"[..]
        );
    }

    /// Only printable ASCII has width. Every byte of a multi-byte character
    /// counts as ZERO, so `e`-acute then a tab yields a full eight spaces --
    /// the same as a line that opens with the tab. Measured, and unchanged
    /// under `LC_ALL=C.UTF-8`; this is not a shortcut for character counting.
    #[test]
    fn a_byte_that_is_not_printable_ascii_has_no_width() {
        // 0xC3 0xA9 is U+00E9. One character, two bytes, zero columns.
        assert_eq!(
            expand_output_tabs(b"\xc3\xa9\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"\xc3\xa9        z"[..]
        );
        // Two of them: still zero, so still eight. Not "one column each".
        assert_eq!(
            expand_output_tabs(b"\xc3\xa9\xc3\xa9\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"\xc3\xa9\xc3\xa9        z"[..]
        );
        // Control bytes and DEL are printed but weightless.
        assert_eq!(
            expand_output_tabs(b"a\x01\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"a\x01       z"[..]
        );
        assert_eq!(
            expand_output_tabs(b"a\x7f\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"a\x7f       z"[..]
        );
        // A space, by contrast, is printable and does have width.
        assert_eq!(
            expand_output_tabs(b"a \tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"a       z"[..]
        );
    }

    /// A backspace backs up one column -- but at column 0 it is DROPPED
    /// rather than printed, so a line cannot back over its own marker.
    #[test]
    fn a_backspace_backs_up_a_column_and_at_column_zero_is_dropped() {
        assert_eq!(
            expand_output_tabs(b"ab\x08\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"ab\x08       z"[..]
        );
        // All three vanish, and the tab then spans a full eight.
        assert_eq!(
            expand_output_tabs(b"\x08\x08\x08\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"        z"[..]
        );
    }

    /// A carriage return re-emits the marker and restarts the column. On a
    /// CRLF file the terminal returns to the left margin, so without this the
    /// text would overprint the `<`.
    #[test]
    fn a_carriage_return_reprints_the_marker_and_restarts_the_column() {
        assert_eq!(
            expand_output_tabs(b"ab\rz", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"ab\r< z"[..]
        );
        // The column really did reset: the tab that follows spans a full
        // eight, not the six it would span from column 2.
        let mut want: Vec<u8> = b"ab\r< ".to_vec();
        want.extend_from_slice(&[b' '; 8]);
        want.push(b'z');
        assert_eq!(
            expand_output_tabs(b"ab\r\tz", b"< ", DEFAULT_TABSIZE).as_slice(),
            want.as_slice()
        );
        // A line ENDING in a carriage return gets no second marker: there is
        // nothing left to overprint.
        assert_eq!(
            expand_output_tabs(b"ab\r", b"< ", DEFAULT_TABSIZE).as_slice(),
            &b"ab\r"[..]
        );
    }

    /// `-T` replaces the space the marker already carries; a marker with no
    /// space simply gains a tab. An EMPTY marker stays empty -- side-by-side
    /// passes one, and GNU prints no separator at all when there is no flag.
    #[test]
    fn initial_tab_replaces_the_markers_space_and_leaves_an_empty_marker_empty() {
        let mut c = cfg();
        c.initial_tab = true;
        assert_eq!(marked(&c, b"< ", b"x").as_slice(), &b"<\tx\n"[..]);
        assert_eq!(marked(&c, b"-", b"x").as_slice(), &b"-\tx\n"[..]);
        assert_eq!(marked(&c, b"", b"x").as_slice(), &b"x\n"[..]);
        // Control: off by default, the space stays a space.
        assert_eq!(marked(&cfg(), b"< ", b"x").as_slice(), &b"< x\n"[..]);
    }

    /// The marker reprinted after a carriage return carries the tab too --
    /// it is the same marker, so `-T` cannot apply to only the first one.
    #[test]
    fn the_marker_reprinted_after_a_carriage_return_carries_the_initial_tab() {
        let mut c = cfg();
        c.expand_tabs = true;
        c.initial_tab = true;
        assert_eq!(
            marked(&c, b"< ", b"ab\rz").as_slice(),
            &b"<\tab\r<\tz\n"[..]
        );
    }

    /// Both spellings of both options, and neither implies the other.
    #[test]
    fn expand_tabs_and_initial_tab_parse_and_are_independent() {
        assert!(run(&["diff", "-t", "a", "b"]).expand_tabs);
        assert!(run(&["diff", "--expand-tabs", "a", "b"]).expand_tabs);
        assert!(run(&["diff", "-T", "a", "b"]).initial_tab);
        assert!(run(&["diff", "--initial-tab", "a", "b"]).initial_tab);

        let both = run(&["diff", "-tT", "a", "b"]);
        assert!(both.expand_tabs && both.initial_tab);

        assert!(!run(&["diff", "-t", "a", "b"]).initial_tab);
        assert!(!run(&["diff", "-T", "a", "b"]).expand_tabs);
        // Control: neither is on when neither is asked for.
        let neither = run(&["diff", "a", "b"]);
        assert!(!neither.expand_tabs && !neither.initial_tab);
    }

    // ---------------- parse_args ----------------

    /// `--width 80` and `-W 80` take the FOLLOWING word.
    ///
    /// Nothing covered this. The attached forms `--width=80` and `-W80` were
    /// exercised through other paths, and the two-word form was not, which is
    /// how a refactor nearly shipped a `diff --width 80` that parsed
    /// `"--width"` as the number and exited 2.
    ///
    /// Both sites read `args[i]` AFTER an `i += 1`, so they LOOK like the
    /// argument the loop is holding and are not. A scripted rewrite of this
    /// file treated them as the loop's own `arg`; the only thing that stopped
    /// it was the anchor matching twice instead of once.
    #[test]
    fn width_takes_the_following_word() {
        // Two words, long and short.
        assert_eq!(run(&["diff", "--width", "80", "a", "b"]).width, 80);
        assert_eq!(run(&["diff", "-W", "80", "a", "b"]).width, 80);

        // Attached, which is what was already covered.
        assert_eq!(run(&["diff", "--width=80", "a", "b"]).width, 80);
        assert_eq!(run(&["diff", "-W80", "a", "b"]).width, 80);

        // The operand after the value is still an operand: `-W` must consume
        // exactly one word, not two.
        let c = run(&["diff", "-W", "80", "left", "right"]);
        assert_eq!(c.path1, "left");
        assert_eq!(c.path2, "right");

        // And the default survives, so a passing assertion above cannot be
        // the default agreeing with the expected value by accident.
        assert_eq!(run(&["diff", "a", "b"]).width, 130);
    }

    /// A bare `-` is an OPERAND, not an option.
    ///
    /// It was falling into the option branch on `starts_with('-')`, so
    /// `cmd | diff base.txt -` answered `missing operand after '-'` -- the
    /// most ordinary way anyone writes a diff in a pipeline.
    #[test]
    fn a_bare_dash_is_an_operand() {
        let c = run(&["diff", "base.txt", "-"]);
        assert_eq!(c.path1, "base.txt");
        assert_eq!(c.path2, "-");
        assert!(c.option_words.is_empty(), "{:?}", c.option_words);
        // ...on either side.
        let c = run(&["diff", "-", "base.txt"]);
        assert_eq!(c.path1, "-");
        // A real option is still an option, and `--` still ends them.
        let c = run(&["diff", "-u", "a", "b"]);
        assert_eq!(c.option_words, vec!["-u"]);
    }

    /// Only a BARE `-` is stdin. `./-` names a file called `-`.
    #[test]
    fn only_a_bare_dash_names_stdin() {
        assert!(is_stdin(Path::new("-")));
        assert!(!is_stdin(Path::new("./-")));
        assert!(!is_stdin(Path::new("-x")));
        assert!(!is_stdin(Path::new("a-")));
        assert!(!is_stdin(Path::new("")));
    }

    /// The `diff -r a/x b/x` header echoes the options AS TYPED, and an
    /// operand that looks like an option's value is still an operand.
    ///
    /// The words are collected by INDEX rather than by subtracting the operand
    /// list from argv by content. `diff -U 5 5 other` names a file called `5`,
    /// and a content-based subtraction would remove the wrong `5` -- leaving
    /// the header reading `diff -U` and the file list intact, which is the
    /// kind of wrong that looks right.
    #[test]
    fn the_option_words_are_kept_as_typed() {
        let c = run(&["diff", "-r", "-u", "a", "b"]);
        assert_eq!(c.option_words, vec!["-r", "-u"]);
        // Measured: GNU echoes `-ru` as `-ru`, not as `-r -u`.
        let c = run(&["diff", "-ru", "a", "b"]);
        assert_eq!(c.option_words, vec!["-ru"]);
        // An option's value is not an operand.
        let c = run(&["diff", "-U", "5", "a", "b"]);
        assert_eq!(c.option_words, vec!["-U", "5"]);
        // ...and an operand spelled like one is still an operand.
        let c = run(&["diff", "-U", "5", "5", "other"]);
        assert_eq!(c.option_words, vec!["-U", "5"]);
        assert_eq!(c.path1, "5");
        assert_eq!(c.path2, "other");
    }

    #[test]
    fn parse_files_only() {
        let c = run(&["diff", "a.txt", "b.txt"]);
        assert_eq!(c.path1, "a.txt");
        assert_eq!(c.path2, "b.txt");
        assert!(matches!(c.format, Format::Normal));
    }

    #[test]
    fn parse_unified_and_brief() {
        assert!(matches!(
            run(&["diff", "-u", "a", "b"]).format,
            Format::Unified
        ));
        assert!(run(&["diff", "-q", "a", "b"]).brief);
        assert!(run(&["diff", "--brief", "a", "b"]).brief);
    }

    #[test]
    fn parse_combined_short_flags() {
        // `-qi` is two flags in one word, which is the case a parser that
        // walks argv rather than characters gets wrong.
        let c = run(&["diff", "-qi", "a", "b"]);
        assert!(c.brief);
        assert!(c.ignore_case);
    }

    #[test]
    fn parse_help_and_version_are_not_runs() {
        assert!(matches!(
            parse_args(&argv(&["diff", "--help"])),
            ParseResult::Help
        ));
        assert!(matches!(
            parse_args(&argv(&["diff", "--version"])),
            ParseResult::Version
        ));
    }

    // ---------------- range_str, on its CURRENT contract ----------------

    #[test]
    fn range_str_takes_a_start_and_a_count_not_two_endpoints() {
        // count 0: the line BEFORE which an insertion lands, printed as-is.
        assert_eq!(range_str(5, 0), "5");
        // count 1: a single line, converted from 0-based to 1-based.
        assert_eq!(range_str(4, 1), "5");
        // count > 1: an inclusive 1-based span.
        assert_eq!(range_str(1, 6), "2,7");
    }

    // ---------------- normalize_line ----------------

    #[test]
    fn normalize_line_applies_only_what_is_asked() {
        let plain = cfg();
        assert_eq!(normalize_line(b"  A  b  ", &plain), b"  A  b  ");

        let mut ci = cfg();
        ci.ignore_case = true;
        assert_eq!(normalize_line(b"AbC", &ci), b"abc");

        let mut all = cfg();
        all.ignore_all_space = true;
        assert_eq!(normalize_line(b" a b ", &all), b"ab");

        let mut some = cfg();
        some.ignore_space_change = true;
        // RUNS COLLAPSE, LEADING WHITESPACE DOES NOT VANISH -- it collapses to
        // one space, which is the whole difference between `-b` and `-w`. This
        // assertion originally read `"a b"`, the implementation said `" a b"`,
        // and GNU was asked rather than either being assumed:
        //
        //     diff -b "  a    b  " vs " a b"  -> SAME
        //     diff -b "  a    b  " vs "a b"   -> DIFFER
        //     diff -w "  a    b  " vs "a b"   -> SAME
        //
        // So the implementation was right and the test was wrong, which is the
        // useful direction for a disagreement between them to run.
        assert_eq!(normalize_line(b"  a    b  ", &some), b" a b");

        // ...and `-w` is the one that removes it, pinned here so the two
        // cannot quietly converge.
        let mut all2 = cfg();
        all2.ignore_all_space = true;
        assert_eq!(normalize_line(b"  a    b  ", &all2), b"ab");
    }

    /// `analyze_hunk`: a line of white space is blank only when white space
    /// is being ignored as well (`-Z`, `-b`, `-w`); otherwise only an empty
    /// line is.
    #[test]
    fn a_blank_line_is_empty_unless_white_space_is_ignored_too() {
        let plain = cfg();
        assert!(is_blank(b"", &plain));
        assert!(!is_blank(b"   ", &plain));
        for set in [
            |c: &mut Config| c.ignore_trailing_space = true,
            |c: &mut Config| c.ignore_space_change = true,
            |c: &mut Config| c.ignore_all_space = true,
        ] {
            let mut c = cfg();
            set(&mut c);
            assert!(is_blank(b"", &c));
            assert!(is_blank(b"   ", &c));
            assert!(is_blank(b"\t \x0b\t", &c));
            assert!(!is_blank(b" x ", &c));
        }
    }

    /// `-B` ignores a hunk only when every changed line in it is blank; a
    /// blank line inside a hunk with a real change is printed with it.
    #[test]
    fn blank_lines_are_ignored_by_the_hunk() {
        let mut c = cfg();
        c.ignore_blank_lines = true;
        let blank_only = [
            Edit::new(Op::Delete, Vec::new()),
            Edit::new(Op::Insert, Vec::new()),
        ];
        assert!(edits_are_ignorable(&blank_only, &c));
        let mixed = [
            Edit::new(Op::Delete, Vec::new()),
            Edit::new(Op::Delete, b"x".to_vec()),
        ];
        assert!(!edits_are_ignorable(&mixed, &c));
        let spaces = [Edit::new(Op::Insert, b"  ".to_vec())];
        assert!(!edits_are_ignorable(&spaces, &c));
        c.ignore_trailing_space = true;
        assert!(edits_are_ignorable(&spaces, &c));
    }

    // ---------------- -y ----------------

    /// The rows of TD-B-DIFF-SIDE-BY-SIDE-PADS-WITH-SPACES-WHERE-GNU-USES-TABS,
    /// measured against GNU before the rule was read from `diff.c`: the gutter
    /// character's column and the right column's, tabs not expanded.
    #[test]
    fn the_side_by_side_columns_are_the_measured_ones() {
        for (width, gutter, right) in [
            (20, 6, 8),
            (21, 10, 16),
            (30, 14, 16),
            (40, 19, 24),
            (100, 46, 48),
            (130, 62, 64),
            (200, 99, 104),
        ] {
            let (half, column2) = sdiff_columns(width, 8, false);
            assert_eq!(column2, right, "right column at width {width}");
            assert_eq!((half + column2 - 1) / 2, gutter, "gutter at width {width}");
        }
        // `-t` is a different layout, which is the trap the measurement fell
        // into: the gutter at 64, not 62.
        let (half, column2) = sdiff_columns(130, 8, true);
        assert_eq!((half, column2), (63, 67));
        assert_eq!((half + column2 - 1) / 2, 64);
    }

    fn sdiff(ops: &[Edit], set: impl Fn(&mut Config)) -> Vec<u8> {
        let mut c = cfg();
        c.width = 20;
        set(&mut c);
        side_by_side(ops, &c)
    }

    fn eq(a: &str, b: &str) -> Edit {
        Edit::equal(a.as_bytes(), b.as_bytes())
    }

    fn del(t: &str) -> Edit {
        Edit::new(Op::Delete, t.as_bytes().to_vec())
    }

    fn ins(t: &str) -> Edit {
        Edit::new(Op::Insert, t.as_bytes().to_vec())
    }

    /// At width 20 the halves are 5 wide, the gutter character sits at 6 and
    /// the right column starts at 8 -- a tab from anywhere before it.
    #[test]
    fn side_by_side_rows_are_laid_out_with_tabs() {
        assert_eq!(sdiff(&[eq("ab", "ab")], |_| {}), b"ab\tab\n");
        assert_eq!(sdiff(&[del("ab"), ins("cd")], |_| {}), b"ab    |\tcd\n");
        assert_eq!(sdiff(&[del("ab")], |_| {}), b"ab    <\n");
        assert_eq!(sdiff(&[ins("cd")], |_| {}), b"      >\tcd\n");
        // Each half is cut at five columns.
        assert_eq!(
            sdiff(&[eq("abcdefg", "abcdefg")], |_| {}),
            b"abcde\tabcde\n"
        );
        // An empty right line is not tabbed to.
        assert_eq!(sdiff(&[eq("", "")], |_| {}), b"\n");
    }

    /// Two removed against one added: the pair, then the surplus.
    #[test]
    fn side_by_side_pairs_a_hunk_and_then_its_surplus() {
        let out = sdiff(&[del("x1"), del("x2"), ins("y1")], |_| {});
        assert_eq!(out, b"x1    |\ty1\nx2    <\n");
        let out = sdiff(&[del("y1"), ins("x1"), ins("x2")], |_| {});
        assert_eq!(out, b"y1    |\tx1\n      >\tx2\n");
    }

    /// Under `-i` the two copies of a common line differ, and each column
    /// shows its own file's.
    #[test]
    fn side_by_side_shows_each_files_copy_of_a_common_line() {
        assert_eq!(sdiff(&[eq("Ab", "aB")], |_| {}), b"Ab\taB\n");
    }

    /// A changed pair where one line lacks its newline is marked `/` or `\`.
    #[test]
    fn side_by_side_marks_a_missing_newline_in_the_gutter() {
        let mut right = ins("b");
        right.no_final_newline = true;
        assert_eq!(sdiff(&[del("a"), right], |_| {}), b"a     /\tb\n");
        let mut left = del("a");
        left.no_final_newline = true;
        assert_eq!(sdiff(&[left, ins("b")], |_| {}), b"a     \\\tb\n");
    }

    /// An ignored hunk's lines are common lines, paired off in order with the
    /// common lines around them, the surplus marked `)` or `(`.
    #[test]
    fn side_by_side_prints_an_ignored_hunk_as_common_lines() {
        let ops = [
            eq("e1", "e1"),
            del("x1"),
            ins("x2"),
            ins("x3"),
            eq("e2", "e2"),
        ];
        let out = sdiff(&ops, |c| {
            c.ignore_matching = vec![ere::bre::compile(b"x", false).unwrap()];
        });
        assert_eq!(out, b"e1\te1\nx1\tx2\ne2\tx3\n      )\te2\n");
    }

    #[test]
    fn side_by_side_column_options() {
        let ops = [eq("e", "e"), del("a"), ins("b")];
        assert_eq!(
            sdiff(&ops, |c| c.left_column = true),
            b"e     (\na     |\tb\n"
        );
        assert_eq!(
            sdiff(&ops, |c| c.suppress_common_lines = true),
            b"a     |\tb\n"
        );
    }

    #[test]
    fn width_and_tabsize_values_are_read_as_upstream_reads_them() {
        assert_eq!(size_value(b"40", usize::MAX), Some(40));
        assert_eq!(size_value(b" +40", usize::MAX), Some(40));
        assert_eq!(size_value(b"40x", usize::MAX), None);
        assert_eq!(size_value(b"0", usize::MAX), None);
        assert_eq!(size_value(b"-1", usize::MAX), None);
        assert_eq!(size_value(b"", usize::MAX), None);
        // `strtoimax` saturates, and upstream takes the saturated value.
        assert_eq!(
            size_value(b"99999999999999999999", usize::MAX),
            usize::try_from(i64::MAX).ok()
        );
        assert_eq!(size_value(b"5", 4), None);
    }

    #[test]
    fn side_by_side_options_parse() {
        let c = run(&[
            "diff",
            "-y",
            "--suppress-common-lines",
            "--left-column",
            "a",
            "b",
        ]);
        assert!(c.suppress_common_lines && c.left_column);
        assert_eq!(run(&["diff", "--tabsize=4", "a", "b"]).tabsize, 4);
        assert_eq!(run(&["diff", "--tabsize", "3", "a", "b"]).tabsize, 3);
        assert_eq!(run(&["diff", "-W", "40", "-W40", "a", "b"]).width, 40);
    }

    // ---------------- compute_diff: the edit-script cases ----------------

    /// The edit script, decoded back to `String` for the assertions.
    ///
    /// Decoding is right HERE and nowhere else: every fixture below is ASCII,
    /// and keeping the return type means all of these cases are the ones that
    /// were here before the conversion rather than restatements of it. The
    /// cases that exercise bytes call `compute_diff` directly.
    fn ops(a: &[&str], b: &[&str]) -> Vec<(Op, String)> {
        compute_diff(&s(a), &s(b), NL, NL, &cfg())
            .into_iter()
            .map(|e| (e.op, String::from_utf8(e.text).expect("ASCII fixture")))
            .collect()
    }

    #[test]
    fn edits_identical_are_all_equal() {
        let got = ops(&["x", "y"], &["x", "y"]);
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|(op, _)| *op == Op::Equal));
    }

    #[test]
    fn edits_pure_insert() {
        let got = ops(&["x"], &["x", "y"]);
        assert_eq!(
            got,
            vec![(Op::Equal, "x".to_string()), (Op::Insert, "y".to_string())]
        );
    }

    #[test]
    fn edits_pure_delete() {
        let got = ops(&["x", "y"], &["x"]);
        assert_eq!(
            got,
            vec![(Op::Equal, "x".to_string()), (Op::Delete, "y".to_string())]
        );
    }

    #[test]
    fn edits_replacement_is_a_delete_and_an_insert() {
        let got = ops(&["a"], &["b"]);
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|(op, t)| *op == Op::Delete && t == "a"));
        assert!(got.iter().any(|(op, t)| *op == Op::Insert && t == "b"));
    }

    #[test]
    fn edits_keep_the_middle_when_only_the_ends_move() {
        let got = ops(&["a", "keep", "b"], &["A", "keep", "B"]);
        let kept: Vec<_> = got.iter().filter(|(op, _)| *op == Op::Equal).collect();
        assert_eq!(kept.len(), 1, "only `keep` is common");
        assert_eq!(kept[0].1, "keep");
    }

    #[test]
    fn edits_against_an_empty_side() {
        assert!(ops(&[], &[]).is_empty());
        assert!(ops(&["a"], &[]).iter().all(|(op, _)| *op == Op::Delete));
        assert!(ops(&[], &["a"]).iter().all(|(op, _)| *op == Op::Insert));
    }
}
