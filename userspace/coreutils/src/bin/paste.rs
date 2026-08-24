//! `paste` — write corresponding lines of several files side by side.
//!
//! ```text
//! paste [-sz] [-d LIST] [--] [FILE...]
//! ```
//!
//! | option | effect |
//! |---|---|
//! | `-s`, `--serial` | one file at a time, its own lines joined, instead of in parallel |
//! | `-d LIST`, `--delimiters=LIST` | cycle through `LIST` instead of separating with a tab |
//! | `-z`, `--zero-terminated` | a line ends at a NUL, not at a newline |
//!
//! ## What this used to be
//!
//! `paste [-d DELIM] [-s] FILE...`, wrong in seven ways:
//!
//! * **The delimiter list did not cycle.** `-d ,;` is "comma, then semicolon,
//!   then comma again"; the old code used the whole two-character string
//!   between every pair of columns, so `paste -d ,\; A B C` printed `a1,;b1,;c1`
//!   where GNU prints `a1,b1;c1`.
//! * **Backslash escapes were not collapsed.** `-d '\n'` meant the two
//!   characters `\` and `n`, not a newline — and `-d '\0'`, the spelling for
//!   "no delimiter at this position at all", was two characters too. A list
//!   ending in a lone backslash, which GNU refuses outright, was accepted.
//! * **`-z` did not exist**, so a NUL-separated stream — the thing `find
//!   -print0` and `sort -z` produce — could not be pasted at all.
//! * **`-` was not standard input**, it was a file called `-`.
//! * **Input was decoded as UTF-8 and split with `lines()`**, so a file that is
//!   not valid UTF-8 stopped at the first bad byte, a CRLF file silently lost
//!   its CRs, and every output line gained a newline whether the input had one
//!   or not.
//! * **`--`, `--serial`, `--delimiters=…` and every other long option were read
//!   as filenames**, which then did not exist.
//! * **Every exit was 0.** A missing file, a read error and a full disk all
//!   reported success — and in parallel mode a missing file, which GNU treats
//!   as fatal *before printing anything*, instead produced a full run with a
//!   silently absent column.
//!
//! ## The two modes are not two spellings of one algorithm
//!
//! They differ in more than the direction they walk. Parallel opens every
//! operand up front and an `fopen` failure there is `error (EXIT_FAILURE, …)` —
//! fatal, before a byte of output, and only the *first* bad operand is ever
//! named. Serial opens each file as it reaches it and uses `error (0, …)` —
//! it names every bad operand, keeps going, and merely remembers to exit 1.
//!
//! ```text
//! $ paste A nosuch B ; echo $?      $ paste -s A nosuch B ; echo $?
//! paste: nosuch: No such file...    a1     a2      a3
//! 1                                 paste: nosuch: No such file...
//!                                   b1     b2
//!                                   1
//! ```
//!
//! A *read* error is `error (0, …)` in both, and in parallel mode it surfaces
//! one output line later than the read that failed: upstream leaves it in the
//! stream's error flag and only looks at it when the file is finally closed,
//! which is the next time round the loop. [`Source::failed`] is that flag.
//!
//! ## The delimiter list
//!
//! `collapse_escapes` rewrites the argument before anything uses it:
//!
//! | written | means |
//! |---|---|
//! | `\0` | *no* delimiter in this position — but the position is still spent |
//! | `\b` `\f` `\n` `\r` `\t` `\v` `\\` | that byte |
//! | `\` followed by anything else | that anything else, the backslash dropped |
//! | a trailing lone `\` | fatal: `delimiter list ends with an unescaped backslash` |
//!
//! "No delimiter" and NUL are the same value upstream (`#define EMPTY_DELIM
//! '\0'`), so a NUL cannot be used *as* a delimiter — which is why `-d ''` is
//! rewritten by `main` to the two characters `\0` rather than to nothing: an
//! empty list would divide by zero when it cycled.
//!
//! The trailing-backslash diagnostic quotes the argument **as it was typed**,
//! before collapsing, and in `c_maybe` style rather than the usual shell one —
//! upstream: *"Don't use the quote() quoting style, because that would double
//! the number of displayed backslashes, making the diagnostic look bogus."*
//! See [`coreutils::quote::quote_c_maybe_colon`]. It is raised *after* the
//! whole command line is read, so `paste -d 'a\' -d ',' A` is fine and
//! `paste -d 'a\' -Q` reports `-Q`; but it comes before the files are opened,
//! so `paste -d '\' nosuch` reports the backslash, not the missing file.
//!
//! The cycle restarts per **output line** in parallel mode and per **file** in
//! serial mode.
//!
//! ## Every `-` is the same stream
//!
//! `paste - -` reads one stream and deals it into two columns, because upstream
//! stores `stdin` in both slots. Standard input is therefore held as a single
//! [`Source`] that the slots point at, not as a reader per slot: two
//! independently buffered readers over one file descriptor would each swallow a
//! block and interleave wrongly.
//!
//! ## The last line
//!
//! POSIX requires the output to end in a line delimiter even when the input did
//! not, so a final unterminated line gains one. Everything else is passed
//! through unchanged: in serial mode the file's very last byte is written
//! *raw*, never converted to a delimiter, which is why a normal file ending in
//! a newline does not end with a stray separator.
//!
//! An empty file is nothing at all in parallel mode — its column is skipped —
//! but a bare line delimiter in serial mode, because upstream's `charold` is
//! `EOF`, which is not the line delimiter, so one is printed.
//!
//! ## Checked against GNU
//!
//! `scripts/paste-diff.sh` runs this and glibc's `paste` over the same command
//! lines and compares stdout, stderr and the exit status byte for byte.
//! `scripts/paste-probe.py` is the ad-hoc probe the rows quoted above came
//! from.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::{quote_c_maybe_colon, quotef_os};
use coreutils::stdfd::{self, Stream};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's `paste >&-`
// as the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const PASTE: Program = Program::new("paste", 1);

