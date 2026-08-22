//! csplit — output pieces of a file separated by patterns.
//!
//! Certified against glibc's `csplit` (GNU coreutils 9.4) through WSL;
//! `scripts/csplit-diff.sh` is the executable form of every claim below.
//!
//! # Why this file was rewritten rather than patched
//!
//! The version this replaces had four faults, and each one of them was the
//! kind that a test asserting the program's own behaviour will happily lock in:
//!
//! * **`{*}` wrote ten thousand files.** It implemented "repeat until the end
//!   of input" by cloning the pattern into the list 10000 times. A repeated
//!   line-number pattern was re-applied *absolutely* — `4 {*}` split at line 4,
//!   then at line 4 again, which is already behind the cursor and so produced
//!   an empty section — so the cursor never advanced and all ten thousand
//!   copies fired. `csplit f 3 '{*}'` left `xx00`…`xx10002` in the directory.
//!   That is also what made the differential harness appear to hang: comparing
//!   the two runs meant `od`-ing twenty thousand files.
//! * **`%REGEX%` discarded the matching line.** The module's own documentation
//!   said "skip to (but don't include) the matching line" and the code said
//!   `current_pos = match_line + 1`.
//! * **Every output line was re-terminated with `\n`.** Input was read through
//!   `BufRead::lines`, which throws the terminator away; a file whose last line
//!   has no newline came back out with one, so the byte counts on stdout were
//!   off by one and the last output file did not match the input.
//! * **Regular expressions were `str::contains`.** `/^5$/` and `/5/` are
//!   different patterns and it treated them as the same one — the same fault
//!   [`ere`] was extracted to fix in `grep`, `sed`, `awk` and `expr`
//!   (design-decisions.md §322).
//!
//! # The model this implements
//!
//! Two cursors, which are *not* the same cursor — this is the single fact the
//! rest of the file follows from:
//!
//! | cursor | meaning | moved by |
//! |---|---|---|
//! | `emit` | first line not yet written to an output file | every pattern |
//! | `search` | first line the next regex will examine | a regex match sets it to *one past the match*; a line number sets it to `emit` |
//!
//! They differ after a regex with an offset, and after a plain regex too:
//!
//! ```text
//! $ printf 'a\nMARK\nb\nc\nMARK\nd\n' | csplit - /MARK/ '{*}'
//! 2   →  xx00 = "a\n"
//! 9   →  xx01 = "MARK\nb\nc\n"
//! 7   →  xx02 = "MARK\nd\n"
//! ```
//!
//! The split happens *before* the matched line, so that line is still unwritten
//! when the next pattern runs. If the next search restarted at `emit` it would
//! match the very same `MARK` again and split into an empty section forever.
//! GNU's loop reads `find_line (++current_line)`, so the line it just matched is
//! behind it; `search = m + 1` is that, and `%MARK% /MARK/` is the case that
//! shows the two cursors apart — the skip leaves `emit` *on* the first `MARK`
//! while `search` is already past it, and the following `/MARK/` finds the
//! second one.
//!
//! # `{*}` is an error on a line number and not on a regex
//!
//! ```text
//! $ seq 20 | csplit - 3 '{*}'      $ seq 20 | csplit - /5/ '{*}'
//! csplit: ‘3’: line number out          8
//!         of range on repetition 6      25
//! …counts…                              18
//! $ echo $?                         $ echo $?
//! 1                                 0
//! ```
//!
//! Both mean "repeat as many times as possible", and they disagree about what
//! happens when that runs out. It is not an inconsistency to tidy up: GNU's
//! `process_regexp` treats `repeat_forever` as the terminating condition and
//! exits 0, while `process_line_count` has no such branch and falls into
//! `handle_line_error`. Scripts depend on the regex form stopping quietly.
//!
//! A repeated line number advances by its own literal value, not by the
//! distance from the pattern before it: GNU computes `lines_required *
//! (repetition + 1)`, so `2 4 {1}` splits at 2, 4 and **8** — not 6.
//!
//! # Which errors leave files behind
//!
//! Three distinct exits, all measured, and the difference is visible in the
//! directory afterwards:
//!
//! | | stdout | files left |
//! |---|---|---|
//! | `csplit f 99` | the count of the piece it did write | none — removed |
//! | `csplit -k f 99` | same | all of them |
//! | `csplit empty 1` | nothing | `xx00`, empty |
//!
//! The last one is not a special case anybody designed. GNU's
//! `process_line_count` calls `create_output_file` and *then*
//! `get_first_line_in_buffer`, which fails with `input disappeared` through
//! `error (EXIT_FAILURE, …)` — a direct exit that runs neither the close (so no
//! count is printed) nor the cleanup (so the file stays). Reproduced because a
//! caller cannot tell a designed behaviour from an emergent one, and both are
//! what the program does.
//!
//! Note also that the counts are printed even when the files are then deleted:
//! `csplit f 99` prints `51` and leaves nothing, because the cleanup path
//! closes the output file — which is what prints — before unlinking it.
//!
//! # A line number one past the end fails *after* writing everything
//!
//! ```text
//! $ seq 20 | csplit - 21
//! 51
//! csplit: ‘21’: line number out of range
//! $ echo $?
//! 1
//! ```
//!
//! Every line was written, counted, and then deleted. GNU's own comment on the
//! check is "ensure that the line number specified is not 1 greater than the
//! number of lines in the file", and it sits *after* `close_output_file`, so a
//! range check performed before writing would get the exit status right and the
//! byte counts wrong. The same check is what ends `3 {*}` on a 20-line file at
//! `on repetition 6` — the repetition whose piece was written in full — rather
//! than at 7 with an empty piece.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, quote, quote_os, quoteaf_os, quotef_os};
use ere::{Regex, bre};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;

/// Measured: `csplit --zzz-bogus; echo $?` is 1.
const CSPLIT: Program = Program::new("csplit", 1);

