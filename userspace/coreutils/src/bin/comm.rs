//! `comm` — compare two sorted files line by line.
//!
//! ```text
//! comm [-123z] [--check-order] [--nocheck-order]
//!      [--output-delimiter=STR] [--total] [--] FILE1 FILE2
//! ```
//!
//! | Option | Effect |
//! |---|---|
//! | `-1` | suppress column 1 — the lines only in `FILE1` |
//! | `-2` | suppress column 2 — the lines only in `FILE2` |
//! | `-3` | suppress column 3 — the lines in both |
//! | `-z`, `--zero-terminated` | lines end with NUL rather than newline |
//! | `--check-order` | an out-of-order line is fatal, even if everything pairs |
//! | `--nocheck-order` | never check the order |
//! | `--output-delimiter=STR` | put `STR` between columns instead of a tab |
//! | `--total` | add a final `n1 n2 n3 total` row |
//!
//! # What this used to be
//!
//! A parser that recognised `-1`, `-2` and `-3` and nothing else, over a reader
//! that could not survive its own input:
//!
//! - Any all-digit cluster was accepted and the digits it did not know were
//!   dropped, so `-9` and `-0` ran a plain three-column comparison where GNU
//!   answers `comm: invalid option -- '9'`.
//! - `-z`, `--total`, `--output-delimiter`, `--check-order`, `--nocheck-order`,
//!   `--help`, `--version` and `--` were all taken for **file names**, so
//!   `comm --total A B` said `comm: requires exactly two files`.
//! - Both files were read into `Vec<String>` up front — the whole of both,
//!   before one byte of output — with `.lines()`, whose two silent corruptions
//!   are worse than the memory: it drops the `\r` of a CRLF file, and
//!   `map_while(Result::ok)` **truncates the file at the first byte that is not
//!   UTF-8**. A comparison against a truncated file is not a wrong answer with
//!   an error attached; it is a wrong answer that exits 0.
//! - The order of the input was never checked, so unsorted input gave a
//!   silently wrong answer where GNU warns and exits 1.
//! - `let _ = writeln!(…)`: a full disk was a truncated result reported as a
//!   complete one.
//!
//! # The three columns are made of separators, not of padding
//!
//! Column 2 is written as *one* separator then the line; column 3 as *two* then
//! the line. But each of those separators is emitted only if the column to its
//! left is still being printed, which is why `-1` does not leave a blank first
//! column behind — it removes column 1 *and* the separator that stood for it:
//!
//! ```text
//! $ comm A B          $ comm -1 A B
//! a                       b
//!         b               c
//!     c                   d
//!         d
//! ```
//!
//! So the separator count is `0`, `1`, `2` only when all three columns are on;
//! with `-1` it is `0`, `0`, `1`, and with `-12` every surviving line starts at
//! the margin.
//!
//! # `--output-delimiter` may be repeated, but only identically
//!
//! Upstream keeps the argument, not a flag, and refuses a *second* one that
//! disagrees with the first — `comm: multiple output delimiters specified`,
//! exit 1. Two identical ones are accepted. That is why [`Settings`] holds the
//! argument as it was typed rather than the separator it becomes: the check
//! compares what was typed.
//!
//! And an **empty** argument is not "no separator". Upstream sets the length to
//! 1 for it (`col_sep_len = *optarg ? strlen (optarg) : 1`) while leaving the
//! pointer at the empty string, so the byte written is that string's own
//! terminator — a **NUL**. Measured, not deduced:
//!
//! ```text
//! $ comm --output-delimiter= A B | od -An -c
//!    a  \n  \0  \0   b  \n  \0   c  \n  \0  \0   d  \n
//! ```
//!
//! # Sorted order is checked, not required
//!
//! `comm` needs sorted input to give a correct answer, but it does not verify
//! that up front — it notices while merging, and what it does about it is a
//! three-way setting rather than a flag:
//!
//! | | when a line is out of order |
//! |---|---|
//! | default | warn **only if** some line has already failed to pair, then exit 1 at the end |
//! | `--check-order` | fatal at once, exit 1 |
//! | `--nocheck-order` | nothing |
//!
//! The default's "only if something has already failed to pair" is the part
//! worth keeping: two files that are each in some *other* order but agree with
//! each other line for line — `comm D D` where `D` holds `c a b` — produce a
//! correct three-column answer and no complaint at all, because nothing about
//! the disorder ever became visible in the output. Ask with `--check-order` and
//! the same command dies on the second line.
//!
//! Each file is complained about at most once, and the run still ends with a
//! second diagnostic, `comm: input is not in sorted order`, which is what
//! carries the failing status.
//!
//! The check also runs **once more at end of file**, against the last two lines
//! read, because the pairing failure that makes disorder reportable may only
//! have arrived after those two lines were compared and passed.
//!
//! # Every line ends with the delimiter, including the one that did not
//!
//! Input is read as delimited *records*, keeping the delimiter, and a final
//! record that ran into end of file gains one — upstream's
//! `readlinebuffer_delim` does the same. Comparison then uses the record minus
//! that last byte, so a file with no trailing newline compares equal to one
//! that has it, and the output always ends in a delimiter.
//!
//! # Comparison is bytewise, and GNU's is not always
//!
//! Upstream compares with `xmemcoll` — `strcoll` — whenever `LC_COLLATE` names
//! anything but `C`/`POSIX`, and with `memcmp` otherwise. This implementation
//! always compares bytes, which is the same answer under `C` and `C.UTF-8`
//! (whose collation *is* byte order) and a different one under, say,
//! `en_US.UTF-8`. That is not a decision `comm` can make alone: there is no
//! collation table in this tree for anything to consult, and the same table is
//! what `sort`, `join`, `ls` and `[[ < ]]` will each want. See
//! `known-issues.md` →
//! `TD-OILS-THE-FUNMAP-LISTING-IS-SORTED-BYTEWISE-WHERE-BASH-COLLATES`, which
//! asks for exactly that table.
//!
//! # Checked against GNU
//!
//! `scripts/comm-diff.sh` runs both binaries over the same fixtures and
//! compares stdout byte for byte through `od -An -c`, stderr in full, and the
//! exit status. `scripts/comm-probe.py` is the ad-hoc measurement the rows
//! quoted above came from.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::{quote, quotef_os};
use coreutils::stdfd::{self, Stream};
use std::cmp::Ordering;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's
// `comm >&-` as the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const COMM: Program = Program::new("comm", 1);