const USAGE: &str = "usage: paste [-sz] [-d LIST] [--] [FILE...]";

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it.
/// Measured with `paste --=x`, whose empty prefix matches every entry.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("serial", Long::Serial),
    ("delimiters", Long::Delimiters),
    ("zero-terminated", Long::ZeroTerminated),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Serial,
    Delimiters,
    ZeroTerminated,
    Help,
    Version,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Paste these operands (`-` meaning standard input).
    Run(Settings, Vec<OsString>),
    Help,
    Version,
}

/// The three things the options decide.
#[derive(Debug, PartialEq, Eq)]
struct Settings {
    /// The collapsed delimiter list, cycled through. Never empty: a NUL byte
    /// in it is the "no delimiter here" position, not an absent list.
    delims: Vec<u8>,
    /// `-s`: one file at a time rather than one line from each.
    serial: bool,
    /// `-z`: the byte a line ends with, NUL rather than newline.
    line_delim: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            delims: vec![b'\t'],
            serial: false,
            line_delim: b'\n',
        }
    }
}

/// A command line that cannot be run.
///
/// The two variants are shaped by the two places upstream refuses one. A getopt
/// diagnostic ends in `Try 'paste --help' for more information.`; the delimiter
/// one is an `error (EXIT_FAILURE, …)` raised after the loop, with no referral.
#[derive(Debug)]
enum Refusal {
    Getopt(getopt::Error),
    Delimiters(String),
}

impl Refusal {
    fn report(&self) -> ExitCode {
        let status = match self {
            Self::Getopt(e) => {
                eprintln!("paste: {}", e.message());
                e.status
            }
            Self::Delimiters(message) => {
                eprintln!("paste: {message}");
                1
            }
        };
        ExitCode::from(u8::try_from(status).unwrap_or(1))
    }
}

/// A failure that ends the run rather than the file.
#[derive(Debug)]
enum Trouble {
    /// Parallel mode only: an operand that would not open. Upstream's
    /// `error (EXIT_FAILURE, errno, "%s", quotef (…))`, which is why the second
    /// bad operand of `paste nosuch nosuch2` is never mentioned.
    Open(OsString, io::Error),
    /// A named file turned out to *be* file descriptor 0, which had been
    /// closed, while some other operand was `-`. Upstream's
    /// `opened_stdin && have_read_stdin`.
    StdinClosed,
    Write(io::Error),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Open(name, e) => eprintln!("paste: {}: {}", quotef_os(name), strerror(e)),
            Self::StdinClosed => eprintln!("paste: standard input is closed"),
            Self::Write(e) => stdfd::write_error("paste", e),
        }
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(refusal) => return refusal.report(),
    };

    // `--help` and `--version` are writes like any other, so they fail like
    // any other: measured, `paste --help >&-` is
    // `paste: write error: Bad file descriptor` and exits 1.
    let mut out = Stream::stdout();
    let (settings, mut files) = match request {
        Request::Help => return say(out, format!("{USAGE}
").as_bytes()),
        Request::Version => return say(out, b"paste (SlateOS coreutils)
"),
        Request::Run(settings, files) => (settings, files),
    };

    if files.is_empty() {
        files.push(OsString::from("-"));
    }

    let mut stdin = Source::new(Box::new(io::stdin().lock()));

    let outcome = if settings.serial {
        paste_serial(&files, &mut stdin, &settings, &mut out)
    } else {
        match open_all(&files) {
            Ok(mut slots) => paste_parallel(&files, &mut slots, &mut stdin, &settings, &mut out),
            Err(trouble) => Err(trouble),
        }
    };
    // `atexit (close_stdout)` runs after the diagnostic that ended the run and
    // overrides its status, so a run that failed for its own reason still
    // delivers what it had written -- and still says so if it could not.
    let earned = match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(trouble) => trouble.report(),
    };

    // Buffered output has to reach the OS before success can be claimed; a
    // flush that fails here is a truncated file reported as a complete one.
    stdfd::close_stdout("paste", out, earned)
}

/// Say one thing and stop -- `--help` and `--version`.
///
/// The stream is closed here rather than left to the end of `main`, because
/// these two return without reaching it -- and closing it is what discovers
/// that there was nowhere to say it.
fn say(mut out: Stream, bytes: &[u8]) -> ExitCode {
    let _ = out.write_all(bytes);
    stdfd::close_stdout("paste", out, ExitCode::SUCCESS)
}

// ----------------------------------------------------------------- delimiters

/// The delimiter list, walked cyclically.
///
/// A NUL entry is upstream's `EMPTY_DELIM`: nothing is written, but the
/// position is still spent, so `-d 'x\0y'` puts `x` between columns 1 and 2,
/// *nothing* between 2 and 3, and `y` between 3 and 4.
struct Delims<'a> {
    list: &'a [u8],
    at: usize,
}

