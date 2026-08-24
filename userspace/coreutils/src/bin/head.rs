//! head — output the first part of files.
//!
//! The fourth of the 85 utilities moved onto the shared [`coreutils::getopt`]
//! (see `known-issues.md` → `TD-COREUTILS-LONG-OPTIONS-DO-NOT-ABBREVIATE`), and
//! the first where the conversion was the smaller half of the job. The parser
//! this replaces knew `-n` and nothing else: no `-c`, no `-q`/`-v`, no `-z`, no
//! negative count, no multiplier suffix — and it silently substituted 10 for
//! any count it could not parse, so `head -n 5O file` (letter O) printed ten
//! lines rather than saying so.
//!
//! It also read input as UTF-8 `String` lines, which is wrong twice over on a
//! filesystem whose paths and contents are bytes: a line that is not UTF-8 was
//! reported as an I/O error and truncated the file, and `\r\n` came back out as
//! `\n` because `BufRead::lines` strips the carriage return. `head` copies
//! bytes; it does not decode them.
//!
//! # Two option syntaxes, and only one of them is getopt's
//!
//! `head -3 file` is the pre-POSIX form, and upstream parses it *before*
//! `getopt_long` ever runs — by hand, off `argv[1]` alone. That position rule is
//! observable and surprising: `head -3 -q f` works and `head -q -3 f` does not,
//! answering the second with `invalid trailing option -- 3`. It comes out of
//! the digits `0123456789` being listed in the short-option string, so that a
//! digit reaching getopt at all means it was not first, and upstream turns that
//! into a diagnostic rather than a count.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quoteaf;
use coreutils::xnum::xdectoumax;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::process::ExitCode;

/// Measured: `head --zzz; echo $?` is 1, like almost every utility and unlike
/// `ls`/`sort`/`grep`.
const HEAD: Program = Program::new("head", 1);

/// The long options in **GNU's declaration order**, which is observable: it is
/// the order `getopt_long` lists candidates in when an abbreviation is
/// ambiguous. Measured with `head --=x`, an empty prefix that matches every
/// entry and so prints the whole table.
///
/// `-presume-input-pipe` looks like a typo and is not. Upstream hides the
/// option by giving it a name that begins with a dash, so the spelling a user
/// must type is `---presume-input-pipe` — three dashes. Since a long name is
/// matched against everything after the leading `--`, the extra dash simply
/// belongs to the name.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Required),
    ("lines", Takes::Required),
    ("-presume-input-pipe", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The count with no `-n`/`-c`, and the number the help text quotes.
const DEFAULT_NUMBER: u64 = 10;

/// Whether the count is in lines or in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Lines,
    Bytes,
}

impl Unit {
    /// The half of the diagnostic that names the unit —
    /// `invalid number of lines` against `invalid number of bytes`.
    fn invalid_number(self) -> &'static str {
        match self {
            Self::Lines => "invalid number of lines",
            Self::Bytes => "invalid number of bytes",
        }
    }
}

/// When to print a `==> name <==` banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Headers {
    Never,
    /// The default: only when there is more than one operand.
    Multiple,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Options {
    unit: Unit,
    n_units: u64,
    /// `-n -5`: print all *but* the last five, rather than the first five.
    elide_from_end: bool,
    headers: Headers,
    /// `-z` makes this NUL. It is what a "line" ends with everywhere below,
    /// which is why it is carried rather than hard-coded.
    line_end: u8,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            unit: Unit::Lines,
            n_units: DEFAULT_NUMBER,
            elide_from_end: false,
            headers: Headers::Multiple,
            line_end: b'\n',
        }
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Options, Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("head (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, files)) => run(&options, &files),
        Err(e) => {
            // The referral, when there is one, is part of the message, and only
            // the first line carries the `head: ` prefix — which is what GNU
            // prints.
            diag!("head: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    format!(
        "\
Usage: head [OPTION]... [FILE]...
Print the first {DEFAULT_NUMBER} lines of each FILE to standard output.
With more than one FILE, precede each with a header giving the file name.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -c, --bytes=[-]NUM       print the first NUM bytes of each file;
                             with the leading '-', print all but the last
                             NUM bytes of each file
  -n, --lines=[-]NUM       print the first NUM lines instead of the first \
{DEFAULT_NUMBER};
                             with the leading '-', print all but the last
                             NUM lines of each file
  -q, --quiet, --silent    never print headers giving file names
  -v, --verbose            always print headers giving file names
  -z, --zero-terminated    line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

NUM may have a multiplier suffix:
b 512, kB 1000, K 1024, MB 1000*1000, M 1024*1024,
GB 1000*1000*1000, G 1024*1024*1024, and so on for T, P, E, Z, Y, R, Q.
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.
"
    )
}

// ---------------------------------------------------------------- parsing ---

/// Parse argv: the obsolete `-NUM` form first, if it is where that form is
/// allowed to be, and then `getopt_long`.
///
/// # Errors
///
/// Any getopt diagnostic, plus `head`'s own two: a count that is not a number,
/// and a digit reaching getopt (which means an obsolete form out of position).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

    // The obsolete form is recognised in exactly one place: the first argument.
    // Upstream's test is `argv[1][0] == '-' && ISDIGIT (argv[1][1])`, which is
    // why `head -q -3 f` is an error while `head -3 -q f` is not.
    if let Some(first) = args.first()
        && let Some(digits) = starts_obsolete(&arg_bytes(first))
    {
        apply_obsolete(&digits, &mut options)?;
        i = 1;
    }

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        if only_operands {
            files.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand, not an option.
            files.push(arg.clone());
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut options)? {
                return Ok(request);
            }
        } else if let Some(request) = short_options(&bytes, args, &mut i, &mut options)? {
            return Ok(request);
        }
    }

    Ok(Request::Run(options, files))
}

