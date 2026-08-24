//! `nice` — run a command with an adjusted niceness.
//!
//! A port of GNU coreutils 9.4's `src/nice.c`, measured against the real binary
//! rather than recalled. The shipped version parsed `-n` and then did this:
//!
//! ```ignore
//! let parsed = parse_args(&args);
//! let _adjustment = parsed.adjustment;
//! ```
//!
//! — the number was read, bound to a discarded name, and the command was run at
//! whatever priority the shell already had. A comment said the syscall was not
//! available yet; `posix::resource::{nice, getpriority, setpriority}` have been
//! real since phases 168–169, so the note outlived its subject and the program
//! silently did nothing for its entire purpose.
//!
//! That was the largest of eight defects, and the others are worth listing
//! because most of them are invisible from the old source:
//!
//! | What it did | What GNU does |
//! |---|---|
//! | discarded the adjustment | `nice(adjustment)` |
//! | `Command::status()` — spawn and wait | `execvp` — *become* the command |
//! | `-n abc` silently meant `-n 10` | `nice: invalid adjustment ‘abc’`, 125 |
//! | no `--help`/`--version` | both, and they win over a bad adjustment |
//! | no obsolete `-5` syntax | `nice -5 cmd` is `-n 5` |
//! | `nice` alone: `missing operand` | prints the current niceness, exits 0 |
//! | `Vec<String>` argv | bytes, all the way to `exec` |
//! | 126 for a failed wait | 126 *found but not runnable*, 127 *not found* |
//!
//! # Spawning is not running, even when the exit status matches
//!
//! `Command::status()` leaves `nice` alive as the parent of the command for as
//! long as the command runs, so `ps` shows a process that does nothing, a
//! signal sent to the job reaches the wrong process, and the command's exit
//! status arrives only after being taken apart and put back together — the old
//! code turned a signal death into 126. `execvp` replaces this process image
//! outright: after it, there is no `nice` left to get any of that wrong.
//!
//! # The obsolete `-NUM` syntax is why the scan is interleaved
//!
//! `nice -5 cmd` predates `getopt` and cannot be expressed to it: `-5` is
//! option-shaped and is not an option. GNU's answer, which this reproduces, is
//! to alternate — test the next word for the digit form, and if it is not that,
//! take **exactly one** item from `getopt` and resume from wherever that left
//! off ([`coreutils::getopt::Parser::optind`] is the other half).
//!
//! A pre-pass that stripped digit-shaped words out of argv would be simpler and
//! would be wrong: `nice -n -5 cmd` must read `-5` as the *argument* of `-n`,
//! and only a scan that lets `-n` consume the following word before the digit
//! test sees it can do that. Measured, GNU gives that command a niceness of −5.
//!
//! The digit test is `s[0] == '-' && ISDIGIT (s[1 + (s[1] == '-' || s[1] ==
//! '+')])`, and the second sign is not decoration: `--5` is −5 and `-+5` is +5,
//! because the adjustment string is everything after the *first* dash. `--` is
//! not caught by it, since the character after the second dash is the
//! terminator rather than a digit.
//!
//! # A refused niceness is a warning; an undeliverable warning is fatal
//!
//! Lowering the niceness needs privilege, and GNU does not treat being refused
//! as a reason not to run the command: `nice -n -5 cmd` as an ordinary user
//! prints `nice: cannot set niceness: Permission denied` and then runs `cmd`
//! anyway, exiting with *its* status. Any other failure to set it is fatal
//! (125). That split is `perm_related_errno`, which is `EACCES || EPERM` —
//! both of which arrive here as [`std::io::ErrorKind::PermissionDenied`].
//!
//! But the warning is only a warning if it was *delivered*. Upstream checks
//! `ferror (stderr)` immediately afterwards and returns 125 if the write
//! failed, so `nice -n -5 true 2>&-` and `nice -n -5 true 2>/dev/full` both
//! exit 125 while running nothing. Measured, both.
//!
//! # `nice >&-` is the reason this binary guards its descriptors
//!
//! With no command, this program's whole output is one line on stdout, and
//! gnulib's `close_stdout` — registered with `initialize_exit_failure
//! (EXIT_CANCELED)` — makes failing to write it fatal:
//!
//! ```text
//! $ nice >&- ; echo $?
//! nice: write error: Bad file descriptor
//! 125
//! ```
//!
//! Neither half of that survives Rust's runtime unassisted: `sanitize_standard_fds`
//! reopens the closed descriptor on `/dev/null` before `main`, and `handle_ebadf`
//! would report the write as having succeeded even if it had not. See
//! [`coreutils::stdfd`], which exists for this and which `nohup` needs for the
//! same reason.
//!
//! The exactness of gnulib's rule matters and is reproduced rather than
//! approximated: `close_stream` fails on `prev_fail || (fclose_fail &&
//! (some_pending || errno != EBADF))`, so a closed stdout with *nothing
//! buffered* is not an error. That is why `nice true >&-` exits 0 (measured)
//! while `nice >&-` exits 125 — the difference is whether a byte was owed.
//!
//! The same rule reaches somewhere less obvious. `nice /nope 2>&-` exits **125,
//! not 127**: the `No such file or directory` report could not be delivered, so
//! `close_stdout`'s stderr branch `_exit`s with the failure status and the
//! command-not-found code never survives. Measured.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes, quote_os};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