impl<'a> Delims<'a> {
    fn new(list: &'a [u8]) -> Self {
        Self { list, at: 0 }
    }

    /// The byte for this position, if any, advancing to the next.
    fn take(&mut self) -> Option<u8> {
        let byte = self.list.get(self.at).copied();
        self.at = self.at.saturating_add(1);
        if self.at >= self.list.len() {
            self.at = 0;
        }
        byte.filter(|&b| b != 0)
    }

    fn reset(&mut self) {
        self.at = 0;
    }
}

/// Upstream's `collapse_escapes`, returning `Err` for the trailing lone
/// backslash it reports on.
///
/// The `\0` case is the interesting one: it produces a NUL, which is the same
/// value as "no delimiter", so the two are indistinguishable by construction.
fn collapse_escapes(arg: &[u8]) -> Result<Vec<u8>, ()> {
    let mut out = Vec::with_capacity(arg.len());
    let mut at = 0usize;
    while let Some(&c) = arg.get(at) {
        at = at.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&escaped) = arg.get(at) else {
            // The list ends in an odd number of backslashes. Upstream stops
            // here — `goto done` — and keeps what it has, but the caller turns
            // that into a fatal diagnostic, so what it kept never gets used.
            return Err(());
        };
        at = at.saturating_add(1);
        out.push(match escaped {
            b'0' => 0,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            // `\\` and every other escape are the same rule — drop the
            // backslash, keep the byte — but upstream spells `\\` out
            // separately, and so does this, because they are different ideas.
            b'\\' => b'\\',
            other => other,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------- input

/// One open stream, plus the error flag upstream keeps inside `FILE`.
struct Source {
    reader: Box<dyn BufRead>,
    /// A read error already met.
    ///
    /// Upstream sets no such field: it reads the stream's own error flag with
    /// `ferror` when it closes the file, which in parallel mode is one output
    /// line *after* the read failed. Holding it here reproduces that delay, and
    /// reproduces the fact that a failed stream keeps reporting end of file in
    /// the meantime.
    failed: Option<io::Error>,
}

/// What stopped [`Source::copy_line`].
struct Stop {
    /// Upstream's `sometodo`: the file had at least one byte to give. False
    /// means end of file, a read error, or an already-closed slot.
    read: bool,
    /// The line delimiter that ended the line, or `None` at end of file.
    terminator: Option<u8>,
}

impl Source {
    fn new(reader: Box<dyn BufRead>) -> Self {
        Self {
            reader,
            failed: None,
        }
    }

    /// Copy this file's next line to `out`, stopping at `line_delim`, which is
    /// consumed but not written.
    ///
    /// `pending` is upstream's `delbuf`: the delimiters of columns whose files
    /// have already closed, held back in case nothing follows them on this
    /// line. They are flushed the moment this file turns out to have something,
    /// *before* any of its own bytes.
    ///
    /// The only error is a write error, which is fatal; a read error is latched
    /// into [`Self::failed`] and looks like end of file here.
    fn copy_line<W: Write>(
        &mut self,
        out: &mut W,
        line_delim: u8,
        pending: &mut Vec<u8>,
    ) -> Result<Stop, Trouble> {
        let mut read = false;
        loop {
            let ended = Stop {
                read,
                terminator: None,
            };
            if self.failed.is_some() {
                return Ok(ended);
            }
            // The chunk is copied out inside the arm rather than escaping the
            // match: a reference that outlived it would keep the reader
            // borrowed, and the error arm has to write to `self.failed`.
            let (body, terminated) = match self.reader.fill_buf() {
                Ok(chunk) => {
                    if chunk.is_empty() {
                        return Ok(ended);
                    }
                    let end = chunk.iter().position(|&b| b == line_delim);
                    (
                        chunk
                            .get(..end.unwrap_or(chunk.len()))
                            .unwrap_or_default()
                            .to_vec(),
                        end.is_some(),
                    )
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.failed = Some(e);
                    return Ok(ended);
                }
            };
            self.reader
                .consume(body.len().saturating_add(usize::from(terminated)));

            if !read {
                read = true;
                if !pending.is_empty() {
                    out.write_all(pending).map_err(Trouble::Write)?;
                    pending.clear();
                }
            }
            out.write_all(&body).map_err(Trouble::Write)?;
            if terminated {
                return Ok(Stop {
                    read,
                    terminator: Some(line_delim),
                });
            }
        }
    }
}

/// One operand's slot in parallel mode.
///
/// `Stdin` carries no reader of its own: every `-` on the command line is the
/// same stream, so they all defer to the one [`Source`] `main` holds.
enum Slot {
    Stdin,
    File(Source),
}

impl Slot {
    fn source<'a>(&'a mut self, stdin: &'a mut Source) -> &'a mut Source {
        match self {
            Self::Stdin => stdin,
            Self::File(source) => source,
        }
    }
}

fn open_file(name: &OsString) -> Result<File, Trouble> {
    File::open(name).map_err(|e| Trouble::Open(name.clone(), e))
}

/// True if this handle is file descriptor 0 — which can only happen when
/// standard input has been closed and the open reused its slot.
#[cfg(unix)]
fn is_stdin_fd(file: &File) -> bool {
    use std::os::unix::io::AsRawFd;
    file.as_raw_fd() == 0
}

#[cfg(not(unix))]
fn is_stdin_fd(_file: &File) -> bool {
    // No file-descriptor numbering to collide with, so the check that depends
    // on it can never fire.
    false
}

/// Open every operand before writing anything, as parallel mode must.
///
/// A failure here is fatal and names only the operand that failed, because
/// upstream's `error (EXIT_FAILURE, …)` does not return.
fn open_all(names: &[OsString]) -> Result<Vec<Option<Slot>>, Trouble> {
    let mut slots = Vec::with_capacity(names.len());
    let mut have_read_stdin = false;
    let mut opened_stdin = false;
    for name in names {
        if name == "-" {
            have_read_stdin = true;
            slots.push(Some(Slot::Stdin));
        } else {
            let file = open_file(name)?;
            opened_stdin |= is_stdin_fd(&file);
            slots.push(Some(Slot::File(Source::new(Box::new(BufReader::new(
                file,
            ))))));
        }
    }
    if opened_stdin && have_read_stdin {
        return Err(Trouble::StdinClosed);
    }
    Ok(slots)
}

// -------------------------------------------------------------------- pasting

/// One line from each file, in order, separated by the cycling delimiter list.
///
/// Upstream's `paste_parallel`, with two of its shapes worth naming. The inner
/// loop stops early when the last open file closes, so a round that closes
/// everything writes no line delimiter at all — that is what keeps `paste A B`
/// from ending in a blank line. And the non-last-file branch upstream contains
/// `if (chr != line_delim && chr != EOF) xputchar (chr);`, which cannot fire:
/// the loop above it only exits on those two values. It is not reproduced.
fn paste_parallel<W: Write>(
    names: &[OsString],
    slots: &mut [Option<Slot>],
    stdin: &mut Source,
    settings: &Settings,
    out: &mut W,
) -> Result<bool, Trouble> {
    let mut ok = true;
    let mut delims = Delims::new(&settings.delims);
    let mut pending: Vec<u8> = Vec::new();
    let mut files_open = slots.iter().filter(|slot| slot.is_some()).count();
    let count = slots.len();

    while files_open > 0 {
        // Both the delimiter cycle and the held-back delimiters are per output
        // line, not per run.
        let mut somedone = false;
        delims.reset();
        pending.clear();

        for i in 0..count {
            if files_open == 0 {
                break;
            }
            let last = i.saturating_add(1) == count;

            let stop = match slots.get_mut(i).and_then(Option::as_mut) {
                Some(slot) => {
                    slot.source(stdin)
                        .copy_line(out, settings.line_delim, &mut pending)?
                }
                None => Stop {
                    read: false,
                    terminator: None,
                },
            };

            if stop.read {
                somedone = true;
                if last {
                    // POSIX requires a line delimiter even where the input ran
                    // out without one.
                    out.write_all(&[stop.terminator.unwrap_or(settings.line_delim)])
                        .map_err(Trouble::Write)?;
                } else if let Some(delim) = delims.take() {
                    out.write_all(&[delim]).map_err(Trouble::Write)?;
                }
                continue;
            }

            // End of file, a read error, or a slot closed on an earlier round.
            if let Some(slot) = slots.get_mut(i)
                && let Some(mut closing) = slot.take()
            {
                // Taking it clears it, which is upstream's `clearerr` for
                // standard input: the error is reported once.
                if let Some(e) = closing.source(stdin).failed.take() {
                    let named = names.get(i).map_or_else(OsString::new, Clone::clone);
                    eprintln!("paste: {}: {}", quotef_os(&named), strerror(&e));
                    ok = false;
                }
                files_open = files_open.saturating_sub(1);
            }

            if last {
                if somedone {
                    // Some column on this line had data, so the delimiters held
                    // back for the closed ones do belong after all.
                    out.write_all(&pending).map_err(Trouble::Write)?;
                    pending.clear();
                    out.write_all(&[settings.line_delim])
                        .map_err(Trouble::Write)?;
                }
            } else if let Some(delim) = delims.take() {
                pending.push(delim);
            }
        }
    }
    Ok(ok)
}

/// Each file's own lines joined onto one output line.
///
/// Unlike parallel mode this opens files as it reaches them and keeps going
/// past one that will not open — upstream's `error (0, …)` — so every bad
/// operand is named and the exit status is the only thing they share.
fn paste_serial<W: Write>(
    names: &[OsString],
    stdin: &mut Source,
    settings: &Settings,
    out: &mut W,
) -> Result<bool, Trouble> {
    let mut ok = true;
    for name in names {
        let mut opened;
        let source = if name == "-" {
            &mut *stdin
        } else {
            match File::open(name) {
                Ok(file) => {
                    opened = Source::new(Box::new(BufReader::new(file)));
                    &mut opened
                }
                Err(e) => {
                    eprintln!("paste: {}: {}", quotef_os(name), strerror(&e));
                    ok = false;
                    continue;
                }
            }
        };
        merge_one(source, settings, out)?;
        if let Some(e) = source.failed.take() {
            eprintln!("paste: {}: {}", quotef_os(name), strerror(&e));
            ok = false;
        }
    }
    Ok(ok)
}

/// One file's lines joined with the cycling delimiter list.
///
/// The delimiter cycle restarts here, per file. The file's **last** byte is
/// written unchanged whatever it is — that is upstream's `xputchar (charold)`
/// after the loop — so a trailing newline stays a newline rather than becoming
/// a separator, and only a file that does not end in one gains a delimiter.
fn merge_one<W: Write>(
    source: &mut Source,
    settings: &Settings,
    out: &mut W,
) -> Result<(), Trouble> {
    let mut delims = Delims::new(&settings.delims);
    // Upstream's `charold`: one byte of lookahead, because whether a byte is
    // interior or final is not known until the next read.
    let mut held: Option<u8> = None;
    loop {
        if source.failed.is_some() {
            break;
        }
        // Copied out inside the arm for the same reason as in `copy_line`.
        let body: Vec<u8> = match source.reader.fill_buf() {
            Ok(chunk) => {
                if chunk.is_empty() {
                    break;
                }
                chunk.to_vec()
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                source.failed = Some(e);
                break;
            }
        };
        source.reader.consume(body.len());
        for &byte in &body {
            if let Some(previous) = held.replace(byte) {
                write_interior(previous, settings.line_delim, &mut delims, out)?;
            }
        }
    }

    match held {
        Some(last) => {
            out.write_all(&[last]).map_err(Trouble::Write)?;
            if last != settings.line_delim {
                out.write_all(&[settings.line_delim])
                    .map_err(Trouble::Write)?;
            }
        }
        // An empty file: upstream's `charold` is `EOF`, which is not the line
        // delimiter, so one is printed and the file becomes a blank line.
        None => out
            .write_all(&[settings.line_delim])
            .map_err(Trouble::Write)?,
    }
    Ok(())
}

/// A byte that is known not to be the file's last: a line delimiter becomes the
/// next delimiter from the list, anything else passes through.
fn write_interior<W: Write>(
    byte: u8,
    line_delim: u8,
    delims: &mut Delims,
    out: &mut W,
) -> Result<(), Trouble> {
    if byte == line_delim {
        if let Some(delim) = delims.take() {
            out.write_all(&[delim]).map_err(Trouble::Write)?;
        }
    } else {
        out.write_all(&[byte]).map_err(Trouble::Write)?;
    }
    Ok(())
}

// -------------------------------------------------------------------- parsing

fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut serial = false;
    let mut line_delim = b'\n';
    // Kept raw until the whole line is read, because the trailing-backslash
    // diagnostic is raised after the option loop and quotes what was typed.
    let mut delim_arg: Vec<u8> = vec![b'\t'];
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
            if let Some(request) = long_option(
                body,
                &bytes,
                args,
                &mut at,
                &mut serial,
                &mut line_delim,
                &mut delim_arg,
            )? {
                return Ok(request);
            }
        } else {
            short_options(
                &bytes,
                args,
                &mut at,
                &mut serial,
                &mut line_delim,
                &mut delim_arg,
            )?;
        }
    }

    let delims = collapse_escapes(&delim_arg).map_err(|()| {
        Refusal::Delimiters(format!(
            "delimiter list ends with an unescaped backslash: {}",
            quote_c_maybe_colon(&delim_arg)
        ))
    })?;
    Ok(Request::Run(
        Settings {
            delims,
            serial,
            line_delim,
        },
        files,
    ))
}

