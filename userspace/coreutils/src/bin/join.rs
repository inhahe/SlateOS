//! join — pair up the lines of two sorted files that share a field.
//!
//! # What this used to be
//!
//! The shipped `join` recognised six options, all of them only as separate
//! whole arguments. `-a 1` worked and `-a1` did not; `-t:` did not; `-1` and
//! `-2` did not accept `-12`. Everything else GNU has — `-i`, `-j`, `-v`, `-z`,
//! `--header`, `--check-order`, `--nocheck-order`, `--help`, `--version`, and a
//! bare `--` — came back as `join: unknown option: -i`, a sentence glibc never
//! prints. Beyond the parser it was wrong in ways that changed *answers*, not
//! just diagnostics:
//!
//! - `-o auto` was accepted and then ignored, so the padding it exists to
//!   produce never appeared.
//! - `-e`'s filler was substituted **before** the comparison, so `-e X` decided
//!   which lines paired rather than only how they printed.
//! - An unpairable line was reprinted as its fields rejoined with a single
//!   separator, which moved the join field out of first position and collapsed
//!   runs of blanks that the original line had.
//! - Input was read with `BufRead::lines`, so a `\r\n` file lost its `\r`, and
//!   one byte that was not UTF-8 anywhere in either file ended the run with
//!   `join: read error: stream did not contain valid UTF-8`.
//! - Every write went through `let _ = writeln!(…)`, so a full disk or a closed
//!   pipe was silent and the exit status was still 0.
//! - Nothing checked the input order, which is `join`'s one diagnostic about
//!   the data itself.
//!
//! # Options are read in order, and an operand may turn out to be an argument
//!
//! `join`'s option string starts with `-` (`"-a:e:i1:2:j:o:t:v:z"`), which puts
//! `getopt_long` in RETURN_IN_ORDER mode: operands are **not** permuted to the
//! end, they arrive interleaved with the options in the order typed. That is
//! not a detail — `join` uses the interleaving to support two obsolescent
//! spellings, and it does so by *retroactively reinterpreting* an operand once
//! a third one shows up:
//!
//! | Command | What happens |
//! |---|---|
//! | `join -j1 3 A B` | `3` is `-j1`'s argument, i.e. `-1 3` |
//! | `join -o 1.1 2.2 A B` | `2.2` continues the `-o` list |
//! | `join -j 1 -j1 A B` | nothing follows `-j1` to claim, so it means `-j 1` |
//!
//! Upstream keeps two operand slots and a status for each — "must be an
//! operand", "might be `-j1`'s argument", "might be `-j2`'s argument", "might
//! be `-o`'s argument" — and when a third name arrives it goes back to the
//! first slot that is not settled, consumes it as the pending option's
//! argument, and shifts. [`Parse::add_file_name`] is that machine, transcribed.
//! It is why
//!
//! ```text
//! $ join -o 1.1 A B C
//! join: invalid file number in field spec: ‘A’
//! ```
//!
//! reports `A` — the name that was never an operand at all — rather than
//! complaining about `C`.
//!
//! # A line is cut into fields in one of three ways
//!
//! `-t` decides, and the third way is easy to miss:
//!
//! | `-t` | Fields |
//! |---|---|
//! | not given | runs of blanks separate; leading blanks are skipped |
//! | `-t CHAR` | every single occurrence of `CHAR` separates |
//! | `-t ''` or `-t` newline | there is one field: the whole line |
//!
//! The third row is what the manual page means by "use `join -t ''` if `sort`
//! has no options": with no separator the join field is the entire line, which
//! is what a plain `sort` ordered. An empty `-t` argument is stored as `'\n'`,
//! which is also why `-t ''` and `-t $'\n'` are compatible with each other but
//! not with anything else.
//!
//! In the blank-separated mode a *trailing* blank produces a trailing **empty**
//! field, because upstream's loop leaves the cursor at the end of the line and
//! then extracts one more zero-length field. `"a "` therefore has two fields,
//! not one.
//!
//! # A field that is absent and a field that is empty are the same field
//!
//! `keycmp` gives a join field past the end of its line a length of 0, and then
//! treats length 0 as sorting before everything except another length 0. So a
//! line too short to have a join field compares equal to a line whose join
//! field is present but empty, and both sort first. That is the whole
//! explanation for
//!
//! ```text
//! $ join -1 99999999999999999999 A B      # status 0, no output
//! ```
//!
//! — the field number is clamped to `PTRDIFF_MAX` rather than rejected, so
//! every line of `A` has an empty key, every comparison says "less", and every
//! line of `A` is unpairable. Only `-1 0`, a non-number, or trailing junk is an
//! error.
//!
//! # `-o`, and what `auto` counts
//!
//! Without `-o` a joined line is: the join field, then the other fields of
//! file 1, then the other fields of file 2. With `-o` it is exactly the listed
//! fields, where `0` means "the join field". `-o auto` is neither: it keeps the
//! default layout but fixes the field *count* of each file to that of its own
//! first line, so an unpairable line still emits the full width — padded with
//! `-e`'s filler if there is one, and with nothing but separators if there is
//! not:
//!
//! ```text
//! $ join -o auto -a 1 A B          $ join -o auto -a 1 -e X A B
//! a 1                              a 1 X
//! b 2 x                            b 2 x
//! d 4 z                            d 4 z
//! ```
//!
//! `-e`'s filler also replaces a field that exists but is *empty*, not only one
//! that is missing — and it is applied at print time only, never before a
//! comparison.
//!
//! # Sorted order is checked, not required
//!
//! The default is neither "check" nor "don't": nothing is said until some line
//! has failed to pair, on the theory that unsorted input which nevertheless
//! pairs completely was probably grouped on purpose. After that first
//! unpairable line, the first descent in either file is reported — once per
//! file — and the run ends with `input is not in sorted order` and status 1
//! even though the output was written. `--check-order` reports from the start
//! and dies on the spot; `--nocheck-order` says nothing.
//!
//! ```text
//! $ join D S                       # D is c,a,b and S is a,b,c
//! join: D:2: is not sorted: a
//! c
//! join: input is not in sorted order
//! ```
//!
//! That message is the one place a file name and a line of input data go into a
//! diagnostic **raw**, with no quoting, and this transcription keeps it that
//! way. Quoting the name alone would buy nothing — the line beside it is
//! attacker-controlled either way — and quoting the line would change the shape
//! of every ordinary warning. What upstream does get, by accident, is that
//! `%.*s` stops at a NUL, so the printed line is truncated at its first NUL
//! byte; [`disorder_message`] reproduces that rather than the length arithmetic
//! alone. Note also that only a trailing `'\n'` is stripped, literally, not the
//! `-z` delimiter.
//!
//! # Comparison is bytewise, and GNU's is not always
//!
//! Upstream calls `xmemcoll` when `LC_COLLATE` names a locale that is not `C`,
//! so in a UTF-8 locale `join` orders keys by the locale's collation. We have
//! no collation tables, so keys are compared as bytes — the same divergence,
//! and the same reason, as the rest of this crate. `-i` folds ASCII case only,
//! which is what upstream's `memcasecmp` does in the C locale (upstream has a
//! FIXME saying so).
//!
//! # Checked against GNU
//!
//! `scripts/join-diff.sh` runs this binary and glibc's `join` (through WSL)
//! over the same fixtures and byte-compares stdout, stderr and status. The rows
//! quoted above are from that harness's fixtures under `LC_ALL=C`.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::{os_bytes, quote, quoteaf_os, quotef_os};
use coreutils::stdfd::{self, Stream};
use std::cmp::Ordering;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's
// `join >&-` as the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const JOIN: Program = Program::new("join", 1);

const USAGE: &str = "usage: join [-a FILENUM] [-e EMPTY] [-i] [-j FIELD] [-o FORMAT] \
                     [-t CHAR] [-v FILENUM] [-1 FIELD] [-2 FIELD] [-z] [--check-order] \
                     [--nocheck-order] [--header] [--] FILE1 FILE2";

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it. Measured
/// with `join --=x`, whose empty prefix matches every entry:
///
/// ```text
/// join: option '--=x' is ambiguous; possibilities: '--ignore-case' '--check-order'
///       '--nocheck-order' '--zero-terminated' '--header' '--help' '--version'
/// ```
///
/// Every one of them takes no argument, so there is no `Takes` column here.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("ignore-case", Long::IgnoreCase),
    ("check-order", Long::CheckOrder),
    ("nocheck-order", Long::NoCheckOrder),
    ("zero-terminated", Long::ZeroTerminated),
    ("header", Long::Header),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    IgnoreCase,
    CheckOrder,
    NoCheckOrder,
    ZeroTerminated,
    Header,
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

