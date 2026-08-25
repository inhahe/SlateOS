//! `nohup` — run a command immune to hangups.
//!
//! A port of GNU coreutils 9.4's `src/nohup.c`, measured against the real
//! binary rather than recalled. The shipped version was 130 lines that spawned
//! a child and waited for it, and almost every decision in it was wrong:
//!
//! 1. **It redirected the child's stdout to `nohup.out` unconditionally**,
//!    admitting in a comment that "we can't easily check isatty". So
//!    `nohup cmd > out.txt` put nothing in `out.txt` — the output went to
//!    `nohup.out` instead, which is the exact opposite of what `--help` tells
//!    people to do to control where output lands. `isatty` has been available
//!    in `posix/src/ioctl.rs` the whole time, and `tty`, `test` and `find` in
//!    this same crate already call it.
//! 2. **It never ignored `SIGHUP`** — the one thing the program is named for.
//!    There was no signal handling of any kind, so a hangup killed the child
//!    exactly as if `nohup` had not been used.
//! 3. **It did not `exec`.** Spawning and waiting leaves `nohup` sitting in the
//!    process tree as the child's parent for the child's whole lifetime, so the
//!    thing you ran in order to *survive* your session keeps a parent that will
//!    not. Upstream replaces itself.
//! 4. **`argv` was `Vec<String>`**, so a command or argument holding a byte
//!    that is not valid UTF-8 — legal here, `design.txt` allows every byte but
//!    `/` and NUL — panicked before anything ran. This is the `argv-utf8`
//!    finding that brought the program up for conversion.
//! 5. **No `--help`, no `--version`, no option rejection**, and the exit
//!    statuses were guesses: `126` when `wait` failed, `127` for every spawn
//!    failure. Upstream distinguishes *not found* (`127`) from *found but not
//!    runnable* (`126`), and uses `125` for nohup's own failures — the
//!    three-way split exists so a script can tell "your command is missing"
//!    from "nohup could not set things up".
//!
//! # What the redirections actually are
//!
//! Each of the three standard descriptors is treated independently, and only
//! if it *is a terminal*. That conditionality is the whole design: `nohup`
//! detaches you from the terminal, and a descriptor already pointing somewhere
//! else needs no help.
//!
//! | descriptor | if it is a terminal | otherwise |
//! |---|---|---|
//! | stdin  | reopened on `/dev/null`, **write-only** | untouched |
//! | stdout | appended to `./nohup.out`, else `$HOME/nohup.out`, mode `0600` | untouched |
//! | stderr | `dup2`'d onto stdout | untouched |
//!
//! Two details of that table are not what they look like, and both were
//! measured rather than assumed:
//!
//! *`/dev/null` is opened for **writing**, so the child's *reads* fail.* The
//! `--help` text says "redirect it from an unreadable file", and it means it:
//! measured, `nohup cat` under a terminal produces `cat: -: Bad file
//! descriptor` and status 1, not the silent end-of-file that a read-only
//! `/dev/null` would give. A command detached from its terminal that quietly
//! reads end-of-file looks like it succeeded on empty input; one that gets an
//! error can say so.
//!
//! *`nohup.out` is also opened when stdout is **closed** and stderr is a
//! terminal.* Not for stdout's sake — for stderr's, which is about to be
//! `dup2`'d *from* stdout and would have nothing to point at. Measured:
//! `nohup true >&-` from a terminal creates `nohup.out`, while
//! `nohup true >&- 2>err` does not.
//!
//! # The saved stderr, which is not an optimisation
//!
//! `nohup`'s own diagnostics have to reach the *terminal*, not the file it just
//! redirected onto. Measured: with stdout a terminal, `nohup /nope/nope` prints
//! `nohup: failed to run command '/nope/nope': No such file or directory` on
//! the terminal and leaves `nohup.out` **empty**. If the message went through
//! the redirected descriptor it would land in `nohup.out`, where the person who
//! just typed the command is not looking — and the exit status alone does not
//! say which of the two failures it was. So the original stderr is duplicated
//! to a high descriptor (close-on-exec, so the child never sees it) *before*
//! any redirection, and every diagnostic after that point goes there.
//!
//! # A diagnostic that cannot be delivered is itself a failure
//!
//! The messages above are not decoration: they are the only record that a
//! command's output went somewhere other than where it was typed. So if one
//! cannot be written, `nohup` refuses to run the command at all and exits 125.
//! Measured across the whole grid — `nohup true 2>&-` and `nohup true
//! 2>/dev/full` from a terminal are both 125, and `nohup.out` is left behind by
//! the first because the file is opened before the message is attempted; with
//! nothing to report (`nohup true </dev/null >out 2>&-`) the same closed stderr
//! is harmless and the status is the command's.
//!
//! The `failed to run command` diagnostic is deliberately *not* held to that
//! rule, because upstream does not hold it to that rule: measured,
//! `nohup /nope/nope </dev/null >out 2>&-` still exits 127. Upstream only
//! notices that message failing at exit, through gnulib's `close_stdout`, which
//! forgives `EBADF` and nothing else — so `2>/dev/full` turns the same case
//! into 125. Both halves are reproduced here; see `run`.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::quoteaf_os;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// `nohup`'s own failures are 125 — distinct from the child's statuses and from
/// 126/127, so "nohup could not set up" is never confused with anything the
/// command did. Measured: `nohup` with no operand, `nohup -x` and a `nohup.out`
/// that cannot be opened are all 125.
const NOHUP_FAILURE: i32 = 125;