/// `-d`'s argument. An empty one becomes the two characters `\0`, exactly as
/// upstream's `optarg[0] == '\0' ? "\\0" : optarg` — so it collapses to a
/// one-position list holding "no delimiter", not to an empty list that would
/// have nothing to cycle through.
fn set_delim_arg(value: &[u8], delim_arg: &mut Vec<u8>) {
    *delim_arg = if value.is_empty() {
        b"\\0".to_vec()
    } else {
        value.to_vec()
    };
}

/// One `--name`, `--name=value` or `--name value` argument.
fn long_option(
    body: &[u8],
    whole: &[u8],
    args: &[OsString],
    next: &mut usize,
    serial: &mut bool,
    line_delim: &mut u8,
    delim_arg: &mut Vec<u8>,
) -> Result<Option<Request>, Refusal> {
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
    let typed = std::str::from_utf8(typed)
        .map_err(|_| Refusal::Getopt(PASTE.unrecognized_option(whole)))?;
    let (name, which) = PASTE
        .resolve_long(typed, whole, LONG_OPTIONS)
        .map_err(Refusal::Getopt)?;

    match which {
        Long::Delimiters => {
            // A required argument may be written either way — `--delimiters=,`
            // or `--delimiters ,` — so only the last argument on the line can
            // leave it genuinely missing.
            let value = match inline {
                Some(value) => value.to_vec(),
                None => {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(PASTE.long_missing_argument(name)));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                }
            };
            set_delim_arg(&value, delim_arg);
        }
        Long::Serial | Long::ZeroTerminated | Long::Help | Long::Version => {
            if inline.is_some() {
                return Err(Refusal::Getopt(PASTE.long_unwanted_argument(name)));
            }
            match which {
                Long::Serial => *serial = true,
                Long::ZeroTerminated => *line_delim = 0,
                Long::Help => return Ok(Some(Request::Help)),
                Long::Version => return Ok(Some(Request::Version)),
                Long::Delimiters => {}
            }
        }
    }
    Ok(None)
}

