//! `time` — run a command and report its resource usage.
//!
//! A port of **GNU Time 1.9**'s `src/time.c`, measured against the real binary
//! rather than recalled; `scripts/time-diff.sh` is the transcript of that
//! measurement, 174 cases wide.
//!
//! ## This is not the shell's `time`
//!
//! `time` is a *keyword* in bash, ksh and zsh, so `time foo` in a script never
//! reaches a binary at all. The keyword and GNU Time share no option, no format
//! string and no output shape:
//!
//! ```text
//! $ time true            # bash's keyword
//!
//! real    0m0.000s
//! user    0m0.000s
//! sys     0m0.000s
//!
//! $ /usr/bin/time true   # GNU Time 1.9 — this program
//! 0.00user 0.00system 0:00.00elapsed 0%CPU (0avgtext+0avgdata 1152maxresident)k
//! 0inputs+0outputs (0major+72minor)pagefaults 0swaps
//! ```
//!
//! The shipped version imitated the **keyword** — a blank line and one
//! `real\t0m0.000s` — which is the one shape that cannot be reached through a
//! `PATH` lookup, since any shell able to look it up would have used its own
//! keyword first. It also read `argv` as `Vec<String>`, so a command name
//! holding a byte over 0x7f panicked before it ran anything, and it printed
//! Rust's `io::Error` text rather than the C library's.
//!
//! | Shipped | Here |
//! |---|---|
//! | bash's `real 0m0.000s` | GNU Time's two-line summary |
//! | no `-f`, `-o`, `-a`, `-p`, `-q`, `-v` | all of them, and the `TIME` variable |
//! | no format engine | the whole `%`/`\` engine, 25 sequences |
//! | `Vec<String>` argv | bytes, all the way to `exec` |
//! | spawn failure: silent 127 | `time: cannot run NAME: …`, and a full summary |
//! | `missing command`, 1 | `missing program to run`, 125 |
//!
//! ## Quirks reproduced on purpose
//!
//! * A format ending in a bare `%` prints `?` and returns **without the closing
//!   newline** — `summarize`'s `case '\0'` is a `return`, not a `break`, so it
//!   skips both the `putc ('\n')` and the write-error check below it.
//! * `-p` suppresses `Command exited with non-zero status N`; spelling the same
//!   format out as `-f 'real %e\nuser %U\nsys %S'` does not. Upstream compares
//!   the format **pointer** against `posix_format`, not its text. [`Format`]
//!   keeps `Posix` as its own variant for exactly that reason.
//! * `-h` is not an option. The help text says `-h,  --help`, the switch has a
//!   `case 'h'`, and the getopt string is `"+af:o:pqvV"` — so only the long form
//!   reaches it and `-h` is `invalid option -- 'h'`.
//! * The help text's `-p` paragraph really does print `real %%e`: upstream
//!   passes a `%%`-escaped string to `fputs`, which does not interpret it.
//! * `Commonly usaged` is upstream's typo, and `-h,  --help` really has two
//!   spaces.
//!
//! ## The one quirk *not* reproduced
//!
//! A format ending in a bare backslash makes upstream print `?`, `\`, the
//! format's own NUL byte, and then whatever follows it in memory — measured,
//! `time -f 'ab\' true` prints `ab?\<NUL>true`, having walked into the adjacent
//! `argv` string. That is an out-of-bounds read, not a quirk; ours stops at the
//! end of the format. `scripts/time-diff.sh` carries it as an xfail.
//!
//! ## Where the numbers come from
//!
//! Upstream uses `wait3`, which hands back the child's own `rusage`. There is
//! no `wait3` here, so this waits and then asks `getrusage (RUSAGE_CHILDREN)`,
//! which is the same answer whenever exactly one child has ever been reaped —
//! and this program reaps exactly one. On SlateOS the kernel currently leaves
//! `RUSAGE_CHILDREN` zeroed (`posix/src/resource.rs`), so every memory and
//! fault field reads `0` until it does not; the shape of the output is right
//! either way, which is what the diff harness compares.
//!
//! ## The name
//!
//! The source file is `time_cmd.rs` because `src/bin/time.rs` would collide
//! with `std::time` in the doc tooling and read badly beside it, so the binary
//! cargo builds is `time_cmd`. Nothing renames it on installation today; the
//! diff harness makes a symlink named `time` so `argv[0]` — and therefore the
//! `time: ` on every diagnostic — matches GNU's.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// Upstream's `PROGRAM_NAME`, and the prefix on every diagnostic.
const NAME: &str = "time";

/// `EXIT_CANCELED` — everything this program gets wrong before it has a child
/// to blame: a bad option, a missing operand, an unopenable `-o` file.
const EXIT_CANCELED: u8 = 125;
/// `EXIT_CANNOT_INVOKE` — the child found the program and could not run it.
const EXIT_CANNOT_INVOKE: u8 = 126;
/// `EXIT_ENOENT` — the child could not find the program at all.
const EXIT_ENOENT: u8 = 127;
/// What a signal number is added to, to make a shell-shaped exit status.
const SIGNALLED_OFFSET: i32 = 128;

const TIME: Program = Program::new(NAME, EXIT_CANCELED as i32);

/// Upstream's option string. The leading `+` stops parsing at the first
/// operand, so everything after the command name belongs to the *command* —
/// `time echo -p` echoes `-p` rather than switching to the POSIX format. Note
/// the absent `h`: `--help` works and `-h` does not.
const SHORT_OPTIONS: &str = "+af:o:pqvV";