const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("suffix-format", Takes::Required),
    ("prefix", Takes::Required),
    ("keep-files", Takes::Nothing),
    ("suppress-matched", Takes::Nothing),
    ("digits", Takes::Required),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("elide-empty-files", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

/// What a parsed command line asks for.
///
/// `Run` carries the input file and the patterns as separate fields rather
/// than one operand list, because a run without a file — or with a file and no
/// patterns — is not a state this program can be in: `parse_args` rejects both
/// with a usage error. Keeping them apart means the split never has to be
/// re-derived later behind an `unwrap` that could not fire.
enum Request {
    Run(Options, OsString, Vec<OsString>),
    Help,
    Version,
}

/// Everything the options set, after the whole command line has been seen.
struct Options {
    prefix: OsString,
    digits: usize,
    /// `-b`. When present it replaces the `%0{digits}d` suffix entirely; the
    /// prefix is still prepended, so `-b 'q%03dz'` names files `xxq000z`.
    suffix: Option<Vec<u8>>,
    keep_files: bool,
    quiet: bool,
    elide_empty: bool,
    suppress_matched: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            prefix: OsString::from("xx"),
            digits: 2,
            suffix: None,
            keep_files: false,
            quiet: false,
            elide_empty: false,
            suppress_matched: false,
        }
    }
}

fn arg_bytes(a: &OsStr) -> Vec<u8> {
    os_bytes(a).into_owned()
}

/// An `OsString` from bytes.
///
/// The mirror of [`coreutils::quote::os_bytes`], and lossy on a Windows host
/// for the same reason: there is no byte view of an `OsStr` there that
/// round-trips. Only the developing-and-testing host is affected — on the
/// target a path is bytes.
#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b.to_vec())
}

#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("csplit: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    match request {
        Request::Help => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Request::Version => {
            println!("csplit (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Request::Run(options, file, patterns) => run(&options, &file, &patterns),
    }
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs that names
/// the GNU project's own bug addresses.
fn help_text() -> String {
    "\
Usage: csplit [OPTION]... FILE PATTERN...
Output pieces of FILE separated by PATTERN(s) to files 'xx00', 'xx01', ...,
and output byte counts of each piece to standard output.

Read standard input if FILE is -

Mandatory arguments to long options are mandatory for short options too.
  -b, --suffix-format=FORMAT  use sprintf FORMAT instead of %02d
  -f, --prefix=PREFIX        use PREFIX instead of 'xx'
  -k, --keep-files           do not remove output files on errors
      --suppress-matched     suppress the lines matching PATTERN
  -n, --digits=DIGITS        use specified number of digits instead of 2
  -s, --quiet, --silent      do not print counts of output file sizes
  -z, --elide-empty-files    suppress empty output files
      --help        display this help and exit
      --version     output version information and exit

Each PATTERN may be:
  INTEGER            copy up to but not including specified line number
  /REGEXP/[OFFSET]   copy up to but not including a matching line
  %REGEXP%[OFFSET]   skip to, but not including a matching line
  {INTEGER}          repeat the previous pattern specified number of times
  {*}                repeat the previous pattern as many times as possible

A line OFFSET is an integer optionally preceded by '+' or '-'
"
    .to_string()
}

/// Parse the whole command line.
///
/// Options may follow operands — `csplit f 4 -z` works — because glibc's
/// `getopt_long` permutes argv unless the caller asks it not to, and `csplit`
/// does not ask. That matters here more than for most utilities, because
/// `csplit`'s operands are patterns and a script that appends `--suppress-matched`
/// after them is idiomatic.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut operands: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        if only_operands {
            operands.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand. So is every
            // pattern: `/RE/`, `%RE%`, `{N}` and a bare integer all fail the
            // leading-`-` test, and a negative line number is not a pattern.
            operands.push(arg.clone());
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut options)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut options)?;
        }
    }

    let mut rest = operands.into_iter();
    let Some(file) = rest.next() else {
        return Err(CSPLIT.usage_referring("missing operand".to_string()));
    };
    let patterns: Vec<OsString> = rest.collect();
    if patterns.is_empty() {
        let name = quote_os(file.as_os_str());
        return Err(CSPLIT.usage_referring(format!("missing operand after {name}")));
    }
    Ok(Request::Run(options, file, patterns))
}

/// One `--name` or `--name=value`, returning a [`Request`] for the two options
/// that end parsing and `None` when it only set something.
fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
) -> Result<Option<Request>, getopt::Error> {
    let body = bytes.get(2..).unwrap_or_default();
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    let typed = std::str::from_utf8(typed).map_err(|_| CSPLIT.unrecognized_option(bytes))?;
    let (name, takes) = CSPLIT.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(CSPLIT.long_unwanted_argument(name));
    }
    let value: Option<OsString> = match (takes, inline) {
        (_, Some(v)) => Some(os_from_bytes(v)),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| CSPLIT.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(next)
        }
        (_, None) => None,
    };
    let value = value.unwrap_or_default();

    match name {
        "suffix-format" => options.suffix = Some(arg_bytes(&value)),
        "prefix" => options.prefix = value,
        "keep-files" => options.keep_files = true,
        "suppress-matched" => options.suppress_matched = true,
        "digits" => options.digits = parse_digits(&arg_bytes(&value))?,
        "quiet" | "silent" => options.quiet = true,
        "elide-empty-files" => options.elide_empty = true,
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// One `-abc` cluster.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
) -> Result<(), getopt::Error> {
    let body = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        match c {
            b'k' => options.keep_files = true,
            b's' | b'q' => options.quiet = true,
            b'z' => options.elide_empty = true,
            b'b' | b'f' | b'n' => {
                let value: OsString = match body.get(at..) {
                    Some(rest) if !rest.is_empty() => {
                        at = body.len();
                        os_from_bytes(rest)
                    }
                    _ => {
                        let next = args
                            .get(*i)
                            .ok_or_else(|| CSPLIT.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        next
                    }
                };
                match c {
                    b'b' => options.suffix = Some(arg_bytes(&value)),
                    b'f' => options.prefix = value,
                    _ => options.digits = parse_digits(&arg_bytes(&value))?,
                }
            }
            _ => return Err(CSPLIT.invalid_option(c)),
        }
    }
    Ok(())
}

/// `-n`'s argument. Measured: `csplit f -n abc 4` says `invalid number: ‘abc’`
/// with no `Try '… --help'` referral.
fn parse_digits(value: &[u8]) -> Result<usize, getopt::Error> {
    let text = std::str::from_utf8(value).ok();
    let digits = text.and_then(|t| t.parse::<usize>().ok());
    digits.ok_or_else(|| CSPLIT.usage(format!("invalid number: {}", quote(value))))
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// What one pattern does when it is reached.
enum Kind {
    /// `/RE/OFFSET` — end the current piece before the matching line.
    Split { re: Regex, offset: i64 },
    /// `%RE%OFFSET` — discard up to, but not including, the matching line.
    Skip { re: Regex, offset: i64 },
    /// `N` — end the current piece before line `N`.
    Line(u64),
}

/// How many extra times a pattern applies after its first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Times(u64),
    Forever,
}

