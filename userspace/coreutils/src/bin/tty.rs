//! `tty` -- print the file name of the terminal connected to standard input.
//!
//! # What was here before
//!
//! Seventy-nine lines that asked a slightly different question and answered it
//! in a shape nothing else could check, with seven defects.
//!
//! 1. **It asked `isatty` first, and invented a name when `ttyname` then had
//!    none.** `tty.c` calls `ttyname (STDIN_FILENO)` and nothing else, under a
//!    comment that says so: *"POSIX requires that ttyname (0) be used here"*.
//!    The old code took `isatty(0)` as the verdict and fell back to the literal
//!    `/dev/tty` when `ttyname` returned null -- so a terminal whose name
//!    cannot be found printed a name that is not its own and exited 0, where
//!    GNU prints `not a tty` and exits 1. `/dev/tty` is in any case the wrong
//!    guess: it names the *controlling* terminal, which need not be the one on
//!    descriptor 0, and a caller doing `cd "$(tty)"`-shaped work with it would
//!    be handed a different device than the one it asked about.
//! 2. **The name was decoded lossily.** `String::from_utf8_lossy` turns a byte
//!    that is not UTF-8 into `U+FFFD`, which for a *path* is silent corruption
//!    -- the one thing this crate's rule 7 names outright.
//! 3. **`-s` was a string comparison against every argument.** Not getopt: so
//!    `tty -- -s` was silent where GNU calls it an operand, `tty -ss` and
//!    `tty -sx` were not silent where glibc clusters short options, and
//!    `--silent` and `--quiet` -- upstream's two long spellings -- did not
//!    exist at all.
//! 4. **No options.** `tty --help` printed `not a tty`.
//! 5. **An operand was accepted silently**, where GNU refuses the first with
//!    `tty: extra operand ‘x’` and exits **2**. Two, not one: `tty.c` has
//!    `enum { TTY_FAILURE = 2, TTY_WRITE_ERROR = 3 }` precisely because 1 is
//!    already spoken for as *there is no terminal*, so a caller cannot tell a
//!    usage mistake from an answer if the usage mistake also exits 1.
//! 6. **The writes went through `println!`**, which fails in both directions at
//!    once. A *closed* stdout was a silent exit 0 or 1 -- the defect that
//!    brought this file into the `coreutils::stdfd` sweep -- and a *full* one
//!    was worse: measured, `tty >/dev/full` panicked and the process died of
//!    `SIGABRT` with status 134 and a Rust backtrace note on stderr, against
//!    GNU's `tty: write error: No space left on device` and status 3.
//! 7. **The platform gate was `target_os = "linux"`**, and the other branch
//!    answered `not a tty` rather than declining. That is a false statement
//!    dressed as an answer: on a host with no `ttyname` the honest report is
//!    that the utility does not apply, which is what `id`, `whoami` and
//!    `logname` say. (The gate happened to be true on SlateOS, whose target
//!    JSON sets `"os": "linux"`, so this cost nothing shipped -- but it tied
//!    the answer to a target string rather than to whether the call exists.)
//!
//! # The three exit statuses
//!
//! Worth stating together, because two of them are unusual and the third
//! overrides the others:
//!
//! | | |
//! |---|---|
//! | `0` | there is a terminal on descriptor 0 |
//! | `1` | there is not |
//! | `2` | the command line was wrong |
//! | `3` | the answer could not be written |
//!
//! 3 wins over 0 and 1: gnulib's `close_stdout` runs from `atexit`, after the
//! status is otherwise settled, and `initialize_exit_failure (TTY_WRITE_ERROR)`
//! is what tells it to use 3. Measured -- `tty >&-` with no terminal exits 3,
//! not 1. But `tty -s >&-` exits 1, because `-s` wrote nothing and
//! `close_stream` forgives an `EBADF` with nothing pending.