// Recorded before `main`, so that a descriptor the caller closed is seen as
// closed rather than as the `/dev/null` the runtime substitutes for it. See the
// module docs and `coreutils::stdfd`.
coreutils::guard_std_fds!();

/// `EXIT_CANCELED`. Everything `nice` itself gets wrong is this: a bad option,
/// an unreadable adjustment, an adjustment with no command, a niceness that
/// could not be set, and a diagnostic that could not be delivered.
const NICE_FAILURE: u8 = 125;

/// The command was found but could not be run.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const EXIT_CANNOT_INVOKE: u8 = 126;

/// The command was not found.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const EXIT_ENOENT: u8 = 127;

const NICE_NAME: &str = "nice";
const NICE: Program = Program::new(NICE_NAME, NICE_FAILURE as i32);

/// The leading `+` stops option parsing at the first operand, so everything
/// after the command name belongs to the *command*. Measured: `nice true
/// --version` runs `true --version` rather than printing `nice`'s version.
const SHORT_OPTIONS: &str = "+n:";

/// Upstream's `longopts`, in its order — `adjustment` first, then the two
/// standard ones. `scripts/getopt-ambiguity-check.py` compares this table
/// against the real binary's, so `--a`, `--adj` and `--h` resolve as GNU's do.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("adjustment", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `NZERO` is 20, so upstream's `1 - 2 * NZERO` is −39 …
const MIN_ADJUSTMENT: i64 = -39;

/// … and `2 * NZERO - 1` is 39. Twice the nice range, because an adjustment is
/// added to the *current* niceness: from 19 you must be able to reach −20.
const MAX_ADJUSTMENT: i64 = 39;

/// What `nice` does with no `-n` and no `-NUM`, and what its `--help` promises.
const DEFAULT_ADJUSTMENT: i32 = 10;

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run {
        /// The adjustment **as typed**, unvalidated. Kept in this shape because
        /// upstream validates after the whole scan, which is what lets
        /// `nice --help -n abc` print the help rather than the complaint.
        adjustment: Option<OsString>,
        /// The command and its arguments, as bytes, so they reach `exec`
        /// exactly as they were typed.
        argv: Vec<OsString>,
    },
}

fn main() -> ExitCode {
    // First, before anything can touch standard I/O: `nice >&-` must see a
    // closed descriptor 1 rather than the runtime's stand-in.
    stdfd::restore();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = Stream::stdout();
    let mut err = Stream::stderr();

    let status = run(&args, &mut out, &mut err);
    close_streams(out, err, status)
}

/// gnulib's `atexit (close_stdout)`, spelled out rather than registered.
///
/// Registering it is not available here — `ExitCode` is returned, not
/// `exit`ed — and spelling it out is better anyway, because the ordering is
/// load-bearing: a failed stdout is *reported* and then replaces the status,
/// while a failed stderr replaces the status silently, there being nowhere left
/// to say so.
fn close_streams(out: Stream, err: Stream, status: u8) -> ExitCode {
    if let Err(e) = out.finish() {
        stdfd::write_error(NICE_NAME, &e);
        return ExitCode::from(NICE_FAILURE);
    }
    if err.finish().is_err() {
        return ExitCode::from(NICE_FAILURE);
    }
    ExitCode::from(status)
}