/// Is this argument the obsolete `-NUM[bkmlqvz...]` form? If so, return it
/// without its leading dash.
///
/// The test is deliberately upstream's rather than "parses as a number": the
/// *second byte* must be a digit and nothing else is examined, so `-3zzz` takes
/// this path and is then rejected by [`apply_obsolete`] with a different
/// sentence than `-zzz` would get.
fn starts_obsolete(bytes: &[u8]) -> Option<Vec<u8>> {
    match (bytes.first(), bytes.get(1)) {
        (Some(b'-'), Some(c)) if c.is_ascii_digit() => Some(bytes.get(1..)?.to_vec()),
        _ => None,
    }
}

/// The obsolete form: digits, then any number of option letters.
///
/// The letters are not independent of the digits — `b`, `k` and `m` are
/// *multiplier* suffixes that upstream appends to the digit string before
/// parsing it, so `head -2k f` is 2048 lines and not "2 lines, and also k".
/// `c` selects bytes and clears any multiplier already seen; `l` selects lines
/// and, notably, does **not** clear it, so `-2kl` is 2048 lines.
fn apply_obsolete(body: &[u8], options: &mut Options) -> Result<(), getopt::Error> {
    let end = body
        .iter()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(body.len());
    let (digits, letters) = body.split_at(end);
    let mut multiplier: Option<u8> = None;
    let mut unit = Unit::Lines;

    for &c in letters {
        match c {
            b'c' => {
                unit = Unit::Bytes;
                multiplier = None;
            }
            b'b' | b'k' | b'm' => {
                unit = Unit::Bytes;
                multiplier = Some(c);
            }
            b'l' => unit = Unit::Lines,
            b'q' => options.headers = Headers::Never,
            b'v' => options.headers = Headers::Always,
            b'z' => options.line_end = 0,
            // Measured: no quotes around the letter, unlike getopt's
            // `invalid option -- 'x'`, and it *does* carry the referral because
            // upstream reaches it through `usage (EXIT_FAILURE)`.
            _ => {
                return Err(
                    HEAD.usage_referring(format!("invalid trailing option -- {}", char::from(c)))
                );
            }
        }
    }

    let mut text = digits.to_vec();
    text.extend(multiplier);
    options.unit = unit;
    options.n_units = parse_count(&text, unit)?;
    // The obsolete form cannot express a negative count: a `-` would have to
    // precede the digits, and there is already one there introducing the
    // option.
    options.elide_from_end = false;
    Ok(())
}

/// One `-abc` cluster. Returns `Some` when an option ends parsing.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
) -> Result<Option<Request>, getopt::Error> {
    let body = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
    // `invalid option -- 'é'`, an option nobody typed.
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        match c {
            b'q' => options.headers = Headers::Never,
            b'v' => options.headers = Headers::Always,
            b'z' => options.line_end = 0,
            b'c' | b'n' => {
                let unit = if c == b'c' { Unit::Bytes } else { Unit::Lines };
                // The value is the rest of the cluster if there is one, else
                // the next argument.
                let value: Vec<u8> = match body.get(at..) {
                    Some(rest) if !rest.is_empty() => {
                        at = body.len();
                        rest.to_vec()
                    }
                    _ => {
                        let next = args
                            .get(*i)
                            .ok_or_else(|| HEAD.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        arg_bytes(&next)
                    }
                };
                set_count(unit, &value, options)?;
            }
            // A digit only reaches here when it was not the first argument,
            // which is precisely the case upstream refuses. The `-NUM` form is
            // handled before this loop ever runs.
            b'0'..=b'9' => {
                return Err(
                    HEAD.usage_referring(format!("invalid trailing option -- {}", char::from(c)))
                );
            }
            _ => return Err(HEAD.invalid_option(c)),
        }
    }
    Ok(None)
}