// The host build stops at the `main` below that refuses to run, so everything
// the real one would have called is unreachable there. Same reason as `id`.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use std::ffi::OsString;

coreutils::guard_std_fds!();

/// `tty`'s usage status is 2 -- measured: `tty x; echo $?` prints 2. It is not
/// the family's usual 1 because 1 already means *no terminal*.
const TTY: Program = Program::new("tty", 2);

/// What a failed write exits with: `TTY_WRITE_ERROR`, passed to
/// `initialize_exit_failure` so that gnulib's `close_stdout` uses it.
const WRITE_ERROR: u8 = 3;

/// GNU `tty`'s `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "s";

/// GNU `tty`'s `longopts[]`: `silent` and `quiet` are two entries naming the
/// same flag, kept as two so that a value given to either is refused under the
/// name that was typed -- measured, `--quiet=` says `option '--quiet' doesn't
/// allow an argument` and not `'--silent'`.
///
/// No prefix is ambiguous between them, `silent` and `quiet` beginning with
/// different letters, so `--s` and `--q` each resolve.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("silent", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--quiet` is `--silent` under another name -- both carry `val == 's'` in
/// GNU's table -- and saying so changes one piece of *output*.
///
/// glibc lists an ambiguous prefix's candidates by comparing each match against
/// `pfound`, the first one, and never against each other. `--silent` is the
/// first entry here, so a prefix matching both of them drops `--quiet` from the
/// list as being the same option already named. Measured:
///
/// ```text
/// $ tty --=x
/// tty: option '--=x' is ambiguous; possibilities: '--silent' '--help' '--version'
/// ```
///
/// The empty prefix matches all four and GNU prints three. Without this map we
/// would print four -- and, less visibly, `--` followed by a prefix of both
/// would be refused where GNU resolves it, if such a prefix existed.
const ALIASES: &[(&str, &str)] = &[("quiet", "silent")];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// Report on descriptor 0. `silent` suppresses the *name* and nothing else
    /// -- the exit status is the answer either way, which is the whole point of
    /// the flag.
    Run {
        silent: bool,
    },
}

/// Upstream's usage line is `[OPTION]...`, with the ellipsis `logname`'s lacks.
fn help_text() -> String {
    "\
Usage: tty [OPTION]...
Print the file name of the terminal connected to standard input.

  -s, --silent, --quiet   print nothing, only return an exit status
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `tty`'s argv.
///
/// # Errors
///
/// An unknown option, a long option given a value it does not take, or any
/// operand at all -- only the first is named, as upstream's `argv[optind]`
/// does. Every one of them is status 2.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut silent = false;
    let mut extra: Option<OsString> = None;

    for item in TTY.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        match item? {
            // Recorded rather than refused on the spot: `--help` still wins
            // over an operand that precedes it, because upstream checks
            // `optind < argc` only after the whole scan. Measured:
            // `tty x --help` prints the help and exits 0.
            Opt::Operand(name) => {
                if extra.is_none() {
                    extra = Some(name.clone());
                }
            }
            Opt::Short(b's', _) | Opt::Long("silent" | "quiet", _) => silent = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(TTY.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(TTY.invalid_option(other)),
        }
    }

    match extra {
        Some(name) => Err(TTY.usage_referring(format!(
            "extra operand {}",
            quote(&os_bytes(name.as_os_str()))
        ))),
        None => Ok(Request::Run { silent }),
    }
}

// ------------------------------------------------------------------- unix ---

#[cfg(unix)]
mod imp {
    use super::{Request, WRITE_ERROR, help_text, parse_args};
    use coreutils::diag;
    use coreutils::stdfd::{self, Stream};
    use std::ffi::{CStr, OsString};
    use std::io::Write;
    use std::process::ExitCode;

    unsafe extern "C" {
        fn ttyname(fd: i32) -> *const std::ffi::c_char;
    }

