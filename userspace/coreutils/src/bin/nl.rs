//! `nl` — number the lines of files, honouring logical page sections.
//!
//! # What the parser this replaces could not say
//!
//! The previous front end knew `-b` and `-w`, took the first character of `-b`'s
//! argument without checking it, fell back to width 6 for any `-w` it could not
//! parse, and **silently ignored every other flag** — so `nl -n rz f` numbered
//! with the default format and said nothing, and `nl -w x f` used 6. It also
//! defaulted `-b` to `a`, where GNU's default is `t`, so *every* invocation with
//! blank lines in the input disagreed with GNU before any option was typed.
//!
//! Missing entirely: `-d`, `-f`, `-h`, `-i`, `-l`, `-n`, `-p`, `-s`, `-v`, every
//! long option, and — the largest of them — the whole notion of a **section**.
//! GNU `nl` splits its input into header, body and footer on delimiter lines, and
//! numbers each with its own style and its own counter. None of that existed.
//!
//! # Three things here that no reading of `--help` suggests
//!
//! **`-h` is not `--help`.** In `nl` it is `--header-numbering` and it takes an
//! argument, so `nl -h f` numbers standard input with `f`'s first byte as the
//! header style. `nl` is the reason a converted utility must copy its short
//! option string from upstream rather than assume the usual letters.
//!
//! **`-d` overwrites the delimiter in place, and the old tail survives.**
//! Upstream keeps the delimiter in a two-byte static buffer and, for an argument
//! of one or two characters, copies the bytes over the front of whatever is
//! there. A one-character `-d` therefore leaves byte 2 alone — which is where
//! `--help`'s "a missing second character implies `:`" comes from, since the
//! initial contents are `\:`. But it is not a rule about `:`; it is a rule about
//! the previous value, and it is observable:
//!
//! | command | delimiter | header line |
//! |---|---|---|
//! | `nl -d x` | `x:` | `x:x:x:` |
//! | `nl -d abc` | `abc` (GNU extension: 3+ bytes replace outright) | `abcabcabc` |
//! | `nl -d abc -d x` | **`xbc`** | `xbcxbcxbc` |
//! | `nl -d ''` | empty — section matching off | none |
//!
//! **Bad option arguments accumulate; bad numbers do not.** An unusable `-b`,
//! `-f`, `-h` or `-n` records a diagnostic and parsing continues, so
//! `nl -bX -nY f` prints two sentences and exactly one `Try 'nl --help'` at the
//! end. An unusable `-v`, `-i`, `-l` or `-w`, and an uncompilable `-bp` regular
//! expression, exit immediately and carry no referral at all.
//!
//! # Reference
//!
//! Measured against glibc's `nl` (GNU coreutils 9.4) through WSL, not against the
//! `nl` on this host's `PATH`, which is MSYS2's. Where measurement could not
//! settle a rule — the `-d` aliasing above, and the blank-line counter surviving
//! a section switch — `coreutils-9.4/src/nl.c` did.
//! `scripts/nl-diff.sh` is the executable form of every claim in this file.

use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{quote, quotef_os};
use coreutils::xnum;
use ere::{Regex, bre};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::process::ExitCode;

/// `nl` exits 1 on a bad command line, like almost every utility here.
/// Measured: `nl --zzz-bogus; echo $?`.
const NL: Program = Program::new("nl", 1);

/// The option table, in upstream's `longopts[]` order rather than alphabetical
/// order, because `getopt_long` lists an ambiguous prefix's candidates in
/// declaration order and that list is output. Measured with `nl --=x`, whose
/// empty prefix matches everything:
///
/// ```text
/// nl: option '--=x' is ambiguous; possibilities: '--header-numbering'
/// '--body-numbering' '--footer-numbering' '--starting-line-number'
/// '--line-increment' '--no-renumber' '--join-blank-lines'
/// '--number-separator' '--number-width' '--number-format'
/// '--section-delimiter' '--help' '--version'
/// ```
///
/// Note `--number-separator`, `--number-width` and `--number-format` are three
/// separate options sharing a five-letter prefix with `--no-renumber`, so `--n`
/// is ambiguous four ways and `--num` three.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("header-numbering", Takes::Required),
    ("body-numbering", Takes::Required),
    ("footer-numbering", Takes::Required),
    ("starting-line-number", Takes::Required),
    ("line-increment", Takes::Required),
    ("no-renumber", Takes::Nothing),
    ("join-blank-lines", Takes::Required),
    ("number-separator", Takes::Required),
    ("number-width", Takes::Required),
    ("number-format", Takes::Required),
    ("section-delimiter", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// How one class of line is numbered.
///
/// Upstream stores this as the raw argument string and switches on its first
/// byte, which is why `nl -baXYZ` is accepted: only the `a` is ever looked at.
/// That is reproduced in [`build_type_arg`] rather than in the type, so that
/// everything downstream sees a decided answer.
enum Style {
    /// `a` — number every line.
    All,
    /// `t` — number only lines that are not empty. GNU's default for the body.
    NonEmpty,
    /// `n` — number nothing.
    Nothing,
    /// `pBRE` — number lines matching a basic regular expression.
    ///
    /// `None` is the *empty* expression, which `nl -bp` alone selects and which
    /// matches at every position, so every line is numbered. It is a variant
    /// rather than a compiled regex because `ere` refuses an empty pattern on
    /// purpose — bash's `[[ x =~ "" ]]` is status 2, not a match — whereas
    /// glibc's `re_compile_pattern` under `RE_SYNTAX_POSIX_BASIC`, which is what
    /// `nl` uses, accepts it. Measured: `printf 'a\n\n' | nl -bp` numbers both
    /// lines. Note this is *not* the same as [`Style::All`], which additionally
    /// takes part in `-l` blank-line joining.
    Matching(Option<Box<Regex>>),
}

/// `-n`: where the number sits in its field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    /// `ln` — left justified, no leading zeros.
    Left,
    /// `rn` — right justified, no leading zeros. The default.
    Right,
    /// `rz` — right justified, leading zeros.
    RightZero,
}

/// What [`check_section`] decided a line is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    Header,
    Body,
    Footer,
    Text,
}

