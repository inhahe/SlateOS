//! split — split a file into pieces.
//!
//! `split FILE` writes `FILE` out as `xaa`, `xab`, … in pieces of a thousand
//! lines. The interesting part is everything else: four different ways to
//! decide where a piece ends, six spellings of "divide this into N", a suffix
//! alphabet that silently grows a digit when it runs out, and an option that
//! replaces the output files with a shell command.
//!
//! Everything below was measured against GNU coreutils 9.4 rather than read
//! off its `--help`, because the parts that matter are not documented anywhere
//! — the suffix-widening scheme in particular is described by no manual page
//! and is pure observed behaviour. Where the measurement left a rule
//! underdetermined, `coreutils-9.4/src/split.c` was read to settle it; the four
//! places that happened are noted below, and every one of them is a place where
//! a plausible reading of the observations was wrong.
//!
//! # The four ways a piece can end
//!
//! | Option | A piece ends when |
//! |---|---|
//! | `-l N` | N records have been written |
//! | `-b N` | N bytes have been written, mid-record if need be |
//! | `-C N` | adding the next *whole* record would pass N bytes |
//! | `-n …` | the input has been divided into a fixed number of pieces |
//!
//! `-C` is the one whose rule is not obvious. It packs whole records up to the
//! limit, and only when a *single* record is longer than the limit does it cut
//! one — and then it cuts at exactly N bytes, repeatedly, until the tail fits.
//! So `-C 4` on `aaaa\nbb\n` gives `aaaa` and then `\nbb\n`: the first piece is
//! four bytes of an over-long record, and the newline that ended that record
//! begins the second piece, where it is joined by a record that does fit.
//!
//! Note also, and this is upstream's own inconsistency reproduced rather than
//! tidied, that a bad `-C` argument is reported as an `invalid number of
//! **lines**` — `-C` shares `-l`'s diagnostic despite counting bytes.
//!
//! # `-n` and where its boundaries fall
//!
//! `-n` takes `N`, `K/N`, `l/N`, `l/K/N`, `r/N` or `r/K/N`. The `K/` forms
//! write the Kth piece to **standard output and create no files at all**,
//! which is why `--filter` refuses to combine with them — there is no file for
//! `$FILE` to name.
//!
//! - **`-n N`** divides by bytes. The remainder goes to the *first* `size % N`
//!   pieces, one byte each, so 10 bytes in 3 gives 4, 3, 3 — not 3, 3, 4.
//!   Partition *m* therefore ends at byte `m*(size/N) + min(m, size%N)`.
//! - **`-n l/N`** cuts the *same* partitions and then lets records overrun
//!   them. A record belongs to the partition its **first byte** is in, so a
//!   piece is "every record that started inside partition *m*" — which is why
//!   pieces can be larger or smaller than `size/N`, and why a record longer
//!   than a partition leaves that partition with nothing in it.
//!
//!   That last consequence is the one worth stating loudly, because the
//!   plausible guess is wrong: an overrun partition still gets a file, and
//!   that file is **empty and in sequence**, not skipped and not moved to the
//!   end. Three records in `-n l/5` gives record, record, *empty*, record,
//!   *empty* — not record, record, record, empty, empty. Guessing the latter
//!   is what sent this to `split.c`.
//!
//!   Concretely: the search for a piece's last separator begins at the
//!   partition's **last byte**, `m*(size/N) + min(m, size%N) - 1`, so a record
//!   ending exactly on a boundary stays on the near side of it. Computing the
//!   partition exactly matters — on a 51-byte file in 7 pieces the fifth
//!   boundary is byte 36, where the truncated `i * (size/N)` would put it at
//!   35, one byte the other side of a newline and so a whole line out.
//! - **`-n r/N`** ignores byte positions entirely and deals records out round
//!   robin: record *i* goes to piece *i mod N*.
//!
//! All three create every one of the N files even when the input ran out
//! early; `-e` is what suppresses the empty ones, and it suppresses the
//! *name* along with the file, so the pieces that do exist stay consecutively
//! named.
//!
//! # Suffixes, and the marker character that makes them grow
//!
//! With no `-a`, the suffix is two characters and **widens by itself** when it
//! runs out. The scheme is not "add a character": the *leading* character of
//! the alphabet's last letter is reserved as a marker, and each widening adds
//! one marker and one body character, so the name grows by two:
//!
//! | Alphabet | runs | then | then |
//! |---|---|---|---|
//! | `a…z` | `aa`…`yz` (650) | `zaaa`…`zyzz` (16 900) | `zzaaaa`… |
//! | `0…9` (`-d`) | `00`…`89` (90) | `9000`…`9899` (900) | `990000`… |
//! | `0…f` (`-x`) | `00`…`ef` (240) | `f000`…`feff` (3 840) | `ff0000`… |
//!
//! The reserved marker is why the first run stops at `yz` rather than `zz`:
//! `z…` has to stay unambiguous, or `zaaa` would sort into the middle of a
//! sequence that also contained a plain `za`.
//!
//! Widening is switched **off**, and the full range used instead, by any of:
//!
//! - an explicit `-a N` (so `-a 2` stops at `zz` and then fails);
//! - an explicit start value, `--numeric-suffixes=5` or `--hex-suffixes=5`
//!   (but not a bare `-d`/`-x`, which keeps widening) — because the names it
//!   generates are not consecutive, and a field that grew underneath them
//!   would put them out of sort order;
//! - any `-n` mode, where the number of pieces is known in advance and the
//!   suffix is instead *pre-sized* to fit: `-n 700` picks three characters up
//!   front and names the pieces `xaaa`…`xbax`.
//!
//! The second and third combine in a way that is not the obvious one. A start
//! value is added to `-n`'s count when sizing the field — so `-n 200
//! --numeric-suffixes=100` needs four digits — but **only when the start is
//! smaller than the count**. A larger start is left out of the sum entirely,
//! for the same sort-order reason: an arbitrary start would otherwise let one
//! run of `split` choose a wider field than another. So `-n 3
//! --numeric-suffixes=999` is not four digits wide; it is
//! `numerical suffix start value is too large for the suffix length`, because
//! the count alone justified only two. That rule is the second thing
//! `split.c` had to settle.
//!
//! # A start value is checked as text, not parsed as a number
//!
//! This is the third. `--numeric-suffixes=FROM` does not run `FROM` through a
//! number parser; it runs `strlen(FROM) != strspn(FROM, alphabet)`, and three
//! consequences follow that a parser would not produce:
//!
//! - **An empty value passes**, vacuously — `--numeric-suffixes=` is accepted
//!   and behaves like a bare `-d` that has nonetheless switched widening off.
//! - **The width check is on the text**, after leading zeros are stripped. So
//!   `--numeric-suffixes=007 -a 1` is fine and `--numeric-suffixes=70 -a 1` is
//!   `numerical suffix start value is too large for the suffix length`, even
//!   though 7 and 70 are both "one or two digits" to a parser.
//! - **The message names the base, not the option**: the same code says
//!   `invalid start value for numerical suffix` for `-d` and
//!   `… for hexadecimal suffix` for `-x`.
//!
//! # `-n` skips whitespace before it looks for `l/`
//!
//! The fourth. The blank-skipping belongs to `-n`'s own argument scan and
//! happens *before* the `l/` or `r/` prefix is looked for, not merely inside the
//! number scan underneath it — so `-n ' l/3'` is `l/3` and not a malformed byte
//! count. The set skipped is C's `isspace`, which includes the vertical tab that
//! Rust's `is_ascii_whitespace` leaves out.
//!
//! # Why the input is read whole
//!
//! `-n` needs the input's size before it can place a single boundary, and it
//! accepts standard input, where the size cannot be asked for. So at least one
//! mode has to buffer the whole input, and every mode does it here for the
//! same reason `csplit` does (`design-decisions.md` §335): one input path is
//! one set of bugs. See §336 for the tradeoff and for what would trigger
//! revisiting it.
//!
//! # Exit status
//!
//! 0 on success, 1 on any failure — except under `--filter`, where a command
//! that fails hands its own status back, so `--filter='exit 3'` exits 3.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf, quoteaf_os, quotef_os};
use coreutils::shell::shell;
use coreutils::xnum::{self, Status};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::{ExitCode, Stdio};

