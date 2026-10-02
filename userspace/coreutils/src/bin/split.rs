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
//! # `--filter`
//!
//! Each piece is handed to a shell command on its standard input, with `$FILE`
//! naming the file the piece would have been. The shell is the user's
//! `$SHELL` -- `/bin/sh` only when that is unset -- named in `argv[0]` by its
//! last component and run as `execl` runs a program, never through a `PATH`
//! search. A command that stops reading early is not a failure (upstream
//! ignores `SIGPIPE` while it filters and lets `EPIPE` pass), and nor is one
//! that dies *of* `SIGPIPE`; its own reader went first.
//!
//! # An output that is the input
//!
//! Before an existing file is truncated it is compared with the input, and a
//! match stops the run: `'in.txt' would overwrite input; aborting`. Reading
//! the whole input first, as this does, would otherwise save the bytes and
//! still replace the file that held them with its own first piece.
//!
//! # Exit status
//!
//! 0 on success, 1 on any failure — except under `--filter`, where a command
//! that fails hands its own status back, so `--filter='exit 3'` exits 3, and
//! one killed by a signal exits 128 plus its number, as a shell reports it.

use coreutils::errmsg::strerror;
use coreutils::fileid;
use coreutils::getopt::{self, Program, Takes};
use coreutils::pathname::last_component;
use coreutils::quote::{os_bytes, os_from_bytes, quote, quoteaf, quoteaf_os, quotef, quotef_os};
use coreutils::shell::shell_as;
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{ChildStdin, Command, ExitCode, ExitStatus, Stdio};

coreutils::guard_std_fds!();

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
/// The message is bytes because two of them quote the `--filter` command, and
/// upstream prints that as it was given -- `with FILE=xaa, exit 3 from
/// command: CMD` -- where a command is argv data and need not be text.
///
/// The status is carried rather than assumed because `--filter` breaks the
/// otherwise-universal rule that a failure exits 1 — it exits with whatever
/// the command exited with, or 128 plus the signal that ended it.
#[derive(Debug)]
struct Fail {
    message: Vec<u8>,
    status: u8,
}

impl Fail {
    fn new(message: String) -> Self {
        Fail {
            message: message.into_bytes(),
            status: 1,
        }
    }
}

impl From<getopt::Error> for Fail {
    fn from(e: getopt::Error) -> Self {
        Fail {
            message: e.message().into_bytes(),
            status: u8::try_from(e.status).unwrap_or(1),
        }
    }
}

/// `split: MESSAGE` on standard error, as bytes -- see [`Fail`].
fn say(message: &[u8]) {
    let mut line = b"split: ".to_vec();
    line.extend_from_slice(message);
    line.push(b'\n');
    stdfd::diag_bytes(&line);
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
///
/// Measured with `split --=x`, which an empty prefix makes print the whole
/// table. `scripts/getopt-ambiguity-check.py` compares this list against that
/// readout on every run; it is how the `-io-blksize` entry below was found
/// missing.
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
    // The leading hyphen is part of the *name*: GNU's table holds
    // `-io-blksize`, so it is typed with three dashes. It is an internal tuning
    // knob deliberately made awkward to type, not a user-facing option, and it
    // takes an argument (`split ---io-blksize` answers `requires an argument`).
    //
    // Because the name starts with `-` it is reachable only from a `---`
    // prefix, so listing it cannot change what any ordinary `--name` resolves
    // to; it is here so that `---i` resolves rather than being called
    // unrecognised.
    ("-io-blksize", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

fn arg_bytes(a: &OsStr) -> Vec<u8> {
    os_bytes(a).into_owned()
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args, getopt::posixly_correct()) {
        Ok(r) => r,
        Err(e) => {
            say(&e.message);
            return ExitCode::from(e.status);
        }
    };
    // Standard output as stdio has it -- by block unless it is a terminal --
    // because that is observable here. A `--filter` command writes to the same
    // descriptor directly, so upstream's `executing with FILE=` lines reach a
    // file or a pipe *after* everything the commands wrote, at the final flush.
    // Every write to it is deliberately unread: a failed write is `Stream`'s
    // to remember and `close_stdout`'s to report, once, as upstream's
    // `atexit (close_stdout)` does -- `split --help >/dev/full` says
    // `split: write error: No space left on device` and exits 1.
    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"split (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Run(options, file, prefix) => match run(&options, &file, &prefix, &mut out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                say(&e.message);
                ExitCode::from(e.status)
            }
        },
    };
    stdfd::close_stdout("split", out, earned)
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