/// Everything the option loop settles.
struct Options {
    header: Style,
    body: Style,
    footer: Style,
    /// `-v`. Signed: `nl -v -3` counts up from minus three, which is why this is
    /// `i64` (upstream's `intmax_t`) rather than a width.
    starting_line_number: i64,
    /// `-i`. Also signed, and zero is legal — `nl -i0` gives every line the same
    /// number.
    increment: i64,
    /// `-p`: keep counting across a section boundary instead of restarting.
    no_renumber: bool,
    /// `-l`: how many consecutive empty lines count as one. At least 1, and only
    /// consulted for [`Style::All`].
    blank_join: i64,
    /// `-s`. Bytes, not text: it is `fputs`ed and its *byte* length is what pads
    /// an unnumbered line.
    separator: Vec<u8>,
    /// `-w`. At least 1, at most `i32::MAX`; a number too wide for it is not
    /// truncated, exactly as `printf("%*jd")` does not truncate.
    width: i32,
    format: Format,
    /// `-d`, already expanded: the *one* delimiter. The header, body and footer
    /// delimiter lines are this repeated three, two and one times.
    section_delimiter: Vec<u8>,
}

impl Default for Options {
    /// GNU's own defaults, which `--help` states as
    /// `-bt -d'\:' -fn -hn -i1 -l1 -n'rn' -s<TAB> -v1 -w6`.
    ///
    /// Note `body` is [`Style::NonEmpty`], not `All`.
    fn default() -> Self {
        Options {
            header: Style::Nothing,
            body: Style::NonEmpty,
            footer: Style::Nothing,
            starting_line_number: 1,
            increment: 1,
            no_renumber: false,
            blank_join: 1,
            separator: b"\t".to_vec(),
            width: 6,
            format: Format::Right,
            section_delimiter: b"\\:".to_vec(),
        }
    }
}

/// What the command line asked for.
enum Request {
    Run(Options, Vec<OsString>),
    Help,
    Version,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("nl (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, files)) => run(&options, &files),
        Err(e) => {
            // The message may be several lines: the deferred diagnostics are
            // joined into it, and only the first carries a `nl: ` prefix of its
            // own from here — the rest embed theirs. See `Deferred::into_error`.
            eprintln!("nl: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs naming the
/// GNU project's own bug addresses.
fn help_text() -> String {
    "\
Usage: nl [OPTION]... [FILE]...
Write each FILE to standard output, with line numbers added.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -b, --body-numbering=STYLE      use STYLE for numbering body lines
  -d, --section-delimiter=CC      use CC for logical page delimiters
  -f, --footer-numbering=STYLE    use STYLE for numbering footer lines
  -h, --header-numbering=STYLE    use STYLE for numbering header lines
  -i, --line-increment=NUMBER     line number increment at each line
  -l, --join-blank-lines=NUMBER   group of NUMBER empty lines counted as one
  -n, --number-format=FORMAT      insert line numbers according to FORMAT
  -p, --no-renumber               do not reset line numbers for each section
  -s, --number-separator=STRING   add STRING after (possible) line number
  -v, --starting-line-number=NUMBER  first line number for each section
  -w, --number-width=NUMBER       use NUMBER columns for line numbers
      --help        display this help and exit
      --version     output version information and exit

Default options are: -bt -d'\\:' -fn -hn -i1 -l1 -n'rn' -s<TAB> -v1 -w6

CC are two delimiter characters used to construct logical page delimiters;
a missing second character implies ':'.  As a GNU extension one can specify
more than two characters, and also specifying the empty string (-d '')
disables section matching.

STYLE is one of:

  a      number all lines
  t      number only nonempty lines
  n      number no lines
  pBRE   number only lines that contain a match for the basic regular
         expression, BRE

FORMAT is one of:

  ln     left justified, no leading zeros
  rn     right justified, no leading zeros
  rz     right justified, leading zeros

"
    .to_string()
}

/// Diagnostics that do not stop parsing.
///
/// Upstream's four "invalid ... style/format" messages set `ok = false` and let
/// the loop run on, so a command line with three of them prints three sentences
/// and then **one** `Try 'nl --help'`. Every other utility converted so far
/// returns on the first error, which would print one sentence and drop the rest.
/// Measured:
///
/// **getopt's own diagnostics accumulate the same way**, and for the same
/// reason: `getopt_long` prints the sentence itself and returns `'?'`, and
/// `nl`'s `default:` case only clears `ok`. So a bad option and a bad style are
/// reported together, in the order they were typed —
/// `nl -Z -bX` prints the `-Z` sentence then the `-bX` one, `nl -bX -Z` the
/// reverse — under one referral. This is why [`getopt::Error`] keeps its
/// sentence and its referral apart.
///
/// ```text
/// $ nl -bX -nY -fQ f
/// nl: invalid body numbering style: 'X'
/// nl: invalid line numbering format: 'Y'
/// nl: invalid footer numbering style: 'Q'
/// Try 'nl --help' for more information.
/// ```
///
/// A *fatal* error (a bad number, an uncompilable regex) still exits at once,
/// but must print the deferred ones first — hence [`Deferred::fatal`] rather
/// than a bare `return Err`.
#[derive(Default)]
struct Deferred(Vec<String>);

impl Deferred {
    fn push(&mut self, message: String) {
        self.0.push(message);
    }

    /// A getopt diagnostic, kept for the same batch as `nl`'s own.
    ///
    /// Only the sentence is taken: the referral belongs to the single
    /// `usage (EXIT_FAILURE)` at the end, not to each diagnostic.
    fn push_getopt(&mut self, error: &getopt::Error) {
        self.0.push(error.sentence.clone());
    }

    /// Prefix all but the first, since `main` supplies exactly one `nl: `.
    fn joined(&self) -> String {
        self.0.join("\nnl: ")
    }

    /// The accumulated diagnostics as one usage error, or `Ok(())` if there
    /// were none. This is where the single trailing referral comes from.
    fn into_error(self) -> Result<(), getopt::Error> {
        if self.0.is_empty() {
            return Ok(());
        }
        Err(NL.usage_referring(self.joined()))
    }

    /// A fatal diagnostic, printed after whatever was already deferred.
    ///
    /// These carry no referral: upstream reaches them through
    /// `error (EXIT_FAILURE, ...)`, which exits without calling `usage`.
    fn fatal(&self, message: String) -> getopt::Error {
        let body = if self.0.is_empty() {
            message
        } else {
            format!("{}\nnl: {message}", self.joined())
        };
        getopt::Error {
            sentence: body,
            referral: None,
            status: 1,
        }
    }
}

/// Parse the whole command line.
///
/// # Errors
///
/// Any getopt diagnostic; `nl`'s four deferred ones, batched; and its fatal
/// ones — a number outside its range, or a regular expression that will not
/// compile.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut deferred = Deferred::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

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
            // A lone `-` names standard input, which is an operand.
            files.push(arg.clone());
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut options, &mut deferred)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut options, &mut deferred)?;
        }
    }

    deferred.into_error()?;
    Ok(Request::Run(options, files))
}

