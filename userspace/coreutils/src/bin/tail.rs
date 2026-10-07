//! tail — output the last part of files.
//!
//! The fifth of the 85 utilities moved onto the shared [`coreutils::getopt`]
//! (see `known-issues.md` → `TD-COREUTILS-LONG-OPTIONS-DO-NOT-ABBREVIATE`), and
//! by some distance the largest: the parser this replaces knew `-n` and nothing
//! else, and the thing it did not have at all was `-f`. It also read input as
//! UTF-8 `String` lines, so a file that is not UTF-8 was truncated at the first
//! bad byte and `\r\n` came back out as `\n`; and it substituted 10 silently
//! for any count it could not parse, so `tail -n 5O file` (letter O) printed
//! ten lines rather than saying so.
//!
//! # Two option syntaxes, and the obsolete one is stricter than `head`'s
//!
//! `tail -3 file` is the pre-POSIX form, parsed by hand before `getopt_long`
//! ever runs. `head` recognises its own version of it whenever it is the first
//! argument; `tail` additionally requires that **the whole command line have
//! one of three shapes**, because the form is ambiguous with the modern one and
//! upstream will not guess:
//!
//! | Shape | Example |
//! |---|---|
//! | the option word alone | `tail -3` |
//! | the option word and one non-option | `tail -3 file`, `tail -3 -` |
//! | the option word, `--`, and at most one more | `tail -3 -- file` |
//!
//! Anything else — `tail -3 a b`, `tail -3 -q f`, `tail -q -3 f` — is refused,
//! and the digit that then reaches getopt produces `option used in invalid
//! context -- 3`. (`head` words the same situation `invalid trailing option --
//! 3`; the two utilities do not share the sentence.)
//!
//! Within the obsolete word the letters are **not** the modern flags. `b`, `c`
//! and `l` choose the unit, `f` means follow, and `b` is also a ×512
//! multiplier — but one applied in two different places depending on whether
//! digits were given, which is why `tail -b` is 5120 *bytes* while `tail -2b`
//! is 1024. Upstream scales its *default* by 512 in the first case, and in the
//! second hands the digits to `xstrtoumax` with `"b"` as the suffix list.
//!
//! # `--help` differs from GNU's on purpose
//!
//! GNU's help mentions `inotify` twice — that `--max-unchanged-stats` is
//! "rarely useful" with it, and that `--pid` is checked "at least once every N
//! seconds" with it. Both sentences are false here: this implementation always
//! polls, so `--max-unchanged-stats` is always in play and `--pid` is checked
//! exactly once per iteration. The clauses are dropped rather than copied.
//!
//! # Which route reads a file, and what its failures say
//!
//! `tail_lines` and `tail_bytes` are upstream's, decision for decision,
//! because the routes do not merely read differently -- they fail
//! differently. Both begin with `fstat` (`cannot fstat 'standard input'` is
//! a closed descriptor 0). A regular file is read backwards from the end it
//! had then, unless it will not seek to that end, and anything else is read
//! forwards with the last lines kept: `/proc/cpuinfo` is regular, says it is
//! empty and refuses `SEEK_END`, and is printed correctly only because of
//! that fallback. A failed read while still looking for the start says
//! `error reading` and goes on to the next operand; one while copying, and
//! any failed seek of a regular file, end the run there, as upstream's do.
//!
//! # Standard output
//!
//! Output goes through glibc's stdio buffer as upstream's does
//! ([`StdioFile`]), written two ways that fail differently: file data through
//! `xwrite_stdout`, which ends the run at its first failure with `error
//! writing 'standard output'`, and the `==> name <==` banners through
//! `printf`, whose failure stays in the stream for `close_stdout` to report
//! at the end as `write error`. So which sentence a full disk produces
//! depends on whether the buffer filled before the end, as upstream's does:
//! `tail -n1 f >/dev/full` is `write error: No space left on device`, and
//! `tail -n100000 big >/dev/full` is `error writing 'standard output': No
//! space left on device`.
//!
//! Every diagnostic flushes standard output first, because glibc's `error()`
//! does, and that decides a sentence too: after a failed flush there, the
//! close at the end finds nothing left to write and says only `write error`,
//! with no reason, as upstream's says.
//!
//! # `-f`, pipes and the reader that goes away
//!
//! A `-` that is a pipe is printed and then not followed, which POSIX
//! requires when it is the only operand; `printf x | tail -f` ends.
//!
//! A standard output that is a pipe is watched: once its reader has gone,
//! `tail` raises `SIGPIPE` on itself and, where that is ignored, exits 1 with
//! nothing said -- upstream's `check_output_alive`. Without it, `tail -f log |
//! head -1` would wait for the log to grow before it noticed. The loop puts
//! the files it polls into non-blocking mode, as upstream's does, so that a
//! FIFO among several files cannot stall the others; a single non-regular
//! file followed by descriptor is read blocking instead, a buffer at a time,
//! with the output flushed after each.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::filekind;
use coreutils::getopt::{self, Program, Takes};
use coreutils::posixver;
use coreutils::quote::{os_bytes, quote, quoteaf, quotef};
use coreutils::stdfd;
use coreutils::stdio::StdioFile;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io::{self, ErrorKind, IsTerminal, Read, Seek, SeekFrom};
use std::mem::ManuallyDrop;
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees the descriptors `tail`
// was given: a closed standard input is `cannot fstat 'standard input'` and a
// closed standard output a write error, as they are upstream, not the
// `/dev/null` Rust's runtime would put on each.
coreutils::guard_std_fds!();

/// glibc's `error (0, ...)`: standard output flushed first, as `error()` opens
/// with `fflush (stdout)`, then the message, and the run goes on. Every
/// diagnostic that can follow output leaves through this or [`die!`].
macro_rules! complain {
    ($out:expr, $($arg:tt)*) => {{
        $out.flush_before_diagnostic();
        diag!($($arg)*);
    }};
}

/// `error (EXIT_FAILURE, ...)`: as [`complain!`], and then the run ends --
/// through `close_stdout`, as upstream's `exit` runs it. See [`Out::die`].
macro_rules! die {
    ($out:expr, $($arg:tt)*) => {
        $out.die(format_args!($($arg)*))
    };
}

/// Measured: `tail --zzz; echo $?` is 1.
const TAIL: Program = Program::new("tail", 1);

/// The count with no `-n`/`-c`, and the number the help text quotes.
const DEFAULT_NUMBER: u64 = 10;

/// `--max-unchanged-stats`'s default, named in the help text.
const DEFAULT_MAX_UNCHANGED: u64 = 5;

/// `--sleep-interval`'s default, in seconds. Also named in the help text.
const DEFAULT_SLEEP: f64 = 1.0;

/// The largest value `--pid` accepts: glibc's `PID_T_MAX`, `pid_t` being `int`.
const PID_MAX: u64 = i32::MAX as u64;

/// The long options in **GNU's declaration order**, which is observable: it is
/// the order `getopt_long` lists candidates in when an abbreviation is
/// ambiguous. Measured with `tail --=x`, an empty prefix that matches every
/// entry and so prints the whole table.
///
/// Two entries are worth stopping on. `follow` takes an **optional** argument,
/// the only one here that does — so `--follow` and `--follow=name` are both
/// legal but `--follow name` is not, `name` being an operand. And
/// `-disable-inotify` and `-presume-input-pipe` are not typos: upstream hides
/// an option by giving it a name that begins with a dash, so the spelling a
/// user must type carries three of them.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Required),
    ("follow", Takes::Optional),
    ("lines", Takes::Required),
    ("max-unchanged-stats", Takes::Required),
    ("-disable-inotify", Takes::Nothing),
    ("pid", Takes::Required),
    ("-presume-input-pipe", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("retry", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("sleep-interval", Takes::Required),
    ("verbose", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--follow`'s argument, and the table `argmatch` resolves abbreviations of it
/// against — `--follow=d` is `descriptor`.
const FOLLOW_MODES: &[(&str, Follow)] =
    &[("descriptor", Follow::Descriptor), ("name", Follow::Name)];

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

/// What `-f` follows: the file that was opened, or whatever the name refers to
/// from moment to moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Follow {
    /// The default. A renamed file is still followed; a new file created under
    /// the old name is not.
    Descriptor,
    /// `--follow=name`, and half of `-F`. Survives log rotation.
    Name,
}

/// When to print a `==> name <==` banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Headers {
    Never,
    /// The default: only when there is more than one operand.
    Multiple,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Options {
    unit: Unit,
    n_units: u64,
    /// `-n +5`: count from the start and skip, rather than count from the end.
    /// Note that this is **sticky** upstream — the flag is set by a `+` and
    /// never cleared by a later `-n` without one, so `tail -n +2 -n 2` skips
    /// rather than printing two lines. Faithfully reproduced.
    from_start: bool,
    /// `-f`: do not stop at end of file.
    forever: bool,
    follow: Follow,
    /// `--retry`: keep trying to open a file that is not there yet.
    retry: bool,
    /// `--pid`: stop once this process has.
    pid: Option<u64>,
    sleep_interval: f64,
    max_unchanged: u64,
    headers: Headers,
    /// `-z` makes this NUL. It is what a "line" ends with everywhere below,
    /// which is why it is carried rather than hard-coded.
    line_end: u8,
    /// `---presume-input-pipe`: take the streaming path even for input that
    /// could be seeked. Unlike in `head`, where the option does nothing, this
    /// one is load-bearing — the two paths are separate code, and this is how
    /// the slower one gets exercised against a seekable file.
    presume_input_pipe: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            unit: Unit::Lines,
            n_units: DEFAULT_NUMBER,
            from_start: false,
            forever: false,
            follow: Follow::Descriptor,
            retry: false,
            pid: None,
            sleep_interval: DEFAULT_SLEEP,
            max_unchanged: DEFAULT_MAX_UNCHANGED,
            headers: Headers::Multiple,
            line_end: b'\n',
            presume_input_pipe: false,
        }
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq)]
enum Request {
    Help,
    Version,
    Run(Options, Vec<OsString>),
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::restore();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = Out::stdout();
    let earned = match parse_args(&args, getopt::posixly_correct(), posixver::posix2_version()) {
        Ok(Request::Help) => {
            out.print(&[help_text().as_bytes()]);
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            out.print(&[b"tail (SlateOS coreutils) 0.1.0\n"]);
            ExitCode::SUCCESS
        }
        Ok(Request::Run(options, files)) => {
            warn_about_unused(&options);
            run(&options, &files, &mut out)
        }
        Err(e) => {
            diag!("tail: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    };
    // `atexit (close_stdout)`.
    if out.close() {
        earned
    } else {
        ExitCode::from(1)
    }
}

/// Standard output as upstream's `tail` uses it.
///
/// It is glibc's buffer ([`StdioFile`]), written two ways that fail
/// differently. File data goes through `xwrite_stdout`, whose failure ends the
/// run at once with `error writing 'standard output'`; the banners go through
/// `printf`, whose failure stays in the stream for `close_stdout` to report as
/// `write error` at the end. Which of the two a full disk meets therefore
/// depends on whether the buffer filled before the end, as it does upstream.
struct Out {
    file: StdioFile,
}

/// `EBADF`, as glibc and SlateOS's POSIX layer number it.
const EBADF: i32 = 9;

impl Out {
    fn stdout() -> Self {
        Self {
            file: StdioFile::stdout(),
        }
    }

    /// `xwrite_stdout`: every byte written, or the run ends with `error writing
    /// 'standard output'`.
    fn xwrite(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Err(e) = self.file.write(data) {
            // "clearerr (stdout); /* To avoid redundant close_stdout
            // diagnostic. */"
            self.file.clear_error();
            die!(
                self,
                "tail: error writing {}: {}",
                quoteaf(b"standard output"),
                strerror(&e)
            );
        }
    }

    /// `printf` and `fputs`, in the pieces `vfprintf` writes a format in: a
    /// failure is the stream's, for [`Out::close`] to report at the end.
    fn print(&mut self, pieces: &[&[u8]]) {
        for piece in pieces {
            // `vfprintf` stops at the first piece that fails. The failure is
            // not lost: it is the stream's error flag, which `close` reads.
            if self.file.write(piece).is_err() {
                return;
            }
        }
    }

    /// The `fflush (stdout)` that glibc's `error()` opens with.
    ///
    /// Not merely a matter of order. A flush that fails here sets the stream's
    /// error flag and empties its buffer, so the close at the end finds nothing
    /// left to write: after `tail big nosuch >/dev/full`, upstream's last word
    /// is `write error` with no reason, because the reason was spent here.
    fn flush_before_diagnostic(&mut self) {
        // `error()` does not look at the result either; what a failure changes
        // is the stream's state, which `close` reads.
        let _ = self.file.flush();
    }

    /// The follow loop's `fflush (stdout)`, whose failure is gnulib's
    /// `write_error`: `write error: REASON`, and the run ends.
    fn flush_or_die(&mut self) {
        if let Err(e) = self.file.flush() {
            // `fpurge` and `clearerr`, "to avoid extraneous diagnostic from
            // close_stdout": the failed flush has already emptied the buffer.
            self.file.clear_error();
            die!(self, "tail: write error: {}", strerror(&e));
        }
    }

    /// `error (EXIT_FAILURE, ...)`: standard output flushed, the message, and
    /// `exit (EXIT_FAILURE)` -- which runs `close_stdout`, as upstream's
    /// `atexit` does on every way out.
    fn die(&mut self, message: std::fmt::Arguments<'_>) -> ! {
        self.flush_before_diagnostic();
        stdfd::diag_line(&message.to_string());
        self.exit(1)
    }

    /// `die_pipe`: "Ensure exit, either with SIGPIPE or EXIT_FAILURE status."
    fn die_pipe(&mut self) -> ! {
        raise_sigpipe();
        self.exit(1)
    }

    /// `exit (status)`: `close_stdout`, then the status -- or `exit_failure`
    /// where either standard stream failed on the way.
    fn exit(&mut self, status: u8) -> ! {
        let status = if self.close() { status } else { 1 };
        stdfd::exit_now(status, 1)
    }

    /// gnulib's `close_stdout`, for standard output: flush and close it, and
    /// say if that or anything earlier failed. `true` when nothing did.
    ///
    /// A stream that failed earlier and then closed cleanly is `write error`
    /// with no reason, since `close_stream` zeroes `errno` then; a descriptor
    /// that was never open is no failure when nothing was written to it.
    fn close(&mut self) -> bool {
        let prev_fail = self.file.has_error();
        let pending = self.file.pending() > 0;
        match self.file.close() {
            Ok(()) if !prev_fail => true,
            Ok(()) => {
                diag!("tail: write error");
                false
            }
            Err(e) if !prev_fail && !pending && e.raw_os_error() == Some(EBADF) => true,
            Err(e) => {
                diag!("tail: write error: {}", strerror(&e));
                false
            }
        }
    }
}

impl Sink for Out {
    fn xwrite(&mut self, data: &[u8]) {
        Out::xwrite(self, data);
    }
}

fn help_text() -> String {
    format!(
        "\
Usage: tail [OPTION]... [FILE]...
Print the last {DEFAULT_NUMBER} lines of each FILE to standard output.
With more than one FILE, precede each with a header giving the file name.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -c, --bytes=[+]NUM       output the last NUM bytes; or use -c +NUM to
                             output starting with byte NUM of each file
  -f, --follow[={{name|descriptor}}]
                           output appended data as the file grows;
                             an absent option argument means 'descriptor'
  -F                       same as --follow=name --retry
  -n, --lines=[+]NUM       output the last NUM lines, instead of the last \
{DEFAULT_NUMBER};
                             or use -n +NUM to skip NUM-1 lines at the start
      --max-unchanged-stats=N
                           with --follow=name, reopen a FILE which has not
                             changed size after N (default {DEFAULT_MAX_UNCHANGED}) iterations
                             to see if it has been unlinked or renamed
                             (this is the usual case of rotated log files)
      --pid=PID            with -f, terminate after process ID, PID dies
  -q, --quiet, --silent    never output headers giving file names
      --retry              keep trying to open a file if it is inaccessible
  -s, --sleep-interval=N   with -f, sleep for approximately N seconds
                             (default {DEFAULT_SLEEP:.1}) between iterations
  -v, --verbose            always output headers giving file names
  -z, --zero-terminated    line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

NUM may have a multiplier suffix:
b 512, kB 1000, K 1024, MB 1000*1000, M 1024*1024,
GB 1000*1000*1000, G 1024*1024*1024, and so on for T, P, E, Z, Y, R, Q.
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

With --follow (-f), tail defaults to following the file descriptor, which
means that even if a tail'ed file is renamed, tail will continue to track
its end.  This default behavior is not desirable when you really want to
track the actual name of the file, not the file descriptor (e.g., log
rotation).  Use --follow=name in that case.  That causes tail to track the
named file in a way that accommodates renaming, removal and creation.
"
    )
}

// ---------------------------------------------------------------- parsing ---

/// Parse argv: the obsolete `-NUM`/`+NUM` form first, if the command line has
/// one of the three shapes that form is allowed to take, and then
/// `getopt_long`.
///
/// # Errors
///
/// Any getopt diagnostic, plus `tail`'s own: a count, PID, iteration limit or
/// sleep interval that is not a number, and a digit reaching getopt (which
/// means an obsolete form that was not in one of the three shapes).
///
/// `posixly_correct` is [`getopt::posixly_correct`], passed in so that a test
/// can choose it. When it is set, the first operand ends option parsing, as it
/// does in glibc's getopt -- see "Where option parsing stops" in that module.
/// `posix2_version` is [`posixver::posix2_version`], passed in for the same
/// reason; it decides which obsolete forms [`parse_obsolete`] reads.
fn parse_args(
    args: &[OsString],
    posixly_correct: bool,
    posix2_version: i32,
) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut i = 0usize;

    if parse_obsolete(args, posix2_version, &mut options)? {
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
            // Under POSIXLY_CORRECT, glibc's getopt stops at the first operand.
            only_operands = posixly_correct;
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, &mut options)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut i, &mut options)?;
        }
    }

    Ok(Request::Run(options, files))
}

