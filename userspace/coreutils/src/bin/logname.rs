//! `logname` -- print the user's login name.
//!
//! # What was here before
//!
//! Twenty-one lines that answered a different question, and five defects.
//! The first is the same one `whoami` had, and it matters more here:
//!
//! 1. **It read the environment.** The old code took `$LOGNAME`, then `$USER`.
//!    GNU calls `getlogin(3)`, which reports the name the *session* was opened
//!    under -- the entry the login program wrote to the user-accounting
//!    database for this controlling terminal. That is the one thing `logname`
//!    is for: after `su alice`, `whoami` says `alice` and `logname` still says
//!    who logged in. Reading `$LOGNAME` collapses the two, and since `su` sets
//!    `$LOGNAME`, the old code gave `whoami`'s answer under `logname`'s name.
//! 2. **It never failed.** `getlogin` returns null on a session with no
//!    accounting entry -- a cron job, a container, a bare `ssh host cmd` -- and
//!    GNU then prints `logname: no login name` and exits 1. Measured: that is
//!    what GNU 9.4 does under WSL, where nothing writes utmp, while the old
//!    code confidently printed `$LOGNAME`.
//! 3. **No options at all.** `logname --help` printed a name.
//! 4. **An operand was accepted silently**, where GNU refuses the first with
//!    `logname: extra operand ‘x’` and refers on to `--help`.
//! 5. **The name was a `String`**, which panics on a name that is not UTF-8,
//!    and **a failed write was reported as success** -- `logname >&-` exited 0
//!    having printed nowhere. The last is what brought the file into the
//!    closed-descriptor sweep; the rest was found on the way.
//!
//! # What `getlogin` currently answers here
//!
//! `posix::pwd::getlogin` returns `root` unconditionally, because our
//! `<utmpx.h>` is a set of stubs over a database nobody writes. So on SlateOS
//! this prints `root` and the `no login name` branch is unreachable -- which
//! is a true answer for a single-account system, and becomes a wrong one the
//! day a second account exists. Tracked in `known-issues.md` as
//! `TD-POSIX-GETLOGIN-IS-A-CONSTANT`; the fix belongs in the POSIX layer, and
//! this file needs no change when it lands.

// The host build stops at the `main` below that refuses to run, so everything
// the real one would have called is unreachable there. Same reason as `id`.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use std::ffi::OsString;

coreutils::guard_std_fds!();

/// `logname`'s usage status is 1 -- measured: `logname x; echo $?` prints 1.
const LOGNAME: Program = Program::new("logname", 1);

/// GNU `logname`'s `getopt_long` string, exactly: it has no short options.
///
/// Empty rather than absent, so that `-x` is `invalid option -- 'x'` and not
/// an operand.
const SHORT_OPTIONS: &str = "";

/// GNU `logname`'s `longopts[]`: the two `parse_long_options` adds, and
/// nothing of its own.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// Print the name. There is nothing to carry: `logname` takes no operand
    /// and has no flag that changes what it prints.
    Run,
}

/// GNU's usage line here is `[OPTION]` and not `[OPTION]...`, unlike
/// `whoami`'s. Transcribed rather than made consistent: the difference is
/// upstream's and a script that greps for it should find what it expects.
fn help_text() -> String {
    "\
Usage: logname [OPTION]
Print the user's login name.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `logname`'s argv.
///
/// # Errors
///
/// An unknown option, a long option given a value it does not take, or any
/// operand at all -- only the first is named, as upstream's `argv[optind]`
/// does.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut extra: Option<OsString> = None;

    for item in LOGNAME.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // Recorded rather than refused on the spot: `--help` still wins
            // over an operand that precedes it, because upstream checks
            // `optind < argc` only after the whole scan. Measured:
            // `logname x --help` prints the help and exits 0.
            Opt::Operand(name) => {
                if extra.is_none() {
                    extra = Some(name.clone());
                }
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(LOGNAME.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(LOGNAME.invalid_option(other)),
        }
    }

    match extra {
        Some(name) => Err(LOGNAME.usage_referring(format!(
            "extra operand {}",
            quote(&os_bytes(name.as_os_str()))
        ))),
        None => Ok(Request::Run),
    }
}

// ------------------------------------------------------------------- unix ---

#[cfg(unix)]
mod imp {
    use super::{Request, help_text, parse_args};
    use coreutils::stdfd::{self, Stream};
    use std::ffi::{CStr, OsString};
    use std::io::Write;
    use std::process::ExitCode;

    unsafe extern "C" {
        fn getlogin() -> *const std::ffi::c_char;
    }