const USAGE: &str = "usage: comm [-123z] [--check-order] [--nocheck-order] \
                     [--output-delimiter=STR] [--total] [--] FILE1 FILE2";

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it. Measured
/// with `comm --=x`, whose empty prefix matches every entry.
///
/// The first four have no short form at all; upstream gives them pseudo-short
/// codes above `CHAR_MAX` so that one `switch` can handle both kinds. Nothing
/// here needs that trick, but the *table* still has to be in this order.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("check-order", Long::CheckOrder),
    ("nocheck-order", Long::NoCheckOrder),
    ("output-delimiter", Long::OutputDelimiter),
    ("total", Long::Total),
    ("zero-terminated", Long::ZeroTerminated),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    CheckOrder,
    NoCheckOrder,
    OutputDelimiter,
    Total,
    ZeroTerminated,
    Help,
    Version,
}

/// What to do about input that is not sorted. Three states, not a `bool`,
/// because the default is neither of the other two: it warns, but only once
/// some line has failed to pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OrderCheck {
    Default,
    Enabled,
    Disabled,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Run(Settings, OsString, OsString),
    Help,
    Version,
}

/// Everything the options decide.
#[derive(Debug, PartialEq, Eq)]
struct Settings {
    /// Print the lines found only in the first file.
    col1: bool,
    /// Print the lines found only in the second file.
    col2: bool,
    /// Print the lines found in both.
    col3: bool,
    /// `-z`: the byte a line ends with, NUL rather than newline.
    line_delim: u8,
    check: OrderCheck,
    /// `--total`: append the counting row.
    total: bool,
    /// `--output-delimiter`'s argument **as typed**, or `None` if it was never
    /// given. Kept raw because a second one is compared against it verbatim,
    /// and because an empty one is not an empty separator — see
    /// [`Settings::sep`].
    sep_arg: Option<Vec<u8>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            col1: true,
            col2: true,
            col3: true,
            line_delim: b'\n',
            check: OrderCheck::Default,
            total: false,
            sep_arg: None,
        }
    }
}

/// The separator when `--output-delimiter` was not given.
const DEFAULT_SEP: &[u8] = b"\t";
/// The separator `--output-delimiter=` gives: one NUL, not nothing. Upstream
/// leaves the pointer at the empty string and sets the length to 1, so the byte
/// it writes is that string's terminator.
const EMPTY_SEP: &[u8] = &[0];

impl Settings {
    fn sep(&self) -> &[u8] {
        match &self.sep_arg {
            None => DEFAULT_SEP,
            Some(arg) if arg.is_empty() => EMPTY_SEP,
            Some(arg) => arg,
        }
    }
}

