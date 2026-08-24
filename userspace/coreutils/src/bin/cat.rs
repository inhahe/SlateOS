//! `cat` — concatenate files and print them on the standard output.
//!
//! ```text
//! cat [-AbeEnstTuv] [--] [FILE...]
//! ```
//!
//! | option | effect |
//! |---|---|
//! | `-n`, `--number` | number every output line |
//! | `-b`, `--number-nonblank` | number only non-empty lines; overrides `-n` |
//! | `-s`, `--squeeze-blank` | collapse a run of empty lines to one |
//! | `-E`, `--show-ends` | write `$` before each newline |
//! | `-T`, `--show-tabs` | write a tab as `^I` |
//! | `-v`, `--show-nonprinting` | write control and high bytes visibly |
//! | `-A`, `--show-all` | `-vET` |
//! | `-e` | `-vE` |
//! | `-t` | `-vT` |
//! | `-u` | accepted and ignored; output is unbuffered by POSIX fiat |
//!
//! ## What this used to be
//!
//! `cat [-n] [FILE...]`, and each of the gaps was worse than a missing option:
//!
//! * **Every exit was `0`.** A missing file printed a diagnostic and then
//!   reported success, so `cat "$f" > out || die` never fired. That is the
//!   failure mode that costs data: the caller believes it has the file.
//! * **An unrecognised option was taken as a filename.** `cat -A f` looked for
//!   a file named `-A`, said it did not exist — and exited 0. A typo could not
//!   be distinguished from a working command by anything a script can test.
//! * **`-n` went through `BufRead::lines()`, which is UTF-8 and `String`.** So
//!   `cat -n` on any file that is not valid UTF-8 stopped at the first bad byte
//!   — and on a CRLF file it *silently deleted the CR*, because `lines()`
//!   strips it. `cat` is the one program in the tree that must never alter a
//!   byte it was given.
//! * **Every write was `let _ = write!(…)`.** A full disk or a closed pipe was
//!   discarded, and — with the exit status hardcoded to 0 — a truncated copy
//!   was indistinguishable from a complete one.
//!
//! ## Text is bytes
//!
//! Nothing here decodes. Lines are split on `\n` with `read_until`, the body is
//! copied through as bytes, and a filename stays an `OsString` from `args_os`
//! to `File::open` so a name that is not valid Unicode still opens. Only
//! diagnostics render a name for a human, and only at the point of printing it.
//!
//! This matters more for `cat` than for its neighbours because `cat` is how a
//! script moves a file it does not understand — a tarball, an image, a
//! serialised index. Its correctness condition is byte-for-byte identity, and
//! any decoding step is a place that condition can fail.
//!
//! ## How `-v` renders a byte
//!
//! | byte | written as |
//! |---|---|
//! | `\n` | itself, always — it is the line terminator, not content |
//! | `\t` | itself, unless `-T`, which writes `^I` |
//! | `0x00`–`0x1f` | `^` and the byte plus `0x40` (`0x01` → `^A`) |
//! | `0x20`–`0x7e` | itself |
//! | `0x7f` | `^?` |
//! | `0x80`–`0xff` | `M-` and then the byte less `0x80` by these same rules |
//!
//! The last row is why `-v` is not a UTF-8 operation and must not become one:
//! `é` is `0xc3 0xa9` and renders as `M-C M-)`, two bytes shown as two bytes.
//! `-v` is a request to see the file's *bytes*, so decoding them first would
//! answer a different question.
//!
//! ## Checked against GNU
//!
//! `scripts/cat-diff.sh` runs this and the host's GNU `cat` over the same
//! command lines and compares stdout and the exit status byte for byte. The
//! corners it pinned down are the ones worth stating, because none is what a
//! reading of the manual would predict:
//!
//! * **`-E` writes no `$` on a final line that has no newline** — the `$`
//!   stands for the newline, so a line without one gets nothing.
//! * **`-b` beats `-n` whichever order they appear in**, so `-nb` and `-bn`
//!   are both `-b`.
//! * **`-s` and the line numbering share one counter across all the files**, so
//!   `cat -s a b` collapses a run that begins at the end of `a` and finishes at
//!   the start of `b`. The files are one stream, not a sequence of streams.
//! * **A "blank" line for `-b` and `-s` is exactly an empty one.** A line
//!   holding a single space is numbered by `-b` and never squeezed by `-s`.
//! * **A file that cannot be opened does not stop the run.** The remaining
//!   files are still copied; the exit status is 1 at the end.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quotef_os;
use coreutils::stdfd::{self, Stream};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::ExitCode;