/// `split --zzz` exits 1, like almost everything that is not `ls`/`sort`/`grep`.
const SPLIT: Program = Program::new("split", 1);

/// The default suffix width, and the width `-a 0` and an unset `-a` both mean.
const DEFAULT_SUFFIX_LENGTH: usize = 2;

const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const DECIMAL: &[u8] = b"0123456789";
const HEXADECIMAL: &[u8] = b"0123456789abcdef";

/// The multiplier letters `-b` and `-C` take. `-l` and `-n` take **none** —
/// `split -l 1k` is an error, which is easy to get wrong by sharing one parser
/// across all four.
///
/// The trailing `0` is not a letter but gnulib's flag for "a second `B` or
/// `iB` suffix is allowed", which is what makes `1KB` a thousand and `1KiB` a
/// thousand and twenty-four.
const SIZE_SUFFIXES: &[u8] = b"bEGKkMmPQRTYZ0";

/// A refusal: the message to print after `split: `, and the status to exit
/// with.
///
/// The status is carried rather than assumed because `--filter` breaks the
/// otherwise-universal rule that a failure exits 1 — it exits with whatever
/// the command exited with.
#[derive(Debug)]
struct Fail {
    message: String,
    status: u8,
}

impl Fail {
    fn new(message: String) -> Self {
        Fail { message, status: 1 }
    }
}

impl From<getopt::Error> for Fail {
    fn from(e: getopt::Error) -> Self {
        Fail {
            message: e.message(),
            status: u8::try_from(e.status).unwrap_or(1),
        }
    }
}

/// Which rule decides where a piece ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// `-l N`
    Lines,
    /// `-b N`
    Bytes,
    /// `-C N`
    LineBytes,
    /// `-n N` / `-n K/N`
    ChunkBytes,
    /// `-n l/N` / `-n l/K/N`
    ChunkLines,
    /// `-n r/N` / `-n r/K/N`
    RoundRobin,
}

impl Kind {
    /// Whether the number of pieces is known before the input is read, which
    /// is what lets the suffix be pre-sized instead of grown.
    const fn is_chunked(self) -> bool {
        matches!(self, Kind::ChunkBytes | Kind::ChunkLines | Kind::RoundRobin)
    }
}

#[derive(Clone, Debug)]
struct Options {
    kind: Kind,
    /// `-l`/`-b`/`-C`'s count, or `-n`'s number of pieces.
    units: u64,
    /// `-n K/N`'s K: the single piece to write to standard output.
    piece: Option<u64>,
    /// `-a`'s width, or `None` for "choose one".
    suffix_length: Option<usize>,
    alphabet: &'static [u8],
    /// The start value as the user typed it, when `--numeric-suffixes=FROM` or
    /// `--hex-suffixes=FROM` gave one. Kept as text because its *length* is
    /// what the width check compares against, so `=005` and `=5` differ.
    start: Option<Vec<u8>>,
    additional: Vec<u8>,
    separator: u8,
    elide_empty: bool,
    verbose: bool,
    filter: Option<OsString>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            kind: Kind::Lines,
            units: 1000,
            piece: None,
            suffix_length: None,
            alphabet: ALPHA,
            start: None,
            additional: Vec::new(),
            separator: b'\n',
            elide_empty: false,
            verbose: false,
            filter: None,
        }
    }
}

/// What the command line asked for.
enum Request {
    /// Boxed because the other two variants carry nothing, and a 200-byte
    /// enum returned by value from the parser would be 200 bytes of stack
    /// moved for every `--help`.
    Run(Box<Options>, OsString, OsString),
    Help,
    Version,
}

/// GNU's option table, in GNU's declaration order, which is the order an
/// ambiguous abbreviation lists its candidates in.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Required),
    ("lines", Takes::Required),
    ("line-bytes", Takes::Required),
    ("number", Takes::Required),
    ("elide-empty-files", Takes::Nothing),
    ("unbuffered", Takes::Nothing),
    ("suffix-length", Takes::Required),
    ("additional-suffix", Takes::Required),
    ("numeric-suffixes", Takes::Optional),
    ("hex-suffixes", Takes::Optional),
    ("filter", Takes::Required),
    ("verbose", Takes::Nothing),
    ("separator", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[cfg(unix)]
fn arg_bytes(a: &OsStr) -> Vec<u8> {
    os_bytes(a).into_owned()
}

#[cfg(not(unix))]
fn arg_bytes(a: &OsStr) -> Vec<u8> {
    os_bytes(a).into_owned()
}

/// An `OsString` from bytes; the mirror of [`coreutils::quote::os_bytes`], and
/// lossy on a Windows host for the same reason.
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
            eprintln!("split: {}", e.message);
            return ExitCode::from(e.status);
        }
    };
    match request {
        Request::Help => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Request::Version => {
            println!("split (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Request::Run(options, file, prefix) => match run(&options, &file, &prefix) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("split: {}", e.message);
                ExitCode::from(e.status)
            }
        },
    }
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs that names
/// the GNU project's own bug addresses.
fn help_text() -> String {
    "\
Usage: split [OPTION]... [FILE [PREFIX]]
Output pieces of FILE to PREFIXaa, PREFIXab, ...;
default size is 1000 lines, and default PREFIX is 'x'.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -a, --suffix-length=N   generate suffixes of length N (default 2)
      --additional-suffix=SUFFIX  append an additional SUFFIX to file names
  -b, --bytes=SIZE        put SIZE bytes per output file
  -C, --line-bytes=SIZE   put at most SIZE bytes of records per output file
  -d                      use numeric suffixes starting at 0, not alphabetic
      --numeric-suffixes[=FROM]  same as -d, but allow setting the start value
  -x                      use hex suffixes starting at 0, not alphabetic
      --hex-suffixes[=FROM]  same as -x, but allow setting the start value
  -e, --elide-empty-files  do not generate empty output files with '-n'
      --filter=COMMAND    write to shell COMMAND; file name is $FILE
  -l, --lines=NUMBER      put NUMBER lines/records per output file
  -n, --number=CHUNKS     generate CHUNKS output files; see explanation below
  -t, --separator=SEP     use SEP instead of newline as the record separator;
                            '\\0' (zero) specifies the NUL character
  -u, --unbuffered        immediately copy input to output with '-n r/...'
      --verbose           print a diagnostic just before each
                            output file is opened
      --help        display this help and exit
      --version     output version information and exit

The SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y,R,Q (powers of 1024) or KB,MB,... (powers of 1000).
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

CHUNKS may be:
  N       split into N files based on size of input
  K/N     output Kth of N to stdout
  l/N     split into N files without splitting lines/records
  l/K/N   output Kth of N to stdout without splitting lines/records
  r/N     like 'l' but use round robin distribution
  r/K/N   likewise but only output Kth of N to stdout
"
    .to_string()
}

// ---------------------------------------------------------------- arguments

/// The mode-setting options, tracked separately from [`Options::kind`] so that
/// a *second* one can be refused. GNU refuses `-l 5 -l 6` as well as
/// `-l 5 -b 5`: the message is about splitting "in more than one way", and
/// naming the same way twice counts.
struct Parsed {
    options: Options,
    mode_set: bool,
    separator_set: Option<u8>,
    operands: Vec<OsString>,
}

fn parse_args(args: &[OsString]) -> Result<Request, Fail> {
    let mut state = Parsed {
        options: Options::default(),
        mode_set: false,
        separator_set: None,
        operands: Vec::new(),
    };
    let mut only_operands = false;
    let mut i = 0usize;

    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        if only_operands {
            state.operands.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand.
            state.operands.push(arg.clone());
        } else if is_obsolete_count(&bytes) {
            // `split -5 FILE`: the historical spelling of `-l 5`, still
            // accepted by GNU. It is not a getopt option — there is no `-5`
            // in the table — so it is recognised before the cluster loop.
            set_mode(&mut state, Kind::Lines)?;
            state.options.units = parse_units(
                bytes.get(1..).unwrap_or_default(),
                None,
                "invalid number of lines",
            )?;
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut state)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut state)?;
        }
    }

    let mut rest = state.operands.into_iter();
    let file = rest.next().unwrap_or_else(|| OsString::from("-"));
    let prefix = rest.next().unwrap_or_else(|| OsString::from("x"));
    if let Some(extra) = rest.next() {
        return Err(SPLIT
            .usage_referring(format!("extra operand {}", quote(&arg_bytes(&extra))))
            .into());
    }
    Ok(Request::Run(Box::new(state.options), file, prefix))
}