// The three items below are used by `imp` on unix and by the tests everywhere.
// The development host is Windows, which builds neither, and the target
// (`toolchain/x86_64-slateos.json`) is `"target-family": ["unix"]`, so this
// only silences the host — as `tty.rs` does for the same reason.
/// The command was found but could not be run.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const EXIT_CANNOT_INVOKE: u8 = 126;

/// The command was not found.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const EXIT_ENOENT: u8 = 127;

const NOHUP: Program = Program::new("nohup", NOHUP_FAILURE);

/// The leading `+` is load-bearing: it stops option parsing at the first
/// operand, so everything after the command name belongs to the *command*.
/// Without it `nohup printf '%s|' a -n` would eat `-n` as nohup's own option
/// and reject it. Measured against the real binary, which passes `-n` and even
/// `--version` straight through to `printf`.
const SHORT_OPTIONS: &str = "+";

/// Upstream calls `parse_gnu_standard_options_only`; these are the two it
/// registers, in order. `scripts/getopt-ambiguity-check.py` compares this table
/// against the real binary's, so `--h` and `--vers` resolve exactly as GNU's do.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    /// The command and its arguments, kept as `OsString` so the bytes reach
    /// `exec` exactly as they were typed.
    Run(Vec<OsString>),
}

fn main() -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    // `NOHUP_FAILURE` rather than 1, because `nohup` is one of the five that
    // call gnulib's `initialize_exit_failure (EXIT_CANCELED)`.
    //
    // The `Diagnostics` inside `imp::run` keeps its *own* record, because after
    // the redirection its messages no longer go to descriptor 2 at all. This
    // catches the ones that do: the usage errors, which happen first.
    stdfd::close_stderr(run_main(), u8::try_from(NOHUP_FAILURE).unwrap_or(1))
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main() -> ExitCode {
    // First, before anything reads or writes a standard descriptor: this is
    // what makes a closed one look closed rather than like the `/dev/null` the
    // runtime substituted. `--help >&-` needs it as much as the exec path does.
    stdfd::restore();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        // `report` prints the sentence *and* the `Try 'nohup --help'` referral,
        // which `Error` carries as its own field — see `getopt::Error`.
        //
        // Decided before the stream exists, because upstream's
        // `usage (EXIT_CANCELED)` reaches `atexit (close_stdout)` with nothing
        // buffered: measured, `nohup >/dev/full` prints the missing-operand
        // pair and no write error after it.
        Err(e) => {
            NOHUP.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `run` is the exec path: it either replaces this process or reports why it
    // could not, and either way nothing of ours is buffered on descriptor 1 by
    // then. Only `--help` and `--version` write, and they fail like any other
    // write: measured, `nohup --help >&-` is `nohup: write error: Bad file
    // descriptor` and exits 125 — `nohup`'s `exit_failure`, not 1.
    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"nohup (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Run(argv) => return run(&argv),
    };
    stdfd::close_stdout_with(
        "nohup",
        out,
        earned,
        u8::try_from(NOHUP_FAILURE).unwrap_or(1),
    )
}