// Before `main`, so that the `fstat` below still sees a caller's `cat >&-` as
// the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const USAGE: &str = "usage: cat [-AbeEnstTuv] [--] [FILE...]";

/// Which of the rendering and numbering options are in force.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Options {
    number: bool,
    number_nonblank: bool,
    squeeze: bool,
    show_ends: bool,
    show_tabs: bool,
    show_nonprinting: bool,
}

impl Options {
    /// Whether the output has to be assembled a line at a time.
    ///
    /// When nothing here is set, `cat` is a byte copy and takes the fast path;
    /// splitting a stream into lines only to rejoin it unchanged would cost
    /// throughput and could not change the answer.
    const fn line_oriented(self) -> bool {
        self.number
            || self.number_nonblank
            || self.squeeze
            || self.show_ends
            || self.show_tabs
            || self.show_nonprinting
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Copy these operands (`-` meaning standard input) under these options.
    Run(Options, Vec<OsString>),
    /// `--help` was given; print the usage and succeed.
    Help,
    /// `--version` was given.
    Version,
}

fn main() -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    stdfd::close_stderr(run_main(), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            // The referral is part of the message, and only the first line
            // carries the `cat: ` prefix — which is what GNU prints.
            diag!("cat: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let (options, mut files) = match request {
        Request::Help => return say(format!("{USAGE}\n").as_bytes()),
        Request::Version => return say(b"cat (SlateOS coreutils)\n"),
        Request::Run(options, files) => (options, files),
    };

    // `if (fstat (STDOUT_FILENO, &stat_buf) < 0) error (EXIT_FAILURE, errno,
    // _("standard output"));` — upstream's line, in upstream's place, which is
    // after the two options that print and before the first operand is opened.
    // The position is observable: `cat missing f >&-` names the descriptor and
    // never mentions `missing`, because it never got as far as opening it.
    if let Err(e) = stdfd::probe(1) {
        diag!("cat: standard output: {}", strerror(&e));
        return ExitCode::FAILURE;
    }

    // No operand means standard input, which is spelled the same way an
    // explicit request for it is, so the loop below has only the one case.
    if files.is_empty() {
        files.push(OsString::from("-"));
    }

    let mut out = Stream::stdout();
    let mut state = Numbering::default();
    let mut failed = false;

    for path in &files {
        let opened: io::Result<Box<dyn Read>> = if path == "-" {
            Ok(Box::new(io::stdin()))
        } else {
            File::open(path).map(|f| Box::new(f) as Box<dyn Read>)
        };
        let reader = match opened {
            Ok(r) => BufReader::new(r),
            Err(e) => {
                // A file we cannot open is not a reason to abandon the ones we
                // can: `cat a b > out` with `a` missing should still contain
                // `b`. The status carries the failure instead.
                diag!("cat: {}: {}", quotef_os(path), strerror(&e));
                failed = true;
                continue;
            }
        };

        let result = if options.line_oriented() {
            copy_lines(reader, &options, &mut state, &mut out)
        } else {
            copy_bytes(reader, &mut out)
        };
        // A write failure is fatal to the whole run, not to this file: there is
        // nowhere left to put the remaining ones. Measured — `cat a missing b
        // >/dev/full` prints one `write error` and nothing about `missing`,
        // because it stopped at `a`.
        //
        // The flush is what makes that true here, and it is not incidental. A
        // buffered stream would carry a 12-byte file all the way to the end of
        // the command line, so the failure would be discovered *after* `missing`
        // had already been opened and complained about — two diagnostics where
        // upstream prints one. Upstream delivers its pending output before
        // moving on, so this does too. Measured with `-n`, `-A` and `-s` as
        // well as the plain copy: all four abort at the first operand.
        //
        // The verdict arrives through `out.error()` rather than through `result`
        // because a `Stream` records a delivery failure instead of returning it,
        // the way `ferror` does. It is asked before `result` so that a write
        // failure outranks a read failure on the same file, as upstream's
        // `error (EXIT_FAILURE, …)` does by exiting on the spot.
        let _ = out.flush(); // never returns `Err`; the verdict is `out.error()`
        if let Some(e) = out.error() {
            stdfd::write_error("cat", &e);
            return ExitCode::FAILURE;
        }
        if let Err(e) = result {
            // A read failure is reported against the file it came from and the
            // run goes on.
            diag!("cat: {}: {}", e.subject(path), strerror(&e.error));
            failed = true;
        }
    }

    // Buffered output has to reach the OS before we can claim success; a flush
    // that fails here is a truncated copy reported as a complete one.
    if let Err(e) = out.finish() {
        stdfd::write_error("cat", &e);
        failed = true;
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Say one thing and stop — `--help` and `--version`.
///
/// These reach standard output *before* the `fstat` above, exactly as upstream
/// does, so `cat --help >&-` is `cat: write error: Bad file descriptor` (the
/// `close_stdout` wording) and not `cat: standard output: …` (the `fstat`
/// wording). Measured; the two differ and both are right.
fn say(bytes: &[u8]) -> ExitCode {
    let mut out = Stream::stdout();
    // `Stream::write_all` records rather than returns; the verdict is `finish`.
    let _ = out.write_all(bytes);
    if let Err(e) = out.finish() {
        stdfd::write_error("cat", &e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// An I/O failure, and which side of the copy it happened on.
struct Failure {
    error: io::Error,
    on_write: bool,
}

impl Failure {
    fn read(error: io::Error) -> Self {
        Self {
            error,
            on_write: false,
        }
    }
    fn write(error: io::Error) -> Self {
        Self {
            error,
            on_write: true,
        }
    }
    /// What to name in the diagnostic: the file for a read, the output for a
    /// write, since naming the input file for a full disk misdirects the reader.
    fn subject(&self, path: &OsString) -> String {
        if self.on_write {
            "write error".to_string()
        } else {
            quotef_os(path)
        }
    }
}

/// The counters that run across the whole command line rather than per file.
///
/// GNU treats the operands as one stream: `cat -n a b` keeps counting into `b`,
/// and `cat -s a b` collapses a blank run that straddles the join. Keeping this
/// outside the per-file loop is what makes both true.
struct Numbering {
    next: u64,
    in_blank_run: bool,
}

impl Default for Numbering {
    /// Line numbers start at one, so this is not a derive: a derived `Default`
    /// would start at zero and every number would be one short.
    fn default() -> Self {
        Self {
            next: 1,
            in_blank_run: false,
        }
    }
}

/// The fast path: no option needs the bytes examined, so none examines them.
fn copy_bytes(mut reader: impl Read, out: &mut impl Write) -> Result<(), Failure> {
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Failure::read(e)),
        };
        let filled = chunk.get(..n).unwrap_or(&[]);
        out.write_all(filled).map_err(Failure::write)?;
    }
}

/// The slow path: split on `\n`, decide about numbering and squeezing, render.
fn copy_lines(
    mut reader: impl BufRead,
    options: &Options,
    state: &mut Numbering,
    out: &mut impl Write,
) -> Result<(), Failure> {
    let mut line: Vec<u8> = Vec::new();
    let mut rendered: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Failure::read(e)),
        }

        // A final line need not be terminated, and the distinction is
        // load-bearing: `-E` marks the newline, so a line without one is
        // written without a `$`, and `cat` must not invent the terminator.
        let terminated = line.last() == Some(&b'\n');
        let body = if terminated {
            line.get(..line.len().saturating_sub(1))
        } else {
            line.get(..)
        };
        let body = body.unwrap_or(&[]);
        let blank = body.is_empty();

        if options.squeeze && blank {
            if state.in_blank_run {
                continue;
            }
            state.in_blank_run = true;
        } else {
            state.in_blank_run = false;
        }

        // `-b` overrides `-n` rather than combining with it, in either order,
        // so it is tested first and `-n` only reached when `-b` is absent.
        if options.number_nonblank {
            if !blank {
                write_number(state.next, out)?;
                state.next = state.next.saturating_add(1);
            }
        } else if options.number {
            write_number(state.next, out)?;
            state.next = state.next.saturating_add(1);
        }

        rendered.clear();
        render_body(body, options, &mut rendered);
        out.write_all(&rendered).map_err(Failure::write)?;

        if terminated {
            if options.show_ends {
                out.write_all(b"$").map_err(Failure::write)?;
            }
            out.write_all(b"\n").map_err(Failure::write)?;
        }
    }
}