/// One `--name` argument. Returns `Some` when the option ends parsing —
/// `--help` and `--version` — and `None` when it only set something.
fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
) -> Result<Option<Request>, getopt::Error> {
    let body = bytes.get(2..).unwrap_or_default();
    // `--name=value`: split before resolving, so the name is what gets matched
    // and the whole argument is what gets echoed back when it resolves to
    // nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    // Every option is ASCII, so a name that is not UTF-8 matches none of them;
    // it takes the unrecognised path rather than erroring differently.
    let typed = std::str::from_utf8(typed).map_err(|_| HEAD.unrecognized_option(bytes))?;
    let (name, takes) = HEAD.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(HEAD.long_unwanted_argument(name));
    }
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| HEAD.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(arg_bytes(&next))
        }
        (_, None) => None,
    };

    // `match_same_arms`: `---presume-input-pipe` and the unreachable catch-all
    // both do nothing, and merging them would delete the only statement of
    // *why* one of them does nothing.
    #[allow(clippy::match_same_arms)]
    match name {
        "bytes" => set_count(Unit::Bytes, &value.unwrap_or_default(), options)?,
        "lines" => set_count(Unit::Lines, &value.unwrap_or_default(), options)?,
        // Upstream uses this to skip an `lseek`-based fast path it has for
        // seekable input and we do not: every input here already takes the
        // streaming path, so the option is accepted and does nothing. It is in
        // the table because the ambiguity list is, and dropping it would let
        // `--p` resolve to something.
        "-presume-input-pipe" => {}
        "quiet" | "silent" => options.headers = Headers::Never,
        "verbose" => options.headers = Headers::Always,
        "zero-terminated" => options.line_end = 0,
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// Apply a `-n`/`-c` value, splitting off the leading `-` that means "from the
/// end".
///
/// The `-` is taken off the *first byte*, before any whitespace is skipped, and
/// the rest is what both the parser and the diagnostic see. That is why
/// `head -n -` reports an empty number rather than `-`, and why `head -n " -5"`
/// is an error while `head -n "- 5"` is five lines from the end.
fn set_count(unit: Unit, value: &[u8], options: &mut Options) -> Result<(), getopt::Error> {
    let from_end = value.first() == Some(&b'-');
    let text = if from_end {
        value.get(1..).unwrap_or_default()
    } else {
        value
    };
    options.unit = unit;
    options.elide_from_end = from_end;
    options.n_units = parse_count(text, unit)?;
    Ok(())
}

/// The suffix list `head` hands to gnulib, verbatim from upstream's two
/// `xdectoumax` calls.
///
/// gnulib's `xstrtoumax` knows more suffixes than this (`c`, `w`, `g`, `t`),
/// but one outside the caller's list is rejected — which is why `head -n 1w` is
/// an error even though `dd`-style `w` exists in the same function. The
/// trailing `0` is not a suffix: it is gnulib's flag enabling the *second*
/// suffix, so that `B`/`D` switch the base to 1000 and `iB` pins it at 1024.
const SUFFIXES: &[u8] = b"bkKmMGTPEZYRQ0";

/// gnulib's `xdectoumax` as `head` calls it: a decimal count with an optional
/// multiplier suffix.
///
/// The grammar itself lives in [`coreutils::xnum`], shared with `fold`, `nl`
/// and every other utility that reads a number through gnulib — this function
/// is only the two things that are `head`'s own: which suffixes are allowed,
/// and which half of the diagnostic names the unit.
///
/// The rules that are not guessable, all measured against glibc and all tested
/// in `xnum`:
///
/// - **Leading whitespace and a leading `+` are accepted** (`strtoumax` skips
///   them), but trailing whitespace is not: `head -n "  5"` works, `head -n "5 "`
///   does not.
/// - **A bare suffix means one of it.** `head -n K` is 1024 lines, because when
///   `strtoumax` consumes nothing gnulib substitutes 1 — but only if the very
///   first byte is itself a valid suffix, so `head -n " K"` is still an error.
/// - **A second suffix changes the base.** `B` or `D` after the first make it a
///   power of 1000; `iB` keeps 1024. So `1K` and `1KiB` are 1024 while `1kB` is
///   1000, and a lone `i` (`1Ki`) is a trailing byte and therefore invalid.
/// - **A bad suffix outranks an overflow.** `head -n 99999999999999999999X`
///   reports an invalid number, not a value too large, even though the digits
///   alone would overflow — so the suffix must be validated before the
///   magnitude is reported.
///
/// `head`'s range is the whole of `u64`, so the `xdectoumax` bound check can
/// never fire and the only tail this can produce is the overflow one. That is
/// why there is no `Numerical result out of range` here and there is one in
/// `fold`: the sentence is chosen by the value, and `head` has no value it
/// rejects for being too small.
///
/// # Errors
///
/// A number that does not parse, or one that overflows `u64`.
fn parse_count(text: &[u8], unit: Unit) -> Result<u64, getopt::Error> {
    // `xdectoumax` quotes the offending text with `quote()`, whose escaping is
    // C's rather than the shell's. The two agree on everything without a quote
    // or a backslash in it, which is why the difference was invisible until a
    // value was passed one — `head -n "a'b"` must say `'a\'b'`, not `"a'b"`.
    xdectoumax(text, 0, u64::MAX, Some(SUFFIXES), unit.invalid_number()).map_err(|m| HEAD.usage(m))
}

// --------------------------------------------------------------- printing ---

/// Run over every operand, returning the exit status.
fn run(options: &Options, files: &[OsString]) -> ExitCode {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    // Whether a banner has been printed yet, which is what decides the blank
    // line that separates them. It counts *printed* banners, not files: a file
    // that fails to open never gets one, so the next file that succeeds is
    // still the first and gets no leading blank line.
    let mut printed_header = false;
    let mut ok = true;
    let default = [OsString::from("-")];
    let operands: &[OsString] = if files.is_empty() { &default } else { files };

    let headers = match options.headers {
        Headers::Never => false,
        Headers::Always => true,
        Headers::Multiple => operands.len() > 1,
    };

    for name in operands {
        let bytes = arg_bytes(name);
        let is_stdin = bytes == b"-";
        let label: &[u8] = if is_stdin { b"standard input" } else { &bytes };

        let mut source: Box<dyn Read> = if is_stdin {
            Box::new(io::stdin())
        } else {
            match File::open(name) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    diag!(
                        "head: cannot open {} for reading: {}",
                        quoteaf(&bytes),
                        strerror(&e)
                    );
                    ok = false;
                    continue;
                }
            }
        };

        if headers {
            let sep = if printed_header { "\n" } else { "" };
            // The banner is the raw name, unquoted — upstream quotes it in
            // diagnostics and not here.
            if write_all(&mut out, sep.as_bytes())
                .and_then(|()| write_all(&mut out, b"==> "))
                .and_then(|()| write_all(&mut out, label))
                .and_then(|()| write_all(&mut out, b" <==\n"))
                .is_err()
            {
                return ExitCode::from(1);
            }
            printed_header = true;
        }

        if let Err(e) = emit(&mut source, &mut out, options) {
            if e.kind() == ErrorKind::BrokenPipe {
                // Nothing downstream is listening; there is nothing to report
                // and nothing left to do.
                return ExitCode::from(u8::from(!ok));
            }
            diag!("head: error reading {}: {}", quoteaf(label), strerror(&e));
            ok = false;
        }
    }

    if out.flush().is_err() {
        return ExitCode::from(1);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn write_all(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    out.write_all(bytes)
}

/// How much is read at a time. Only a performance choice — every routine below
/// is correct for any chunking, including a pipe that dribbles one byte at a
/// time.
const CHUNK: usize = 64 * 1024;

/// Copy the requested part of `source` to `out`.
///
/// Every path here streams. That is not an optimisation but a correctness
/// requirement: `yes | head -n1` must print one line and exit, so nothing may
/// wait for end of input that does not have to. The two eliding paths do have
/// to, and they buffer only the tail they might still need to drop.
fn emit(source: &mut impl Read, out: &mut impl Write, options: &Options) -> io::Result<()> {
    match (options.unit, options.elide_from_end) {
        (Unit::Bytes, false) => head_bytes(source, out, options.n_units),
        (Unit::Lines, false) => head_lines(source, out, options.n_units, options.line_end),
        (Unit::Bytes, true) => elide_tail_bytes(source, out, options.n_units),
        (Unit::Lines, true) => elide_tail_lines(source, out, options.n_units, options.line_end),
    }
}

/// The first `n` bytes.
fn head_bytes(source: &mut impl Read, out: &mut impl Write, n: u64) -> io::Result<()> {
    let mut left = n;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let want = usize::try_from(left.min(CHUNK as u64)).unwrap_or(CHUNK);
        let got = source.read(buf.get_mut(..want).unwrap_or_default())?;
        if got == 0 {
            break;
        }
        out.write_all(buf.get(..got).unwrap_or_default())?;
        left = left.saturating_sub(got as u64);
    }
    Ok(())
}