/// Upstream's `longopts`, in its order — which is the order an ambiguous
/// prefix's candidate list is built from, so `--v` must offer `verbose` before
/// `version`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("append", Takes::Nothing),
    ("format", Takes::Required),
    ("help", Takes::Nothing),
    ("output-file", Takes::Required),
    ("portability", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Upstream's `default_format`. The embedded newline is real: the summary is
/// two lines, and a third — the one `summarize` adds at the end — closes it.
const DEFAULT_FORMAT: &str = "%Uuser %Ssystem %Eelapsed %PCPU \
     (%Xavgtext+%Davgdata %Mmaxresident)k\n\
     %Iinputs+%Ooutputs (%Fmajor+%Rminor)pagefaults %Wswaps";

/// Upstream's `posix_format`, reached by `-p`.
const POSIX_FORMAT: &str = "real %e\nuser %U\nsys %S";

/// Upstream's `longstats`, which `-v` concatenates into one format string.
/// The last entry has no trailing newline because `summarize` supplies it.
const LONGSTATS: &[&str] = &[
    "\tCommand being timed: \"%C\"\n",
    "\tUser time (seconds): %U\n",
    "\tSystem time (seconds): %S\n",
    "\tPercent of CPU this job got: %P\n",
    "\tElapsed (wall clock) time (h:mm:ss or m:ss): %E\n",
    "\tAverage shared text size (kbytes): %X\n",
    "\tAverage unshared data size (kbytes): %D\n",
    "\tAverage stack size (kbytes): %p\n",
    "\tAverage total size (kbytes): %K\n",
    "\tMaximum resident set size (kbytes): %M\n",
    "\tAverage resident set size (kbytes): %t\n",
    "\tMajor (requiring I/O) page faults: %F\n",
    "\tMinor (reclaiming a frame) page faults: %R\n",
    "\tVoluntary context switches: %w\n",
    "\tInvoluntary context switches: %c\n",
    "\tSwaps: %W\n",
    "\tFile system inputs: %I\n",
    "\tFile system outputs: %O\n",
    "\tSocket messages sent: %s\n",
    "\tSocket messages received: %r\n",
    "\tSignals delivered: %k\n",
    "\tPage size (bytes): %Z\n",
    "\tExit status: %x",
];

/// Upstream's `TICKS_PER_SEC`, and the conversion built on it. It is a
/// hard-coded 100 there too — `MSEC_TO_TICKS` turns the millisecond totals
/// below back into clock ticks, because the `rusage` memory fields are
/// kilobyte-*ticks* and have to be divided by the tick count to become
/// kilobytes.
const MSEC_PER_TICK: i64 = 10;

// ---------------------------------------------------------------- types ----

/// The format string, kept as *which* format rather than as its text.
///
/// [`Format::Posix`] exists as its own variant because upstream's `summarize`
/// tests `output_format != posix_format` — a **pointer** comparison. `-p` and
/// `-f 'real %e\nuser %U\nsys %S'` therefore behave differently despite
/// producing identical output, and a port that stored only the text could not
/// tell them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Format {
    Default,
    Posix,
    Verbose,
    Given(Vec<u8>),
}

impl Format {
    fn text(&self) -> Vec<u8> {
        match self {
            Format::Default => DEFAULT_FORMAT.as_bytes().to_vec(),
            Format::Posix => POSIX_FORMAT.as_bytes().to_vec(),
            Format::Verbose => LONGSTATS.concat().into_bytes(),
            Format::Given(bytes) => bytes.clone(),
        }
    }

    /// Whether the abnormal-termination line is suppressed. See the type docs.
    fn is_posix(&self) -> bool {
        matches!(self, Format::Posix)
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    format: Format,
    quiet: bool,
    append: bool,
    outfile: Option<OsString>,
    /// The command and its arguments. Empty means `missing program to run`.
    command: Vec<OsString>,
}

/// `struct rusage`, in the fields this program reads. Kilobytes on Linux —
/// upstream's `RUSAGE_MEM_TO_KB` is the identity there, and only the page-based
/// kernels it no longer supports needed the conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Usage {
    utime_sec: i64,
    utime_usec: i64,
    stime_sec: i64,
    stime_usec: i64,
    maxrss: i64,
    ixrss: i64,
    idrss: i64,
    isrss: i64,
    minflt: i64,
    majflt: i64,
    nswap: i64,
    inblock: i64,
    oublock: i64,
    msgsnd: i64,
    msgrcv: i64,
    nsignals: i64,
    nvcsw: i64,
    nivcsw: i64,
}

/// Upstream's `RESUSE`: what the child cost, and how it ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Resuse {
    elapsed_sec: i64,
    elapsed_usec: i64,
    usage: Usage,
    /// A raw `wait` status, not an exit code — `summarize` and `main` both pull
    /// it apart with the `W*` macros, and `%x` prints only its exit half.
    waitstatus: i32,
}

// ----------------------------------------------------------------- main ----

fn main() -> ExitCode {
    // Reopen-as-`/dev/null` undone: a descriptor the shell closed must fail
    // with `EBADF` here, because a `ferror` on the summary stream is the whole
    // subject of `time true 2>&-`. See [`stdfd::restore`].
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let time_env = std::env::var_os("TIME");
    ExitCode::from(run(&args, time_env.as_deref()))
}

fn run(args: &[OsString], time_env: Option<&OsStr>) -> u8 {
    let settings = match scan(args, time_env) {
        Ok(Action::Help) => {
            let mut out = Stream::stdout();
            let _ = out.write_all(help_text().as_bytes());
            // Deliberately discarded. GNU Time does **not** register gnulib's
            // `close_stdout`, so nothing checks this write: measured,
            // `time --help >&-` exits 0 in silence and `time --help
            // >/dev/full` exits 0 too. `nice`'s rules are the opposite ones,
            // and copying them here would be wrong.
            let _ = out.finish();
            return 0;
        }
        Ok(Action::Version) => {
            let mut out = Stream::stdout();
            let _ = out.write_all(b"time (SlateOS coreutils) 0.1.0\n");
            let _ = out.finish();
            return 0;
        }
        Ok(Action::Run(settings)) => settings,
        Err(e) => {
            TIME.report(&e);
            return EXIT_CANCELED;
        }
    };

    if settings.command.is_empty() {
        // `error (0, 0, _("missing program to run"))` followed by
        // `usage (EXIT_CANCELED)`, which prints only the referral.
        TIME.report(&TIME.usage_referring("missing program to run".to_string()));
        return EXIT_CANCELED;
    }

    let mut sink = match open_sink(settings.outfile.as_deref(), settings.append) {
        Ok(sink) => sink,
        Err(status) => return status,
    };

    let res = match run_command(&settings.command) {
        Ok(res) => res,
        Err(status) => return status,
    };
    let fmt = settings.format.text();
    let outcome = summarize(
        &mut sink,
        &fmt,
        &settings.command,
        &res,
        settings.quiet,
        settings.format.is_posix(),
    );

    if outcome.is_err() {
        // `error (1, errno, "write error")`. Note the status: not 125, and not
        // the child's — a summary that could not be written is worth 1.
        let mut line = b"time: write error".to_vec();
        if let Some(e) = sink.take_error() {
            line.extend_from_slice(b": ");
            line.extend_from_slice(strerror(&e).as_bytes());
        }
        line.push(b'\n');
        stdfd::diag_bytes(&line);
        return 1;
    }

    // Upstream's `fflush (outfp)`, unchecked — which is why `time -o /dev/full
    // true` exits 0 while `time true 2>/dev/full` exits 1. A `-o FILE` stream
    // is block-buffered, so its first write happens here, after the last
    // `ferror` test that could have noticed it.
    sink.finish();

    exit_status(res.waitstatus)
}

/// Upstream's `main` tail: a `wait` status becomes a shell-shaped exit code.
fn exit_status(waitstatus: i32) -> u8 {
    if wifstopped(waitstatus) {
        clamp_status(wstopsig(waitstatus).saturating_add(SIGNALLED_OFFSET))
    } else if wifsignaled(waitstatus) {
        clamp_status(wtermsig(waitstatus).saturating_add(SIGNALLED_OFFSET))
    } else if wifexited(waitstatus) {
        clamp_status(wexitstatus(waitstatus))
    } else {
        // "shouldn't happen", says upstream, and it is right — but the branch
        // is here rather than folded into one of the others because folding it
        // would invent a status for a state neither of them describes.
        stdfd::diag_line(&format!("time: unknown status from command ({waitstatus})"));
        1
    }
}