/// The obsolete `-NUM[bcl][f]` / `+NUM[bcl][f]` word, returning whether it was
/// there and consumed.
///
/// The shape test comes first and is upstream's verbatim, argument counts and
/// all: the form is only recognised on a command line that could not be
/// anything else. Note that it is a test on the *whole* command line, so
/// whether `-2` is a count or an error depends on what follows it two
/// arguments later.
///
/// Which forms are read depends on the edition `_POSIX2_VERSION` names, as
/// upstream's does. POSIX 1003.1-2001 withdrew `+NUM`, so in its window a
/// leading `+` is a file name; and before 2001 a bare `-` and a bare `-c` were
/// obsolete options too -- ten lines and ten bytes from the end -- where since
/// then they are standard input and an option wanting its argument. Measured
/// against GNU 9.4 on 2026-09-25: `_POSIX2_VERSION=200112 tail +2 f` fails to
/// open `+2`, and `_POSIX2_VERSION=199209 tail -c` prints the last ten bytes.
///
/// # Errors
///
/// Only one: digits that overflow, or whose `b` suffix overflows. A letter that
/// does not belong is not an error here — it makes the word not an obsolete
/// option, and it is then getopt's problem.
fn parse_obsolete(
    args: &[OsString],
    posix2_version: i32,
    options: &mut Options,
) -> Result<bool, getopt::Error> {
    let Some(first) = args.first() else {
        return Ok(false);
    };
    if !obsolete_shape(args) {
        return Ok(false);
    }
    let whole = arg_bytes(first);
    let mut at = 1usize;
    // Upstream's two readings of the edition. Before 2001 everything was
    // obsolete usage; from 2008 the traditional forms were allowed back; the
    // window between is the only one that refuses `+NUM`.
    let obsolete_usage = posix2_version < 200_112;
    let traditional_usage = !posixver::withdraws_obsolete_forms(posix2_version);
    let from_start = match whole.first() {
        // Upstream: "Leading "+" is a file name in the standard form."
        Some(b'+') if traditional_usage => true,
        Some(b'-') => {
            // Upstream: `if (!obsolete_usage && !p[p[0] == 'c']) return false;`
            // — under a modern `_POSIX2_VERSION`, a bare `-` is standard input
            // and a bare `-c` is an option needing an argument, so both must
            // fall through to getopt. Everything else starting with a dash is a
            // candidate, including `-f` and `-b`, which have no digits at all.
            let body = whole.get(1..).unwrap_or_default();
            let probe = usize::from(body.first() == Some(&b'c'));
            if !obsolete_usage && body.get(probe).is_none() {
                return Ok(false);
            }
            false
        }
        _ => return Ok(false),
    };

    let digits_from = at;
    while whole.get(at).is_some_and(u8::is_ascii_digit) {
        at = at.saturating_add(1);
    }
    let has_digits = at > digits_from;

    // The unit letter, which is also where the ×512 multiplier lives.
    let mut default_count = DEFAULT_NUMBER;
    let mut unit = Unit::Lines;
    match whole.get(at) {
        Some(b'b') => {
            default_count = default_count.saturating_mul(512);
            unit = Unit::Bytes;
            at = at.saturating_add(1);
        }
        Some(b'c') => {
            unit = Unit::Bytes;
            at = at.saturating_add(1);
        }
        Some(b'l') => at = at.saturating_add(1),
        _ => {}
    }

    let forever = whole.get(at) == Some(&b'f');
    if forever {
        at = at.saturating_add(1);
    }
    if at != whole.len() {
        // Trailing junk: not this form after all. `tail -2x f` ends up here and
        // is then refused by getopt for the digit, not for the `x`.
        return Ok(false);
    }

    options.n_units = if has_digits {
        // The digits *and everything after them* go to `xstrtoumax` with `b` as
        // the only suffix, which is how `-2b` becomes 1024 rather than 2. A
        // suffix character it does not know is masked off and ignored — that is
        // the `l` in `-2lf` — but an overflow is not.
        obsolete_number(whole.get(digits_from..).unwrap_or_default(), &whole)?
    } else {
        default_count
    };
    options.unit = unit;
    options.from_start = from_start;
    options.forever = forever;
    Ok(true)
}

/// Upstream's three-shape test, which decides whether the obsolete form is
/// looked for at all.
///
/// `args` here is `argv[1..]`, so upstream's `argc` is one more than its
/// length.
fn obsolete_shape(args: &[OsString]) -> bool {
    match args.len() {
        1 => true,
        // `! (argv[2][0] == '-' && argv[2][1])`: a second word may be an
        // operand or a lone `-`, but not an option.
        2 => args.get(1).is_some_and(|a| {
            let b = arg_bytes(a);
            !(b.first() == Some(&b'-') && b.len() > 1)
        }),
        3 => args.get(1).is_some_and(|a| arg_bytes(a) == b"--"),
        _ => false,
    }
}

/// `xstrtoumax(digits, nullptr, 10, &n, "b")` with `LONGINT_INVALID_SUFFIX_CHAR`
/// masked off — the obsolete form's number, which is a different parser from
/// the modern `-n`'s.
///
/// The differences that matter: only `b` is a suffix (and it means 512), and a
/// character that is not one is ignored rather than rejected. What is *not*
/// ignored is an overflow, which is reported against the whole original word.
fn obsolete_number(text: &[u8], whole: &[u8]) -> Result<u64, getopt::Error> {
    let mut value: u64 = 0;
    let mut at = 0usize;
    let mut overflowed = false;
    while let Some(d) = text.get(at).filter(|c| c.is_ascii_digit()) {
        at = at.saturating_add(1);
        let digit = u64::from(d.wrapping_sub(b'0'));
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => value = v,
            None => overflowed = true,
        }
    }
    if overflowed {
        value = u64::MAX;
    }
    if text.get(at) == Some(&b'b') {
        match value.checked_mul(512) {
            Some(v) => value = v,
            None => overflowed = true,
        }
    }
    if overflowed {
        // Upstream passes `errno` — ERANGE, set by `strtoumax` — to `error`, so
        // this one sentence ends in `strerror(ERANGE)` rather than in the fixed
        // wording `xdectoumax` uses, and it quotes the *whole* word including
        // its leading sign.
        return Err(TAIL.usage(format!(
            "invalid number: {}: Numerical result out of range",
            quote(whole)
        )));
    }
    Ok(value)
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
    // Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
    // `invalid option -- 'é'`, an option nobody typed.
    while let Some(&c) = body.get(at) {
        at = at.saturating_add(1);
        match c {
            b'q' => options.headers = Headers::Never,
            b'v' => options.headers = Headers::Always,
            b'z' => options.line_end = 0,
            b'f' => options.forever = true,
            b'F' => {
                options.forever = true;
                options.follow = Follow::Name;
                options.retry = true;
            }
            b'c' | b'n' | b's' => {
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
                            .ok_or_else(|| TAIL.short_missing_argument(c))?
                            .clone();
                        *i = i.saturating_add(1);
                        arg_bytes(&next)
                    }
                };
                if c == b's' {
                    options.sleep_interval = parse_seconds(&value)?;
                } else {
                    let unit = if c == b'c' { Unit::Bytes } else { Unit::Lines };
                    set_count(unit, &value, options)?;
                }
            }
            // A digit only reaches here when the obsolete form was not in one
            // of its three shapes — the form itself is handled before this loop
            // ever runs. Note the wording: `head` says `invalid trailing
            // option` for the same situation, and neither sentence carries the
            // `Try 'tail --help'` referral.
            b'0'..=b'9' => {
                return Err(TAIL.usage(format!(
                    "option used in invalid context -- {}",
                    char::from(c)
                )));
            }
            _ => return Err(TAIL.invalid_option(c)),
        }
    }
    Ok(())
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
    let typed = std::str::from_utf8(typed).map_err(|_| TAIL.unrecognized_option(bytes))?;
    let (name, takes) = TAIL.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(TAIL.long_unwanted_argument(name));
    }
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = args
                .get(*i)
                .ok_or_else(|| TAIL.long_missing_argument(name))?
                .clone();
            *i = i.saturating_add(1);
            Some(arg_bytes(&next))
        }
        // `Takes::Optional` never consumes the next argument: `--follow name`
        // is follow-by-descriptor of a file called `name`.
        (_, None) => None,
    };

    match name {
        "bytes" => set_count(Unit::Bytes, &value.unwrap_or_default(), options)?,
        "lines" => set_count(Unit::Lines, &value.unwrap_or_default(), options)?,
        "follow" => {
            options.forever = true;
            options.follow = match value {
                None => Follow::Descriptor,
                Some(v) => TAIL.argmatch(&v, "--follow", FOLLOW_MODES)?,
            };
        }
        "max-unchanged-stats" => {
            options.max_unchanged = parse_uint(
                &value.unwrap_or_default(),
                u64::MAX,
                "invalid maximum number of unchanged stats between opens",
            )?;
        }
        "pid" => {
            options.pid = Some(parse_uint(
                &value.unwrap_or_default(),
                PID_MAX,
                "invalid PID",
            )?);
        }
        "sleep-interval" => options.sleep_interval = parse_seconds(&value.unwrap_or_default())?,
        "retry" => options.retry = true,
        "quiet" | "silent" => options.headers = Headers::Never,
        "verbose" => options.headers = Headers::Always,
        "zero-terminated" => options.line_end = 0,
        // Upstream's two escape hatches for testing its own fast paths. We have
        // no inotify at all, so the first is already true and disabling it is a
        // no-op; the second forces the streaming reader over the seeking one,
        // which here is a real choice and is honoured.
        "-disable-inotify" => {}
        "-presume-input-pipe" => options.presume_input_pipe = true,
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // `resolve_long` returns only names from the table, all of which are
        // above.
        _ => {}
    }
    Ok(None)
}

/// Apply a `-n`/`-c` value, splitting off the leading sign.
///
/// `+` means "from the start", and is left on the string for the number parser
/// (which accepts it, as `strtoumax` does). `-` means the default, from the
/// end, and is taken *off* — which is why `tail -n -x` reports `'x'` while
/// `tail -n +x` reports `'+x'`.
///
/// Nothing here ever clears `from_start`. That is upstream's, and it is
/// observable: `tail -n +2 -n 2` skips the first line rather than printing the
/// last two.
fn set_count(unit: Unit, value: &[u8], options: &mut Options) -> Result<(), getopt::Error> {
    let text = match value.first() {
        Some(b'+') => {
            options.from_start = true;
            value
        }
        Some(b'-') => value.get(1..).unwrap_or_default(),
        _ => value,
    };
    options.unit = unit;
    options.n_units = parse_count(text, unit)?;
    Ok(())
}

/// The multiplier suffixes `tail` accepts, and the power of the base each
/// stands for.
///
/// This is upstream's `"bkKmMGTPEZYRQ0"` less the `0`, which is not a suffix at
/// all: it is gnulib's flag asking for the second-suffix base switch that
/// [`parse_count`] implements below.
const SUFFIXES: &[(u8, u32)] = &[
    (b'b', 0), // 512 exactly, handled below rather than as a power
    (b'k', 1),
    (b'K', 1),
    (b'm', 2),
    (b'M', 2),
    (b'G', 3),
    (b'T', 4),
    (b'P', 5),
    (b'E', 6),
    (b'Z', 7),
    (b'Y', 8),
    (b'R', 9),
    (b'Q', 10),
];

/// gnulib's `xdectoumax` as `tail` calls it for `-n`/`-c`: a decimal count with
/// an optional multiplier suffix. Identical to `head`'s, which is no
/// coincidence — both call the same function with the same suffix list.
///
/// The rules that are not guessable, all measured against glibc:
///
/// - **Leading whitespace and a leading `+` are accepted** (`strtoumax` skips
///   them), but trailing whitespace is not.
/// - **A bare suffix means one of it.** `tail -n K` is 1024 lines, because when
///   `strtoumax` consumes nothing gnulib substitutes 1 — but only if the very
///   first byte is itself a valid suffix, so `tail -n " K"` is still an error.
/// - **A second suffix changes the base.** `B` or `D` after the first make it a
///   power of 1000; `iB` keeps 1024.
/// - **A bad suffix outranks an overflow**, so the suffix must be validated
///   before the magnitude is reported.
///
/// # Errors
///
/// A number that does not parse, or one that overflows `u64`.
fn parse_count(text: &[u8], unit: Unit) -> Result<u64, getopt::Error> {
    // `quote`, not `quoteaf`: gnulib's `xdectoumax` echoes the offending text
    // with `quote()`, whose escaping is C's rather than the shell's.
    let invalid = || TAIL.usage(format!("{}: {}", unit.invalid_number(), quote(text)));
    let overflow = || {
        TAIL.usage(format!(
            "{}: {}: Value too large for defined data type",
            unit.invalid_number(),
            quote(text)
        ))
    };

    let (mut value, mut at, mut overflowed) = scan_uint(text, true).ok_or_else(invalid)?;

    // The suffix, if any.
    if let Some(&first) = text.get(at) {
        let Some(power) = suffix_power(first) else {
            return Err(invalid());
        };
        at = at.saturating_add(1);
        let base = match (text.get(at), text.get(at.saturating_add(1))) {
            // `iB` — explicitly binary, and the only use of a lone `i`.
            (Some(b'i'), Some(b'B')) => {
                at = at.saturating_add(2);
                1024u64
            }
            // `B` and the obsolescent `D` — decimal.
            (Some(b'B' | b'D'), _) => {
                at = at.saturating_add(1);
                1000u64
            }
            _ => 1024u64,
        };
        if at != text.len() {
            return Err(invalid());
        }
        let factor = if first == b'b' {
            // `b` is 512 flat and takes no second suffix — it is not in the
            // base-switching group at all.
            512u64
        } else {
            base.checked_pow(power).ok_or_else(overflow)?
        };
        match value.checked_mul(factor) {
            Some(v) => value = v,
            None => overflowed = true,
        }
    } else if at != text.len() {
        return Err(invalid());
    }
    if overflowed {
        return Err(overflow());
    }
    Ok(value)
}