/// One `-abc` cluster.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    next: &mut usize,
    serial: &mut bool,
    line_delim: &mut u8,
    delim_arg: &mut Vec<u8>,
) -> Result<(), Refusal> {
    let cluster = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = cluster.get(at) {
        match c {
            b's' => *serial = true,
            b'z' => *line_delim = 0,
            b'd' => {
                // A *required* argument: the rest of the cluster if there is
                // one, otherwise the whole of the next argument. So a cluster
                // ends at `-d` — in `-sd,` the `,` is the list, and in `-ds,`
                // the list is `s,`.
                let rest = cluster.get(at.saturating_add(1)..).unwrap_or_default();
                let value = if rest.is_empty() {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(PASTE.short_missing_argument(b'd')));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                } else {
                    rest.to_vec()
                };
                set_delim_arg(&value, delim_arg);
                return Ok(());
            }
            _ => return Err(Refusal::Getopt(PASTE.invalid_option(c))),
        }
        at = at.saturating_add(1);
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

    const A: &[u8] = b"a1\na2\na3\n";
    const B: &[u8] = b"b1\nb2\n";
    const C: &[u8] = b"c1\n";
    /// Empty.
    const E: &[u8] = b"";
    /// Unterminated.
    const U: &[u8] = b"x1\nx2";

    fn settings(options: &[&str]) -> Settings {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        match parse_args(&args) {
            Ok(Request::Run(settings, _)) => settings,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn operands(options: &[&str]) -> Vec<OsString> {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        match parse_args(&args) {
            Ok(Request::Run(_, files)) => files,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// The diagnostic one command line produces, without the `paste: ` prefix
    /// and without the referral.
    fn refuse(options: &[&str]) -> String {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        match parse_args(&args) {
            Ok(other) => panic!("expected a refusal, got {other:?}"),
            Err(Refusal::Getopt(e)) => e.sentence,
            Err(Refusal::Delimiters(message)) => message,
        }
    }

    fn source(body: &[u8]) -> Source {
        Source::new(Box::new(io::Cursor::new(body.to_vec())))
    }

    fn parallel(options: &[&str], files: &[&[u8]]) -> Vec<u8> {
        let settings = settings(options);
        let names: Vec<OsString> = (0..files.len())
            .map(|i| OsString::from(format!("f{i}")))
            .collect();
        let mut slots: Vec<Option<Slot>> = files
            .iter()
            .map(|body| Some(Slot::File(source(body))))
            .collect();
        let mut stdin = source(b"");
        let mut out: Vec<u8> = Vec::new();
        paste_parallel(&names, &mut slots, &mut stdin, &settings, &mut out)
            .expect("no write can fail");
        out
    }

    fn serial(options: &[&str], files: &[&[u8]]) -> Vec<u8> {
        let settings = settings(options);
        let mut out: Vec<u8> = Vec::new();
        for body in files {
            merge_one(&mut source(body), &settings, &mut out).expect("no write can fail");
        }
        out
    }

    fn text(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("ascii in, ascii out")
    }

    #[test]
    fn parallel_takes_one_line_from_each_file() {
        assert_eq!(text(parallel(&[], &[A, B])), "a1\tb1\na2\tb2\na3\t\n");
        assert_eq!(text(parallel(&[], &[A])), "a1\na2\na3\n");
    }

    #[test]
    fn parallel_pads_the_files_that_ran_out() {
        // The delimiters of the closed columns are still printed, because a
        // later column had something — but only up to the last one that did.
        assert_eq!(
            text(parallel(&[], &[A, B, C])),
            "a1\tb1\tc1\na2\tb2\t\na3\t\t\n"
        );
        assert_eq!(
            text(parallel(&[], &[C, B, A])),
            "c1\tb1\ta1\n\tb2\ta2\n\t\ta3\n"
        );
    }

    #[test]
    fn the_delimiter_list_cycles_within_a_line_and_restarts_at_each() {
        assert_eq!(
            text(parallel(&["-d", ",;"], &[A, B, C])),
            "a1,b1;c1\na2,b2;\na3,;\n"
        );
        // Serial cycles within the file and restarts at the next one.
        assert_eq!(text(serial(&["-d", ",;"], &[A, B])), "a1,a2;a3\nb1,b2\n");
    }

    #[test]
    fn a_nul_position_writes_nothing_but_still_costs_a_turn() {
        // `x`, then nothing, then back to `x`: columns 1|2 get `x`, 2|3 get
        // nothing. A list that merely dropped the position would put `y` there.
        assert_eq!(
            text(parallel(&["-d", "x\\0y"], &[A, B, C])),
            "a1xb1c1\na2xb2\na3x\n"
        );
    }

    #[test]
    fn an_empty_delimiter_argument_becomes_the_empty_position() {
        assert_eq!(settings(&["-d", ""]).delims, vec![0]);
        assert_eq!(text(parallel(&["-d", ""], &[A, B])), "a1b1\na2b2\na3\n");
        assert_eq!(text(serial(&["-d", ""], &[A])), "a1a2a3\n");
    }

    #[test]
    fn the_backslash_escapes() {
        assert_eq!(
            collapse_escapes(b"\\b\\f\\n\\r\\t\\v\\\\\\q"),
            Ok(b"\x08\x0c\n\r\t\x0b\\q".to_vec())
        );
        // An even run of backslashes is fine; an odd one is not.
        assert_eq!(collapse_escapes(b"a\\\\"), Ok(b"a\\".to_vec()));
        assert_eq!(collapse_escapes(b"a\\\\\\"), Err(()));
    }

    #[test]
    fn a_trailing_lone_backslash_is_fatal_and_quoted_c_style() {
        // The argument is quoted as typed, before collapsing, and in `c_maybe`
        // style — so the single backslash stays single and stays bare.
        assert_eq!(
            refuse(&["-d", "a\\"]),
            "delimiter list ends with an unescaped backslash: a\\"
        );
        assert_eq!(
            refuse(&["-d", "\\"]),
            "delimiter list ends with an unescaped backslash: \\"
        );
        // …but a byte that cannot be written bare turns the quotes on.
        assert_eq!(
            refuse(&["-d", "a\tb\\"]),
            "delimiter list ends with an unescaped backslash: \"a\\tb\\\\\""
        );
    }

    #[test]
    fn the_delimiter_error_is_raised_after_the_whole_command_line_is_read() {
        // A later `-d` rescues an earlier bad one…
        assert_eq!(settings(&["-d", "a\\", "-d", ","]).delims, vec![b',']);
        // …and a getopt error anywhere on the line preempts it, because that
        // one is raised inside the loop.
        assert_eq!(refuse(&["-d", "a\\", "-Q"]), "invalid option -- 'Q'");
        // A missing file does not preempt it: the files are opened later.
        assert_eq!(
            refuse(&["-d", "\\", "nosuch"]),
            "delimiter list ends with an unescaped backslash: \\"
        );
    }

    #[test]
    fn an_empty_file_is_nothing_in_parallel_but_a_bare_newline_in_serial() {
        assert_eq!(text(parallel(&[], &[E])), "");
        assert_eq!(text(serial(&[], &[E])), "\n");
        assert_eq!(text(serial(&[], &[E, A])), "\na1\ta2\ta3\n");
        // Its column is still spaced for, as long as something else has data.
        assert_eq!(text(parallel(&[], &[E, A])), "\ta1\n\ta2\n\ta3\n");
        assert_eq!(text(parallel(&[], &[A, E])), "a1\t\na2\t\na3\t\n");
    }

    #[test]
    fn an_unterminated_last_line_gains_a_delimiter() {
        assert_eq!(text(parallel(&[], &[U])), "x1\nx2\n");
        assert_eq!(text(serial(&[], &[U])), "x1\tx2\n");
        assert_eq!(text(parallel(&[], &[U, A])), "x1\ta1\nx2\ta2\n\ta3\n");
    }

    #[test]
    fn zero_terminated_changes_what_a_line_is_and_how_it_ends() {
        // No NUL in either file, so each is a single line, and the output ends
        // in a NUL rather than a newline.
        assert_eq!(
            parallel(&["-z"], &[A, B]),
            b"a1\na2\na3\n\tb1\nb2\n\0".to_vec()
        );
        assert_eq!(
            serial(&["-sz"], &[A, B]),
            b"a1\na2\na3\n\0b1\nb2\n\0".to_vec()
        );
        assert_eq!(serial(&["-zs"], &[E]), b"\0".to_vec());
        assert_eq!(
            serial(&["-z"], &[b"p\0q\0"]),
            b"p\tq\0".to_vec(),
            "the last byte is written raw, so the trailing NUL stays a NUL"
        );
    }

    #[test]
    fn the_delimiters_of_closed_files_are_held_back_until_something_follows() {
        // Nothing follows the closed first column on the last line, so its
        // delimiter is dropped rather than printed alone.
        assert_eq!(
            text(parallel(&[], &[C, C, A])),
            "c1\tc1\ta1\n\t\ta2\n\t\ta3\n"
        );
        // And when the *last* file is the one that closes, the held-back
        // delimiters are flushed before the line ends.
        assert_eq!(
            text(parallel(&[], &[A, C, C])),
            "a1\tc1\tc1\na2\t\t\na3\t\t\n"
        );
    }

    /// Gives `first`, then fails — the shape that makes upstream report a read
    /// error one output line after the read that failed.
    struct Flaky {
        first: Vec<u8>,
        spent: bool,
    }

    impl io::Read for Flaky {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            unreachable!("paste reads through fill_buf only");
        }
    }

    impl io::BufRead for Flaky {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.spent {
                return Err(io::Error::other("boom"));
            }
            Ok(&self.first)
        }

        fn consume(&mut self, amount: usize) {
            self.first.drain(..amount);
            self.spent = self.first.is_empty();
        }
    }

    #[test]
    fn a_read_error_surfaces_one_output_line_after_the_read_that_failed() {
        // The first file gives `p` and then fails, with no line delimiter — so
        // the failure is met while `sometodo` is already true, the file is not
        // closed, and its column still gets its delimiter. Only on the next
        // round, when it has nothing at all, is it closed and named.
        let names = vec![OsString::from("flaky"), OsString::from("g")];
        let mut slots = vec![
            Some(Slot::File(Source::new(Box::new(Flaky {
                first: b"p".to_vec(),
                spent: false,
            })))),
            Some(Slot::File(source(b"q\nr\n"))),
        ];
        let mut stdin = source(b"");
        let mut out: Vec<u8> = Vec::new();
        let ok = paste_parallel(&names, &mut slots, &mut stdin, &settings(&[]), &mut out)
            .expect("no write can fail");
        assert!(!ok, "a read error is an unsuccessful run");
        assert_eq!(text(out), "p\tq\n\tr\n");
    }

    #[test]
    fn every_spelling_of_the_options_agrees() {
        for options in [
            vec!["-s", "-d", ","],
            vec!["-sd,"],
            vec!["-s", "-d,"],
            vec!["--serial", "--delimiters=,"],
            vec!["--serial", "--delimiters", ","],
            vec!["--ser", "--d", ","],
        ] {
            let s = settings(&options);
            assert!(s.serial, "for {options:?}");
            assert_eq!(s.delims, vec![b','], "for {options:?}");
        }
    }

    #[test]
    fn a_cluster_ends_at_d_because_the_rest_is_its_argument() {
        // In `-ds,` the list is `s,`, not "serial then a comma".
        let s = settings(&["-ds,"]);
        assert!(!s.serial);
        assert_eq!(s.delims, vec![b's', b',']);
        // The other order is two options.
        let s = settings(&["-sd,"]);
        assert!(s.serial);
        assert_eq!(s.delims, vec![b',']);
    }

    #[test]
    fn the_last_delimiter_option_wins() {
        assert_eq!(settings(&["-d", ",", "-d", ":"]).delims, vec![b':']);
        assert_eq!(settings(&["-d", ",", "--delimiters="]).delims, vec![0]);
    }

    #[test]
    fn the_getopt_diagnostics() {
        assert_eq!(refuse(&["-Q"]), "invalid option -- 'Q'");
        assert_eq!(refuse(&["--nope"]), "unrecognized option '--nope'");
        assert_eq!(refuse(&["-d"]), "option requires an argument -- 'd'");
        assert_eq!(
            refuse(&["--delimiters"]),
            "option '--delimiters' requires an argument"
        );
        assert_eq!(
            refuse(&["--serial=x"]),
            "option '--serial' doesn't allow an argument"
        );
    }

    #[test]
    fn the_ambiguity_message_lists_the_candidates_in_declaration_order() {
        assert_eq!(
            refuse(&["--=x"]),
            "option '--=x' is ambiguous; possibilities: '--serial' '--delimiters' \
             '--zero-terminated' '--help' '--version'"
        );
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        assert_eq!(operands(&["--", "-d"]), vec![OsString::from("-d")]);
        assert_eq!(settings(&["--", "-d"]).delims, vec![b'\t']);
        // A lone `-` is an operand, not an option.
        assert_eq!(
            operands(&["-", "-s", "-"]),
            vec![OsString::from("-"), OsString::from("-")]
        );
    }

    #[test]
    fn bytes_that_are_not_text_pass_through_untouched() {
        assert_eq!(
            parallel(&[], &[b"\xff\xfe\n", b"\x80\n"]),
            b"\xff\xfe\t\x80\n".to_vec()
        );
        assert_eq!(settings(&["-d", "é"]).delims, vec![0xc3, 0xa9]);
    }
}