/// Write a line number the way GNU does: six columns, right-aligned, then a tab.
fn write_number(n: u64, out: &mut impl Write) -> Result<(), Failure> {
    write!(out, "{n:6}\t").map_err(Failure::write)
}

/// Append a line's bytes to `out`, applying `-v` and `-T`.
///
/// Takes the whole line rather than a byte at a time so the common case — no
/// rendering asked for — is one `extend_from_slice` instead of a loop.
fn render_body(body: &[u8], options: &Options, out: &mut Vec<u8>) {
    if !options.show_tabs && !options.show_nonprinting {
        out.extend_from_slice(body);
        return;
    }
    for &b in body {
        match b {
            b'\t' if options.show_tabs => out.extend_from_slice(b"^I"),
            b'\t' => out.push(b'\t'),
            _ if options.show_nonprinting => render_visible(b, out),
            _ => out.push(b),
        }
    }
}

/// Append one byte in `-v` form.
fn render_visible(b: u8, out: &mut Vec<u8>) {
    // The high half is the low half with an `M-` in front — "meta", from the
    // terminal convention where the meta key set bit 7. Expressing it that way
    // rather than as a fourth case is not just brevity: it is why `M-^@` and
    // `M-^?` come out right without either being written down.
    if b >= 0x80 {
        out.extend_from_slice(b"M-");
        render_visible(b & 0x7f, out);
    } else if b == 0x7f {
        out.extend_from_slice(b"^?");
    } else if b < 0x20 {
        out.push(b'^');
        out.push(b.wrapping_add(0x40));
    } else {
        out.push(b);
    }
}