/// `xdectoumax` with an **empty** suffix list and a ceiling — how `--pid` and
/// `--max-unchanged-stats` are parsed.
///
/// No suffix is accepted at all (`--pid=5k` is an error), and a value above
/// `max` reports the overflow wording rather than the invalid one, which is how
/// `--pid=99999999999999999999` and `--pid=x` come out differently.
///
/// # Errors
///
/// A number that does not parse, or one above `max`.
fn parse_uint(text: &[u8], max: u64, what: &str) -> Result<u64, getopt::Error> {
    let invalid = || TAIL.usage(format!("{what}: {}", quote(text)));
    let overflow = || {
        TAIL.usage(format!(
            "{what}: {}: Value too large for defined data type",
            quote(text)
        ))
    };

    // `false`: with no valid suffixes there is no bare-suffix fallback, so a
    // string with no digits in it is simply invalid.
    let (value, at, overflowed) = scan_uint(text, false).ok_or_else(invalid)?;
    if at != text.len() {
        return Err(invalid());
    }
    if overflowed || value > max {
        return Err(overflow());
    }
    Ok(value)
}

/// The `strtoumax` half of both parsers: optional whitespace, optional `+`,
/// then digits.
///
/// Returns the value, how far it got, and whether it overflowed — or `None`
/// when nothing at all was consumed and no fallback applies. `bare_suffix_ok`
/// is gnulib's rule that an argument which is *entirely* a suffix means one of
/// them; it looks at `text[0]`, before the whitespace was skipped, which is why
/// `" K"` does not qualify.
fn scan_uint(text: &[u8], bare_suffix_ok: bool) -> Option<(u64, usize, bool)> {
    let mut at = 0usize;
    while text.get(at).is_some_and(u8::is_ascii_whitespace) {
        at = at.saturating_add(1);
    }
    if text.get(at) == Some(&b'+') {
        at = at.saturating_add(1);
    }
    let digits_from = at;
    let mut value: u64 = 0;
    let mut overflowed = false;
    while let Some(d) = text.get(at).filter(|c| c.is_ascii_digit()) {
        at = at.saturating_add(1);
        let digit = u64::from(d.wrapping_sub(b'0'));
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => value = v,
            // `strtoumax` keeps consuming digits after ERANGE and returns
            // UINTMAX_MAX; whatever follows still has to be valid.
            None => overflowed = true,
        }
    }
    if at == digits_from {
        if !(bare_suffix_ok && text.first().is_some_and(|c| suffix_power(*c).is_some())) {
            return None;
        }
        at = 0;
        value = 1;
    }
    if overflowed {
        value = u64::MAX;
    }
    Some((value, at, overflowed))
}

fn suffix_power(c: u8) -> Option<u32> {
    SUFFIXES.iter().find(|(s, _)| *s == c).map(|(_, p)| *p)
}

/// `-s`'s value: gnulib's `xstrtod` with `strtod`, plus upstream's `0 <= s`.
///
/// `strtod`, not Rust's `f64::from_str`: leading whitespace is skipped, and a
/// hexadecimal float is a number. `nan` parses and is then rejected by the
/// range test, every comparison against it being false — which is upstream's
/// behaviour rather than a special case anyone wrote.
///
/// # Errors
///
/// Anything `strtod` would not consume entirely, and anything negative or NaN.
fn parse_seconds(text: &[u8]) -> Result<f64, getopt::Error> {
    let bad = || TAIL.usage(format!("invalid number of seconds: {}", quote(text)));
    let trimmed = {
        let mut at = 0usize;
        while text.get(at).is_some_and(u8::is_ascii_whitespace) {
            at = at.saturating_add(1);
        }
        text.get(at..).unwrap_or_default()
    };
    let body = std::str::from_utf8(trimmed).map_err(|_| bad())?;
    let value = match parse_hex_float(body) {
        Some(v) => v,
        None => body.parse::<f64>().map_err(|_| bad())?,
    };
    if value >= 0.0 { Ok(value) } else { Err(bad()) }
}