/// One item of `-o`'s list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OutSpec {
    /// `0`: whichever file's join field is the one being printed on this line.
    JoinField,
    /// `1.N` or `2.N`, with `index` already zero-based.
    Field { file: u8, index: usize },
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
    /// The zero-based join field of each file. Resolved from the several ways
    /// of naming it (`-1`, `-2`, `-j`, `-j1`, `-j2`) only once parsing is over.
    jf: [usize; 2],
    /// `-a 1` / `-a 2`: also print the lines of that file that paired with
    /// nothing.
    unpairable: [bool; 2],
    /// Cleared by `-v`, which is `-a` plus "and print nothing else".
    pairables: bool,
    /// `-i`: fold ASCII case when comparing join fields.
    ignore_case: bool,
    /// `--header`: pair the two first lines unconditionally and restart the
    /// order check behind them.
    header: bool,
    /// `-z`: the byte a line ends with, NUL rather than newline.
    eol: u8,
    /// `-t`'s character, or `None` for the default "runs of blanks". `'\n'`
    /// here means "one field, the whole line" — see the module docs.
    tab: Option<u8>,
    /// `-e`'s replacement for an absent or empty field, at print time only.
    empty_filler: Option<Vec<u8>>,
    /// `-o`'s list, empty when it was never given.
    outlist: Vec<OutSpec>,
    /// `-o auto`: take each file's field count from its own first line.
    autoformat: bool,
    check: OrderCheck,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            jf: [0, 0],
            unpairable: [false, false],
            pairables: true,
            ignore_case: false,
            header: false,
            eol: b'\n',
            tab: None,
            empty_filler: None,
            outlist: Vec::new(),
            autoformat: false,
            check: OrderCheck::Default,
        }
    }
}

impl Settings {
    /// The byte written between output fields: `-t`'s character, or a space
    /// when fields were separated by runs of blanks.
    fn out_sep(&self) -> u8 {
        self.tab.unwrap_or(b' ')
    }
}

/// A failure that ends the run.
#[derive(Debug)]
enum Trouble {
    /// An operand that would not open. Upstream's
    /// `error (EXIT_FAILURE, errno, "%s", quotef (…))`.
    Open(OsString, io::Error),
    /// Upstream's `read error`, which names no file — the two streams are
    /// interchangeable by the time it can happen.
    Read(io::Error),
    Write(io::Error),
    /// Both operands were `-`. Upstream passes `errno` here but it is 0 at that
    /// point, so nothing is appended.
    BothStdin,
    /// `--check-order` found a descent. Fatal on the spot, so unlike the
    /// default mode's warning this ends the run where it happens, with whatever
    /// had already been written left written. Carries the whole sentence
    /// because it embeds a raw line of input.
    Unsorted(Vec<u8>),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Open(name, e) => eprintln!("join: {}: {}", quotef_os(name), strerror(e)),
            Self::Read(e) => eprintln!("join: read error: {}", strerror(e)),
            Self::Write(e) => stdfd::write_error("join", e),
            Self::BothStdin => eprintln!("join: both files cannot be standard input"),
            Self::Unsorted(sentence) => diagnose(sentence),
        }
        ExitCode::FAILURE
    }
}