struct Control {
    kind: Kind,
    /// The argument as typed, quoted back in this pattern's diagnostics. GNU
    /// echoes the original text — `‘/5/+100’: line number out of range` — not
    /// the parsed pieces, so it has to be kept.
    arg: Vec<u8>,
    repeat: Repeat,
}

/// A fatal diagnostic, and what to do about the files already written.
#[derive(Debug)]
struct Fail {
    message: String,
    /// Close (and so count) the output file that is open. False only for
    /// `input disappeared`, which GNU reaches through a direct
    /// `error (EXIT_FAILURE, …)` that runs no cleanup at all.
    close: bool,
    /// Remove the outputs. Suppressed by `-k`, and by the same direct exit.
    remove: bool,
}

impl Fail {
    fn fatal(message: String) -> Self {
        Fail {
            message,
            close: true,
            remove: true,
        }
    }
    fn bare(message: String) -> Self {
        Fail {
            message,
            close: false,
            remove: false,
        }
    }
}

/// Parse the pattern operands.
///
/// GNU parses `{…}` as a *suffix of the pattern before it*, not as a pattern in
/// its own right — the loop looks ahead one argument. That is why a leading
/// `{3}` is reported as `'{3}': invalid pattern` rather than as a repeat count
/// with nothing to repeat: it reached the integer branch and `xstrtoumax`
/// refused it.
fn parse_patterns(args: &[OsString]) -> Result<Vec<Control>, Fail> {
    let mut controls: Vec<Control> = Vec::new();
    let mut last_line: u64 = 0;
    let mut i = 0usize;

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        let bytes = arg_bytes(arg);
        let control = match bytes.first() {
            Some(&b'/') => extract_regexp(&bytes, false)?,
            Some(&b'%') => extract_regexp(&bytes, true)?,
            _ => line_control(&bytes, &mut last_line)?,
        };
        controls.push(control);

        // A `{…}` immediately after attaches to what was just pushed.
        if let Some(next) = args.get(i) {
            let next_bytes = arg_bytes(next);
            if next_bytes.first() == Some(&b'{') {
                i = i.saturating_add(1);
                let repeat = parse_repeat_count(&next_bytes)?;
                if let Some(last) = controls.last_mut() {
                    last.repeat = repeat;
                }
            }
        }
    }
    Ok(controls)
}

/// A bare integer pattern, with GNU's two ordering checks.
fn line_control(bytes: &[u8], last_line: &mut u64) -> Result<Control, Fail> {
    let text = std::str::from_utf8(bytes).ok();
    // GNU's `xstrtoumax (…, "")`: the whole argument must be the number, so a
    // leading `+`, a trailing space or `4x` are all "invalid pattern" rather
    // than a partial parse.
    let value = text
        .filter(|t| t.bytes().all(|b| b.is_ascii_digit()))
        .filter(|t| !t.is_empty())
        .and_then(|t| t.parse::<u64>().ok())
        .ok_or_else(|| Fail::fatal(format!("{}: invalid pattern", quote(bytes))))?;

    if value == 0 {
        // Measured: this one is *not* quoted, where "invalid pattern" is.
        return Err(Fail::fatal(format!(
            "{}: line number must be greater than zero",
            String::from_utf8_lossy(bytes)
        )));
    }
    if value < *last_line {
        // `quote()`, so the marks follow §351 and are curly. Measured against
        // GNU csplit 9.4 under `LC_ALL=C.UTF-8`, which prints
        // `line number ‘2’ is smaller than preceding line number, 4`. This read
        // `'{}'` until the harness moved off its `C` reference and caught it —
        // straight marks agree with GNU only in the locale we no longer use.
        return Err(Fail::fatal(format!(
            "line number {} is smaller than preceding line number, {}",
            quote(bytes),
            *last_line
        )));
    }
    if value == *last_line {
        // A warning, not an error: GNU carries on and writes the empty piece.
        // Curly for the same measured reason as the error just above.
        eprintln!(
            "csplit: warning: line number {} is the same as preceding line number",
            quote(bytes)
        );
    }
    *last_line = value;
    Ok(Control {
        kind: Kind::Line(value),
        arg: bytes.to_vec(),
        repeat: Repeat::Times(0),
    })
}

/// `/RE/OFFSET` or `%RE%OFFSET`.
///
/// The closing delimiter is the *last* one in the argument, not the first —
/// GNU uses `strrchr` — so `/a/b/` is the pattern `a/b` and not `a` followed by
/// the garbage `b/`.
fn extract_regexp(bytes: &[u8], skip: bool) -> Result<Control, Fail> {
    let delim = *bytes.first().unwrap_or(&b'/');
    let rest = bytes.get(1..).unwrap_or_default();
    let close = rest.iter().rposition(|&c| c == delim).ok_or_else(|| {
        Fail::fatal(format!(
            "{}: closing delimiter '{}' missing",
            String::from_utf8_lossy(bytes),
            delim as char
        ))
    })?;
    let pattern = rest.get(..close).unwrap_or_default();
    let tail = rest.get(close.saturating_add(1)..).unwrap_or_default();

    let offset = if tail.is_empty() {
        0i64
    } else {
        parse_offset(tail).ok_or_else(|| {
            Fail::fatal(format!(
                "{}: integer expected after delimiter",
                quote(bytes)
            ))
        })?
    };

    // Basic regular expressions: `csplit` is specified in terms of `ed`'s, the
    // same dialect `grep` and `sed` use without `-E`.
    let re = bre::compile(pattern, false).map_err(|e| {
        Fail::fatal(format!(
            "{}: {}",
            quote(pattern),
            String::from_utf8_lossy(&e.0)
        ))
    })?;

    Ok(Control {
        kind: if skip {
            Kind::Skip { re, offset }
        } else {
            Kind::Split { re, offset }
        },
        arg: bytes.to_vec(),
        repeat: Repeat::Times(0),
    })
}

/// An offset after the closing delimiter: digits, optionally signed.
fn parse_offset(tail: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(tail).ok()?;
    let digits = text.strip_prefix('+').or_else(|| text.strip_prefix('-'));
    let digits = digits.unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.strip_prefix('+').unwrap_or(text).parse::<i64>().ok()
}

