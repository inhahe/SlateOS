//! `printenv` — print all or part of the environment.
//!
//! ```text
//! Usage: printenv [OPTION]... [VARIABLE]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/printenv.c`. There was no `printenv` on
//! this system before this file; scripts that ask `printenv HOME` -- a common
//! way to test for a variable without the shell's `${VAR+x}` -- got `command
//! not found`.
//!
//! # What upstream does that is easy to get wrong
//!
//! All of these are observable, and all were read out of the 9.4 source rather
//! than recalled:
//!
//! * **The exit status counts names, not lines.** 0 when every name asked for
//!   was found, 1 when any was not -- so `printenv HOME NOPE` prints `$HOME`
//!   and exits 1. Usage errors and write errors are 2 (`PRINTENV_FAILURE`),
//!   because 1 already means "not found".
//! * **A name with `=` in it is never found**, even when an entry spelled
//!   exactly that way exists: `printenv a=b` is silent and exits 1.
//! * **Every matching entry is printed.** An environment can hold the same
//!   name twice (an `execve` is free to pass that), and upstream prints each
//!   one it meets rather than the first.
//! * **The empty name is never found.** Upstream's comparison loop cannot
//!   match a zero-length name, so `printenv ''` exits 1 with no output.
//! * **Options stop at the first operand** (`+` in the option string):
//!   `printenv HOME -0` looks up a variable called `-0`.
//! * **`-i` and `-u NAME` are accepted and do nothing but fail.** They are in
//!   upstream's option string -- a leftover from sharing it with `env` -- and in
//!   no case of its `switch`, so getopt takes them and `usage (2)` prints only
//!   the `Try 'printenv --help'` line, with no complaint before it. `-u` without
//!   its argument is getopt's own `option requires an argument`.
//!
//! # Bytes
//!
//! Names and values are printed as the bytes the environment holds. A variable
//! on this system may hold any byte but NUL, as a path may, and a `printenv`
//! that decoded them would print something other than what a child process
//! receives.
//!
//! # Deliberate differences from GNU
//!
//! * **`--help` omits the GNU project's ancillary block and the `USAGE_BUILTIN_WARNING`
//!   paragraph**, as every converted utility here does with the first; the
//!   second warns that a shell may have a builtin of the same name, which no
//!   shell here has.
//! * **`--version` names SlateOS.**
//! * **The environment is read through `std::env::vars_os`**, as `env` reads
//!   it, which skips an entry holding no `=` at all. Upstream would print such
//!   an entry in the no-argument listing. No launcher on this system can create
//!   one -- `setenv`, `env` and `posix_spawn`'s callers all write `NAME=VALUE`
//!   -- so the difference is unreachable rather than accepted.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use std::ffi::OsString;

coreutils::guard_std_fds!();

/// `PRINTENV_FAILURE`: the status for a bad command line and a failed write.
const PRINTENV: Program = Program::new("printenv", 2);

/// Upstream's `getopt_long` string, exactly: `+` to stop at the first operand,
/// and the `i` and `u:` it shares with `env` and handles nowhere.
const SHORT_OPTIONS: &str = "+iu:0";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("null", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// `-i`, or `-u NAME`: accepted by getopt and refused by upstream's
    /// `switch` with nothing but the pointer to `--help`.
    BareUsage,
    /// Print the environment, or the named variables, one record per line --
    /// or per NUL, with `-0`.
    Print {
        nul: bool,
        names: Vec<OsString>,
    },
}

fn help_text() -> String {
    "\
Usage: printenv [OPTION]... [VARIABLE]...
Print the values of the specified environment VARIABLE(s).
If no VARIABLE is specified, print name and value pairs for them all.

  -0, --null     end each output line with NUL, not newline
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// Parse `printenv`'s argv.
///
/// # Errors
///
/// getopt's own: an unknown option, `-u` with no argument, or a value given
/// to a long option that takes none.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut nul = false;
    let mut names = Vec::new();
    for item in PRINTENV.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'0', _) | Opt::Long("null", _) => nul = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Upstream stops at the first of these -- `usage` does not return
            // -- so a `--help` after one is never reached. Measured the same
            // way: `printenv -i --help` prints only the referral.
            Opt::Short(b'i' | b'u', _) => return Ok(Request::BareUsage),
            Opt::Operand(name) => names.push(name.clone()),
            // Unreachable: the parser yields only names from the tables, and
            // every one is handled above. Refused rather than ignored, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(PRINTENV.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(PRINTENV.invalid_option(other)),
        }
    }
    Ok(Request::Print { nul, names })
}

/// The values the environment holds for `name`, in environment order.
///
/// Every match, not the first: see the module documentation. Empty for a name
/// that is empty or contains `=`, which upstream's comparison can never match.
fn values_of<'e>(env: &'e [(Vec<u8>, Vec<u8>)], name: &[u8]) -> Vec<&'e [u8]> {
    if name.is_empty() || name.contains(&b'=') {
        return Vec::new();
    }
    env.iter()
        .filter(|(k, _)| k.as_slice() == name)
        .map(|(_, v)| v.as_slice())
        .collect()
}