/// The name every diagnostic is stamped with, and the status a bad command
/// line exits with, both bound once.
///
/// The 1 is measured (`cat --zzz-bogus; echo $?`) and is the common case;
/// `sort`'s 2 is the exception, so this is a number to check rather than copy
/// from whichever utility was converted last.
const CAT: Program = Program::new("cat", 1);

/// Every long option `cat` knows, with what it takes — which is nothing, in
/// every case.
///
/// **The order is GNU's declaration order, not alphabetical**, because
/// `getopt_long` lists an ambiguous prefix's candidates in table order. It was
/// measured rather than recalled, with the one command that shows the whole
/// table — an empty prefix matches every option:
///
/// ```text
/// $ cat --=x
/// cat: option '--=x' is ambiguous; possibilities: '--number-nonblank' '--number'
///      '--squeeze-blank' '--show-nonprinting' '--show-ends' '--show-tabs'
///      '--show-all' '--help' '--version'
/// ```
///
/// Note `--number-nonblank` precedes `--number`, which is what makes `cat --num`
/// ambiguous rather than a match for `--number`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("number-nonblank", Takes::Nothing),
    ("number", Takes::Nothing),
    ("squeeze-blank", Takes::Nothing),
    ("show-nonprinting", Takes::Nothing),
    ("show-ends", Takes::Nothing),
    ("show-tabs", Takes::Nothing),
    ("show-all", Takes::Nothing),
    // `--help` and `--version` are in the table rather than special-cased
    // ahead of it, because getopt sees them too: they appear among an
    // ambiguous prefix's possibilities, and `cat --help=x` is measured to be
    // `option '--help' doesn't allow an argument`, not a printed usage.
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Parse `cat`'s argv.
///
/// Errors rather than guessing: an operand starting with `-` that is not an
/// option it knows is a mistake, and the old behaviour of treating it as a
/// filename turned every typo into a silent success.
///
/// Option resolution is [`coreutils::getopt`]'s, so long options abbreviate to
/// any unambiguous prefix the way every GNU utility's do — `cat --squeeze` and
/// `cat --show-a` work, and `cat --num` is refused as ambiguous rather than
/// silently taken for `--number`.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;

    for arg in args {
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
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            // `--name=value`: split before resolving, so the name is what gets
            // matched and the whole argument is what gets echoed back when it
            // resolves to nothing.
            let (typed, inline) = match body.iter().position(|&c| c == b'=') {
                Some(at) => (
                    body.get(..at).unwrap_or_default(),
                    Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
                ),
                None => (body, None),
            };
            // Every option is ASCII, so a name that is not UTF-8 matches none
            // of them; it takes the unrecognised path rather than erroring
            // differently, and is reported as the bytes that were typed.
            let typed = std::str::from_utf8(typed).map_err(|_| CAT.unrecognized_option(&bytes))?;
            let (name, _) = CAT.resolve_long(typed, &bytes, LONG_OPTIONS)?;
            if inline.is_some() {
                // Every one of cat's options is `Takes::Nothing`.
                return Err(CAT.long_unwanted_argument(name));
            }
            match name {
                "help" => return Ok(Request::Help),
                "version" => return Ok(Request::Version),
                _ => apply_long(name, &mut options),
            }
        } else {
            // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s
            // would report `invalid option -- 'é'`, an option nobody typed.
            for &b in bytes.get(1..).unwrap_or_default() {
                apply_short(b, &mut options)?;
            }
        }
    }

    Ok(Request::Run(options, files))
}