/// `-5`, `-1000`: a lone hyphen followed by nothing but digits.
fn is_obsolete_count(bytes: &[u8]) -> bool {
    match bytes.get(1..) {
        Some(digits) => !digits.is_empty() && digits.iter().all(u8::is_ascii_digit),
        None => false,
    }
}

fn set_mode(state: &mut Parsed, kind: Kind) -> Result<(), Fail> {
    if state.mode_set {
        return Err(SPLIT
            .usage_referring("cannot split in more than one way".to_string())
            .into());
    }
    state.mode_set = true;
    state.options.kind = kind;
    Ok(())
}

fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    state: &mut Parsed,
) -> Result<Option<Request>, Fail> {
    let body = bytes.get(2..).unwrap_or_default();
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    let typed = std::str::from_utf8(typed).map_err(|_| SPLIT.unrecognized_option(bytes))?;
    let (name, takes) = SPLIT.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(SPLIT.long_unwanted_argument(name).into());
    }
    let value: Option<OsString> = match (takes, inline) {
        (_, Some(v)) => Some(os_from_bytes(v)),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| SPLIT.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(next)
        }
        (_, None) => None,
    };
    let text = value.as_ref().map(|v| arg_bytes(v)).unwrap_or_default();

    match name {
        "bytes" => {
            set_mode(state, Kind::Bytes)?;
            state.options.units =
                parse_units(&text, Some(SIZE_SUFFIXES), "invalid number of bytes")?;
        }
        "lines" => {
            set_mode(state, Kind::Lines)?;
            state.options.units = parse_units(&text, None, "invalid number of lines")?;
        }
        "line-bytes" => {
            set_mode(state, Kind::LineBytes)?;
            state.options.units =
                parse_units(&text, Some(SIZE_SUFFIXES), "invalid number of lines")?;
        }
        "number" => parse_number(&text, state)?,
        "elide-empty-files" => state.options.elide_empty = true,
        // `-u` promises that `-n r/…` copies through without buffering. This
        // implementation reads the input whole, so there is nothing to turn
        // off; the option is accepted because refusing it would break scripts
        // over a difference they cannot observe in the output.
        "unbuffered" => {}
        "suffix-length" => state.options.suffix_length = Some(parse_suffix_length(&text)?),
        "additional-suffix" => {
            if text.contains(&b'/') {
                return Err(SPLIT
                    .usage_referring(format!(
                        "invalid suffix {}, contains directory separator",
                        quote(&text)
                    ))
                    .into());
            }
            state.options.additional = text;
        }
        "numeric-suffixes" => set_alphabet(state, DECIMAL, value.as_deref())?,
        "hex-suffixes" => set_alphabet(state, HEXADECIMAL, value.as_deref())?,
        "filter" => state.options.filter = value,
        "verbose" => state.options.verbose = true,
        "separator" => set_separator(state, &text)?,
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    state: &mut Parsed,
) -> Result<(), Fail> {
    let body = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        match c {
            b'd' => set_alphabet(state, DECIMAL, None)?,
            b'x' => set_alphabet(state, HEXADECIMAL, None)?,
            b'e' => state.options.elide_empty = true,
            b'u' => {}
            b'a' | b'b' | b'C' | b'l' | b'n' | b't' => {
                let value: Vec<u8> = match body.get(at..) {
                    Some(rest) if !rest.is_empty() => {
                        at = body.len();
                        rest.to_vec()
                    }
                    _ => {
                        let next = args
                            .get(*i)
                            .ok_or_else(|| SPLIT.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        arg_bytes(&next)
                    }
                };
                match c {
                    b'a' => state.options.suffix_length = Some(parse_suffix_length(&value)?),
                    b'b' => {
                        set_mode(state, Kind::Bytes)?;
                        state.options.units =
                            parse_units(&value, Some(SIZE_SUFFIXES), "invalid number of bytes")?;
                    }
                    b'C' => {
                        set_mode(state, Kind::LineBytes)?;
                        state.options.units =
                            parse_units(&value, Some(SIZE_SUFFIXES), "invalid number of lines")?;
                    }
                    b'l' => {
                        set_mode(state, Kind::Lines)?;
                        state.options.units = parse_units(&value, None, "invalid number of lines")?;
                    }
                    b'n' => parse_number(&value, state)?,
                    _ => set_separator(state, &value)?,
                }
            }
            _ => return Err(SPLIT.invalid_option(c).into()),
        }
    }
    Ok(())
}

/// `-d`, `-x`, `--numeric-suffixes[=FROM]`, `--hex-suffixes[=FROM]`.
///
/// The presence of `FROM` is what turns auto-widening off, so it is tracked
/// even when the value is zero: `--numeric-suffixes=0` and a bare `-d` produce
/// identical names right up to the point where `-d` grows a digit and
/// `=0` gives up with `output file suffixes exhausted`.
fn set_alphabet(
    state: &mut Parsed,
    alphabet: &'static [u8],
    from: Option<&OsStr>,
) -> Result<(), Fail> {
    state.options.alphabet = alphabet;
    let Some(from) = from else {
        return Ok(());
    };
    let text = arg_bytes(from);
    // The value is checked character by character against the alphabet, not
    // parsed as a number, so a sign or a digit outside the base is refused
    // here rather than surviving to name a file. An *empty* value passes —
    // `strlen == strspn` holds vacuously — and behaves like a bare `-d` that
    // has nonetheless turned widening off.
    if !text.is_empty() && text.iter().any(|c| !alphabet.contains(c)) {
        let kind = if alphabet.len() == HEXADECIMAL.len() {
            "hexadecimal"
        } else {
            "numerical"
        };
        return Err(SPLIT
            .usage_referring(format!(
                "{}: invalid start value for {kind} suffix",
                quote(&text)
            ))
            .into());
    }
    // Leading zeros are dropped before anything measures the value, so
    // `--numeric-suffixes=007 -a 2` is accepted where `=700 -a 2` is not.
    let mut trimmed = text.as_slice();
    while trimmed.len() > 1 && trimmed.first() == Some(&b'0') {
        trimmed = trimmed.get(1..).unwrap_or_default();
    }
    state.options.start = Some(trimmed.to_vec());
    Ok(())
}

fn set_separator(state: &mut Parsed, text: &[u8]) -> Result<(), Fail> {
    // `-t '\0'` is the documented spelling of NUL, and the only escape the
    // option understands: `-t '\n'` is two characters and is refused.
    let byte = if text == b"\\0" {
        0
    } else {
        match text.split_first() {
            None => {
                return Err(Fail::new("empty record separator".to_string()));
            }
            Some((&only, [])) => only,
            Some(_) => {
                return Err(Fail::new(format!(
                    "multi-character separator {}",
                    quote(text)
                )));
            }
        }
    };
    if state.separator_set.is_some_and(|prior| prior != byte) {
        return Err(Fail::new(
            "multiple separator characters specified".to_string(),
        ));
    }
    state.separator_set = Some(byte);
    state.options.separator = byte;
    Ok(())
}

/// `-l`, `-b`, `-C`: gnulib's `xstrtoumax` with a floor of one, and — this is
/// the part that is not `xdectoumax` — **no diagnostic on overflow**.
///
/// `split -b 99999999999999999999999999` succeeds, saturating at the largest
/// representable count, because upstream's `parse_n_units` maps
/// `LONGINT_OVERFLOW` to `UINTMAX_MAX` and only treats the other statuses as
/// failures. Reaching for `xdectoumax` here would both reject that command and
/// bolt a `: Numerical result out of range` tail onto `-b 0`, which upstream
/// does not print.
fn parse_units(text: &[u8], suffixes: Option<&[u8]>, what: &str) -> Result<u64, Fail> {
    let (value, status) = xnum::xstrtoumax(text, suffixes.or(Some(b"")));
    match status {
        Status::Overflow => Ok(u64::MAX),
        Status::Ok if value != 0 => Ok(value),
        _ => Err(Fail::new(format!("{what}: {}", quote(text)))),
    }
}