/// One `--name` or `--name=value`, returning a [`Request`] for the two options
/// that end parsing and `None` when it only set something.
fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
    deferred: &mut Deferred,
) -> Result<Option<Request>, getopt::Error> {
    let body = bytes.get(2..).unwrap_or_default();
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so one that is not UTF-8 matches none of them
    // and takes the unrecognised path rather than erroring differently.
    let Ok(typed) = std::str::from_utf8(typed) else {
        deferred.push_getopt(&NL.unrecognized_option(bytes));
        return Ok(None);
    };
    // Each of these three is one `getopt_long` sentence and a `'?'` return: the
    // option is dropped and the loop goes on to the next argument.
    let (name, takes) = match NL.resolve_long(typed, bytes, LONG_OPTIONS) {
        Ok(resolved) => resolved,
        Err(e) => {
            deferred.push_getopt(&e);
            return Ok(None);
        }
    };

    if takes == Takes::Nothing && inline.is_some() {
        deferred.push_getopt(&NL.long_unwanted_argument(name));
        return Ok(None);
    }
    let value: Vec<u8> = match (takes, inline) {
        (_, Some(v)) => v.to_vec(),
        (Takes::Required, None) => {
            let Some(next) = args.get(*i).cloned() else {
                deferred.push_getopt(&NL.long_missing_argument(name));
                return Ok(None);
            };
            *i = i.saturating_add(1);
            arg_bytes(&next)
        }
        (_, None) => Vec::new(),
    };

    match name {
        "header-numbering" => set_style(&value, "header", &mut options.header, deferred)?,
        "body-numbering" => set_style(&value, "body", &mut options.body, deferred)?,
        "footer-numbering" => set_style(&value, "footer", &mut options.footer, deferred)?,
        "starting-line-number" => {
            options.starting_line_number = xdectoimax(
                &value,
                i64::MIN,
                i64::MAX,
                "invalid starting line number",
                deferred,
            )?;
        }
        "line-increment" => {
            options.increment = xdectoimax(
                &value,
                i64::MIN,
                i64::MAX,
                "invalid line number increment",
                deferred,
            )?;
        }
        "no-renumber" => options.no_renumber = true,
        "join-blank-lines" => {
            options.blank_join = xdectoimax(
                &value,
                1,
                i64::MAX,
                "invalid line number of blank lines",
                deferred,
            )?;
        }
        "number-separator" => options.separator = value,
        "number-width" => {
            let w = xdectoimax(
                &value,
                1,
                i64::from(i32::MAX),
                "invalid line number field width",
                deferred,
            )?;
            options.width = i32::try_from(w).unwrap_or(i32::MAX);
        }
        "number-format" => set_format(&value, &mut options.format, deferred),
        "section-delimiter" => set_section_delimiter(&value, &mut options.section_delimiter),
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// One `-abc` cluster, against upstream's short option string
/// `"h:b:f:v:i:pl:s:w:n:d:"`.
///
/// Every letter but `p` takes an argument, `-h` included — `nl` has no short
/// `--help`.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    options: &mut Options,
    deferred: &mut Deferred,
) -> Result<(), getopt::Error> {
    let body = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
    // `invalid option -- 'é'`, an option nobody typed.
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        if c == b'p' {
            options.no_renumber = true;
            continue;
        }
        if !matches!(
            c,
            b'h' | b'b' | b'f' | b'v' | b'i' | b'l' | b's' | b'w' | b'n' | b'd'
        ) {
            // getopt reports the letter and carries on with the rest of the
            // cluster, so `nl -Zb a` still sets the body style.
            deferred.push_getopt(&NL.invalid_option(c));
            continue;
        }
        // The value is the rest of the cluster if there is one, else the next
        // argument.
        let value: Vec<u8> = match body.get(at..) {
            Some(rest) if !rest.is_empty() => {
                at = body.len();
                rest.to_vec()
            }
            _ => {
                let Some(next) = args.get(*i).cloned() else {
                    deferred.push_getopt(&NL.short_missing_argument(c));
                    break;
                };
                *i = i.saturating_add(1);
                arg_bytes(&next)
            }
        };
        match c {
            b'h' => set_style(&value, "header", &mut options.header, deferred)?,
            b'b' => set_style(&value, "body", &mut options.body, deferred)?,
            b'f' => set_style(&value, "footer", &mut options.footer, deferred)?,
            b'v' => {
                options.starting_line_number = xdectoimax(
                    &value,
                    i64::MIN,
                    i64::MAX,
                    "invalid starting line number",
                    deferred,
                )?;
            }
            b'i' => {
                options.increment = xdectoimax(
                    &value,
                    i64::MIN,
                    i64::MAX,
                    "invalid line number increment",
                    deferred,
                )?;
            }
            b'l' => {
                options.blank_join = xdectoimax(
                    &value,
                    1,
                    i64::MAX,
                    "invalid line number of blank lines",
                    deferred,
                )?;
            }
            b's' => options.separator = value,
            b'w' => {
                let w = xdectoimax(
                    &value,
                    1,
                    i64::from(i32::MAX),
                    "invalid line number field width",
                    deferred,
                )?;
                options.width = i32::try_from(w).unwrap_or(i32::MAX);
            }
            b'n' => set_format(&value, &mut options.format, deferred),
            b'd' => set_section_delimiter(&value, &mut options.section_delimiter),
            // Checked above.
            _ => {}
        }
    }
    Ok(())
}

/// Upstream's `build_type_arg`: decide a numbering style from its argument.
///
/// **Only the first byte is examined.** `nl -baXYZ` is accepted and means `a`,
/// because upstream switches on `*optarg` and then stores the whole pointer,
/// never looking past it again. An unrecognised first byte is deferred, not
/// fatal; a `p` whose regular expression will not compile is fatal.
fn set_style(
    value: &[u8],
    which: &str,
    slot: &mut Style,
    deferred: &mut Deferred,
) -> Result<(), getopt::Error> {
    match value.first() {
        Some(b'a') => *slot = Style::All,
        Some(b't') => *slot = Style::NonEmpty,
        Some(b'n') => *slot = Style::Nothing,
        Some(b'p') => {
            let pattern = value.get(1..).unwrap_or_default();
            *slot = Style::Matching(if pattern.is_empty() {
                // See `Style::Matching`: the empty expression is legal here and
                // matches everything, but `ere` will not compile it.
                None
            } else {
                // POSIX *basic* expressions, upstream's `RE_SYNTAX_POSIX_BASIC`,
                // backreferences included. What still differs from glibc is only
                // the wording of a compile error, which `scripts/nl-diff.sh`
                // marks xfail rather than pretending the two agree.
                let compiled = bre::compile(pattern, false)
                    .map_err(|e| deferred.fatal(String::from_utf8_lossy(&e.0).into_owned()))?;
                Some(Box::new(compiled))
            });
        }
        _ => deferred.push(format!("invalid {which} numbering style: {}", quote(value))),
    }
    Ok(())
}