/// The bytes up to and including the `n`th line terminator.
///
/// A final line with no terminator is emitted whole — this copies bytes and
/// never adds one. Ending at the terminator rather than at the start of the
/// next line is what makes `printf 'a\nb' | head -n1` print `a\n` and not
/// `a\nb`.
fn head_lines(
    source: &mut impl Read,
    out: &mut impl Write,
    n: u64,
    line_end: u8,
) -> io::Result<()> {
    if n == 0 {
        return Ok(());
    }
    let mut left = n;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let got = source.read(&mut buf)?;
        if got == 0 {
            return Ok(());
        }
        let chunk = buf.get(..got).unwrap_or_default();
        let mut at = 0usize;
        while let Some(rel) = chunk
            .get(at..)
            .and_then(|t| t.iter().position(|&b| b == line_end))
        {
            at = at.saturating_add(rel).saturating_add(1);
            left = left.saturating_sub(1);
            if left == 0 {
                return out.write_all(chunk.get(..at).unwrap_or_default());
            }
        }
        out.write_all(chunk)?;
    }
}

/// Everything but the last `n` bytes.
///
/// Held bytes never exceed `n`: a byte is written as soon as `n` later ones
/// exist to keep it out of the tail.
fn elide_tail_bytes(source: &mut impl Read, out: &mut impl Write, n: u64) -> io::Result<()> {
    if n == 0 {
        return io::copy(source, out).map(|_| ());
    }
    let Ok(keep) = usize::try_from(n) else {
        // More bytes than this machine can address means the tail is the whole
        // input, whatever its length.
        return drain(source);
    };
    let mut held: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let got = source.read(&mut buf)?;
        if got == 0 {
            return Ok(());
        }
        held.extend_from_slice(buf.get(..got).unwrap_or_default());
        if let Some(spill) = held.len().checked_sub(keep) {
            out.write_all(held.get(..spill).unwrap_or_default())?;
            held.drain(..spill);
        }
    }
}