/// `{N}` or `{*}`.
///
/// Both diagnostics reproduce GNU's odd quoting: it NUL-terminates the argument
/// at the `}` before quoting it and then prints a `}` of its own, so `{x}` comes
/// back as `'{x'}`.
fn parse_repeat_count(bytes: &[u8]) -> Result<Repeat, Fail> {
    if bytes.last() != Some(&b'}') {
        return Err(Fail::fatal(format!(
            "{}: '}}' is required in repeat count",
            quote(bytes)
        )));
    }
    let inner = bytes
        .get(1..bytes.len().saturating_sub(1))
        .unwrap_or_default();
    if inner == b"*" {
        return Ok(Repeat::Forever);
    }
    let text = std::str::from_utf8(inner).ok();
    let value = text
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse::<u64>().ok())
        .ok_or_else(|| {
            let truncated = bytes
                .get(..bytes.len().saturating_sub(1))
                .unwrap_or_default();
            Fail::fatal(format!(
                "{}}}: integer required between '{{' and '}}'",
                quote(truncated)
            ))
        })?;
    Ok(Repeat::Times(value))
}

// ---------------------------------------------------------------------------
// Output files
// ---------------------------------------------------------------------------

/// Builds the output file names and owns the one that is open.
struct Sink {
    prefix: OsString,
    digits: usize,
    format: Option<SuffixFormat>,
    quiet: bool,
    elide_empty: bool,
    /// The next number a file will be given. Not incremented for a file that
    /// `-z` elided, which is why `csplit -z f 1` leaves a single `xx00` holding
    /// everything rather than an `xx01`.
    index: usize,
    /// Names that have been closed and counted, in creation order, for the
    /// cleanup an error triggers.
    written: Vec<OsString>,
    open: Option<(OsString, File, u64)>,
}

impl Sink {
    fn name(&self, index: usize) -> OsString {
        let suffix = match &self.format {
            Some(f) => f.render(index),
            None => format!("{index:0width$}", width = self.digits).into_bytes(),
        };
        let mut name = self.prefix.clone();
        name.push(os_from_bytes(&suffix));
        name
    }

    fn create(&mut self) -> Result<(), Fail> {
        let name = self.name(self.index);
        // GNU names the output file bare and lets the errno finish the
        // sentence — `csplit: ro/x00: Permission denied` — rather than using
        // the "cannot open ... for writing" phrasing it uses for the *input*.
        // Measured, not assumed; the two openings really are worded
        // differently.
        let file = File::create(&name)
            .map_err(|e| Fail::fatal(format!("{}: {}", quotef_os(&name), strerror(&e))))?;
        self.open = Some((name, file, 0));
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), Fail> {
        let Some((name, file, bytes)) = self.open.as_mut() else {
            return Ok(());
        };
        file.write_all(chunk).map_err(|e| {
            Fail::fatal(format!(
                "write error for {}: {}",
                quoteaf_os(&*name),
                strerror(&e)
            ))
        })?;
        *bytes = bytes.saturating_add(chunk.len() as u64);
        Ok(())
    }

    /// Close the open file, print its size, and take its number.
    ///
    /// Under `-z` an empty file is removed instead, and — the part that is
    /// observable rather than tidy — its number is *not* consumed, so the next
    /// piece gets it.
    fn close(&mut self) {
        let Some((name, file, bytes)) = self.open.take() else {
            return;
        };
        drop(file);
        if bytes == 0 && self.elide_empty {
            let _ = std::fs::remove_file(&name);
            // The file never existed as far as numbering is concerned.
            return;
        }
        if !self.quiet {
            println!("{bytes}");
        }
        self.written.push(name);
        self.index = self.index.saturating_add(1);
    }

    fn remove_all(&mut self) {
        for name in self.written.drain(..) {
            // Best effort: GNU's `remove_files` reports a failure to unlink but
            // is already on its way out with a status of 1, which is what we
            // return regardless.
            let _ = std::fs::remove_file(&name);
        }
    }
}

// ---------------------------------------------------------------------------
// -b: one printf integer conversion
// ---------------------------------------------------------------------------

/// A validated `-b` format: literal text with exactly one integer conversion.
struct SuffixFormat {
    head: Vec<u8>,
    tail: Vec<u8>,
    flag_minus: bool,
    flag_zero: bool,
    flag_hash: bool,
    width: usize,
    precision: Option<usize>,
    conv: u8,
}

impl SuffixFormat {
    /// GNU validates the format once, up front, and refuses two conversions or
    /// none — `sprintf`ing an unchecked user format with one integer argument
    /// is the classic way to read the stack.
    fn parse(format: &[u8]) -> Result<SuffixFormat, Fail> {
        let mut head: Vec<u8> = Vec::new();
        let mut tail: Vec<u8> = Vec::new();
        let mut spec: Option<SuffixFormat> = None;
        let mut i = 0usize;

        while let Some(&c) = format.get(i) {
            i = i.saturating_add(1);
            if c != b'%' {
                if spec.is_some() {
                    tail.push(c)
                } else {
                    head.push(c)
                }
                continue;
            }
            if format.get(i) == Some(&b'%') {
                i = i.saturating_add(1);
                if spec.is_some() {
                    tail.push(b'%')
                } else {
                    head.push(b'%')
                }
                continue;
            }
            if spec.is_some() {
                return Err(Fail::fatal(
                    "too many % conversion specifications in suffix".to_string(),
                ));
            }
            let (parsed, next) = SuffixFormat::parse_spec(format, i)?;
            spec = Some(parsed);
            i = next;
        }

        let mut spec = spec.ok_or_else(|| {
            Fail::fatal("missing % conversion specification in suffix".to_string())
        })?;
        spec.head = head;
        spec.tail = tail;
        Ok(spec)
    }