/// `-n`. Exact spellings only: upstream compares with `STREQ`, not `argmatch`,
/// so there is no abbreviation and no "ambiguous" message — `nl -n l` is simply
/// invalid.
fn set_format(value: &[u8], slot: &mut Format, deferred: &mut Deferred) {
    match value {
        b"ln" => *slot = Format::Left,
        b"rn" => *slot = Format::Right,
        b"rz" => *slot = Format::RightZero,
        _ => deferred.push(format!("invalid line numbering format: {}", quote(value))),
    }
}

/// `-d`, with upstream's two paths and the aliasing between them.
///
/// An argument of one or two bytes is copied **over the front of the current
/// delimiter**, leaving any further bytes in place; anything else (including the
/// empty string) replaces it outright. The module doc has the table; the short
/// version is that `-d abc -d x` yields `xbc`, not `x:`.
///
/// Upstream does this by writing through a `char *` that may point either at a
/// static two-byte buffer or into `argv`, which is why the surviving tail can be
/// something the user typed. Reproduced with a `Vec` rather than a pointer, and
/// the one place C would run off the end — a one-byte argument after `-d ''`
/// left the buffer empty — grows it instead, which is what glibc was measured
/// doing anyway (`nl -d '' -d x` keeps section matching disabled, because the
/// result is one byte long and [`check_section`] needs two).
fn set_section_delimiter(value: &[u8], slot: &mut Vec<u8>) {
    if value.len() == 1 || value.len() == 2 {
        for (at, &b) in value.iter().enumerate() {
            match slot.get_mut(at) {
                Some(slot_byte) => *slot_byte = b,
                None => slot.push(b),
            }
        }
    } else {
        *slot = value.to_vec();
    }
}

/// gnulib's `xdectoimax` with an empty suffix list: a decimal integer, and
/// nothing else.
///
/// Leading whitespace and a leading `+` or `-` are accepted, trailing anything
/// is not, and there are no multiplier suffixes — `nl -v 1K` is an error though
/// `head -n 1K` is not, because `nl` passes `""` where `head` passes `"bkKmM…"`.
///
/// Out of range produces one of *two* sentences, each the `strerror` of the
/// errno gnulib sets, and which one is decided by a heuristic on the **value**
/// rather than on the limit that was violated:
/// `errno = (tnum < INT_MIN / 2 || INT_MAX / 2 < tnum) ? EOVERFLOW : ERANGE`.
/// So a modest out-of-range value reads as `ERANGE` no matter which end it fell
/// off, and a wild one reads as `EOVERFLOW` even when it fell off the *near*
/// end:
///
/// | argument | message tail |
/// |---|---|
/// | `-w 0`, `-w -1`, `-l -5` (out of range but small) | `: Numerical result out of range` |
/// | `-w 2147483648` (above the caller's ceiling) | `: Value too large for defined data type` |
/// | `-l -3000000000` (below the floor, but past `INT_MIN / 2`) | `: Value too large for defined data type` |
/// | `-v 99999999999999999999` (beyond `intmax_t`) | `: Value too large for defined data type` |
/// | `-v abc` (not a number at all) | none |
///
/// That third row is why this is now a two-line adapter over
/// [`coreutils::xnum`] rather than the parser it used to be. The hand-written
/// version encoded an *older* gnulib rule — `errno = min <= tnum ? EOVERFLOW :
/// ERANGE` — which agrees with 9.4's on every case the harness happened to
/// carry and disagrees on `nl -l -3000000000`, where it said `Numerical result
/// out of range` and GNU says `Value too large`. Two partial copies of one
/// gnulib function disagreeing in exactly the place no one thought to test is
/// the reason `xnum` exists.
fn xdectoimax(
    value: &[u8],
    min: i64,
    max: i64,
    what: &str,
    deferred: &Deferred,
) -> Result<i64, getopt::Error> {
    // `Some(b"")` — an empty suffix *list*, which is not the same as `None`:
    // `None` is gnulib's `NULL`, meaning "accept and ignore any trailing text".
    xnum::xdectoimax(value, min, max, Some(b""), what).map_err(|m| deferred.fatal(m))
}

/// A line reader that keeps the terminator, the way gnulib's `readlinebuffer`
/// does.
///
/// Keeping it matters twice over here: [`check_section`] measures the line
/// *without* it (`length - 1`), and [`Numberer::text`] decides "is this line
/// empty?" by asking whether the length is greater than 1 — so a reader that
/// stripped terminators would need both facts restated, and would get the
/// unterminated final line wrong. gnulib appends a newline at an unterminated
/// end of file, so `printf 'a\nb' | nl` prints `b` terminated, and so does this.
struct Reader<R: BufRead> {
    inner: R,
    /// The first read error, reported once after the input runs out, exactly as
    /// upstream's single `ferror` check after the loop does.
    failure: Option<io::Error>,
}

impl<R: BufRead> Reader<R> {
    fn new(inner: R) -> Self {
        Reader {
            inner,
            failure: None,
        }
    }

    /// The next line including its `\n`, or `None` at end of input.
    fn next_line(&mut self, buf: &mut Vec<u8>) -> bool {
        buf.clear();
        match self.inner.read_until(b'\n', buf) {
            Ok(0) => false,
            Ok(_) => {
                if buf.last() != Some(&b'\n') {
                    buf.push(b'\n');
                }
                true
            }
            Err(e) => {
                if self.failure.is_none() {
                    self.failure = Some(e);
                }
                false
            }
        }
    }
}

/// Which section a line announces, or [`Section::Text`] if it announces none.
///
/// Upstream's fast rejection is the first clause: a line shorter than two bytes,
/// or a delimiter shorter than two bytes, or a line whose first two bytes are
/// not the delimiter's, is text without further comparison. The second of those
/// is how `-d ''` switches section matching off, and it is the only reason the
/// empty delimiter needs no special case anywhere else.
///
/// `line` still carries its terminator; the comparison is against everything
/// before it.
fn check_section(line: &[u8], delimiter: &[u8]) -> Section {
    let body = line.split_last().map_or(line, |(_, rest)| rest);
    if body.len() < 2 || delimiter.len() < 2 || body.get(..2) != delimiter.get(..2) {
        return Section::Text;
    }
    let repeats = |n: usize| body.len() == delimiter.len().saturating_mul(n);
    if repeats(3) && body.chunks(delimiter.len()).all(|c| c == delimiter) {
        return Section::Header;
    }
    if repeats(2) && body.chunks(delimiter.len()).all(|c| c == delimiter) {
        return Section::Body;
    }
    if repeats(1) && body == delimiter {
        return Section::Footer;
    }
    Section::Text
}