fn clamp_status(value: i32) -> u8 {
    u8::try_from(value).unwrap_or(1)
}

// ------------------------------------------------------- wait(2) status ----

fn wifexited(status: i32) -> bool {
    status & 0x7f == 0
}

fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xff
}

fn wifstopped(status: i32) -> bool {
    status & 0xff == 0x7f
}

fn wstopsig(status: i32) -> i32 {
    (status >> 8) & 0xff
}

fn wifsignaled(status: i32) -> bool {
    // glibc: `((signed char) (((status) & 0x7f) + 1) >> 1) > 0` — true for a
    // termination signal, false for both 0 (exited) and 0x7f (stopped).
    let sig = status & 0x7f;
    sig != 0 && sig != 0x7f
}

fn wtermsig(status: i32) -> i32 {
    status & 0x7f
}

// -------------------------------------------------- the command line ----

fn scan(args: &[OsString], time_env: Option<&OsStr>) -> Result<Action, getopt::Error> {
    // The environment first, so that `-f` and `-p` and `-v` all override it.
    // Upstream reads it before the option loop for exactly this reason, and
    // its comment says so.
    let mut format = match time_env {
        Some(value) => Format::Given(os_bytes(value).into_owned()),
        None => Format::Default,
    };
    let mut verbose = false;
    let mut quiet = false;
    let mut append = false;
    let mut outfile: Option<OsString> = None;

    let mut parser = TIME.parse(args, SHORT_OPTIONS, LONG_OPTIONS);
    let at = loop {
        let Some(item) = parser.next() else {
            break parser.optind();
        };
        match item? {
            Opt::Short(b'a', _) | Opt::Long("append", _) => append = true,
            Opt::Short(b'f', value) | Opt::Long("format", value) => {
                format = Format::Given(
                    value
                        .as_deref()
                        .map(|v| os_bytes(v).into_owned())
                        .unwrap_or_default(),
                );
            }
            // No `-h`: it is absent from [`SHORT_OPTIONS`], so it arrives as
            // `invalid option -- 'h'` and never reaches here.
            Opt::Long("help", _) => return Ok(Action::Help),
            Opt::Short(b'o', value) | Opt::Long("output-file", value) => outfile = value,
            Opt::Short(b'p', _) | Opt::Long("portability", _) => format = Format::Posix,
            Opt::Short(b'q', _) | Opt::Long("quiet", _) => quiet = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => verbose = true,
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Action::Version),
            Opt::Operand(_) => {
                // `+` in the shorts string means the parser has stopped here,
                // and `optind` is one *past* the operand.
                break parser.optind().saturating_sub(1);
            }
            Opt::Short(..) | Opt::Long(..) => {}
        }
    };

    // After the loop, and unconditionally — which is why `-v` beats a later
    // `-f` as well as an earlier one, and beats `-p` in both orders.
    if verbose {
        format = Format::Verbose;
    }

    Ok(Action::Run(Settings {
        format,
        quiet,
        append,
        outfile,
        command: args.get(at..).unwrap_or_default().to_vec(),
    }))
}

// ------------------------------------------------ where the summary goes ----

/// `BUFSIZ`, near enough: what a `-o FILE` stream holds before it writes.
const BLOCK: usize = 4096;

/// The summary's destination, reduced to what [`summarize`] needs of it.
///
/// A trait rather than the concrete [`Sink`] so the format engine can be tested
/// against an in-memory writer, which is the only way to assert on a format's
/// bytes without a child process and a temporary file.
trait Out {
    fn put(&mut self, bytes: &[u8]);
    /// `ferror (fp)`: whether any write so far has failed.
    fn errored(&self) -> bool;
}

/// Where the summary goes — and the reason `time true 2>/dev/full` exits 1
/// while `time -o /dev/full true` exits 0.
///
/// `stderr` is unbuffered, so a failed write is visible *inside* `summarize`,
/// where upstream checks `ferror` and calls `error (1, …)`. A file stream is
/// block-buffered, so nothing is written until `main`'s `fflush` — which
/// upstream does not check. Reproducing that asymmetry needs both buffering
/// disciplines, which is why this is not simply a `Stream`.
enum Sink {
    Standard(Stream),
    File {
        file: std::fs::File,
        buf: Vec<u8>,
        /// The first failure, kept sticky the way `ferror` is.
        error: Option<std::io::Error>,
    },
}

fn open_sink(outfile: Option<&OsStr>, append: bool) -> Result<Sink, u8> {
    let Some(path) = outfile else {
        return Ok(Sink::Standard(Stream::stderr()));
    };
    let mut options = std::fs::OpenOptions::new();
    if append {
        options.append(true).create(true);
    } else {
        options.write(true).create(true).truncate(true);
    }
    match options.open(path) {
        Ok(file) => Ok(Sink::File {
            file,
            buf: Vec::with_capacity(BLOCK),
            error: None,
        }),
        Err(e) => {
            // `error (EXIT_CANCELED, errno, "%s", outfile)`: the name unquoted,
            // and as the bytes it was given — `-o` takes a path, and a path
            // here is not necessarily UTF-8.
            let mut line = format!("{NAME}: ").into_bytes();
            line.extend_from_slice(&os_bytes(path));
            line.extend_from_slice(b": ");
            line.extend_from_slice(strerror(&e).as_bytes());
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            Err(EXIT_CANCELED)
        }
    }
}

fn flush_block(file: &mut std::fs::File, buf: &mut Vec<u8>, error: &mut Option<std::io::Error>) {
    if buf.is_empty() {
        return;
    }
    // Keep the *first* failure: `ferror` is a sticky flag upstream, and the
    // errno `write error: …` quotes is the one that first set it.
    if let Err(e) = file.write_all(buf)
        && error.is_none()
    {
        *error = Some(e);
    }
    buf.clear();
}

impl Sink {
    /// The failure behind [`Out::errored`], for wording `write error: …`.
    fn take_error(&mut self) -> Option<std::io::Error> {
        match self {
            Sink::Standard(stream) => stream.error(),
            Sink::File { error, .. } => error.take(),
        }
    }

    /// Upstream's unchecked `fflush (outfp)`. The result is discarded on
    /// purpose; see the type docs.
    fn finish(self) {
        match self {
            Sink::Standard(stream) => {
                let _ = stream.finish();
            }
            Sink::File {
                mut file,
                mut buf,
                mut error,
            } => flush_block(&mut file, &mut buf, &mut error),
        }
    }
}