    /// Parse `[flags][width][.precision]conv` starting just after the `%`.
    fn parse_spec(format: &[u8], start: usize) -> Result<(SuffixFormat, usize), Fail> {
        let mut i = start;
        let mut spec = SuffixFormat {
            head: Vec::new(),
            tail: Vec::new(),
            flag_minus: false,
            flag_zero: false,
            flag_hash: false,
            width: 0,
            precision: None,
            conv: b'd',
        };
        // The flag set is `-`, `0`, `#` and `'` — deliberately *not* printf's
        // full set. csplit's suffix names a file, so a leading `+` or space
        // would be a character in a filename rather than a sign column, and
        // GNU does not take them: measured, `csplit -b '%+d'` and `-b '% d'`
        // both fail with `invalid conversion specifier in suffix: +` / `: `,
        // which is GNU's parser stopping at the flag and reading it as the
        // conversion. We accepted both and produced files called `+0`, `+1` —
        // valid-looking output for a command line GNU rejects, which is the
        // worst shape a divergence can take. See `known-issues.md`
        // → BUG-CSPLIT-ACCEPTS-TWO-SUFFIX-FLAGS-GNU-REJECTS.
        while let Some(&c) = format.get(i) {
            match c {
                b'-' => spec.flag_minus = true,
                b'0' => spec.flag_zero = true,
                b'#' => spec.flag_hash = true,
                // Accepted and ignored, as glibc's thousands-grouping flag is
                // in the C locale.
                b'\'' => {}
                _ => break,
            }
            i = i.saturating_add(1);
        }
        while let Some(&c) = format.get(i) {
            if !c.is_ascii_digit() {
                break;
            }
            spec.width = spec
                .width
                .saturating_mul(10)
                .saturating_add(usize::from(c - b'0'));
            i = i.saturating_add(1);
        }
        if format.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            let mut precision = 0usize;
            while let Some(&c) = format.get(i) {
                if !c.is_ascii_digit() {
                    break;
                }
                precision = precision
                    .saturating_mul(10)
                    .saturating_add(usize::from(c - b'0'));
                i = i.saturating_add(1);
            }
            spec.precision = Some(precision);
        }
        let Some(&conv) = format.get(i) else {
            return Err(Fail::fatal(
                "missing conversion specifier in suffix".to_string(),
            ));
        };
        i = i.saturating_add(1);
        if !matches!(conv, b'd' | b'i' | b'o' | b'u' | b'x' | b'X') {
            let shown = if conv.is_ascii_graphic() || conv == b' ' {
                format!("{}", conv as char)
            } else {
                format!("\\{conv:03o}")
            };
            return Err(Fail::fatal(format!(
                "invalid conversion specifier in suffix: {shown}"
            )));
        }
        spec.conv = conv;
        Ok((spec, i))
    }

    fn render(&self, value: usize) -> Vec<u8> {
        let digits = match self.conv {
            b'o' => format!("{value:o}"),
            b'x' => format!("{value:x}"),
            b'X' => format!("{value:X}"),
            _ => format!("{value}"),
        };
        // No sign column: `+` and space are not flags here (see `parse_spec`),
        // and the values are section indices, which are never negative.
        let mut body = String::new();
        if self.flag_hash {
            match self.conv {
                b'x' if value != 0 => body.push_str("0x"),
                b'X' if value != 0 => body.push_str("0X"),
                b'o' if !digits.starts_with('0') => body.push('0'),
                _ => {}
            }
        }
        // A precision is a minimum digit count and, per C, turns `0` off.
        let pad_to = self.precision.unwrap_or(0);
        for _ in digits.len()..pad_to {
            body.push('0');
        }
        body.push_str(&digits);

        let mut out = self.head.clone();
        if body.len() >= self.width {
            out.extend_from_slice(body.as_bytes());
        } else if self.flag_minus {
            out.extend_from_slice(body.as_bytes());
            out.resize(
                out.len()
                    .saturating_add(self.width.saturating_sub(body.len())),
                b' ',
            );
        } else if self.flag_zero && self.precision.is_none() {
            // Zero padding goes after a `#` prefix (`0x`/`0X`/`0`), not before
            // it: `%#08x` of 255 is `0x0000ff`, not `000000xff`.
            let split = body.len().saturating_sub(digits.len());
            out.extend_from_slice(body.get(..split).unwrap_or_default().as_bytes());
            out.resize(
                out.len()
                    .saturating_add(self.width.saturating_sub(body.len())),
                b'0',
            );
            out.extend_from_slice(body.get(split..).unwrap_or_default().as_bytes());
        } else {
            out.resize(
                out.len()
                    .saturating_add(self.width.saturating_sub(body.len())),
                b' ',
            );
            out.extend_from_slice(body.as_bytes());
        }
        out.extend_from_slice(&self.tail);
        out
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

fn run(options: &Options, file: &OsString, patterns: &[OsString]) -> ExitCode {
    let format = match &options.suffix {
        Some(f) => match SuffixFormat::parse(f) {
            Ok(parsed) => Some(parsed),
            Err(e) => {
                eprintln!("csplit: {}", e.message);
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    let controls = match parse_patterns(patterns) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("csplit: {}", e.message);
            return ExitCode::FAILURE;
        }
    };

    let data = match read_input(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("csplit: {e}");
            return ExitCode::FAILURE;
        }
    };
    let lines = split_lines(&data);

    let mut sink = Sink {
        prefix: options.prefix.clone(),
        digits: options.digits,
        format,
        quiet: options.quiet,
        elide_empty: options.elide_empty,
        index: 0,
        written: Vec::new(),
        open: None,
    };

    match split(options, &lines, &controls, &mut sink) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("csplit: {}", e.message);
            if e.close {
                sink.close();
            }
            if e.remove && !options.keep_files {
                sink.remove_all();
            }
            ExitCode::FAILURE
        }
    }
}

fn read_input(file: &OsString) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    if file == OsStr::new("-") {
        io::stdin()
            .read_to_end(&mut data)
            .map_err(|e| format!("read error: {}", strerror(&e)))?;
        return Ok(data);
    }
    let mut handle = File::open(file).map_err(|e| {
        format!(
            "cannot open {} for reading: {}",
            quoteaf_os(file),
            strerror(&e)
        )
    })?;
    // GNU's read failure names no file at all — `csplit: read error: Is a
    // directory` — because it reads through the same buffer for stdin and for
    // a named file, and that buffer does not know where it came from.
    handle
        .read_to_end(&mut data)
        .map_err(|e| format!("read error: {}", strerror(&e)))?;
    Ok(data)
}

/// Split into lines that each still carry their terminator.
///
/// Keeping the `\n` on is what makes a file whose last line has none come back
/// out unchanged, and it is why the byte counts are just slice lengths.
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            let end = i.saturating_add(1);
            lines.push(data.get(start..end).unwrap_or_default());
            start = end;
        }
    }
    if start < data.len() {
        lines.push(data.get(start..).unwrap_or_default());
    }
    lines
}

