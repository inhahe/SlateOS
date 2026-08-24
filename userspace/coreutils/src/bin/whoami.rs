//! `whoami` -- print the user name associated with the effective user ID.
//!
//! # What was here before
//!
//! Seventy-five lines that answered a different question, and six defects.
//! The first is not a detail:
//!
//! 1. **It read the environment.** The old `user_from_env` consulted `$USER`,
//!    then `$LOGNAME`, and printed whichever it found. GNU `whoami` does not
//!    look at the environment at all -- it is documented as "same as `id -un`",
//!    which is `getpwuid(geteuid())`. The difference is the whole point of the
//!    utility: `$USER` is a *claim* the environment makes and any process can
//!    set, while the effective uid is what the kernel will actually enforce.
//!    Measured, GNU 9.4: `USER=zzz LOGNAME=zzz whoami` prints the real account
//!    name. Ours printed `zzz`. A script that used `whoami` to decide whether
//!    it was root could be told anything at all.
//! 2. **It fell back to printing a number.** With no name available the old
//!    code printed the uid as if that were an answer. GNU refuses:
//!    `whoami: cannot find name for user ID 31337` on stderr, exit 1, nothing
//!    on stdout. Measured under `unshare --map-user=31337`.
//! 3. **No options at all.** `whoami --help` printed a user name; `whoami -x`
//!    did too, where GNU refuses with `invalid option -- 'x'`.
//! 4. **An operand was accepted silently.** GNU refuses the first one --
//!    `whoami: extra operand ‘x’` -- and refers on to `--help`.
//! 5. **A failed write was reported as success.** `whoami >&-` exited 0,
//!    having printed nowhere. GNU reports `whoami: write error: Bad file
//!    descriptor` and exits 1. This is what brought the file into scope; the
//!    rest was found on the way.
//! 6. **The name was a `String`.** A login name is a field of `/etc/passwd`
//!    and is bytes; forcing UTF-8 on it panics on a line that is not. It is
//!    written as bytes now.
//!
//! # Why the operand is quoted curly
//!
//! `whoami.c` reports the extra operand with gnulib's *locale* `quote()`, not
//! with `quotef`, so under a UTF-8 locale it comes out `‘x’` rather than
//! `'x'`. Measured, GNU 9.4, `LC_ALL=C.UTF-8`. The same choice as `pwd`'s
//! diagnostics and the opposite of most of the suite; see the `quoting`
//! crate's module docs for the split.

// The host build stops at the `main` below that refuses to run, so everything
// the real one would have called is unreachable there. Same reason as `id`.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use std::ffi::OsString;

coreutils::guard_std_fds!();

/// `whoami`'s usage status is 1 -- measured: `whoami x; echo $?` prints 1.
const WHOAMI: Program = Program::new("whoami", 1);

/// GNU `whoami`'s `getopt_long` string, exactly: it has no short options.
///
/// Empty rather than absent, so that `-x` is `invalid option -- 'x'` and not
/// an operand. And with no leading `+`, so `whoami x --help` still prints the
/// help -- measured: it exits 0 rather than complaining about `x`.
const SHORT_OPTIONS: &str = "";

/// GNU `whoami`'s `longopts[]`: the two `parse_long_options` adds, and nothing
/// of its own.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// Print the name. There is nothing to carry: `whoami` takes no operand
    /// and has no flag that changes what it prints.
    Run,
}