/// C99 hexadecimal floating point — `0x1.8p3` is 12 — which `strtod` accepts
/// and Rust's parser does not.
///
/// Returns `None` when the text is not in that form, leaving the decimal parser
/// to have its say; the two are disjoint because only this one starts `0x`.
fn parse_hex_float(text: &str) -> Option<f64> {
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'-') => (-1.0f64, text.get(1..)?),
        Some(b'+') => (1.0f64, text.get(1..)?),
        _ => (1.0f64, text),
    };
    let digits = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))?;
    let (mantissa, exponent) = match digits.find(['p', 'P']) {
        Some(at) => (digits.get(..at)?, Some(digits.get(at.saturating_add(1)..)?)),
        None => (digits, None),
    };
    let (whole, fraction) = match mantissa.find('.') {
        Some(at) => (mantissa.get(..at)?, mantissa.get(at.saturating_add(1)..)?),
        None => (mantissa, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let mut value = 0.0f64;
    for c in whole.chars() {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let mut scale = 1.0f64 / 16.0;
    for c in fraction.chars() {
        value += f64::from(c.to_digit(16)?) * scale;
        scale /= 16.0;
    }
    // The exponent is a *decimal* power of two, and is mandatory in C99 but
    // optional in glibc's `strtod`, which is what is being matched.
    if let Some(text) = exponent {
        let exponent: i32 = text.parse().ok()?;
        value *= 2.0f64.powi(exponent);
    }
    Some(sign * value)
}

/// The three warnings upstream prints after parsing, when an option was given
/// that the rest of the command line makes pointless.
///
/// They go to standard error, they stop nothing, and they do not change the
/// exit status. They are also *not* printed for `--help`/`--version`, which
/// return before this runs — hence the call site in `main` rather than at the
/// end of [`parse_args`].
fn warn_about_unused(options: &Options) {
    if options.retry {
        if !options.forever {
            diag!("tail: warning: --retry ignored; --retry is useful only when following");
        } else if options.follow == Follow::Descriptor {
            diag!("tail: warning: --retry only effective for the initial open");
        }
    }
    if options.pid.is_some() && !options.forever {
        diag!("tail: warning: PID ignored; --pid=PID is useful only when following");
    }
}

// --------------------------------------------------------------- printing ---

/// glibc's `BUFSIZ`: what upstream reads at a time, the block its backwards
/// scan walks a file in, and the most one read of the follow loop's blocking
/// mode takes before the output is flushed.
const BUFSIZ: usize = 8192;

/// gnulib's fallback for `ST_BLKSIZE`, for a file that reports no usable
/// block size.
const DEV_BSIZE: u64 = 512;

/// `OFF_T_MAX`: a count above it cannot be a seek.
const OFF_T_MAX: u64 = i64::MAX.unsigned_abs();

/// `EPERM` and `ENOENT`, as glibc and SlateOS's POSIX layer number them.
const EPERM: i32 = 1;
const ENOENT: i32 = 2;

/// `O_NONBLOCK`, which is the same number on Linux and here.
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;

/// Why a name is not currently being read: upstream's `f->errnum`, which is
/// `0` while the file is open and well, `-1` for a trouble that is not an
/// `errno` -- a file whose type cannot be followed, or one whose first read
/// failed -- and otherwise the `errno` of the failure.
///
/// The follow loop compares it with the last iteration's to decide whether a
/// diagnostic would be a repeat, which is why it is the number and not a
/// kind: two failures are the same failure when their `errno` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    None,
    NotAnErrno,
    Errno(i32),
}

/// A failure's `errno`. Every call whose failure is recorded here reports
/// one; a failure that somehow has none is given a number no `errno` is, so
/// that it compares equal only to itself.
fn errnum(e: &io::Error) -> i32 {
    e.raw_os_error().unwrap_or(i32::MIN)
}

/// An operand's open file.
///
/// Standard input is descriptor 0 itself, read in place as upstream reads it,
/// and so never closed by being dropped: it is closed once, at the end of the
/// run, where upstream checks that close (`have_read_stdin`). A duplicate
/// would share the position and so read the same bytes, but dropping one is
/// not the close upstream reports, and when descriptor 0 is closed there is
/// nothing to duplicate -- the reads must fail as they do upstream, with
/// `EBADF`.
enum Handle {
    /// A file `tail` opened, and closes.
    Opened(File),
    /// Descriptor 0, borrowed.
    Stdin(ManuallyDrop<File>),
}

impl std::ops::Deref for Handle {
    type Target = File;

    fn deref(&self) -> &File {
        match self {
            Handle::Opened(file) => file,
            Handle::Stdin(file) => file,
        }
    }
}

impl std::ops::DerefMut for Handle {
    fn deref_mut(&mut self) -> &mut File {
        match self {
            Handle::Opened(file) => file,
            Handle::Stdin(file) => file,
        }
    }
}

/// Descriptor 0, read in place: see [`Handle`].
fn stdin_handle() -> io::Result<Handle> {
    filekind::borrowed_stdin()
        .map(Handle::Stdin)
        // Only on a platform that is neither Unix nor Windows, where there is
        // no descriptor 0 to name: as if it were closed.
        .ok_or_else(|| io::Error::from_raw_os_error(EBADF))
}

/// One operand, plus everything the follow loop has to remember between
/// iterations -- upstream's `struct File_spec`.
struct Watched {
    /// The operand as typed, which is what gets reopened.
    name: OsString,
    /// What diagnostics and banners call it: `standard input` for `-`.
    label: Vec<u8>,
    is_stdin: bool,
    /// `None` once the file has been closed or could never be opened:
    /// upstream's `f->fd == -1`.
    file: Option<Handle>,
    trouble: Trouble,
    /// Set when there is no point looking at this name again.
    ignore: bool,
    /// Whether the last look found something that could be followed. Only used
    /// to decide whether a *change* is worth reporting.
    tailable: bool,
    /// How far the file has been read, and what its last `fstat` said, so
    /// that growth, truncation and replacement can be told apart -- what
    /// upstream's `record_open_fd` keeps.
    size: u64,
    modified: Option<std::time::SystemTime>,
    regular: bool,
    /// `S_ISFIFO`, for `ignore_fifo_and_pipe`.
    fifo: bool,
    id: Option<FileId>,
    /// Upstream's `f->blocking`: `None` for standard input, whose mode nobody
    /// has looked at yet (`-1`), and otherwise whether the descriptor is in
    /// blocking mode. `None` counts as blocking, as `-1` is true in C.
    blocking: Option<bool>,
    /// Consecutive iterations in which nothing about the file changed. Only
    /// `--follow=name` uses it, to decide when to look at the *name* again.
    unchanged: u64,
}

impl Watched {
    fn new(name: &OsString) -> Self {
        let bytes = arg_bytes(name);
        let is_stdin = bytes == b"-";
        Watched {
            name: name.clone(),
            label: if is_stdin {
                b"standard input".to_vec()
            } else {
                bytes
            },
            is_stdin,
            file: None,
            trouble: Trouble::None,
            ignore: false,
            tailable: true,
            size: 0,
            modified: None,
            regular: false,
            fifo: false,
            id: None,
            blocking: None,
            unchanged: 0,
        }
    }

    /// Whether reads of this file wait for data: upstream's `f->blocking`
    /// taken as a C truth value.
    fn reads_block(&self) -> bool {
        self.blocking != Some(false)
    }
}

/// Upstream's `record_open_fd`: keep the open file and what its `fstat` said.
fn record_open(
    w: &mut Watched,
    handle: Handle,
    stats: &Metadata,
    size: u64,
    blocking: Option<bool>,
) {
    w.regular = is_regular_file(&handle, stats);
    w.fifo = is_fifo(&handle, stats);
    w.size = size;
    w.modified = modification_time(stats);
    w.id = Some(file_id(stats));
    w.blocking = blocking;
    w.unchanged = 0;
    w.ignore = false;
    w.file = Some(handle);
}

/// `st_mtime`. A file system that keeps no modification time gives `None`
/// every time it is asked, which compares as a time that never changes --
/// the only thing the follow loop asks of it.
fn modification_time(stats: &Metadata) -> Option<std::time::SystemTime> {
    stats.modified().ok()
}

/// Run over every operand, returning the exit status -- the part of upstream's
/// `main` after the options are parsed.
fn run(options: &Options, files: &[OsString], out: &mut Out) -> ExitCode {
    let mut options = *options;
    // "To start printing with item N_UNITS from the start of the file, skip
    // N_UNITS - 1 items." `+0` and `+1` therefore mean the same thing, which is
    // upstream's stated concession to Unix compatibility.
    if options.from_start {
        options.n_units = options.n_units.saturating_sub(1);
    }

    let default = [OsString::from("-")];
    let operands: &[OsString] = if files.is_empty() { &default } else { files };
    let has_stdin = operands.iter().any(|f| arg_bytes(f) == b"-");

    // A name is what `--follow=name` follows, and standard input has none.
    if has_stdin && options.follow == Follow::Name {
        diag!("tail: cannot follow {} by name", quoteaf(b"-"));
        return ExitCode::from(1);
    }
    if options.forever && has_stdin {
        // Upstream's condition, which is subtler than it looks: the warning is
        // for the case where the loop would poll `fstat` on a terminal and
        // never see it change. When there is exactly one file, no `--pid` and
        // no `--follow=name`, the read itself blocks and does the waiting, so
        // there is nothing to warn about.
        let blocking = options.pid.is_none()
            && options.follow == Follow::Descriptor
            && operands.len() == 1
            && stdin_is_unregular();
        if !blocking && io::stdin().is_terminal() {
            diag!("tail: warning: following standard input indefinitely is ineffective");
        }
    }

    // Nothing will ever be printed, so nothing is opened and no banner appears
    // -- `tail -v -n0 file` prints nothing at all, not even the header.
    if options.n_units == 0 && !options.forever && !options.from_start {
        return ExitCode::SUCCESS;
    }

    let print_headers = match options.headers {
        Headers::Never => false,
        Headers::Always => true,
        Headers::Multiple => operands.len() > 1,
    };

    let mut watched: Vec<Watched> = operands.iter().map(Watched::new).collect();
    let mut ok = true;
    let mut first_header = true;

    for w in &mut watched {
        ok &= start_file(w, out, &options, print_headers, &mut first_header);
    }

    if options.forever && ignore_fifo_and_pipe(&mut watched) > 0 {
        // "If stdout is a fifo or pipe, then monitor it so that we exit if the
        // reader goes away."
        let monitor_output = match stdfd::metadata(1) {
            Ok(m) => is_fifo_metadata(&m),
            // The host, where no descriptor can be asked: unmonitored.
            Err(e) if e.kind() == ErrorKind::Unsupported => false,
            Err(e) => die!(out, "tail: standard output: {}", strerror(&e)),
        };
        follow(
            &mut watched,
            out,
            &options,
            print_headers,
            first_header,
            monitor_output,
        );
    }

    // `have_read_stdin`: every operand was reached, so a `-` among them was
    // read, and descriptor 0 is closed and the close checked.
    if has_stdin && let Err(e) = stdfd::close_stdin() {
        die!(out, "tail: -: {}", strerror(&e));
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Whether standard input is something other than a regular file -- which
/// decides whether a plain `read` on it would block. Not when it cannot be
/// asked: upstream's `! fstat (STDIN_FILENO, &in_stat) && ! S_ISREG (...)`.
fn stdin_is_unregular() -> bool {
    filekind::borrowed_stdin().and_then(|stdin| filekind::regular(&stdin)) == Some(false)
}

/// Open one operand and print the part of it that was asked for -- upstream's
/// `tail_file`.
///
/// Returns whether it succeeded. A failure is not fatal: the remaining operands
/// are still processed, and with `-f` the name may still be worth watching.
fn start_file(
    w: &mut Watched,
    out: &mut Out,
    options: &Options,
    print_headers: bool,
    first_header: &mut bool,
) -> bool {
    let opened = if w.is_stdin {
        stdin_handle()
    } else {
        // `open_safer`, as upstream's `fcntl--.h` makes every `open`: with
        // standard output closed, the file must not become descriptor 1, or
        // the `fstat` that decides whether to watch standard output finds
        // the file instead of saying `standard output: Bad file descriptor`.
        File::open(&w.name)
            .and_then(stdfd::fd_safer)
            .map(Handle::Opened)
    };
    // `--retry` is what makes an unopenable name worth keeping.
    w.tailable = !(options.retry && opened.is_err());
    let mut handle = match opened {
        Ok(handle) => handle,
        Err(e) => {
            if options.forever {
                w.trouble = Trouble::Errno(errnum(&e));
                w.ignore = !options.retry;
            }
            complain!(
                out,
                "tail: cannot open {} for reading: {}",
                quoteaf(&w.label),
                strerror(&e)
            );
            return false;
        }
    };

    if print_headers {
        write_header(out, &w.label, first_header);
    }
    let mut ok = match emit(&mut handle, out, options) {
        Ok(()) => true,
        Err(failure) => {
            report(out, &w.label, failure);
            false
        }
    };

    if options.forever {
        // `f->errnum = ok - 1`: a read that failed is a trouble, though not
        // an `errno` one. Every way out of this block but the one that keeps
        // the file is a failure, so `ok` is only read, never reset, below.
        w.trouble = if ok {
            Trouble::None
        } else {
            Trouble::NotAnErrno
        };
        match handle.metadata() {
            Err(e) => {
                w.trouble = Trouble::Errno(errnum(&e));
                complain!(
                    out,
                    "tail: error reading {}: {}",
                    quoteaf(&w.label),
                    strerror(&e)
                );
            }
            Ok(m) if !tailable(&m) => {
                w.trouble = Trouble::NotAnErrno;
                w.tailable = false;
                w.ignore = !options.retry;
                complain!(
                    out,
                    "tail: {}: cannot follow end of this type of file{}",
                    quotef(&w.label),
                    if w.ignore {
                        "; giving up on this name"
                    } else {
                        ""
                    }
                );
            }
            Ok(m) => {
                if ok {
                    // Where reading actually stopped, which is not the same as
                    // the length: the file may have grown between the last
                    // read and this `fstat`, and those bytes must not be
                    // skipped. A pipe has no position, and its length is never
                    // consulted.
                    let read_pos = handle.stream_position().unwrap_or(m.len());
                    let blocking = if w.is_stdin { None } else { Some(true) };
                    record_open(w, handle, &m, read_pos, blocking);
                    return true;
                }
            }
        }
        // A file whose first read failed is not followed either, unless
        // `--retry` says to keep looking at the name.
        w.ignore = !options.retry;
        w.file = None;
        return false;
    }

    // "if (!is_stdin && close (fd))": standard input is closed once, at the end.
    if let Handle::Opened(file) = handle
        && let Err(e) = stdfd::close(file)
    {
        complain!(
            out,
            "tail: error reading {}: {}",
            quoteaf(&w.label),
            strerror(&e)
        );
        ok = false;
    }
    ok
}

/// `==> name <==`, with a blank line before every one but the first --
/// upstream's `printf`, whose failure is the stream's (see [`Out::print`]).
///
/// The flag counts banners *printed*, not files seen: a file that fails to open
/// never gets one, so the next file that succeeds is still the first and gets
/// no leading blank line.
fn write_header(out: &mut Out, label: &[u8], first: &mut bool) {
    let sep: &[u8] = if *first { b"" } else { b"\n" };
    out.print(&[sep, b"==> ", label, b" <==\n"]);
    *first = false;
}

/// How reading an operand went wrong.
///
/// Upstream decides what to say, and whether to carry on, by *where* it went
/// wrong, so the paths below report the place and [`report`] says the
/// sentence.
#[derive(Debug)]
enum Failure {
    /// The `fstat` that `tail_lines` and `tail_bytes` begin with: `cannot
    /// fstat %s`, and on to the next operand.
    Fstat(io::Error),
    /// A read while still looking for where to start -- `pipe_lines`,
    /// `pipe_bytes`, `start_lines`, `start_bytes`, and `file_lines`'s own
    /// reads: `error reading %s`, and on to the next operand.
    Read(io::Error),
    /// A read while copying, in `dump_remainder`: `error reading %s`, and the
    /// run ends there.
    Copy(io::Error),
    /// `xlseek`: `%s: cannot seek to ...`, and the run ends there.
    Seek(Whence, io::Error),
}

/// A seek `xlseek` was asked to make, for the sentence that reports its
/// failure. Upstream never asks it for a seek relative to the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whence {
    /// `SEEK_SET`.
    Set(u64),
    /// `SEEK_CUR`.
    Current(i64),
}

impl Whence {
    /// The middle of upstream's sentence, offset and all.
    fn sentence(self) -> String {
        match self {
            Whence::Set(offset) => format!("cannot seek to offset {offset}"),
            Whence::Current(offset) => format!("cannot seek to relative offset {offset}"),
        }
    }
}

/// Say what went wrong reading an operand, as upstream says it where it went
/// wrong: the failures it recovers from are reported and the run goes on to
/// the next operand; a failed copy or seek ends the run.
fn report(out: &mut Out, label: &[u8], failure: Failure) {
    match failure {
        Failure::Fstat(e) => {
            complain!(
                out,
                "tail: cannot fstat {}: {}",
                quoteaf(label),
                strerror(&e)
            );
        }
        Failure::Read(e) => {
            complain!(
                out,
                "tail: error reading {}: {}",
                quoteaf(label),
                strerror(&e)
            );
        }
        Failure::Copy(e) => die!(
            out,
            "tail: error reading {}: {}",
            quoteaf(label),
            strerror(&e)
        ),
        Failure::Seek(to, e) => {
            die!(
                out,
                "tail: {}: {}: {}",
                quotef(label),
                to.sentence(),
                strerror(&e)
            );
        }
    }
}

/// Where the data paths write: upstream's `xwrite_stdout`, which writes all of
/// what it is given or ends the run. [`Out`] is the real one; the tests
/// collect into a `Vec<u8>`.
trait Sink {
    fn xwrite(&mut self, data: &[u8]);
}

impl Sink for Vec<u8> {
    fn xwrite(&mut self, data: &[u8]) {
        self.extend_from_slice(data);
    }
}

/// What the `fstat` at the start of `tail_lines` and `tail_bytes` said, as far
/// as the paths below need it.
#[derive(Debug, Clone, Copy)]
struct Facts {
    /// `S_ISREG`, and so also `usable_st_size`.
    regular: bool,
    /// `st_size`.
    size: u64,
    /// gnulib's `ST_BLKSIZE`.
    blksize: u64,
}

impl Facts {
    fn of(file: &File, stats: &Metadata) -> Self {
        Facts {
            regular: is_regular_file(file, stats),
            size: stats.len(),
            blksize: st_blksize(stats),
        }
    }
}

/// `S_ISREG`, of an open file. On Unix its metadata answers that; on the host
/// a pipe claims to be a regular file there, so the handle is asked instead
/// (see `filekind`).
fn is_regular_file(file: &File, stats: &Metadata) -> bool {
    if cfg!(unix) {
        stats.is_file()
    } else {
        filekind::is_regular(file)
    }
}

/// `S_ISFIFO`, of an open file: a pipe, or a FIFO by name.
fn is_fifo(file: &File, stats: &Metadata) -> bool {
    if cfg!(unix) {
        is_fifo_metadata(stats)
    } else {
        filekind::is_pipe(file)
    }
}

/// `S_ISFIFO`, of what `fstat` said.
#[cfg(unix)]
fn is_fifo_metadata(stats: &Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    stats.file_type().is_fifo()
}

#[cfg(not(unix))]
fn is_fifo_metadata(_stats: &Metadata) -> bool {
    // The host's metadata has no such type; [`is_fifo`] asks the handle.
    false
}

/// gnulib's `ST_BLKSIZE`: the file's block size when it is a sane one --
/// positive, and no more than `SIZE_MAX / 8 + 1` -- and `DEV_BSIZE` otherwise.
#[cfg(unix)]
fn st_blksize(stats: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let size = stats.blksize();
    match size.checked_sub(1) {
        Some(below) if below <= (usize::MAX / 8) as u64 => size,
        _ => DEV_BSIZE,
    }
}

#[cfg(not(unix))]
fn st_blksize(_stats: &Metadata) -> u64 {
    // The host reports no block size, which is gnulib's own fallback case.
    DEV_BSIZE
}

/// A count that has been checked against [`OFF_T_MAX`], as an offset.
fn signed(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Upstream's `tail`: `tail_lines` or `tail_bytes`, both of which begin with
/// the `fstat` whose failure is `cannot fstat`.
fn emit(file: &mut File, out: &mut impl Sink, options: &Options) -> Result<(), Failure> {
    let stats = file.metadata().map_err(Failure::Fstat)?;
    let facts = Facts::of(file, &stats);
    let n = options.n_units;
    match options.unit {
        Unit::Lines => tail_lines(file, out, n, options, facts),
        Unit::Bytes => tail_bytes(file, out, n, options, facts),
    }
}

/// Upstream's `tail_bytes`, after its `fstat`: the last `n` bytes, or with `+`
/// everything after the first `n`.
///
/// Which of upstream's routes is taken is decided as upstream decides it,
/// because the routes fail differently and read differently: a regular file
/// is seeked, and a seek that fails ends the run; anything else is asked to
/// seek and read instead when it will not; and an input no larger than one
/// block is read from where it stands rather than seeked in at all -- which is
/// what makes `tail -c5 /proc/self/status` work, a file that says it is empty
/// and cannot seek to its end.
fn tail_bytes(
    file: &mut (impl Read + Seek),
    out: &mut impl Sink,
    n: u64,
    options: &Options,
    facts: Facts,
) -> Result<(), Failure> {
    if options.from_start {
        let skipped = if options.presume_input_pipe || n > OFF_T_MAX {
            false
        } else if facts.regular {
            xlseek(file, Whence::Current(signed(n)))?;
            true
        } else {
            seek_unregular(file, SeekFrom::Current(signed(n))).is_some()
        };
        if !skipped && start_bytes(file, out, n)? == Start::AtEof {
            return Ok(());
        }
        return dump(file, out, Amount::ToEof).map(|_| ());
    }

    let mut end_pos: Option<u64> = None;
    let mut current_pos: Option<u64> = None;
    if !options.presume_input_pipe && n <= OFF_T_MAX {
        if facts.regular {
            end_pos = Some(facts.size);
        } else if let Some(at) = seek_unregular(file, SeekFrom::End(signed(n).saturating_neg())) {
            current_pos = Some(at);
            end_pos = Some(at.saturating_add(n));
        }
    }
    // `end_pos <= ST_BLKSIZE (stats)`, where an end that could not be found
    // is `-1`, below every size.
    let end = match end_pos {
        Some(end) if end > facts.blksize => end,
        _ => return pipe_bytes(file, out, n),
    };
    let mut current = match current_pos {
        Some(at) => at,
        None => xlseek(file, Whence::Current(0))?,
    };
    if current < end && n < end.saturating_sub(current) {
        current = end.saturating_sub(n);
        xlseek(file, Whence::Set(current))?;
    }
    dump(file, out, Amount::AtMost(n)).map(|_| ())
}

/// Upstream's `tail_lines`, after its `fstat`: the last `n` lines, or with `+`
/// everything after the first `n`.
///
/// "Use `file_lines` only if FD refers to a regular file for which `lseek (...
/// SEEK_END)` works" -- anything else, `/proc/cpuinfo` included, is read
/// forwards and the last lines kept.
fn tail_lines(
    file: &mut (impl Read + Seek),
    out: &mut impl Sink,
    n: u64,
    options: &Options,
    facts: Facts,
) -> Result<(), Failure> {
    let line_end = options.line_end;
    if options.from_start {
        if start_lines(file, out, n, line_end)? == Start::AtEof {
            return Ok(());
        }
        return dump(file, out, Amount::ToEof).map(|_| ());
    }

    let mut start_pos = None;
    if !options.presume_input_pipe && facts.regular {
        // Two bare `lseek`s, whose failure is an answer -- read forwards
        // instead -- rather than an error.
        start_pos = file.stream_position().ok();
        if let Some(start) = start_pos
            && let Some(end) = file.seek(SeekFrom::End(0)).ok().filter(|&end| start < end)
        {
            return file_lines(file, out, n, start, end, line_end);
        }
    }
    // "Under very unlikely circumstances, it is possible to reach this point
    // after positioning the file pointer to end of file via the 'lseek
    // (...SEEK_END)' above. In that case, reposition the file pointer back to
    // start_pos before calling pipe_lines."
    if let Some(start) = start_pos {
        xlseek(file, Whence::Set(start))?;
    }
    pipe_lines(file, out, n, line_end)
}

/// Upstream's `xlseek`: the seek, or a [`Failure::Seek`], which ends the run.
fn xlseek(file: &mut impl Seek, to: Whence) -> Result<u64, Failure> {
    let target = match to {
        Whence::Set(offset) => SeekFrom::Start(offset),
        Whence::Current(offset) => SeekFrom::Current(offset),
    };
    file.seek(target).map_err(|e| Failure::Seek(to, e))
}

/// A bare `lseek` on a file that is not regular, whose failure upstream takes
/// as "this cannot seek" and reads instead: the new position, or `None`.
///
/// On the host the answer is always `None`. An MSYS pipe there accepts a seek
/// and moves nothing (see `filekind`), and a pipe that cannot seek is what
/// every POSIX system reports.
fn seek_unregular(file: &mut impl Seek, to: SeekFrom) -> Option<u64> {
    if !cfg!(unix) {
        return None;
    }
    // The failure *is* the answer: upstream tests `lseek (...) != -1` and
    // takes the reading path otherwise, saying nothing.
    file.seek(to).ok()
}

/// How much `dump_remainder` copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Amount {
    /// `COPY_TO_EOF`.
    ToEof,
    /// `COPY_A_BUFFER`: one read's worth, for the follow loop's blocking mode.
    ABuffer,
    /// At most this many bytes.
    AtMost(u64),
}

/// gnulib's `safe_read`: one `read`, repeated if a signal interrupted it.
fn safe_read(source: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match source.read(buf) {
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            done => return done,
        }
    }
}

/// Upstream's `dump_remainder`: copy from the current position, a buffer at a
/// time, to end of file or until `amount` is reached, and return how much was
/// copied.
///
/// A failed read is [`Failure::Copy`], which ends the run -- except `EAGAIN`,
/// a non-blocking descriptor with nothing more to give yet, which ends the
/// copy quietly. That is how the follow loop's reads of a pipe stop.
fn dump(source: &mut impl Read, out: &mut impl Sink, amount: Amount) -> Result<u64, Failure> {
    let mut buf = vec![0u8; BUFSIZ];
    let mut copied: u64 = 0;
    let mut remaining = match amount {
        Amount::AtMost(n) => n,
        Amount::ToEof | Amount::ABuffer => u64::MAX,
    };
    loop {
        let want = usize::try_from(remaining).map_or(BUFSIZ, |left| left.min(BUFSIZ));
        let got = match safe_read(source, buf.get_mut(..want).unwrap_or_default()) {
            Ok(got) => got,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => return Err(Failure::Copy(e)),
        };
        if got == 0 {
            break;
        }
        out.xwrite(buf.get(..got).unwrap_or_default());
        copied = copied.saturating_add(got as u64);
        if amount != Amount::ToEof {
            remaining = remaining.saturating_sub(got as u64);
            if remaining == 0 || amount == Amount::ABuffer {
                break;
            }
        }
    }
    Ok(copied)
}

/// What skipping the start of an input found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    /// The skip ended inside the input. What followed it in the same read has
    /// been printed, and the rest is for `dump_remainder`.
    Ready,
    /// The input ended first, so there is nothing to print.
    AtEof,
}