/// Apply an option [`LONG_OPTIONS`] already resolved, so the name is one of
/// that table's and there is no failure left to report.
fn apply_long(name: &str, options: &mut Options) {
    match name {
        "number" => options.number = true,
        "number-nonblank" => options.number_nonblank = true,
        "squeeze-blank" => options.squeeze = true,
        "show-ends" => options.show_ends = true,
        "show-tabs" => options.show_tabs = true,
        "show-nonprinting" => options.show_nonprinting = true,
        "show-all" => {
            options.show_nonprinting = true;
            options.show_ends = true;
            options.show_tabs = true;
        }
        // `--help` and `--version` are answered by the caller, and every other
        // name in the table is above; an unknown one cannot reach here.
        _ => {}
    }
}

fn apply_short(c: u8, options: &mut Options) -> Result<(), getopt::Error> {
    match c {
        b'n' => options.number = true,
        b'b' => options.number_nonblank = true,
        b's' => options.squeeze = true,
        b'E' => options.show_ends = true,
        b'T' => options.show_tabs = true,
        b'v' => options.show_nonprinting = true,
        b'A' => {
            options.show_nonprinting = true;
            options.show_ends = true;
            options.show_tabs = true;
        }
        b'e' => {
            options.show_nonprinting = true;
            options.show_ends = true;
        }
        b't' => {
            options.show_nonprinting = true;
            options.show_tabs = true;
        }
        // POSIX requires `-u` to be accepted and permits it to do nothing; the
        // output here is flushed at exit either way.
        b'u' => {}
        _ => return Err(CAT.invalid_option(c)),
    }
    Ok(())
}

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

    fn parse(items: &[&str]) -> (Options, Vec<OsString>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(o, f) => (o, f),
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// Run the line-oriented path over one input and return its bytes.
    fn run(input: &[u8], items: &[&str]) -> Vec<u8> {
        let (options, _) = parse(items);
        let mut out: Vec<u8> = Vec::new();
        let mut state = Numbering::default();
        copy_lines(io::BufReader::new(input), &options, &mut state, &mut out)
            .unwrap_or_else(|e| panic!("io: {}", e.error));
        out
    }

    // ------------------------------------------------------------ parsing

    #[test]
    fn no_arguments_reads_standard_input() {
        let (options, files) = parse(&[]);
        assert_eq!(options, Options::default());
        assert!(files.is_empty());
        assert!(!options.line_oriented());
    }

    #[test]
    fn a_lone_dash_is_an_operand_not_an_option() {
        let (_, files) = parse(&["-"]);
        assert_eq!(files, args(&["-"]));
    }

    #[test]
    fn short_options_bundle() {
        let (options, files) = parse(&["-nE", "f"]);
        assert!(options.number && options.show_ends);
        assert!(!options.show_tabs);
        assert_eq!(files, args(&["f"]));
    }

    #[test]
    fn show_all_is_the_other_three() {
        let (options, _) = parse(&["-A"]);
        assert!(options.show_nonprinting && options.show_ends && options.show_tabs);
        assert_eq!(parse(&["-vET"]).0, options);
        assert_eq!(parse(&["--show-all"]).0, options);
    }

    #[test]
    fn e_and_t_are_v_plus_one_more() {
        assert_eq!(parse(&["-e"]).0, parse(&["-vE"]).0);
        assert_eq!(parse(&["-t"]).0, parse(&["-vT"]).0);
    }

    #[test]
    fn long_options_match_their_short_forms() {
        assert_eq!(parse(&["--number"]).0, parse(&["-n"]).0);
        assert_eq!(parse(&["--number-nonblank"]).0, parse(&["-b"]).0);
        assert_eq!(parse(&["--squeeze-blank"]).0, parse(&["-s"]).0);
        assert_eq!(parse(&["--show-ends"]).0, parse(&["-E"]).0);
        assert_eq!(parse(&["--show-tabs"]).0, parse(&["-T"]).0);
        assert_eq!(parse(&["--show-nonprinting"]).0, parse(&["-v"]).0);
    }

    #[test]
    fn u_is_accepted_and_does_nothing() {
        let (options, files) = parse(&["-u", "f"]);
        assert_eq!(options, Options::default());
        assert_eq!(files, args(&["f"]));
    }

    #[test]
    fn double_dash_ends_the_options() {
        let (options, files) = parse(&["--", "-n"]);
        assert!(!options.number);
        assert_eq!(files, args(&["-n"]));
    }

    #[test]
    fn an_unknown_option_is_an_error_not_a_filename() {
        // The whole point of the rewrite: this used to look for a file called
        // `-Z`, fail to find it, and exit 0.
        let e = parse_args(&args(&["-Z", "f"])).unwrap_err();
        assert!(e.sentence.contains("invalid option"), "{e}");
        let e = parse_args(&args(&["--nope"])).unwrap_err();
        assert!(e.sentence.contains("unrecognized option"), "{e}");
    }

    /// The literals are glibc's, measured from `cat` under `LC_ALL=C`.
    fn fail_msg(items: &[&str]) -> String {
        let e = parse_args(&args(items)).unwrap_err();
        // Every one of these is status 1 for `cat` — not `sort`'s 2.
        assert_eq!(e.status, 1, "{e}");
        assert_eq!(
            e.referral,
            Some("cat"),
            "every option diagnostic ends with the referral"
        );
        e.sentence
    }

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        // These three were all refused before `cat` used the shared getopt,
        // which is the bug the module was written for.
        assert!(parse(&["--squeeze"]).0.squeeze);
        let (o, _) = parse(&["--show-a"]);
        assert!(o.show_nonprinting && o.show_ends && o.show_tabs);
        assert!(parse(&["--number-non"]).0.number_nonblank);
        // An exact match wins over the prefix rule: `--number` is a prefix of
        // nothing here, but `--show-all` is reached exactly while `--show` is
        // ambiguous.
        assert!(parse(&["--number"]).0.number);
        assert_eq!(
            fail_msg(&["--show"]),
            "option '--show' is ambiguous; possibilities: \
             '--show-nonprinting' '--show-ends' '--show-tabs' '--show-all'"
        );
        // `--num` is ambiguous rather than `--number`, because
        // `--number-nonblank` is declared first and also starts with it.
        assert_eq!(
            fail_msg(&["--num"]),
            "option '--num' is ambiguous; possibilities: '--number-nonblank' '--number'"
        );
    }

    #[test]
    fn every_getopt_sentence_matches_glibc() {
        assert_eq!(fail_msg(&["-x"]), "invalid option -- 'x'");
        assert_eq!(fail_msg(&["--nope"]), "unrecognized option '--nope'");
        // A name that resolved nothing is echoed whole, `=VALUE` included.
        assert_eq!(fail_msg(&["--nope=1"]), "unrecognized option '--nope=1'");
        // A name that resolved something is reported by its resolution.
        assert_eq!(
            fail_msg(&["--sq=1"]),
            "option '--squeeze-blank' doesn't allow an argument"
        );
        assert_eq!(
            fail_msg(&["--show-e=1"]),
            "option '--show-ends' doesn't allow an argument"
        );
        // Ambiguity is settled before the value is complained about, so an
        // ambiguous name with a value is an ambiguity — and is echoed whole,
        // since there is no resolution to name. Measured; it is not obvious
        // which of the two checks glibc runs first.
        assert_eq!(
            fail_msg(&["--numb=1"]),
            "option '--numb=1' is ambiguous; possibilities: '--number-nonblank' '--number'"
        );
        // `--help` is an ordinary table entry, so it gets the ordinary refusal
        // rather than printing the usage.
        assert_eq!(
            fail_msg(&["--help=x"]),
            "option '--help' doesn't allow an argument"
        );
        // The empty prefix matches everything: this is the instrument the
        // table's order was measured with, so it is also what pins the order.
        assert_eq!(
            fail_msg(&["--=x"]),
            "option '--=x' is ambiguous; possibilities: '--number-nonblank' \
             '--number' '--squeeze-blank' '--show-nonprinting' '--show-ends' \
             '--show-tabs' '--show-all' '--help' '--version'"
        );
    }

    #[test]
    fn an_option_name_cannot_forge_a_second_diagnostic_line() {
        // A file picked up by `cat *` may be named anything but `/` and NUL,
        // so a raw newline here would let it write a line `cat` never wrote.
        let e = fail_msg(&["--fo\ncat: /etc/shadow: Permission denied"]);
        assert_eq!(
            e,
            r#"unrecognized option '--fo\ncat: /etc/shadow: Permission denied'"#
        );
        assert_eq!(e.lines().count(), 1);
    }

    #[test]
    fn a_short_option_is_named_by_the_byte_not_the_char() {
        // `-é` is two bytes. Iterating `char`s reported `'é'`, an option
        // nobody typed; each byte is now named as itself.
        assert_eq!(fail_msg(&["-\u{e9}"]), r"invalid option -- '\303'");
    }

    #[test]
    fn an_unknown_option_inside_a_bundle_is_still_an_error() {
        assert!(parse_args(&args(&["-nZ"])).is_err());
    }

    #[test]
    fn help_and_version_are_recognised() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn a_plain_copy_does_not_take_the_line_path() {
        assert!(!parse(&["-u", "f"]).0.line_oriented());
        assert!(parse(&["-n"]).0.line_oriented());
        assert!(parse(&["-v"]).0.line_oriented());
    }

    // ------------------------------------------------------------ copying

    #[test]
    fn bytes_pass_through_untouched() {
        let input: &[u8] = b"\x00\x01\xff\r\nno newline at the end";
        let mut out: Vec<u8> = Vec::new();
        copy_bytes(input, &mut out).unwrap_or_else(|e| panic!("io: {}", e.error));
        assert_eq!(out, input);
    }

    #[test]
    fn a_carriage_return_survives_numbering() {
        // `BufRead::lines()`, which this used to use, strips the `\r` — so
        // `cat -n` silently rewrote every CRLF file it was given.
        assert_eq!(run(b"a\r\nb\r\n", &["-n"]), b"     1\ta\r\n     2\tb\r\n");
    }

    #[test]
    fn undecodable_bytes_survive_numbering() {
        assert_eq!(run(b"\xff\xfe\n", &["-n"]), b"     1\t\xff\xfe\n");
    }

    #[test]
    fn numbering_is_six_wide_and_grows_past_it() {
        assert_eq!(run(b"x\n", &["-n"]), b"     1\tx\n");
        let mut state = Numbering {
            next: 999_999,
            in_blank_run: false,
        };
        let mut out: Vec<u8> = Vec::new();
        copy_lines(
            io::BufReader::new(&b"a\nb\n"[..]),
            &parse(&["-n"]).0,
            &mut state,
            &mut out,
        )
        .unwrap_or_else(|e| panic!("io: {}", e.error));
        assert_eq!(out, b"999999\ta\n1000000\tb\n");
    }

    #[test]
    fn number_nonblank_skips_empty_lines() {
        assert_eq!(run(b"a\n\nb\n", &["-b"]), b"     1\ta\n\n     2\tb\n");
    }

    #[test]
    fn number_nonblank_overrides_number_in_either_order() {
        let expected: &[u8] = b"     1\ta\n\n     2\tb\n";
        assert_eq!(run(b"a\n\nb\n", &["-bn"]), expected);
        assert_eq!(run(b"a\n\nb\n", &["-nb"]), expected);
    }

    #[test]
    fn a_line_of_one_space_is_not_blank() {
        // For both `-b` and `-s`, "blank" is empty, not whitespace-only.
        assert_eq!(
            run(b"a\n \nb\n", &["-b"]),
            b"     1\ta\n     2\t \n     3\tb\n"
        );
        assert_eq!(run(b"a\n \n \nb\n", &["-s"]), b"a\n \n \nb\n");
    }

    #[test]
    fn squeeze_collapses_a_run_to_one() {
        assert_eq!(run(b"a\n\n\n\nb\n", &["-s"]), b"a\n\nb\n");
        assert_eq!(run(b"\n\n\na\n\n\n", &["-s"]), b"\na\n\n");
    }

    #[test]
    fn squeeze_happens_before_numbering() {
        // The line that survives is numbered; the ones removed never were.
        assert_eq!(
            run(b"a\n\n\n\nb\n", &["-ns"]),
            b"     1\ta\n     2\t\n     3\tb\n"
        );
    }

    #[test]
    fn numbering_and_squeezing_run_across_files() {
        // One stream, not a sequence of them: the counter carries over and a
        // blank run that straddles the join is still one run.
        let options = parse(&["-ns"]).0;
        let mut state = Numbering::default();
        let mut out: Vec<u8> = Vec::new();
        for input in [&b"a\n\n"[..], &b"\n\nb\n"[..]] {
            copy_lines(io::BufReader::new(input), &options, &mut state, &mut out)
                .unwrap_or_else(|e| panic!("io: {}", e.error));
        }
        assert_eq!(out, b"     1\ta\n     2\t\n     3\tb\n");
    }

    #[test]
    fn show_ends_marks_each_newline() {
        assert_eq!(run(b"a\n\nb\n", &["-E"]), b"a$\n$\nb$\n");
    }

    #[test]
    fn show_ends_marks_nothing_on_an_unterminated_last_line() {
        // The `$` stands for the newline. There isn't one, so there isn't a `$`
        // — and `cat` must not supply the missing terminator either.
        assert_eq!(run(b"a\nb", &["-E"]), b"a$\nb");
        assert_eq!(run(b"a\nb", &["-n"]), b"     1\ta\n     2\tb");
    }

    #[test]
    fn show_tabs_leaves_other_control_bytes_alone() {
        assert_eq!(run(b"a\tb\x01\n", &["-T"]), b"a^Ib\x01\n");
    }

    #[test]
    fn show_nonprinting_leaves_the_tab_alone() {
        // `-v` is about unprintable bytes; a tab prints. Only `-T` touches it.
        // Note the `^A` here is the rendering of `\x01`, and the tab beside it
        // is still a tab.
        assert_eq!(run(b"a\tb\x01\n", &["-v"]), b"a\tb^A\n");
    }

    #[test]
    fn show_nonprinting_renders_the_control_range() {
        assert_eq!(run(b"\x00\x01\x1f\n", &["-v"]), b"^@^A^_\n");
    }

    #[test]
    fn show_nonprinting_renders_delete_and_the_high_half() {
        assert_eq!(run(b"\x7f\x80\xff\n", &["-v"]), b"^?M-^@M-^?\n");
    }

    #[test]
    fn show_nonprinting_is_a_byte_operation_not_a_character_one() {
        // `é` is two bytes and is shown as two, because `-v` is a request to
        // see the bytes.
        assert_eq!(run("é\n".as_bytes(), &["-v"]), b"M-CM-)\n");
    }

    #[test]
    fn show_all_combines_all_three() {
        assert_eq!(run(b"a\tb\x01\xff\n", &["-A"]), b"a^Ib^AM-^?$\n");
    }

    #[test]
    fn a_newline_is_never_rendered_as_a_control_byte() {
        // It is the terminator, not content, so `-v` leaves it as a newline.
        assert_eq!(run(b"a\n", &["-v"]), b"a\n");
    }

    #[test]
    fn an_empty_input_produces_nothing() {
        assert_eq!(run(b"", &["-nsEA"]), b"");
        let mut out: Vec<u8> = Vec::new();
        copy_bytes(&b""[..], &mut out).unwrap_or_else(|e| panic!("io: {}", e.error));
        assert!(out.is_empty());
    }
}