/// `-a N`.
///
/// Upstream routes this through `xdectoumax`, whose out-of-range branch adds a
/// `strerror` tail — so `-a -1` is `invalid suffix length: ‘-1’: Numerical
/// result out of range` while `-a x` is `invalid suffix length: ‘x’` with no
/// tail. The difference is that a leading `-` reaches C's `strtoumax`, which
/// wraps it into a huge value that then fails the range check, where `x` never
/// becomes a number at all. Our `xstrtoumax` refuses the sign earlier (which
/// is what `fold -w -1` needs), so the negative case is recognised here.
fn parse_suffix_length(text: &[u8]) -> Result<usize, Fail> {
    let after_space = text
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(text.len());
    let negative = text.get(after_space) == Some(&b'-')
        && text
            .get(after_space.saturating_add(1))
            .is_some_and(u8::is_ascii_digit);
    if negative {
        return Err(Fail::new(format!(
            "invalid suffix length: {}: Numerical result out of range",
            quote(text)
        )));
    }
    let value = xnum::xdectoumax(
        text,
        0,
        u64::MAX.saturating_sub(1),
        Some(b""),
        "invalid suffix length",
    )
    .map_err(Fail::new)?;
    usize::try_from(value).map_err(|_| {
        Fail::new(format!(
            "invalid suffix length: {}: Value too large for defined data type",
            quote(text)
        ))
    })
}

/// `-n CHUNKS`.
///
/// The grammar is `[l/|r/][K/]N`, and the error messages give away exactly how
/// upstream reads it — with `strtoumax`'s end pointer rather than by splitting
/// on `/` first:
///
/// | argument | message | because |
/// |---|---|---|
/// | `x/3` | `invalid number of chunks: ‘x/3’` | no digits at all, so it never became a `K/N` |
/// | `/3` | `invalid number of chunks: ‘/3’` | likewise |
/// | `2/x` | `invalid number of chunks: ‘x’` | `2` converted, so `x` is the N that failed |
/// | `3/` | `invalid number of chunks: ‘’` | the N is the empty string |
/// | `2/3/4` | `invalid number of chunks: ‘3/4’` | only one `K/` is recognised |
/// | `0/3` | `invalid chunk number: ‘0’` | K converted but is outside `1..=N` |
///
/// Splitting on the first `/` and reporting each half would get the first two
/// rows wrong, which is why the end-pointer model is reproduced rather than
/// approximated.
fn parse_number(text: &[u8], state: &mut Parsed) -> Result<(), Fail> {
    // Leading whitespace is skipped before the prefix is looked for, not only
    // by the number scan underneath, so `-n ' l/3'` is `l/3` and not a
    // malformed byte-chunk count.
    // `\x0b` is in C's `isspace` and not in Rust's `is_ascii_whitespace`.
    let blank = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let text = match text.iter().position(|c| !blank(c)) {
        Some(at) => text.get(at..).unwrap_or_default(),
        None => b"",
    };
    let (kind, rest) = if text.starts_with(b"l/") {
        (Kind::ChunkLines, text.get(2..).unwrap_or_default())
    } else if text.starts_with(b"r/") {
        (Kind::RoundRobin, text.get(2..).unwrap_or_default())
    } else {
        (Kind::ChunkBytes, text)
    };
    set_mode(state, kind)?;

    let bad_chunks = |what: &[u8]| Fail::new(format!("invalid number of chunks: {}", quote(what)));

    let scan = scan_decimal(rest);
    let (count_text, piece) = match scan {
        // A number followed by `/`: the K/N form.
        Some((value, end)) if rest.get(end) == Some(&b'/') => (
            rest.get(end.saturating_add(1)..).unwrap_or_default(),
            Some(value),
        ),
        // A number and nothing else, or something that never converted: either
        // way the whole argument is the N being reported on.
        _ => (rest, None),
    };

    let count = match scan_decimal(count_text) {
        Some((value, end)) if end == count_text.len() && value != 0 => value,
        _ => return Err(bad_chunks(count_text)),
    };
    if let Some(k) = piece
        && (k == 0 || k > count)
    {
        let shown = rest
            .get(
                ..rest
                    .len()
                    .saturating_sub(count_text.len())
                    .saturating_sub(1),
            )
            .unwrap_or_default();
        return Err(Fail::new(format!("invalid chunk number: {}", quote(shown))));
    }
    state.options.units = count;
    state.options.piece = piece;
    Ok(())
}

/// C's `strtoumax` in base ten, saturating, returning the value and the index
/// one past the last digit — `None` when nothing converted.
fn scan_decimal(text: &[u8]) -> Option<(u64, usize)> {
    let mut at = text
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .unwrap_or(text.len());
    if text.get(at) == Some(&b'+') {
        at = at.saturating_add(1);
    }
    let first = at;
    let mut value = 0u64;
    while let Some(digit) = text.get(at).and_then(|c| (*c as char).to_digit(10)) {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(digit)))
            .unwrap_or(u64::MAX);
        at = at.saturating_add(1);
    }
    (at != first).then_some((value, at))
}

// ------------------------------------------------------------------- naming

/// The output file names, in order.
///
/// The body is a counter in `alphabet`'s base, most significant digit first;
/// `markers` copies of the alphabet's *last* character sit in front of it. See
/// the module doc for why the last character is reserved.
struct Namer {
    prefix: Vec<u8>,
    additional: Vec<u8>,
    alphabet: &'static [u8],
    body: Vec<usize>,
    markers: usize,
    widen: bool,
    started: bool,
}

impl Namer {
    fn new(
        prefix: Vec<u8>,
        additional: Vec<u8>,
        alphabet: &'static [u8],
        width: usize,
        start: u64,
        widen: bool,
    ) -> Self {
        let mut body = vec![0usize; width.max(1)];
        let base = u64::try_from(alphabet.len()).unwrap_or(26);
        let mut left = start;
        for slot in body.iter_mut().rev() {
            *slot = usize::try_from(left % base).unwrap_or(0);
            left /= base;
        }
        Namer {
            prefix,
            additional,
            alphabet,
            body,
            markers: 0,
            widen,
            started: false,
        }
    }

    fn render(&self) -> Vec<u8> {
        let marker = self.alphabet.last().copied().unwrap_or(b'z');
        let mut out = self.prefix.clone();
        out.resize(out.len().saturating_add(self.markers), marker);
        for &digit in &self.body {
            out.push(self.alphabet.get(digit).copied().unwrap_or(marker));
        }
        out.extend_from_slice(&self.additional);
        out
    }

    /// The next name, or `None` once the suffixes are exhausted.
    fn next(&mut self) -> Option<Vec<u8>> {
        if !self.started {
            self.started = true;
            return Some(self.render());
        }
        let base = self.alphabet.len();
        let width = self.body.len();
        for at in (0..width).rev() {
            let Some(slot) = self.body.get_mut(at) else {
                continue;
            };
            let raised = slot.saturating_add(1);
            if raised < base {
                *slot = raised;
                // The leading position reaching the marker character is the
                // signal to widen rather than a name to use: `xyz` is followed
                // by `xzaaa`, never by `xza`.
                if at == 0 && self.widen && raised == base.saturating_sub(1) {
                    self.markers = self.markers.saturating_add(1);
                    self.body = vec![0usize; width.saturating_add(1)];
                }
                return Some(self.render());
            }
            *slot = 0;
        }
        None
    }
}

/// How many digits `value` needs in `base`, at least one.
fn digits_needed(value: u64, base: u64) -> usize {
    let mut needed = 1usize;
    let mut left = value;
    while left >= base {
        left /= base;
        needed = needed.saturating_add(1);
    }
    needed
}