/// Read the command line.
///
/// Unlike most utilities here, parsing **stops at the first operand** — see
/// [`SHORT_OPTIONS`]. `--` also stops it, and is consumed.
///
/// # Errors
///
/// A getopt diagnostic for an unknown option or an argument given to one that
/// takes none, or a `missing operand` error when no command is named. Both
/// carry status 125.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut argv: Vec<OsString> = Vec::new();
    for item in NOHUP.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => argv.push(word.clone()),
            // Every short option is rejected by `parse` (the table is empty
            // but for `+`), and both long options are handled above.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    if argv.is_empty() {
        // Not produced by `getopt`, but it is the same shape: upstream prints
        // it with `error (0, …)` followed by `usage (EXIT_CANCELED)`, so it
        // carries the referral and nohup's own status. Measured: `nohup` and
        // `nohup --` both give exactly `nohup: missing operand` and the
        // `Try 'nohup --help' for more information.` line, status 125.
        return Err(getopt::Error {
            sentence: "missing operand".to_string(),
            referral: Some("nohup"),
            status: NOHUP_FAILURE,
        });
    }
    Ok(Request::Run(argv))
}

/// GNU's `--help`, minus the project's `Report bugs to:` block and the note
/// about shell built-ins, as every converted utility here omits them.
fn help_text() -> String {
    "\
Usage: nohup COMMAND [ARG]...
  or:  nohup OPTION
Run COMMAND, ignoring hangup signals.

      --help        display this help and exit
      --version     output version information and exit

If standard input is a terminal, redirect it from an unreadable file.
If standard output is a terminal, append output to 'nohup.out' if possible,
'$HOME/nohup.out' otherwise.
If standard error is a terminal, redirect it to standard output.
To save output to FILE, use 'nohup COMMAND > FILE'.
"
    .to_string()
}

// ------------------------------------------------------------- diagnostics ---

/// The bare `ignoring input`, which is announced **only** when nothing else is
/// being announced.
///
/// This is the one wording the first draft of this port got wrong, and the
/// mistake is worth recording because it is invisible from the source: it
/// looks natural to say "ignoring input" whenever stdin was redirected and to
/// append the rest. Upstream does not. Measured, with stdin a terminal:
///
/// | stdout | stderr | what is said |
/// |---|---|---|
/// | file | file | `ignoring input` |
/// | file | tty  | `ignoring input and redirecting stderr to stdout` — *only* |
/// | tty  | file | `ignoring input and appending output to 'nohup.out'` |
///
/// So the fact is folded into whichever other message is already being
/// printed, and stands alone only when there is no other message.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn input_message(
    ignoring_input: bool,
    redirecting_stdout: bool,
    redirecting_stderr: bool,
) -> Option<&'static str> {
    (ignoring_input && !redirecting_stdout && !redirecting_stderr).then_some("ignoring input")
}

/// The message printed when stdout was redirected to `target`.
///
/// The name is rendered with `quoteaf_os`, which quotes *always*, not with
/// `quotef_os`, which quotes only names that would otherwise be ambiguous.
/// Upstream uses `quoteaf` here and the difference shows in the ordinary case:
/// measured, GNU says `appending output to 'nohup.out'` — with the quotes —
/// where `quotef` gives `appending output to nohup.out`. The first version of
/// this port used `quotef_os` and the unit test caught it; a test written only
/// with a name containing a space would not have.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn stdout_message(ignoring_input: bool, target: &std::path::Path) -> String {
    if ignoring_input {
        format!(
            "ignoring input and appending output to {}",
            quoteaf_os(target)
        )
    } else {
        format!("appending output to {}", quoteaf_os(target))
    }
}

/// The message printed when stderr was redirected.
///
/// Suppressed entirely when stdout was *also* redirected, because
/// [`redirect_message`] has already said so and upstream does not repeat
/// itself. Measured: with both a terminal, the only line is
/// `ignoring input and appending output to 'nohup.out'`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn stderr_message(ignoring_input: bool, redirecting_stdout: bool) -> Option<String> {
    if redirecting_stdout {
        return None;
    }
    Some(if ignoring_input {
        "ignoring input and redirecting stderr to stdout".to_string()
    } else {
        "redirecting stderr to stdout".to_string()
    })
}

/// 127 if the command was not found, 126 if it was found but could not run.
///
/// The split is upstream's and is what lets a script distinguish a typo in the
/// command name from a file that is present but not executable.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn exec_failure_status(kind: std::io::ErrorKind) -> u8 {
    if kind == std::io::ErrorKind::NotFound {
        EXIT_ENOENT
    } else {
        EXIT_CANNOT_INVOKE
    }
}

// ------------------------------------------------------------------- unix ----

#[cfg(target_os = "linux")]
mod imp {
    use super::{
        NOHUP_FAILURE, exec_failure_status, input_message, stderr_message, stdout_message,
    };
    use coreutils::errmsg::strerror;
    use coreutils::quote::quoteaf_os;
    use coreutils::stdfd;
    use std::ffi::OsString;
    use std::fs::{File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};