/// Write one diagnostic whose body is raw bytes rather than text.
///
/// The order-check message embeds a line of the input verbatim, which no
/// `String` can hold and no quoting may touch (see the module docs), so it
/// cannot go through `eprintln!`.
fn diagnose(body: &[u8]) {
    let mut err = io::stderr().lock();
    // A failed write to stderr has nowhere left to be reported, and upstream
    // ignores it too: `error()` checks nothing.
    let _ = err.write_all(b"join: ");
    let _ = err.write_all(body);
    let _ = err.write_all(b"\n");
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("join: {}", e.message());
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `--help` and `--version` are writes like any other, so they fail like
    // any other: measured, `join --help >&-` is
    // `join: write error: Bad file descriptor` and exits 1.
    let mut out = Stream::stdout();
    let (settings, first, second) = match request {
        Request::Help => return say(out, format!("{USAGE}
").as_bytes()),
        Request::Version => return say(out, b"join (SlateOS coreutils)
"),
        Request::Run(settings, first, second) => (settings, first, second),
    };
    let outcome = run(&settings, &first, &second, &mut out);

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
        eprintln!("join: input is not in sorted order");
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
    stdfd::close_stdout("join", out, ExitCode::SUCCESS)
}

/// Open both operands and join them. `true` if either file was warned about.
fn run<W: Write>(
    settings: &Settings,
    first: &OsStr,
    second: &OsStr,
    out: &mut W,
) -> Result<bool, Trouble> {
    // Checked before opening rather than after, as upstream does, because the
    // only way two `FILE *` can be equal is for both to be standard input — and
    // in that case neither open could have failed, so the order is not
    // observable.
    if first == "-" && second == "-" {
        return Err(Trouble::BothStdin);
    }
    let mut in1 = Input::open(first, 1)?;
    let mut in2 = Input::open(second, 2)?;
    join(out, settings, &mut in1, &mut in2)?;
    Ok(in1.warned || in2.warned)
}

// ---------------------------------------------------------------------- input

/// One record, and where its fields sit inside it.
///
/// The record keeps its delimiter, as upstream's buffer does; the fields never
/// include it, because `xfields` works up to `length - 1`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    record: Vec<u8>,
    fields: Vec<(usize, usize)>,
}

impl Line {
    /// Upstream's `uni_blank`: the stand-in for "the other file had no line
    /// here". Zero fields, so every field of it is absent.
    fn blank() -> Self {
        Self {
            record: Vec::new(),
            fields: Vec::new(),
        }
    }

    fn nfields(&self) -> usize {
        self.fields.len()
    }

    /// Field `n`, or `None` if the line is too short to have one.
    fn field(&self, n: usize) -> Option<&[u8]> {
        self.fields
            .get(n)
            .map(|&(start, end)| self.record.get(start..end).unwrap_or_default())
    }

    /// The join field's bytes, with an absent field rendered as the empty
    /// slice — which is exactly how `keycmp` treats it.
    fn key(&self, n: usize) -> &[u8] {
        self.field(n).unwrap_or_default()
    }
}

/// One of the two files: its stream, its name for diagnostics, and the little
/// history the order check needs.
struct Input {
    name: OsString,
    reader: Box<dyn BufRead>,
    /// 1 or 2, which is what the diagnostics and the settings arrays index by.
    which: u8,
    line_no: u64,
    /// The *key* of the previous line, which is all `keycmp` would have read
    /// from the line itself. `--header` clears it so the header cannot be
    /// compared against the first real line.
    prev_key: Option<Vec<u8>>,
    /// Upstream's `issued_disorder_warning`: a file is complained about once.
    warned: bool,
}

impl Input {
    fn open(name: &OsStr, which: u8) -> Result<Self, Trouble> {
        let reader: Box<dyn BufRead> = if name == "-" {
            Box::new(io::stdin().lock())
        } else {
            let file = File::open(name).map_err(|e| Trouble::Open(name.to_os_string(), e))?;
            Box::new(BufReader::new(file))
        };
        Ok(Self {
            name: name.to_os_string(),
            reader,
            which,
            line_no: 0,
            prev_key: None,
            warned: false,
        })
    }

    fn join_field(&self, settings: &Settings) -> usize {
        if self.which == 1 {
            settings.jf[0]
        } else {
            settings.jf[1]
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

/// Read the next line of `input`, split it, and check the order behind it.
///
/// `seen_unpairable` is passed in rather than read from a field because *when*
/// it is set is observable: upstream sets it immediately **after** the advance
/// that discovered the unpairable line, so the line that advance read is still
/// checked under the old value.
fn get_line(
    input: &mut Input,
    settings: &Settings,
    seen_unpairable: bool,
) -> Result<Option<Line>, Trouble> {
    let Some(record) = read_record(&mut *input.reader, settings.eol).map_err(Trouble::Read)? else {
        return Ok(None);
    };
    input.line_no = input.line_no.saturating_add(1);
    let fields = split_fields(&record, settings.tab);
    let line = Line { record, fields };
    let key = line.key(input.join_field(settings)).to_vec();

    // Taken out rather than borrowed: `check_order` needs `input` mutably (it
    // records that this file has been warned about), and the previous key is
    // overwritten immediately afterwards either way.
    let previous = input.prev_key.take();
    if let Some(previous) = previous {
        check_order(input, settings, seen_unpairable, &previous, &key, &line)?;
    }
    input.prev_key = Some(key);
    Ok(Some(line))
}

/// Upstream's `check_order`, including the two conditions that keep it quiet.
fn check_order(
    input: &mut Input,
    settings: &Settings,
    seen_unpairable: bool,
    previous: &[u8],
    key: &[u8],
    line: &Line,
) -> Result<(), Trouble> {
    let checking = settings.check != OrderCheck::Disabled
        && (settings.check == OrderCheck::Enabled || seen_unpairable);
    if !checking || input.warned {
        return Ok(());
    }
    if keycmp(previous, key, settings.ignore_case) != Ordering::Greater {
        return Ok(());
    }
    let sentence = disorder_message(&input.name, input.line_no, &line.record);
    if settings.check == OrderCheck::Enabled {
        return Err(Trouble::Unsorted(sentence));
    }
    diagnose(&sentence);
    input.warned = true;
    Ok(())
}

/// `NAME:LINE: is not sorted: TEXT`, with upstream's two truncations.
///
/// Upstream computes a length by stripping one trailing `'\n'` — the literal
/// newline, *not* `-z`'s delimiter — and capping at `INT_MAX`, then hands the
/// buffer to `%.*s`, which also stops at the first NUL. All three apply.
fn disorder_message(name: &OsStr, line_no: u64, record: &[u8]) -> Vec<u8> {
    let mut len = record.len();
    if record.last() == Some(&b'\n') {
        len = len.saturating_sub(1);
    }
    len = len.min(i32::MAX as usize);
    let mut text = record.get(..len).unwrap_or_default();
    if let Some(nul) = text.iter().position(|&c| c == 0) {
        text = text.get(..nul).unwrap_or_default();
    }
    let mut sentence = os_bytes(name).into_owned();
    sentence.extend_from_slice(format!(":{line_no}: is not sorted: ").as_bytes());
    sentence.extend_from_slice(text);
    sentence
}

/// Is this byte one of the ones that separates fields when `-t` was not given?
///
/// Upstream's `field_sep` is `isblank (ch) || ch == '\n'`, and the newline is
/// not redundant: under `-z` a record may hold newlines.
fn field_sep(ch: u8) -> bool {
    ch == b' ' || ch == b'\t' || ch == b'\n'
}

/// Upstream's `xfields`, transcribed: cut `record` — delimiter included — into
/// the half-open ranges of its fields.
fn split_fields(record: &[u8], tab: Option<u8>) -> Vec<(usize, usize)> {
    let mut fields: Vec<(usize, usize)> = Vec::new();
    // Everything works up to `length - 1`, which is the delimiter.
    let lim = record.len().saturating_sub(1);
    let mut ptr = 0usize;
    if ptr == lim {
        return fields;
    }

    match tab {
        // A single character separates, and every occurrence of it separates:
        // `a::b` is four fields with `-t:`, not three.
        Some(sep_byte) if sep_byte != b'\n' => {
            while let Some(offset) = record
                .get(ptr..lim)
                .and_then(|rest| rest.iter().position(|&c| c == sep_byte))
            {
                let sep = ptr.saturating_add(offset);
                fields.push((ptr, sep));
                ptr = sep.saturating_add(1);
            }
        }
        // Runs of blanks separate, and leading blanks belong to no field.
        None => {
            while field_sep(record.get(ptr).copied().unwrap_or(0)) {
                ptr = ptr.saturating_add(1);
                if ptr == lim {
                    return fields;
                }
            }
            loop {
                let mut sep = ptr.saturating_add(1);
                while sep != lim && !field_sep(record.get(sep).copied().unwrap_or(0)) {
                    sep = sep.saturating_add(1);
                }
                fields.push((ptr, sep));
                if sep == lim {
                    return fields;
                }
                ptr = sep.saturating_add(1);
                while ptr != lim && field_sep(record.get(ptr).copied().unwrap_or(0)) {
                    ptr = ptr.saturating_add(1);
                }
                // Leaving the loop here rather than after another field is what
                // gives a trailing blank a trailing *empty* field: the fall-out
                // below extracts one of zero length.
                if ptr == lim {
                    break;
                }
            }
        }
        // `-t` given a newline (which `-t ''` also means) matches neither arm
        // above, so control reaches the fall-out with `ptr` still 0 and the
        // whole line becomes one field.
        Some(_) => {}
    }

    fields.push((ptr, lim));
    fields
}

/// Upstream's `keycmp`, on the two keys rather than the two lines.
///
/// An absent field arrives here as an empty slice, which is deliberate: `join`
/// gives a missing field length 0 and then compares lengths, so a line too
/// short to have a join field and a line whose join field is empty are equal to
/// each other and less than everything else.
fn keycmp(a: &[u8], b: &[u8], ignore_case: bool) -> Ordering {
    if a.is_empty() {
        return if b.is_empty() {
            Ordering::Equal
        } else {
            Ordering::Less
        };
    }
    if b.is_empty() {
        return Ordering::Greater;
    }
    if ignore_case {
        // `memcasecmp` over the shorter length, then length as the tiebreak.
        // ASCII only, which is what upstream's does in the C locale — it
        // carries a FIXME saying multibyte case folding is not handled.
        for (x, y) in a.iter().zip(b.iter()) {
            let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
            if order != Ordering::Equal {
                return order;
            }
        }
        return a.len().cmp(&b.len());
    }
    // `memcmp` over the shorter length then the length tiebreak *is* slice
    // ordering; the two are the same function written twice.
    a.cmp(b)
}

// --------------------------------------------------------------------- output

/// Upstream's `prfield`: field `n` of `line`, or the `-e` filler when the field
/// is absent **or** empty.
fn prfield<W: Write>(out: &mut W, settings: &Settings, n: usize, line: &Line) -> io::Result<()> {
    match line.field(n) {
        Some(bytes) if !bytes.is_empty() => out.write_all(bytes),
        _ => match &settings.empty_filler {
            Some(filler) => out.write_all(filler),
            None => Ok(()),
        },
    }
}

/// Upstream's `prfields`: every field of `line` except the join field, each
/// preceded by a separator.
///
/// The count is `autocount` under `-o auto` and the line's own otherwise, which
/// is the whole of what `auto` does: a short line still emits the full width.
fn prfields<W: Write>(
    out: &mut W,
    settings: &Settings,
    line: &Line,
    join_field: usize,
    autocount: usize,
) -> io::Result<()> {
    let nfields = if settings.autoformat {
        autocount
    } else {
        line.nfields()
    };
    let sep = [settings.out_sep()];
    for i in 0..join_field.min(nfields) {
        out.write_all(&sep)?;
        prfield(out, settings, i, line)?;
    }
    for i in join_field.saturating_add(1)..nfields {
        out.write_all(&sep)?;
        prfield(out, settings, i, line)?;
    }
    Ok(())
}

/// Upstream's `prjoin`. `None` for either line is `uni_blank` — the file that
/// had nothing to pair with.
fn prjoin<W: Write>(
    out: &mut W,
    settings: &Settings,
    auto: [usize; 2],
    line1: Option<&Line>,
    line2: Option<&Line>,
) -> io::Result<()> {
    let blank = Line::blank();
    let l1 = line1.unwrap_or(&blank);
    let l2 = line2.unwrap_or(&blank);
    // The join field comes from file 1 unless file 1 is the blank one, which is
    // how an unpairable line of file 2 still leads with its key.
    let (key_line, key_field) = if line1.is_none() {
        (l2, settings.jf[1])
    } else {
        (l1, settings.jf[0])
    };
    let sep = [settings.out_sep()];

    if settings.outlist.is_empty() {
        prfield(out, settings, key_field, key_line)?;
        prfields(out, settings, l1, settings.jf[0], auto[0])?;
        prfields(out, settings, l2, settings.jf[1], auto[1])?;
    } else {
        for (i, spec) in settings.outlist.iter().enumerate() {
            if i != 0 {
                out.write_all(&sep)?;
            }
            match *spec {
                OutSpec::JoinField => prfield(out, settings, key_field, key_line)?,
                OutSpec::Field { file, index } => {
                    let line = if file == 1 { l1 } else { l2 };
                    prfield(out, settings, index, line)?;
                }
            }
        }
    }
    out.write_all(&[settings.eol])
}

// ---------------------------------------------------------------------- merge

/// Upstream's `join`, with its `struct seq` bookkeeping unrolled.
///
/// Upstream collects a run of equal-keyed lines into a growable array and then
/// reads one line too many to know the run has ended, incrementing `count` even
/// at end of file so that the run is always `count - 1` long and the extra
/// entry is the lookahead. A `Vec` of the run plus a separate `next` says the
/// same thing: both of upstream's exits — swap the lookahead to the front and
/// set `count = 1`, or set `count = 0` — are `cur = next`.
fn join<W: Write>(
    out: &mut W,
    settings: &Settings,
    in1: &mut Input,
    in2: &mut Input,
) -> Result<(), Trouble> {
    let mut seen_unpairable = false;
    let mut cur1 = get_line(in1, settings, seen_unpairable)?;
    let mut cur2 = get_line(in2, settings, seen_unpairable)?;

    // Fixed before the header is printed, so `--header` and `-o auto` together
    // count the header's own fields.
    let auto = if settings.autoformat {
        [
            cur1.as_ref().map_or(0, Line::nfields),
            cur2.as_ref().map_or(0, Line::nfields),
        ]
    } else {
        [0, 0]
    };

    if settings.header && (cur1.is_some() || cur2.is_some()) {
        prjoin(out, settings, auto, cur1.as_ref(), cur2.as_ref()).map_err(Trouble::Write)?;
        // The header is not part of the sorted sequence, so nothing after it is
        // compared against it.
        in1.prev_key = None;
        in2.prev_key = None;
        if cur1.is_some() {
            cur1 = get_line(in1, settings, seen_unpairable)?;
        }
        if cur2.is_some() {
            cur2 = get_line(in2, settings, seen_unpairable)?;
        }
    }

    while let (Some(l1), Some(l2)) = (cur1.as_ref(), cur2.as_ref()) {
        let order = keycmp(
            l1.key(settings.jf[0]),
            l2.key(settings.jf[1]),
            settings.ignore_case,
        );

        if order == Ordering::Less {
            if settings.unpairable[0]
                && let Some(l1) = &cur1
            {
                prjoin(out, settings, auto, Some(l1), None).map_err(Trouble::Write)?;
            }
            cur1 = get_line(in1, settings, seen_unpairable)?;
            seen_unpairable = true;
            continue;
        }
        if order == Ordering::Greater {
            if settings.unpairable[1]
                && let Some(l2) = &cur2
            {
                prjoin(out, settings, auto, None, Some(l2)).map_err(Trouble::Write)?;
            }
            cur2 = get_line(in2, settings, seen_unpairable)?;
            seen_unpairable = true;
            continue;
        }

        let (Some(first1), Some(first2)) = (cur1.take(), cur2.take()) else {
            // Unreachable: `order` was computed from both being present.
            break;
        };
        // Each run is bounded by the *other* file's original line, exactly as
        // upstream compares against `seq2.lines[0]` and `seq1.lines[0]`.
        let key1 = first1.key(settings.jf[0]).to_vec();
        let key2 = first2.key(settings.jf[1]).to_vec();

        let mut run1 = vec![first1];
        let mut next1 = None;
        while let Some(line) = get_line(in1, settings, seen_unpairable)? {
            if keycmp(line.key(settings.jf[0]), &key2, settings.ignore_case) == Ordering::Equal {
                run1.push(line);
            } else {
                next1 = Some(line);
                break;
            }
        }
        let mut run2 = vec![first2];
        let mut next2 = None;
        while let Some(line) = get_line(in2, settings, seen_unpairable)? {
            if keycmp(&key1, line.key(settings.jf[1]), settings.ignore_case) == Ordering::Equal {
                run2.push(line);
            } else {
                next2 = Some(line);
                break;
            }
        }

        if settings.pairables {
            for left in &run1 {
                for right in &run2 {
                    prjoin(out, settings, auto, Some(left), Some(right)).map_err(Trouble::Write)?;
                }
            }
        }
        cur1 = next1;
        cur2 = next2;
    }

    // The tails: whatever is left of the file that did not run out. It is read
    // to the end even with nothing to print, so that the order check sees it —
    // unless both files have already been complained about.
    //
    // Upstream also sets `seen_unpairable` here, twice, and both statements are
    // dead: the loop above only ends when one of the two counts is zero, so the
    // other file's count is zero in each block. They are not transcribed.
    let checktail = settings.check != OrderCheck::Disabled && !(in1.warned && in2.warned);

    if (settings.unpairable[0] || checktail) && cur1.is_some() {
        if settings.unpairable[0]
            && let Some(l1) = &cur1
        {
            prjoin(out, settings, auto, Some(l1), None).map_err(Trouble::Write)?;
        }
        while let Some(line) = get_line(in1, settings, seen_unpairable)? {
            if settings.unpairable[0] {
                prjoin(out, settings, auto, Some(&line), None).map_err(Trouble::Write)?;
            }
            if in1.warned && !settings.unpairable[0] {
                break;
            }
        }
    }

    if (settings.unpairable[1] || checktail) && cur2.is_some() {
        if settings.unpairable[1]
            && let Some(l2) = &cur2
        {
            prjoin(out, settings, auto, None, Some(l2)).map_err(Trouble::Write)?;
        }
        while let Some(line) = get_line(in2, settings, seen_unpairable)? {
            if settings.unpairable[1] {
                prjoin(out, settings, auto, None, Some(&line)).map_err(Trouble::Write)?;
            }
            if in2.warned && !settings.unpairable[1] {
                break;
            }
        }
    }

    Ok(())
}

// -------------------------------------------------------------------- numbers

/// What gnulib's `xstrtoimax` distinguishes, reduced to what `join` acts on.
///
/// The three states are not interchangeable. A clean overflow is *accepted* and
/// clamped, while trailing junk — even trailing junk on a number that also
/// overflowed — is refused. That is why `join -1 99999999999999999999` runs and
/// `join -1 1x` does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Number {
    Value(i64),
    Overflow,
    Invalid,
}

/// `xstrtoimax (s, nullptr, 10, &val, "")`: leading C whitespace, an optional
/// sign, decimal digits, and nothing else.
///
/// The whitespace set is C's `isspace`, which includes the vertical tab that
/// Rust's `is_ascii_whitespace` leaves out.
fn strtoimax(bytes: &[u8]) -> Number {
    let mut at = 0usize;
    while let Some(&c) = bytes.get(at) {
        if c == b' ' || (0x09..=0x0d).contains(&c) {
            at = at.saturating_add(1);
        } else {
            break;
        }
    }
    let negative = match bytes.get(at) {
        Some(&b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(&b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };

    let first_digit = at;
    // Any magnitude past this is out of range whatever the sign, and clamping
    // keeps a line of ten thousand digits from growing the accumulator.
    const CAP: u128 = 1 << 65;
    let mut magnitude: u128 = 0;
    while let Some(&c) = bytes.get(at) {
        if !c.is_ascii_digit() {
            break;
        }
        magnitude = magnitude
            .saturating_mul(10)
            .saturating_add(u128::from(c.wrapping_sub(b'0')));
        magnitude = magnitude.min(CAP);
        at = at.saturating_add(1);
    }

    // No digits at all is `LONGINT_INVALID`; digits followed by anything is
    // `LONGINT_INVALID_SUFFIX_CHAR`. `join` refuses both, and refuses them even
    // when the digits also overflowed — the two codes are OR'd together
    // upstream, and the result is neither `LONGINT_OK` nor `LONGINT_OVERFLOW`.
    if at == first_digit || at != bytes.len() {
        return Number::Invalid;
    }

    let m = i128::try_from(magnitude).unwrap_or(i128::MAX);
    let value = if negative { m.saturating_neg() } else { m };
    match i64::try_from(value) {
        Ok(v) => Number::Value(v),
        Err(_) => Number::Overflow,
    }
}

/// Upstream's `string_to_join_field`: a 1-based field number in, a 0-based
/// index out, with an out-of-range one clamped rather than refused.
fn string_to_join_field(bytes: &[u8]) -> Result<usize, getopt::Error> {
    let value = match strtoimax(bytes) {
        // `PTRDIFF_MAX`, which on every target we build for is `i64::MAX`.
        Number::Overflow => i64::MAX,
        Number::Value(v) if v > 0 => v,
        _ => {
            return Err(JOIN.usage(format!("invalid field number: {}", quote(bytes))));
        }
    };
    Ok(usize::try_from(value.saturating_sub(1)).unwrap_or(usize::MAX))
}

/// `-a`'s and `-v`'s argument, which must be exactly 1 or 2.
///
/// Note the message: upstream reuses `invalid field number` here even though
/// what is wrong is a *file* number.
fn parse_file_number(bytes: &[u8]) -> Result<u8, getopt::Error> {
    match strtoimax(bytes) {
        Number::Value(1) => Ok(1),
        Number::Value(2) => Ok(2),
        _ => Err(JOIN.usage(format!("invalid field number: {}", quote(bytes)))),
    }
}

/// One item of `-o`'s list: `0`, or `1.N` / `2.N`.
fn decode_field_spec(spec: &[u8]) -> Result<OutSpec, getopt::Error> {
    match spec.first() {
        Some(b'0') => {
            if spec.len() > 1 {
                // `0` must be all alone: there is no `0.FIELD`.
                Err(JOIN.usage(format!("invalid field specifier: {}", quote(spec))))
            } else {
                Ok(OutSpec::JoinField)
            }
        }
        Some(&first @ (b'1' | b'2')) => {
            if spec.get(1) != Some(&b'.') {
                return Err(JOIN.usage(format!("invalid field specifier: {}", quote(spec))));
            }
            let index = string_to_join_field(spec.get(2..).unwrap_or_default())?;
            Ok(OutSpec::Field {
                file: if first == b'1' { 1 } else { 2 },
                index,
            })
        }
        // Including an *empty* spec, which a trailing separator produces:
        // upstream reads `s[0]` and finds the terminator.
        _ => Err(JOIN.usage(format!(
            "invalid file number in field spec: {}",
            quote(spec)
        ))),
    }
}

// -------------------------------------------------------------------- parsing

/// What an operand recorded so far might still turn out to be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    MustBeOperand,
    MightBeJ1Arg,
    MightBeJ2Arg,
    MightBeOArg,
}

/// The two operand slots and what each one might still be.
#[derive(Debug)]
struct Operands {
    names: [Option<OsString>; 2],
    status: [Status; 2],
    count: usize,
}

/// The whole of the command line's state while it is being read.
#[derive(Debug)]
struct Parse {
    settings: Settings,
    /// The join fields, `None` for upstream's `-1` sentinel meaning "not yet
    /// determined" — which is not the same as 0, because 0 is a legal value
    /// that a second, disagreeing option must be refused against.
    jf: [Option<usize>; 2],
    /// How many `-j1` / `-j2` options are still waiting to find out whether an
    /// operand was their argument.
    joption_count: [i32; 2],
    operands: Operands,
    /// Upstream's `prev_optc_status`: what the *previous* iteration left
    /// behind, which is what an operand arriving now is recorded as.
    prev: Status,
}

impl Parse {
    fn new() -> Self {
        Self {
            settings: Settings::default(),
            jf: [None, None],
            joption_count: [0, 0],
            operands: Operands {
                names: [None, None],
                status: [Status::MustBeOperand, Status::MustBeOperand],
                count: 0,
            },
            prev: Status::MustBeOperand,
        }
    }

    /// Upstream's `set_join_field`: the same field may be named twice only if
    /// both namings agree. The numbers in the message are zero-based, because
    /// what upstream prints is the stored value.
    fn set_join_field(&mut self, which: usize, value: usize) -> Result<(), getopt::Error> {
        if let Some(previous) = self.jf.get(which).copied().flatten()
            && previous != value
        {
            return Err(JOIN.usage(format!("incompatible join fields {previous}, {value}")));
        }
        if let Some(slot) = self.jf.get_mut(which) {
            *slot = Some(value);
        }
        Ok(())
    }

    /// Upstream's `add_field_list`: comma- or blank-separated field specs.
    ///
    /// The loop is a `do`/`while`, so a *trailing* separator produces one more,
    /// empty, spec — and that is an error rather than being ignored.
    fn add_field_list(&mut self, list: &[u8]) -> Result<(), getopt::Error> {
        let mut at = 0usize;
        loop {
            let rest = list.get(at..).unwrap_or_default();
            let end = rest
                .iter()
                .position(|&c| c == b',' || c == b' ' || c == b'\t');
            let item = match end {
                Some(offset) => rest.get(..offset).unwrap_or_default(),
                None => rest,
            };
            let spec = decode_field_spec(item)?;
            self.settings.outlist.push(spec);
            match end {
                Some(offset) => at = at.saturating_add(offset).saturating_add(1),
                None => return Ok(()),
            }
        }
    }

    /// Upstream's `add_file_name`, which is where an operand can stop being one.
    ///
    /// With both slots full, the arriving name forces a decision about the
    /// older ones: the first slot that was not already settled as an operand is
    /// consumed as the pending option's argument and the rest shift down. If
    /// *both* were settled, there is genuinely a third file and that is the
    /// error.
    fn add_file_name(&mut self, name: &OsStr, current: &mut Status) -> Result<(), getopt::Error> {
        if self.operands.count == 2 {
            let slot = usize::from(self.operands.status[0] == Status::MustBeOperand);
            let pending = self
                .operands
                .status
                .get(slot)
                .copied()
                .unwrap_or(Status::MustBeOperand);
            let arg = self
                .operands
                .names
                .get(slot)
                .cloned()
                .flatten()
                .unwrap_or_default();
            match pending {
                Status::MustBeOperand => {
                    // `quoteaf`, not `quote`: GNU join spells this one with
                    // the always-quote flavour, which §351 keeps straight in
                    // every locale. So it stays `'C'` where the `quote()`
                    // family went curly.
                    return Err(JOIN.usage_referring(format!("extra operand {}", quoteaf_os(name))));
                }
                Status::MightBeJ1Arg => {
                    self.joption_count[0] = self.joption_count[0].saturating_sub(1);
                    let value = string_to_join_field(&arg_bytes(&arg))?;
                    self.set_join_field(0, value)?;
                }
                Status::MightBeJ2Arg => {
                    self.joption_count[1] = self.joption_count[1].saturating_sub(1);
                    let value = string_to_join_field(&arg_bytes(&arg))?;
                    self.set_join_field(1, value)?;
                }
                Status::MightBeOArg => self.add_field_list(&arg_bytes(&arg))?,
            }
            if slot == 0 {
                self.operands.status[0] = self.operands.status[1];
                self.operands.names[0] = self.operands.names[1].take();
            }
            self.operands.count = 1;
        }

        let n = self.operands.count;
        if let Some(status) = self.operands.status.get_mut(n) {
            *status = self.prev;
        }
        if let Some(entry) = self.operands.names.get_mut(n) {
            *entry = Some(name.to_os_string());
        }
        self.operands.count = n.saturating_add(1);
        // A name recorded as `-o`'s continuation makes the *next* name one too,
        // which is what lets `-o 1.1 2.2 1.3 A B` work.
        if self.prev == Status::MightBeOArg {
            *current = Status::MightBeOArg;
        }
        Ok(())
    }
}

fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut parse = Parse::new();
    let mut at = 0usize;
    let mut only_operands = false;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);

        if only_operands {
            // Upstream resets `prev_optc_status` once before its post-`--`
            // loop and never updates it again, so nothing after `--` can be
            // anything but an operand.
            parse.prev = Status::MustBeOperand;
            let mut current = Status::MustBeOperand;
            parse.add_file_name(arg, &mut current)?;
            continue;
        }

        let bytes = arg_bytes(arg);
        if bytes == b"--" {
            only_operands = true;
            continue;
        }

        // Upstream's option string begins with `-`, so operands are handled
        // where they appear instead of being permuted to the end.
        let mut current = Status::MustBeOperand;
        if bytes == b"-" || bytes.first() != Some(&b'-') {
            parse.add_file_name(arg, &mut current)?;
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) = long_option(body, &bytes, &mut parse)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut at, &mut parse, &mut current)?;
        }
        parse.prev = current;
    }

    if parse.operands.count != 2 {
        return Err(if parse.operands.count == 0 {
            JOIN.usage_referring("missing operand".to_string())
        } else {
            // `argv[argc - 1]`, not the operand — and with no permutation those
            // are different arguments: `join A -z` names `-z`.
            let last = args.last().map(|a| arg_bytes(a)).unwrap_or_default();
            JOIN.usage_referring(format!("missing operand after {}", quote(&last)))
        });
    }

    // A `-j1` that never found an argument was plain `-j 1` after all, and
    // likewise `-j2`. The loop index *is* the zero-based field number.
    for i in 0..2usize {
        if parse.joption_count.get(i).copied().unwrap_or(0) != 0 {
            parse.set_join_field(0, i)?;
            parse.set_join_field(1, i)?;
        }
    }

    let mut settings = parse.settings;
    settings.jf = [parse.jf[0].unwrap_or(0), parse.jf[1].unwrap_or(0)];
    let mut names = parse.operands.names.into_iter().flatten();
    match (names.next(), names.next()) {
        (Some(first), Some(second)) => Ok(Request::Run(settings, first, second)),
        // Unreachable: the count was checked to be 2 just above.
        _ => Err(JOIN.usage_referring("missing operand".to_string())),
    }
}