    /// The session's login name, or `None` where the accounting database has
    /// no entry for this terminal.
    ///
    /// The returned bytes are copied out immediately: `getlogin` hands back a
    /// pointer into a static buffer that the next call to it -- or to any of
    /// the `utmp` family -- may overwrite.
    fn login_name() -> Option<Vec<u8>> {
        // SAFETY: `getlogin` takes no arguments and returns either null or a
        // pointer to a NUL-terminated string in static storage, which is
        // valid at least until the next call into the utmp family. Nothing
        // else runs between the call and the copy.
        let name = unsafe {
            let p = getlogin();
            if p.is_null() {
                return None;
            }
            CStr::from_ptr(p).to_bytes().to_vec()
        };
        Some(name)
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();

        // Decided before the stream exists: upstream's `usage (EXIT_FAILURE)`
        // never reaches `atexit (close_stdout)` with anything buffered, so
        // `logname x >&-` prints only the operand complaint.
        let request = match parse_args(&args) {
            Ok(request) => request,
            Err(e) => {
                eprintln!("logname: {e}");
                return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
            }
        };

        let mut out = Stream::stdout();
        let earned = match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
                ExitCode::SUCCESS
            }
            Request::Version => {
                let _ = out.write_all(b"logname (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Run => match login_name() {
                Some(name) => {
                    // Bytes, not text: a login name comes from the accounting
                    // database and need not be UTF-8.
                    let _ = out.write_all(&name);
                    let _ = out.write_all(b"\n");
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("logname: no login name");
                    ExitCode::FAILURE
                }
            },
        };
        // Reached even when there was no name: upstream's `die` runs the
        // `atexit` handler too, so a closed stdout is still reported. It is
        // silent here, because nothing was buffered -- gnulib's `close_stream`
        // forgives `EBADF` when there was nothing left to write.
        stdfd::close_stdout("logname", out, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    imp::main()
}

/// The host build exists only so `cargo test` runs on the developer machine.
///
/// There is no honest answer here: Windows has no `getlogin` and no user
/// accounting database, and the old code's guess -- `$LOGNAME` -- is exactly
/// the environment-trusting behaviour this file was rewritten to remove. `id`
/// and `whoami` say the same thing for the same reason.
#[cfg(not(unix))]
fn main() {
    eprintln!("logname: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_bare_invocation_is_a_run() {
        assert_eq!(parse_args(&argv(&[])).unwrap(), Request::Run);
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
    }

    /// The scan runs to the end before the operand check, so a `--help` after
    /// an operand still wins -- measured: `logname x --help` prints the help.
    #[test]
    fn help_wins_over_an_operand_that_precedes_it() {
        assert_eq!(parse_args(&argv(&["x", "--help"])).unwrap(), Request::Help);
    }

    /// After `--` it is an operand, however it is spelled -- measured:
    /// `logname -- --help` says `extra operand ‘--help’`.
    #[test]
    fn a_double_dash_turns_an_option_into_an_operand() {
        let e = parse_args(&argv(&["--", "--help"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}--help\u{2019}\nTry 'logname --help' for more information."
        );
    }

    #[test]
    fn an_unambiguous_prefix_is_accepted() {
        assert_eq!(parse_args(&argv(&["--hel"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--v"])).unwrap(), Request::Version);
    }

    #[test]
    fn the_first_operand_is_the_one_named() {
        let e = parse_args(&argv(&["a", "b"])).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "extra operand \u{2018}a\u{2019}\nTry 'logname --help' for more information."
        );
    }

    #[test]
    fn an_unknown_option_is_refused() {
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'logname --help' for more information."
        );
        let e = parse_args(&argv(&["--nope"])).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'logname --help' for more information."
        );
    }

    #[test]
    fn a_value_given_to_a_flag_is_refused() {
        let e = parse_args(&argv(&["--help=1"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--help' doesn't allow an argument\n\
             Try 'logname --help' for more information."
        );
    }

    /// An operand that is not UTF-8 must reach the diagnostic unchanged --
    /// escaped, as gnulib's `quote` escapes it, not replaced.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_operand_is_escaped_not_corrupted() {
        use std::os::unix::ffi::OsStringExt;
        let e = parse_args(&[OsString::from_vec(b"a\xffb".to_vec())]).unwrap_err();
        assert!(
            e.message()
                .starts_with("extra operand \u{2018}a\\377b\u{2019}"),
            "got {:?}",
            e.message()
        );
    }

    /// Upstream's usage line is `[OPTION]`, without the ellipsis `whoami` has.
    #[test]
    fn the_help_text_is_upstreams_wording() {
        let text = help_text();
        assert!(text.starts_with("Usage: logname [OPTION]\n"));
        assert!(text.contains("Print the user's login name.\n"));
        assert!(text.ends_with("output version information and exit\n"));
    }
}