/// Everything between the two stream lifetimes: on success this does not
/// return, because `exec` replaces the process.
fn run(args: &[OsString], out: &mut Stream, err: &mut Stream) -> u8 {
    let request = match scan(args) {
        Ok(request) => request,
        Err(e) => return report(err, &e.message(), status_of(&e)),
    };

    let (adjustment, argv) = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            return 0;
        }
        Request::Version => {
            let _ = out.write_all(b"nice (SlateOS coreutils) 0.1.0\n");
            return 0;
        }
        Request::Run { adjustment, argv } => (adjustment, argv),
    };

    // Validated here rather than as it is scanned, because `--help` and
    // `--version` come first: measured, `nice --help -n abc` prints the help.
    let adjustment = match adjustment.as_ref().map(read_adjustment) {
        Some(Err(message)) => return report(err, &message, NICE_FAILURE),
        Some(Ok(value)) => Some(value),
        None => None,
    };

    if argv.is_empty() {
        return match adjustment {
            // An adjustment nobody can apply. Upstream prints this with
            // `error (0, …)` and then `usage (EXIT_CANCELED)`, so it carries
            // the `Try 'nice --help'` referral that the messages above do not.
            Some(_) => report(
                err,
                &NICE
                    .usage_referring("a command must be given with an adjustment".to_string())
                    .message(),
                NICE_FAILURE,
            ),
            None => print_niceness(out, err),
        };
    }

    match imp::set_niceness(adjustment.unwrap_or(DEFAULT_ADJUSTMENT)) {
        Ok(()) => {}
        // Refusal is a warning and the command still runs; anything else is
        // fatal. See the module docs.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let _ = writeln!(err, "{NICE_NAME}: cannot set niceness: {}", strerror(&e));
            // Upstream's `if (ferror (stderr)) return EXIT_CANCELED;`. A
            // warning nobody received is not a warning.
            if err.errored() {
                return NICE_FAILURE;
            }
        }
        Err(e) => {
            return report(
                err,
                &format!("cannot set niceness: {}", strerror(&e)),
                NICE_FAILURE,
            );
        }
    }

    // Does not return on success.
    let failure = imp::exec(&argv);
    let status = exec_failure_status(failure.kind());
    let name = argv.first().map_or_else(OsString::new, Clone::clone);
    report(
        err,
        &format!("{}: {}", quote_os(&name), strerror(&failure)),
        status,
    )
}

/// Print `nice: MESSAGE` and hand back the status to exit with.
///
/// Through the caller's [`Stream`] rather than `eprintln!`, because whether the
/// message arrived is part of this program's answer — see the module docs.
fn report(err: &mut Stream, message: &str, status: u8) -> u8 {
    let _ = writeln!(err, "{NICE_NAME}: {message}");
    status
}

/// With no command and no adjustment, `nice` is a query.
fn print_niceness(out: &mut Stream, err: &mut Stream) -> u8 {
    match imp::get_niceness() {
        Ok(niceness) => {
            let _ = writeln!(out, "{niceness}");
            0
        }
        Err(e) => report(
            err,
            &format!("cannot get niceness: {}", strerror(&e)),
            NICE_FAILURE,
        ),
    }
}

/// 127 if the command was not found, 126 if it was found but could not run.
///
/// Upstream's split, and what lets a script tell a typo in the command name
/// from a file that is present but not executable.
fn exec_failure_status(kind: std::io::ErrorKind) -> u8 {
    if kind == std::io::ErrorKind::NotFound {
        EXIT_ENOENT
    } else {
        EXIT_CANNOT_INVOKE
    }
}

/// A getopt error's status, which is 125 for every one `nice` can produce.
fn status_of(e: &getopt::Error) -> u8 {
    u8::try_from(e.status).unwrap_or(NICE_FAILURE)
}

// ------------------------------------------------------------- scanning ----