    unsafe extern "C" {
        fn isatty(fd: i32) -> i32;
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn close(fd: i32) -> i32;
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn signal(signum: i32, handler: usize) -> usize;
        fn umask(mask: u32) -> u32;
    }

    const F_DUPFD_CLOEXEC: i32 = 1030;
    const SIGHUP: i32 = 1;
    const SIG_IGN: usize = 1;
    const EBADF: i32 = 9;

    // ------------------------------------- the descriptors we were handed ----

    // Record which of the standard descriptors were closed when the process
    // began, before Rust's start-up reopens them on `/dev/null`; see
    // `coreutils::stdfd`, which exists because of this program. The
    // substitution is destructive here in the program's own subject matter:
    //
    // * `nohup cmd >&-` from a terminal must create `nohup.out`, because
    //   stderr is about to be pointed at descriptor 1 and would otherwise have
    //   nothing to point at. With the descriptor silently replaced by
    //   `/dev/null`, we would instead redirect the command's stderr *into*
    //   `/dev/null` — losing output that GNU saves to a file.
    // * `nohup cmd 2>&-` must fail with 125, because the message saying where
    //   the output went cannot be delivered. With the descriptor replaced, the
    //   message is "delivered" to `/dev/null` and the command runs with nobody
    //   told anything.
    // * The command itself must inherit the descriptors it was invoked with. A
    //   command run with stdout closed gets `EBADF` on write under GNU; under
    //   the substitution it would silently discard its output instead.
    coreutils::guard_std_fds!();

    /// The lowest descriptor the saved stderr may take: above all three
    /// standard ones, so saving it can never collide with a descriptor that is
    /// about to be redirected.
    const SAVED_STDERR_MIN: i32 = 3;

    /// The mode `nohup.out` is created with, and the `umask` that forces it.
    ///
    /// `0600`, because the output of a command you detached from your terminal
    /// is not something to hand to the rest of the machine by default, and
    /// because the mode is applied only at *creation* — measured, an existing
    /// `nohup.out` keeps whatever mode it already had, so a file that starts
    /// out world-readable stays that way for every later run.
    ///
    /// The mask is applied around the open and restored afterwards, which is
    /// what makes the mode exact rather than merely requested: an ordinary
    /// `open(…, 0600)` is still filtered through the inherited `umask`, so
    /// under `umask 0200` it would yield `0400` — a `nohup.out` its own owner
    /// cannot append to. Measured: GNU produces `0600` under `umask 000`,
    /// `077`, `0200` and `0466` alike.
    const NOHUP_OUT_MODE: u32 = 0o600;
    const NOHUP_OUT_UMASK: u32 = 0o777 & !NOHUP_OUT_MODE;

    fn is_tty(fd: i32) -> bool {
        // SAFETY: `isatty` is a pure query on a descriptor number and has no
        // effect on the process whichever number is passed.
        unsafe { isatty(fd) == 1 }
    }

    /// Whether `fd` is a terminal, and — when it is not — whether that is
    /// because it is not open at all.
    ///
    /// The second half matters only for stdout, and only because stderr may be
    /// about to be pointed at it; see the module docs.
    fn tty_or_closed(fd: i32) -> (bool, bool) {
        if is_tty(fd) {
            return (true, false);
        }
        let closed = std::io::Error::last_os_error().raw_os_error() == Some(EBADF);
        (false, closed)
    }

    /// Where `nohup`'s own diagnostics go once stderr may have been redirected.
    ///
    /// Holds a duplicate of the original stderr when one could be made, and
    /// falls back to the live stderr otherwise — a lost message is worse than a
    /// message in the wrong file. Records the first write failure, because a
    /// diagnostic that did not arrive changes the exit status; see `run`.
    struct Diagnostics {
        to: Option<File>,
        failure: Option<std::io::Error>,
    }

    impl Diagnostics {
        /// Duplicate stderr to a descriptor at or above [`SAVED_STDERR_MIN`],
        /// close-on-exec so the child never inherits it.
        fn save() -> Self {
            // SAFETY: `F_DUPFD_CLOEXEC` only allocates a new descriptor for an
            // existing one; it does not read or write through either.
            let fd = unsafe { fcntl(2, F_DUPFD_CLOEXEC, SAVED_STDERR_MIN) };
            let to = if fd < 0 {
                None
            } else {
                // SAFETY: `fcntl` returned a fresh descriptor that nothing else
                // owns, so `File` may take it and will close it exactly once.
                Some(unsafe { File::from_raw_fd(fd) })
            };
            Self { to, failure: None }
        }