impl Out for Sink {
    fn put(&mut self, bytes: &[u8]) {
        match self {
            Sink::Standard(stream) => {
                // Never fails; the failure is recorded on the stream and read
                // back by `errored` below, which is exactly `ferror`.
                let _ = stream.write_all(bytes);
            }
            Sink::File { file, buf, error } => {
                buf.extend_from_slice(bytes);
                if buf.len() >= BLOCK {
                    flush_block(file, buf, error);
                }
            }
        }
    }

    fn errored(&self) -> bool {
        match self {
            Sink::Standard(stream) => stream.errored(),
            Sink::File { error, .. } => error.is_some(),
        }
    }
}

// --------------------------------------------------- the format engine ----

/// The millisecond and microsecond totals every `%` directive is derived from,
/// computed once before the walk exactly as upstream does.
struct Numbers {
    /// Elapsed real milliseconds.
    r: i64,
    /// Elapsed virtual (CPU) milliseconds.
    v: i64,
    /// Elapsed real microseconds.
    us_r: i64,
    /// Elapsed virtual (CPU) microseconds.
    us_v: i64,
    /// `MSEC_TO_TICKS (v)`: the divisor that turns the `rusage` memory fields
    /// from kilobyte-ticks into kilobytes. Zero means "took no measurable CPU
    /// time", and upstream prints a plain `0` rather than dividing by it.
    ticks: i64,
}

/// A memory field averaged over the CPU time the child used.
fn per_tick(value: i64, ticks: i64) -> i64 {
    if ticks == 0 { 0 } else { value / ticks }
}

/// Upstream's `summarize`.
///
/// Returns `Err` where upstream calls `error (1, errno, "write error")` — after
/// any directive whose write failed, and after the closing newline.
fn summarize(
    out: &mut dyn Out,
    fmt: &[u8],
    command: &[OsString],
    resp: &Resuse,
    quiet: bool,
    posix: bool,
) -> Result<(), ()> {
    // `if (!quiet && output_format != posix_format)`. The second half is a
    // pointer comparison upstream, so `-p` suppresses this line and an
    // identical `-f` string does not. See [`Format`].
    if !quiet && !posix {
        let status = resp.waitstatus;
        if wifstopped(status) {
            out.put(format!("Command stopped by signal {}\n", wstopsig(status)).as_bytes());
        } else if wifsignaled(status) {
            out.put(format!("Command terminated by signal {}\n", wtermsig(status)).as_bytes());
        } else if wifexited(status) && wexitstatus(status) != 0 {
            out.put(
                format!(
                    "Command exited with non-zero status {}\n",
                    wexitstatus(status)
                )
                .as_bytes(),
            );
        }
    }

    let u = &resp.usage;
    let v = u
        .utime_sec
        .saturating_mul(1000)
        .saturating_add(u.utime_usec / 1000)
        .saturating_add(u.stime_sec.saturating_mul(1000))
        .saturating_add(u.stime_usec / 1000);
    let numbers = Numbers {
        r: resp
            .elapsed_sec
            .saturating_mul(1000)
            .saturating_add(resp.elapsed_usec / 1000),
        v,
        us_r: resp.elapsed_usec,
        us_v: u.utime_usec.saturating_add(u.stime_usec),
        ticks: v / MSEC_PER_TICK,
    };

    let mut at = 0usize;
    while let Some(&byte) = fmt.get(at) {
        at = at.saturating_add(1);
        match byte {
            b'%' => match fmt.get(at) {
                // `case '\0': putc ('?', fp); return;` — a `return`, not a
                // `break`, so the closing newline below is never written and
                // neither is the write-error check. Measured: `time -f 'abc%'
                // true` prints `abc?` with no newline.
                None => {
                    out.put(b"?");
                    return Ok(());
                }
                Some(&c) => {
                    directive(out, c, command, resp, &numbers);
                    at = at.saturating_add(1);
                }
            },
            b'\\' => match fmt.get(at) {
                // Where upstream reads past the end of the string; see the
                // module docs. Ours stops, and the closing newline below still
                // happens — which is what upstream would have done had its
                // `default` case not been fed a NUL.
                None => out.put(b"?\\"),
                Some(b't') => {
                    out.put(b"\t");
                    at = at.saturating_add(1);
                }
                Some(b'n') => {
                    out.put(b"\n");
                    at = at.saturating_add(1);
                }
                Some(b'\\') => {
                    out.put(b"\\");
                    at = at.saturating_add(1);
                }
                Some(&c) => {
                    out.put(b"?\\");
                    out.put(&[c]);
                    at = at.saturating_add(1);
                }
            },
            _ => out.put(&[byte]),
        }

        if out.errored() {
            return Err(());
        }
    }

    out.put(b"\n");
    if out.errored() {
        return Err(());
    }
    Ok(())
}

/// One `%` directive. The letters are upstream's, in upstream's order.
fn directive(out: &mut dyn Out, c: u8, command: &[OsString], resp: &Resuse, n: &Numbers) {
    let u = &resp.usage;
    let text = match c {
        b'%' => {
            out.put(b"%");
            return;
        }
        b'C' => {
            // `fprintargv (fp, command, " ")` — the words as bytes, joined by
            // a single space and neither quoted nor escaped.
            for (index, word) in command.iter().enumerate() {
                if index > 0 {
                    out.put(b" ");
                }
                out.put(&os_bytes(word));
            }
            return;
        }
        // Average unshared data size: data plus stack, each averaged apart.
        b'D' => (per_tick(u.idrss, n.ticks).saturating_add(per_tick(u.isrss, n.ticks))).to_string(),
        b'E' => {
            let sec = resp.elapsed_sec;
            if sec >= 3600 {
                format!("{}:{:02}:{:02}", sec / 3600, (sec % 3600) / 60, sec % 60)
            } else {
                format!(
                    "{}:{:02}.{:02}",
                    sec / 60,
                    sec % 60,
                    resp.elapsed_usec / 10_000
                )
            }
        }
        b'F' => u.majflt.to_string(),
        b'I' => u.inblock.to_string(),
        // Average total memory: data, stack and text.
        b'K' => per_tick(u.idrss, n.ticks)
            .saturating_add(per_tick(u.isrss, n.ticks))
            .saturating_add(per_tick(u.ixrss, n.ticks))
            .to_string(),
        b'M' => u.maxrss.to_string(),
        b'O' => u.oublock.to_string(),
        b'P' => {
            // Total CPU time over elapsed time, falling back to microseconds
            // for a command too short to register a millisecond, and to `?`
            // for one too short to register a microsecond.
            if n.r > 0 {
                format!("{}%", n.v.saturating_mul(100) / n.r)
            } else if n.us_r > 0 {
                format!("{}%", n.us_v.saturating_mul(100) / n.us_r)
            } else {
                "?%".to_string()
            }
        }
        b'R' => u.minflt.to_string(),
        b'S' => centiseconds(u.stime_sec, u.stime_usec),
        b'U' => centiseconds(u.utime_sec, u.utime_usec),
        b'W' => u.nswap.to_string(),
        b'X' => per_tick(u.ixrss, n.ticks).to_string(),
        b'Z' => imp::page_size().to_string(),
        b'c' => u.nivcsw.to_string(),
        b'e' => centiseconds(resp.elapsed_sec, resp.elapsed_usec),
        b'k' => u.nsignals.to_string(),
        b'p' => per_tick(u.isrss, n.ticks).to_string(),
        b'r' => u.msgrcv.to_string(),
        b's' => u.msgsnd.to_string(),
        b't' => per_tick(u.idrss, n.ticks).to_string(),
        b'w' => u.nvcsw.to_string(),
        b'x' => wexitstatus(resp.waitstatus).to_string(),
        _ => {
            // `default: putc ('?', fp); putc (*fmt, fp);`
            out.put(b"?");
            out.put(&[c]);
            return;
        }
    };
    out.put(text.as_bytes());
}

