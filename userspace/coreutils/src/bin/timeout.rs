//! `timeout` -- run a command, and stop it if it is still running after a time
//! limit.
//!
//! A port of GNU coreutils 9.4's `src/timeout.c`, measured against the real
//! binary by `scripts/timeout-diff.sh`. It replaces `userspace/timeout`, a
//! from-scratch program that resembled it from the outside and differed in
//! nearly everything a script would notice:
//!
//! | | the standalone (deleted) | GNU, and this port |
//! |---|---|---|
//! | waiting | polled the child every 10 ms | sleeps in `sigsuspend` until the timer or the child |
//! | who is signalled | the child alone, so its own children ran on | the child and its whole process group |
//! | a signal sent to `timeout` | killed `timeout`, the command ran on | passed on to the command |
//! | a command killed by a signal | exit 1 | `timeout` dies of the same signal, so the shell sees 128+N |
//! | `-s` | eight names, numbers to 31 | every name `kill -l` knows, `RTMIN+n`, `137` read as 9 |
//! | `--foreground` | accepted and ignored | no new process group, no `SIGCONT` |
//! | `timeout inf cmd` | a panic: `Duration::from_secs_f64(inf)` | no time limit |
//! | `timeout 5 -v cmd` | `-v` read as an option | `-v` is the command, as with `nice` |
//! | `-9` | an option meaning `-s 9` | `invalid option -- '9'` |
//!
//! # How upstream runs the command, and why this does the same
//!
//! `timeout` puts itself in a new process group, installs its handlers, and
//! only then forks, so that no signal can arrive before there is something to
//! catch it. The child resets `SIGTTIN` and `SIGTTOU` (which the parent
//! ignores, so that a background command reading the terminal stops instead
//! of `timeout`) and calls `execvp`; if that fails it says so itself and exits
//! 126 or 127. The parent arms a timer whose expiry is `SIGALRM`, blocks every
//! signal its handler acts on and `SIGCHLD`, and then alternates a
//! non-blocking `waitpid` with `sigsuspend` -- which unblocks them for exactly
//! as long as it waits, so none is lost between the check and the wait.
//!
//! When time runs out the handler sends the signal to the command and to the
//! whole group (ignoring it itself first), then `SIGCONT` to both so that a
//! stopped command can act on it; with `-k` it re-arms the timer for
//! `SIGKILL`. A signal sent to `timeout` itself goes the same way.
//!
//! `std::process::Command` cannot be made to do this -- see `libcall::process`
//! -- so these are the C library's own calls, through `libcall`.
//!
//! # The timer is `setitimer`: upstream's other arm
//!
//! `timeout.c` arms its timer with `timer_create` and `timer_settime` where
//! `configure` finds them, else with `setitimer`, else with `alarm`. This uses
//! the `setitimer` arm, which upstream compiles on systems without POSIX
//! timers. On SlateOS that is not a matter of taste: its library's
//! `timer_settime` reports success and arms nothing
//! (`known-issues/B-POSIX-TIMER-SETTIME-REPORTS-SUCCESS-AND-ARMS-NOTHING.md`),
//! so the first arm would never time anything out, while `setitimer` reaches
//! the kernel's real interval timer. The difference elsewhere is resolution, a
//! microsecond instead of a nanosecond, which no process limit can observe.
//!
//! # A command killed by a signal takes `timeout` with it
//!
//! A shell reports a command killed by signal N as `128+N`, and can tell that
//! from a command that exited 128+N only by asking how it ended. So unless the
//! time limit is what killed it, `timeout` makes itself non-dumpable (no core
//! file on top of the command's) and raises the same signal against itself:
//! measured, `timeout 9 sh -c 'kill -TERM $$'` leaves the shell a status of
//! 143 *and* the "Terminated" message. If it cannot disable core dumps it
//! warns and exits 128+N instead.
//!
//! On SlateOS today that is what happens: the library's native `prctl` does
//! not take `PR_SET_DUMPABLE` yet, though the kernel keeps the flag
//! (`requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`).
//!
//! # What upstream's handler does that is checked here
//!
//! `timeout -v` prints from inside the handler, so the line is put together
//! in a stack buffer and written with one `write(2)`: [`sig2str::signame`]
//! names the signal without allocating, and the quoted command was prepared
//! before any handler was installed. The handler saves and restores `errno`
//! around everything it does, where upstream saves it around the timer call
//! alone -- a difference no output can show, and one that closes a window in
//! which the interrupted code would read the handler's `errno` as its own.

use coreutils::getopt::{self, Opt, Program, Report, Takes};
use coreutils::interval;
use coreutils::quote::{os_bytes, quote};
use coreutils::sig2str;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

// Recorded before `main`: a descriptor the caller closed must reach the
// command closed, not as the `/dev/null` the runtime puts there.
coreutils::guard_std_fds!();

/// `EXIT_TIMEDOUT`: the command ran out of time.
#[cfg_attr(not(unix), allow(dead_code))]
const EXIT_TIMEDOUT: u8 = 124;
/// `EXIT_CANCELED`: `timeout` itself failed -- a bad command line, a `fork`
/// that did not happen, a diagnostic that could not be written.
const EXIT_CANCELED: u8 = 125;
/// `EXIT_CANNOT_INVOKE`: the command was found but could not be run.
#[cfg_attr(not(unix), allow(dead_code))]
const EXIT_CANNOT_INVOKE: u8 = 126;
/// `EXIT_ENOENT`: the command was not found.
#[cfg_attr(not(unix), allow(dead_code))]
const EXIT_ENOENT: u8 = 127;