/// Does this line match, ignoring the terminator?
///
/// `$` has to anchor at the end of the *text*, so the `\n` cannot be part of
/// what the engine sees — otherwise `/^5$/` would never match anything.
fn line_matches(re: &Regex, line: &[u8], arg: &[u8]) -> Result<bool, Fail> {
    let text = line.strip_suffix(b"\n").unwrap_or(line);
    re.is_match(text).map_err(|_| {
        Fail::fatal(format!(
            "{}: regular expression is too complex to match",
            quote(arg)
        ))
    })
}

/// The whole split. Writes every output file, including the trailing piece.
fn split(
    options: &Options,
    lines: &[&[u8]],
    controls: &[Control],
    sink: &mut Sink,
) -> Result<(), Fail> {
    // `emit` is the first line not yet written; `search` is the first line a
    // regex will look at. See the module doc: they are not the same cursor.
    let mut emit = 0usize;
    let mut search = 0usize;

    for control in controls {
        let limit = match control.repeat {
            Repeat::Times(n) => n,
            Repeat::Forever => u64::MAX,
        };
        let mut repetition: u64 = 0;
        loop {
            let step = apply(
                options,
                control,
                repetition,
                lines,
                sink,
                &mut emit,
                &mut search,
            )?;
            if step == Step::Finished {
                // `{*}` ran out. GNU exits 0 from inside the loop, which skips
                // the trailing piece — for a skip pattern that means no output
                // file is produced at all.
                return Ok(());
            }
            if repetition >= limit {
                break;
            }
            repetition = repetition.saturating_add(1);
        }
    }

    sink.create()?;
    let rest = lines.get(emit..).unwrap_or_default();
    for line in rest {
        sink.write(line)?;
    }
    sink.close();
    Ok(())
}

#[derive(PartialEq, Eq)]
enum Step {
    /// The pattern applied; carry on.
    Applied,
    /// A `{*}` ran out of input. Everything is done.
    Finished,
}

/// Apply one control once.
fn apply(
    options: &Options,
    control: &Control,
    repetition: u64,
    lines: &[&[u8]],
    sink: &mut Sink,
    emit: &mut usize,
    search: &mut usize,
) -> Result<Step, Fail> {
    let forever = control.repeat == Repeat::Forever;
    match &control.kind {
        Kind::Line(n) => {
            sink.create()?;
            if *emit >= lines.len() {
                // GNU's `get_first_line_in_buffer`, which runs *after* the
                // output file has been created and exits without cleanup.
                return Err(Fail::bare("input disappeared".to_string()));
            }
            // A repeated line number is an absolute multiple, not a delta from
            // where the previous pattern stopped: `2 4 {1}` splits at 2, 4 and
            // 8, never at 6.
            let target = n
                .checked_mul(repetition.saturating_add(1))
                .and_then(|t| usize::try_from(t).ok())
                .unwrap_or(usize::MAX);
            // Clamp rather than reject: GNU writes out whatever is left before
            // it complains, so `csplit f 99` still prints the count of the
            // truncated piece. Whether it noticed inside its copy loop (99) or
            // in the `no_more_lines ()` check just after it (21) is invisible
            // from outside, because both funnel through `cleanup_fatal`, which
            // closes — and so counts — the open file first.
            let boundary = target.saturating_sub(1).clamp(*emit, lines.len());
            write_range(sink, lines, *emit, boundary)?;
            sink.close();
            *emit = advance(options, boundary, lines.len());
            *search = *emit;
            // GNU's own comment on this check: "Ensure that the line number
            // specified is not 1 greater than the number of lines in the
            // file." Splitting a 20-line file at 21 is an error even though
            // every line was successfully written first — the section that the
            // split was supposed to *begin* has no lines to begin with.
            if *emit >= lines.len() {
                return Err(Fail::fatal(format!(
                    "{}: line number out of range{}",
                    quote(&control.arg),
                    on_repetition(repetition)
                )));
            }
            Ok(Step::Applied)
        }
        Kind::Split { re, offset } => {
            sink.create()?;
            let found = find(re, lines, *search, &control.arg)?;
            let Some(m) = found else {
                write_range(sink, lines, *emit, lines.len())?;
                if forever {
                    sink.close();
                    return Ok(Step::Finished);
                }
                return Err(Fail::fatal(format!(
                    "{}: match not found{}",
                    quote(&control.arg),
                    on_repetition(repetition)
                )));
            };
            let boundary = boundary_of(m, *offset, lines.len()).ok_or_else(|| {
                Fail::fatal(format!(
                    "{}: line number out of range{}",
                    quote(&control.arg),
                    on_repetition(repetition)
                ))
            });
            let boundary = match boundary {
                Ok(b) => b.max(*emit),
                Err(e) => {
                    write_range(sink, lines, *emit, lines.len())?;
                    return Err(e);
                }
            };
            write_range(sink, lines, *emit, boundary)?;
            sink.close();
            *emit = advance(options, boundary, lines.len());
            *search = m.saturating_add(1);
            Ok(Step::Applied)
        }
        Kind::Skip { re, offset } => {
            // No output file: a skipped run of lines is discarded, not written,
            // which is why `csplit f '%nomatch%'` leaves nothing behind at all
            // where `csplit f /nomatch/` leaves a counted (then deleted) piece.
            let found = find(re, lines, *search, &control.arg)?;
            let Some(m) = found else {
                if forever {
                    return Ok(Step::Finished);
                }
                return Err(Fail::fatal(format!(
                    "{}: match not found{}",
                    quote(&control.arg),
                    on_repetition(repetition)
                )));
            };
            let boundary = boundary_of(m, *offset, lines.len())
                .ok_or_else(|| {
                    Fail::fatal(format!(
                        "{}: line number out of range{}",
                        quote(&control.arg),
                        on_repetition(repetition)
                    ))
                })?
                .max(*emit);
            *emit = advance(options, boundary, lines.len());
            *search = m.saturating_add(1);
            Ok(Step::Applied)
        }
    }
}

/// `--suppress-matched` drops the line the split landed on.
///
/// Measured to apply to line numbers too, not only to the patterns whose name
/// the option carries: `csplit f 4 --suppress-matched` writes 1-3 and then 5-20.
fn advance(options: &Options, boundary: usize, total: usize) -> usize {
    if options.suppress_matched && boundary < total {
        boundary.saturating_add(1)
    } else {
        boundary
    }
}