/// Settle the suffix width, and whether it may grow.
///
/// Three separate things can pin it down, and two of them produce a diagnostic
/// when they disagree with the user's `-a` — different diagnostics, because
/// they are different mistakes: a width too small for the *count* `-n` asked
/// for is a mistake in `-a`, while a width too small for the *start value* is a
/// mistake in the start value.
///
/// The `-n` sizing has one turn in it worth stating plainly. A start value is
/// folded into the count — so `-n 200 -d` needs three digits and `-n 200
/// --numeric-suffixes=100` needs four — but only when the start is *smaller
/// than the count*. A larger one is ignored, deliberately: upstream's comment
/// is that letting an arbitrary start widen the field "would break sort order
/// for files generated from multiple split runs". So `-n 3
/// --numeric-suffixes=999` does not quietly become four digits wide; it is an
/// error, because 999 does not fit the two digits the count alone justified.
fn suffix_plan(options: &Options) -> Result<(usize, u64, bool), Fail> {
    let base = u64::try_from(options.alphabet.len()).unwrap_or(26);
    let start = match &options.start {
        Some(text) => xnum::xstrtoumax_base(text, u32::try_from(base).unwrap_or(10), Some(b"")).0,
        None => 0,
    };
    // An explicit start turns widening off — the names it generates are not
    // all consecutive, so growing the field would put them out of order.
    let mut widen = options.start.is_none();

    let mut needed = 0usize;
    if options.kind.is_chunked() {
        widen = false;
        let mut last = options.units.saturating_sub(1);
        // The start is read in *decimal* here whatever the alphabet is, so a
        // hexadecimal start with a letter in it simply fails to convert and
        // leaves the count to size the field by itself.
        if let Some(text) = &options.start {
            let (value, status) = xnum::xstrtoumax(text, Some(b""));
            if status == Status::Ok && value < options.units {
                last = last.saturating_add(value);
            }
        }
        needed = digits_needed(last, base);
    }

    // `-a 0` is not "a width of zero"; upstream tests the width for truth, so
    // zero reads as "not given" and the default applies.
    let width = match options.suffix_length {
        Some(0) | None => needed.max(DEFAULT_SUFFIX_LENGTH),
        Some(given) if given >= needed => {
            widen = false;
            given
        }
        Some(_) => {
            return Err(Fail::new(format!(
                "the suffix length needs to be at least {needed}"
            )));
        }
    };

    // The start value is measured as *text*, not as a number, and after the
    // leading zeros have been stripped: `--numeric-suffixes=07` fits a width of
    // one, `=70` does not.
    if let Some(text) = &options.start
        && text.len() > width
    {
        return Err(SPLIT
            .usage_referring(
                "numerical suffix start value is too large for the suffix length".to_string(),
            )
            .into());
    }

    Ok((width, start, widen))
}

// ------------------------------------------------------------------ writing

/// Where a piece goes: to the next output file (or filter command), or to
/// standard output because `-n K/N` asked for one piece.
enum Sink {
    Files(Namer),
    Stdout,
}

struct Emitter<'a> {
    options: &'a Options,
    sink: Sink,
}

impl Emitter<'_> {
    fn emit(&mut self, data: &[u8]) -> Result<(), Fail> {
        let namer = match &mut self.sink {
            Sink::Stdout => {
                let mut out = io::stdout().lock();
                return out
                    .write_all(data)
                    .and_then(|()| out.flush())
                    .map_err(|e| Fail::new(format!("write error: {}", strerror(&e))));
            }
            Sink::Files(namer) => namer,
        };
        // `-e` suppresses the name as well as the file, so the pieces that do
        // get written stay consecutively named.
        if data.is_empty() && self.options.elide_empty {
            return Ok(());
        }
        let name = namer
            .next()
            .ok_or_else(|| Fail::new("output file suffixes exhausted".to_string()))?;
        match &self.options.filter {
            Some(command) => self.run_filter(command, &name, data),
            None => self.write_file(&name, data),
        }
    }

    fn write_file(&self, name: &[u8], data: &[u8]) -> Result<(), Fail> {
        let path = os_from_bytes(name);
        if self.options.verbose {
            // `quoteaf`, not `quote`: GNU spells this
            // `fprintf (stdout, _("creating file %s\n"), quoteaf (name))`, and
            // `quoteaf` is the shell-escape-always style, whose marks are
            // straight in every locale. Measured against GNU split 9.4 under
            // `LC_ALL=C.UTF-8`, which prints `creating file 'xaa'`. This read
            // `quote` — curly since §351 — until the harness moved off its `C`
            // reference, where the two styles are indistinguishable.
            println!("creating file {}", quoteaf(name));
        }
        // GNU names the output file bare and lets the errno finish the
        // sentence — `split: nosuchdir/aa: No such file or directory`.
        let mut file = File::create(&path)
            .map_err(|e| Fail::new(format!("{}: {}", quotef_os(&path), strerror(&e))))?;
        file.write_all(data)
            .and_then(|()| file.flush())
            .map_err(|e| Fail::new(format!("{}: {}", quotef_os(&path), strerror(&e))))
    }

    fn run_filter(&self, command: &OsStr, name: &[u8], data: &[u8]) -> Result<(), Fail> {
        let shown = String::from_utf8_lossy(name).into_owned();
        if self.options.verbose {
            println!("executing with FILE={shown}");
        }
        let mut child = shell(command)
            .env("FILE", os_from_bytes(name))
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| Fail::new(format!("with FILE={shown}: {}", strerror(&e))))?;
        if let Some(mut pipe) = child.stdin.take() {
            // A filter that exits without reading everything closes the pipe;
            // that is the command's business, not an error of ours, and the
            // non-zero status below is what reports it.
            let _ = pipe.write_all(data);
            drop(pipe);
        }
        let status = child
            .wait()
            .map_err(|e| Fail::new(format!("with FILE={shown}: {}", strerror(&e))))?;
        if status.success() {
            return Ok(());
        }
        let code = status.code().unwrap_or(1);
        Err(Fail {
            message: format!(
                "with FILE={shown}, exit {code} from command: {}",
                command.to_string_lossy()
            ),
            status: u8::try_from(code).unwrap_or(1),
        })
    }
}

// ------------------------------------------------------------------ splitting

/// The byte ranges of each record, terminator included. A trailing fragment
/// with no terminator is a record too.
fn records(data: &[u8], separator: u8) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (at, &byte) in data.iter().enumerate() {
        if byte == separator {
            out.push((start, at.saturating_add(1)));
            start = at.saturating_add(1);
        }
    }
    if start < data.len() {
        out.push((start, data.len()));
    }
    out
}

fn line_pieces(data: &[u8], separator: u8, per_file: u64) -> Vec<(usize, usize)> {
    let per_file = usize::try_from(per_file).unwrap_or(usize::MAX);
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut count = 0usize;
    for (_, end) in records(data, separator) {
        count = count.saturating_add(1);
        if count >= per_file {
            pieces.push((start, end));
            start = end;
            count = 0;
        }
    }
    if start < data.len() {
        pieces.push((start, data.len()));
    }
    pieces
}

fn byte_pieces(data: &[u8], per_file: u64) -> Vec<(usize, usize)> {
    let per_file = usize::try_from(per_file).unwrap_or(usize::MAX).max(1);
    let mut pieces = Vec::new();
    let mut start = 0usize;
    while start < data.len() {
        let end = start.saturating_add(per_file).min(data.len());
        pieces.push((start, end));
        start = end;
    }
    pieces
}

/// `-C`: whole records up to the limit, and a record longer than the limit cut
/// into limit-sized bites until the tail fits.
fn line_byte_pieces(data: &[u8], separator: u8, limit: u64) -> Vec<(usize, usize)> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX).max(1);
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut used = 0usize;
    for (record_start, record_end) in records(data, separator) {
        let length = record_end.saturating_sub(record_start);
        if used.saturating_add(length) <= limit {
            used = used.saturating_add(length);
            continue;
        }
        if used > 0 {
            let end = start.saturating_add(used);
            pieces.push((start, end));
            start = end;
        }
        // `used` is not cleared here: the tail of the record this one begins
        // becomes the whole of the next piece's contents, and that is what the
        // assignment below the cutting loop stores.
        let mut left = length;
        while left > limit {
            let end = start.saturating_add(limit);
            pieces.push((start, end));
            start = end;
            left = left.saturating_sub(limit);
        }
        used = left;
    }
    if used > 0 {
        pieces.push((start, start.saturating_add(used)));
    }
    pieces
}

/// `-n N`: equal byte shares, with the remainder handed to the *first* pieces.
fn chunk_byte_pieces(data: &[u8], count: u64) -> Vec<(usize, usize)> {
    let size = u128::try_from(data.len()).unwrap_or(0);
    let count = u128::from(count.max(1));
    let share = size / count;
    let extra = size % count;
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut index = 1u128;
    while index <= count {
        let end = index.saturating_mul(share).saturating_add(index.min(extra));
        let end = usize::try_from(end).unwrap_or(data.len()).min(data.len());
        let end = end.max(start);
        pieces.push((start, end));
        start = end;
        index = index.saturating_add(1);
    }
    pieces
}