/// `posixly_correct` is [`getopt::posixly_correct`], passed in so that a test
/// can choose it. When it is set, the first operand ends option parsing, as it
/// does in glibc's getopt -- see "Where option parsing stops" in that module.
fn parse_args(args: &[OsString], posixly_correct: bool) -> Result<Request, Fail> {
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
            // Under POSIXLY_CORRECT, glibc's getopt stops at the first operand.
            only_operands = posixly_correct;
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
        // Same reasoning as `unbuffered` above, and the same conclusion: it
        // names the size of the read buffer, which this implementation does not
        // have, because it reads the input whole. There is no observable
        // difference for it to make, so accepting it is honest rather than
        // permissive. It is spelled with a leading hyphen in GNU's table, hence
        // three dashes on the command line.
        "-io-blksize" => {}
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
            *slot = usize::try_from(left.checked_rem(base).unwrap_or(0)).unwrap_or(0);
            left = left.checked_div(base).unwrap_or(0);
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
    // Every alphabet has at least two digits; a base below two would never
    // finish dividing, so it gets the one digit it can have.
    while base > 1 && left >= base {
        left = left.checked_div(base).unwrap_or(0);
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
    /// The input operand, `-` for standard input, and what `fstat` said of it:
    /// upstream's `infile` and `in_stat_buf`, which [`Emitter::create`]
    /// compares every output file against.
    input_name: &'a OsStr,
    input_meta: &'a Metadata,
    /// Standard output, for `--verbose` and for the round-robin `-n r/K/N`.
    out: &'a mut Stream,
}

impl Emitter<'_> {
    fn emit(&mut self, data: &[u8]) -> Result<(), Fail> {
        let namer = match &mut self.sink {
            Sink::Stdout => return self.write_stdout(data),
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
        // The options outlive this borrow of `self`, so the command can be
        // lent to a method that needs `self` mutably.
        let options = self.options;
        match &options.filter {
            Some(command) => self.run_filter(command, &name, data),
            None => self.write_file(&name, data),
        }
    }

    /// `-n K/N`: the one piece, on standard output, written the way each of
    /// upstream's modes writes it -- which decides how a failure reads. The
    /// byte mode writes straight to descriptor 1 and names it `-`, so a full
    /// disk is `split: -: No space left on device`; the line and round-robin
    /// modes say `write error`, which is what [`Stream`] says at the close.
    fn write_stdout(&mut self, data: &[u8]) -> Result<(), Fail> {
        if self.options.kind == Kind::ChunkBytes {
            return match stdfd::write_all(1, data) {
                Ok(()) => Ok(()),
                // Nobody is left to read it: see `stdfd::reader_gone`.
                Err(e) if stdfd::reader_gone(&e) => Ok(()),
                Err(e) => Err(Fail::new(format!("{}: {}", quotef(b"-"), strerror(&e)))),
            };
        }
        // Unread: see `run_main`.
        let _ = self.out.write_all(data);
        Ok(())
    }

    /// One output file: upstream's `create`, the write, and `closeout`.
    fn write_file(&mut self, name: &[u8], data: &[u8]) -> Result<(), Fail> {
        if self.options.verbose {
            // `quoteaf`, not `quote`: GNU spells this
            // `fprintf (stdout, _("creating file %s\n"), quoteaf (name))`, and
            // `quoteaf` is the shell-escape-always style, whose marks are
            // straight in every locale. Measured against GNU split 9.4 under
            // `LC_ALL=C.UTF-8`, which prints `creating file 'xaa'`. This read
            // `quote` — curly since §351 — until the harness moved off its `C`
            // reference, where the two styles are indistinguishable. Printed
            // before the open, so a name that is then refused was announced.
            // Unread: see `run_main`.
            let _ = writeln!(self.out, "creating file {}", quoteaf(name));
        }
        // GNU names the output file bare and lets the errno finish the
        // sentence — `split: nosuchdir/aa: No such file or directory` — for the
        // open, the write and the close alike.
        let failed = |e: io::Error| Fail::new(format!("{}: {}", quotef(name), strerror(&e)));
        let mut file = self.create(name)?;
        file.write_all(data).map_err(failed)?;
        stdfd::close(file).map_err(failed)
    }

    /// Open a piece's file the way upstream's `create` does.
    ///
    /// First with `O_EXCL`, which settles the common case -- a name nobody has
    /// used -- in one call. A name that already exists is opened again without
    /// it, and examined before a byte is written: if it is the *input* the run
    /// stops, rather than truncate the file it is splitting. (`split -l 1 -a 1
    /// --additional-suffix=.txt in.txt i` reaches `in.txt` at its fourteenth
    /// piece.) Only then is it emptied, by `ftruncate` rather than `O_TRUNC`,
    /// and only when there is something to empty: a FIFO or a device may be an
    /// output, and truncating one can fail without that being an error.
    fn create(&self, name: &[u8]) -> Result<File, Fail> {
        let path = os_from_bytes(name);
        let failed = |e: io::Error| Fail::new(format!("{}: {}", quotef(name), strerror(&e)));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok(file),
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(failed(e)),
            Err(_) => {}
        }
        // No `O_TRUNC`: what is there is looked at first, below.
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(failed)?;
        let meta = file.metadata().map_err(|e| {
            Fail::new(format!(
                "failed to stat {}: {}",
                quoteaf(name),
                strerror(&e)
            ))
        })?;
        if fileid::same_inode(
            (Path::new(self.input_name), self.input_meta),
            (Path::new(&path), &meta),
        ) {
            return Err(Fail::new(format!(
                "{} would overwrite input; aborting",
                quoteaf(name)
            )));
        }
        let regular = meta.is_file();
        if !(regular && meta.len() == 0)
            && let Err(e) = file.set_len(0)
            && regular
        {
            return Err(Fail::new(format!(
                "{}: error truncating: {}",
                quotef(name),
                strerror(&e)
            )));
        }
        Ok(file)
    }

    /// `--filter`: run COMMAND with `$FILE` naming this piece, and hand it the
    /// piece on its standard input -- upstream's `create` and `closeout`.
    ///
    /// The shell is the user's: `$SHELL`, or `/bin/sh` only when that is
    /// *unset* (an empty `SHELL` is used, and fails). It is named in `argv[0]`
    /// by its last component, as `execl (shell_prog, last_component
    /// (shell_prog), "-c", filter_command, (char *) nullptr)` names it, so a
    /// command that fails under `SHELL=/bin/bash` says `bash: line 1: ...`.
    fn run_filter(&mut self, command: &OsStr, name: &[u8], data: &[u8]) -> Result<(), Fail> {
        if self.options.verbose {
            // `quotef`, where `creating file` has `quoteaf`: measured, and
            // upstream's own `fprintf (stdout, _("executing with FILE=%s\n"),
            // quotef (name))`. Unread: see `run_main`.
            let _ = writeln!(self.out, "executing with FILE={}", quotef(name));
        }
        let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
        let shell_bytes = os_bytes(&shell).into_owned();
        let arg0 = os_from_bytes(last_component(&shell_bytes));
        let mut run = shell_as(exec_path(&shell_bytes), &arg0, command);
        run.env("FILE", os_from_bytes(name)).stdin(Stdio::piped());
        if stdfd::sigpipe_ignored_at_startup() {
            keep_sigpipe_ignored(&mut run);
        }
        let spawned = run.spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                // Upstream forks first and execs in the child, so a shell that
                // cannot be run is reported *by the child*, which then exits 1
                // -- and the parent reports that as it would any command's
                // status. Both lines, in that order:
                //
                //   split: failed to run command: "/nope -c cat": No such file or directory
                //   split: with FILE=xaa, exit 1 from command: cat
                //
                // Every way `spawn` can fail is taken for an exec failure. The
                // others -- no pipe, no process -- have their own sentences
                // upstream, but reaching one here takes an exhausted system.
                let mut line = b"failed to run command: \"".to_vec();
                line.extend_from_slice(&shell_bytes);
                line.extend_from_slice(b" -c ");
                line.extend_from_slice(&os_bytes(command));
                line.extend_from_slice(format!("\": {}", strerror(&e)).as_bytes());
                say(&line);
                return Err(command_failed(name, command, "exit 1", 1));
            }
        };
        if let Some(mut pipe) = child.stdin.take() {
            match pipe.write_all(data) {
                Ok(()) => {}
                // Upstream ignores SIGPIPE while it filters and lets EPIPE pass
                // (`ignorable`): a command that stops reading early -- `head
                // -1` -- has taken what it wanted, and its exit status is what
                // says whether that was a failure.
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
                Err(e) => {
                    return Err(Fail::new(format!("{}: {}", quotef(name), strerror(&e))));
                }
            }
            close_pipe(pipe)
                .map_err(|e| Fail::new(format!("{}: {}", quotef(name), strerror(&e))))?;
        }
        let status = child
            .wait()
            .map_err(|e| Fail::new(format!("waiting for child process: {}", strerror(&e))))?;
        command_verdict(name, command, status)
    }
}