/// Everything but the last `n` lines.
///
/// A final line with no terminator counts as a line, so
/// `printf 'a\nb' | head -n -1` prints `a\n`. That is only knowable at end of
/// input, which is why the streaming loop holds one line more than it strictly
/// needs and the decision is made once the read returns zero.
fn elide_tail_lines(
    source: &mut impl Read,
    out: &mut impl Write,
    n: u64,
    line_end: u8,
) -> io::Result<()> {
    if n == 0 {
        return io::copy(source, out).map(|_| ());
    }
    let mut held: Vec<u8> = Vec::new();
    // Terminators inside `held`, kept incrementally so the buffer is not
    // rescanned on every read.
    let mut lines: u64 = 0;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let got = source.read(&mut buf)?;
        if got == 0 {
            break;
        }
        let chunk = buf.get(..got).unwrap_or_default();
        // `naive_bytecount` wants the `bytecount` crate; the coreutils do not
        // depend on it, and this is not a hot path — the eliding form is the
        // rare one, and the scan is once per byte read either way.
        #[allow(clippy::naive_bytecount)]
        let found = chunk.iter().filter(|&&b| b == line_end).count();
        lines = lines.saturating_add(found as u64);
        held.extend_from_slice(chunk);
        // While there are more terminated lines than the tail could need, the
        // earliest is safe to write no matter what follows.
        while lines > n {
            let Some(at) = held.iter().position(|&b| b == line_end) else {
                break;
            };
            let end = at.saturating_add(1);
            out.write_all(held.get(..end).unwrap_or_default())?;
            held.drain(..end);
            lines = lines.saturating_sub(1);
        }
    }
    // An unterminated remainder is a line too, and being last it is always
    // inside the elided tail — so it only ever raises the total, never gets
    // printed.
    let total = if held.last().is_some_and(|&b| b != line_end) {
        lines.saturating_add(1)
    } else {
        lines
    };
    let mut emit_lines = total.saturating_sub(n);
    let mut at = 0usize;
    while emit_lines > 0 {
        let Some(rel) = held
            .get(at..)
            .and_then(|t| t.iter().position(|&b| b == line_end))
        else {
            break;
        };
        at = at.saturating_add(rel).saturating_add(1);
        emit_lines = emit_lines.saturating_sub(1);
    }
    out.write_all(held.get(..at).unwrap_or_default())
}

/// Consume the input without printing any of it, so that a pipe's writer is not
/// left blocked on a reader that vanished.
fn drain(source: &mut impl Read) -> io::Result<()> {
    let mut buf = vec![0u8; CHUNK];
    while source.read(&mut buf)? != 0 {}
    Ok(())
}

// -------------------------------------------------------------- byte paths ---