/// `-n l/N`: the byte boundaries of `-n N`'s *ideal* division, each rounded
/// forward to the end of a record.
fn chunk_line_pieces(data: &[u8], separator: u8, count: u64) -> Vec<(usize, usize)> {
    let size = u128::try_from(data.len()).unwrap_or(0);
    let total = u128::from(count.max(1));
    let share = size / total;
    let extra = size % total;
    // The end of partition *m*, in bytes. GNU accumulates this with
    // `chunk_end += chunk_size + (chunk_no < rem)`, which comes to the same
    // closed form the byte chunks use — that equality is what lets `-n l/K/N`
    // seek straight to the K'th partition instead of replaying the file.
    let boundary = |m: u128| -> u128 { m.saturating_mul(share).saturating_add(m.min(extra)) };

    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut written = 0u128;
    let mut number = 1u128;
    let mut chunk_end = boundary(1);
    let mut start_new = true;
    let mut truncated = false;

    while written < size {
        // The search for the record terminator begins at the partition's LAST
        // byte, not its first byte past the end: a record that ends exactly on
        // the boundary belongs to this piece.
        let skip = chunk_end.saturating_sub(1).saturating_sub(written);
        let from = usize::try_from(written.saturating_add(skip)).unwrap_or(usize::MAX);
        let from = from.min(data.len());
        let (stop, found) = match data
            .get(from..)
            .and_then(|tail| tail.iter().position(|&c| c == separator))
        {
            Some(at) => (from.saturating_add(at).saturating_add(1), true),
            None => (data.len(), false),
        };
        let begin = usize::try_from(written).unwrap_or(usize::MAX);
        if start_new {
            pieces.push((begin, stop));
        } else if let Some(last) = pieces.last_mut() {
            last.1 = stop;
        }
        written = u128::try_from(stop).unwrap_or(size);
        start_new = found;

        // A record can be long enough to swallow whole partitions. Each one it
        // swallowed still gets a file, and that file is empty.
        let mut next = found;
        while next || chunk_end <= written {
            if !next && written >= size {
                truncated = true;
                break;
            }
            number = number.saturating_add(1);
            chunk_end = boundary(number);
            if chunk_end <= written {
                pieces.push((stop, stop));
            } else {
                next = false;
            }
        }
    }

    if truncated {
        number = number.saturating_add(1);
    }
    // Every one of the N files is created even when the input ran out first.
    while number <= total {
        pieces.push((data.len(), data.len()));
        number = number.saturating_add(1);
    }
    pieces
}

/// `-n r/N`: record *i* to piece *i mod N*.
fn round_robin_pieces(data: &[u8], separator: u8, count: u64) -> Vec<Vec<u8>> {
    let count = usize::try_from(count).unwrap_or(usize::MAX).max(1);
    let mut pieces: Vec<Vec<u8>> = vec![Vec::new(); count];
    for (index, (start, end)) in records(data, separator).into_iter().enumerate() {
        let Some(slot) = pieces.get_mut(index % count) else {
            continue;
        };
        if let Some(bytes) = data.get(start..end) {
            slot.extend_from_slice(bytes);
        }
    }
    pieces
}

// ----------------------------------------------------------------- the run

fn read_input(file: &OsString) -> Result<Vec<u8>, Fail> {
    let mut data = Vec::new();
    if file == OsStr::new("-") {
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| Fail::new(format!("read error: {}", strerror(&e))))?;
        return Ok(data);
    }
    // `quoteaf_os`, not `quote_os`: upstream is
    // `error (EXIT_FAILURE, errno, _("cannot open %s for reading"), quoteaf (infile))`,
    // and `quoteaf` is shell-escape-always, whose marks stay straight in every
    // locale. Measured, GNU split 9.4, `LC_ALL=C.UTF-8`:
    // `split: cannot open 'nosuch' for reading: No such file or directory`.
    let mut handle = File::open(file).map_err(|e| {
        Fail::new(format!(
            "cannot open {} for reading: {}",
            quoteaf_os(file),
            strerror(&e)
        ))
    })?;
    handle
        .read_to_end(&mut data)
        .map_err(|e| Fail::new(format!("{}: read error: {}", quotef_os(file), strerror(&e))))?;
    Ok(data)
}

fn run(options: &Options, file: &OsString, prefix: &OsString) -> Result<(), Fail> {
    if options.piece.is_some() && options.filter.is_some() {
        return Err(SPLIT
            .usage_referring("--filter does not process a chunk extracted to stdout".to_string())
            .into());
    }
    let (width, start, widen) = suffix_plan(options)?;
    let data = read_input(file)?;

    let sink = if options.piece.is_some() {
        Sink::Stdout
    } else {
        Sink::Files(Namer::new(
            arg_bytes(prefix),
            options.additional.clone(),
            options.alphabet,
            width,
            start,
            widen,
        ))
    };
    let mut emitter = Emitter { options, sink };

    if options.kind == Kind::RoundRobin {
        let pieces = round_robin_pieces(&data, options.separator, options.units);
        return emit_selected(&mut emitter, options, pieces.iter().map(Vec::as_slice));
    }

    let ranges = match options.kind {
        Kind::Lines => line_pieces(&data, options.separator, options.units),
        Kind::Bytes => byte_pieces(&data, options.units),
        Kind::LineBytes => line_byte_pieces(&data, options.separator, options.units),
        Kind::ChunkBytes => chunk_byte_pieces(&data, options.units),
        Kind::ChunkLines => chunk_line_pieces(&data, options.separator, options.units),
        Kind::RoundRobin => Vec::new(),
    };
    let pieces = ranges
        .into_iter()
        .map(|(start, end)| data.get(start..end).unwrap_or_default());
    emit_selected(&mut emitter, options, pieces)
}