/// Upstream's `start_bytes`: read past the first `n` bytes of an input that
/// cannot be seeked, printing whatever the last read brought beyond them.
fn start_bytes(source: &mut impl Read, out: &mut impl Sink, n: u64) -> Result<Start, Failure> {
    let mut left = n;
    let mut buf = vec![0u8; BUFSIZ];
    while left > 0 {
        let got = safe_read(source, &mut buf).map_err(Failure::Read)?;
        if got == 0 {
            return Ok(Start::AtEof);
        }
        match usize::try_from(left) {
            // "Print extra characters if there are any."
            Ok(skip) if skip < got => {
                out.xwrite(buf.get(skip..got).unwrap_or_default());
                break;
            }
            _ => left = left.saturating_sub(got as u64),
        }
    }
    Ok(Start::Ready)
}

/// Upstream's `start_lines`: read past the first `n` line terminators, printing
/// whatever the read that found the last of them brought after it.
///
/// Works on any input, seekable or not: unlike counting from the end, counting
/// from the start needs no lookahead.
fn start_lines(
    source: &mut impl Read,
    out: &mut impl Sink,
    n: u64,
    line_end: u8,
) -> Result<Start, Failure> {
    if n == 0 {
        return Ok(Start::Ready);
    }
    let mut left = n;
    let mut buf = vec![0u8; BUFSIZ];
    loop {
        let got = safe_read(source, &mut buf).map_err(Failure::Read)?;
        if got == 0 {
            return Ok(Start::AtEof);
        }
        let chunk = buf.get(..got).unwrap_or_default();
        let mut at = 0usize;
        while let Some(rel) = chunk
            .get(at..)
            .and_then(|rest| rest.iter().position(|&b| b == line_end))
        {
            at = at.saturating_add(rel).saturating_add(1);
            left = left.saturating_sub(1);
            if left == 0 {
                out.xwrite(chunk.get(at..).unwrap_or_default());
                return Ok(Start::Ready);
            }
        }
    }
}

/// Upstream's `pipe_bytes`: the last `n` bytes of an input that is read
/// forwards to its end.
///
/// Held bytes never exceed `n`: a byte is dropped as soon as `n` later ones
/// exist to displace it, so this costs the size of the answer and not the size
/// of the input. A failed read prints nothing at all, as upstream's does.
fn pipe_bytes(source: &mut impl Read, out: &mut impl Sink, n: u64) -> Result<(), Failure> {
    let keep = usize::try_from(n).unwrap_or(usize::MAX);
    let mut held: VecDeque<u8> = VecDeque::new();
    let mut buf = vec![0u8; BUFSIZ];
    loop {
        let got = safe_read(source, &mut buf).map_err(Failure::Read)?;
        if got == 0 {
            break;
        }
        held.extend(buf.get(..got).unwrap_or_default());
        if let Some(excess) = held.len().checked_sub(keep) {
            held.drain(..excess);
        }
    }
    let (front, back) = held.as_slices();
    out.xwrite(front);
    out.xwrite(back);
    Ok(())
}

/// Upstream's `pipe_lines`: the last `n` lines of an input that is read
/// forwards to its end.
///
/// Holds at most `n` complete lines plus the one being read, which is the least
/// that can answer the question -- the last line cannot be known until the
/// input ends. A final line with no terminator is a line, and a failed read
/// prints nothing at all, as upstream's does.
fn pipe_lines(
    source: &mut impl Read,
    out: &mut impl Sink,
    n: u64,
    line_end: u8,
) -> Result<(), Failure> {
    let keep = usize::try_from(n).unwrap_or(usize::MAX);
    let mut lines: VecDeque<Vec<u8>> = VecDeque::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; BUFSIZ];
    loop {
        let got = safe_read(source, &mut buf).map_err(Failure::Read)?;
        if got == 0 {
            break;
        }
        if keep == 0 {
            // "This prevents a core dump when the pipe contains no newlines":
            // nothing is kept, but the input is still read to its end.
            continue;
        }
        let mut chunk = buf.get(..got).unwrap_or_default();
        while let Some(at) = chunk.iter().position(|&b| b == line_end) {
            partial.extend_from_slice(chunk.get(..=at).unwrap_or_default());
            lines.push_back(std::mem::take(&mut partial));
            if lines.len() > keep {
                lines.pop_front();
            }
            chunk = chunk.get(at.saturating_add(1)..).unwrap_or_default();
        }
        partial.extend_from_slice(chunk);
    }
    // "Count the incomplete line on files that don't end with a newline."
    if !partial.is_empty() {
        lines.push_back(partial);
        if lines.len() > keep {
            lines.pop_front();
        }
    }
    for line in &lines {
        out.xwrite(line);
    }
    Ok(())
}

/// Upstream's `file_lines`: the last `n` lines of a regular file, found by
/// reading it backwards a block at a time from `end_pos` towards `start_pos`,
/// then copied forwards.
///
/// The blocks are aligned to `BUFSIZ` from `start_pos`, so the first one read
/// -- the last in the file -- is the ragged one. A final line with no
/// terminator counts as one of the `n`, which is why that block is looked at
/// before the scan begins: `printf 'a\nb' | tail -n1` is `b`, not `a\nb`. The
/// copy is bounded by `end_pos`, so a file that grows meanwhile does not
/// lengthen the answer, and `n` of zero is nothing at all -- not the
/// unterminated last line, which is what `tail -n0 -f` would otherwise begin
/// by printing.
fn file_lines(
    file: &mut (impl Read + Seek),
    out: &mut impl Sink,
    n: u64,
    start_pos: u64,
    end_pos: u64,
    line_end: u8,
) -> Result<(), Failure> {
    if n == 0 {
        return Ok(());
    }
    let mut want = n;
    let mut buf = vec![0u8; BUFSIZ];
    // "Set 'bytes_read' to the size of the last, probably partial, buffer;
    // 0 < 'bytes_read' <= 'BUFSIZ'."
    let ragged = end_pos
        .saturating_sub(start_pos)
        .checked_rem(BUFSIZ as u64)
        .and_then(|r| usize::try_from(r).ok())
        .unwrap_or(0);
    let mut bytes_read = if ragged == 0 { BUFSIZ } else { ragged };
    let mut pos = end_pos.saturating_sub(bytes_read as u64);
    xlseek(file, Whence::Set(pos))?;
    bytes_read =
        safe_read(file, buf.get_mut(..bytes_read).unwrap_or_default()).map_err(Failure::Read)?;
    if bytes_read
        .checked_sub(1)
        .and_then(|last| buf.get(last))
        .is_some_and(|&b| b != line_end)
    {
        want = want.saturating_sub(1);
    }
    loop {
        // "Scan backward, counting the newlines in this bufferfull."
        let mut scan = bytes_read;
        while scan > 0 {
            let Some(nl) = buf
                .get(..scan)
                .and_then(|block| block.iter().rposition(|&b| b == line_end))
            else {
                break;
            };
            scan = nl;
            if want == 0 {
                // "If this newline isn't the last character in the buffer,
                // output the part that is after it."
                out.xwrite(
                    buf.get(nl.saturating_add(1)..bytes_read)
                        .unwrap_or_default(),
                );
                let rest = end_pos.saturating_sub(pos.saturating_add(bytes_read as u64));
                dump(file, out, Amount::AtMost(rest))?;
                return Ok(());
            }
            want = want.saturating_sub(1);
        }
        // "Not enough newlines in that bufferfull."
        if pos == start_pos {
            // "Not enough lines in the file; print everything from start_pos
            // to the end." Bounded by `end_pos` itself, as upstream bounds it.
            xlseek(file, Whence::Set(start_pos))?;
            dump(file, out, Amount::AtMost(end_pos))?;
            return Ok(());
        }
        pos = pos.saturating_sub(BUFSIZ as u64);
        xlseek(file, Whence::Set(pos))?;
        bytes_read = safe_read(file, &mut buf).map_err(Failure::Read)?;
        if bytes_read == 0 {
            return Ok(());
        }
    }
}

// --------------------------------------------------------------- following ---

/// Upstream's `ignore_fifo_and_pipe`: under `-f`, a `-` that is a pipe or a
/// FIFO is not followed. POSIX requires it when there is no operand, and
/// upstream extends it to every `-`. Returns how many operands are left to
/// follow; with none, `-f` ends the run as soon as the files are printed.
fn ignore_fifo_and_pipe(watched: &mut [Watched]) -> usize {
    let mut viable = 0usize;
    for w in watched {
        if w.is_stdin && !w.ignore && w.file.is_some() && w.fifo {
            w.file = None;
            w.ignore = true;
        } else {
            viable = viable.saturating_add(1);
        }
    }
    viable
}

/// What [`follow`] carries from one file and one iteration to the next.
struct Following<'a> {
    options: &'a Options,
    print_headers: bool,
    /// Whether no banner has been printed yet: upstream's `first_file`.
    first_header: bool,
    /// The file the last banner named: upstream's `last`.
    last: usize,
    /// "Use blocking I/O as an optimization, when it's easy": one file,
    /// followed by descriptor, no `--pid`, and not a regular file.
    blocking: bool,
    /// `1 < n_files`.
    several: bool,
}

/// Poll the watched files until there is nothing left to watch -- upstream's
/// `tail_forever`.
///
/// This is the polling loop and only the polling loop. Upstream has a second,
/// `inotify`-driven implementation that it prefers where the kernel offers it;
/// we have no such interface, so the `---disable-inotify` switch is already the
/// permanent state of affairs. That is not merely a missing optimisation -- it
/// changes what the help text can honestly say, which is why `--help` here is
/// two clauses shorter than GNU's.
fn follow(
    watched: &mut [Watched],
    out: &mut Out,
    options: &Options,
    print_headers: bool,
    first_header: bool,
    monitor_output: bool,
) {
    let blocking = options.pid.is_none()
        && options.follow == Follow::Descriptor
        && watched.len() == 1
        && watched
            .first()
            .is_some_and(|w| w.file.is_some() && !w.regular);
    let mut st = Following {
        options,
        print_headers,
        first_header,
        last: watched.len().saturating_sub(1),
        blocking,
        several: watched.len() > 1,
    };
    let mut writer_is_dead = false;

    loop {
        let mut any_input = false;
        for (index, w) in watched.iter_mut().enumerate() {
            if w.ignore {
                continue;
            }
            if w.file.is_none() {
                recheck(w, out, options, blocking);
                continue;
            }
            any_input |= poll_one(w, index, out, &mut st);
        }

        if !any_live(watched, options) {
            complain!(out, "tail: no files remaining");
            return;
        }
        if !any_input || blocking {
            out.flush_or_die();
        }
        if monitor_output {
            check_output_alive(out);
        }
        // If nothing was read, sleep and/or check for dead writers.
        if !any_input {
            if writer_is_dead {
                return;
            }
            // "Once the writer is dead, read the files once more to avoid a
            // race condition."
            writer_is_dead = options.pid.is_some_and(|p| !process_alive(p));
            if !writer_is_dead {
                sleep(options.sleep_interval);
            }
        }
    }
}

/// One file, one iteration of [`follow`] -- the body of upstream's loop.
/// Whether anything was read.
fn poll_one(w: &mut Watched, index: usize, out: &mut Out, st: &mut Following<'_>) -> bool {
    let options = st.options;
    let Some(handle) = w.file.as_ref() else {
        return false;
    };

    if w.blocking != Some(st.blocking) {
        match add_nonblocking(handle, !st.blocking) {
            Ok(()) => w.blocking = Some(st.blocking),
            // "This happens when using tail -f on a file with the
            // append-only attribute." The mode stays as it was, and is asked
            // for again next time.
            Err(e) if w.regular && e.raw_os_error() == Some(EPERM) => {}
            Err(e) => die!(
                out,
                "tail: {}: cannot change nonblocking mode: {}",
                quotef(&w.label),
                strerror(&e)
            ),
        }
    }

    let mut read_unchanged = false;
    if !w.reads_block() {
        let stats = match w.file.as_ref().map(|h| h.metadata()) {
            Some(Ok(stats)) => stats,
            Some(Err(e)) => {
                w.trouble = Trouble::Errno(errnum(&e));
                complain!(out, "tail: {}: {}", quotef(&w.label), strerror(&e));
                w.file = None;
                return false;
            }
            None => return false,
        };
        let regular = w.file.as_ref().is_some_and(|h| is_regular_file(h, &stats));
        // `mode`, as it was before this look.
        let was_regular = w.regular;
        let modified = modification_time(&stats);

        if w.regular == regular && (!regular || w.size == stats.len()) && w.modified == modified {
            let seen = w.unchanged;
            w.unchanged = w.unchanged.saturating_add(1);
            let mut moved = false;
            if options.max_unchanged <= seen && options.follow == Follow::Name {
                // The file has not moved for a while; the *name* may have.
                // This is the log-rotation case, and the only thing
                // `--max-unchanged-stats` controls.
                moved = recheck(w, out, options, false);
                w.unchanged = 0;
            }
            if moved || regular || st.several {
                return false;
            }
            // A pipe or terminal whose `mtime` never moves: reading is the
            // only way to find out whether anything arrived.
            read_unchanged = true;
        }

        // "This file has changed. Print out what we can, and then keep
        // looping."
        w.modified = modified;
        w.regular = regular;
        if !read_unchanged {
            w.unchanged = 0;
        }

        // "XXX: This is only a heuristic, as the file may have also been
        // truncated and written to if st_size >= size (in which case we ignore
        // new data <= size)."
        if was_regular && stats.len() < w.size {
            complain!(out, "tail: {}: file truncated", quotef(&w.label));
            // "Assume the file was truncated to 0, and therefore output all
            // "new" data."
            if let Some(handle) = w.file.as_mut()
                && let Err(failure) = xlseek(&mut **handle, Whence::Set(0))
            {
                report(out, &w.label, failure);
            }
            w.size = 0;
        }

        if index != st.last {
            if st.print_headers {
                write_header(out, &w.label, &mut st.first_header);
            }
            st.last = index;
        }
    }

    // There are no remote file systems here to bound a read by `st_size`, so
    // a non-blocking read goes to the end and a blocking one takes a buffer.
    let amount = if w.reads_block() {
        Amount::ABuffer
    } else {
        Amount::ToEof
    };
    let Some(handle) = w.file.as_mut() else {
        return false;
    };
    let read = match dump(&mut **handle, out, amount) {
        Ok(read) => read,
        Err(failure) => {
            report(out, &w.label, failure);
            0
        }
    };
    if read_unchanged && read != 0 {
        w.unchanged = 0;
    }
    w.size = w.size.saturating_add(read);
    read != 0
}