/// What a finished `--filter` command's status means: nothing, if it exited 0
/// or died of `SIGPIPE`. Otherwise the run stops, saying which, and exits with
/// the command's own status -- or, for a signal, 128 plus its number --
/// exactly as `closeout` does.
fn command_verdict(name: &[u8], command: &OsStr, status: ExitStatus) -> Result<(), Fail> {
    if let Some(code) = status.code() {
        if code == 0 {
            return Ok(());
        }
        let exit = u8::try_from(code).unwrap_or(1);
        return Err(command_failed(name, command, &format!("exit {code}"), exit));
    }
    signal_verdict(name, command, status)
}

#[cfg(unix)]
fn signal_verdict(name: &[u8], command: &OsStr, status: ExitStatus) -> Result<(), Fail> {
    use coreutils::sig2str::sig2str;
    use std::os::unix::process::ExitStatusExt;
    let Some(signal) = status.signal() else {
        // Neither exited nor signalled: upstream's "shouldn't happen".
        return Err(Fail::new(format!(
            "unknown status from command (0x{:X})",
            status.into_raw()
        )));
    };
    // The command's own reader left first; what it wrote was somebody else's
    // to want, and its death is not this run's failure.
    if signal == libcall::SIGPIPE {
        return Ok(());
    }
    // gnulib's `sig2str`, so `TERM` and `RTMIN+3`; a number it cannot name is
    // printed as the number.
    let named = sig2str(signal).unwrap_or_else(|| signal.to_string());
    let exit = u8::try_from(signal.saturating_add(128)).unwrap_or(u8::MAX);
    Err(command_failed(
        name,
        command,
        &format!("signal {named}"),
        exit,
    ))
}