/// The numbering state machine: everything that survives from one line to the
/// next, and from one *file* to the next.
///
/// The lifetime of each field is upstream's, and two of them are surprising:
///
/// - **`line_no` is not reset per file.** `nl a b` numbers `b` continuing from
///   `a`. Only a section boundary resets it, and only when `-p` was not given.
/// - **`blank_lines` is a function-level `static` in upstream and survives a
///   section switch too**, which is visible with `-ba -l3` when the run of empty
///   lines straddles a delimiter line: the count carries across, so the third
///   empty line gets a number even though the first two were in the previous
///   section. Measured, and reproduced deliberately.
struct Numberer<'o> {
    options: &'o Options,
    current: &'o Style,
    line_no: i64,
    /// Set once the counter has run past `i64`. Checked *before* the next number
    /// is printed, so the last representable number is printed and only then
    /// does `nl` refuse.
    overflowed: bool,
    blank_lines: i64,
}

impl<'o> Numberer<'o> {
    fn new(options: &'o Options) -> Self {
        Numberer {
            options,
            current: &options.body,
            line_no: options.starting_line_number,
            overflowed: false,
            blank_lines: 0,
        }
    }

    /// A delimiter line: switch style, maybe restart the counter, and emit the
    /// blank line that upstream's `putchar ('\n')` emits.
    fn section(&mut self, section: Section, out: &mut impl Write) -> io::Result<()> {
        self.current = match section {
            Section::Header => &self.options.header,
            Section::Body => &self.options.body,
            Section::Footer | Section::Text => &self.options.footer,
        };
        if !self.options.no_renumber {
            self.line_no = self.options.starting_line_number;
            self.overflowed = false;
        }
        out.write_all(b"\n")
    }

    /// An ordinary line: decide whether it is numbered, emit the number or the
    /// blank field that stands in for it, then the line itself.
    fn text(&mut self, line: &[u8], out: &mut impl Write) -> io::Result<bool> {
        let numbered = match self.current {
            Style::All => {
                if self.options.blank_join > 1 {
                    if line.len() > 1 {
                        self.blank_lines = 0;
                        true
                    } else {
                        self.blank_lines = self.blank_lines.saturating_add(1);
                        if self.blank_lines >= self.options.blank_join {
                            self.blank_lines = 0;
                            true
                        } else {
                            false
                        }
                    }
                } else {
                    true
                }
            }
            Style::NonEmpty => line.len() > 1,
            Style::Nothing => false,
            // The search is over the line without its terminator, as upstream's
            // `re_search (…, line_buf.length - 1, …)` is: `-bp'x$'` must not be
            // asked to match against the newline.
            Style::Matching(None) => true,
            // An abandoned search (only a backreference pattern can cause one)
            // is not "did not match": numbering the line anyway would put a
            // wrong number on every line after it, so the run stops.
            Style::Matching(Some(re)) => re
                .find(line.split_last().map_or(line, |(_, rest)| rest))
                .map_err(|e| io::Error::other(e.to_string()))?
                .is_some(),
        };

        if numbered {
            if self.overflowed {
                return Ok(false);
            }
            self.write_lineno(out)?;
            match self.line_no.checked_add(self.options.increment) {
                Some(next) => self.line_no = next,
                None => self.overflowed = true,
            }
        } else {
            // The stand-in for a number is `width + strlen(separator)` spaces —
            // not the separator itself — so an unnumbered line lines up with a
            // numbered one whatever the separator is.
            let pad = usize::try_from(self.options.width)
                .unwrap_or(0)
                .saturating_add(self.options.separator.len());
            write_spaces(out, pad)?;
        }
        out.write_all(line)?;
        Ok(true)
    }

    /// `printf (lineno_format, lineno_width, line_no, separator_str)`, written
    /// out because the three formats differ in more than justification: `rz`
    /// zero-pads *after* the sign, so -5 in a field of four is `-005`.
    fn write_lineno(&self, out: &mut impl Write) -> io::Result<()> {
        let digits = self.line_no.unsigned_abs().to_string();
        let sign: &[u8] = if self.line_no < 0 { b"-" } else { b"" };
        let width = usize::try_from(self.options.width).unwrap_or(0);
        // A number wider than the field is not truncated, so the padding
        // saturates at zero rather than going negative.
        let pad = width.saturating_sub(digits.len().saturating_add(sign.len()));

        match self.options.format {
            Format::Left => {
                out.write_all(sign)?;
                out.write_all(digits.as_bytes())?;
                write_spaces(out, pad)?;
            }
            Format::Right => {
                write_spaces(out, pad)?;
                out.write_all(sign)?;
                out.write_all(digits.as_bytes())?;
            }
            Format::RightZero => {
                out.write_all(sign)?;
                write_bytes(out, b'0', pad)?;
                out.write_all(digits.as_bytes())?;
            }
        }
        out.write_all(&self.options.separator)
    }
}

/// Write `n` spaces without allocating `n` of them.
///
/// `-w` accepts up to `i32::MAX`, and upstream really does `xmalloc` a field
/// that wide; there is no reason to copy that particular decision when a fixed
/// chunk emits the same bytes.
fn write_spaces(out: &mut impl Write, n: usize) -> io::Result<()> {
    write_bytes(out, b' ', n)
}

fn write_bytes(out: &mut impl Write, byte: u8, n: usize) -> io::Result<()> {
    const CHUNK: usize = 256;
    let filler = [byte; CHUNK];
    let mut left = n;
    while left > 0 {
        let take = left.min(CHUNK);
        out.write_all(filler.get(..take).unwrap_or(&filler))?;
        left = left.saturating_sub(take);
    }
    Ok(())
}