fn help_text() -> String {
    "\
Usage: whoami [OPTION]...
Print the user name associated with the current effective user ID.
Same as id -un.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `whoami`'s argv.
///
/// # Errors
///
/// An unknown option, a long option given a value it does not take, or any
/// operand at all -- only the first is named, as upstream's `argv[optind]`
/// does.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut extra: Option<OsString> = None;

    for item in WHOAMI.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // Recorded rather than refused on the spot: `--help` still wins
            // over an operand that precedes it, because upstream checks
            // `optind != argc` only after the whole scan.
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
                return Err(WHOAMI.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(WHOAMI.invalid_option(other)),
        }
    }

    match extra {
        Some(name) => Err(WHOAMI.usage_referring(format!(
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
    use pwdb::Db;
    use std::ffi::OsString;
    use std::io::Write;
    use std::process::ExitCode;

    unsafe extern "C" {
        fn geteuid() -> u32;
    }

    /// The effective uid.
    ///
    /// Upstream compares the result against `(uid_t) -1` before looking it up,
    /// which is a guard for systems where `geteuid` can fail. POSIX says it
    /// cannot, and a uid of `0xffff_ffff` is a legitimate (if unusual) account
    /// number here rather than a sentinel, so the lookup is simply attempted
    /// and its failure reported the same way any other missing account is.
    fn effective_uid() -> u32 {
        // SAFETY: a POSIX getter with no arguments and no pointers, which
        // POSIX requires cannot fail.
        unsafe { geteuid() }
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();

        // Decided before the stream exists: upstream's `usage (EXIT_FAILURE)`
        // never reaches `atexit (close_stdout)` with anything buffered, so
        // `whoami x >&-` prints only the operand complaint.
        let request = match parse_args(&args) {
            Ok(request) => request,
            Err(e) => {
                eprintln!("whoami: {e}");
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
                let _ = out.write_all(b"whoami (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Run => {
                let uid = effective_uid();
                // Loaded here rather than at the top so that `--help` does not
                // read `/etc/passwd` -- which matters when the database is the
                // thing that is broken.
                match Db::load().user_by_uid(uid) {
                    Some(user) => {
                        // Bytes, not text: a login name is a field of
                        // `/etc/passwd` and need not be UTF-8.
                        let _ = out.write_all(&user.name);
                        let _ = out.write_all(b"\n");
                        ExitCode::SUCCESS
                    }
                    None => {
                        eprintln!("whoami: cannot find name for user ID {uid}");
                        ExitCode::FAILURE
                    }
                }
            }
        };
        // Reached even when the lookup failed: upstream's `die` runs the
        // `atexit` handler too, so a closed stdout is still reported. It is
        // silent here, because nothing was buffered -- gnulib's `close_stream`
        // forgives `EBADF` when there was nothing left to write.
        stdfd::close_stdout("whoami", out, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    imp::main()
}

/// The host build exists only so `cargo test` runs on the developer machine.
///
/// There is no honest answer here: Windows has no effective uid and no
/// `/etc/passwd`, and the old code's guess -- `%USERNAME%` -- is exactly the
/// environment-trusting behaviour this file was rewritten to remove. `id` says
/// the same thing for the same reason.
#[cfg(not(unix))]
fn main() {
    eprintln!("whoami: unix-only utility; not supported on this platform");
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
    /// an operand still wins -- measured: `whoami x --help` exits 0.
    #[test]
    fn help_wins_over_an_operand_that_precedes_it() {
        assert_eq!(parse_args(&argv(&["x", "--help"])).unwrap(), Request::Help);
    }

    /// An unambiguous prefix is accepted; `--h` and `--v` are distinct.
    #[test]
    fn an_unambiguous_prefix_is_accepted() {
        assert_eq!(parse_args(&argv(&["--h"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--vers"])).unwrap(), Request::Version);
    }

    #[test]
    fn the_first_operand_is_the_one_named() {
        let e = parse_args(&argv(&["x", "y"])).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "extra operand \u{2018}x\u{2019}\nTry 'whoami --help' for more information."
        );
    }

    /// `--` ends the options, so what follows is an operand rather than a
    /// flag -- and an operand is still an error.
    #[test]
    fn a_double_dash_does_not_excuse_an_operand() {
        let e = parse_args(&argv(&["--", "x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}x\u{2019}\nTry 'whoami --help' for more information."
        );
    }

    /// An empty word is an operand like any other, and is quoted to nothing
    /// between the marks -- measured: `whoami ''` says `extra operand ‘’`.
    #[test]
    fn the_empty_operand_is_an_operand() {
        let e = parse_args(&argv(&[""])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}\u{2019}\nTry 'whoami --help' for more information."
        );
    }

    /// A lone `-` is not an option: with an empty short-option string there is
    /// nothing for it to introduce, and getopt hands it back as an operand.
    #[test]
    fn a_lone_dash_is_an_operand() {
        let e = parse_args(&argv(&["-"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}-\u{2019}\nTry 'whoami --help' for more information."
        );
    }

    #[test]
    fn an_unknown_option_is_refused() {
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'whoami --help' for more information."
        );
        let e = parse_args(&argv(&["--nope"])).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'whoami --help' for more information."
        );
    }

    #[test]
    fn a_value_given_to_a_flag_is_refused() {
        let e = parse_args(&argv(&["--help=1"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--help' doesn't allow an argument\n\
             Try 'whoami --help' for more information."
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

    #[test]
    fn the_help_text_names_the_program_and_both_options() {
        let text = help_text();
        assert!(text.starts_with("Usage: whoami [OPTION]...\n"));
        assert!(text.contains("Same as id -un.\n"));
        assert!(text.contains("      --help        display this help and exit\n"));
        assert!(text.ends_with("output version information and exit\n"));
    }
}