/// Off Unix a status with no code has nothing more to say.
#[cfg(not(unix))]
fn signal_verdict(_name: &[u8], _command: &OsStr, _status: ExitStatus) -> Result<(), Fail> {
    Err(Fail::new("unknown status from command".to_string()))
}

/// `with FILE=NAME, WHAT from command: COMMAND` -- the name quoted for a
/// shell if it needs it, the command exactly as it was given.
fn command_failed(name: &[u8], command: &OsStr, what: &str, status: u8) -> Fail {
    let mut message = format!("with FILE={}, {what} from command: ", quotef(name)).into_bytes();
    message.extend_from_slice(&os_bytes(command));
    Fail { message, status }
}

/// The name to run `shell` by, so that it is found the way `execl` finds it:
/// as a path, never by searching `PATH`.
///
/// `Command` searches `PATH` for a name with no slash in it, which `execl` does
/// not do -- so `SHELL=bash` would quietly run `/bin/bash` where upstream runs
/// `./bash`, which normally is not there, and says so. A `./` in front makes
/// the two agree. An empty name is left alone: `execl ("")` fails `ENOENT`, and
/// so does running the empty name, where `./` would name a directory.
fn exec_path(shell: &[u8]) -> OsString {
    if shell.is_empty() || shell.contains(&b'/') {
        return os_from_bytes(shell);
    }
    let mut path = b"./".to_vec();
    path.extend_from_slice(shell);
    os_from_bytes(&path)
}