/// Number every operand, continuing after a file that could not be read.
///
/// The counter and the current section carry across files, so this is one
/// [`Numberer`] rather than one per file.
fn run(options: &Options, files: &[OsString]) -> ExitCode {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut numberer = Numberer::new(options);
    let mut ok = true;

    let stdin_only = [OsString::from("-")];
    let operands: &[OsString] = if files.is_empty() { &stdin_only } else { files };

    for path in operands {
        let opened: io::Result<Box<dyn Read>> = if path == "-" {
            Ok(Box::new(io::stdin()))
        } else {
            File::open(path).map(|f| Box::new(f) as Box<dyn Read>)
        };
        let reader = match opened {
            Ok(r) => r,
            Err(e) => {
                // `quotef`, not `quote`: a file name in an I/O error is shell
                // quoting with the quotes elided when they are not needed.
                eprintln!("nl: {}: {}", quotef_os(path), errno_text(&e));
                ok = false;
                continue;
            }
        };
        match number_stream(BufReader::new(reader), &mut numberer, &mut out) {
            Ok(true) => {}
            Ok(false) => ok = false,
            Err(e) => {
                let _ = out.flush();
                eprintln!("nl: {}", errno_text(&e));
                return ExitCode::from(1);
            }
        }
        if let Err(e) = flush_error(&mut out) {
            eprintln!("nl: {}", errno_text(&e));
            return ExitCode::from(1);
        }
    }

    if let Err(e) = out.flush() {
        eprintln!("nl: {}", errno_text(&e));
        return ExitCode::from(1);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// One input. Returns false when the input could not be read to its end, which
/// makes `nl` exit 1 without abandoning the remaining operands.
fn number_stream(
    input: impl BufRead,
    numberer: &mut Numberer<'_>,
    out: &mut impl Write,
) -> io::Result<bool> {
    let mut reader = Reader::new(input);
    let mut line: Vec<u8> = Vec::new();
    while reader.next_line(&mut line) {
        match check_section(&line, &numberer.options.section_delimiter) {
            Section::Text => {
                if !numberer.text(&line, out)? {
                    // The counter ran past `intmax_t`. Upstream calls
                    // `error (EXIT_FAILURE, …)` from inside the print, so the
                    // lines already written stay written and nothing further is.
                    out.flush()?;
                    eprintln!("nl: line number overflow");
                    std::process::exit(1);
                }
            }
            section => numberer.section(section, out)?,
        }
    }
    match reader.failure {
        Some(e) => Err(e),
        None => Ok(true),
    }
}

/// Surface a write failure that `BufWriter` swallowed until now.
fn flush_error(out: &mut impl Write) -> io::Result<()> {
    out.flush()
}

/// glibc's `strerror` wording for the errors `nl` can print, which is what the
/// harness compares against. `io::Error`'s own `Display` adds an ` (os error N)`
/// tail that GNU does not print.
fn errno_text(e: &io::Error) -> String {
    match e.kind() {
        ErrorKind::NotFound => "No such file or directory".to_string(),
        ErrorKind::PermissionDenied => "Permission denied".to_string(),
        ErrorKind::IsADirectory => "Is a directory".to_string(),
        other => {
            let _ = other;
            let text = e.to_string();
            match text.find(" (os error ") {
                Some(at) => text.get(..at).unwrap_or(&text).to_string(),
                None => text,
            }
        }
    }
}

/// An argument's bytes, without going through `String`.
#[cfg(unix)]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    a.as_os_str().as_bytes().to_vec()
}

/// On the host build an `OsString` is UTF-16; the lossy conversion is confined
/// to the harness's platform and never runs on the target.
#[cfg(not(unix))]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    a.to_string_lossy().into_owned().into_bytes()
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

    fn parse(args: &[&str]) -> Result<Options, String> {
        let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
        match parse_args(&owned) {
            Ok(Request::Run(o, _)) => Ok(o),
            Ok(_) => Err("help or version".to_string()),
            Err(e) => Err(e.message()),
        }
    }

    /// The diagnostic a command line produces. `Options` holds a compiled
    /// regular expression and so is not `Debug`, which rules out `unwrap_err`.
    fn err(args: &[&str]) -> String {
        match parse(args) {
            Err(e) => e,
            Ok(_) => panic!("expected a diagnostic from {args:?}"),
        }
    }

    fn files_of(args: &[&str]) -> Vec<String> {
        let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
        match parse_args(&owned) {
            Ok(Request::Run(_, f)) => f.iter().map(|s| s.to_string_lossy().into_owned()).collect(),
            _ => panic!("expected a run"),
        }
    }

    fn number(args: &[&str], input: &[u8]) -> String {
        let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
        let Ok(Request::Run(options, _)) = parse_args(&owned) else {
            panic!("expected a run");
        };
        let mut numberer = Numberer::new(&options);
        let mut out: Vec<u8> = Vec::new();
        number_stream(input, &mut numberer, &mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn the_default_body_style_is_t_not_a() {
        assert_eq!(number(&[], b"a\n\nb\n"), "     1\ta\n       \n     2\tb\n");
    }

    #[test]
    fn style_a_numbers_empty_lines_too() {
        assert_eq!(
            number(&["-ba"], b"a\n\nb\n"),
            "     1\ta\n     2\t\n     3\tb\n"
        );
    }

    #[test]
    fn style_n_numbers_nothing_but_still_pads() {
        assert_eq!(number(&["-bn"], b"a\nb\n"), "       a\n       b\n");
    }

    #[test]
    fn only_the_first_byte_of_a_style_is_examined() {
        // Upstream switches on `*optarg` and never looks further, so this is
        // `-ba` with three bytes of decoration.
        assert_eq!(number(&["-baXYZ"], b"\n"), "     1\t\n");
    }

    #[test]
    fn an_unusable_style_names_which_of_the_three_it_was() {
        assert!(err(&["-bX"]).starts_with("invalid body numbering style: 'X'"));
        assert!(err(&["-hX"]).starts_with("invalid header numbering style: 'X'"));
        assert!(err(&["-fX"]).starts_with("invalid footer numbering style: 'X'"));
        assert!(err(&["-b", ""]).starts_with("invalid body numbering style: ''"));
    }

    #[test]
    fn bad_styles_accumulate_and_share_one_referral() {
        let e = err(&["-bX", "-nY", "-fQ"]);
        assert_eq!(
            e,
            "invalid body numbering style: 'X'\n\
             nl: invalid line numbering format: 'Y'\n\
             nl: invalid footer numbering style: 'Q'\n\
             Try 'nl --help' for more information."
        );
    }

    /// getopt's diagnostics join the same batch, in the order they were typed.
    ///
    /// Upstream's loop survives a `'?'` return exactly as it survives a bad
    /// style, so neither kind of message can hide the other, and the pair swaps
    /// order with the arguments.
    #[test]
    fn a_getopt_diagnostic_and_a_deferred_one_are_reported_together() {
        assert_eq!(
            err(&["-Z", "-bX"]),
            "invalid option -- 'Z'\n\
             nl: invalid body numbering style: 'X'\n\
             Try 'nl --help' for more information."
        );
        assert_eq!(
            err(&["-bX", "-Z"]),
            "invalid body numbering style: 'X'\n\
             nl: invalid option -- 'Z'\n\
             Try 'nl --help' for more information."
        );
        // Two of getopt's own, and the rest of a cluster still takes effect:
        // `b` here reads its argument from the next word.
        assert_eq!(
            err(&["-Zb", "a", "--zz"]),
            "invalid option -- 'Z'\n\
             nl: unrecognized option '--zz'\n\
             Try 'nl --help' for more information."
        );
        // A missing argument ends the cluster but not the parse.
        assert_eq!(
            err(&["-b"]),
            "option requires an argument -- 'b'\n\
             Try 'nl --help' for more information."
        );
    }

    #[test]
    fn a_fatal_number_prints_the_deferred_ones_first_and_does_not_refer() {
        let e = err(&["-bX", "-w0"]);
        assert_eq!(
            e,
            "invalid body numbering style: 'X'\n\
             nl: invalid line number field width: '0': Numerical result out of range"
        );
    }

    #[test]
    fn a_regex_style_numbers_only_matching_lines() {
        assert_eq!(
            number(&["-bp^foo"], b"foo\nbar\nfoobar\n"),
            "     1\tfoo\n       bar\n     2\tfoobar\n"
        );
    }

    #[test]
    fn a_regex_is_matched_against_the_line_without_its_terminator() {
        // `x$` must anchor at the end of the text, not before the newline that
        // the reader kept.
        assert_eq!(number(&["-bpx$"], b"ax\nxb\n"), "     1\tax\n       xb\n");
    }

    #[test]
    fn an_empty_regex_matches_every_line() {
        assert_eq!(number(&["-bp"], b"a\n\n"), "     1\ta\n     2\t\n");
    }

    #[test]
    fn the_three_number_formats_differ_in_more_than_justification() {
        assert_eq!(number(&["-ba", "-nln", "-w4"], b"a\n"), "1   \ta\n");
        assert_eq!(number(&["-ba", "-nrn", "-w4"], b"a\n"), "   1\ta\n");
        assert_eq!(number(&["-ba", "-nrz", "-w4"], b"a\n"), "0001\ta\n");
    }

    #[test]
    fn leading_zeros_go_after_the_sign_not_before_it() {
        assert_eq!(
            number(&["-ba", "-nrz", "-w4", "-v", "-5"], b"a\n"),
            "-005\ta\n"
        );
    }

    #[test]
    fn a_number_wider_than_the_field_is_not_truncated() {
        assert_eq!(number(&["-ba", "-w1"], b"a\n"), "1\ta\n");
        assert_eq!(number(&["-ba", "-w1", "-v", "1234"], b"a\n"), "1234\ta\n");
    }

    #[test]
    fn only_exact_format_spellings_are_accepted() {
        assert!(parse(&["-n", "l"]).is_err());
        assert!(parse(&["-n", "LN"]).is_err());
        assert!(parse(&["-n", "lnx"]).is_err());
        assert_eq!(parse(&["-n", "ln"]).unwrap().format, Format::Left);
    }

    #[test]
    fn an_unnumbered_line_is_padded_by_width_plus_the_separator_length() {
        // Two-byte separator, width 3: five spaces, not three plus a separator.
        assert_eq!(
            number(&["-bt", "-s--", "-w3"], b"x\n\ny\n"),
            "  1--x\n     \n  2--y\n"
        );
    }

    #[test]
    fn an_empty_separator_still_works() {
        assert_eq!(number(&["-ba", "-w1", "-s", ""], b"p\nq\n"), "1p\n2q\n");
    }

    #[test]
    fn sections_switch_style_and_restart_the_counter() {
        let input = b"\\:\\:\\:\nH1\n\\:\\:\nB1\nB2\n\\:\nF1\n\\:\\:\nB3\n";
        assert_eq!(
            number(&[], input),
            "\n       H1\n\n     1\tB1\n     2\tB2\n\n       F1\n\n     1\tB3\n"
        );
    }

    #[test]
    fn dash_p_keeps_the_counter_running_across_sections() {
        let input = b"\\:\\:\\:\nH1\n\\:\\:\nB1\nB2\n\\:\nF1\n\\:\\:\nB3\n";
        assert_eq!(
            number(&["-ba", "-p"], input),
            "\n       H1\n\n     1\tB1\n     2\tB2\n\n       F1\n\n     3\tB3\n"
        );
    }

    #[test]
    fn v_and_i_are_signed_and_i_may_be_zero() {
        assert_eq!(
            number(&["-ba", "-v", "-3"], b"a\nb\n"),
            "    -3\ta\n    -2\tb\n"
        );
        assert_eq!(
            number(&["-ba", "-i", "0"], b"a\nb\n"),
            "     1\ta\n     1\tb\n"
        );
        assert_eq!(
            number(&["-ba", "-i", "-2"], b"a\nb\n"),
            "     1\ta\n    -1\tb\n"
        );
    }

    #[test]
    fn blank_join_counts_runs_of_empty_lines_as_one() {
        assert_eq!(
            number(&["-ba", "-l3"], b"a\n\n\n\nb\n\n\nc\n"),
            "     1\ta\n       \n       \n     2\t\n     3\tb\n       \n       \n     4\tc\n"
        );
    }

    #[test]
    fn blank_join_survives_a_section_switch() {
        // Upstream's counter is a function-level `static`, so the two empty
        // lines before the delimiter and the two after it are one run of four:
        // the third of them takes a number. Reproduced deliberately.
        let input = b"a\n\n\\:\\:\n\n\nb\n";
        assert_eq!(
            number(&["-ba", "-l3"], input),
            "     1\ta\n       \n\n       \n     1\t\n     2\tb\n"
        );
    }

    #[test]
    fn blank_join_applies_only_to_style_a() {
        assert_eq!(
            number(&["-bt", "-l3"], b"a\n\n\n\nb\n"),
            "     1\ta\n       \n       \n       \n     2\tb\n"
        );
    }

    #[test]
    fn a_one_byte_delimiter_keeps_the_second_byte_of_the_old_one() {
        assert_eq!(parse(&["-d", "x"]).unwrap().section_delimiter, b"x:");
        assert_eq!(parse(&["-d", "xy"]).unwrap().section_delimiter, b"xy");
        assert_eq!(parse(&["-d", "abc"]).unwrap().section_delimiter, b"abc");
        // The aliasing: three bytes then one leaves the tail of the three.
        assert_eq!(
            parse(&["-d", "abc", "-d", "x"]).unwrap().section_delimiter,
            b"xbc"
        );
        assert_eq!(
            parse(&["-d", "abc", "-d", "yz"]).unwrap().section_delimiter,
            b"yzc"
        );
    }

    #[test]
    fn an_empty_delimiter_disables_section_matching() {
        let o = parse(&["-d", ""]).unwrap();
        assert!(o.section_delimiter.is_empty());
        assert_eq!(
            check_section(b"\\:\\:\\:\n", &o.section_delimiter),
            Section::Text
        );
        assert_eq!(
            number(&["-d", "", "-ba"], b"\\:\\:\\:\nH\n"),
            "     1\t\\:\\:\\:\n     2\tH\n"
        );
    }

    #[test]
    fn a_custom_delimiter_is_repeated_three_two_and_one_times() {
        let d = b"abc".to_vec();
        assert_eq!(check_section(b"abcabcabc\n", &d), Section::Header);
        assert_eq!(check_section(b"abcabc\n", &d), Section::Body);
        assert_eq!(check_section(b"abc\n", &d), Section::Footer);
        assert_eq!(check_section(b"abcabcabcabc\n", &d), Section::Text);
        assert_eq!(check_section(b"abcx\n", &d), Section::Text);
    }

    #[test]
    fn a_delimiter_line_must_match_exactly_not_merely_start_the_line() {
        let d = b"\\:".to_vec();
        assert_eq!(check_section(b"\\:x\n", &d), Section::Text);
        assert_eq!(check_section(b"\\:\n", &d), Section::Footer);
        assert_eq!(check_section(b"\n", &d), Section::Text);
    }

    #[test]
    fn an_unterminated_final_line_is_numbered_and_terminated() {
        assert_eq!(number(&["-ba"], b"a\nb"), "     1\ta\n     2\tb\n");
    }

    #[test]
    fn short_h_is_header_numbering_not_help() {
        // `-h` takes an argument, so the operand is consumed as the style.
        assert_eq!(files_of(&["-h", "a"]), Vec::<String>::new());
        assert!(matches!(parse(&["-h", "a"]).unwrap().header, Style::All));
    }

    #[test]
    fn xdectoimax_takes_whitespace_and_a_sign_but_no_suffix() {
        assert_eq!(parse(&["-v", " 5"]).unwrap().starting_line_number, 5);
        assert_eq!(parse(&["-v", "+5"]).unwrap().starting_line_number, 5);
        assert!(parse(&["-v", "1K"]).is_err());
        assert!(parse(&["-v", "0x10"]).is_err());
        assert!(parse(&["-v", "5x"]).is_err());
        assert!(parse(&["-v", ""]).is_err());
    }

    #[test]
    fn the_two_out_of_range_messages_split_by_magnitude_not_by_limit() {
        // A *small* out-of-range value says "out of range", whichever end it
        // fell off…
        assert_eq!(
            err(&["-w", "0"]),
            "invalid line number field width: '0': Numerical result out of range"
        );
        assert_eq!(
            err(&["-w", "-1"]),
            "invalid line number field width: '-1': Numerical result out of range"
        );
        assert_eq!(
            err(&["-l", "-5"]),
            "invalid line number of blank lines: '-5': Numerical result out of range"
        );
        // …but a value past `INT_MIN / 2` says "value too large" even though it
        // fell off the *floor*, because gnulib picks the sentence by the value
        // and not by the limit. This is the case the hand-written parser this
        // replaced got wrong: it said "out of range" here, and GNU does not.
        assert_eq!(
            err(&["-l", "-3000000000"]),
            "invalid line number of blank lines: '-3000000000': \
             Value too large for defined data type"
        );
        assert_eq!(
            err(&["-w", "-3000000000"]),
            "invalid line number field width: '-3000000000': \
             Value too large for defined data type"
        );
        // Above the ceiling says the same thing whether the ceiling is the
        // caller's `INT_MAX` or `intmax_t` itself.
        assert_eq!(
            err(&["-w", "2147483648"]),
            "invalid line number field width: '2147483648': \
             Value too large for defined data type"
        );
        assert_eq!(
            err(&["-v", "9223372036854775808"]),
            "invalid starting line number: '9223372036854775808': \
             Value too large for defined data type"
        );
        // A magnitude past `intmax_t` overflows the same way in either
        // direction: it never reaches the floor comparison.
        assert_eq!(
            err(&["-v", "-9223372036854775809"]),
            "invalid starting line number: '-9223372036854775809': \
             Value too large for defined data type"
        );
    }

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        assert_eq!(parse(&["--number-f=ln"]).unwrap().format, Format::Left);
        assert!(err(&["--n=ln"]).starts_with(
            "option '--n=ln' is ambiguous; possibilities: \
             '--no-renumber' '--number-separator' '--number-width' '--number-format'"
        ));
        assert!(err(&["--zz"]).starts_with("unrecognized option '--zz'"));
    }

    #[test]
    fn an_empty_prefix_lists_the_table_in_declaration_order() {
        let e = err(&["--=x"]);
        assert!(e.starts_with(
            "option '--=x' is ambiguous; possibilities: '--header-numbering' \
             '--body-numbering' '--footer-numbering' '--starting-line-number' \
             '--line-increment' '--no-renumber' '--join-blank-lines' \
             '--number-separator' '--number-width' '--number-format' \
             '--section-delimiter' '--help' '--version'"
        ));
    }

    #[test]
    fn double_dash_ends_options_and_a_lone_dash_is_an_operand() {
        assert_eq!(files_of(&["--", "-ba"]), vec!["-ba".to_string()]);
        assert_eq!(
            files_of(&["-", "f"]),
            vec!["-".to_string(), "f".to_string()]
        );
    }

    #[test]
    fn an_option_missing_its_argument_is_a_getopt_error() {
        assert!(err(&["-b"]).starts_with("option requires an argument -- 'b'"));
        assert!(
            err(&["--body-numbering"])
                .starts_with("option '--body-numbering' requires an argument")
        );
        assert!(err(&["-Z"]).starts_with("invalid option -- 'Z'"));
    }

    #[test]
    fn the_counter_continues_across_files_but_a_section_resets_it() {
        let options = Options {
            body: Style::All,
            ..Options::default()
        };
        let mut numberer = Numberer::new(&options);
        let mut out: Vec<u8> = Vec::new();
        number_stream(&b"a\nb\n"[..], &mut numberer, &mut out).unwrap();
        number_stream(&b"c\n"[..], &mut numberer, &mut out).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out),
            "     1\ta\n     2\tb\n     3\tc\n"
        );
    }

    #[test]
    fn non_utf8_input_passes_through_unchanged() {
        // Bytes, not text: `nl` must not decode its input, so an invalid UTF-8
        // sequence comes out exactly as it went in. Compared as bytes, because
        // the lossy conversion `number` uses would hide precisely this bug.
        let options = Options {
            body: Style::All,
            ..Options::default()
        };
        let mut numberer = Numberer::new(&options);
        let mut out: Vec<u8> = Vec::new();
        number_stream(&b"\xff\xfe\n\x80\n"[..], &mut numberer, &mut out).unwrap();
        assert_eq!(out, b"     1\t\xff\xfe\n     2\t\x80\n".to_vec());
    }
}