/// Walk the command line the way `nice.c`'s `main` loop does: one word at a
/// time, testing for the obsolete `-NUM` form before letting `getopt` have a
/// turn. See the module docs for why the two must interleave.
///
/// # Errors
///
/// A getopt diagnostic for an unknown option or a missing option argument. The
/// adjustment is *not* validated here; see [`read_adjustment`].
fn scan(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut adjustment: Option<OsString> = None;
    let mut at = 0usize;

    while at < args.len() {
        let Some(word) = args.get(at) else { break };
        if let Some(digits) = obsolete_adjustment(word) {
            adjustment = Some(digits);
            at = at.saturating_add(1);
            continue;
        }

        let rest = args.get(at..).unwrap_or_default();
        let mut parser = NICE.parse(rest, SHORT_OPTIONS, LONG_OPTIONS);
        let Some(item) = parser.next() else {
            // No option here and none after it — `--` on its own, or the end of
            // argv. `optind` says how much of `rest` was consumed getting here.
            at = at.saturating_add(parser.optind());
            break;
        };
        match item? {
            // `Takes::Required` guarantees the value, but a `None` here would
            // mean "no adjustment given" rather than a panic, which is the
            // right way for an impossible case to fail.
            Opt::Short(b'n', value) | Opt::Long("adjustment", value) => adjustment = value,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(_) => {
                // The command. `+` in [`SHORT_OPTIONS`] means the parser has
                // stopped, and `optind` is one *past* the operand, so step back
                // onto it: everything from here is the command's own argv.
                at = at.saturating_add(parser.optind()).saturating_sub(1);
                break;
            }
            Opt::Short(..) | Opt::Long(..) => {}
        }
        at = at.saturating_add(parser.optind());
    }

    Ok(Request::Run {
        adjustment,
        argv: args.get(at..).unwrap_or_default().to_vec(),
    })
}

/// The pre-`getopt` `nice -5` syntax: a dash, an optional second sign, and a
/// digit.
///
/// Upstream is `s[0] == '-' && ISDIGIT (s[1 + (s[1] == '-' || s[1] == '+')])`,
/// with the adjustment taken as `s + 1` — so everything after the *first* dash,
/// second sign included. Measured: `-5` is +5, `--5` is −5, `-+5` is +5, and
/// `-5x` is the invalid adjustment `5x` rather than an unknown option.
///
/// Only the first digit is tested, which is why the rest is left to
/// [`read_adjustment`] to reject.
fn obsolete_adjustment(word: &OsString) -> Option<OsString> {
    let bytes = os_bytes(word.as_os_str());
    if bytes.first() != Some(&b'-') {
        return None;
    }
    // A second `-` or `+` is skipped over for the digit test but kept in the
    // number, which is the whole of what makes `--5` negative.
    let sign = usize::from(matches!(bytes.get(1), Some(b'-' | b'+')));
    if !bytes
        .get(sign.saturating_add(1))
        .is_some_and(u8::is_ascii_digit)
    {
        return None;
    }
    Some(os_from_bytes(bytes.get(1..).unwrap_or_default()))
}

/// Read the adjustment, and clamp it rather than refusing it when it is large.
///
/// Upstream is `xstrtol (adjustment_given, nullptr, 10, &tmp, "")` guarded by
/// `LONGINT_OVERFLOW < …`, which accepts both `LONGINT_OK` **and**
/// `LONGINT_OVERFLOW` — so a number too large for a `long` is not an error, it
/// is `LONG_MAX` on its way to being clamped. Measured: `nice -n
/// 99999999999999999999 sh -c nice` prints 19, not a complaint.
///
/// The empty suffix list is what makes the rest strict: leading whitespace and
/// a sign are `strtol`'s and are accepted (`-n ' 5'` is 5, `-n +5` is 5), while
/// any trailing character at all is refused (`-n '5 '`, `-n 0x10`, `-n 5x`).
///
/// # Errors
///
/// `invalid adjustment ‘X’`, which upstream prints with `error (EXIT_CANCELED,
/// …)` — one line, and measured to carry **no** `Try 'nice --help'` referral.
fn read_adjustment(text: &OsString) -> Result<i32, String> {
    let bytes = os_bytes(text.as_os_str());
    let (value, status) = xnum::xstrtoimax(&bytes, Some(b""));
    match status {
        Status::Ok | Status::Overflow => {
            let clamped = value.clamp(MIN_ADJUSTMENT, MAX_ADJUSTMENT);
            // In range by construction; naming a fallback rather than
            // unwrapping keeps the no-panic rule without a lint waiver.
            Ok(i32::try_from(clamped).unwrap_or(DEFAULT_ADJUSTMENT))
        }
        Status::Invalid | Status::InvalidSuffix | Status::InvalidSuffixWithOverflow => {
            Err(format!("invalid adjustment {}", quote_os(text)))
        }
    }
}