        fn say(&mut self, message: &str) {
            let line = format!("nohup: {message}\n");
            // The saved duplicate when there is one, and the live descriptor 2
            // otherwise — which, when stderr was closed, is a closed
            // descriptor, and `stdfd::write_all` says so rather than
            // pretending. `std::io::stderr().write_all` would not: its `Write`
            // impl passes the result through the standard library's
            // `handle_ebadf`, which turns `EBADF` into `Ok(buf.len())`.
            // Measured, that was the difference between exiting 125 and
            // exiting 0 for `nohup true 2>&-`, because "the diagnostic could
            // not be delivered" is precisely the condition `run` tests before
            // letting the command start.
            let fd = self.to.as_ref().map_or(2, AsRawFd::as_raw_fd);
            let result = stdfd::write_all(fd, line.as_bytes());
            if let Err(e) = result
                && self.failure.is_none()
            {
                self.failure = Some(e);
            }
        }

        /// Whether any message so far failed to reach its descriptor.
        fn undelivered(&self) -> bool {
            self.failure.is_some()
        }

        /// Whether a message failed for a reason other than the descriptor
        /// being closed. This distinction is not ours; see the module docs on
        /// `failed to run command` and gnulib's `close_stdout`.
        fn undelivered_other_than_closed(&self) -> bool {
            self.failure
                .as_ref()
                .is_some_and(|e| e.raw_os_error() != Some(EBADF))
        }
    }

    fn nohup_failed() -> ExitCode {
        ExitCode::from(u8::try_from(NOHUP_FAILURE).unwrap_or(1))
    }