/// Look at the *name* again and report what became of it -- upstream's
/// `recheck`. Returns whether the file being read changed: closed, or replaced
/// by a new one.
///
/// Two of upstream's diagnostics are unreachable here and are absent rather
/// than dead: `has been replaced with an untailable symbolic link` and `has
/// been replaced with an untailable remote file` are both guarded by
/// `! disable_inotify`, and inotify is permanently disabled in this
/// implementation.
fn recheck(w: &mut Watched, out: &mut Out, options: &Options, blocking: bool) -> bool {
    let was_tailable = w.tailable;
    let previous = w.trouble;

    let opened = if w.is_stdin {
        stdin_handle()
    } else {
        open_to_follow(&w.name, blocking)
    };
    // "If the open fails because the file doesn't exist, then mark the file
    // as not tailable."
    w.tailable = !(options.retry && opened.is_err());

    let looked = opened.and_then(|handle| {
        let stats = handle.metadata()?;
        Ok((handle, stats))
    });
    let (mut handle, stats) = match looked {
        Ok(pair) => pair,
        Err(e) => {
            let errno = errnum(&e);
            w.trouble = Trouble::Errno(errno);
            if !w.tailable {
                if was_tailable {
                    complain!(
                        out,
                        "tail: {} has become inaccessible: {}",
                        quoteaf(&w.label),
                        strerror(&e)
                    );
                }
                // "say nothing... it's still not tailable"
            } else if previous != Trouble::Errno(errno) {
                // A different failure from last time is news; the same one
                // again is not.
                complain!(out, "tail: {}: {}", quotef(&w.label), strerror(&e));
            }
            return w.file.take().is_some();
        }
    };

    if !tailable(&stats) {
        w.trouble = Trouble::NotAnErrno;
        w.tailable = false;
        w.ignore = !(options.retry && options.follow == Follow::Name);
        if was_tailable || previous != Trouble::NotAnErrno {
            complain!(
                out,
                "tail: {} has been replaced with an untailable file{}",
                quoteaf(&w.label),
                if w.ignore {
                    "; giving up on this name"
                } else {
                    ""
                }
            );
        }
        return w.file.take().is_some();
    }

    w.trouble = Trouble::None;
    let id = file_id(&stats);
    let new_file = if previous != Trouble::None && previous != Trouble::Errno(ENOENT) {
        complain!(out, "tail: {} has become accessible", quoteaf(&w.label));
        true
    } else if w.file.is_none() {
        // "A new file even when inodes haven't changed as <dev,inode> pairs
        // can be reused, and we know the file was missing on the previous
        // iteration."
        complain!(
            out,
            "tail: {} has appeared;  following new file",
            quoteaf(&w.label)
        );
        true
    } else if w.id != Some(id) {
        // "File has been replaced (e.g., via log rotation) -- tail the new
        // one."
        complain!(
            out,
            "tail: {} has been replaced;  following new file",
            quoteaf(&w.label)
        );
        true
    } else {
        false
    };
    if !new_file {
        // "No changes detected, so close new fd."
        return false;
    }

    // "Start at the beginning of the file."
    if is_regular_file(&handle, &stats)
        && let Err(failure) = xlseek(&mut *handle, Whence::Set(0))
    {
        report(out, &w.label, failure);
    }
    let blocking = if w.is_stdin { None } else { Some(blocking) };
    record_open(w, handle, &stats, 0, blocking);
    true
}

/// `open (name, O_RDONLY | (blocking ? 0 : O_NONBLOCK))`: the follow loop's
/// reopen, which must not wait for a FIFO's writer when its reads may not.
fn open_to_follow(name: &OsString, blocking: bool) -> io::Result<Handle> {
    let mut how = std::fs::OpenOptions::new();
    how.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if !blocking {
            how.custom_flags(O_NONBLOCK);
        }
    }
    // The host has no non-blocking open; its reads of a pipe wait.
    #[cfg(not(unix))]
    let _: bool = blocking;
    // `open_safer`: see `start_file`.
    how.open(name).and_then(stdfd::fd_safer).map(Handle::Opened)
}

/// Upstream's switch into the follow loop's reading mode: `O_NONBLOCK` added
/// when `nonblocking`, and otherwise the flags left as they are -- upstream
/// never takes the flag away again.
#[cfg(unix)]
fn add_nonblocking(file: &File, nonblocking: bool) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    let fd = file.as_raw_fd();
    // SAFETY: `fd` is open for as long as `file` is borrowed, and `F_GETFL`
    // only reads its flags. The variable argument is an `i64`, so that a
    // callee reading a word reads a defined one.
    let old = unsafe { fcntl(fd, F_GETFL, 0i64) };
    if old < 0 {
        return Err(io::Error::last_os_error());
    }
    let new = if nonblocking { old | O_NONBLOCK } else { old };
    // SAFETY: as above; `F_SETFL` changes only the descriptor's status flags.
    if new != old && unsafe { fcntl(fd, F_SETFL, i64::from(new)) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
// The signature is the Unix version's, whose `fcntl` can fail; this one
// cannot, and says so by always succeeding.
#[allow(clippy::unnecessary_wraps)]
fn add_nonblocking(_file: &File, _nonblocking: bool) -> io::Result<()> {
    // The host has no `O_NONBLOCK`. Taking the switch as made keeps the loop
    // from asking again; its reads of a pipe wait, which only the host sees.
    Ok(())
}

/// Upstream's `check_output_alive`: when standard output is a pipe whose
/// reader has gone, end the run as a write to it would have -- by `SIGPIPE`,
/// or where that is ignored, status 1 and no message.
fn check_output_alive(out: &mut Out) {
    if output_broken() {
        out.die_pipe();
    }
}

/// gnulib's `iopoll (-1, STDOUT_FILENO, false) == IOPOLL_BROKEN_OUTPUT`: a
/// `poll` of descriptor 1 that does not wait, broken when it reports
/// `POLLERR`, `POLLHUP` or `POLLNVAL`.
///
/// Upstream polls again when the answer has none of those, which on Linux
/// cannot happen for a pipe -- `POLLRDBAND`, the one event asked for, is never
/// raised on the writing end. Here such an answer is taken as "alive", the
/// conclusion upstream's next poll would reach, rather than looped on.
#[cfg(unix)]
fn output_broken() -> bool {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    const POLLERR: i16 = 0x008;
    const POLLHUP: i16 = 0x010;
    const POLLNVAL: i16 = 0x020;
    const POLLRDBAND: i16 = 0x080;
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
    }

    let mut pfd = PollFd {
        fd: 1,
        events: POLLRDBAND,
        revents: 0,
    };
    loop {
        // SAFETY: `pfd` is one valid, writable `struct pollfd` for the whole
        // call, and `nfds` says so; a zero timeout returns at once.
        let ready = unsafe { poll(&raw mut pfd, 1, 0) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            // `IOPOLL_ERROR`, which is not `IOPOLL_BROKEN_OUTPUT`.
            return false;
        }
        return ready > 0 && pfd.revents & (POLLERR | POLLHUP | POLLNVAL) != 0;
    }
}

#[cfg(not(unix))]
fn output_broken() -> bool {
    // Never asked: standard output is monitored only where `fstat` said it is
    // a FIFO, which the host cannot say.
    false
}

/// `raise (SIGPIPE)`: at its default disposition the signal ends the process,
/// as the write to a gone reader would have; ignored, it returns.
#[cfg(unix)]
fn raise_sigpipe() {
    const SIGPIPE: i32 = 13;
    unsafe extern "C" {
        fn raise(sig: i32) -> i32;
    }
    // SAFETY: `raise` takes a signal number and touches no memory of ours.
    // Its result is that of sending the signal; upstream ignores it as well,
    // since `exit` follows whatever happened.
    unsafe { raise(SIGPIPE) };
}

#[cfg(not(unix))]
fn raise_sigpipe() {}

/// Whether anything is still worth watching -- upstream's `any_live_files`.
///
/// `--retry --follow=name` is always worth watching: the whole point of the
/// combination is to wait for a file that does not exist yet. Otherwise a file
/// is live while it is open, or while `--retry` keeps a name that has not been
/// given up on.
fn any_live(watched: &[Watched], options: &Options) -> bool {
    if options.retry && options.follow == Follow::Name {
        return true;
    }
    watched
        .iter()
        .any(|w| w.file.is_some() || (!w.ignore && options.retry))
}

/// Whether a file of this type can be followed at all: upstream's
/// `IS_TAILABLE_FILE_TYPE`, which is everything except a directory and a block
/// device.
#[cfg(unix)]
fn tailable(m: &Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    let t = m.file_type();
    m.is_file() || t.is_fifo() || t.is_socket() || t.is_char_device()
}

#[cfg(not(unix))]
fn tailable(m: &Metadata) -> bool {
    // The host build. There is no file-type detail here beyond "directory or
    // not", and a directory cannot be opened in the first place — see the
    // module doc.
    !m.is_dir()
}

/// A file's identity, for telling a rotated log from the same log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileId {
    volume: u64,
    file: u64,
}

#[cfg(unix)]
fn file_id(m: &Metadata) -> FileId {
    use std::os::unix::fs::MetadataExt;
    FileId {
        volume: m.dev(),
        file: m.ino(),
    }
}

#[cfg(not(unix))]
fn file_id(m: &Metadata) -> FileId {
    use std::os::windows::fs::MetadataExt;
    // The real answer is the volume serial number and the file index, which
    // `std` only exposes behind an unstable feature. Creation time is the
    // stand-in: it does not identify a file, but it does change when one name
    // comes to refer to a different file, which is the only question asked of
    // it here. This is the host test build; SlateOS presents as `unix`.
    FileId {
        volume: 0,
        file: m.creation_time(),
    }
}

/// Wait between iterations. A non-finite or absurd interval sleeps for as long
/// as the clock allows rather than failing, which is what `-s inf` asks for.
fn sleep(seconds: f64) {
    let duration = std::time::Duration::try_from_secs_f64(seconds)
        .unwrap_or(std::time::Duration::from_secs(u64::MAX / 2));
    std::thread::sleep(duration);
}

/// Whether the process `--pid` names is still running.
///
/// "Running" includes "running but not ours to signal": a process owned by
/// somebody else answers `EPERM`, which proves it exists.
#[cfg(unix)]
fn process_alive(pid: u64) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 performs the existence and permission checks and
    // delivers nothing. The call takes two integers and touches no memory of
    // ours.
    if unsafe { kill(pid, 0) } == 0 {
        return true;
    }
    io::Error::last_os_error().kind() == ErrorKind::PermissionDenied
}