/// A failure that ends the run.
#[derive(Debug)]
enum Trouble {
    /// An operand that would not open, or would not read. Both are upstream's
    /// `error (EXIT_FAILURE, errno, "%s", quotef (…))` and both name the file,
    /// which is why a directory — which opens, and fails on the first read —
    /// is reported as `Is a directory` rather than as an open failure.
    Input(OsString, io::Error),
    Write(io::Error),
    /// `--check-order` found file 1 or 2 out of order. Fatal on the spot, so
    /// unlike the default mode's warning this one ends the run where it
    /// happens, with whatever had already been written left written.
    Unsorted(u8),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Input(name, e) => eprintln!("comm: {}: {}", quotef_os(name), strerror(e)),
            Self::Write(e) => stdfd::write_error("comm", e),
            Self::Unsorted(which) => eprintln!("comm: file {which} is not in sorted order"),
        }
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("comm: {}", e.message());
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `--help` and `--version` are writes like any other, so they fail like
    // any other: measured, `comm --help >&-` is
    // `comm: write error: Bad file descriptor` and exits 1.
    let mut out = Stream::stdout();
    let (settings, first, second) = match request {
        Request::Help => return say(out, format!("{USAGE}
").as_bytes()),
        Request::Version => return say(out, b"comm (SlateOS coreutils)
"),
        Request::Run(settings, first, second) => (settings, first, second),
    };

    let mut stdin = io::stdin().lock();

    let outcome = compare_files(&first, &second, &mut stdin, &settings, &mut out);

    // Buffered output has to reach the OS on *every* exit path, including the
    // ones ending in a diagnostic: upstream gets that from
    // `atexit (close_stdout)`, and `--check-order`'s fatal exit is exactly the
    // case where the lines already written must not be lost.
    // The reader having gone away is the one write failure not reported: GNU
    // dies of `SIGPIPE` there and says nothing, and this system has no signal
    // to die of -- see `coreutils::stdfd::reader_gone`. It therefore counts as
    // a flush that succeeded, and the run keeps the status it had earned.
    let flushed = match out.finish() {
        Err(e) if stdfd::reader_gone(&e) => Ok(()),
        verdict => verdict,
    };

    let disordered = match outcome {
        Ok(disordered) => disordered,
        // `close_stdout` runs *after* the diagnostic and overrides its status,
        // so a run that failed for its own reason still reports a standard
        // output that could not take what it had written.
        Err(trouble) => {
            let code = trouble.report();
            return match flushed {
                Ok(()) => code,
                Err(e) => Trouble::Write(e).report(),
            };
        }
    };
    if let Err(e) = flushed {
        return Trouble::Write(e).report();
    }
    if disordered {
        eprintln!("comm: input is not in sorted order");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Say one thing and stop -- `--help` and `--version`.
///
/// The stream is closed here rather than at the end of `main`, because these
/// two return without reaching it -- and closing it is what discovers that
/// there was nowhere to say it.
fn say(mut out: Stream, bytes: &[u8]) -> ExitCode {
    let _ = out.write_all(bytes);
    stdfd::close_stdout("comm", out, ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------- input

/// One operand's stream.
///
/// `Stdin` carries no reader of its own because `comm - -` names the same
/// stream twice, and two independently buffered readers over one file
/// descriptor would each swallow bytes the other needed.
enum Slot {
    Stdin,
    Stream(Box<dyn BufRead>),
}

impl Slot {
    /// Read through whichever stream this slot stands for. A method rather than
    /// a borrow of the reader, so that the shared standard input never has to
    /// be handed out alongside a `&mut Column` that also owns one.
    fn read(&mut self, stdin: &mut dyn BufRead, delim: u8) -> io::Result<Option<Vec<u8>>> {
        match self {
            Self::Stdin => read_record(stdin, delim),
            Self::Stream(reader) => read_record(&mut **reader, delim),
        }
    }
}

/// Read one delimited record, keeping the delimiter and supplying one if the
/// file ended without it. `None` at end of file.
fn read_record(reader: &mut dyn BufRead, delim: u8) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    if reader.read_until(delim, &mut line)? == 0 {
        return Ok(None);
    }
    if line.last() != Some(&delim) {
        line.push(delim);
    }
    Ok(Some(line))
}

/// A line without its delimiter, which is what comparisons and order checks use.
fn body(line: &[u8]) -> &[u8] {
    line.get(..line.len().saturating_sub(1)).unwrap_or_default()
}

/// One of the two files: its stream, the line waiting to be compared, and
/// enough history to check the order.
struct Column {
    name: OsString,
    slot: Slot,
    /// The line waiting to be compared, or `None` at end of file. Includes its
    /// delimiter.
    current: Option<Vec<u8>>,
    /// The line before [`Self::current`].
    prev: Option<Vec<u8>>,
    /// The line before that.
    ///
    /// Upstream rotates four buffers per file and looks two back; two owned
    /// lines say the same thing plainly. The one difference is invisible: after
    /// a single line has been read, upstream's "two back" still points at that
    /// same line, so its end-of-file re-check compares it with itself and finds
    /// it in order. Here `prev2` is simply `None` and the re-check is skipped,
    /// which reaches the same silence by a shorter road.
    prev2: Option<Vec<u8>>,
    /// A file is complained about at most once.
    warned: bool,
}

impl Column {
    /// Open an operand and read its first line. `-` is standard input.
    fn open(name: &OsString, stdin: &mut dyn BufRead, line_delim: u8) -> Result<Self, Trouble> {
        let slot = if name == "-" {
            Slot::Stdin
        } else {
            let file = File::open(name).map_err(|e| Trouble::Input(name.clone(), e))?;
            Slot::Stream(Box::new(BufReader::new(file)))
        };
        let mut column = Self {
            name: name.clone(),
            slot,
            current: None,
            prev: None,
            prev2: None,
            warned: false,
        };
        let first = column.slot.read(stdin, line_delim);
        column.current = first.map_err(|e| Trouble::Input(name.clone(), e))?;
        Ok(column)
    }

    /// Consume the current line and read the next, checking the order as
    /// upstream does — after the read, and *before* the read's own error is
    /// looked at, so a file that both ends abruptly and ends out of order says
    /// so in that order.
    fn advance(
        &mut self,
        stdin: &mut dyn BufRead,
        settings: &Settings,
        which: u8,
        seen_unpairable: bool,
    ) -> Result<(), Trouble> {
        self.prev2 = self.prev.take();
        self.prev = self.current.take();
        let read = self.slot.read(stdin, settings.line_delim);
        let read = read.map_err(|e| Trouble::Input(self.name.clone(), e));
        match read {
            Ok(Some(line)) => {
                self.current = Some(line);
                self.check_order(which, settings, seen_unpairable, false)
            }
            Ok(None) => self.check_order(which, settings, seen_unpairable, true),
            Err(trouble) => {
                self.check_order(which, settings, seen_unpairable, true)?;
                Err(trouble)
            }
        }
    }

    /// Upstream's `check_order`: compare two consecutive lines of this file and
    /// complain if the later one sorts first.
    ///
    /// `at_eof` selects which pair. Normally it is the line just consumed
    /// against the line just read; at end of file there is no new line, so it
    /// is the two lines before that — a re-check, because the pairing failure
    /// that makes disorder reportable may have arrived after they were first
    /// compared.
    fn check_order(
        &mut self,
        which: u8,
        settings: &Settings,
        seen_unpairable: bool,
        at_eof: bool,
    ) -> Result<(), Trouble> {
        let interested = match settings.check {
            OrderCheck::Disabled => false,
            OrderCheck::Enabled => true,
            OrderCheck::Default => seen_unpairable,
        };
        if !interested || self.warned {
            return Ok(());
        }
        let (earlier, later) = if at_eof {
            (&self.prev2, &self.prev)
        } else {
            (&self.prev, &self.current)
        };
        let (Some(earlier), Some(later)) = (earlier, later) else {
            return Ok(());
        };
        if body(earlier) <= body(later) {
            return Ok(());
        }
        if settings.check == OrderCheck::Enabled {
            return Err(Trouble::Unsorted(which));
        }
        eprintln!("comm: file {which} is not in sorted order");
        self.warned = true;
        Ok(())
    }
}

// --------------------------------------------------------------------- output

/// Which column a line belongs in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    First,
    Second,
    Both,
}

/// Write one line in its column, or nothing if that column is suppressed.
///
/// The separators stand for the columns to the *left* that are still being
/// printed, which is what makes `-1` shift everything to the margin rather than
/// leave a gap where column 1 was.
fn write_line<W: Write>(
    line: &[u8],
    class: Class,
    settings: &Settings,
    out: &mut W,
) -> Result<(), Trouble> {
    let leading = match class {
        Class::First => {
            if !settings.col1 {
                return Ok(());
            }
            0
        }
        Class::Second => {
            if !settings.col2 {
                return Ok(());
            }
            usize::from(settings.col1)
        }
        Class::Both => {
            if !settings.col3 {
                return Ok(());
            }
            usize::from(settings.col1).saturating_add(usize::from(settings.col2))
        }
    };
    for _ in 0..leading {
        out.write_all(settings.sep()).map_err(Trouble::Write)?;
    }
    out.write_all(line).map_err(Trouble::Write)
}

/// `--total`'s row: the three counts and the word `total`, separated by the
/// output delimiter and ended by the line delimiter — so `-z` ends it with a
/// NUL, and `--output-delimiter` reaches it too.
fn write_totals<W: Write>(
    counts: [u64; 3],
    settings: &Settings,
    out: &mut W,
) -> Result<(), Trouble> {
    for count in counts {
        out.write_all(count.to_string().as_bytes())
            .map_err(Trouble::Write)?;
        out.write_all(settings.sep()).map_err(Trouble::Write)?;
    }
    out.write_all(b"total").map_err(Trouble::Write)?;
    out.write_all(&[settings.line_delim])
        .map_err(Trouble::Write)
}

// ---------------------------------------------------------------------- merge

fn compare_files<W: Write>(
    first: &OsString,
    second: &OsString,
    stdin: &mut dyn BufRead,
    settings: &Settings,
    out: &mut W,
) -> Result<bool, Trouble> {
    // Opened one at a time, each read from before the next is opened: that is
    // why `comm nosuch nosuch2` names only the first, and why a directory as
    // the first operand is reported before the second operand is looked at.
    let mut left = Column::open(first, stdin, settings.line_delim)?;
    let mut right = Column::open(second, stdin, settings.line_delim)?;
    merge(&mut left, &mut right, stdin, settings, out)
}

/// Merge two opened files. Returns whether a disorder warning was issued, which
/// is what makes the run fail after everything has been written.
fn merge<W: Write>(
    left: &mut Column,
    right: &mut Column,
    stdin: &mut dyn BufRead,
    settings: &Settings,
    out: &mut W,
) -> Result<bool, Trouble> {
    let mut counts = [0u64; 3];
    let mut seen_unpairable = false;

    while left.current.is_some() || right.current.is_some() {
        let order = match (&left.current, &right.current) {
            // A file that has ended loses every comparison from here on, which
            // is how the other one drains.
            (None, _) => Ordering::Greater,
            (_, None) => Ordering::Less,
            (Some(a), Some(b)) => body(a).cmp(body(b)),
        };
        let (class, slot) = match order {
            Ordering::Less => (Class::First, 0usize),
            Ordering::Greater => (Class::Second, 1usize),
            Ordering::Equal => (Class::Both, 2usize),
        };
        if order != Ordering::Equal {
            // "Unpairable" is what turns the default order check on, so it is
            // set before the lines that follow are read.
            seen_unpairable = true;
        }
        if let Some(count) = counts.get_mut(slot) {
            *count = count.saturating_add(1);
        }
        // A matched pair is printed from the *second* file, as upstream does.
        // The two lines are equal but for their delimiters, which the second
        // file's copy carries and the first file's may not.
        let source = if class == Class::First {
            &left.current
        } else {
            &right.current
        };
        if let Some(line) = source {
            write_line(line, class, settings, out)?;
        }

        // The file the line came from steps; on a match, both do. Left first,
        // matching upstream's loop, because when both step it decides which
        // file's disorder warning is printed first.
        if order != Ordering::Greater {
            left.advance(stdin, settings, 1, seen_unpairable)?;
        }
        if order != Ordering::Less {
            right.advance(stdin, settings, 2, seen_unpairable)?;
        }
    }

    if settings.total {
        write_totals(counts, settings, out)?;
    }
    Ok(left.warned || right.warned)
}

// -------------------------------------------------------------------- parsing

fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut settings = Settings::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut at = 0usize;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        if only_operands {
            files.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand.
            files.push(arg.clone());
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) = long_option(body, &bytes, args, &mut at, &mut settings)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, &mut settings)?;
        }
    }

    // Exactly two operands, and upstream names the offending one with `quote()`.
    // For the missing case it names `argv[argc - 1]` rather than the operand —
    // but getopt's permutation has moved the options to the front by then, so
    // with one operand on the line those are the same argument.
    let mut operands = files.into_iter();
    match (operands.next(), operands.next(), operands.next()) {
        (Some(first), Some(second), None) => Ok(Request::Run(settings, first, second)),
        (Some(_), Some(_), Some(extra)) => {
            Err(COMM.usage_referring(format!("extra operand {}", quote(&arg_bytes(&extra)))))
        }
        (Some(only), None, _) => Err(COMM.usage_referring(format!(
            "missing operand after {}",
            quote(&arg_bytes(&only))
        ))),
        (None, _, _) => Err(COMM.usage_referring("missing operand".to_string())),
    }
}