    /// Point `fd` at whatever `file` refers to, consuming `file`.
    ///
    /// This is gnulib's `fd_reopen` in the shape Rust makes natural: open
    /// first, then move the descriptor into place. Doing it in that order —
    /// rather than closing `fd` and relying on the open to land on the lowest
    /// free number — is what makes it correct when some *lower* descriptor is
    /// also closed, e.g. `nohup cmd <&-` with stdout a terminal.
    ///
    /// The `raw == fd` case is not a micro-optimisation but a correctness
    /// requirement, and it is reachable precisely because
    /// [`stdfd::restore`] runs first: with descriptor 1 closed, the
    /// `open` of `nohup.out` lands *on* descriptor 1, and a `dup2(1, 1)`
    /// followed by dropping the `File` would close the descriptor that was just
    /// put in place — leaving the command with no stdout at all.
    fn redirect(file: File, fd: i32) -> std::io::Result<()> {
        let raw = file.into_raw_fd();
        if raw == fd {
            return Ok(());
        }
        // SAFETY: `dup2` acts only on the descriptor table, and `raw` is a live
        // descriptor this function now owns.
        let rc = unsafe { dup2(raw, fd) };
        let failure = (rc < 0).then(std::io::Error::last_os_error);
        // SAFETY: `raw` is owned here — `into_raw_fd` gave up the `File`'s claim
        // on it — and is closed exactly once, on both paths.
        unsafe { close(raw) };
        match failure {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Open the file stdout will be appended to: `./nohup.out`, falling back to
    /// `$HOME/nohup.out`, and put it on descriptor 1.
    ///
    /// On failure, returns every `(path, error)` pair that was tried, in the
    /// order they were tried. Both are reported, because they are two different
    /// pieces of news — measured, GNU prints
    /// `failed to open 'nohup.out': Permission denied` *and*
    /// `failed to open '/home/u/nohup.out': Permission denied`. Reporting only
    /// the first would leave the reader believing `$HOME` had not been tried.
    fn open_nohup_out() -> Result<PathBuf, Vec<(PathBuf, std::io::Error)>> {
        let local = PathBuf::from("nohup.out");
        let first = match open_append(&local).and_then(|f| redirect(f, 1)) {
            Ok(()) => return Ok(local),
            Err(e) => e,
        };
        // An *empty* `HOME` is deliberately not filtered out here, though it
        // looks like it should be. Upstream joins with gnulib's
        // `file_name_concat`, which given `""` yields the bare `nohup.out` —
        // so GNU retries the same path and prints the same failure twice.
        // Measured, and `Path::join` reproduces it for the same reason: pushing
        // onto an empty path yields the pushed component alone.
        let Some(home) = std::env::var_os("HOME") else {
            return Err(vec![(local, first)]);
        };
        let in_home = Path::new(&home).join("nohup.out");
        match open_append(&in_home).and_then(|f| redirect(f, 1)) {
            Ok(()) => Ok(in_home),
            Err(second) => Err(vec![(local, first), (in_home, second)]),
        }
    }

    fn open_append(path: &Path) -> std::io::Result<File> {
        // SAFETY: `umask` only reads and replaces a per-process value.
        let saved = unsafe { umask(NOHUP_OUT_UMASK) };
        let opened = OpenOptions::new().create(true).append(true).open(path);
        // SAFETY: as above; restoring the value the process arrived with, so
        // that the mask the command inherits across `exec` is unchanged.
        unsafe { umask(saved) };
        opened
    }

    pub fn run(argv: &[OsString]) -> ExitCode {
        // `stdfd::restore` has already run, at the top of `run_main` — which is
        // before `Diagnostics::save` below, so a stderr the caller closed is
        // seen as closed rather than as the runtime's `/dev/null`, and before
        // `tty_or_closed(1)`, which is asked the same question about output.
        let mut diag = Diagnostics::save();

        let ignoring_input = is_tty(0);
        let (redirecting_stdout, stdout_is_closed) = tty_or_closed(1);
        let redirecting_stderr = is_tty(2);

        if ignoring_input {
            // Write-only on purpose, so the child's reads fail rather than
            // returning end-of-file. See the module docs.
            match OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .and_then(|f| redirect(f, 0))
            {
                Ok(()) => {}
                Err(e) => {
                    diag.say(&format!(
                        "failed to render standard input unusable: {}",
                        strerror(&e)
                    ));
                    return nohup_failed();
                }
            }
        }

        // Also when stdout is *closed* and stderr is a terminal: the `dup2`
        // below needs something on descriptor 1 to copy.
        if redirecting_stdout || (redirecting_stderr && stdout_is_closed) {
            match open_nohup_out() {
                Ok(path) => diag.say(&stdout_message(ignoring_input, &path)),
                Err(problems) => {
                    for (path, e) in &problems {
                        diag.say(&format!(
                            "failed to open {}: {}",
                            quoteaf_os(path),
                            strerror(e)
                        ));
                    }
                    return nohup_failed();
                }
            }
        }

        if let Some(m) = input_message(ignoring_input, redirecting_stdout, redirecting_stderr) {
            diag.say(m);
        }

        if redirecting_stderr {
            if let Some(m) = stderr_message(ignoring_input, redirecting_stdout) {
                diag.say(&m);
            }
            // SAFETY: `dup2` acts only on the descriptor table; descriptor 1 is
            // open, being either the inherited stdout or the `nohup.out` just
            // put there.
            if unsafe { dup2(1, 2) } < 0 {
                let e = std::io::Error::last_os_error();
                diag.say(&format!(
                    "failed to redirect standard error: {}",
                    strerror(&e)
                ));
                return nohup_failed();
            }
        }

        // Nothing above this point may be silently lost: those messages are the
        // only record of where the command's output went. See the module docs.
        if diag.undelivered() {
            return nohup_failed();
        }

        // The program's whole purpose, and what the old version omitted
        // entirely. Set after the redirections so that a failure above is
        // still interruptible, and before `exec` so the disposition is
        // inherited — `SIG_IGN` survives `exec`, unlike a handler function.
        // SAFETY: installing `SIG_IGN` runs no user code and cannot fail in a
        // way that matters here; the return value is the previous disposition.
        unsafe { signal(SIGHUP, SIG_IGN) };

        let Some((program, rest)) = argv.split_first() else {
            // `parse_args` rejects an empty argv, so this is unreachable; it is
            // written as a value rather than a panic because a `nohup` that
            // aborts is strictly worse than one that reports and exits.
            diag.say("missing operand");
            return nohup_failed();
        };

        // `exec` replaces this process rather than spawning a child, so nothing
        // of `nohup` is left in the process tree. It only returns on failure.
        let e = Command::new(program).args(rest).exec();
        let status = exec_failure_status(e.kind());
        diag.say(&format!(
            "failed to run command {}: {}",
            quoteaf_os(program),
            strerror(&e)
        ));
        // Deliberately *not* the `undelivered` check above: upstream notices
        // this one only at exit, via gnulib's `close_stdout`, which forgives a
        // closed descriptor and nothing else. Reproduced rather than tidied,
        // because a script that reads 127 as "no such command" would be misled
        // by a 125 here. See the module docs.
        if diag.undelivered_other_than_closed() {
            return nohup_failed();
        }
        ExitCode::from(status)
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use coreutils::diag;
    use std::ffi::OsString;
    use std::process::ExitCode;

    /// The development host is Windows and has neither `isatty` on descriptor
    /// numbers nor `exec`. The unit tests below cover the pure parts on every
    /// host; this arm exists so the crate builds there.
    pub fn run(_argv: &[OsString]) -> ExitCode {
        diag!("nohup: not supported on this host");
        ExitCode::from(125)
    }
}

fn run(argv: &[OsString]) -> ExitCode {
    imp::run(argv)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    fn os(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    // --------------------------------------------------------- parse_args ---

    #[test]
    fn no_operand_is_an_error_with_nohups_own_status() {
        let e = parse_args(&os(&[])).unwrap_err();
        assert_eq!(e.sentence, "missing operand");
        assert_eq!(e.status, NOHUP_FAILURE);
        // The referral is a separate field, and it has to be present: measured,
        // GNU follows the sentence with `Try 'nohup --help' for more
        // information.`, which for a program whose only two options are `--help`
        // and `--version` is the entire discoverability of the interface.
        assert_eq!(e.referral, Some("nohup"));
        assert_eq!(
            e.message(),
            "missing operand\nTry 'nohup --help' for more information."
        );
    }

    #[test]
    fn a_bare_double_dash_still_leaves_no_command() {
        // Measured: `nohup --` is `missing operand`, status 125 — the `--` is
        // consumed as a separator rather than becoming the command.
        let e = parse_args(&os(&["--"])).unwrap_err();
        assert_eq!(e.sentence, "missing operand");
        assert_eq!(e.status, NOHUP_FAILURE);
    }

    #[test]
    fn the_command_and_its_arguments_are_kept_in_order() {
        assert_eq!(
            parse_args(&os(&["echo", "hello", "world"])).unwrap(),
            Request::Run(os(&["echo", "hello", "world"]))
        );
    }

    #[test]
    fn help_and_version_are_recognised() {
        assert_eq!(parse_args(&os(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&os(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unambiguous_abbreviations_resolve() {
        assert_eq!(parse_args(&os(&["--h"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&os(&["--vers"])).unwrap(), Request::Version);
    }

    #[test]
    fn options_after_the_command_belong_to_the_command() {
        // The `+` in SHORT_OPTIONS. Measured against the real binary:
        // `nohup printf '%s|' a -n --version` passes `-n` and `--version` to
        // `printf`. Without `+` this would be an "unrecognized option" error,
        // and the option most likely to be typed after a command is `--help`.
        assert_eq!(
            parse_args(&os(&["printf", "-n", "--version"])).unwrap(),
            Request::Run(os(&["printf", "-n", "--version"]))
        );
        assert_eq!(
            parse_args(&os(&["ls", "--help"])).unwrap(),
            Request::Run(os(&["ls", "--help"]))
        );
    }

    #[test]
    fn an_unknown_option_before_the_command_is_rejected() {
        let e = parse_args(&os(&["-x", "ls"])).unwrap_err();
        assert_eq!(e.status, NOHUP_FAILURE);
        let e = parse_args(&os(&["--nope", "ls"])).unwrap_err();
        assert_eq!(e.status, NOHUP_FAILURE);
    }

    #[test]
    fn help_takes_no_argument() {
        assert!(parse_args(&os(&["--help=x"])).is_err());
    }

    #[test]
    fn a_command_that_looks_like_an_option_survives_after_a_separator() {
        assert_eq!(
            parse_args(&os(&["--", "-x"])).unwrap(),
            Request::Run(os(&["-x"]))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_command_name_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(vec![b'n', b'a', 0xff, b'm', b'e']);
        let parsed = parse_args(std::slice::from_ref(&odd)).unwrap();
        assert_eq!(parsed, Request::Run(vec![odd]));
    }

    #[cfg(windows)]
    #[test]
    fn an_unpaired_surrogate_command_name_round_trips() {
        // The development host cannot hold the byte the target case uses, so
        // the equivalent unrepresentable-in-UTF-8 value is used instead: what
        // is being checked is that the parser never converts to `String`.
        use std::os::windows::ffi::OsStringExt;
        let odd = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        let parsed = parse_args(std::slice::from_ref(&odd)).unwrap();
        assert_eq!(parsed, Request::Run(vec![odd]));
    }

    // ---------------------------------------------------------- messages ---

    #[test]
    fn the_two_stdout_wordings_and_their_quoting() {
        let p = std::path::Path::new("nohup.out");
        assert_eq!(
            stdout_message(true, p),
            "ignoring input and appending output to 'nohup.out'"
        );
        assert_eq!(stdout_message(false, p), "appending output to 'nohup.out'");
    }

    #[test]
    fn the_home_fallback_is_named_in_full() {
        // Measured: the message carries whichever path was actually opened, so
        // the reader is not sent to a `nohup.out` that is not there.
        let p = std::path::Path::new("/home/u/nohup.out");
        assert_eq!(
            stdout_message(true, p),
            "ignoring input and appending output to '/home/u/nohup.out'"
        );
    }

    #[test]
    fn bare_ignoring_input_is_said_only_when_nothing_else_is() {
        // The table in `input_message`'s docs, which is the mistake this port
        // made first: it is *not* "say it whenever stdin was redirected".
        assert_eq!(input_message(true, false, false), Some("ignoring input"));
        assert_eq!(input_message(true, true, false), None);
        assert_eq!(input_message(true, false, true), None);
        assert_eq!(input_message(true, true, true), None);
        // And never at all when stdin was left alone.
        assert_eq!(input_message(false, false, false), None);
        assert_eq!(input_message(false, true, true), None);
    }

    #[test]
    fn the_message_set_for_every_terminal_combination() {
        // The whole grid, as one table, because the three helpers are only
        // correct *together*: each measured combination names exactly the lines
        // GNU printed for it, in order. `t` marks a terminal.
        let path = std::path::Path::new("nohup.out");
        /// `(stdin, stdout, stderr)` — each `true` meaning "that descriptor is a
        /// terminal" — paired with the lines GNU printed for it, in order.
        type Case = ((bool, bool, bool), &'static [&'static str]);
        let cases: &[Case] = &[
            (
                (true, true, true),
                &["ignoring input and appending output to 'nohup.out'"],
            ),
            (
                (true, false, true),
                &["ignoring input and redirecting stderr to stdout"],
            ),
            ((true, false, false), &["ignoring input"]),
            (
                (true, true, false),
                &["ignoring input and appending output to 'nohup.out'"],
            ),
            ((false, true, true), &["appending output to 'nohup.out'"]),
            ((false, false, true), &["redirecting stderr to stdout"]),
            ((false, false, false), &[]),
            ((false, true, false), &["appending output to 'nohup.out'"]),
        ];
        for &((stdin, stdout, stderr), want) in cases {
            let mut got: Vec<String> = Vec::new();
            if stdout {
                got.push(stdout_message(stdin, path));
            }
            if let Some(m) = input_message(stdin, stdout, stderr) {
                got.push(m.to_string());
            }
            if stderr && let Some(m) = stderr_message(stdin, stdout) {
                got.push(m);
            }
            assert_eq!(got, want, "for (in={stdin}, out={stdout}, err={stderr})");
        }
    }

    #[test]
    fn stderr_is_only_mentioned_when_stdout_was_not() {
        assert_eq!(stderr_message(true, true), None);
        assert_eq!(stderr_message(false, true), None);
        assert_eq!(
            stderr_message(true, false).unwrap(),
            "ignoring input and redirecting stderr to stdout"
        );
        assert_eq!(
            stderr_message(false, false).unwrap(),
            "redirecting stderr to stdout"
        );
    }

    // ------------------------------------------------------------ status ---

    #[test]
    fn a_missing_command_is_127_and_an_unrunnable_one_is_126() {
        use std::io::ErrorKind;
        assert_eq!(exec_failure_status(ErrorKind::NotFound), 127);
        assert_eq!(exec_failure_status(ErrorKind::PermissionDenied), 126);
        // Anything else is "found but could not be run", which is the safer
        // of the two: 127 tells a script to stop looking, and it should only
        // be said when the file really is absent.
        assert_eq!(exec_failure_status(ErrorKind::Other), 126);
    }

    #[test]
    fn nohups_own_status_is_distinct_from_the_commands() {
        assert_ne!(NOHUP_FAILURE, i32::from(EXIT_CANNOT_INVOKE));
        assert_ne!(NOHUP_FAILURE, i32::from(EXIT_ENOENT));
    }

    #[test]
    fn help_text_documents_both_options_and_where_output_goes() {
        let h = help_text();
        assert!(h.starts_with("Usage: nohup COMMAND [ARG]...\n"));
        assert!(h.contains("      --help        display this help and exit\n"));
        assert!(h.contains("      --version     output version information and exit\n"));
        // The line that the old version's unconditional redirect made a lie.
        assert!(h.contains("To save output to FILE, use 'nohup COMMAND > FILE'."));
    }
}