const TIMEOUT_NAME: &str = "timeout";
const TIMEOUT: Program = Program::new(TIMEOUT_NAME, EXIT_CANCELED as i32);

/// Upstream's `"+k:s:v"`. The `+` stops option parsing at the first operand,
/// so everything after the duration belongs to the command: measured,
/// `timeout 5 -v true` runs a command called `-v`.
const SHORT_OPTIONS: &str = "+k:s:v";

/// Upstream's `long_options`, in its order, so that an abbreviation resolves
/// as it does there -- `--k` is `--kill-after`, and `--v` is ambiguous
/// between `--verbose` and `--version`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("kill-after", Takes::Required),
    ("signal", Takes::Required),
    ("verbose", Takes::Nothing),
    ("foreground", Takes::Nothing),
    ("preserve-status", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum Request {
    Help,
    Version,
    Run(Job),
}

/// A command, and the terms it runs on.
#[cfg_attr(test, derive(Debug, PartialEq))]
struct Job {
    /// Seconds until the first signal; 0 is no limit at all.
    duration: f64,
    /// `-k`: seconds after the first signal until `SIGKILL`; 0 is never.
    kill_after: f64,
    /// `-s`: the first signal, `TERM` unless given.
    signal: i32,
    /// `--foreground`: no process group of its own, no `SIGCONT`.
    foreground: bool,
    /// `--preserve-status`: the command's status even when it timed out.
    preserve_status: bool,
    /// `-v`: say which signal is sent.
    verbose: bool,
    /// The command and its arguments; never empty.
    argv: Vec<OsString>,
}

/// Why the command line was refused.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum Refusal {
    /// A diagnostic, then the exit.
    Said(getopt::Error),
    /// Upstream's bare `usage (EXIT_CANCELED)`, when there are fewer than two
    /// operands: the referral line and nothing before it. Measured.
    Usage,
}

/// GNU 9.4's `--help`, without the block of project links no bin here carries.
const HELP: &str = "\
Usage: timeout [OPTION] DURATION COMMAND [ARG]...
  or:  timeout [OPTION]
Start COMMAND, and kill it if still running after DURATION.

Mandatory arguments to long options are mandatory for short options too.
      --preserve-status
                 exit with the same status as COMMAND, even when the
                   command times out
      --foreground
                 when not running timeout directly from a shell prompt,
                   allow COMMAND to read from the TTY and get TTY signals;
                   in this mode, children of COMMAND will not be timed out
  -k, --kill-after=DURATION
                 also send a KILL signal if COMMAND is still running
                   this long after the initial signal was sent
  -s, --signal=SIGNAL
                 specify the signal to be sent on timeout;
                   SIGNAL may be a name like 'HUP' or a number;
                   see 'kill -l' for a list of signals
  -v, --verbose  diagnose to stderr any signal sent upon timeout
      --help        display this help and exit
      --version     output version information and exit

DURATION is a floating point number with an optional suffix:
's' for seconds (the default), 'm' for minutes, 'h' for hours or 'd' for days.
A duration of 0 disables the associated timeout.

Upon timeout, send the TERM signal to COMMAND, if no other SIGNAL specified.
The TERM signal kills any process that does not block or catch that signal.
It may be necessary to use the KILL signal, since this signal can't be caught.

Exit status:
  124  if COMMAND times out, and --preserve-status is not specified
  125  if the timeout command itself fails
  126  if COMMAND is found but cannot be invoked
  127  if COMMAND cannot be found
  137  if COMMAND (or timeout itself) is sent the KILL (9) signal (128+9)
  -    the exit status of COMMAND otherwise
";

/// The funnel: a diagnostic that could not be delivered makes the status 125,
/// which is upstream's `atexit (close_stdout)` with `EXIT_CANCELED` as its
/// failure.
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), EXIT_CANCELED)
}

fn run_main() -> ExitCode {
    // First, before anything can touch a standard descriptor: one the caller
    // closed is passed on closed.
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    let request = match scan(&args) {
        Ok(request) => request,
        Err(Refusal::Said(e)) => {
            TIMEOUT.report(&e);
            return ExitCode::from(EXIT_CANCELED);
        }
        Err(Refusal::Usage) => {
            stdfd::diag_line(&format!(
                "Try '{TIMEOUT_NAME} --help' for more information."
            ));
            return ExitCode::from(EXIT_CANCELED);
        }
    };

    match request {
        Request::Help => {
            let mut out = Stream::stdout();
            // Unchecked here: the verdict is `close_stdout_with`'s, below.
            let _ = out.write_all(HELP.as_bytes());
            stdfd::close_stdout_with(TIMEOUT_NAME, out, ExitCode::SUCCESS, EXIT_CANCELED)
        }
        Request::Version => {
            let mut out = Stream::stdout();
            // As for the help.
            let _ = out.write_all(b"timeout (SlateOS coreutils) 0.1.0\n");
            stdfd::close_stdout_with(TIMEOUT_NAME, out, ExitCode::SUCCESS, EXIT_CANCELED)
        }
        Request::Run(job) => ExitCode::from(imp::run(&job)),
    }
}