/// The 0-based line the piece ends before, or `None` if the offset walked off
/// the end of the input.
fn boundary_of(matched: usize, offset: i64, total: usize) -> Option<usize> {
    let base = i64::try_from(matched).ok()?;
    let at = base.checked_add(offset)?;
    if at > i64::try_from(total).ok()? {
        return None;
    }
    Some(usize::try_from(at.max(0)).unwrap_or(0))
}

fn find(re: &Regex, lines: &[&[u8]], from: usize, arg: &[u8]) -> Result<Option<usize>, Fail> {
    let mut i = from;
    while let Some(line) = lines.get(i) {
        if line_matches(re, line, arg)? {
            return Ok(Some(i));
        }
        i = i.saturating_add(1);
    }
    Ok(None)
}

fn write_range(sink: &mut Sink, lines: &[&[u8]], from: usize, to: usize) -> Result<(), Fail> {
    let slice = lines.get(from..to.max(from)).unwrap_or_default();
    for line in slice {
        sink.write(line)?;
    }
    Ok(())
}

/// The ` on repetition N` that GNU appends only when N is not zero.
fn on_repetition(repetition: u64) -> String {
    if repetition == 0 {
        String::new()
    } else {
        format!(" on repetition {repetition}")
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn os(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn sink(digits: usize, format: Option<&str>) -> Sink {
        Sink {
            prefix: OsString::from("xx"),
            digits,
            format: format.map(|f| SuffixFormat::parse(f.as_bytes()).unwrap()),
            quiet: true,
            elide_empty: false,
            index: 0,
            written: Vec::new(),
            open: None,
        }
    }

    // ---------------- names ----------------

    #[test]
    fn default_names_are_two_digits() {
        let s = sink(2, None);
        assert_eq!(s.name(0), OsString::from("xx00"));
        assert_eq!(s.name(7), OsString::from("xx07"));
        assert_eq!(s.name(100), OsString::from("xx100"));
    }

    #[test]
    fn zero_digits_does_not_pad() {
        // Measured: `csplit f -n 0 4` writes `xx0` and `xx1`.
        let s = sink(0, None);
        assert_eq!(s.name(0), OsString::from("xx0"));
        assert_eq!(s.name(11), OsString::from("xx11"));
    }

    #[test]
    fn suffix_format_keeps_the_prefix() {
        // Measured: `csplit f -b 'q%03dz' 4` writes `xxq000z`.
        let s = sink(2, Some("q%03dz"));
        assert_eq!(s.name(0), OsString::from("xxq000z"));
        assert_eq!(s.name(1), OsString::from("xxq001z"));
    }

    #[test]
    fn suffix_format_hex_and_bare() {
        assert_eq!(sink(2, Some("%03x")).name(255), OsString::from("xx0ff"));
        assert_eq!(sink(2, Some("%d")).name(3), OsString::from("xx3"));
        assert_eq!(sink(2, Some("%5d")).name(3), OsString::from("xx    3"));
        assert_eq!(sink(2, Some("%-5d")).name(3), OsString::from("xx3    "));
    }

    #[test]
    fn suffix_format_must_have_exactly_one_conversion() {
        assert_eq!(
            SuffixFormat::parse(b"%s").err().unwrap().message,
            "invalid conversion specifier in suffix: s"
        );
        assert_eq!(
            SuffixFormat::parse(b"plain").err().unwrap().message,
            "missing % conversion specification in suffix"
        );
        assert_eq!(
            SuffixFormat::parse(b"%d%d").err().unwrap().message,
            "too many % conversion specifications in suffix"
        );
        // `%%` is a literal percent and does not count as the conversion.
        assert_eq!(
            SuffixFormat::parse(b"100%%").err().unwrap().message,
            "missing % conversion specification in suffix"
        );
        assert_eq!(
            SuffixFormat::parse(b"100%%-%02d").unwrap().render(4),
            b"100%-04".to_vec()
        );
    }

    /// The flag set is GNU's, not printf's: `+` and a space are rejected.
    ///
    /// GNU's parser stops at an unknown flag and reads that byte as the
    /// conversion specifier, so both come back through the *invalid
    /// conversion* sentence rather than an "unknown flag" one — including the
    /// space, which prints as itself and leaves the message ending in a
    /// trailing blank. All measured against GNU 9.4 under `LC_ALL=C.UTF-8`.
    ///
    /// We used to accept both, so `csplit -b '%+d'` quietly wrote files named
    /// `+0` and `+1` for a command line GNU refuses. See `known-issues.md`
    /// → BUG-CSPLIT-ACCEPTS-TWO-SUFFIX-FLAGS-GNU-REJECTS.
    #[test]
    fn the_suffix_flag_set_is_gnus_and_not_printfs() {
        assert_eq!(
            SuffixFormat::parse(b"%+d").err().unwrap().message,
            "invalid conversion specifier in suffix: +"
        );
        assert_eq!(
            SuffixFormat::parse(b"% d").err().unwrap().message,
            "invalid conversion specifier in suffix:  "
        );
        assert_eq!(
            SuffixFormat::parse(b"% 5d").err().unwrap().message,
            "invalid conversion specifier in suffix:  "
        );
        // The four GNU does take still work, which is the half a parser with
        // no flags at all would fail. `'` is accepted and ignored, as the
        // thousands separator is empty in this locale.
        assert_eq!(sink(2, Some("%-5d")).name(3), OsString::from("xx3    "));
        assert_eq!(sink(2, Some("%05d")).name(3), OsString::from("xx00003"));
        assert_eq!(sink(2, Some("%#o")).name(8), OsString::from("xx010"));
        assert_eq!(sink(2, Some("%'d")).name(3), OsString::from("xx3"));
    }

    // ---------------- pattern parsing ----------------

    #[test]
    fn line_numbers_must_ascend() {
        let e = parse_patterns(&os(&["4", "2"])).err().unwrap();
        assert_eq!(
            e.message,
            // Curly: this is `quote()`, measured against GNU 9.4 under
            // `LC_ALL=C.UTF-8`. Contrast `zero_is_refused_unquoted` below,
            // where GNU quotes the number not at all.
            "line number ‘2’ is smaller than preceding line number, 4"
        );
    }

    #[test]
    fn zero_is_refused_unquoted() {
        // Measured: this diagnostic alone does not quote its argument.
        let e = parse_patterns(&os(&["0"])).err().unwrap();
        assert_eq!(e.message, "0: line number must be greater than zero");
    }

    #[test]
    fn a_leading_repeat_is_an_invalid_pattern() {
        // Not "no preceding pattern": GNU looks ahead for `{`, so a leading one
        // reaches the integer branch.
        let e = parse_patterns(&os(&["{3}"])).err().unwrap();
        assert_eq!(e.message, "‘{3}’: invalid pattern");
    }

    #[test]
    fn repeat_count_diagnostics_reproduce_gnus_quoting() {
        let e = parse_patterns(&os(&["4", "{x}"])).err().unwrap();
        // Only the echoed-back argument goes through `quote()`, so only it is
        // curly. The braces on the right are csplit's own, spelled straight
        // into the format string, and the `}` immediately after `‘{x’` is the
        // one GNU prints separately because it NUL-terminated there.
        assert_eq!(e.message, "‘{x’}: integer required between '{' and '}'");
        let e = parse_patterns(&os(&["4", "{1"])).err().unwrap();
        assert_eq!(e.message, "‘{1’: '}' is required in repeat count");
    }

    #[test]
    fn missing_closing_delimiter() {
        let e = parse_patterns(&os(&["/5"])).err().unwrap();
        assert_eq!(e.message, "/5: closing delimiter '/' missing");
    }

    #[test]
    fn offset_must_be_an_integer() {
        let e = parse_patterns(&os(&["/5/xyz"])).err().unwrap();
        assert_eq!(e.message, "‘/5/xyz’: integer expected after delimiter");
    }

    #[test]
    fn offsets_parse_with_either_sign() {
        assert_eq!(parse_offset(b"+3"), Some(3));
        assert_eq!(parse_offset(b"-2"), Some(-2));
        assert_eq!(parse_offset(b"7"), Some(7));
        assert_eq!(parse_offset(b""), None);
        assert_eq!(parse_offset(b"+"), None);
        assert_eq!(parse_offset(b"3x"), None);
    }

    #[test]
    fn the_last_delimiter_closes_the_regexp() {
        let pats = parse_patterns(&os(&["/a/b/"])).unwrap();
        match &pats[0].kind {
            Kind::Split { re, offset } => {
                assert_eq!(*offset, 0);
                assert!(re.is_match(b"xa/by").unwrap());
                assert!(!re.is_match(b"ab").unwrap());
            }
            _ => panic!("expected a split pattern"),
        }
    }

    #[test]
    fn repeat_attaches_to_the_pattern_before_it() {
        let pats = parse_patterns(&os(&["4", "{2}", "/x/", "{*}"])).unwrap();
        assert_eq!(pats.len(), 2);
        assert!(pats[0].repeat == Repeat::Times(2));
        assert!(pats[1].repeat == Repeat::Forever);
    }

    // ---------------- line splitting ----------------

    #[test]
    fn lines_keep_their_terminators() {
        assert_eq!(split_lines(b"a\nb\n"), vec![&b"a\n"[..], &b"b\n"[..]]);
    }

    #[test]
    fn a_final_line_without_a_newline_is_still_a_line() {
        // The bug this replaces: `BufRead::lines` dropped the distinction and
        // every piece was re-terminated, so `csplit nonl 2` reported 3 bytes
        // where GNU reports 3 and wrote 4.
        assert_eq!(
            split_lines(b"x\ny\nz"),
            vec![&b"x\n"[..], &b"y\n"[..], &b"z"[..]]
        );
        assert_eq!(split_lines(b""), Vec::<&[u8]>::new());
        assert_eq!(split_lines(b"\n"), vec![&b"\n"[..]]);
    }

    // ---------------- boundaries ----------------

    #[test]
    fn an_offset_walking_past_the_end_is_out_of_range() {
        assert_eq!(boundary_of(4, 100, 20), None);
        assert_eq!(boundary_of(4, 0, 20), Some(4));
        // Landing exactly on the end is in range: the piece is the whole rest.
        assert_eq!(boundary_of(19, 1, 20), Some(20));
        // A negative offset that would go before the start clamps at zero.
        assert_eq!(boundary_of(1, -5, 20), Some(0));
    }

    #[test]
    fn repetition_zero_is_not_announced() {
        assert_eq!(on_repetition(0), "");
        assert_eq!(on_repetition(6), " on repetition 6");
    }

    // ---------------- options ----------------

    #[test]
    fn digits_must_be_a_number() {
        let e = parse_digits(b"abc").err().unwrap();
        assert_eq!(e.sentence, "invalid number: ‘abc’");
        assert_eq!(e.referral, None);
    }

    #[test]
    fn missing_operands() {
        let e = parse_args(&os(&[])).err().unwrap();
        assert_eq!(e.sentence, "missing operand");
        let e = parse_args(&os(&["f"])).err().unwrap();
        assert_eq!(e.sentence, "missing operand after ‘f’");
    }

    #[test]
    fn options_may_follow_the_patterns() {
        let r = parse_args(&os(&["f", "4", "-z", "--suppress-matched"])).unwrap();
        match r {
            Request::Run(o, file, patterns) => {
                assert!(o.elide_empty);
                assert!(o.suppress_matched);
                assert_eq!(file, OsString::from("f"));
                assert_eq!(patterns, os(&["4"]));
            }
            _ => panic!("expected a run"),
        }
    }

    #[test]
    fn a_lone_dash_is_an_operand() {
        let r = parse_args(&os(&["-", "4"])).unwrap();
        match r {
            Request::Run(_, file, patterns) => {
                assert_eq!(file, OsString::from("-"));
                assert_eq!(patterns, os(&["4"]));
            }
            _ => panic!("expected a run"),
        }
    }

    #[test]
    fn short_option_arguments_may_be_attached_or_separate() {
        let r = parse_args(&os(&["-fpart", "-n", "4", "f", "2"])).unwrap();
        match r {
            Request::Run(o, file, patterns) => {
                assert_eq!(o.prefix, OsString::from("part"));
                assert_eq!(o.digits, 4);
                assert_eq!(file, OsString::from("f"));
                assert_eq!(patterns, os(&["2"]));
            }
            _ => panic!("expected a run"),
        }
    }

    #[test]
    fn help_and_version_end_parsing() {
        assert!(matches!(parse_args(&os(&["--help"])), Ok(Request::Help)));
        assert!(matches!(
            parse_args(&os(&["-z", "--version"])),
            Ok(Request::Version)
        ));
    }
}