    /// The name of the terminal on descriptor 0, or `None` where there is not
    /// one -- or where there is and it cannot be named, which upstream reports
    /// the same way and this must too.
    ///
    /// Not gated on `isatty`. `ttyname` answers both questions at once, and
    /// asking `isatty` first only creates a third outcome (*a terminal with no
    /// name*) that `tty` has no way to report.
    ///
    /// The bytes are copied out immediately: `ttyname` hands back a pointer
    /// into static storage that the next call to it may overwrite. They stay
    /// bytes, because this is a path.
    fn terminal_name() -> Option<Vec<u8>> {
        // SAFETY: `ttyname` takes a descriptor and returns either null or a
        // pointer to a NUL-terminated string in static storage, valid at least
        // until the next call to it. Nothing else runs between the call and
        // the copy.
        let name = unsafe {
            let p = ttyname(0);
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

        // Decided before the stream exists: upstream's `usage (TTY_FAILURE)`
        // reaches `atexit (close_stdout)` with nothing buffered on stdout, so
        // `tty x >&-` prints only the operand complaint and still exits 2.
        let request = match parse_args(&args) {
            Ok(request) => request,
            Err(e) => {
                diag!("tty: {e}");
                // The 2 still passes the `close_stderr` in `main`, because
                // upstream's `usage` calls `exit`, which runs the `atexit`
                // handler, which closes stderr -- so a complaint that could
                // not be delivered turns this 2 into a 3. Measured:
                // `tty x 2>/dev/full` is 3, `tty x` is 2.
                return ExitCode::from(u8::try_from(e.status).unwrap_or(2));
            }
        };

        let mut out = Stream::stdout();
        let earned = match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
                ExitCode::SUCCESS
            }
            Request::Version => {
                let _ = out.write_all(b"tty (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Run { silent } => {
                // Asked before the flag is consulted, because the status
                // depends on it either way: `-s` silences the report, not the
                // question.
                let name = terminal_name();
                if !silent {
                    match name.as_deref() {
                        Some(n) => {
                            let _ = out.write_all(n);
                            let _ = out.write_all(b"\n");
                        }
                        None => {
                            let _ = out.write_all(b"not a tty\n");
                        }
                    }
                }
                if name.is_some() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
        };

        // Not `stdfd::close_stdout`, which would report status 1: `tty`'s
        // failure status is 3, and 1 is already the answer *no terminal*. It is
        // reached even when the answer was `not a tty` -- upstream's `atexit`
        // handler runs on every exit path, and its status overrides the one the
        // run had earned.
        stdfd::close_stdout_with("tty", out, earned, WRITE_ERROR)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    coreutils::stdfd::close_stderr(imp::main(), WRITE_ERROR)
}

/// The host build exists only so `cargo test` runs on the developer machine.
///
/// There is no honest answer here: Windows has no `ttyname`, and the old
/// code's guess -- `not a tty`, unconditionally -- is a claim about a
/// descriptor it never looked at. `id`, `whoami` and `logname` say the same
/// thing for the same reason.
#[cfg(not(unix))]
fn main() {
    // `diag!` and not `eprintln!` even here, where the message is the whole of
    // what the program does: `eprintln!` panics when the write fails, and the
    // panic message then fails to print for the same reason, so `tty 2>&-`
    // would abort with 134 instead of refusing with 1.
    coreutils::diag!("tty: unix-only utility; not supported on this platform");
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
    fn a_bare_invocation_reports() {
        assert_eq!(
            parse_args(&argv(&[])).unwrap(),
            Request::Run { silent: false }
        );
    }

    #[test]
    fn every_spelling_of_the_flag_is_the_flag() {
        for spelling in ["-s", "--silent", "--quiet", "--sil", "--qu", "--s", "--q"] {
            assert_eq!(
                parse_args(&argv(&[spelling])).unwrap(),
                Request::Run { silent: true },
                "{spelling}"
            );
        }
    }

    /// Short options cluster, which the old string-comparison `-s` could not
    /// see -- measured: `tty -ss` is silent.
    #[test]
    fn the_flag_may_be_given_more_than_once() {
        assert_eq!(
            parse_args(&argv(&["-ss"])).unwrap(),
            Request::Run { silent: true }
        );
        assert_eq!(
            parse_args(&argv(&["-s", "--quiet"])).unwrap(),
            Request::Run { silent: true }
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
        assert_eq!(parse_args(&argv(&["--h"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--v"])).unwrap(), Request::Version);
    }

    /// The scan runs to the end before the operand check, so a `--help` after
    /// an operand still wins -- measured: `tty x --help` prints the help.
    #[test]
    fn help_wins_over_an_operand_that_precedes_it() {
        assert_eq!(parse_args(&argv(&["x", "--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["-s", "--help"])).unwrap(), Request::Help);
    }

    /// The defect that made `-s` a string comparison visible: after `--` it is
    /// an operand, however it is spelled -- measured: `tty -- -s` says
    /// `extra operand ‘-s’` and exits 2.
    #[test]
    fn a_double_dash_turns_the_flag_into_an_operand() {
        let e = parse_args(&argv(&["--", "-s"])).unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            e.message(),
            "extra operand \u{2018}-s\u{2019}\nTry 'tty --help' for more information."
        );
    }

    #[test]
    fn the_first_operand_is_the_one_named() {
        let e = parse_args(&argv(&["a", "b"])).unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            e.message(),
            "extra operand \u{2018}a\u{2019}\nTry 'tty --help' for more information."
        );
    }

    /// The empty word and a lone `-` are operands too, not options.
    #[test]
    fn the_empty_operand_and_a_lone_dash_are_operands() {
        for word in ["", "-"] {
            let e = parse_args(&argv(&[word])).unwrap_err();
            assert_eq!(e.status, 2, "{word:?}");
            assert!(
                e.message().starts_with("extra operand \u{2018}"),
                "{word:?}: {}",
                e.message()
            );
        }
    }

    /// `--quiet` is `--silent` again, and `--silent` is the first entry, so
    /// glibc drops it from an ambiguous prefix's candidates: four entries in
    /// the table, three in the message. Measured -- `tty --=x` prints exactly
    /// this. Without `ALIASES` the list would have four names and be wrong in
    /// user-visible output.
    #[test]
    fn the_ambiguous_list_drops_the_alias_of_the_first_entry() {
        let e = parse_args(&argv(&["--=x"])).unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            e.message(),
            "option '--=x' is ambiguous; possibilities: '--silent' '--help' '--version'\n\
             Try 'tty --help' for more information."
        );
    }

    #[test]
    fn an_unknown_option_is_refused() {
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'tty --help' for more information."
        );
        // Clustered after the flag it does know.
        let e = parse_args(&argv(&["-sx"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'tty --help' for more information."
        );
        let e = parse_args(&argv(&["--nope"])).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'tty --help' for more information."
        );
    }

    /// Refused under the spelling that was typed, which is why `silent` and
    /// `quiet` are two table entries rather than one with an alias.
    #[test]
    fn a_value_given_to_a_flag_is_refused_under_its_own_name() {
        let e = parse_args(&argv(&["--silent=1"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--silent' doesn't allow an argument\n\
             Try 'tty --help' for more information."
        );
        let e = parse_args(&argv(&["--quiet="])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--quiet' doesn't allow an argument\n\
             Try 'tty --help' for more information."
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
    fn the_help_text_is_upstreams_wording() {
        let text = help_text();
        assert!(text.starts_with("Usage: tty [OPTION]...\n"));
        assert!(
            text.contains("Print the file name of the terminal connected to standard input.\n")
        );
        assert!(
            text.contains("  -s, --silent, --quiet   print nothing, only return an exit status\n")
        );
        assert!(text.ends_with("output version information and exit\n"));
    }
}