/// Everything `printenv` writes for `names` -- the whole environment when
/// there are none -- and whether every name was found.
fn render(env: &[(Vec<u8>, Vec<u8>)], names: &[Vec<u8>], nul: bool) -> (Vec<u8>, bool) {
    let end = if nul { b'\0' } else { b'\n' };
    let mut out = Vec::new();
    if names.is_empty() {
        for (k, v) in env {
            out.extend_from_slice(k);
            out.push(b'=');
            out.extend_from_slice(v);
            out.push(end);
        }
        return (out, true);
    }
    let mut all_found = true;
    for name in names {
        let values = values_of(env, name);
        if values.is_empty() {
            all_found = false;
        }
        for value in values {
            out.extend_from_slice(value);
            out.push(end);
        }
    }
    (out, all_found)
}

fn main() -> std::process::ExitCode {
    use coreutils::stdfd::{self, Stream};
    use std::io::Write;
    use std::process::ExitCode;

    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            PRINTENV.report(&e);
            return stdfd::close_stderr(ExitCode::from(2), 2);
        }
    };

    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"printenv (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::BareUsage => {
            // `emit_try_help`: the referral alone, with no `printenv: ` in
            // front of it, because there is no complaint for it to prefix.
            stdfd::diag_line("Try 'printenv --help' for more information.");
            ExitCode::from(2)
        }
        Request::Print { nul, names } => {
            let env: Vec<(Vec<u8>, Vec<u8>)> = std::env::vars_os()
                .map(|(k, v)| (os_bytes(&k).into_owned(), os_bytes(&v).into_owned()))
                .collect();
            let names: Vec<Vec<u8>> = names.iter().map(|n| os_bytes(n).into_owned()).collect();
            let (bytes, all_found) = render(&env, &names, nul);
            // Deliberately unread: a failed write is `Stream`'s to remember
            // and `close_stdout_with`'s to report, once, with upstream's
            // wording and status.
            let _ = out.write_all(&bytes);
            if all_found {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    };
    stdfd::close_stdout_with("printenv", out, earned, 2)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
        pairs
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect()
    }

    fn names(ns: &[&str]) -> Vec<Vec<u8>> {
        ns.iter().map(|n| n.as_bytes().to_vec()).collect()
    }

    #[test]
    fn no_names_prints_every_entry() {
        let e = env(&[("A", "1"), ("B", "x=y")]);
        assert_eq!(render(&e, &[], false), (b"A=1\nB=x=y\n".to_vec(), true));
        assert_eq!(render(&e, &[], true), (b"A=1\0B=x=y\0".to_vec(), true));
    }

    #[test]
    fn a_found_name_prints_its_value_only() {
        let e = env(&[("HOME", "/root"), ("X", "")]);
        assert_eq!(
            render(&e, &names(&["HOME"]), false),
            (b"/root\n".to_vec(), true)
        );
        // An empty value is a found variable with an empty line, not a miss.
        assert_eq!(render(&e, &names(&["X"]), false), (b"\n".to_vec(), true));
    }

    #[test]
    fn one_missing_name_fails_the_run_but_not_the_others() {
        let e = env(&[("A", "1")]);
        assert_eq!(
            render(&e, &names(&["A", "NOPE", "A"]), false),
            (b"1\n1\n".to_vec(), false)
        );
    }

    #[test]
    fn a_name_holding_an_equals_sign_is_never_found() {
        let e = env(&[("a", "b=c")]);
        assert_eq!(render(&e, &names(&["a=b"]), false), (Vec::new(), false));
    }

    #[test]
    fn the_empty_name_is_never_found() {
        let e = env(&[("A", "1")]);
        assert_eq!(render(&e, &names(&[""]), false), (Vec::new(), false));
    }

    #[test]
    fn every_duplicate_entry_is_printed() {
        let e = env(&[("D", "first"), ("E", "e"), ("D", "second")]);
        assert_eq!(
            render(&e, &names(&["D"]), false),
            (b"first\nsecond\n".to_vec(), true)
        );
    }

    #[test]
    fn matching_is_exact_not_a_prefix() {
        let e = env(&[("PATHX", "1"), ("PAT", "2")]);
        assert_eq!(render(&e, &names(&["PATH"]), false), (Vec::new(), false));
    }

    #[test]
    fn the_null_option_and_its_long_spelling() {
        for a in [&["-0"][..], &["--null"], &["--nu"]] {
            assert_eq!(
                parse_args(&argv(a)).unwrap(),
                Request::Print {
                    nul: true,
                    names: Vec::new()
                }
            );
        }
    }

    #[test]
    fn options_stop_at_the_first_operand() {
        assert_eq!(
            parse_args(&argv(&["HOME", "-0"])).unwrap(),
            Request::Print {
                nul: false,
                names: argv(&["HOME", "-0"])
            }
        );
    }

    #[test]
    fn i_and_u_are_accepted_then_refused_with_only_the_referral() {
        assert_eq!(parse_args(&argv(&["-i"])).unwrap(), Request::BareUsage);
        assert_eq!(parse_args(&argv(&["-u", "X"])).unwrap(), Request::BareUsage);
        assert_eq!(parse_args(&argv(&["-uX"])).unwrap(), Request::BareUsage);
        assert_eq!(
            parse_args(&argv(&["-i", "--help"])).unwrap(),
            Request::BareUsage
        );
    }

    #[test]
    fn u_without_its_argument_is_getopts_complaint() {
        let e = parse_args(&argv(&["-u"])).unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            e.message(),
            "option requires an argument -- 'u'\nTry 'printenv --help' for more information."
        );
    }

    #[test]
    fn an_unknown_option_is_refused_with_status_2() {
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(e.status, 2);
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'printenv --help' for more information."
        );
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
        assert!(help_text().starts_with("Usage: printenv [OPTION]... [VARIABLE]...\n"));
    }
}