/// Write every piece, or — under `-n K/N` — only the Kth.
fn emit_selected<'a, I: Iterator<Item = &'a [u8]>>(
    emitter: &mut Emitter<'_>,
    options: &Options,
    pieces: I,
) -> Result<(), Fail> {
    for (index, piece) in pieces.enumerate() {
        match options.piece {
            Some(wanted) => {
                if u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1) == wanted {
                    emitter.emit(piece)?;
                }
            }
            None => emitter.emit(piece)?,
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    fn os(text: &str) -> OsString {
        OsString::from(text)
    }

    fn parse(words: &[&str]) -> Result<Options, Fail> {
        let args: Vec<OsString> = words.iter().map(|w| os(w)).collect();
        match parse_args(&args)? {
            Request::Run(options, _, _) => Ok(*options),
            _ => panic!("expected a run"),
        }
    }

    fn names(options: &Options, howmany: usize) -> Vec<String> {
        let (width, start, widen) = suffix_plan(options).unwrap();
        let mut namer = Namer::new(
            b"x".to_vec(),
            options.additional.clone(),
            options.alphabet,
            width,
            start,
            widen,
        );
        let mut out = Vec::new();
        for _ in 0..howmany {
            match namer.next() {
                Some(name) => out.push(String::from_utf8_lossy(&name).into_owned()),
                None => {
                    out.push("<exhausted>".to_string());
                    break;
                }
            }
        }
        out
    }

    // ------------------------------------------------------------ suffixes

    #[test]
    fn alphabetic_suffixes_start_at_aa() {
        let options = parse(&["-l", "1"]).unwrap();
        assert_eq!(names(&options, 3), ["xaa", "xab", "xac"]);
    }

    #[test]
    fn alphabetic_suffixes_widen_after_yz() {
        let options = parse(&["-l", "1"]).unwrap();
        let all = names(&options, 652);
        assert_eq!(all.get(648).map(String::as_str), Some("xyy"));
        assert_eq!(all.get(649).map(String::as_str), Some("xyz"));
        assert_eq!(all.get(650).map(String::as_str), Some("xzaaa"));
        assert_eq!(all.get(651).map(String::as_str), Some("xzaab"));
    }

    #[test]
    fn alphabetic_suffixes_widen_a_second_time() {
        let options = parse(&["-l", "1"]).unwrap();
        // 650 two-character names, then 25 * 26 * 26 = 16900 four-character
        // ones, and the next widening adds a second marker.
        let all = names(&options, 17_552);
        assert_eq!(all.get(17_549).map(String::as_str), Some("xzyzz"));
        assert_eq!(all.get(17_550).map(String::as_str), Some("xzzaaaa"));
    }

    #[test]
    fn numeric_suffixes_widen_after_89() {
        let options = parse(&["-d", "-l", "1"]).unwrap();
        let all = names(&options, 92);
        assert_eq!(all.get(89).map(String::as_str), Some("x89"));
        assert_eq!(all.get(90).map(String::as_str), Some("x9000"));
        assert_eq!(all.get(91).map(String::as_str), Some("x9001"));
    }

    #[test]
    fn numeric_suffixes_widen_a_second_time_after_9899() {
        let options = parse(&["-d", "-l", "1"]).unwrap();
        let all = names(&options, 992);
        assert_eq!(all.get(989).map(String::as_str), Some("x9899"));
        assert_eq!(all.get(990).map(String::as_str), Some("x990000"));
    }

    #[test]
    fn hex_suffixes_widen_after_ef() {
        let options = parse(&["-x", "-l", "1"]).unwrap();
        let all = names(&options, 242);
        assert_eq!(all.get(239).map(String::as_str), Some("xef"));
        assert_eq!(all.get(240).map(String::as_str), Some("xf000"));
    }

    #[test]
    fn explicit_suffix_length_uses_the_whole_range_then_stops() {
        let options = parse(&["-a", "2", "-l", "1"]).unwrap();
        let all = names(&options, 678);
        assert_eq!(all.get(675).map(String::as_str), Some("xzz"));
        assert_eq!(all.get(676).map(String::as_str), Some("<exhausted>"));
    }

    #[test]
    fn a_start_value_turns_widening_off() {
        let options = parse(&["--numeric-suffixes=95", "-l", "1"]).unwrap();
        assert_eq!(
            names(&options, 6),
            ["x95", "x96", "x97", "x98", "x99", "<exhausted>"]
        );
    }

    #[test]
    fn chunk_mode_presizes_the_suffix() {
        let options = parse(&["-n", "700"]).unwrap();
        let all = names(&options, 700);
        assert_eq!(all.first().map(String::as_str), Some("xaaa"));
        assert_eq!(all.get(699).map(String::as_str), Some("xbax"));
    }

    #[test]
    fn chunk_mode_keeps_the_default_width_when_it_fits() {
        let options = parse(&["-n", "3"]).unwrap();
        assert_eq!(names(&options, 3), ["xaa", "xab", "xac"]);
    }

    #[test]
    fn additional_suffix_is_appended() {
        let options = parse(&["--additional-suffix=.txt", "-l", "1"]).unwrap();
        assert_eq!(names(&options, 2), ["xaa.txt", "xab.txt"]);
    }

    // -------------------------------------------------------------- errors

    #[test]
    fn two_modes_are_refused() {
        let e = parse(&["-l", "5", "-b", "5"]).unwrap_err();
        assert!(e.message.starts_with("cannot split in more than one way"));
    }

    #[test]
    fn the_same_mode_twice_is_also_refused() {
        let e = parse(&["-l", "5", "-l", "6"]).unwrap_err();
        assert!(e.message.starts_with("cannot split in more than one way"));
    }

    #[test]
    fn zero_bytes_is_refused_without_an_errno_tail() {
        let e = parse(&["-b", "0"]).unwrap_err();
        assert_eq!(e.message, "invalid number of bytes: ‘0’");
    }

    #[test]
    fn an_enormous_byte_count_saturates_rather_than_failing() {
        let options = parse(&["-b", "99999999999999999999999999"]).unwrap();
        assert_eq!(options.units, u64::MAX);
    }

    #[test]
    fn lines_take_no_multiplier_suffix() {
        let e = parse(&["-l", "1k"]).unwrap_err();
        assert_eq!(e.message, "invalid number of lines: ‘1k’");
    }

    #[test]
    fn line_bytes_take_a_multiplier_suffix() {
        let options = parse(&["-C", "1k"]).unwrap();
        assert_eq!(options.units, 1024);
    }

    #[test]
    fn line_bytes_borrows_the_lines_diagnostic() {
        let e = parse(&["-C", "0"]).unwrap_err();
        assert_eq!(e.message, "invalid number of lines: ‘0’");
    }

    #[test]
    fn chunks_report_the_whole_argument_when_nothing_converted() {
        assert_eq!(
            parse(&["-n", "x/3"]).unwrap_err().message,
            "invalid number of chunks: ‘x/3’"
        );
        assert_eq!(
            parse(&["-n", "/3"]).unwrap_err().message,
            "invalid number of chunks: ‘/3’"
        );
    }

    #[test]
    fn chunks_report_only_the_count_when_the_k_converted() {
        assert_eq!(
            parse(&["-n", "2/x"]).unwrap_err().message,
            "invalid number of chunks: ‘x’"
        );
        assert_eq!(
            parse(&["-n", "3/"]).unwrap_err().message,
            "invalid number of chunks: ‘’"
        );
    }

    #[test]
    fn only_one_slash_pair_is_recognised() {
        assert_eq!(
            parse(&["-n", "2/3/4"]).unwrap_err().message,
            "invalid number of chunks: ‘3/4’"
        );
    }

    #[test]
    fn a_chunk_number_outside_the_count_is_its_own_message() {
        assert_eq!(
            parse(&["-n", "0/3"]).unwrap_err().message,
            "invalid chunk number: ‘0’"
        );
        assert_eq!(
            parse(&["-n", "4/3"]).unwrap_err().message,
            "invalid chunk number: ‘4’"
        );
        assert_eq!(
            parse(&["-n", "l/0/3"]).unwrap_err().message,
            "invalid chunk number: ‘0’"
        );
    }

    #[test]
    fn chunks_take_no_multiplier_suffix() {
        assert_eq!(
            parse(&["-n", "2k"]).unwrap_err().message,
            "invalid number of chunks: ‘2k’"
        );
    }

    #[test]
    fn chunks_accept_leading_space_and_a_plus() {
        assert_eq!(parse(&["-n", " 3"]).unwrap().units, 3);
        assert_eq!(parse(&["-n", "+3"]).unwrap().units, 3);
    }

    #[test]
    fn a_negative_suffix_length_is_out_of_range_not_unparsable() {
        assert_eq!(
            parse(&["-a", "-1"]).unwrap_err().message,
            "invalid suffix length: ‘-1’: Numerical result out of range"
        );
        assert_eq!(
            parse(&["-a", "x"]).unwrap_err().message,
            "invalid suffix length: ‘x’"
        );
    }

    #[test]
    fn a_start_value_too_wide_for_the_suffix_is_its_own_message() {
        let options = parse(&["-a", "1", "--numeric-suffixes=95"]).unwrap();
        let e = suffix_plan(&options).unwrap_err();
        assert!(
            e.message
                .starts_with("numerical suffix start value is too large for the suffix length")
        );
    }

    #[test]
    fn a_suffix_too_narrow_for_the_chunk_count_names_the_width() {
        let options = parse(&["-n", "700", "-a", "2"]).unwrap();
        let e = suffix_plan(&options).unwrap_err();
        assert_eq!(e.message, "the suffix length needs to be at least 3");
    }

    #[test]
    fn a_separator_must_be_one_character() {
        assert_eq!(
            parse(&["-t", "xy"]).unwrap_err().message,
            "multi-character separator ‘xy’"
        );
        assert_eq!(
            parse(&["-t", ""]).unwrap_err().message,
            "empty record separator"
        );
        assert_eq!(
            parse(&["-t", "a", "-t", "b"]).unwrap_err().message,
            "multiple separator characters specified"
        );
    }

    #[test]
    fn the_same_separator_twice_is_allowed() {
        assert_eq!(parse(&["-t", "a", "-t", "a"]).unwrap().separator, b'a');
    }

    #[test]
    fn backslash_zero_is_nul() {
        assert_eq!(parse(&["-t", "\\0"]).unwrap().separator, 0);
    }

    #[test]
    fn an_additional_suffix_may_not_contain_a_slash() {
        let e = parse(&["--additional-suffix=/x"]).unwrap_err();
        assert!(
            e.message
                .starts_with("invalid suffix ‘/x’, contains directory separator")
        );
    }

    #[test]
    fn a_bad_start_value_is_reported_with_the_value_first() {
        let e = parse(&["--numeric-suffixes=abc"]).unwrap_err();
        assert!(
            e.message
                .starts_with("‘abc’: invalid start value for numerical suffix")
        );
    }

    #[test]
    fn hex_start_values_accept_hex_digits() {
        let options = parse(&["--hex-suffixes=e", "-l", "1"]).unwrap();
        assert_eq!(names(&options, 2), ["x0e", "x0f"]);
    }

    #[test]
    fn an_extra_operand_is_refused() {
        let e = parse(&["f", "y", "extra"]).unwrap_err();
        assert!(e.message.starts_with("extra operand ‘extra’"));
    }

    #[test]
    fn the_obsolete_count_still_works() {
        let options = parse(&["-5"]).unwrap();
        assert_eq!(options.kind, Kind::Lines);
        assert_eq!(options.units, 5);
    }

    #[test]
    fn an_unknown_short_option_is_getopts_message() {
        let e = parse(&["-z"]).unwrap_err();
        assert!(e.message.starts_with("invalid option -- 'z'"));
    }

    #[test]
    fn an_ambiguous_long_option_lists_the_candidates() {
        let e = parse(&["--num=3"]).unwrap_err();
        assert!(
            e.message.starts_with(
                "option '--num=3' is ambiguous; possibilities: '--number' '--numeric-suffixes'"
            ),
            "{}",
            e.message
        );
    }

    // ------------------------------------------------------------- pieces

    fn shown(data: &[u8], ranges: &[(usize, usize)]) -> Vec<String> {
        ranges
            .iter()
            .map(|&(start, end)| {
                String::from_utf8_lossy(data.get(start..end).unwrap_or_default()).into_owned()
            })
            .collect()
    }

    #[test]
    fn lines_group_records() {
        let data = b"a\nb\nc\n";
        assert_eq!(shown(data, &line_pieces(data, b'\n', 2)), ["a\nb\n", "c\n"]);
    }

    #[test]
    fn a_final_record_without_a_terminator_still_counts() {
        let data = b"a\nb\nc";
        assert_eq!(shown(data, &line_pieces(data, b'\n', 2)), ["a\nb\n", "c"]);
    }

    #[test]
    fn bytes_cut_mid_record() {
        let data = b"abcdefg";
        assert_eq!(shown(data, &byte_pieces(data, 3)), ["abc", "def", "g"]);
    }

    #[test]
    fn line_bytes_packs_whole_records() {
        let data = b"aaaa\nbb\ncccccccccccc\nd\n";
        assert_eq!(
            shown(data, &line_byte_pieces(data, b'\n', 10)),
            ["aaaa\nbb\n", "cccccccccc", "cc\nd\n"]
        );
    }

    #[test]
    fn line_bytes_cuts_an_over_long_record_at_the_limit() {
        let data = b"aaaa\nbb\ncccccccccccc\nd\n";
        assert_eq!(
            shown(data, &line_byte_pieces(data, b'\n', 4)),
            ["aaaa", "\nbb\n", "cccc", "cccc", "cccc", "\nd\n"]
        );
    }

    #[test]
    fn line_bytes_five() {
        let data = b"aaaa\nbb\ncccccccccccc\nd\n";
        assert_eq!(
            shown(data, &line_byte_pieces(data, b'\n', 5)),
            ["aaaa\n", "bb\n", "ccccc", "ccccc", "cc\nd\n"]
        );
    }

    #[test]
    fn chunk_bytes_give_the_remainder_to_the_first_pieces() {
        let data = b"abcdefghij";
        assert_eq!(
            shown(data, &chunk_byte_pieces(data, 3)),
            ["abcd", "efg", "hij"]
        );
        assert_eq!(
            shown(data, &chunk_byte_pieces(data, 4)),
            ["abc", "def", "gh", "ij"]
        );
        assert_eq!(
            shown(data, &chunk_byte_pieces(data, 7)),
            ["ab", "cd", "ef", "g", "h", "i", "j"]
        );
    }

    #[test]
    fn chunk_bytes_create_empty_pieces_past_the_end() {
        let data = b"abc";
        assert_eq!(
            shown(data, &chunk_byte_pieces(data, 5)),
            ["a", "b", "c", "", ""]
        );
    }

    #[test]
    fn chunk_lines_round_boundaries_forward() {
        let data = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20\n";
        assert_eq!(
            shown(data, &chunk_line_pieces(data, b'\n', 3)),
            [
                "1\n2\n3\n4\n5\n6\n7\n8\n9\n",
                "10\n11\n12\n13\n14\n15\n",
                "16\n17\n18\n19\n20\n"
            ]
        );
    }

    /// The boundary is `i * size / n` computed exactly, not `i * (size / n)`:
    /// the fifth boundary of a 51-byte file in seven differs between the two,
    /// and the whole of line 16 moves file.
    #[test]
    fn chunk_lines_use_an_exact_boundary_not_a_truncated_share() {
        let data = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20\n";
        assert_eq!(
            shown(data, &chunk_line_pieces(data, b'\n', 7)),
            [
                "1\n2\n3\n4\n",
                "5\n6\n7\n8\n",
                "9\n10\n11\n",
                "12\n13\n",
                "14\n15\n16\n",
                "17\n18\n",
                "19\n20\n"
            ]
        );
    }

    /// Fewer records than pieces. The empties are *interleaved*, not swept to
    /// the end: a record that overruns a partition consumes it, and the file
    /// for the consumed partition is still created, in its place in the
    /// sequence. Three records in five pieces here gives record, record,
    /// empty, record, empty — not record, record, record, empty, empty.
    #[test]
    fn chunk_lines_leave_the_overrun_partitions_empty_in_place() {
        let data = b"a\nb\nc\n";
        let pieces = chunk_line_pieces(data, b'\n', 5);
        assert_eq!(shown(data, &pieces), ["a\n", "b\n", "", "c\n", ""]);
    }

    /// The same effect from the other side: one record longer than several
    /// partitions swallows them all, and each swallowed partition still gets
    /// its (empty) file.
    #[test]
    fn chunk_lines_create_an_empty_piece_per_swallowed_partition() {
        let data = b"aaaaaaaaaaaa\nb\n";
        let pieces = chunk_line_pieces(data, b'\n', 5);
        assert_eq!(shown(data, &pieces), ["aaaaaaaaaaaa\n", "", "", "", "b\n"]);
    }

    /// No separator at all: the single record takes the first piece and every
    /// other file is created empty. The last one comes from the "ensure NUMBER
    /// files are created" sweep rather than from the swallowing loop, which is
    /// a different code path reaching the same place.
    #[test]
    fn chunk_lines_without_a_separator_fill_the_first_piece() {
        let data = b"0123456789";
        let pieces = chunk_line_pieces(data, b'\n', 3);
        assert_eq!(shown(data, &pieces), ["0123456789", "", ""]);
    }

    #[test]
    fn round_robin_deals_records_out() {
        let data = b"1\n2\n3\n4\n5\n6\n7\n";
        let pieces = round_robin_pieces(data, b'\n', 3);
        let text: Vec<String> = pieces
            .iter()
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        assert_eq!(text, ["1\n4\n7\n", "2\n5\n", "3\n6\n"]);
    }

    #[test]
    fn round_robin_keeps_an_unterminated_last_record() {
        let data = b"a\nb\nc";
        let pieces = round_robin_pieces(data, b'\n', 2);
        let text: Vec<String> = pieces
            .iter()
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        assert_eq!(text, ["a\nc", "b\n"]);
    }

    #[test]
    fn an_empty_input_yields_no_pieces_outside_chunk_modes() {
        assert!(line_pieces(b"", b'\n', 2).is_empty());
        assert!(byte_pieces(b"", 2).is_empty());
        assert!(line_byte_pieces(b"", b'\n', 2).is_empty());
    }

    #[test]
    fn an_empty_input_still_yields_every_chunk() {
        assert_eq!(chunk_byte_pieces(b"", 3).len(), 3);
        assert_eq!(chunk_line_pieces(b"", b'\n', 3).len(), 3);
        assert_eq!(round_robin_pieces(b"", b'\n', 3).len(), 3);
    }

    #[test]
    fn a_separator_other_than_newline_delimits_records() {
        let data = b"a\0b\0c\0";
        assert_eq!(shown(data, &line_pieces(data, 0, 2)), ["a\0b\0", "c\0"]);
    }
}