/// Hand a `--filter` command `SIGPIPE` ignored, as `split` was started.
///
/// Upstream restores the default in its child only when the default is what
/// it found at startup (`default_SIGPIPE`), so a `split` run with the signal
/// ignored passes that on -- and a `yes | head -1` inside the command then
/// sees `EPIPE` and says so, instead of dying of the signal in silence. Rust's
/// `Command` restores the default in every child, so the hook puts back what
/// `split` was given, recorded before the runtime replaced it.
#[cfg(unix)]
fn keep_sigpipe_ignored(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the hook runs in the child between `fork` and `exec`, where only
    // async-signal-safe calls are allowed. It makes one, `signal`, allocates
    // nothing and takes no lock.
    unsafe {
        command.pre_exec(|| {
            // Unchecked, as upstream's own `signal (SIGPIPE, SIG_DFL)` is: it
            // can fail only for a number that is no signal, and 13 is one.
            let _ = libcall::ignore_signal(libcall::SIGPIPE);
            Ok(())
        });
    }
}

/// Off Unix there is no `SIGPIPE` to hand on.
#[cfg(not(unix))]
fn keep_sigpipe_ignored(_command: &mut Command) {}

/// Close a `--filter` command's input, saying if that failed -- `closeout`'s
/// `close (fd)`. Dropping it would close it too, and discard the verdict.
fn close_pipe(pipe: ChildStdin) -> io::Result<()> {
    #[cfg(unix)]
    let file = File::from(std::os::fd::OwnedFd::from(pipe));
    #[cfg(windows)]
    let file = File::from(std::os::windows::io::OwnedHandle::from(pipe));
    stdfd::close(file)
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
    let share = size.checked_div(count).unwrap_or(0);
    let extra = size.checked_rem(count).unwrap_or(0);
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
    let share = size.checked_div(total).unwrap_or(0);
    let extra = size.checked_rem(total).unwrap_or(0);
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
        let Some(slot) = pieces.get_mut(index.checked_rem(count).unwrap_or(0)) else {
            continue;
        };
        if let Some(bytes) = data.get(start..end) {
            slot.extend_from_slice(bytes);
        }
    }
    pieces
}

// ----------------------------------------------------------------- the run

/// The whole input, and what `fstat` said of it -- upstream's `in_stat_buf`,
/// which every output file is compared against before it is truncated.
struct Input {
    data: Vec<u8>,
    meta: Metadata,
}

/// Read the input operand, `-` meaning standard input.
///
/// Every failure after the open names the input the way upstream does, bare
/// and with the errno finishing the sentence: `split: d: Is a directory`. The
/// two chunk modes word a failed read differently, because upstream reads
/// there to learn the size before it splits anything --
/// `split: d: cannot determine file size: Is a directory`.
fn read_input(file: &OsStr, kind: Kind) -> Result<Input, Fail> {
    let named = |e: &io::Error| format!("{}: {}", quotef_os(file), strerror(e));
    let (mut reader, meta): (Box<dyn Read>, Metadata) = if file == OsStr::new("-") {
        let meta = stdin_metadata().map_err(|e| Fail::new(named(&e)))?;
        (Box::new(io::stdin().lock()), meta)
    } else {
        // `quoteaf_os`, not `quote_os`: upstream is
        // `error (EXIT_FAILURE, errno, _("cannot open %s for reading"), quoteaf (infile))`,
        // and `quoteaf` is shell-escape-always, whose marks stay straight in
        // every locale. Measured, GNU split 9.4, `LC_ALL=C.UTF-8`:
        // `split: cannot open 'nosuch' for reading: No such file or directory`.
        let handle = File::open(file).map_err(|e| {
            Fail::new(format!(
                "cannot open {} for reading: {}",
                quoteaf_os(file),
                strerror(&e)
            ))
        })?;
        let meta = handle.metadata().map_err(|e| Fail::new(named(&e)))?;
        (Box::new(handle), meta)
    };
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(|e| {
        Fail::new(if matches!(kind, Kind::ChunkBytes | Kind::ChunkLines) {
            format!(
                "{}: cannot determine file size: {}",
                quotef_os(file),
                strerror(&e)
            )
        } else {
            named(&e)
        })
    })?;
    Ok(Input { data, meta })
}