/// GNU's `--help`, minus the project's `Report bugs to:` block and the note
/// about shell built-ins, as every converted utility here omits them.
fn help_text() -> String {
    "\
Usage: nice [OPTION] [COMMAND [ARG]...]
Run COMMAND with an adjusted niceness, which affects process scheduling.
With no COMMAND, print the current niceness.  Niceness values range from
-20 (most favorable to the process) to 19 (least favorable to the process).

Mandatory arguments to long options are mandatory for short options too.
  -n, --adjustment=N   add integer N to the niceness (default 10)
      --help        display this help and exit
      --version     output version information and exit

Exit status:
  125  if the nice command itself fails
  126  if COMMAND is found but cannot be invoked
  127  if COMMAND cannot be found
  -    the exit status of COMMAND otherwise
"
    .to_string()
}

// ------------------------------------------------------------------ unix ----

/// The three things that are not portable: reading the niceness, setting it,
/// and becoming the command.
///
/// The target (`toolchain/x86_64-slateos.json`) is `"os": "linux"`, so this is
/// the arm that ships; the development host is Windows and gets the stubs
/// below, which is the convention `tty.rs` and `nohup.rs` already follow. The
/// scanning above is deliberately outside this module so that it is unit-tested
/// on both.
#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::OsString;
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    unsafe extern "C" {
        fn nice(inc: i32) -> i32;
        fn getpriority(which: i32, who: u32) -> i32;
        fn __errno_location() -> *mut i32;
    }

    /// `PRIO_PROCESS`: the niceness of a process, as against a group or a user.
    const PRIO_PROCESS: i32 = 0;

    /// Both `nice` and `getpriority` return −1 as a legitimate *value*, so the
    /// only way to tell a failure from a niceness of −1 is to clear `errno`
    /// first and look at it afterwards. That is the documented idiom and it is
    /// why these two are wrapped rather than called directly.
    fn errno_slot() -> *mut i32 {
        // SAFETY: `__errno_location` is defined to return a valid pointer to
        // this thread's `errno` and never fails.
        unsafe { __errno_location() }
    }

    fn clear_errno() {
        // SAFETY: the pointer is this thread's `errno`, which is a live `int`
        // for the whole life of the thread.
        unsafe { *errno_slot() = 0 };
    }

    fn errno() -> i32 {
        // SAFETY: as above.
        unsafe { *errno_slot() }
    }

    /// The current niceness — upstream's `GET_NICENESS ()`.
    ///
    /// # Errors
    ///
    /// Whatever `getpriority` reports, distinguished from a niceness of −1 by
    /// the cleared `errno`.
    pub fn get_niceness() -> io::Result<i32> {
        clear_errno();
        // SAFETY: `getpriority` only reads scheduling state and takes no
        // pointers; `0` means "this process".
        let value = unsafe { getpriority(PRIO_PROCESS, 0) };
        let e = errno();
        if value == -1 && e != 0 {
            return Err(io::Error::from_raw_os_error(e));
        }
        Ok(value)
    }

    /// Add `adjustment` to the current niceness.
    ///
    /// # Errors
    ///
    /// `EACCES`/`EPERM` when the caller may not raise its own priority — which
    /// the caller treats as a warning rather than a refusal — or anything else
    /// `nice(2)` reports.
    pub fn set_niceness(adjustment: i32) -> io::Result<()> {
        clear_errno();
        // SAFETY: `nice` takes no pointers and only alters this process's own
        // scheduling priority.
        let value = unsafe { nice(adjustment) };
        let e = errno();
        if value == -1 && e != 0 {
            return Err(io::Error::from_raw_os_error(e));
        }
        Ok(())
    }

    /// Become the command. Returns only on failure.
    ///
    /// `CommandExt::exec` is `execvp`, so a bare name is searched for on
    /// `PATH` — which is what upstream does and what makes `nice ls` work.
    pub fn exec(argv: &[OsString]) -> io::Error {
        let Some((program, arguments)) = argv.split_first() else {
            // Unreachable: the caller has already checked for an empty argv.
            return io::Error::from(io::ErrorKind::NotFound);
        };
        Command::new(program).args(arguments).exec()
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::ffi::OsString;
    use std::io;

    pub fn get_niceness() -> io::Result<i32> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn set_niceness(_adjustment: i32) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn exec(_argv: &[OsString]) -> io::Error {
        io::Error::from(io::ErrorKind::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::{Request, obsolete_adjustment, read_adjustment, scan};
    use std::ffi::OsString;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn run_of(words: &[&str]) -> (Option<String>, Vec<String>) {
        match scan(&args(words)) {
            Ok(Request::Run { adjustment, argv }) => (
                adjustment.map(|a| a.to_string_lossy().into_owned()),
                argv.iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
            ),
            other => panic!("expected a command line, got {other:?}"),
        }
    }

    // --------------------------------------------------- obsolete -NUM ----

    #[test]
    fn a_bare_number_is_a_positive_adjustment() {
        assert_eq!(
            run_of(&["-5", "true"]),
            (Some("5".into()), vec!["true".into()])
        );
    }

    #[test]
    fn a_second_dash_makes_it_negative() {
        // `--5` is −5 because the adjustment is everything after the *first*
        // dash. This is the rule a reimplementation is most likely to miss.
        assert_eq!(
            run_of(&["--5", "true"]),
            (Some("-5".into()), vec!["true".into()])
        );
    }

    #[test]
    fn a_second_plus_is_kept_and_is_positive() {
        assert_eq!(
            run_of(&["-+5", "true"]),
            (Some("+5".into()), vec!["true".into()])
        );
    }

    #[test]
    fn trailing_rubbish_is_an_adjustment_not_an_unknown_option() {
        // `-5x` reaches validation as the adjustment `5x`; measured, GNU says
        // `invalid adjustment ‘5x’` rather than `invalid option -- '5'`.
        assert_eq!(run_of(&["-5x"]), (Some("5x".into()), vec![]));
    }

    #[test]
    fn a_lone_dash_is_a_command_not_an_adjustment() {
        assert_eq!(obsolete_adjustment(&OsString::from("-")), None);
        assert_eq!(run_of(&["-"]), (None, vec!["-".into()]));
    }

    #[test]
    fn a_double_dash_is_not_an_adjustment() {
        // `s[1] == '-'` so the digit test looks at `s[2]`, which terminates.
        assert_eq!(obsolete_adjustment(&OsString::from("--")), None);
    }

    #[test]
    fn a_named_option_is_not_an_adjustment() {
        assert_eq!(obsolete_adjustment(&OsString::from("-n")), None);
        assert_eq!(obsolete_adjustment(&OsString::from("--help")), None);
    }

    // ------------------------------------------------------ interleaving ----

    #[test]
    fn a_negative_argument_belongs_to_the_option_that_asked_for_it() {
        // The whole reason the scan interleaves: `-5` here is `-n`'s value, not
        // an obsolete adjustment of its own.
        assert_eq!(
            run_of(&["-n", "-5", "true"]),
            (Some("-5".into()), vec!["true".into()])
        );
    }

    #[test]
    fn the_last_adjustment_wins_across_both_spellings() {
        assert_eq!(run_of(&["-n", "3", "-5", "true"]).0, Some("5".into()));
        assert_eq!(run_of(&["-5", "-n", "3", "true"]).0, Some("3".into()));
        assert_eq!(run_of(&["-n", "5", "-n", "3", "true"]).0, Some("3".into()));
    }

    #[test]
    fn the_value_may_be_joined_or_separate_or_long() {
        assert_eq!(run_of(&["-n5", "true"]).0, Some("5".into()));
        assert_eq!(run_of(&["-n", "5", "true"]).0, Some("5".into()));
        assert_eq!(run_of(&["--adjustment=7", "true"]).0, Some("7".into()));
        assert_eq!(run_of(&["--adj=3", "true"]).0, Some("3".into()));
        assert_eq!(run_of(&["--a=3", "true"]).0, Some("3".into()));
    }

    // ---------------------------------------------------------- operands ----

    #[test]
    fn options_after_the_command_belong_to_the_command() {
        // `+` in the shorts string. Measured: `nice true --version` runs
        // `true --version` rather than printing nice's version.
        assert_eq!(
            run_of(&["true", "--version", "-n", "5"]),
            (
                None,
                vec!["true".into(), "--version".into(), "-n".into(), "5".into()]
            )
        );
    }

    #[test]
    fn a_double_dash_hands_the_next_word_to_the_command() {
        assert_eq!(
            run_of(&["--", "-n", "5"]),
            (None, vec!["-n".into(), "5".into()])
        );
    }

    #[test]
    fn a_double_dash_alone_leaves_no_command() {
        assert_eq!(run_of(&["--"]), (None, vec![]));
    }

    #[test]
    fn a_double_dash_after_an_adjustment_leaves_no_command() {
        // Measured: `nice -n 5 --` is `a command must be given with an
        // adjustment`, so the adjustment must survive and the argv must not.
        assert_eq!(run_of(&["-n", "5", "--"]), (Some("5".into()), vec![]));
    }

    #[test]
    fn nothing_at_all_is_the_query_form() {
        assert_eq!(run_of(&[]), (None, vec![]));
    }

    // ------------------------------------------------- help and version ----

    #[test]
    fn help_and_version_win_over_a_bad_adjustment() {
        // Upstream validates after the scan, so these come first.
        assert_eq!(scan(&args(&["--help", "-n", "abc"])), Ok(Request::Help));
        assert_eq!(
            scan(&args(&["-n", "abc", "--version"])),
            Ok(Request::Version)
        );
    }

    #[test]
    fn help_after_the_command_is_the_commands_business() {
        assert_eq!(
            run_of(&["true", "--help"]).1,
            vec!["true".to_string(), "--help".into()]
        );
    }

    // -------------------------------------------------------- diagnostics ----

    #[test]
    fn an_unknown_option_is_a_getopt_error() {
        let e = scan(&args(&["-x"])).expect_err("an unknown option is refused");
        assert_eq!(e.sentence, "invalid option -- 'x'");
        assert_eq!(e.referral, Some("nice"));
        assert_eq!(e.status, 125);
    }

    #[test]
    fn a_missing_option_argument_is_a_getopt_error() {
        let e = scan(&args(&["-n"])).expect_err("-n needs a value");
        assert_eq!(e.sentence, "option requires an argument -- 'n'");
        let e = scan(&args(&["--adjustment"])).expect_err("--adjustment needs a value");
        assert_eq!(e.sentence, "option '--adjustment' requires an argument");
    }

    // --------------------------------------------------------- the number ----

    #[test]
    fn plain_numbers_read_as_written() {
        assert_eq!(read_adjustment(&OsString::from("5")), Ok(5));
        assert_eq!(read_adjustment(&OsString::from("-5")), Ok(-5));
        assert_eq!(read_adjustment(&OsString::from("+5")), Ok(5));
        assert_eq!(read_adjustment(&OsString::from("0")), Ok(0));
        assert_eq!(read_adjustment(&OsString::from("-0")), Ok(0));
    }

    #[test]
    fn leading_whitespace_is_strtols_and_is_accepted() {
        assert_eq!(read_adjustment(&OsString::from(" 5")), Ok(5));
    }

    #[test]
    fn trailing_anything_is_refused() {
        for bad in ["5 ", "5x", "0x10", "abc", "", "true"] {
            assert!(
                read_adjustment(&OsString::from(bad)).is_err(),
                "{bad:?} should not be an adjustment"
            );
        }
    }

    #[test]
    fn the_message_quotes_the_text_and_names_no_referral() {
        assert_eq!(
            read_adjustment(&OsString::from("abc")),
            Err("invalid adjustment \u{2018}abc\u{2019}".to_string())
        );
    }

    #[test]
    fn an_out_of_range_number_is_clamped_rather_than_refused() {
        // `LONGINT_OVERFLOW` passes upstream's guard, so this is 39 and not a
        // complaint. Measured: GNU gives the command a niceness of 19, which is
        // 39 clamped again by the kernel.
        assert_eq!(
            read_adjustment(&OsString::from("99999999999999999999")),
            Ok(39)
        );
        assert_eq!(
            read_adjustment(&OsString::from("-99999999999999999999")),
            Ok(-39)
        );
        assert_eq!(read_adjustment(&OsString::from("40")), Ok(39));
        assert_eq!(read_adjustment(&OsString::from("-40")), Ok(-39));
    }

    #[test]
    fn a_non_utf8_adjustment_is_refused_rather_than_panicking() {
        let bad = coreutils::quote::os_from_bytes(b"\xff");
        assert!(read_adjustment(&bad).is_err());
    }
}