/// `%ld.%02ld` of a `timeval` — seconds and hundredths.
///
/// Upstream spells the hundredths `TV_MSEC / 10` for `%S` and `%U` and
/// `tv_usec / 10000` for `%e`; integer division is associative enough that
/// `(usec / 1000) / 10 == usec / 10000` for every non-negative `usec`, so one
/// function serves all three.
fn centiseconds(sec: i64, usec: i64) -> String {
    format!("{}.{:02}", sec, usec / 10_000)
}

// -------------------------------------------------------- running it ----

/// Upstream's `run_command`, minus the fork.
///
/// The difference that shows: upstream's `execvp` failure is reported by the
/// **child**, which then `_exit`s 126 or 127, so the parent still waits, still
/// prints `Command exited with non-zero status N`, and still prints the whole
/// summary. `Command::spawn` fails in the *parent*, so the message is written
/// here and the wait status the child would have left is synthesised — without
/// which `time /nope` would print one line where GNU prints four.
fn run_command(command: &[OsString]) -> Result<Resuse, u8> {
    let mut resp = Resuse::default();
    let Some(program) = command.first() else {
        return Ok(resp);
    };

    let start = std::time::Instant::now();
    let mut builder = std::process::Command::new(program);
    builder.args(command.get(1..).unwrap_or_default());

    match builder.spawn() {
        Ok(mut child) => {
            // "Have signals kill the child but not self (if possible)."  Set
            // after the spawn, so the child keeps the default dispositions —
            // which is what fork-then-ignore gives upstream too.
            let interrupt = imp::ignore(imp::SIGINT);
            let quit = imp::ignore(imp::SIGQUIT);
            let waited = child.wait();
            imp::restore(imp::SIGINT, interrupt);
            imp::restore(imp::SIGQUIT, quit);
            match waited {
                Ok(status) => resp.waitstatus = imp::raw_status(&status),
                Err(e) => {
                    // `error (1, errno, "error waiting for child process")`,
                    // which exits before anything is summarised.
                    stdfd::diag_line(&format!(
                        "{NAME}: error waiting for child process: {}",
                        strerror(&e)
                    ));
                    return Err(1);
                }
            }
        }
        Err(e) => {
            let mut line = format!("{NAME}: cannot run ").into_bytes();
            line.extend_from_slice(&os_bytes(program));
            line.extend_from_slice(b": ");
            line.extend_from_slice(strerror(&e).as_bytes());
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            // `saved_errno == ENOENT ? EXIT_ENOENT : EXIT_CANNOT_INVOKE`.
            // Which one it is is glibc's rule for `execvp`: the most specific
            // failure of the whole `PATH` walk wins, so a single `EACCES`
            // anywhere beats the trailing `ENOENT`.
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                EXIT_ENOENT
            } else {
                EXIT_CANNOT_INVOKE
            };
            resp.waitstatus = i32::from(code) << 8;
        }
    }

    let elapsed = start.elapsed();
    resp.elapsed_sec = i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX);
    resp.elapsed_usec = i64::from(elapsed.subsec_micros());
    // Upstream gets this from `wait3`, which reports the one child it reaped.
    // `RUSAGE_CHILDREN` is the same answer here because this process reaps
    // exactly one child in its whole life.
    resp.usage = imp::children_usage();
    Ok(resp)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Usage;
    use std::process::ExitStatus;

    pub const SIGINT: i32 = 2;
    pub const SIGQUIT: i32 = 3;
    const SIG_IGN: usize = 1;
    const RUSAGE_CHILDREN: i32 = -1;
    const SC_PAGESIZE: i32 = 30;

    #[repr(C)]
    struct Timeval {
        sec: i64,
        usec: i64,
    }

    /// `struct rusage`: two `timeval`s and fourteen `long`s, in the order
    /// `maxrss ixrss idrss isrss minflt majflt nswap inblock oublock msgsnd
    /// msgrcv nsignals nvcsw nivcsw`.
    #[repr(C)]
    struct RawUsage {
        utime: Timeval,
        stime: Timeval,
        counters: [i64; 14],
    }

    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut RawUsage) -> i32;
        fn sysconf(name: i32) -> i64;
        fn signal(signum: i32, handler: usize) -> usize;
    }

    pub fn children_usage() -> Usage {
        let mut raw = RawUsage {
            utime: Timeval { sec: 0, usec: 0 },
            stime: Timeval { sec: 0, usec: 0 },
            counters: [0; 14],
        };
        // SAFETY: `raw` is a live, fully-initialised `RawUsage` laid out to
        // match `struct rusage`, and `getrusage` only writes through the
        // pointer for the duration of the call.
        if unsafe { getrusage(RUSAGE_CHILDREN, &mut raw) } != 0 {
            return Usage::default();
        }
        let c = raw.counters;
        Usage {
            utime_sec: raw.utime.sec,
            utime_usec: raw.utime.usec,
            stime_sec: raw.stime.sec,
            stime_usec: raw.stime.usec,
            maxrss: c[0],
            ixrss: c[1],
            idrss: c[2],
            isrss: c[3],
            minflt: c[4],
            majflt: c[5],
            nswap: c[6],
            inblock: c[7],
            oublock: c[8],
            msgsnd: c[9],
            msgrcv: c[10],
            nsignals: c[11],
            nvcsw: c[12],
            nivcsw: c[13],
        }
    }

    /// `getpagesize ()`, which POSIX spells `sysconf (_SC_PAGESIZE)`.
    pub fn page_size() -> i64 {
        // SAFETY: `sysconf` reads no memory through a pointer and has no
        // precondition beyond a valid name.
        let value = unsafe { sysconf(SC_PAGESIZE) };
        if value > 0 { value } else { 4096 }
    }

    pub fn ignore(signum: i32) -> usize {
        // SAFETY: installing `SIG_IGN` runs no Rust code on delivery, so there
        // is no async-signal-safety obligation to discharge.
        unsafe { signal(signum, SIG_IGN) }
    }

    pub fn restore(signum: i32, previous: usize) {
        // SAFETY: `previous` is whatever `signal` handed back for this same
        // signal a moment ago, so it is a disposition this process already had.
        unsafe {
            signal(signum, previous);
        }
    }

    /// The raw `wait` status, which `summarize` and `exit_status` both take
    /// apart with the `W*` macros. `ExitStatus::code` would have thrown away
    /// the signal half.
    pub fn raw_status(status: &ExitStatus) -> i32 {
        use std::os::unix::process::ExitStatusExt;
        status.into_raw()
    }
}