/// One `--name`, `--name=value` or `--name value` argument.
fn long_option(
    body: &[u8],
    whole: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<Option<Request>, getopt::Error> {
    // Split before resolving, so the *name* is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 matches none of
    // them and takes the unrecognised path, reported as the bytes typed.
    let typed = std::str::from_utf8(typed).map_err(|_| COMM.unrecognized_option(whole))?;
    let (name, which) = COMM.resolve_long(typed, whole, LONG_OPTIONS)?;

    if which == Long::OutputDelimiter {
        // A required argument may be written either way —
        // `--output-delimiter=:` or `--output-delimiter :` — so only the last
        // argument on the line can leave it genuinely missing.
        let value = match inline {
            Some(value) => value.to_vec(),
            None => {
                let Some(separate) = args.get(*next) else {
                    return Err(COMM.long_missing_argument(name));
                };
                *next = next.saturating_add(1);
                arg_bytes(separate)
            }
        };
        return set_output_delimiter(&value, settings).map(|()| None);
    }

    if inline.is_some() {
        return Err(COMM.long_unwanted_argument(name));
    }
    match which {
        Long::CheckOrder => settings.check = OrderCheck::Enabled,
        Long::NoCheckOrder => settings.check = OrderCheck::Disabled,
        Long::Total => settings.total = true,
        Long::ZeroTerminated => settings.line_delim = 0,
        Long::Help => return Ok(Some(Request::Help)),
        Long::Version => return Ok(Some(Request::Version)),
        Long::OutputDelimiter => {}
    }
    Ok(None)
}

/// `--output-delimiter`'s argument, which may be repeated only identically.
///
/// The comparison is against the argument *as typed*, so `--output-delimiter=`
/// twice is accepted even though what it means — a NUL — is not what it says.
fn set_output_delimiter(value: &[u8], settings: &mut Settings) -> Result<(), getopt::Error> {
    if let Some(previous) = &settings.sep_arg
        && previous != value
    {
        return Err(COMM.usage("multiple output delimiters specified".to_string()));
    }
    settings.sep_arg = Some(value.to_vec());
    Ok(())
}

/// One `-abc` cluster. No short option here takes an argument, so a cluster
/// never ends early.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
fn short_options(bytes: &[u8], settings: &mut Settings) -> Result<(), getopt::Error> {
    for &c in bytes.get(1..).unwrap_or_default() {
        match c {
            b'1' => settings.col1 = false,
            b'2' => settings.col2 = false,
            b'3' => settings.col3 = false,
            b'z' => settings.line_delim = 0,
            // `-0`, `-4` and the rest of the digits are *not* accepted, which
            // the shipped parser got wrong by treating any all-digit cluster as
            // column suppression.
            _ => return Err(COMM.invalid_option(c)),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// `a b d`, sorted.
    const A: &[u8] = b"a\nb\nd\n";
    /// `b c d`, sorted, sharing `b` and `d` with A.
    const B: &[u8] = b"b\nc\nd\n";
    /// Not sorted: `c a b`.
    const D: &[u8] = b"c\na\nb\n";
    /// Sorted, and pairs with only one line of D.
    const S: &[u8] = b"a\nb\nc\n";

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn settings(items: &[&str]) -> Settings {
        match parse_args(&args(items)) {
            Ok(Request::Run(settings, _, _)) => settings,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn operands(items: &[&str]) -> (OsString, OsString) {
        match parse_args(&args(items)) {
            Ok(Request::Run(_, first, second)) => (first, second),
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn refuse(items: &[&str]) -> String {
        match parse_args(&args(items)) {
            Err(e) => e.sentence,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// A column over bytes held in memory, with its first line already read.
    fn column(data: &[u8], delim: u8) -> Column {
        let mut slot = Slot::Stream(Box::new(io::Cursor::new(data.to_vec())));
        let mut stdin = io::empty();
        let current = slot.read(&mut stdin, delim).unwrap();
        Column {
            name: OsString::from("-"),
            slot,
            current,
            prev: None,
            prev2: None,
            warned: false,
        }
    }

    /// Merge two in-memory files, returning the output and whether a disorder
    /// warning was issued.
    fn output(settings: &Settings, first: &[u8], second: &[u8]) -> (Vec<u8>, bool) {
        let mut left = column(first, settings.line_delim);
        let mut right = column(second, settings.line_delim);
        let mut stdin = io::empty();
        let mut out = Vec::new();
        let disordered = merge(&mut left, &mut right, &mut stdin, settings, &mut out).unwrap();
        (out, disordered)
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    // -------------------------------------------------------------- the merge

    #[test]
    fn three_columns_by_default() {
        let (out, _) = output(&Settings::default(), A, B);
        assert_eq!(text(&out), "a\n\t\tb\n\tc\n\t\td\n");
    }

    #[test]
    fn suppressing_a_column_removes_its_separator_too() {
        let (out, _) = output(&settings(&["-1", "A", "B"]), A, B);
        assert_eq!(text(&out), "\tb\nc\n\td\n");
        let (out, _) = output(&settings(&["-12", "A", "B"]), A, B);
        assert_eq!(text(&out), "b\nd\n");
        let (out, _) = output(&settings(&["-123", "A", "B"]), A, B);
        assert_eq!(text(&out), "");
    }

    #[test]
    fn an_empty_file_leaves_the_other_to_drain() {
        let (out, _) = output(&Settings::default(), A, b"");
        assert_eq!(text(&out), "a\nb\nd\n");
        let (out, _) = output(&Settings::default(), b"", B);
        assert_eq!(text(&out), "\tb\n\tc\n\td\n");
        let (out, _) = output(&Settings::default(), b"", b"");
        assert_eq!(text(&out), "");
    }

    #[test]
    fn a_missing_final_delimiter_is_supplied() {
        // `a z` against `a b c`: `z` sorts last and arrives with no newline.
        let (out, _) = output(&Settings::default(), b"a\nz", S);
        assert_eq!(text(&out), "\t\ta\n\tb\n\tc\nz\n");
    }

    #[test]
    fn the_delimiter_is_not_part_of_the_comparison() {
        // One file ends its last line, the other does not; they still pair.
        let (out, _) = output(&Settings::default(), b"a\n", b"a");
        assert_eq!(text(&out), "\t\ta\n");
    }

    #[test]
    fn a_line_that_is_a_prefix_of_another_sorts_first() {
        let (out, _) = output(&Settings::default(), b"ab\n", b"a\n");
        assert_eq!(text(&out), "\ta\nab\n");
    }

    #[test]
    fn bytes_that_are_not_text_survive() {
        let (out, _) = output(&Settings::default(), b"\xff\n", b"\x80\n");
        assert_eq!(out, b"\t\x80\n\xff\n".to_vec());
    }

    #[test]
    fn zero_terminated_makes_each_file_one_record() {
        let (out, _) = output(&settings(&["-z", "A", "B"]), A, B);
        assert_eq!(out, b"a\nb\nd\n\0\tb\nc\nd\n\0".to_vec());
    }

    // ------------------------------------------------------------- the totals

    #[test]
    fn totals_count_every_line_including_suppressed_ones() {
        let (out, _) = output(&settings(&["--total", "A", "B"]), A, B);
        assert_eq!(text(&out), "a\n\t\tb\n\tc\n\t\td\n1\t1\t2\ttotal\n");
        let (out, _) = output(&settings(&["--total", "-12", "A", "B"]), A, B);
        assert_eq!(text(&out), "b\nd\n1\t1\t2\ttotal\n");
    }

    #[test]
    fn totals_of_two_empty_files_are_zero() {
        let (out, _) = output(&settings(&["--total", "A", "B"]), b"", b"");
        assert_eq!(text(&out), "0\t0\t0\ttotal\n");
    }

    #[test]
    fn the_totals_row_uses_both_delimiters() {
        let (out, _) = output(&settings(&["-z", "--total", "A", "B"]), A, B);
        assert_eq!(out, b"a\nb\nd\n\0\tb\nc\nd\n\x001\t1\t0\ttotal\0".to_vec());
    }

    // --------------------------------------------------------- the separators

    #[test]
    fn output_delimiter_replaces_every_separator() {
        let (out, _) = output(&settings(&["--output-delimiter=::", "A", "B"]), A, B);
        assert_eq!(text(&out), "a\n::::b\n::c\n::::d\n");
    }

    #[test]
    fn an_empty_output_delimiter_is_a_nul_not_nothing() {
        let (out, _) = output(&settings(&["--output-delimiter=", "A", "B"]), A, B);
        assert_eq!(out, b"a\n\0\0b\n\0c\n\0\0d\n".to_vec());
    }

    #[test]
    fn the_same_output_delimiter_twice_is_allowed() {
        let s = settings(&["--output-delimiter=:", "--output-delimiter=:", "A", "B"]);
        assert_eq!(s.sep(), b":".as_slice());
        let s = settings(&["--output-delimiter=", "--output-delimiter=", "A", "B"]);
        assert_eq!(s.sep(), EMPTY_SEP);
    }

    #[test]
    fn two_different_output_delimiters_are_refused() {
        assert_eq!(
            refuse(&["--output-delimiter=:", "--output-delimiter=;", "A", "B"]),
            "multiple output delimiters specified"
        );
    }

    // -------------------------------------------------------------- the order

    #[test]
    fn disorder_is_ignored_while_everything_pairs() {
        // Both files hold `c a b`; nothing is unpairable, so nothing is said.
        let (out, disordered) = output(&Settings::default(), D, D);
        assert_eq!(text(&out), "\t\tc\n\t\ta\n\t\tb\n");
        assert!(!disordered);
    }

    #[test]
    fn disorder_is_reported_once_a_line_fails_to_pair() {
        let (out, disordered) = output(&Settings::default(), D, S);
        assert_eq!(text(&out), "\ta\n\tb\n\t\tc\na\nb\n");
        assert!(disordered);
    }

    #[test]
    fn nocheck_order_stays_quiet() {
        let (out, disordered) = output(&settings(&["--nocheck-order", "A", "B"]), D, S);
        assert_eq!(text(&out), "\ta\n\tb\n\t\tc\na\nb\n");
        assert!(!disordered);
    }

    #[test]
    fn check_order_is_fatal_at_once() {
        let s = settings(&["--check-order", "A", "B"]);
        let mut left = column(D, s.line_delim);
        let mut right = column(D, s.line_delim);
        let mut stdin = io::empty();
        let mut out = Vec::new();
        // The two files pair line for line, so the default would say nothing.
        match merge(&mut left, &mut right, &mut stdin, &s, &mut out) {
            Err(Trouble::Unsorted(1)) => {}
            other => panic!("expected file 1 to be fatal, got {other:?}"),
        }
        assert_eq!(text(&out), "\t\tc\n");
    }

    // ------------------------------------------------------------- the parser

    #[test]
    fn short_options_cluster() {
        let s = settings(&["-12z", "A", "B"]);
        assert!(!s.col1 && !s.col2 && s.col3);
        assert_eq!(s.line_delim, 0);
    }

    #[test]
    fn digits_other_than_one_two_three_are_invalid() {
        assert_eq!(refuse(&["-0", "A", "B"]), "invalid option -- '0'");
        assert_eq!(refuse(&["-4", "A", "B"]), "invalid option -- '4'");
        assert_eq!(refuse(&["-9", "A", "B"]), "invalid option -- '9'");
    }

    #[test]
    fn long_options_abbreviate() {
        assert!(settings(&["--tot", "A", "B"]).total);
        assert_eq!(settings(&["--check", "A", "B"]).check, OrderCheck::Enabled);
        assert_eq!(settings(&["--noc", "A", "B"]).check, OrderCheck::Disabled);
        assert_eq!(settings(&["--out", ":", "A", "B"]).sep(), b":".as_slice());
    }

    #[test]
    fn an_empty_abbreviation_lists_the_whole_table_in_gnus_order() {
        assert_eq!(
            refuse(&["--=x", "A", "B"]),
            "option '--=x' is ambiguous; possibilities: '--check-order' \
             '--nocheck-order' '--output-delimiter' '--total' \
             '--zero-terminated' '--help' '--version'"
        );
    }

    #[test]
    fn the_options_that_take_nothing_say_so() {
        assert_eq!(
            refuse(&["--total=x", "A", "B"]),
            "option '--total' doesn't allow an argument"
        );
        assert_eq!(
            refuse(&["--output-delimiter"]),
            "option '--output-delimiter' requires an argument"
        );
    }

    #[test]
    fn unknown_options() {
        assert_eq!(refuse(&["-Q", "A", "B"]), "invalid option -- 'Q'");
        assert_eq!(
            refuse(&["--nope", "A", "B"]),
            "unrecognized option '--nope'"
        );
    }

    #[test]
    fn operands_are_counted_exactly() {
        assert_eq!(refuse(&[]), "missing operand");
        assert_eq!(refuse(&["A"]), "missing operand after ‘A’");
        assert_eq!(refuse(&["-z", "A"]), "missing operand after ‘A’");
        assert_eq!(refuse(&["A", "-z"]), "missing operand after ‘A’");
        assert_eq!(
            refuse(&["--output-delimiter", ":", "A"]),
            "missing operand after ‘A’"
        );
        assert_eq!(refuse(&["A", "B", "C"]), "extra operand ‘C’");
        assert_eq!(refuse(&["A", "B", "C", "D"]), "extra operand ‘C’");
    }

    #[test]
    fn double_dash_ends_the_options() {
        let (first, second) = operands(&["--", "-1", "-2"]);
        assert_eq!(first, OsString::from("-1"));
        assert_eq!(second, OsString::from("-2"));
        // ...and a lone `-` is standard input, an operand, not an option.
        let (first, second) = operands(&["-", "A"]);
        assert_eq!(first, OsString::from("-"));
        assert_eq!(second, OsString::from("A"));
    }

    #[test]
    fn help_and_version_win_over_a_bad_operand_count() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn a_bad_option_beats_a_bad_operand_count() {
        // The option loop runs to completion first, so `comm -Q` is an invalid
        // option rather than a missing operand.
        assert_eq!(refuse(&["-Q"]), "invalid option -- 'Q'");
    }
}