/// `timeout: MESSAGE` on stderr -- upstream's `error (0, …)`.
#[cfg_attr(not(unix), allow(dead_code))]
fn say(message: &str) {
    stdfd::diag_line(&format!("{TIMEOUT_NAME}: {message}"));
}

/// `strerror` for an `errno`.
#[cfg_attr(not(unix), allow(dead_code))]
fn strerror_of(errno: i32) -> String {
    coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

// ------------------------------------------------------------- scanning ----

/// Read the command line as `timeout.c`'s `main` does: options in order, each
/// acted on as it is met -- so a bad `-k` or `-s` stops the scan where it
/// stands, and `--help` wins only over what comes after it -- then at least
/// two operands, the first a duration.
///
/// # Errors
///
/// A getopt diagnostic, an invalid interval or signal, or too few operands.
fn scan(args: &[OsString]) -> Result<Request, Refusal> {
    let mut job = Job {
        duration: 0.0,
        kill_after: 0.0,
        signal: libcall::SIGTERM,
        foreground: false,
        preserve_status: false,
        verbose: false,
        argv: Vec::new(),
    };
    let mut parser = TIMEOUT.parse(args, SHORT_OPTIONS, LONG_OPTIONS);
    let mut first_operand = None;
    // `while let` rather than `for`: the operand arm needs `optind`, which a
    // `for` loop's borrow of the parser would not let it ask for.
    #[allow(clippy::while_let_on_iterator)]
    while let Some(item) = parser.next() {
        match item.map_err(Refusal::Said)? {
            Opt::Short(b'k', value) | Opt::Long("kill-after", value) => {
                job.kill_after = duration_of(&value.unwrap_or_default())?;
            }
            Opt::Short(b's', value) | Opt::Long("signal", value) => {
                job.signal = signal_of(&value.unwrap_or_default())?;
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => job.verbose = true,
            Opt::Long("foreground", _) => job.foreground = true,
            Opt::Long("preserve-status", _) => job.preserve_status = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(_) => {
                // The `+` has stopped the parser, one word *past* the operand.
                first_operand = Some(parser.optind().saturating_sub(1));
                break;
            }
            // Unreachable: the parser yields only the options declared above.
            // Refusing rather than ignoring, so one added to the tables
            // without a handler fails loudly.
            Opt::Short(other, _) => return Err(Refusal::Said(TIMEOUT.invalid_option(other))),
            Opt::Long(other, _) => {
                return Err(Refusal::Said(
                    TIMEOUT.usage_referring(format!("option '--{other}' is unhandled")),
                ));
            }
        }
    }
    // No operand met: the end of argv, or `--` (after which `optind` points).
    let start = first_operand.unwrap_or_else(|| parser.optind());
    // Counted before the duration is read: upstream's `argc - optind < 2`
    // comes first, so `timeout x` is the bare referral and not a complaint
    // about `x`. Measured.
    let Some([duration, command @ ..]) = args.get(start..) else {
        return Err(Refusal::Usage);
    };
    if command.is_empty() {
        return Err(Refusal::Usage);
    }
    job.duration = duration_of(duration)?;
    job.argv = command.to_vec();
    Ok(Request::Run(job))
}

/// A duration, or upstream's `invalid time interval ‘X’` and referral.
fn duration_of(text: &OsString) -> Result<f64, Refusal> {
    let bytes = os_bytes(text.as_os_str());
    interval::seconds(&bytes).ok_or_else(|| {
        Refusal::Said(TIMEOUT.usage_referring(format!("invalid time interval {}", quote(&bytes))))
    })
}

/// A signal, or `operand2sig`'s `‘X’: invalid signal` and the referral.
fn signal_of(text: &OsString) -> Result<i32, Refusal> {
    let bytes = os_bytes(text.as_os_str());
    sig2str::operand2sig(&bytes).ok_or_else(|| {
        Refusal::Said(TIMEOUT.usage_referring(format!("{}: invalid signal", quote(&bytes))))
    })
}

// ---------------------------------------------------------------- timing ----

/// gnulib's `dtotimespec`: seconds as a `double`, as whole seconds and
/// nanoseconds -- the fraction rounded *up* to the next nanosecond, so that no
/// positive duration rounds down to none, and anything outside `time_t`'s
/// range clamped to its ends.
#[cfg_attr(not(unix), allow(dead_code))]
fn dtotimespec(sec: f64) -> (i64, i64) {
    const HZ: i64 = 1_000_000_000;
    // `TYPE_MINIMUM (time_t)` and `1.0 + TYPE_MAXIMUM (time_t)`: -2^63 and
    // 2^63, both exact in a `double`.
    #[allow(clippy::cast_precision_loss)]
    let (lowest, past_highest) = (i64::MIN as f64, 1.0 + i64::MAX as f64);
    if sec.is_nan() || sec <= lowest {
        return (i64::MIN, 0);
    }
    if sec >= past_highest {
        return (i64::MAX, HZ.saturating_sub(1));
    }
    // C's conversion, toward zero; in range, by the two tests above.
    #[allow(clippy::cast_possible_truncation)]
    let whole = sec as i64;
    #[allow(clippy::cast_precision_loss)]
    let frac = (HZ as f64) * (sec - whole as f64);
    #[allow(clippy::cast_possible_truncation)]
    let mut ns = frac as i64;
    #[allow(clippy::cast_precision_loss)]
    let short = (ns as f64) < frac;
    if short {
        ns = ns.saturating_add(1);
    }
    let mut whole = whole.saturating_add(ns / HZ);
    ns %= HZ;
    if ns < 0 {
        whole = whole.saturating_sub(1);
        ns = ns.saturating_add(HZ);
    }
    (whole, ns)
}

/// `settimeout`'s `setitimer` arm: whole seconds and nanoseconds as the
/// seconds and microseconds `setitimer` takes, the nanoseconds rounded up --
/// carrying into the seconds, or, at the very top of the range, giving back
/// the microsecond rather than overflowing.
#[cfg_attr(not(unix), allow(dead_code))]
fn to_timeval(sec: i64, nsec: i64) -> (i64, i64) {
    let usec = nsec.saturating_add(999) / 1000;
    if usec < 1_000_000 {
        (sec, usec)
    } else if sec == i64::MAX {
        (sec, 999_999)
    } else {
        (sec.saturating_add(1), 0)
    }
}

/// The whole seconds `alarm` is given when `setitimer` is refused: the
/// duration rounded up, or `UINT_MAX` for anything that large.
#[cfg_attr(not(unix), allow(dead_code))]
fn alarm_seconds(duration: f64) -> u32 {
    if f64::from(u32::MAX) <= duration {
        return u32::MAX;
    }
    // In range: below `u32::MAX` by the test above, and never negative or
    // NaN, because the interval reader refuses both.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let floor = duration as u32;
    floor.saturating_add(u32::from(f64::from(floor) < duration))
}

// ------------------------------------------------------------------ unix ----

/// Running the command: everything that needs `fork`, signals and a timer.
#[cfg(unix)]
mod imp {
    use super::{
        EXIT_CANCELED, EXIT_CANNOT_INVOKE, EXIT_ENOENT, EXIT_TIMEDOUT, Job, TIMEOUT_NAME,
        alarm_seconds, dtotimespec, say, strerror_of, to_timeval,
    };
    use coreutils::quote::{os_bytes, quote};
    use coreutils::sig2str;
    use coreutils::stdfd;
    use libcall::process::{self, Forked};
    use libcall::signal::{
        self, ErrnoGuard, SIGALRM, SIGCHLD, SIGCONT, SIGHUP, SIGINT, SIGKILL, SIGPIPE, SIGQUIT,
        SIGTERM, SIGTTIN, SIGTTOU, SigSet,
    };
    use std::ffi::{CStr, CString};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering::SeqCst};

    // What the handler reads. Statics, because a handler is called with a
    // signal number and nothing else; atomics, because it runs between two
    // instructions of the code it interrupts. All are set before any handler
    // is installed.

    /// The command's process id. 0 until `fork` returns it -- and always 0 in
    /// the child, whose copy was made before the parent stored it, which is
    /// how the handler tells the two apart.
    static MONITORED_PID: AtomicI32 = AtomicI32::new(0);
    /// The timer has run out. Read once the command has been reaped.
    static TIMED_OUT: AtomicBool = AtomicBool::new(false);
    /// The signal the timer's expiry sends: `-s`, then `SIGKILL` once `-k`'s
    /// second timer is armed.
    static TERM_SIGNAL: AtomicI32 = AtomicI32::new(SIGTERM);
    /// `-k`'s seconds, as `f64` bits; 0 once spent, or if never given.
    static KILL_AFTER: AtomicU64 = AtomicU64::new(0);
    static FOREGROUND: AtomicBool = AtomicBool::new(false);
    static VERBOSE: AtomicBool = AtomicBool::new(false);
    /// The command's name, already quoted for `-v`'s line, so that the handler
    /// need not allocate to print it.
    static COMMAND: OnceLock<Vec<u8>> = OnceLock::new();

    /// Run the job: upstream's `main` from `setpgid` on. Returns the status to
    /// exit with -- in the child as well, when its `execvp` failed.
    pub fn run(job: &Job) -> u8 {
        let Some(program) = job.argv.first() else {
            // `scan` refuses a command line without one.
            return EXIT_CANCELED;
        };
        let name = os_bytes(program.as_os_str()).into_owned();
        // `set` fails only if already set, and this runs once.
        let _ = COMMAND.set(quote(&name).into_bytes());
        TERM_SIGNAL.store(job.signal, SeqCst);
        KILL_AFTER.store(job.kill_after.to_bits(), SeqCst);
        FOREGROUND.store(job.foreground, SeqCst);
        VERBOSE.store(job.verbose, SeqCst);

        // The argument vector, built before the fork so that the child goes
        // to `execvp` without allocating. A word of argv cannot hold a NUL, so
        // `CString::new` cannot fail on one; if it somehow did, the command is
        // one that cannot be run.
        let Ok(words) = job
            .argv
            .iter()
            .map(|word| CString::new(os_bytes(word.as_os_str()).into_owned()))
            .collect::<Result<Vec<CString>, _>>()
        else {
            say(&format!(
                "failed to run command {}: {}",
                quote(&name),
                strerror_of(libcall::EINVAL)
            ));
            return EXIT_CANNOT_INVOKE;
        };
        let argv: Vec<&CStr> = words.iter().map(CString::as_c_str).collect();
        let mut slots = vec![std::ptr::null::<u8>(); argv.len().saturating_add(1)];

        // "Ensure we're in our own group so all subprocesses can be killed."
        // Unchecked, as upstream's: it fails only for a session leader, which
        // stays in the group it leads -- one the group kill reaches all the
        // same.
        if !job.foreground {
            let _ = process::set_process_group(0, 0);
        }

        // "Setup handlers before fork() so that we handle any signals caused
        // by child, without races."
        install_cleanup(job.signal);
        // "Don't stop if background child needs tty." Unchecked, as upstream's:
        // both are signals and may be ignored.
        let _ = libcall::ignore_signal(SIGTTIN);
        let _ = libcall::ignore_signal(SIGTTOU);
        install_sigchld();

        // SAFETY: `timeout` has one thread, so the child holds no lock another
        // thread took; and the child allocates nothing before `execvp` anyway.
        match unsafe { process::fork() } {
            Err(e) => {
                say(&format!("fork system call failed: {}", strerror_of(e)));
                EXIT_CANCELED
            }
            Ok(Forked::Child) => child(&name, &argv, &mut slots),
            Ok(Forked::Parent(pid)) => {
                MONITORED_PID.store(pid, SeqCst);
                monitor(pid, job)
            }
        }
    }

    /// The child: become the command, or say why not.
    fn child(name: &[u8], argv: &[&CStr], slots: &mut [*const u8]) -> u8 {
        // "exec doesn't reset SIG_IGN -> SIG_DFL." Unchecked, as upstream's.
        let _ = signal::set_default(SIGTTIN);
        let _ = signal::set_default(SIGTTOU);
        // The runtime ignored SIGPIPE before `main`, and an ignored signal
        // stays ignored across `exec`. Upstream never touches it, so its
        // command gets whatever `timeout` was given -- which is what this
        // restores. Unchecked: SIGPIPE is a signal and may be reset.
        if !stdfd::sigpipe_ignored_at_startup() {
            let _ = signal::set_default(SIGPIPE);
        }
        let errno = process::execvp(argv, slots);
        // "exit like sh, env, nohup, ..."
        let status = if errno == libcall::ENOENT {
            EXIT_ENOENT
        } else {
            EXIT_CANNOT_INVOKE
        };
        say(&format!(
            "failed to run command {}: {}",
            quote(name),
            strerror_of(errno)
        ));
        status
    }

    /// The parent: time the command, wait for it, and turn how it ended into
    /// a status.
    fn monitor(pid: i32, job: &Job) -> u8 {
        // "We configure timers so that SIGALRM is sent on expiry. Therefore
        // ensure we don't inherit a mask blocking SIGALRM."
        unblock_signal(SIGALRM);
        settimeout(job.duration, true);

        // "Ensure we don't cleanup() after waitpid() reaps the child, to avoid
        // sending signals to a possibly different process."
        let cleanup_set = block_cleanup_and_chld(job.signal);
        let waited = loop {
            match process::wait_nohang(pid) {
                // "Wait with cleanup signals unblocked." Its one return is a
                // handler having run, which is the cue to look again.
                Ok(None) => {
                    let _ = signal::suspend(&cleanup_set);
                }
                other => break other,
            }
        };

        // Blocked from here on, so this is the final word.
        let timed_out = TIMED_OUT.load(SeqCst);
        let mut preserve_status = job.preserve_status;
        let status = match waited {
            Ok(Some(ended)) => {
                if let Some(code) = ended.exit_code() {
                    code
                } else if let Some(sig) = ended.signal() {
                    if ended.core_dumped() {
                        say("the monitored command dumped core");
                    }
                    if !timed_out && disable_core_dumps() {
                        // "exit with the signal flag set." Unchecked: if the
                        // signal does not end this process, the status below
                        // still says what it was.
                        let _ = signal::set_default(sig);
                        unblock_signal(sig);
                        let _ = signal::raise(sig);
                    }
                    // "Allow users to distinguish if command was forcibly
                    // killed. Needed with --foreground where we don't send
                    // SIGKILL to the timeout process itself."
                    if timed_out && sig == SIGKILL {
                        preserve_status = true;
                    }
                    // "what sh returns for signaled processes."
                    sig.saturating_add(128)
                } else {
                    // "shouldn't happen."
                    say(&format!("unknown status from command ({})", ended.raw()));
                    1
                }
            }
            Err(e) => {
                // "shouldn't happen."
                say(&format!("error waiting for command: {}", strerror_of(e)));
                i32::from(EXIT_CANCELED)
            }
            // The loop above leaves only with a status or an error.
            Ok(None) => i32::from(EXIT_CANCELED),
        };
        if timed_out && !preserve_status {
            return EXIT_TIMEDOUT;
        }
        // An exit status is 0..=255 and 128 plus a signal at most 192.
        u8::try_from(status).unwrap_or(EXIT_CANCELED)
    }

    /// Upstream's `install_cleanup`: the handler for the timer, and for the
    /// signals `timeout` passes on to its command.
    fn install_cleanup(sigterm: i32) {
        // Each unchecked, as upstream's. The one that can be refused is
        // `sigterm` itself when it is `KILL`, `STOP` or 0, none of which can
        // be caught -- and then it is simply sent, not caught.
        for sig in [SIGALRM, SIGINT, SIGQUIT, SIGHUP, SIGTERM, sigterm] {
            let _ = signal::set_handler(sig, cleanup, true);
        }
    }

    /// Upstream's `install_sigchld`: a handler that does nothing, so that
    /// `sigsuspend` returns when the command ends.
    fn install_sigchld() {
        // Unchecked, as upstream's: SIGCHLD may always be caught.
        let _ = signal::set_handler(SIGCHLD, chld, true);
        // "We inherit the signal mask from our parent process, so ensure
        // SIGCHLD is not blocked."
        unblock_signal(SIGCHLD);
    }

    /// Upstream's `block_cleanup_and_chld`: block every signal `cleanup`
    /// handles, and SIGCHLD, and return the mask from before.
    fn block_cleanup_and_chld(sigterm: i32) -> SigSet {
        let mut set = SigSet::empty();
        for sig in [SIGALRM, SIGINT, SIGQUIT, SIGHUP, SIGTERM, sigterm, SIGCHLD] {
            // Unchecked, as upstream's: only `sigterm` can be refused, as 0.
            let _ = set.add(sig);
        }
        match signal::block(&set) {
            Ok(before) => before,
            Err(e) => {
                say(&format!("warning: sigprocmask: {}", strerror_of(e)));
                // Upstream goes on with whatever its `old_set` held, which after
                // a failed call is unspecified. Nothing blocked is the honest
                // stand-in: the wait still ends for every signal it waits for.
                SigSet::empty()
            }
        }
    }

    /// Upstream's `unblock_signal`.
    fn unblock_signal(sig: i32) {
        let mut set = SigSet::empty();
        // Unchecked, as upstream's: `sig` is always a real signal here.
        let _ = set.add(sig);
        if let Err(e) = signal::unblock(&set) {
            say(&format!("warning: sigprocmask: {}", strerror_of(e)));
        }
    }

    /// Upstream's `disable_core_dumps`, `prctl` arm.
    fn disable_core_dumps() -> bool {
        match process::disable_core_dumps() {
            Ok(()) => true,
            Err(e) => {
                say(&format!(
                    "warning: disabling core dumps failed: {}",
                    strerror_of(e)
                ));
                false
            }
        }
    }

    /// Upstream's `settimeout`, `setitimer` arm: `SIGALRM` in `duration`
    /// seconds, 0 meaning never, falling back to `alarm`'s whole seconds if
    /// the timer is refused. See the module docs for why this arm.
    ///
    /// Called from the handler too, with `warn` false -- in which case it
    /// allocates nothing.
    fn settimeout(duration: f64, warn: bool) {
        let (sec, nsec) = dtotimespec(duration);
        let (sec, usec) = to_timeval(sec, nsec);
        if let Err(e) = signal::set_alarm_timer(sec, usec) {
            if warn && e != libcall::ENOSYS {
                say(&format!("warning: setitimer: {}", strerror_of(e)));
            }
            // "fallback to single second resolution provided by alarm()." What
            // it returns is the time left on an earlier alarm: there is none.
            let _ = signal::alarm(alarm_seconds(duration));
        }
    }

    /// Upstream's `chld`: nothing, so that `sigsuspend` returns.
    extern "C" fn chld(_sig: i32) {}

    /// Upstream's `cleanup`: the timer has run out, or a signal has arrived
    /// that `timeout` passes on.
    extern "C" fn cleanup(received: i32) {
        let _errno = ErrnoGuard::save();
        let mut sig = received;
        if sig == SIGALRM {
            TIMED_OUT.store(true, SeqCst);
            sig = TERM_SIGNAL.load(SeqCst);
        }
        let pid = MONITORED_PID.load(SeqCst);
        if pid == 0 {
            // "we're the child or the child is not exec'd yet."
            process::exit_immediately(sig.saturating_add(128));
        }
        if f64::from_bits(KILL_AFTER.load(SeqCst)) > 0.0 {
            // "Start a new timeout after which we'll send SIGKILL."
            TERM_SIGNAL.store(SIGKILL, SeqCst);
            settimeout(f64::from_bits(KILL_AFTER.load(SeqCst)), false);
            // "Don't let later signals reset kill alarm."
            KILL_AFTER.store(0, SeqCst);
        }
        // "Send the signal directly to the monitored child, in case it has
        // itself become group leader, or is not running in a separate group."
        if VERBOSE.load(SeqCst) {
            announce(sig);
        }
        send_sig(pid, sig);
        // "The normal case is the job has remained in our newly created
        // process group, so send to all processes in that."
        if !FOREGROUND.load(SeqCst) {
            send_sig(0, sig);
            if sig != SIGKILL && sig != SIGCONT {
                send_sig(pid, SIGCONT);
                send_sig(0, SIGCONT);
            }
        }
    }

    /// Upstream's `send_sig`: to the command, or (for 0) to the whole group,
    /// ignoring the signal first "so we don't go into a signal loop". Each
    /// unchecked, as upstream's: a command that has already gone is the
    /// ordinary case, not an error.
    fn send_sig(pid: i32, sig: i32) {
        if pid == 0 {
            let _ = libcall::ignore_signal(sig);
            let _ = libcall::kill_own_group(sig);
        } else {
            let _ = libcall::kill(pid, sig);
        }
    }

    /// `-v`'s line, `timeout: sending signal TERM to command ‘sleep’`, from
    /// inside the handler: put together without allocating, and written
    /// with one `write(2)` when it fits the buffer -- as stdio would write it
    /// -- or a piece at a time when it does not.
    fn announce(sig: i32) {
        let name = sig2str::signame(sig);
        let mut number = [0u8; 12];
        let shown: &[u8] = match &name {
            Some(name) => name.as_bytes(),
            // Upstream's `snprintf (signame, sizeof signame, "%d", sig)`, for
            // a number with no name. Every signal sent here has one.
            None => decimal(sig, &mut number),
        };
        let command = COMMAND.get().map_or(&[][..], Vec::as_slice);
        let parts: [&[u8]; 6] = [
            TIMEOUT_NAME.as_bytes(),
            b": sending signal ",
            shown,
            b" to command ",
            command,
            b"\n",
        ];
        let mut line = [0u8; 512];
        let mut len = 0usize;
        for part in parts {
            let Some(room) = len
                .checked_add(part.len())
                .and_then(|end| line.get_mut(len..end))
            else {
                for part in parts {
                    stdfd::diag_bytes_in_handler(part);
                }
                return;
            };
            room.copy_from_slice(part);
            len = len.saturating_add(part.len());
        }
        stdfd::diag_bytes_in_handler(line.get(..len).unwrap_or_default());
    }

    /// `n` in decimal, into `buf`, without allocating.
    fn decimal(n: i32, buf: &mut [u8; 12]) -> &[u8] {
        let mut at = buf.len();
        let mut rest = n.unsigned_abs();
        loop {
            at = at.saturating_sub(1);
            if let (Some(slot), Ok(digit)) = (buf.get_mut(at), u8::try_from(rest % 10)) {
                *slot = b'0'.saturating_add(digit);
            }
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        if n < 0 {
            at = at.saturating_sub(1);
            if let Some(slot) = buf.get_mut(at) {
                *slot = b'-';
            }
        }
        buf.get(at..).unwrap_or_default()
    }

    #[cfg(test)]
    mod tests {
        use super::decimal;

        #[test]
        fn decimal_writes_what_printf_writes() {
            let mut buf = [0u8; 12];
            assert_eq!(decimal(0, &mut buf), b"0");
            assert_eq!(decimal(64, &mut buf), b"64");
            assert_eq!(decimal(-7, &mut buf), b"-7");
            assert_eq!(decimal(i32::MIN, &mut buf), b"-2147483648");
            assert_eq!(decimal(i32::MAX, &mut buf), b"2147483647");
        }
    }
}

/// Off Unix there is no `fork`, no signal and no timer to run a command
/// under, so the command is refused as one that cannot be run -- the shape
/// `nice` gives the same refusal.
#[cfg(not(unix))]
mod imp {
    use super::{EXIT_CANNOT_INVOKE, Job, TIMEOUT_NAME};

    pub fn run(job: &Job) -> u8 {
        let name = job
            .argv
            .first()
            .map(coreutils::quote::quote_os)
            .unwrap_or_default();
        let why =
            coreutils::errmsg::strerror(&std::io::Error::from(std::io::ErrorKind::Unsupported));
        coreutils::stdfd::diag_line(&format!(
            "{TIMEOUT_NAME}: failed to run command {name}: {why}"
        ));
        EXIT_CANNOT_INVOKE
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn job(words: &[&str]) -> Job {
        match scan(&argv(words)) {
            Ok(Request::Run(job)) => job,
            other => panic!("wanted a job, got {other:?}"),
        }
    }

    fn refused(words: &[&str]) -> String {
        match scan(&argv(words)) {
            Err(Refusal::Said(e)) => e.message(),
            Err(Refusal::Usage) => "<usage>".to_string(),
            other => panic!("wanted a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_duration_and_a_command_are_a_job_with_the_defaults() {
        let j = job(&["5", "sleep", "10"]);
        assert_eq!(j.duration, 5.0);
        assert_eq!(j.kill_after, 0.0);
        assert_eq!(j.signal, libcall::SIGTERM);
        assert!(!j.foreground && !j.preserve_status && !j.verbose);
        assert_eq!(j.argv, argv(&["sleep", "10"]));
    }

    #[test]
    fn every_option_lands() {
        let j = job(&[
            "-v",
            "-k",
            "2m",
            "-s",
            "hup",
            "--foreground",
            "--preserve-status",
            "1.5",
            "cmd",
        ]);
        assert!(j.verbose && j.foreground && j.preserve_status);
        assert_eq!(j.kill_after, 120.0);
        assert_eq!(j.signal, 1);
        assert_eq!(j.duration, 1.5);
        let j = job(&["--kill-after=3", "--signal=KILL", "--verbose", "1", "x"]);
        assert_eq!((j.kill_after, j.signal, j.verbose), (3.0, 9, true));
        // Abbreviations resolve as glibc resolves them against this table.
        let j = job(&["--k=1", "--s=9", "--f", "--p", "1", "x"]);
        assert_eq!((j.kill_after, j.signal), (1.0, 9));
        assert!(j.foreground && j.preserve_status);
    }

    /// The `+`: the first operand ends the options, so what follows is the
    /// command's -- measured, `timeout 5 -v true` runs `-v`.
    #[test]
    fn the_first_operand_ends_the_options() {
        let j = job(&["5", "-v", "true"]);
        assert!(!j.verbose);
        assert_eq!(j.argv, argv(&["-v", "true"]));
        let j = job(&["1", "true", "--help"]);
        assert_eq!(j.argv, argv(&["true", "--help"]));
        let j = job(&["--", "1", "true"]);
        assert_eq!(j.argv, argv(&["true"]));
        let j = job(&["-v", "--", "-0", "true"]);
        assert_eq!(j.duration, 0.0);
    }

    /// Fewer than two operands is the bare referral, whatever they are --
    /// counted before the duration is read. Measured.
    #[test]
    fn too_few_operands_is_the_bare_referral() {
        assert_eq!(refused(&[]), "<usage>");
        assert_eq!(refused(&["5"]), "<usage>");
        assert_eq!(refused(&["x"]), "<usage>");
        assert_eq!(refused(&["-v"]), "<usage>");
        assert_eq!(refused(&["--"]), "<usage>");
        assert_eq!(refused(&["--", "5"]), "<usage>");
    }

    #[test]
    fn a_bad_interval_or_signal_is_refused_with_the_referral() {
        let try_help = "\nTry 'timeout --help' for more information.";
        assert_eq!(
            refused(&["x", "true"]),
            format!("invalid time interval \u{2018}x\u{2019}{try_help}")
        );
        assert_eq!(
            refused(&["-k", "1x", "1", "true"]),
            format!("invalid time interval \u{2018}1x\u{2019}{try_help}")
        );
        assert_eq!(
            refused(&["-s", "FOO", "1", "true"]),
            format!("\u{2018}FOO\u{2019}: invalid signal{try_help}")
        );
        assert_eq!(
            refused(&["--", "-1", "true"]),
            format!("invalid time interval \u{2018}-1\u{2019}{try_help}")
        );
    }

    /// Options act as they are met: a bad `-s` before `--help` is the error,
    /// and `--help` before a bad `-s` is the help. Both measured.
    #[test]
    fn options_are_acted_on_in_order() {
        assert_eq!(scan(&argv(&["--help", "-s", "BAD"])), Ok(Request::Help));
        assert!(refused(&["-s", "BAD", "--help"]).contains("invalid signal"));
        assert!(refused(&["-k", "BAD", "--version"]).contains("invalid time interval"));
        assert_eq!(scan(&argv(&["--version", "x"])), Ok(Request::Version));
    }

    #[test]
    fn getopt_s_own_complaints() {
        assert_eq!(
            refused(&["-x", "1", "true"]),
            "invalid option -- 'x'\nTry 'timeout --help' for more information."
        );
        assert_eq!(
            refused(&["-9", "1", "true"]),
            "invalid option -- '9'\nTry 'timeout --help' for more information."
        );
        assert_eq!(
            refused(&["-k"]),
            "option requires an argument -- 'k'\nTry 'timeout --help' for more information."
        );
        assert!(refused(&["--v", "1", "true"]).starts_with("option '--v' is ambiguous"));
        assert!(refused(&["--bogus", "1", "true"]).starts_with("unrecognized option '--bogus'"));
    }

    #[test]
    fn the_help_is_upstream_s_text() {
        assert!(HELP.starts_with(
            "Usage: timeout [OPTION] DURATION COMMAND [ARG]...\n  or:  timeout [OPTION]\n"
        ));
        assert!(HELP.ends_with("  -    the exit status of COMMAND otherwise\n"));
    }

    /// `dtotimespec` rounds the fraction up and clamps the ends.
    #[test]
    fn a_duration_becomes_a_timespec_as_gnulib_s_does() {
        assert_eq!(dtotimespec(0.0), (0, 0));
        assert_eq!(dtotimespec(-0.0), (0, 0));
        assert_eq!(dtotimespec(1.5), (1, 500_000_000));
        // 1e-10 of a second is a tenth of a nanosecond: up, not down to none.
        assert_eq!(dtotimespec(1e-10), (0, 1));
        // 1e9 times the double nearest 0.1 rounds to exactly 1e8: no carry.
        assert_eq!(dtotimespec(0.1), (0, 100_000_000));
        assert_eq!(dtotimespec(f64::INFINITY), (i64::MAX, 999_999_999));
        assert_eq!(dtotimespec(1e300), (i64::MAX, 999_999_999));
        assert_eq!(dtotimespec(f64::NAN), (i64::MIN, 0));
        assert_eq!(dtotimespec(f64::NEG_INFINITY), (i64::MIN, 0));
    }

    #[test]
    fn a_timespec_becomes_a_timeval_rounding_up() {
        assert_eq!(to_timeval(0, 0), (0, 0));
        assert_eq!(to_timeval(0, 1), (0, 1));
        assert_eq!(to_timeval(1, 999_999_999), (2, 0), "the carry");
        assert_eq!(to_timeval(i64::MAX, 999_999_999), (i64::MAX, 999_999));
        assert_eq!(to_timeval(3, 500_000_000), (3, 500_000));
    }

    #[test]
    fn alarm_gets_whole_seconds_rounded_up() {
        assert_eq!(alarm_seconds(0.0), 0);
        assert_eq!(alarm_seconds(0.1), 1);
        assert_eq!(alarm_seconds(2.0), 2);
        assert_eq!(alarm_seconds(2.5), 3);
        assert_eq!(alarm_seconds(f64::INFINITY), u32::MAX);
        assert_eq!(alarm_seconds(f64::from(u32::MAX)), u32::MAX);
    }
}