/// `fstat (STDIN_FILENO)`: what standard input is, through a duplicate of the
/// descriptor so that nothing here owns -- or closes -- descriptor 0.
#[cfg(unix)]
fn stdin_metadata() -> io::Result<Metadata> {
    use std::os::fd::AsFd;
    File::from(io::stdin().as_fd().try_clone_to_owned()?).metadata()
}

#[cfg(windows)]
fn stdin_metadata() -> io::Result<Metadata> {
    use std::os::windows::io::AsHandle;
    File::from(io::stdin().as_handle().try_clone_to_owned()?).metadata()
}

fn run(
    options: &Options,
    file: &OsString,
    prefix: &OsString,
    out: &mut Stream,
) -> Result<(), Fail> {
    if options.piece.is_some() && options.filter.is_some() {
        return Err(SPLIT
            .usage_referring("--filter does not process a chunk extracted to stdout".to_string())
            .into());
    }
    let (width, start, widen) = suffix_plan(options)?;
    let input = read_input(file, options.kind)?;
    let data: &[u8] = &input.data;

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
    let mut emitter = Emitter {
        options,
        sink,
        input_name: file,
        input_meta: &input.meta,
        out,
    };

    if options.kind == Kind::RoundRobin {
        let pieces = round_robin_pieces(data, options.separator, options.units);
        return emit_selected(&mut emitter, options, pieces.iter().map(Vec::as_slice));
    }

    let ranges = match options.kind {
        Kind::Lines => line_pieces(data, options.separator, options.units),
        Kind::Bytes => byte_pieces(data, options.units),
        Kind::LineBytes => line_byte_pieces(data, options.separator, options.units),
        Kind::ChunkBytes => chunk_byte_pieces(data, options.units),
        Kind::ChunkLines => chunk_line_pieces(data, options.separator, options.units),
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

    impl Fail {
        /// The message as text, to compare with a literal. Every message these
        /// tests provoke is text.
        fn text(&self) -> String {
            String::from_utf8(self.message.clone()).unwrap()
        }
    }

    /// `parse_args` with `POSIXLY_CORRECT` pinned off, so that a test putting an
    /// option after an operand does not depend on the environment `cargo test`
    /// inherited. The tests of the variable itself call `super::parse_args`.
    fn parse_args(args: &[OsString]) -> Result<Request, Fail> {
        super::parse_args(args, false)
    }

    /// Measured against GNU on 2026-09-25: `POSIXLY_CORRECT=1 split f -l1` takes
    /// `-l1` for the output prefix, where without the variable it is an option.
    #[test]
    fn posixly_correct_makes_an_option_after_an_operand_an_operand() {
        let argv: Vec<OsString> = ["f", "-l1"].iter().map(OsString::from).collect();
        let Ok(Request::Run(_, _, prefix)) = super::parse_args(&argv, true) else {
            panic!("expected a run");
        };
        assert_eq!(Some(&prefix), argv.get(1));
        let Ok(Request::Run(_, _, prefix)) = super::parse_args(&argv, false) else {
            panic!("expected a run");
        };
        assert_eq!(prefix, OsString::from("x"));
    }

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
        assert!(e.text().starts_with("cannot split in more than one way"));
    }

    #[test]
    fn the_same_mode_twice_is_also_refused() {
        let e = parse(&["-l", "5", "-l", "6"]).unwrap_err();
        assert!(e.text().starts_with("cannot split in more than one way"));
    }

    #[test]
    fn zero_bytes_is_refused_without_an_errno_tail() {
        let e = parse(&["-b", "0"]).unwrap_err();
        assert_eq!(e.text(), "invalid number of bytes: ‘0’");
    }

    #[test]
    fn an_enormous_byte_count_saturates_rather_than_failing() {
        let options = parse(&["-b", "99999999999999999999999999"]).unwrap();
        assert_eq!(options.units, u64::MAX);
    }

    #[test]
    fn lines_take_no_multiplier_suffix() {
        let e = parse(&["-l", "1k"]).unwrap_err();
        assert_eq!(e.text(), "invalid number of lines: ‘1k’");
    }

    #[test]
    fn line_bytes_take_a_multiplier_suffix() {
        let options = parse(&["-C", "1k"]).unwrap();
        assert_eq!(options.units, 1024);
    }

    #[test]
    fn line_bytes_borrows_the_lines_diagnostic() {
        let e = parse(&["-C", "0"]).unwrap_err();
        assert_eq!(e.text(), "invalid number of lines: ‘0’");
    }

    #[test]
    fn chunks_report_the_whole_argument_when_nothing_converted() {
        assert_eq!(
            parse(&["-n", "x/3"]).unwrap_err().text(),
            "invalid number of chunks: ‘x/3’"
        );
        assert_eq!(
            parse(&["-n", "/3"]).unwrap_err().text(),
            "invalid number of chunks: ‘/3’"
        );
    }

    #[test]
    fn chunks_report_only_the_count_when_the_k_converted() {
        assert_eq!(
            parse(&["-n", "2/x"]).unwrap_err().text(),
            "invalid number of chunks: ‘x’"
        );
        assert_eq!(
            parse(&["-n", "3/"]).unwrap_err().text(),
            "invalid number of chunks: ‘’"
        );
    }

    #[test]
    fn only_one_slash_pair_is_recognised() {
        assert_eq!(
            parse(&["-n", "2/3/4"]).unwrap_err().text(),
            "invalid number of chunks: ‘3/4’"
        );
    }

    #[test]
    fn a_chunk_number_outside_the_count_is_its_own_message() {
        assert_eq!(
            parse(&["-n", "0/3"]).unwrap_err().text(),
            "invalid chunk number: ‘0’"
        );
        assert_eq!(
            parse(&["-n", "4/3"]).unwrap_err().text(),
            "invalid chunk number: ‘4’"
        );
        assert_eq!(
            parse(&["-n", "l/0/3"]).unwrap_err().text(),
            "invalid chunk number: ‘0’"
        );
    }

    #[test]
    fn chunks_take_no_multiplier_suffix() {
        assert_eq!(
            parse(&["-n", "2k"]).unwrap_err().text(),
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
            parse(&["-a", "-1"]).unwrap_err().text(),
            "invalid suffix length: ‘-1’: Numerical result out of range"
        );
        assert_eq!(
            parse(&["-a", "x"]).unwrap_err().text(),
            "invalid suffix length: ‘x’"
        );
    }

    #[test]
    fn a_start_value_too_wide_for_the_suffix_is_its_own_message() {
        let options = parse(&["-a", "1", "--numeric-suffixes=95"]).unwrap();
        let e = suffix_plan(&options).unwrap_err();
        assert!(
            e.text()
                .starts_with("numerical suffix start value is too large for the suffix length")
        );
    }

    #[test]
    fn a_suffix_too_narrow_for_the_chunk_count_names_the_width() {
        let options = parse(&["-n", "700", "-a", "2"]).unwrap();
        let e = suffix_plan(&options).unwrap_err();
        assert_eq!(e.text(), "the suffix length needs to be at least 3");
    }

    #[test]
    fn a_separator_must_be_one_character() {
        assert_eq!(
            parse(&["-t", "xy"]).unwrap_err().text(),
            "multi-character separator ‘xy’"
        );
        assert_eq!(
            parse(&["-t", ""]).unwrap_err().text(),
            "empty record separator"
        );
        assert_eq!(
            parse(&["-t", "a", "-t", "b"]).unwrap_err().text(),
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
            e.text()
                .starts_with("invalid suffix ‘/x’, contains directory separator")
        );
    }

    #[test]
    fn a_bad_start_value_is_reported_with_the_value_first() {
        let e = parse(&["--numeric-suffixes=abc"]).unwrap_err();
        assert!(
            e.text()
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
        assert!(e.text().starts_with("extra operand ‘extra’"));
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
        assert!(e.text().starts_with("invalid option -- 'z'"));
    }

    #[test]
    fn an_ambiguous_long_option_lists_the_candidates() {
        let e = parse(&["--num=3"]).unwrap_err();
        assert!(
            e.text().starts_with(
                "option '--num=3' is ambiguous; possibilities: '--number' '--numeric-suffixes'"
            ),
            "{}",
            e.text()
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