/// One `--name` or `--name=value` argument. None of `join`'s long options takes
/// a value, so the only thing `=` can produce here is a diagnostic.
fn long_option(
    body: &[u8],
    whole: &[u8],
    parse: &mut Parse,
) -> Result<Option<Request>, getopt::Error> {
    // Split before resolving, so the *name* is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(eq) => (
            body.get(..eq).unwrap_or_default(),
            Some(body.get(eq.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 matches none of
    // them and takes the unrecognised path, reported as the bytes typed.
    let typed = std::str::from_utf8(typed).map_err(|_| JOIN.unrecognized_option(whole))?;
    let (name, which) = JOIN.resolve_long(typed, whole, LONG_OPTIONS)?;
    if inline.is_some() {
        return Err(JOIN.long_unwanted_argument(name));
    }
    match which {
        Long::IgnoreCase => parse.settings.ignore_case = true,
        Long::CheckOrder => parse.settings.check = OrderCheck::Enabled,
        Long::NoCheckOrder => parse.settings.check = OrderCheck::Disabled,
        Long::ZeroTerminated => parse.settings.eol = 0,
        Long::Header => parse.settings.header = true,
        Long::Help => return Ok(Some(Request::Help)),
        Long::Version => return Ok(Some(Request::Version)),
    }
    Ok(None)
}

/// One `-abc` cluster, which may end in an option that takes an argument.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    next: &mut usize,
    parse: &mut Parse,
    current: &mut Status,
) -> Result<(), getopt::Error> {
    let mut at = 1usize;
    while let Some(&c) = bytes.get(at) {
        at = at.saturating_add(1);
        // Upstream resets `optc_status` at the top of every iteration of its
        // option loop, and one iteration is one *option*, not one argument.
        *current = Status::MustBeOperand;
        match c {
            b'i' => parse.settings.ignore_case = true,
            b'z' => parse.settings.eol = 0,
            b'a' | b'e' | b'v' | b'1' | b'2' | b'j' | b'o' | b't' => {
                let attached = bytes.get(at..).filter(|rest| !rest.is_empty());
                let (value, attached_at_2) = match attached {
                    Some(rest) => {
                        let starts_at_2 = at == 2;
                        at = bytes.len();
                        (rest.to_vec(), starts_at_2)
                    }
                    None => {
                        let Some(separate) = args.get(*next) else {
                            return Err(JOIN.short_missing_argument(c));
                        };
                        *next = next.saturating_add(1);
                        (arg_bytes(separate), false)
                    }
                };
                option_with_argument(c, &value, attached_at_2, parse, current)?;
            }
            _ => return Err(JOIN.invalid_option(c)),
        }
    }
    Ok(())
}

/// The short options that take an argument.
///
/// `attached_at_2` is upstream's `optarg == argv[optind - 1] + 2` — true only
/// when the value was written joined to an option that began the argument, as
/// in `-j1`. It is what separates the ambiguous obsolescent `-j1` from a plain
/// `-j 1`, and it is deliberately false inside a cluster: `-ij1` is `-j 1`.
fn option_with_argument(
    c: u8,
    value: &[u8],
    attached_at_2: bool,
    parse: &mut Parse,
    current: &mut Status,
) -> Result<(), getopt::Error> {
    match c {
        b'v' => {
            parse.settings.pairables = false;
            set_unpairable(parse, value)?;
        }
        b'a' => set_unpairable(parse, value)?,
        b'e' => {
            if let Some(previous) = &parse.settings.empty_filler
                && previous.as_slice() != value
            {
                return Err(JOIN.usage("conflicting empty-field replacement strings".to_string()));
            }
            parse.settings.empty_filler = Some(value.to_vec());
        }
        b'1' => {
            let field = string_to_join_field(value)?;
            parse.set_join_field(0, field)?;
        }
        b'2' => {
            let field = string_to_join_field(value)?;
            parse.set_join_field(1, field)?;
        }
        b'j' => {
            if attached_at_2 && (value == b"1" || value == b"2") {
                let which = usize::from(value == b"2");
                parse.joption_count[which] = parse.joption_count[which].saturating_add(1);
                *current = if which == 1 {
                    Status::MightBeJ2Arg
                } else {
                    Status::MightBeJ1Arg
                };
            } else {
                let field = string_to_join_field(value)?;
                parse.set_join_field(0, field)?;
                parse.set_join_field(1, field)?;
            }
        }
        b'o' => {
            if value == b"auto" {
                // Note this does *not* make the next operand a continuation:
                // `auto` is a whole format, not the first item of a list.
                parse.settings.autoformat = true;
            } else {
                parse.add_field_list(value)?;
                *current = Status::MightBeOArg;
            }
        }
        b't' => set_tab(parse, value)?,
        _ => {}
    }
    Ok(())
}

/// `-a` and `-v`'s shared body.
fn set_unpairable(parse: &mut Parse, value: &[u8]) -> Result<(), getopt::Error> {
    let which = parse_file_number(value)?;
    if which == 1 {
        parse.settings.unpairable[0] = true;
    } else {
        parse.settings.unpairable[1] = true;
    }
    Ok(())
}

/// `-t`'s argument: one byte, the two-character `\0`, or nothing at all.
fn set_tab(parse: &mut Parse, value: &[u8]) -> Result<(), getopt::Error> {
    let newtab = match value.first() {
        // `-t ''` means "the whole line is one field", which is spelled as a
        // newline separator because a newline never occurs inside a record.
        None => b'\n',
        Some(&first) => {
            if value.len() > 1 {
                if value == br"\0" {
                    0
                } else {
                    return Err(JOIN.usage(format!("multi-character tab {}", quote(value))));
                }
            } else {
                first
            }
        }
    };
    if let Some(previous) = parse.settings.tab
        && previous != newtab
    {
        return Err(JOIN.usage("incompatible tabs".to_string()));
    }
    parse.settings.tab = Some(newtab);
    Ok(())
}

#[cfg(unix)]
fn arg_bytes(arg: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsStr) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

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
            Err(e) => e.message(),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// Run the merge over two in-memory files, returning stdout and whether
    /// either file was warned about.
    fn output(settings: &Settings, left: &[u8], right: &[u8]) -> (Vec<u8>, bool) {
        let mut in1 = Input {
            name: OsString::from("A"),
            reader: Box::new(io::Cursor::new(left.to_vec())),
            which: 1,
            line_no: 0,
            prev_key: None,
            warned: false,
        };
        let mut in2 = Input {
            name: OsString::from("B"),
            reader: Box::new(io::Cursor::new(right.to_vec())),
            which: 2,
            line_no: 0,
            prev_key: None,
            warned: false,
        };
        let mut buffer: Vec<u8> = Vec::new();
        join(&mut buffer, settings, &mut in1, &mut in2).unwrap();
        (buffer, in1.warned || in2.warned)
    }

    fn joined(items: &[&str], left: &[u8], right: &[u8]) -> String {
        let settings = settings(&[items, &["A", "B"]].concat());
        let (bytes, _) = output(&settings, left, right);
        String::from_utf8(bytes).unwrap()
    }

    fn ranges(record: &[u8], tab: Option<u8>) -> Vec<&[u8]> {
        split_fields(record, tab)
            .into_iter()
            .map(|(start, end)| &record[start..end])
            .collect()
    }

    const A: &[u8] = b"a 1\nb 2\nd 4\n";
    const B: &[u8] = b"b x\nc y\nd z\n";

    // ------------------------------------------------------------- splitting

    #[test]
    fn blanks_separate_in_runs_and_leading_ones_belong_to_nobody() {
        assert_eq!(ranges(b"a  b\tc\n", None), vec![&b"a"[..], b"b", b"c"]);
        assert_eq!(ranges(b"   a b\n", None), vec![&b"a"[..], b"b"]);
        assert_eq!(ranges(b"   \n", None), Vec::<&[u8]>::new());
    }

    #[test]
    fn a_trailing_blank_makes_a_trailing_empty_field() {
        // Two fields, not one: upstream's loop leaves the cursor at the end and
        // then extracts one more field of length zero.
        assert_eq!(ranges(b"a \n", None), vec![&b"a"[..], b""]);
        assert_eq!(ranges(b"a b \n", None), vec![&b"a"[..], b"b", b""]);
    }

    #[test]
    fn every_occurrence_of_an_explicit_tab_separates() {
        assert_eq!(ranges(b"a::b\n", Some(b':')), vec![&b"a"[..], b"", b"b"]);
        assert_eq!(ranges(b":a\n", Some(b':')), vec![&b""[..], b"a"]);
        assert_eq!(ranges(b"a:\n", Some(b':')), vec![&b"a"[..], b""]);
    }

    #[test]
    fn a_newline_separator_makes_the_line_one_field() {
        // Which is what `-t ''` asks for.
        assert_eq!(ranges(b"a b c\n", Some(b'\n')), vec![&b"a b c"[..]]);
    }

    #[test]
    fn an_empty_record_has_no_fields_at_all() {
        assert_eq!(ranges(b"\n", None), Vec::<&[u8]>::new());
        assert_eq!(ranges(b"\n", Some(b':')), Vec::<&[u8]>::new());
    }

    // ------------------------------------------------------------ comparison

    #[test]
    fn an_absent_field_and_an_empty_one_are_the_same_field() {
        assert_eq!(keycmp(b"", b"", false), Ordering::Equal);
        assert_eq!(keycmp(b"", b"a", false), Ordering::Less);
        assert_eq!(keycmp(b"a", b"", false), Ordering::Greater);
    }

    #[test]
    fn a_prefix_sorts_before_what_it_is_a_prefix_of() {
        assert_eq!(keycmp(b"ab", b"abc", false), Ordering::Less);
        assert_eq!(keycmp(b"ab", b"ab", false), Ordering::Equal);
    }

    #[test]
    fn ignore_case_folds_ascii_and_still_breaks_ties_by_length() {
        assert_eq!(keycmp(b"AB", b"ab", true), Ordering::Equal);
        assert_eq!(keycmp(b"AB", b"abc", true), Ordering::Less);
        assert_eq!(keycmp(b"AB", b"ab", false), Ordering::Less);
    }

    // ---------------------------------------------------------------- output

    #[test]
    fn the_default_layout_is_key_then_the_rest_of_each_file() {
        assert_eq!(joined(&[], A, B), "b 2 x\nd 4 z\n");
    }

    #[test]
    fn an_unpairable_line_keeps_its_key_in_front() {
        assert_eq!(joined(&["-a", "1"], A, B), "a 1\nb 2 x\nd 4 z\n");
        assert_eq!(joined(&["-a", "2"], A, B), "b 2 x\nc y\nd 4 z\n");
    }

    #[test]
    fn v_prints_only_the_unpairable_ones() {
        assert_eq!(joined(&["-v", "1"], A, B), "a 1\n");
        assert_eq!(joined(&["-v", "1", "-v", "2"], A, B), "a 1\nc y\n");
    }

    #[test]
    fn auto_pads_a_short_line_to_the_width_of_the_first() {
        // A trailing separator with nothing after it, which is what the width
        // costs when there is no `-e`.
        assert_eq!(
            joined(&["-o", "auto", "-a", "1"], A, B),
            "a 1 \nb 2 x\nd 4 z\n"
        );
        assert_eq!(
            joined(&["-o", "auto", "-a", "1", "-e", "X"], A, B),
            "a 1 X\nb 2 x\nd 4 z\n"
        );
    }

    #[test]
    fn an_output_list_prints_exactly_what_it_lists() {
        assert_eq!(joined(&["-o", "2.2,1.2"], A, B), "x 2\nz 4\n");
        assert_eq!(joined(&["-o", "0"], A, B), "b\nd\n");
    }

    #[test]
    fn the_filler_replaces_an_empty_field_not_only_a_missing_one() {
        // The second line of the left file has an empty second field.
        let left = b"k v\nk \n";
        let right = b"k w\n";
        assert_eq!(joined(&["-e", "X"], left, right), "k v w\nk X w\n");
    }

    #[test]
    fn the_filler_is_not_applied_before_the_comparison() {
        // Both keys are empty here, so they pair; a filler substituted first
        // would have made them "X" and "X" — the same answer — but a filler
        // substituted first also pairs a *missing* key with a literal "X".
        let left = b" \n";
        let right = b"X y\n";
        assert_eq!(joined(&["-e", "X"], left, right), "");
    }

    #[test]
    fn every_line_of_a_run_pairs_with_every_line_of_the_other() {
        let left = b"k 1\nk 2\n";
        let right = b"k a\nk b\n";
        assert_eq!(joined(&[], left, right), "k 1 a\nk 1 b\nk 2 a\nk 2 b\n");
    }

    #[test]
    fn header_pairs_the_first_two_lines_whatever_they_say() {
        assert_eq!(joined(&["--header"], A, B), "a 1 x\nd 4 z\n");
        // With one file empty the header still prints, taking its key from the
        // other file.
        assert_eq!(joined(&["--header"], b"", B), "b x\n");
    }

    #[test]
    fn zero_terminated_makes_each_file_one_record() {
        // Only the *record* delimiter changes: the newlines inside the one
        // record are still field separators, and because the record ends with
        // one, each file ends in an empty fourth field.
        let settings = settings(&["-z", "A", "B"]);
        let (bytes, _) = output(&settings, b"c\na\nb\n", b"c\na\nb\n");
        assert_eq!(bytes, b"c a b  a b \0".to_vec());
    }

    #[test]
    fn bytes_that_are_not_text_survive() {
        let left = b"\xff k\n";
        let right = b"\xff v\n";
        let settings = settings(&["A", "B"]);
        let (bytes, _) = output(&settings, left, right);
        assert_eq!(bytes, b"\xff k v\n".to_vec());
    }

    // ----------------------------------------------------------- order check

    #[test]
    fn disorder_is_ignored_while_everything_pairs() {
        let unsorted = b"c\na\nb\n";
        let settings = settings(&["A", "B"]);
        let (_, warned) = output(&settings, unsorted, unsorted);
        assert!(!warned);
    }

    #[test]
    fn disorder_is_reported_once_a_line_fails_to_pair() {
        let settings = settings(&["A", "B"]);
        let (out, warned) = output(&settings, b"c\na\nb\n", b"a\nb\nc\n");
        assert!(warned);
        // The pairing that did happen is still written.
        assert_eq!(out, b"c\n".to_vec());
    }

    #[test]
    fn nocheck_order_stays_quiet() {
        let settings = settings(&["--nocheck-order", "A", "B"]);
        let (_, warned) = output(&settings, b"c\na\nb\n", b"a\nb\nc\n");
        assert!(!warned);
    }

    #[test]
    fn check_order_is_fatal_at_once() {
        let settings = settings(&["--check-order", "A", "B"]);
        let mut in1 = Input {
            name: OsString::from("A"),
            reader: Box::new(io::Cursor::new(b"c\na\n".to_vec())),
            which: 1,
            line_no: 0,
            prev_key: None,
            warned: false,
        };
        let mut in2 = Input {
            name: OsString::from("B"),
            reader: Box::new(io::Cursor::new(b"c\n".to_vec())),
            which: 2,
            line_no: 0,
            prev_key: None,
            warned: false,
        };
        let mut buffer: Vec<u8> = Vec::new();
        match join(&mut buffer, &settings, &mut in1, &mut in2) {
            Err(Trouble::Unsorted(sentence)) => {
                assert_eq!(sentence, b"A:2: is not sorted: a".to_vec());
            }
            other => panic!("expected a fatal disorder, got {other:?}"),
        }
    }

    #[test]
    fn the_disorder_message_strips_a_newline_and_stops_at_a_nul() {
        let name = OsString::from("D");
        assert_eq!(
            disorder_message(&name, 2, b"a b\n"),
            b"D:2: is not sorted: a b".to_vec()
        );
        // `-z`'s delimiter is not a newline, so nothing is stripped — but
        // `%.*s` stops at it anyway.
        assert_eq!(
            disorder_message(&name, 7, b"a b\0"),
            b"D:7: is not sorted: a b".to_vec()
        );
        // A NUL in the middle of an ordinary line truncates there too.
        assert_eq!(
            disorder_message(&name, 1, b"a\0b\n"),
            b"D:1: is not sorted: a".to_vec()
        );
    }

    // --------------------------------------------------------------- numbers

    #[test]
    fn a_number_may_have_space_and_a_sign_but_not_a_suffix() {
        assert_eq!(strtoimax(b"3"), Number::Value(3));
        assert_eq!(strtoimax(b" \t\x0b+3"), Number::Value(3));
        assert_eq!(strtoimax(b"-3"), Number::Value(-3));
        assert_eq!(strtoimax(b""), Number::Invalid);
        assert_eq!(strtoimax(b"3x"), Number::Invalid);
        assert_eq!(strtoimax(b"3 "), Number::Invalid);
        assert_eq!(strtoimax(b"x"), Number::Invalid);
    }

    #[test]
    fn overflow_is_a_separate_answer_from_invalid() {
        assert_eq!(strtoimax(b"9223372036854775807"), Number::Value(i64::MAX));
        assert_eq!(strtoimax(b"9223372036854775808"), Number::Overflow);
        // The most negative value is representable; one past it is not.
        assert_eq!(strtoimax(b"-9223372036854775808"), Number::Value(i64::MIN));
        assert_eq!(strtoimax(b"-9223372036854775809"), Number::Overflow);
        // Overflow *and* a suffix is refused, not clamped.
        assert_eq!(strtoimax(b"99999999999999999999x"), Number::Invalid);
    }

    #[test]
    fn a_field_number_too_large_is_clamped_and_a_zero_one_is_not() {
        assert_eq!(string_to_join_field(b"1").unwrap(), 0);
        // Clamped to `PTRDIFF_MAX`, which is `i64::MAX` here, then made
        // zero-based — not to `usize::MAX`.
        assert_eq!(
            string_to_join_field(b"99999999999999999999").unwrap(),
            i64::MAX as usize - 1
        );
        assert!(string_to_join_field(b"0").is_err());
        assert!(string_to_join_field(b"-1").is_err());
        assert!(string_to_join_field(b"1x").is_err());
    }

    #[test]
    fn a_clamped_field_number_makes_every_key_empty() {
        // Which is why this runs rather than failing, and prints nothing.
        assert_eq!(joined(&["-1", "99999999999999999999"], A, B), "");
    }

    // --------------------------------------------------------------- parsing

    #[test]
    fn short_options_cluster_and_the_last_may_take_the_rest() {
        let s = settings(&["-iz", "A", "B"]);
        assert!(s.ignore_case);
        assert_eq!(s.eol, 0);
        let s = settings(&["-t:", "A", "B"]);
        assert_eq!(s.tab, Some(b':'));
        let s = settings(&["-12", "A", "B"]);
        assert_eq!(s.jf, [1, 0]);
        let s = settings(&["-a1", "A", "B"]);
        assert_eq!(s.unpairable, [true, false]);
    }

    #[test]
    fn an_option_argument_may_be_the_next_word_instead() {
        let s = settings(&["-t", ":", "A", "B"]);
        assert_eq!(s.tab, Some(b':'));
        assert_eq!(
            refuse(&["-t"]),
            "option requires an argument -- 't'\nTry 'join --help' for more information."
        );
    }

    #[test]
    fn an_empty_tab_argument_means_the_whole_line() {
        let s = settings(&["-t", "", "A", "B"]);
        assert_eq!(s.tab, Some(b'\n'));
        // And it is compatible with a literal newline, which is the same value.
        assert_eq!(settings(&["-t", "", "-t", "\n", "A", "B"]).tab, Some(b'\n'));
        assert_eq!(settings(&["-t", "\\0", "A", "B"]).tab, Some(0));
        assert_eq!(refuse(&["-t", "xy", "A", "B"]), "multi-character tab ‘xy’");
        assert_eq!(refuse(&["-t:", "-t,", "A", "B"]), "incompatible tabs");
    }

    #[test]
    fn long_options_abbreviate_and_an_empty_one_lists_the_table() {
        assert!(settings(&["--ign", "A", "B"]).ignore_case);
        assert_eq!(settings(&["--zero", "A", "B"]).eol, 0);
        assert_eq!(
            refuse(&["--=x", "A", "B"]),
            "option '--=x' is ambiguous; possibilities: '--ignore-case' '--check-order' \
             '--nocheck-order' '--zero-terminated' '--header' '--help' '--version'\n\
             Try 'join --help' for more information."
        );
    }

    #[test]
    fn the_long_options_take_nothing() {
        assert_eq!(
            refuse(&["--header=x", "A", "B"]),
            "option '--header' doesn't allow an argument\nTry 'join --help' for more information."
        );
    }

    #[test]
    fn unknown_options() {
        assert_eq!(
            refuse(&["-Q", "A", "B"]),
            "invalid option -- 'Q'\nTry 'join --help' for more information."
        );
        assert_eq!(
            refuse(&["--nope", "A", "B"]),
            "unrecognized option '--nope'\nTry 'join --help' for more information."
        );
    }

    #[test]
    fn operands_are_counted_exactly() {
        assert_eq!(
            operands(&["A", "B"]),
            (OsString::from("A"), OsString::from("B"))
        );
        assert_eq!(
            refuse(&["A", "B", "C"]),
            "extra operand 'C'\nTry 'join --help' for more information."
        );
        assert_eq!(
            refuse(&[]),
            "missing operand\nTry 'join --help' for more information."
        );
        // The name in the message is the last *argument*, not the last operand,
        // because nothing is permuted.
        assert_eq!(
            refuse(&["A", "-z"]),
            // Curly, unlike `extra operand` above: this one goes through
            // `quote()`, which follows the locale, while `extra operand` uses
            // `quoteaf_os` and stays straight in every locale.
            "missing operand after ‘-z’\nTry 'join --help' for more information."
        );
    }

    #[test]
    fn double_dash_ends_the_options() {
        assert_eq!(
            operands(&["--", "-a", "-z"]),
            (OsString::from("-a"), OsString::from("-z"))
        );
    }

    #[test]
    fn a_lone_dash_is_an_operand() {
        assert_eq!(
            operands(&["-", "B"]),
            (OsString::from("-"), OsString::from("B"))
        );
    }

    #[test]
    fn help_and_version_win_over_a_bad_operand_count() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["--version"])), Ok(Request::Version));
    }

    // -------------------------------------------- operands that are arguments

    #[test]
    fn an_operand_after_j1_may_be_its_argument() {
        // `-j1 3 A B`: three names arrive, so the first is reinterpreted.
        let s = settings(&["-j1", "3", "A", "B"]);
        assert_eq!(s.jf, [2, 0]);
        assert_eq!(
            operands(&["-j1", "3", "A", "B"]),
            (OsString::from("A"), OsString::from("B"))
        );
        // `-j2` sets the *second* field only.
        assert_eq!(settings(&["-j2", "3", "A", "B"]).jf, [0, 2]);
    }

    #[test]
    fn a_j1_with_nothing_to_claim_is_plain_j_1() {
        assert_eq!(settings(&["-j1", "A", "B"]).jf, [0, 0]);
        assert_eq!(settings(&["-j2", "A", "B"]).jf, [1, 1]);
        // And it agrees with an explicit `-j 1`, which is what makes this legal.
        assert_eq!(settings(&["-j", "1", "-j1", "A", "B"]).jf, [0, 0]);
    }

    #[test]
    fn j_inside_a_cluster_is_never_the_ambiguous_form() {
        // `-ij1` is `-i -j 1`, because the value does not begin at offset 2.
        let s = settings(&["-ij1", "A", "B"]);
        assert!(s.ignore_case);
        assert_eq!(s.jf, [0, 0]);
        // Whereas `-ij1 3 A B` would be four names and an extra operand.
        assert_eq!(
            refuse(&["-ij1", "3", "A", "B"]),
            "extra operand 'B'\nTry 'join --help' for more information."
        );
    }

    #[test]
    fn an_operand_after_o_continues_its_list() {
        let s = settings(&["-o", "1.1", "2.2", "A", "B"]);
        assert_eq!(
            s.outlist,
            vec![
                OutSpec::Field { file: 1, index: 0 },
                OutSpec::Field { file: 2, index: 1 }
            ]
        );
        // The continuation chains: every operand after the first keeps the
        // status, so three specs and two files works too.
        let s = settings(&["-o", "1.1", "2.2", "1.2", "A", "B"]);
        assert_eq!(s.outlist.len(), 3);
    }

    #[test]
    fn a_third_file_after_o_is_read_as_another_spec_and_fails_there() {
        // The name reported is `A`, which was never an operand — this is the
        // measured behaviour of `join -o 1.1 A B C`.
        assert_eq!(
            refuse(&["-o", "1.1", "A", "B", "C"]),
            "invalid file number in field spec: ‘A’"
        );
    }

    #[test]
    fn auto_does_not_start_a_list() {
        let s = settings(&["-o", "auto", "A", "B"]);
        assert!(s.autoformat);
        assert!(s.outlist.is_empty());
        // So a third name here really is an extra operand.
        assert_eq!(
            refuse(&["-o", "auto", "A", "B", "C"]),
            "extra operand 'C'\nTry 'join --help' for more information."
        );
    }

    #[test]
    fn a_field_list_may_be_separated_by_blanks_and_a_trailing_one_is_an_error() {
        assert_eq!(settings(&["-o", "1.1 2.2", "A", "B"]).outlist.len(), 2);
        assert_eq!(settings(&["-o", "1.1\t2.2", "A", "B"]).outlist.len(), 2);
        assert_eq!(
            refuse(&["-o", "1.1,", "A", "B"]),
            "invalid file number in field spec: ‘’"
        );
    }

    #[test]
    fn a_field_spec_names_a_file_and_a_field() {
        assert_eq!(
            refuse(&["-o", "3.1", "A", "B"]),
            "invalid file number in field spec: ‘3.1’"
        );
        assert_eq!(
            refuse(&["-o", "0.1", "A", "B"]),
            "invalid field specifier: ‘0.1’"
        );
        assert_eq!(
            refuse(&["-o", "1", "A", "B"]),
            "invalid field specifier: ‘1’"
        );
        assert_eq!(refuse(&["-o", "1.", "A", "B"]), "invalid field number: ‘’");
    }

    #[test]
    fn the_same_join_field_named_twice_must_agree() {
        assert_eq!(settings(&["-1", "2", "-1", "2", "A", "B"]).jf, [1, 0]);
        assert_eq!(
            refuse(&["-1", "1", "-1", "2", "A", "B"]),
            "incompatible join fields 0, 1"
        );
        // `-j` sets both, so it clashes with a disagreeing `-2` as well.
        assert_eq!(
            refuse(&["-j", "1", "-2", "2", "A", "B"]),
            "incompatible join fields 0, 1"
        );
    }

    #[test]
    fn a_file_number_must_be_one_or_two() {
        assert_eq!(refuse(&["-a", "3", "A", "B"]), "invalid field number: ‘3’");
        assert_eq!(refuse(&["-v", "0", "A", "B"]), "invalid field number: ‘0’");
        assert_eq!(refuse(&["-a", "", "A", "B"]), "invalid field number: ‘’");
    }

    #[test]
    fn a_second_filler_must_be_the_same_filler() {
        assert_eq!(
            settings(&["-e", "X", "-e", "X", "A", "B"]).empty_filler,
            Some(b"X".to_vec())
        );
        assert_eq!(
            refuse(&["-e", "X", "-e", "Y", "A", "B"]),
            "conflicting empty-field replacement strings"
        );
    }
}