#[cfg(unix)]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    a.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    a.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn parse(items: &[&str]) -> Options {
        match parse_args(&args(items)) {
            Ok(Request::Run(o, _)) => o,
            other => panic!("expected a run request, got {other:?}"),
        }
    }

    fn operands(items: &[&str]) -> Vec<String> {
        match parse_args(&args(items)) {
            Ok(Request::Run(_, files)) => files
                .iter()
                .map(|f| f.to_string_lossy().into_owned())
                .collect(),
            other => panic!("expected a run request, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    /// The diagnostic's own sentence, without the referral most of these carry.
    fn body(e: &getopt::Error) -> String {
        e.sentence.clone()
    }

    fn run_on(input: &[u8], items: &[&str]) -> Vec<u8> {
        let options = parse(items);
        let mut out: Vec<u8> = Vec::new();
        emit(&mut &input[..], &mut out, &options).unwrap();
        out
    }

    // ------------------------------------------------------------ options ---

    #[test]
    fn the_default_is_ten_lines_from_the_front() {
        let o = parse(&[]);
        assert_eq!(o.unit, Unit::Lines);
        assert_eq!(o.n_units, 10);
        assert!(!o.elide_from_end);
        assert_eq!(o.headers, Headers::Multiple);
        assert_eq!(o.line_end, b'\n');
    }

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        assert_eq!(parse(&["--by", "3"]).unit, Unit::Bytes);
        assert_eq!(parse(&["--li=3"]).unit, Unit::Lines);
        assert_eq!(parse(&["--verb"]).headers, Headers::Always);
        assert_eq!(parse(&["--zero"]).line_end, 0);
        // `--q` and `--s` are distinct spellings of the same thing, and both
        // resolve because neither prefixes anything else.
        assert_eq!(parse(&["--q"]).headers, Headers::Never);
        assert_eq!(parse(&["--s"]).headers, Headers::Never);
    }

    /// The hidden option really does take three dashes, and really is in the
    /// ambiguity list under that spelling.
    #[test]
    fn presume_input_pipe_is_spelled_with_three_dashes() {
        assert_eq!(parse(&["---presume-input-pipe"]), Options::default());
        assert_eq!(parse(&["---p"]), Options::default());
        // Two dashes is a different name, and matches nothing.
        assert_eq!(
            body(&fail(&["--presume-input-pipe"])),
            "unrecognized option '--presume-input-pipe'"
        );
    }

    /// Measured with `head --=x`: an empty prefix matches every option, so the
    /// list is the whole table in declaration order.
    #[test]
    fn the_ambiguity_list_is_in_gnus_declaration_order() {
        assert_eq!(
            body(&fail(&["--=x"])),
            "option '--=x' is ambiguous; possibilities: '--bytes' '--lines' \
             '---presume-input-pipe' '--quiet' '--silent' '--verbose' \
             '--zero-terminated' '--help' '--version'"
        );
    }

    #[test]
    fn every_getopt_sentence_matches_glibc() {
        assert_eq!(body(&fail(&["-x"])), "invalid option -- 'x'");
        assert_eq!(body(&fail(&["-c"])), "option requires an argument -- 'c'");
        assert_eq!(body(&fail(&["--fo"])), "unrecognized option '--fo'");
        assert_eq!(
            body(&fail(&["--lines"])),
            "option '--lines' requires an argument"
        );
        assert_eq!(
            body(&fail(&["--verbose=1"])),
            "option '--verbose' doesn't allow an argument"
        );
        for a in [["-x"], ["--fo"], ["--lines"]] {
            assert_eq!(fail(&a).status, 1);
        }
    }

    #[test]
    fn a_lone_dash_and_everything_after_dash_dash_are_operands() {
        assert_eq!(operands(&["-n1", "-", "f"]), vec!["-", "f"]);
        assert_eq!(operands(&["--", "-3", "-v"]), vec!["-3", "-v"]);
        // And an option after an operand is still an option: glibc permutes.
        assert_eq!(parse(&["f", "-v"]).headers, Headers::Always);
    }

    #[test]
    fn a_short_cluster_takes_its_value_from_the_rest_or_the_next_argument() {
        assert_eq!(parse(&["-qn2"]).n_units, 2);
        assert_eq!(parse(&["-qn", "2"]).n_units, 2);
        assert_eq!(
            parse(&["-vz"]),
            Options {
                headers: Headers::Always,
                line_end: 0,
                ..Options::default()
            }
        );
        // `-c2n`: the whole rest of the cluster is `c`'s argument, so the `n`
        // is part of the number and the number is bad.
        assert_eq!(fail(&["-c2n"]).sentence, "invalid number of bytes: ‘2n’");
    }

    // ---------------------------------------------------- the obsolete form ---

    #[test]
    fn the_obsolete_form_is_only_the_first_argument() {
        assert_eq!(parse(&["-3", "f"]).n_units, 3);
        // Overridden by a later `-n`, because it is parsed first.
        assert_eq!(parse(&["-3", "-n2"]).n_units, 2);
        // Anywhere else a digit is an option, and upstream refuses it with its
        // own sentence rather than getopt's `invalid option`.
        for a in [
            vec!["-n2", "-3"],
            vec!["-q", "-3"],
            vec!["f", "-3"],
            vec!["-3", "-3"],
        ] {
            let e = fail(&a);
            assert_eq!(body(&e), "invalid trailing option -- 3", "{a:?}");
            assert_eq!(e.status, 1);
        }
    }

    #[test]
    fn the_obsolete_forms_letters_are_multipliers_not_flags() {
        // `b`, `k` and `m` are suffixes on the digits, so they scale.
        assert_eq!(parse(&["-2b"]).n_units, 1024);
        assert_eq!(parse(&["-2b"]).unit, Unit::Bytes);
        assert_eq!(parse(&["-2k"]).n_units, 2048);
        assert_eq!(parse(&["-2m"]).n_units, 2 * 1024 * 1024);
        // `c` selects bytes and clears the multiplier …
        assert_eq!(parse(&["-2kc"]).n_units, 2);
        // … but `l` selects lines and does not, which is upstream's asymmetry
        // and not a slip here.
        assert_eq!(
            parse(&["-2kl"]),
            Options {
                unit: Unit::Lines,
                n_units: 2048,
                ..Options::default()
            }
        );
        assert_eq!(
            parse(&["-3qz"]),
            Options {
                n_units: 3,
                headers: Headers::Never,
                line_end: 0,
                ..Options::default()
            }
        );
        // An unknown letter is the trailing-option sentence, with the letter
        // unquoted — unlike getopt's `invalid option -- 'x'`.
        assert_eq!(body(&fail(&["-3x"])), "invalid trailing option -- x");
    }

    // ------------------------------------------------------------ numbers ---

    #[test]
    fn a_leading_dash_on_the_value_means_from_the_end() {
        let o = parse(&["-n", "-2"]);
        assert!(o.elide_from_end);
        assert_eq!(o.n_units, 2);
        assert!(parse(&["-c-3"]).elide_from_end);
        // The dash is taken off the first byte, before whitespace is skipped,
        // and the rest is what gets parsed *and* what gets quoted back.
        assert_eq!(parse(&["-n", "- 5"]).n_units, 5);
        assert_eq!(
            fail(&["-n", " -5"]).sentence,
            "invalid number of lines: ‘ -5’"
        );
        // A lone `-` leaves nothing behind it.
        assert_eq!(fail(&["-n", "-"]).sentence, "invalid number of lines: ‘’");
        // Two dashes leave one, which an unsigned parse refuses — and the
        // message shows the stripped string, not what was typed.
        assert_eq!(
            fail(&["-n", "--5"]).sentence,
            "invalid number of lines: ‘-5’"
        );
    }

    #[test]
    fn a_multiplier_suffix_scales_the_count() {
        assert_eq!(parse(&["-n", "5K"]).n_units, 5 * 1024);
        assert_eq!(parse(&["-n", "5k"]).n_units, 5 * 1024);
        assert_eq!(parse(&["-n", "5kB"]).n_units, 5000);
        assert_eq!(parse(&["-n", "5KiB"]).n_units, 5 * 1024);
        assert_eq!(parse(&["-n", "1M"]).n_units, 1024 * 1024);
        assert_eq!(parse(&["-n", "1MB"]).n_units, 1_000_000);
        // The obsolescent second suffix `D` is decimal like `B`.
        assert_eq!(parse(&["-n", "1MD"]).n_units, 1_000_000);
        assert_eq!(parse(&["-n", "1b"]).n_units, 512);
        assert_eq!(parse(&["-n", "1G"]).n_units, 1024 * 1024 * 1024);
        // A bare suffix is one of it.
        assert_eq!(parse(&["-n", "K"]).n_units, 1024);
        assert_eq!(parse(&["-n", "b"]).n_units, 512);
    }

    #[test]
    fn the_suffixes_head_does_not_accept() {
        // gnulib knows these; `head`'s list does not include them.
        for bad in ["1w", "1c", "1B", "1g", "1t", "1D"] {
            assert_eq!(
                fail(&["-n", bad]).sentence,
                format!("invalid number of lines: ‘{bad}’"),
            );
        }
        // A lone `i` is not `iB`, so it is a trailing byte.
        assert_eq!(
            fail(&["-n", "1Ki"]).sentence,
            "invalid number of lines: ‘1Ki’"
        );
        assert_eq!(
            fail(&["-n", "1KiBB"]).sentence,
            "invalid number of lines: ‘1KiBB’"
        );
        assert_eq!(
            fail(&["-n", "5K5"]).sentence,
            "invalid number of lines: ‘5K5’"
        );
    }

    #[test]
    fn whitespace_and_sign_are_accepted_only_where_strtoumax_accepts_them() {
        assert_eq!(parse(&["-n", "  5"]).n_units, 5);
        assert_eq!(parse(&["-n", "+5"]).n_units, 5);
        assert_eq!(parse(&["-n", "0005"]).n_units, 5);
        // Trailing space is a trailing byte.
        assert_eq!(
            fail(&["-n", "5 "]).sentence,
            "invalid number of lines: ‘5 ’"
        );
        // The bare-suffix fallback looks at the first byte of the whole string,
        // so a suffix behind whitespace does not qualify.
        assert_eq!(
            fail(&["-n", " K"]).sentence,
            "invalid number of lines: ‘ K’"
        );
        assert_eq!(
            fail(&["-n", "+K"]).sentence,
            "invalid number of lines: ‘+K’"
        );
        assert_eq!(fail(&["-n", " "]).sentence, "invalid number of lines: ‘ ’");
        // Base 10 only: `0x10` stops at the `x`.
        assert_eq!(
            fail(&["-n", "0x10"]).sentence,
            "invalid number of lines: ‘0x10’"
        );
    }

    #[test]
    fn overflow_is_a_different_sentence_and_a_bad_suffix_outranks_it() {
        assert_eq!(parse(&["-n", "18446744073709551615"]).n_units, u64::MAX);
        assert_eq!(
            fail(&["-n", "18446744073709551616"]).sentence,
            "invalid number of lines: ‘18446744073709551616’: \
             Value too large for defined data type"
        );
        // 2^64 / 1024 rounds to this; one more overflows.
        assert_eq!(
            parse(&["-n", "18014398509481983K"]).n_units,
            18_014_398_509_481_983 * 1024
        );
        assert_eq!(
            fail(&["-n", "18014398509481984K"]).sentence,
            "invalid number of lines: ‘18014398509481984K’: \
             Value too large for defined data type"
        );
        // 1024^7 and up cannot fit at all.
        assert_eq!(parse(&["-n", "1E"]).n_units, 1024u64.pow(6));
        for big in ["1Z", "1Y", "1R", "1Q"] {
            assert!(
                fail(&["-n", big])
                    .sentence
                    .ends_with("Value too large for defined data type"),
                "{big}"
            );
        }
        // A suffix that is not a suffix wins over the magnitude: this reports
        // an invalid number even though the digits alone would overflow.
        assert_eq!(
            fail(&["-c", "99999999999999999999X"]).sentence,
            "invalid number of bytes: ‘99999999999999999999X’"
        );
        // And the unit changes the noun.
        assert_eq!(fail(&["-c", "x"]).sentence, "invalid number of bytes: ‘x’");
        // These are the utility's own usage errors, so no referral.
        assert_eq!(fail(&["-c", "x"]).referral, None);
    }

    // ------------------------------------------------------------ copying ---

    #[test]
    fn the_first_n_lines_end_at_the_nth_terminator() {
        assert_eq!(run_on(b"a\nb\nc\n", &["-n2"]), b"a\nb\n");
        // A final line with no terminator is copied as it is; nothing is added.
        assert_eq!(run_on(b"a\nb", &["-n1"]), b"a\n");
        assert_eq!(run_on(b"a\nb", &["-n5"]), b"a\nb");
        assert_eq!(run_on(b"a\nb\nc\n", &["-n0"]), b"");
        assert_eq!(run_on(b"", &["-n5"]), b"");
    }

    #[test]
    fn bytes_are_bytes_and_are_never_decoded() {
        // Invalid UTF-8 and a CRLF both survive intact, which the previous
        // `BufRead::lines` implementation could not manage: it reported the
        // first as an I/O error and silently ate the `\r` of the second.
        assert_eq!(run_on(b"\xff\xfe\n\x80\n", &["-n1"]), b"\xff\xfe\n");
        assert_eq!(run_on(b"a\r\nb\r\n", &["-n1"]), b"a\r\n");
        assert_eq!(run_on(b"\xc3\x28abc", &["-c3"]), b"\xc3\x28a");
        assert_eq!(run_on(b"abc", &["-c9"]), b"abc");
        assert_eq!(run_on(b"abc", &["-c0"]), b"");
    }

    #[test]
    fn zero_terminated_changes_what_a_line_is() {
        assert_eq!(run_on(b"a\0b\0c\0", &["-z", "-n2"]), b"a\0b\0");
        // A newline is now just a byte inside a record.
        assert_eq!(run_on(b"a\nb\0c\0", &["-z", "-n1"]), b"a\nb\0");
        assert_eq!(run_on(b"a\0b\0c", &["-z", "-n5"]), b"a\0b\0c");
        assert_eq!(run_on(b"a\0b\0c\0", &["-z", "-n", "-1"]), b"a\0b\0");
    }

    #[test]
    fn eliding_from_the_end_counts_an_unterminated_line() {
        assert_eq!(run_on(b"a\nb\nc\n", &["-n", "-1"]), b"a\nb\n");
        // `b` has no terminator but is still a line, so it is the one dropped.
        assert_eq!(run_on(b"a\nb", &["-n", "-1"]), b"a\n");
        assert_eq!(run_on(b"a\nb\nc\n", &["-n", "-9"]), b"");
        assert_eq!(run_on(b"a\nb\nc\n", &["-n", "-0"]), b"a\nb\nc\n");
        assert_eq!(run_on(b"", &["-n", "-1"]), b"");
        // An unterminated line is never *printed* by this path — it only
        // raises the total. Here it does raise it, so two whole lines print
        // and the partial `c` is what gets dropped.
        assert_eq!(run_on(b"a\nb\nc", &["-n", "-1"]), b"a\nb\n");
        assert_eq!(run_on(b"a\nb\nc", &["-n", "-2"]), b"a\n");
    }

    #[test]
    fn eliding_bytes_from_the_end() {
        assert_eq!(run_on(b"abcde", &["-c", "-1"]), b"abcd");
        assert_eq!(run_on(b"abcde", &["-c", "-5"]), b"");
        assert_eq!(run_on(b"abcde", &["-c", "-9"]), b"");
        assert_eq!(run_on(b"abcde", &["-c", "-0"]), b"abcde");
    }

    /// Every routine must be correct for any chunking, because a pipe decides
    /// the chunking and not us. This feeds one byte per `read`.
    #[test]
    fn a_dribbling_reader_gives_the_same_answer_as_one_big_read() {
        struct OneByteAtATime<'a>(&'a [u8]);
        impl Read for OneByteAtATime<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                match (self.0.first(), buf.first_mut()) {
                    (Some(&b), Some(slot)) => {
                        *slot = b;
                        self.0 = &self.0[1..];
                        Ok(1)
                    }
                    _ => Ok(0),
                }
            }
        }
        let input = b"one\ntwo\nthree\nfour";
        for flags in [
            vec!["-n2"],
            vec!["-c5"],
            vec!["-n", "-2"],
            vec!["-c", "-4"],
            vec!["-n", "-1"],
        ] {
            let options = parse(&flags);
            let mut slow: Vec<u8> = Vec::new();
            emit(&mut OneByteAtATime(input), &mut slow, &options).unwrap();
            let mut fast: Vec<u8> = Vec::new();
            emit(&mut &input[..], &mut fast, &options).unwrap();
            assert_eq!(slow, fast, "{flags:?}");
        }
    }

    /// The streaming requirement, stated as a test: a source with no end must
    /// not prevent `head -n1` from finishing.
    #[test]
    fn a_bounded_head_never_reads_past_what_it_needs() {
        struct Endless {
            reads: usize,
        }
        impl Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.reads = self.reads.saturating_add(1);
                assert!(self.reads < 1000, "head kept reading an endless source");
                let pattern = b"y\n";
                let n = buf.len().min(pattern.len() * 512);
                for (at, slot) in buf.iter_mut().take(n).enumerate() {
                    *slot = pattern[at % pattern.len()];
                }
                Ok(n)
            }
        }
        let mut out: Vec<u8> = Vec::new();
        emit(&mut Endless { reads: 0 }, &mut out, &parse(&["-n1"])).unwrap();
        assert_eq!(out, b"y\n");
        let mut out: Vec<u8> = Vec::new();
        emit(&mut Endless { reads: 0 }, &mut out, &parse(&["-c3"])).unwrap();
        assert_eq!(out, b"y\ny");
    }

    #[test]
    fn help_and_version_end_parsing() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["--vers"])), Ok(Request::Version));
        // Even behind other options, and even with a bad operand after.
        assert_eq!(
            parse_args(&args(&["-v", "--help", "-x"])),
            Ok(Request::Help)
        );
        assert!(help_text().starts_with("Usage: head [OPTION]... [FILE]...\n"));
    }
}