/// The host build, so that `cargo test` and the workspace check compile. None
/// of it runs on the target.
#[cfg(not(target_os = "linux"))]
mod imp {
    use super::Usage;
    use std::process::ExitStatus;

    pub const SIGINT: i32 = 2;
    pub const SIGQUIT: i32 = 3;

    pub fn children_usage() -> Usage {
        Usage::default()
    }

    pub fn page_size() -> i64 {
        4096
    }

    pub fn ignore(_signum: i32) -> usize {
        0
    }

    pub fn restore(_signum: i32, _previous: usize) {}

    pub fn raw_status(status: &ExitStatus) -> i32 {
        (status.code().unwrap_or(0) & 0xff) << 8
    }
}

// ------------------------------------------------------------- --help ----

/// Upstream's `usage (EXIT_SUCCESS)`, less its closing three lines.
///
/// Those name the GNU project's website, manual and bug address, and this is
/// not that project; `scripts/time-diff.sh` carries `--help` as an xfail for
/// that reason and no other. Everything above them is verbatim, including
/// `Commonly usaged`, the two spaces in `-h,  --help`, and the `%%` in the
/// `-p` paragraph — upstream hands a `%%`-escaped string to `fputs`, which
/// does not interpret it, so the doubled sign really is what it prints.
fn help_text() -> String {
    format!(
        "\
Usage: {NAME} [OPTIONS] COMMAND [ARG]...
Run COMMAND, then print system resource usage.

  -a, --append              with -o FILE, append instead of overwriting
  -f, --format=FORMAT       use the specified FORMAT instead of the default
  -o, --output=FILE         write to FILE instead of STDERR
  -p, --portability         print POSIX standard 1003.2 conformant string:
                              real %%e
                              user %%U
                              sys %%S
  -q, --quiet               do not print information about abnormal program
                            termination (non-zero exit codes or signals)
  -v, --verbose             print all resource usage information instead of
                            the default format
  -h,  --help               display this help and exit
  -V,  --version            output version information and exit

Commonly usaged format sequences for -f/--format:
(see documentation for full list)
  %%   a literal '%'
  %C   command line and arguments
  %c   involuntary context switches
  %E   elapsed real time (wall clock) in [hour:]min:sec
  %e   elapsed real time (wall clock) in seconds
  %F   major page faults
  %M   maximum resident set size in KB
  %P   percent of CPU this job got
  %R   minor page faults
  %S   system (kernel) time in seconds
  %U   user time in seconds
  %w   voluntary context switches
  %x   exit status of command

Default output format:
{DEFAULT_FORMAT}

NOTE: your shell may have its own version of {NAME}, which usually supersedes
the version described here.  Please refer to your shell's documentation
for details about the options it supports.
"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn settings(words: &[&str]) -> Settings {
        match scan(&args(words), None) {
            Ok(Action::Run(settings)) => settings,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn with_env(env: &str, words: &[&str]) -> Settings {
        match scan(&args(words), Some(OsStr::new(env))) {
            Ok(Action::Run(settings)) => settings,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// An [`Out`] that keeps what it was given, and can be told to fail.
    #[derive(Default)]
    struct Recorder {
        bytes: Vec<u8>,
        broken: bool,
    }

    impl Out for Recorder {
        fn put(&mut self, bytes: &[u8]) {
            if !self.broken {
                self.bytes.extend_from_slice(bytes);
            }
        }

        fn errored(&self) -> bool {
            self.broken
        }
    }

    /// A `RESUSE` with values chosen so every derived field is distinguishable:
    /// 1.23 s user, 0.45 s system, 75.25 s elapsed, and memory counters that do
    /// not divide evenly by the 168 ticks that 1.68 s of CPU comes to.
    fn sample() -> Resuse {
        Resuse {
            elapsed_sec: 75,
            elapsed_usec: 250_000,
            usage: Usage {
                utime_sec: 1,
                utime_usec: 230_000,
                stime_sec: 0,
                stime_usec: 450_000,
                maxrss: 1152,
                ixrss: 300,
                idrss: 400,
                isrss: 500,
                minflt: 72,
                majflt: 1,
                nswap: 0,
                inblock: 8,
                oublock: 16,
                msgsnd: 2,
                msgrcv: 3,
                nsignals: 4,
                nvcsw: 5,
                nivcsw: 6,
            },
            waitstatus: 0,
        }
    }

    fn render(fmt: &str, resp: &Resuse) -> String {
        render_as(fmt, resp, false, false)
    }

    fn render_as(fmt: &str, resp: &Resuse, quiet: bool, posix: bool) -> String {
        let mut out = Recorder::default();
        let command = args(&["cmd", "arg"]);
        summarize(&mut out, fmt.as_bytes(), &command, resp, quiet, posix).unwrap();
        String::from_utf8(out.bytes).unwrap()
    }

    // --------------------------------------------------- the format table ----

    #[test]
    fn nothing_given_is_the_default_format() {
        assert_eq!(settings(&["true"]).format, Format::Default);
    }

    #[test]
    fn the_environment_supplies_a_format() {
        assert_eq!(
            with_env("T=%e", &["true"]).format,
            Format::Given(b"T=%e".to_vec())
        );
    }

    #[test]
    fn dash_f_overrides_the_environment() {
        assert_eq!(
            with_env("T=%e", &["-f", "F=%e", "true"]).format,
            Format::Given(b"F=%e".to_vec())
        );
    }

    #[test]
    fn an_empty_format_is_a_format() {
        // Not the same as no `-f` at all: `time -f '' false` prints only the
        // status line and a newline.
        assert_eq!(settings(&["-f", "", "true"]).format, Format::Given(vec![]));
    }

    #[test]
    fn dash_p_is_its_own_variant() {
        // And not `Given(POSIX_FORMAT)`, which is the whole point: upstream
        // compares the format pointer, so `-p` and an identical `-f` differ.
        assert_eq!(settings(&["-p", "true"]).format, Format::Posix);
        assert!(settings(&["-p", "true"]).format.is_posix());
        assert!(!settings(&["-f", POSIX_FORMAT, "true"]).format.is_posix());
    }

    #[test]
    fn the_last_of_dash_f_and_dash_p_wins() {
        assert_eq!(
            settings(&["-p", "-f", "e=%e", "true"]).format.text(),
            b"e=%e"
        );
        assert_eq!(
            settings(&["-f", "e=%e", "-p", "true"]).format,
            Format::Posix
        );
    }

    #[test]
    fn verbose_wins_in_either_order() {
        // Upstream applies `-v` *after* the option loop, so it beats a later
        // `-f` as well as an earlier one.
        for words in [
            &["-v", "-f", "e=%e", "true"][..],
            &["-f", "e=%e", "-v", "true"][..],
            &["-v", "-p", "true"][..],
            &["-p", "-v", "true"][..],
        ] {
            assert_eq!(settings(words).format, Format::Verbose, "{words:?}");
        }
    }

    #[test]
    fn verbose_beats_the_environment_too() {
        assert_eq!(with_env("T=%e", &["-v", "true"]).format, Format::Verbose);
    }

    #[test]
    fn the_verbose_format_is_the_longstats_run_together() {
        let text = Format::Verbose.text();
        assert!(text.starts_with(b"\tCommand being timed: \"%C\"\n"));
        assert!(text.ends_with(b"\tExit status: %x"));
        assert_eq!(text.iter().filter(|&&b| b == b'\n').count(), 22);
    }

    // ------------------------------------------------------- the operands ----

    #[test]
    fn options_after_the_command_belong_to_the_command() {
        // The `+` in [`SHORT_OPTIONS`]. `time echo -p` echoes `-p`.
        let run = settings(&["echo", "-p"]);
        assert_eq!(run.command, args(&["echo", "-p"]));
        assert_eq!(run.format, Format::Default);
    }

    #[test]
    fn even_help_after_the_command_belongs_to_the_command() {
        let run = settings(&["sh", "-c", "echo hi", "sh", "--help"]);
        assert_eq!(run.command, args(&["sh", "-c", "echo hi", "sh", "--help"]));
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        assert_eq!(settings(&["--", "true"]).command, args(&["true"]));
        assert_eq!(
            settings(&["--", "echo", "-p"]).command,
            args(&["echo", "-p"])
        );
        assert_eq!(settings(&["--", "-"]).command, args(&["-"]));
    }

    #[test]
    fn a_bare_double_dash_leaves_no_command() {
        assert!(settings(&["--"]).command.is_empty());
    }

    #[test]
    fn no_arguments_at_all_leaves_no_command() {
        assert!(settings(&[]).command.is_empty());
    }

    #[test]
    fn an_option_with_no_command_leaves_no_command() {
        // `time -p` is `missing program to run`, not a run of nothing.
        assert!(settings(&["-p"]).command.is_empty());
        assert!(settings(&["-a"]).command.is_empty());
    }

    #[test]
    fn the_command_may_be_bytes_that_are_not_utf8() {
        use std::ffi::OsString;
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"caf\xe9".to_vec())
        };
        #[cfg(not(unix))]
        let name = OsString::from("cafe");
        let argv = vec![name.clone()];
        match scan(&argv, None) {
            Ok(Action::Run(run)) => assert_eq!(run.command, vec![name]),
            other => panic!("expected a run, got {other:?}"),
        }
    }

    // --------------------------------------------------- the other flags ----

    #[test]
    fn quiet_and_append_and_output_file() {
        let run = settings(&["-q", "-a", "-o", "out", "true"]);
        assert!(run.quiet);
        assert!(run.append);
        assert_eq!(run.outfile, Some(OsString::from("out")));
    }

    #[test]
    fn output_is_an_unambiguous_abbreviation_of_output_file() {
        assert_eq!(
            settings(&["--output=out", "true"]).outfile,
            Some(OsString::from("out"))
        );
    }

    #[test]
    fn long_help_and_version_are_recognised() {
        assert_eq!(scan(&args(&["--help"]), None), Ok(Action::Help));
        assert_eq!(scan(&args(&["--version"]), None), Ok(Action::Version));
        assert_eq!(scan(&args(&["-V"]), None), Ok(Action::Version));
    }

    #[test]
    fn short_h_is_not_an_option() {
        // Upstream's getopt string is "+af:o:pqvV" — no `h`, despite the help
        // text advertising `-h,  --help` and the switch having a `case 'h'`.
        let e = scan(&args(&["-h", "true"]), None).unwrap_err();
        assert!(e.message().starts_with("invalid option"), "{}", e.message());
    }

    #[test]
    fn an_ambiguous_abbreviation_is_rejected() {
        // `--v` is both `--verbose` and `--version`.
        assert!(scan(&args(&["--v", "true"]), None).is_err());
    }

    // ------------------------------------------------- the format engine ----

    #[test]
    fn plain_text_gets_a_closing_newline() {
        assert_eq!(
            render("no sequences at all", &sample()),
            "no sequences at all\n"
        );
        assert_eq!(render("", &sample()), "\n");
    }

    #[test]
    fn a_literal_percent() {
        assert_eq!(render("lit%%eral", &sample()), "lit%eral\n");
        assert_eq!(render("%%%%", &sample()), "%%\n");
    }

    #[test]
    fn an_unknown_directive_is_a_question_mark_and_the_letter() {
        assert_eq!(render("a%Qb", &sample()), "a?Qb\n");
        assert_eq!(render("%q", &sample()), "?q\n");
    }

    #[test]
    fn a_trailing_percent_loses_the_closing_newline() {
        // `case '\0'` is a `return`, not a `break`. Measured against GNU.
        assert_eq!(render("abc%", &sample()), "abc?");
        assert_eq!(render("%", &sample()), "?");
    }

    #[test]
    fn escapes() {
        assert_eq!(render("a\\tb\\nc\\\\d", &sample()), "a\tb\nc\\d\n");
    }

    #[test]
    fn an_unknown_escape_is_a_question_mark_and_both_characters() {
        assert_eq!(render("\\q", &sample()), "?\\q\n");
        assert_eq!(render("a\\qb", &sample()), "a?\\qb\n");
    }

    #[test]
    fn a_trailing_backslash_stops_at_the_end_of_the_format() {
        // Upstream prints `?`, `\`, the format's own NUL, and then whatever
        // follows it in memory. This is the one difference on purpose; see the
        // module docs and the xfail in `scripts/time-diff.sh`.
        assert_eq!(render("ab\\", &sample()), "ab?\\\n");
    }

    #[test]
    fn the_command_line_is_joined_with_spaces() {
        assert_eq!(render("C=[%C]", &sample()), "C=[cmd arg]\n");
    }

    #[test]
    fn times_are_seconds_and_hundredths() {
        assert_eq!(render("%U", &sample()), "1.23\n");
        assert_eq!(render("%S", &sample()), "0.45\n");
        assert_eq!(render("%e", &sample()), "75.25\n");
    }

    #[test]
    fn elapsed_under_an_hour_is_minutes_and_seconds() {
        assert_eq!(render("%E", &sample()), "1:15.25\n");
    }

    #[test]
    fn elapsed_over_an_hour_switches_to_hours() {
        let mut resp = sample();
        resp.elapsed_sec = 3661;
        assert_eq!(render("%E", &resp), "1:01:01\n");
    }

    #[test]
    fn percent_cpu_is_cpu_over_elapsed() {
        // 1680 ms of CPU in 75250 ms of wall clock.
        assert_eq!(render("%P", &sample()), "2%\n");
    }

    #[test]
    fn percent_cpu_falls_back_to_microseconds_then_to_a_question_mark() {
        let mut resp = Resuse::default();
        resp.elapsed_usec = 400;
        resp.usage.utime_usec = 100;
        resp.usage.stime_usec = 100;
        assert_eq!(render("%P", &resp), "50%\n");

        assert_eq!(render("%P", &Resuse::default()), "?%\n");
    }

    #[test]
    fn the_memory_fields_are_averaged_over_the_cpu_ticks() {
        // 1.68 s of CPU is 168 ticks: ixrss 300 -> 1, idrss 400 -> 2,
        // isrss 500 -> 2.
        assert_eq!(render("%X %t %p %D %K %M", &sample()), "1 2 2 4 5 1152\n");
    }

    #[test]
    fn a_command_too_short_to_measure_prints_zero_rather_than_dividing() {
        let resp = Resuse {
            usage: Usage {
                ixrss: 300,
                idrss: 400,
                isrss: 500,
                ..Usage::default()
            },
            ..Resuse::default()
        };
        assert_eq!(render("%X %t %p %D %K", &resp), "0 0 0 0 0\n");
    }

    #[test]
    fn the_plain_counters() {
        assert_eq!(
            render("%F %I %O %R %W %c %k %r %s %w", &sample()),
            "1 8 16 72 0 6 4 3 2 5\n"
        );
    }

    #[test]
    fn the_exit_status_directive_is_the_exit_half_of_the_wait_status() {
        // Quiet, so that the announcement line an abnormal status also earns
        // (covered by `a_non_zero_exit_is_announced` and `a_signal_is_announced`)
        // does not crowd out the one directive under test.
        let mut resp = sample();
        resp.waitstatus = 42 << 8;
        assert_eq!(render_as("x=%x", &resp, true, false), "x=42\n");
        resp.waitstatus = 9; // killed by SIGKILL: no exit status at all
        assert_eq!(render_as("x=%x", &resp, true, false), "x=0\n");
    }

    // ------------------------------------- the abnormal-termination line ----

    #[test]
    fn a_non_zero_exit_is_announced() {
        let mut resp = sample();
        resp.waitstatus = 42 << 8;
        assert_eq!(
            render("", &resp),
            "Command exited with non-zero status 42\n\n"
        );
    }

    #[test]
    fn a_zero_exit_is_not_announced() {
        assert_eq!(render("", &sample()), "\n");
    }

    #[test]
    fn a_signal_is_announced() {
        let mut resp = sample();
        resp.waitstatus = 15;
        assert_eq!(render("", &resp), "Command terminated by signal 15\n\n");
    }

    #[test]
    fn a_stop_is_announced() {
        let mut resp = sample();
        resp.waitstatus = (19 << 8) | 0x7f;
        assert_eq!(render("", &resp), "Command stopped by signal 19\n\n");
    }

    #[test]
    fn quiet_drops_the_line_and_nothing_else() {
        let mut resp = sample();
        resp.waitstatus = 42 << 8;
        assert_eq!(render_as("x=%x", &resp, true, false), "x=42\n");
    }

    #[test]
    fn the_posix_format_drops_the_line_too() {
        let mut resp = sample();
        resp.waitstatus = 42 << 8;
        assert_eq!(render_as("x=%x", &resp, false, true), "x=42\n");
    }

    // ------------------------------------------------------ write errors ----

    #[test]
    fn a_broken_stream_is_reported_after_the_first_directive() {
        let mut out = Recorder {
            bytes: Vec::new(),
            broken: true,
        };
        let command = args(&["cmd"]);
        assert_eq!(
            summarize(&mut out, b"abc", &command, &sample(), false, false),
            Err(())
        );
    }

    #[test]
    fn a_trailing_percent_never_notices_a_broken_stream() {
        // The `return` skips the write-error check as well as the newline, so
        // `time -f 'abc%' true 2>/dev/full` exits with the child's status.
        let mut out = Recorder {
            bytes: Vec::new(),
            broken: true,
        };
        let command = args(&["cmd"]);
        assert_eq!(
            summarize(&mut out, b"%", &command, &sample(), false, false),
            Ok(())
        );
    }

    // ------------------------------------------------------ wait statuses ----

    #[test]
    fn statuses_become_shell_shaped_exit_codes() {
        assert_eq!(exit_status(0), 0);
        assert_eq!(exit_status(42 << 8), 42);
        assert_eq!(exit_status(255 << 8), 255);
        assert_eq!(exit_status(15), 143);
        assert_eq!(exit_status(9), 137);
        assert_eq!(exit_status((19 << 8) | 0x7f), 147);
    }

    #[test]
    fn the_wait_macros_agree_on_which_kind_a_status_is() {
        assert!(wifexited(0) && !wifsignaled(0) && !wifstopped(0));
        assert!(wifsignaled(15) && !wifexited(15) && !wifstopped(15));
        let stopped = (19 << 8) | 0x7f;
        assert!(wifstopped(stopped) && !wifsignaled(stopped) && !wifexited(stopped));
    }

    // ------------------------------------------------------------ pieces ----

    #[test]
    fn hundredths_truncate_rather_than_round() {
        assert_eq!(centiseconds(0, 0), "0.00");
        assert_eq!(centiseconds(0, 9_999), "0.00");
        assert_eq!(centiseconds(0, 10_000), "0.01");
        assert_eq!(centiseconds(7, 999_999), "7.99");
    }

    #[test]
    fn per_tick_treats_no_ticks_as_zero() {
        assert_eq!(per_tick(1000, 0), 0);
        assert_eq!(per_tick(1000, 3), 333);
    }

    #[test]
    fn the_default_format_is_the_two_lines_gnu_prints() {
        // Spelled out here rather than referred to, so that a stray edit to
        // the constant fails a test instead of quietly changing the output.
        let expected = concat!(
            "%Uuser %Ssystem %Eelapsed %PCPU ",
            "(%Xavgtext+%Davgdata %Mmaxresident)k\n",
            "%Iinputs+%Ooutputs (%Fmajor+%Rminor)pagefaults %Wswaps"
        );
        assert_eq!(Format::Default.text(), expected.as_bytes());
    }

    #[test]
    fn the_help_text_keeps_upstreams_typos() {
        let help = help_text();
        assert!(help.contains("Commonly usaged format sequences"));
        assert!(help.contains("  -h,  --help "));
        assert!(help.contains("                              real %%e\n"));
        assert!(!help.contains("gnu.org"));
    }
}