#[cfg(windows)]
fn process_alive(pid: u64) -> bool {
    use core::ffi::c_void;
    /// `PROCESS_QUERY_LIMITED_INFORMATION` — the least that can answer this,
    /// and the most that is granted for a process of another user.
    const QUERY_LIMITED: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    let Ok(pid) = u32::try_from(pid) else {
        return false;
    };
    // SAFETY: `OpenProcess` returns null on failure and is checked for it. The
    // handle is closed on every path out.
    unsafe {
        let handle = OpenProcess(QUERY_LIMITED, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let got = GetExitCodeProcess(handle, &raw mut code);
        CloseHandle(handle);
        got != 0 && code == STILL_ACTIVE
    }
}

#[cfg(not(any(unix, windows)))]
fn process_alive(_pid: u64) -> bool {
    // No way to ask, so never stop early on this account.
    true
}

// -------------------------------------------------------------- byte paths ---

fn arg_bytes(a: &OsString) -> Vec<u8> {
    os_bytes(a.as_os_str()).into_owned()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::arithmetic_side_effects)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
// `sleep_interval` is parsed from decimal text that binary floating point
// holds exactly (0, 0.5, 1, 2, 10, 12, 16), so comparing it exactly is what
// the tests mean, not a rounding hazard.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// `parse_args` with `POSIXLY_CORRECT` pinned off, so that a test putting an
    /// option after an operand does not depend on the environment `cargo test`
    /// inherited. The tests of the variable itself call `super::parse_args`.
    fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
        super::parse_args(args, false, posixver::DEFAULT)
    }

    /// Measured against GNU 9.4 on 2026-09-25, row by row.
    #[test]
    fn the_obsolete_forms_follow_posix2_version() {
        let argv = |words: &[&str]| -> Vec<OsString> { words.iter().map(OsString::from).collect() };
        let run = |words: &[&str], version: i32| super::parse_args(&argv(words), false, version);
        // `+2` is a count from the start in every edition but 2001's...
        for version in [199_209, posixver::DEFAULT] {
            let Ok(Request::Run(o, files)) = run(&["+2", "f"], version) else {
                panic!("`+2 f` under {version}");
            };
            assert!(o.from_start);
            assert_eq!(files, argv(&["f"]));
        }
        // ...where it is a file name.
        let Ok(Request::Run(o, files)) = run(&["+2", "f"], 200_112) else {
            panic!("`+2 f` under 200112");
        };
        assert!(!o.from_start);
        assert_eq!(files, argv(&["+2", "f"]));
        // A bare `-c` is an option wanting its argument since 2001...
        assert!(run(&["-c"], posixver::DEFAULT).is_err());
        // ...and before it, ten bytes from the end of standard input.
        let Ok(Request::Run(o, files)) = run(&["-c"], 199_209) else {
            panic!("`-c` under 199209");
        };
        assert!(files.is_empty());
        assert!(!o.from_start);
    }

    /// Measured against GNU on 2026-09-25: `POSIXLY_CORRECT=1 tail f -n1` takes
    /// `-n1` for a second file, where without the variable it is an option.
    #[test]
    fn posixly_correct_makes_an_option_after_an_operand_an_operand() {
        let argv: Vec<OsString> = ["f", "-n1"].iter().map(OsString::from).collect();
        let Ok(Request::Run(_, files)) = super::parse_args(&argv, true, posixver::DEFAULT) else {
            panic!("expected a run");
        };
        assert_eq!(files, argv);
        let Ok(Request::Run(_, files)) = super::parse_args(&argv, false, posixver::DEFAULT) else {
            panic!("expected a run");
        };
        assert_eq!(files, argv[..1]);
    }

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

    /// The request's options as `run` hands them on: a `+N` already turned
    /// into the `N - 1` items to skip.
    fn as_run_sees(items: &[&str]) -> Options {
        let mut options = parse(items);
        if options.from_start {
            options.n_units = options.n_units.saturating_sub(1);
        }
        options
    }

    /// `tail_lines` or `tail_bytes`, as [`emit`] chooses between them.
    fn routes(
        source: &mut (impl Read + Seek),
        out: &mut Vec<u8>,
        options: &Options,
        facts: Facts,
    ) -> Result<(), Failure> {
        let n = options.n_units;
        match options.unit {
            Unit::Lines => tail_lines(source, out, n, options, facts),
            Unit::Bytes => tail_bytes(source, out, n, options, facts),
        }
    }

    /// Run a request through the routes a pipe takes -- the ones that read
    /// forwards and never seek, which is what `---presume-input-pipe` selects
    /// -- over bytes in memory.
    fn stream(input: &[u8], items: &[&str]) -> Vec<u8> {
        let mut options = as_run_sees(items);
        options.presume_input_pipe = true;
        let pipe = Facts {
            regular: false,
            size: 0,
            blksize: DEV_BSIZE,
        };
        let mut out: Vec<u8> = Vec::new();
        routes(&mut io::Cursor::new(input), &mut out, &options, pipe).unwrap();
        out
    }

    /// The same request against a regular file in memory whose block size is
    /// zero, so that every route that seeks is taken however small the input.
    fn seeking(input: &[u8], items: &[&str]) -> Vec<u8> {
        let options = as_run_sees(items);
        let regular = Facts {
            regular: true,
            size: input.len() as u64,
            blksize: 0,
        };
        let mut out: Vec<u8> = Vec::new();
        routes(&mut io::Cursor::new(input), &mut out, &options, regular).unwrap();
        out
    }

    /// The same request against a real file, through [`emit`]: the `fstat`,
    /// and the routes it chooses, are the real ones.
    fn on_disk(input: &[u8], items: &[&str]) -> Vec<u8> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "slateos-tail-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, input).unwrap();
        let options = as_run_sees(items);
        let mut file = File::open(&path).unwrap();
        let mut out: Vec<u8> = Vec::new();
        let result = emit(&mut file, &mut out, &options);
        drop(file);
        let _ = std::fs::remove_file(&path);
        result.unwrap();
        out
    }

    /// All three at once: they are required to be indistinguishable.
    fn both(input: &[u8], items: &[&str]) -> Vec<u8> {
        let streamed = stream(input, items);
        for (route, got) in [
            ("seeking", seeking(input, items)),
            ("on-disk", on_disk(input, items)),
        ] {
            assert_eq!(
                String::from_utf8_lossy(&streamed),
                String::from_utf8_lossy(&got),
                "the streaming and {route} routes disagree for {items:?}"
            );
        }
        streamed
    }

    /// An input that serves `data`, then fails every read once `fail_at`
    /// bytes have been read -- with `EAGAIN`'s kind when `would_block`, and an
    /// I/O error otherwise -- and that refuses every seek when `seek_fails`.
    struct Faulty {
        data: Vec<u8>,
        pos: u64,
        fail_at: u64,
        would_block: bool,
        seek_fails: bool,
    }

    impl Faulty {
        fn new(data: &[u8], fail_at: u64) -> Self {
            Faulty {
                data: data.to_vec(),
                pos: 0,
                fail_at,
                would_block: false,
                seek_fails: false,
            }
        }
    }

    impl Read for Faulty {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.fail_at {
                return Err(if self.would_block {
                    io::Error::from(ErrorKind::WouldBlock)
                } else {
                    io::Error::other("injected read failure")
                });
            }
            let start = usize::try_from(self.pos).unwrap().min(self.data.len());
            let room = usize::try_from(self.fail_at - self.pos).unwrap_or(usize::MAX);
            let n = buf.len().min(self.data.len() - start).min(room);
            buf[..n].copy_from_slice(&self.data[start..start + n]);
            self.pos += n as u64;
            Ok(n)
        }
    }

    impl Seek for Faulty {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            if self.seek_fails {
                return Err(io::Error::other("injected seek failure"));
            }
            let len = i64::try_from(self.data.len()).unwrap();
            let at = match to {
                SeekFrom::Start(offset) => i64::try_from(offset).unwrap(),
                SeekFrom::Current(delta) => i64::try_from(self.pos).unwrap() + delta,
                SeekFrom::End(delta) => len + delta,
            };
            self.pos = u64::try_from(at).map_err(|_| io::Error::other("before the start"))?;
            Ok(self.pos)
        }
    }

    // ------------------------------------------------------------ options ---

    #[test]
    fn the_default_is_ten_lines_from_the_end() {
        let o = parse(&[]);
        assert_eq!(o, Options::default());
        assert_eq!(o.unit, Unit::Lines);
        assert_eq!(o.n_units, 10);
        assert!(!o.from_start);
        assert!(!o.forever);
        assert_eq!(o.follow, Follow::Descriptor);
        assert_eq!(o.headers, Headers::Multiple);
        assert_eq!(o.line_end, b'\n');
        assert_eq!(o.sleep_interval, 1.0);
        assert_eq!(o.max_unchanged, 5);
    }

    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        assert_eq!(parse(&["--by", "3"]).unit, Unit::Bytes);
        assert_eq!(parse(&["--li=3"]).n_units, 3);
        assert_eq!(parse(&["--verb"]).headers, Headers::Always);
        assert_eq!(parse(&["--zero"]).line_end, 0);
        assert_eq!(parse(&["--sl=2"]).sleep_interval, 2.0);
        // `--q` and `--s` are ambiguous here in a way they are not in `head`:
        // `--s` prefixes both `--silent` and `--sleep-interval`, which differ.
        assert_eq!(parse(&["--q"]).headers, Headers::Never);
        assert_eq!(
            body(&fail(&["--s"])),
            "option '--s' is ambiguous; possibilities: '--silent' '--sleep-interval'"
        );
        // `--r` is likewise `--retry` alone, since nothing else starts with r.
        assert!(parse(&["--r", "-f"]).retry);
    }

    /// The two hidden options really do take three dashes.
    #[test]
    fn the_hidden_options_are_spelled_with_three_dashes() {
        assert!(parse(&["---presume-input-pipe"]).presume_input_pipe);
        assert!(parse(&["---p"]).presume_input_pipe);
        assert_eq!(parse(&["---d"]), Options::default());
        // Two dashes is a different name, and matches nothing.
        assert_eq!(
            body(&fail(&["--presume-input-pipe"])),
            "unrecognized option '--presume-input-pipe'"
        );
    }

    /// Measured with `tail --=x`: an empty prefix matches every option, so the
    /// list is the whole table in GNU's declaration order.
    #[test]
    fn the_ambiguity_list_is_in_gnus_declaration_order() {
        assert_eq!(
            body(&fail(&["--=x"])),
            "option '--=x' is ambiguous; possibilities: '--bytes' '--follow' \
             '--lines' '--max-unchanged-stats' '---disable-inotify' '--pid' \
             '---presume-input-pipe' '--quiet' '--retry' '--silent' \
             '--sleep-interval' '--verbose' '--zero-terminated' '--help' \
             '--version'"
        );
    }

    #[test]
    fn every_getopt_sentence_matches_glibc() {
        assert_eq!(body(&fail(&["-x"])), "invalid option -- 'x'");
        assert_eq!(body(&fail(&["-c"])), "option requires an argument -- 'c'");
        // Not `--fol`, which is a unique prefix of `--follow` and resolves.
        assert_eq!(body(&fail(&["--nosuch"])), "unrecognized option '--nosuch'");
        assert_eq!(
            body(&fail(&["--lines"])),
            "option '--lines' requires an argument"
        );
        assert_eq!(
            body(&fail(&["--verbose=1"])),
            "option '--verbose' doesn't allow an argument"
        );
        for a in [["-x"], ["--lines"]] {
            assert_eq!(fail(&a).status, 1);
        }
    }

    /// `--follow`'s argument goes through `argmatch`, which abbreviates too and
    /// has its own two-line diagnostic.
    #[test]
    fn follow_takes_an_optional_abbreviated_keyword() {
        assert_eq!(parse(&["--follow"]).follow, Follow::Descriptor);
        assert!(parse(&["--follow"]).forever);
        assert_eq!(parse(&["--follow=n"]).follow, Follow::Name);
        assert_eq!(parse(&["--follow=d"]).follow, Follow::Descriptor);
        // An *optional* argument is never taken from the next word, so this is
        // follow-by-descriptor of a file called `name`.
        assert_eq!(parse(&["--follow", "name"]).follow, Follow::Descriptor);
        assert_eq!(operands(&["--follow", "name"]), vec!["name"]);
        assert_eq!(
            body(&fail(&["--follow=x"])),
            "invalid argument ‘x’ for ‘--follow’\n\
             Valid arguments are:\n  - ‘descriptor’\n  - ‘name’"
        );
    }

    #[test]
    fn dash_capital_f_is_follow_name_plus_retry() {
        let o = parse(&["-F"]);
        assert!(o.forever);
        assert!(o.retry);
        assert_eq!(o.follow, Follow::Name);
    }

    #[test]
    fn a_lone_dash_and_everything_after_dash_dash_are_operands() {
        assert_eq!(operands(&["-n1", "-", "f"]), vec!["-", "f"]);
        assert_eq!(operands(&["--", "-3", "-v"]), vec!["-3", "-v"]);
        // An option after an operand is still an option: glibc permutes.
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
        assert_eq!(body(&fail(&["-c2n"])), "invalid number of bytes: ‘2n’");
    }

    // ---------------------------------------------------- the obsolete form ---

    #[test]
    fn the_obsolete_form_is_only_recognised_in_three_shapes() {
        // One word.
        assert_eq!(parse(&["-3"]).n_units, 3);
        // Two, where the second is not an option.
        assert_eq!(parse(&["-3", "f"]).n_units, 3);
        assert_eq!(parse(&["-3", "-"]).n_units, 3);
        // Three, where the second is exactly `--`.
        assert_eq!(parse(&["-3", "--", "f"]).n_units, 3);
        // Anything else and the digit reaches getopt, which refuses it — note
        // that whether `-3` is a count depends on words that come *after* it.
        for a in [
            vec!["-3", "-q"],
            vec!["-3", "f", "g"],
            vec!["-3", "-", "-"],
            vec!["-3", "--", "f", "g"],
            vec!["-q", "-3"],
            vec!["f", "-3"],
        ] {
            let e = fail(&a);
            assert_eq!(body(&e), "option used in invalid context -- 3", "{a:?}");
            assert_eq!(e.status, 1);
        }
    }

    #[test]
    fn the_obsolete_form_counts_from_the_start_with_a_plus() {
        let o = parse(&["+3", "f"]);
        assert!(o.from_start);
        assert_eq!(o.n_units, 3);
        // A `+` word in any other position is an operand, not a count.
        assert_eq!(operands(&["-q", "+3"]), vec!["+3"]);
    }

    #[test]
    fn a_bare_dash_and_a_bare_dash_c_fall_through_to_getopt() {
        // Upstream's `!p[p[0] == 'c']` test: `-` is standard input …
        assert_eq!(operands(&["-"]), vec!["-"]);
        // … and `-c` is an option that wants an argument.
        assert_eq!(body(&fail(&["-c"])), "option requires an argument -- 'c'");
        // But `-f`, `-b` and `-l` have no digits and are still the obsolete
        // form, which is how `tail -b` means 5120 bytes.
        assert_eq!(
            parse(&["-f"]),
            Options {
                forever: true,
                ..Options::default()
            }
        );
        assert_eq!(
            parse(&["-b"]),
            Options {
                unit: Unit::Bytes,
                n_units: 5120,
                ..Options::default()
            }
        );
        assert_eq!(parse(&["-l"]), Options::default());
    }

    #[test]
    fn the_obsolete_letters_are_units_and_b_is_also_a_multiplier() {
        // With digits, `b` multiplies the digits …
        assert_eq!(
            parse(&["-2b"]),
            Options {
                unit: Unit::Bytes,
                n_units: 1024,
                ..Options::default()
            }
        );
        // … while without them it multiplies the default, which is the same
        // rule applied in a different place and gives 10 × 512.
        assert_eq!(parse(&["-b"]).n_units, 5120);
        assert_eq!(
            parse(&["-2c"]),
            Options {
                unit: Unit::Bytes,
                n_units: 2,
                ..Options::default()
            }
        );
        assert_eq!(parse(&["-2l"]).n_units, 2);
        assert_eq!(parse(&["-2l"]).unit, Unit::Lines);
        // A trailing `f` is `-f`, and only there.
        assert!(parse(&["-2f"]).forever);
        assert!(parse(&["-2lf"]).forever);
        assert!(parse(&["+2bf"]).forever);
        // `k` and `m` are *not* suffixes in this form — they are junk, which
        // makes the word not obsolete at all, so the digit reaches getopt.
        assert_eq!(body(&fail(&["-2k"])), "option used in invalid context -- 2");
    }

    #[test]
    fn the_obsolete_number_reports_overflow_against_the_whole_word() {
        assert_eq!(
            body(&fail(&["-99999999999999999999999"])),
            "invalid number: ‘-99999999999999999999999’: \
             Numerical result out of range"
        );
        // The ×512 can overflow on its own, with the same sentence.
        assert_eq!(
            body(&fail(&["-99999999999999999999b"])),
            "invalid number: ‘-99999999999999999999b’: \
             Numerical result out of range"
        );
    }

    // ------------------------------------------------------------ numbers ---

    #[test]
    fn counts_take_the_multiplier_suffixes() {
        assert_eq!(parse(&["-n", "3"]).n_units, 3);
        assert_eq!(parse(&["-c", "1b"]).n_units, 512);
        assert_eq!(parse(&["-c", "2K"]).n_units, 2048);
        assert_eq!(parse(&["-c", "2k"]).n_units, 2048);
        assert_eq!(parse(&["-c", "2KB"]).n_units, 2000);
        assert_eq!(parse(&["-c", "2KiB"]).n_units, 2048);
        assert_eq!(parse(&["-c", "2M"]).n_units, 2 * 1024 * 1024);
        assert_eq!(parse(&["-c", "2MB"]).n_units, 2_000_000);
        // A bare suffix means one of it …
        assert_eq!(parse(&["-c", "K"]).n_units, 1024);
        // … but only when it is the very first byte.
        assert_eq!(body(&fail(&["-c", " K"])), "invalid number of bytes: ‘ K’");
        // Leading whitespace and a leading `+` are `strtoumax`'s to skip.
        assert_eq!(parse(&["-n", " 3"]).n_units, 3);
        assert_eq!(parse(&["-n", "+3"]).n_units, 3);
        assert!(parse(&["-n", "+3"]).from_start);
    }

    #[test]
    fn a_bad_count_is_quoted_the_way_gnulib_quotes_it() {
        // `quote()`, whose escaping is C's and not the shell's — the two agree
        // on everything without a backslash in it. A straight `'` inside the
        // value is *not* escaped, because the curly marks around it can never
        // be mistaken for it; only glibc's straight-marked style has to escape.
        assert_eq!(
            body(&fail(&["-n", "a'b"])),
            "invalid number of lines: ‘a'b’"
        );
        assert_eq!(
            body(&fail(&["-n", "a\\b"])),
            "invalid number of lines: ‘a\\\\b’"
        );
        assert_eq!(body(&fail(&["-c", "x"])), "invalid number of bytes: ‘x’");
        // The sign is stripped for `-` and kept for `+`, so the same bad text
        // is echoed back two different ways.
        assert_eq!(body(&fail(&["-n", "-x"])), "invalid number of lines: ‘x’");
        assert_eq!(body(&fail(&["-n", "+x"])), "invalid number of lines: ‘+x’");
        assert_eq!(
            body(&fail(&["-n", "99999999999999999999"])),
            "invalid number of lines: ‘99999999999999999999’: \
             Value too large for defined data type"
        );
        // A bad suffix outranks an overflow.
        assert_eq!(
            body(&fail(&["-n", "99999999999999999999x"])),
            "invalid number of lines: ‘99999999999999999999x’"
        );
    }

    #[test]
    fn from_start_is_sticky_once_set() {
        // Upstream never clears the flag, so this skips one line rather than
        // printing the last two.
        let o = parse(&["-n", "+2", "-n", "2"]);
        assert!(o.from_start);
        assert_eq!(o.n_units, 2);
    }

    #[test]
    fn pid_and_max_unchanged_take_no_suffix_and_have_their_own_wording() {
        assert_eq!(parse(&["--pid=7", "-f"]).pid, Some(7));
        assert_eq!(parse(&["--max-unchanged-stats=3"]).max_unchanged, 3);
        assert_eq!(body(&fail(&["--pid=5k"])), "invalid PID: ‘5k’");
        assert_eq!(body(&fail(&["--pid=x"])), "invalid PID: ‘x’");
        // Above INT_MAX is the overflow wording, not the invalid one.
        assert_eq!(
            body(&fail(&["--pid=2147483648"])),
            "invalid PID: ‘2147483648’: Value too large for defined data type"
        );
        assert_eq!(parse(&["--pid=2147483647", "-f"]).pid, Some(PID_MAX));
        assert_eq!(
            body(&fail(&["--max-unchanged-stats=x"])),
            "invalid maximum number of unchanged stats between opens: ‘x’"
        );
    }

    #[test]
    fn the_sleep_interval_is_a_float_and_strtod_parses_it() {
        assert_eq!(parse(&["-s", "0.5"]).sleep_interval, 0.5);
        assert_eq!(parse(&["-s", "1e1"]).sleep_interval, 10.0);
        assert_eq!(parse(&["-s", " 2"]).sleep_interval, 2.0);
        assert_eq!(parse(&["-s", "0"]).sleep_interval, 0.0);
        // A hexadecimal float is a number to `strtod`, and is not to Rust's
        // own parser — which is why there is a hand-written one.
        assert_eq!(parse(&["-s", "0x1.8p3"]).sleep_interval, 12.0);
        assert_eq!(parse(&["-s", "0x10"]).sleep_interval, 16.0);
        assert_eq!(body(&fail(&["-s", "x"])), "invalid number of seconds: ‘x’");
        assert_eq!(
            body(&fail(&["-s", "-1"])),
            "invalid number of seconds: ‘-1’"
        );
        // NaN parses and is then rejected by `0 <= s`, every comparison
        // against it being false.
        assert_eq!(
            body(&fail(&["-s", "nan"])),
            "invalid number of seconds: ‘nan’"
        );
        // Infinity is not rejected: it is not negative.
        assert!(parse(&["-s", "inf"]).sleep_interval.is_infinite());
    }

    // --------------------------------------------------------------- body ---

    const FIVE: &[u8] = b"1\n2\n3\n4\n5\n";

    #[test]
    fn the_last_n_lines_are_printed() {
        assert_eq!(both(FIVE, &["-n", "2"]), b"4\n5\n");
        assert_eq!(both(FIVE, &["-n", "5"]), FIVE);
        // More than there are is all of them, not an error.
        assert_eq!(both(FIVE, &["-n", "99"]), FIVE);
        assert_eq!(both(FIVE, &[]), FIVE);
        assert_eq!(both(FIVE, &["-n", "0"]), b"");
        assert_eq!(both(b"", &["-n", "2"]), b"");
    }

    #[test]
    fn an_unterminated_final_line_counts_as_a_line() {
        assert_eq!(both(b"1\n2\n3", &["-n", "1"]), b"3");
        assert_eq!(both(b"1\n2\n3", &["-n", "2"]), b"2\n3");
        // And so does a file that is one unterminated line.
        assert_eq!(both(b"only", &["-n", "1"]), b"only");
        assert_eq!(both(b"only", &["-n", "3"]), b"only");
    }

    #[test]
    fn a_trailing_terminator_does_not_open_an_empty_last_line() {
        // `1\n2\n` is two lines, so the last one is `2\n` and not an empty
        // string after it.
        assert_eq!(both(b"1\n2\n", &["-n", "1"]), b"2\n");
        // A file of nothing but terminators is that many empty lines.
        assert_eq!(both(b"\n\n\n", &["-n", "2"]), b"\n\n");
    }

    #[test]
    fn the_last_n_bytes_are_printed() {
        assert_eq!(both(FIVE, &["-c", "4"]), b"4\n5\n");
        assert_eq!(both(FIVE, &["-c", "0"]), b"");
        assert_eq!(both(FIVE, &["-c", "99"]), FIVE);
        assert_eq!(both(b"abc", &["-c", "1"]), b"c");
    }

    #[test]
    fn a_plus_count_skips_from_the_start() {
        assert_eq!(both(FIVE, &["-n", "+3"]), b"3\n4\n5\n");
        assert_eq!(both(FIVE, &["-n", "+1"]), FIVE);
        // Past the end is nothing, and not an error.
        assert_eq!(both(FIVE, &["-n", "+99"]), b"");
        assert_eq!(both(FIVE, &["-c", "+3"]), b"2\n3\n4\n5\n");
        assert_eq!(both(FIVE, &["-c", "+1"]), FIVE);
        assert_eq!(both(FIVE, &["-c", "+99"]), b"");
    }

    /// `-n +0` and `-n 0` are opposites, which is the one place the sign
    /// matters for a value that is otherwise the same number.
    #[test]
    fn plus_zero_is_the_whole_file_and_zero_is_none_of_it() {
        assert_eq!(both(FIVE, &["-n", "+0"]), FIVE);
        assert_eq!(both(FIVE, &["-n", "0"]), b"");
        assert_eq!(both(FIVE, &["-c", "+0"]), FIVE);
        assert_eq!(both(FIVE, &["-c", "0"]), b"");
    }

    #[test]
    fn zero_terminated_changes_what_a_line_is() {
        let input = b"a\0b\0c\0";
        assert_eq!(both(input, &["-z", "-n", "2"]), b"b\0c\0");
        // And newlines are then just bytes.
        assert_eq!(both(b"a\nb\n", &["-z", "-n", "1"]), b"a\nb\n");
    }

    /// The backwards scan reads the file in blocks, so a file bigger than one
    /// block is the case that exercises the loop rather than its first step.
    #[test]
    fn the_backwards_scan_crosses_block_boundaries() {
        let mut input: Vec<u8> = Vec::new();
        for i in 1..=40_000u32 {
            input.extend_from_slice(format!("{i}\n").as_bytes());
        }
        assert_eq!(both(&input, &["-n", "3"]), b"39998\n39999\n40000\n");
        assert_eq!(both(&input, &["-n", "1"]), b"40000\n");
        assert_eq!(both(&input, &["-c", "6"]), b"40000\n");
        assert_eq!(both(&input, &["-c", "7"]), b"\n40000\n");
        assert_eq!(both(&input, &["-n", "+39999"]), b"39999\n40000\n");
        // A line count larger than the file is still the whole file.
        assert_eq!(both(&input, &["-n", "100000"]).len(), input.len());
    }

    /// A line longer than one block is the other way the loop can be wrong: the
    /// terminator it is looking for is nowhere in the block it just read.
    #[test]
    fn a_line_longer_than_a_block_is_still_one_line() {
        let mut input: Vec<u8> = vec![b'x'; BUFSIZ * 3 + 7];
        input.push(b'\n');
        input.extend_from_slice(b"last\n");
        assert_eq!(both(&input, &["-n", "1"]), b"last\n");
        assert_eq!(both(&input, &["-n", "2"]).len(), input.len());
    }

    // ------------------------------------------------------------ routes ---

    /// `-n0` from the end prints nothing at all -- not even an unterminated
    /// last line, which would otherwise be the one line the count is spent
    /// on. `run` stops before reading for a plain `-n0`, but `-f -n0` reads,
    /// and `tail -n0 -f log` must begin silent.
    #[test]
    fn no_lines_from_the_end_is_nothing_even_unterminated() {
        assert_eq!(both(b"abc\ndef", &["-n", "0"]), b"");
        assert_eq!(both(b"abc\ndef\n", &["-n", "0"]), b"");
        assert_eq!(both(b"abc", &["-c", "0"]), b"");
    }

    /// The copy after the backwards scan stops at the end the file had when
    /// its end was measured, so lines written meanwhile are left for `-f`.
    #[test]
    fn the_backwards_scan_copies_no_further_than_the_end_it_measured() {
        let mut input = io::Cursor::new(&b"1\n2\n3\n4\n"[..]);
        let mut out = Vec::new();
        file_lines(&mut input, &mut out, 1, 0, 4, b'\n').unwrap();
        assert_eq!(out, b"2\n");
        let mut out = Vec::new();
        file_lines(&mut input, &mut out, 5, 0, 4, b'\n').unwrap();
        assert_eq!(out, b"1\n2\n");
    }

    /// "Use file_lines only if FD refers to a regular file for which lseek
    /// (... SEEK_END) works": a regular file that will not seek -- as
    /// `/proc/cpuinfo` will not seek to its end -- is read forwards instead.
    /// And a byte count from the end of a regular file that says it is empty,
    /// as every file in `/proc` says, is no more than a block, so it too is
    /// read rather than sought.
    #[test]
    fn a_regular_file_that_will_not_seek_is_read_forwards() {
        let options = as_run_sees(&["-n", "2"]);
        let mut input = Faulty::new(b"a\nb\nc\n", u64::MAX);
        input.seek_fails = true;
        let regular = Facts {
            regular: true,
            size: 6,
            blksize: 0,
        };
        let mut out = Vec::new();
        tail_lines(&mut input, &mut out, 2, &options, regular).unwrap();
        assert_eq!(out, b"b\nc\n");

        let options = as_run_sees(&["-c", "3"]);
        let mut input = Faulty::new(b"a\nb\nc\n", u64::MAX);
        input.seek_fails = true;
        let proc_file = Facts {
            regular: true,
            size: 0,
            blksize: 4096,
        };
        let mut out = Vec::new();
        tail_bytes(&mut input, &mut out, 3, &options, proc_file).unwrap();
        assert_eq!(out, b"\nc\n");
    }

    /// Where a read fails decides what upstream says and whether it goes on:
    /// while still looking for the start it is `Read`, and the run moves on to
    /// the next operand; while copying it is `Copy`, and the run ends.
    #[test]
    fn a_failed_read_is_told_apart_by_where_it_happened() {
        let pipe = Facts {
            regular: false,
            size: 0,
            blksize: DEV_BSIZE,
        };
        let mut options = as_run_sees(&["-n", "2"]);
        options.presume_input_pipe = true;
        let mut out = Vec::new();
        // Counting from the end reads everything first, so a failure prints
        // nothing at all.
        let got = tail_lines(
            &mut Faulty::new(b"a\nb\nc\n", 3),
            &mut out,
            2,
            &options,
            pipe,
        );
        assert!(matches!(got, Err(Failure::Read(_))), "{got:?}");
        let got = tail_bytes(&mut Faulty::new(b"abcdef", 3), &mut out, 2, &options, pipe);
        assert!(matches!(got, Err(Failure::Read(_))), "{got:?}");
        assert!(out.is_empty());

        // From the start: a failure while skipping is `Read`...
        let plus = as_run_sees(&["-n", "+3"]);
        let got = tail_lines(
            &mut Faulty::new(b"a\nb\nc\nd\n", 1),
            &mut out,
            plus.n_units,
            &plus,
            pipe,
        );
        assert!(matches!(got, Err(Failure::Read(_))), "{got:?}");
        assert!(out.is_empty());
        // ...and one after it, while copying, is `Copy`, with what the skip's
        // last read brought already written.
        let got = tail_lines(
            &mut Faulty::new(b"a\nb\nc\nd\n", 6),
            &mut out,
            plus.n_units,
            &plus,
            pipe,
        );
        assert!(matches!(got, Err(Failure::Copy(_))), "{got:?}");
        assert_eq!(out, b"c\n");
    }

    /// A regular file that will not seek where it must ends the run, and the
    /// sentence names the seek it wanted.
    #[test]
    fn a_failed_seek_names_the_offset_it_wanted() {
        let regular = Facts {
            regular: true,
            size: 10_000,
            blksize: 0,
        };
        let mut input = Faulty::new(&[b'x'; 10_000], u64::MAX);
        input.seek_fails = true;
        let options = as_run_sees(&["-c", "+5"]);
        let mut out = Vec::new();
        let got = tail_bytes(&mut input, &mut out, options.n_units, &options, regular);
        assert!(
            matches!(got, Err(Failure::Seek(Whence::Current(4), _))),
            "{got:?}"
        );
        assert_eq!(
            Whence::Current(4).sentence(),
            "cannot seek to relative offset 4"
        );
        assert_eq!(Whence::Set(8192).sentence(), "cannot seek to offset 8192");
    }

    /// `dump_remainder`'s bounds -- a count, one buffer, or the end -- and a
    /// non-blocking input with nothing more to give, which ends a copy
    /// quietly rather than failing it.
    #[test]
    fn a_copy_stops_at_its_bound() {
        let data = vec![b'z'; BUFSIZ * 2 + 5];
        let mut out = Vec::new();
        assert_eq!(
            dump(&mut &data[..], &mut out, Amount::AtMost(3)).unwrap(),
            3
        );
        assert_eq!(
            dump(&mut &data[..], &mut out, Amount::AtMost(0)).unwrap(),
            0
        );
        let mut out = Vec::new();
        assert_eq!(
            dump(&mut &data[..], &mut out, Amount::ABuffer).unwrap(),
            BUFSIZ as u64
        );
        let mut out = Vec::new();
        assert_eq!(
            dump(&mut &data[..], &mut out, Amount::ToEof).unwrap(),
            data.len() as u64
        );
        assert_eq!(out, data);

        let mut out = Vec::new();
        let mut drained = Faulty::new(b"abc", 2);
        drained.would_block = true;
        assert_eq!(dump(&mut drained, &mut out, Amount::ToEof).unwrap(), 2);
        assert_eq!(out, b"ab");
    }

    /// Under `-f`, a `-` that is a pipe is not followed -- `printf x | tail
    /// -f` ends once it has printed -- and nothing else is dropped. An operand
    /// already given up on still counts, as upstream counts it: the loop then
    /// finds nothing live, and says so.
    #[test]
    fn a_piped_standard_input_is_not_followed() {
        let open = |name: &str, fifo: bool| {
            let mut w = Watched::new(&OsString::from(name));
            w.file = Some(Handle::Opened(
                File::open(std::env::current_exe().unwrap()).unwrap(),
            ));
            w.fifo = fifo;
            w
        };
        let mut ws = vec![open("-", true), open("log", true), open("-", false)];
        assert_eq!(ignore_fifo_and_pipe(&mut ws), 2);
        assert!(ws[0].ignore && ws[0].file.is_none());
        assert!(!ws[1].ignore && ws[1].file.is_some());
        assert!(!ws[2].ignore && ws[2].file.is_some());
        assert_eq!(ignore_fifo_and_pipe(&mut ws[..1]), 1);
    }

    /// Upstream's `any_live_files`: an open file is live; so is a name
    /// `--retry` has not given up on, and with `--follow=name` everything is.
    #[test]
    fn what_counts_as_still_worth_following() {
        let mut gone = Watched::new(&OsString::from("log"));
        gone.ignore = true;
        let waiting = Watched::new(&OsString::from("log"));
        let plain = parse(&["-f"]);
        let retry = parse(&["-f", "--retry"]);
        let by_name = parse(&["-F"]);
        assert!(!any_live(std::slice::from_ref(&gone), &plain));
        assert!(!any_live(std::slice::from_ref(&waiting), &plain));
        assert!(any_live(std::slice::from_ref(&waiting), &retry));
        assert!(!any_live(std::slice::from_ref(&gone), &retry));
        assert!(any_live(std::slice::from_ref(&gone), &by_name));
    }

    // ----------------------------------------------------------- warnings ---

    #[test]
    fn the_warned_about_combinations_are_recognised() {
        // `--retry` without following at all …
        let o = parse(&["--retry"]);
        assert!(o.retry && !o.forever);
        // … and with following by descriptor, which is a different warning.
        let o = parse(&["--retry", "-f"]);
        assert!(o.retry && o.forever && o.follow == Follow::Descriptor);
        // `-F` is neither, since it sets `--follow=name` itself.
        let o = parse(&["-F"]);
        assert!(o.retry && o.forever && o.follow == Follow::Name);
        // `--pid` without `-f`.
        assert_eq!(parse(&["--pid=1"]).pid, Some(1));
        assert!(!parse(&["--pid=1"]).forever);
    }

    // --------------------------------------------------------------- help ---

    #[test]
    fn help_and_version_end_parsing_and_outrank_a_bad_operand() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["--version"])), Ok(Request::Version));
        // Abbreviated, and after other options.
        assert_eq!(parse_args(&args(&["-q", "--hel"])), Ok(Request::Help));
        // But a diagnostic earlier on the line still wins, because parsing is
        // left to right.
        assert!(parse_args(&args(&["-x", "--help"])).is_err());
    }

    #[test]
    fn the_help_text_mentions_every_option_it_has() {
        let help = help_text();
        for name in [
            "--bytes",
            "--follow",
            "-F",
            "--lines",
            "--max-unchanged-stats",
            "--pid",
            "--quiet",
            "--silent",
            "--retry",
            "--sleep-interval",
            "--verbose",
            "--zero-terminated",
            "--help",
            "--version",
        ] {
            assert!(help.contains(name), "{name} is missing from --help");
        }
        // The hidden ones stay hidden.
        assert!(!help.contains("presume-input-pipe"));
        assert!(!help.contains("disable-inotify"));
    }
}
