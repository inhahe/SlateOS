//! `unlink` — call the unlink function to remove the specified file.
//!
//! ```text
//! Usage: unlink FILE
//!   or:  unlink OPTION
//! ```
//!
//! A port of GNU coreutils 9.4's `src/unlink.c`. POSIX requires the utility,
//! and there was none: a script that wanted exactly one `unlink(2)` -- no
//! recursion, no prompting, no `-f` swallowing the error, which is the whole
//! reason to call `unlink` instead of `rm` -- got `command not found`.
//!
//! # What it does, exactly
//!
//! One `unlink(2)` on the operand, and nothing else. That is the point of the
//! program, so none of `rm`'s policy is here: no `-i`, no refusal to remove
//! `.` or `..` by name (the kernel refuses those), no directory check (the
//! kernel answers `EISDIR` or `EPERM` itself, and that answer is the message).
//!
//! # Options
//!
//! gnulib's `parse_gnu_standard_options_only`: `--help`, `--version`, their
//! unambiguous prefixes, and nothing short. It calls `getopt_long` once, so the
//! first option anywhere on the line decides -- `unlink FILE --help` prints the
//! help -- and a line with no option at all is operands only.
//!
//! # Checked against GNU
//!
//! `scripts/unlink-diff.sh`, which also covers `link`.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf_os};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const UNLINK: Program = Program::new("unlink", 1);

/// `parse_gnu_standard_options_only`'s table, in the order it registers them.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Unlink(OsString),
}

fn help_text() -> String {
    "\
Usage: unlink FILE
  or:  unlink OPTION
Call the unlink function to remove the specified FILE.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// gnulib's single `getopt_long` call, then upstream's operand count.
///
/// # Errors
///
/// An unknown option; no operand (`missing operand`); or more than one
/// (`extra operand`, naming the second).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut operands: Vec<OsString> = Vec::new();
    for item in UNLINK.parse(args, "", LONG_OPTIONS) {
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the table has two names, both handled.
            Opt::Long(other, _) => {
                return Err(UNLINK.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(UNLINK.invalid_option(c)),
        }
    }
    let mut it = operands.into_iter();
    let Some(file) = it.next() else {
        return Err(UNLINK.usage_referring("missing operand".to_string()));
    };
    if let Some(extra) = it.next() {
        return Err(UNLINK.usage_referring(format!("extra operand {}", quote(&os_bytes(&extra)))));
    }
    Ok(Request::Unlink(file))
}

fn main() -> std::process::ExitCode {
    use coreutils::diag;
    use coreutils::stdfd::{self, Stream};
    use std::io::Write;
    use std::process::ExitCode;

    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            UNLINK.report(&e);
            return stdfd::close_stderr(ExitCode::FAILURE, 1);
        }
    };
    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"unlink (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        // `std::fs::remove_file` is exactly `unlink(2)` on a unix target --
        // the one call upstream makes, with no retry and no fallback.
        Request::Unlink(file) => match std::fs::remove_file(&file) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                diag!(
                    "unlink: cannot unlink {}: {}",
                    quoteaf_os(&file),
                    strerror(&e)
                );
                ExitCode::FAILURE
            }
        },
    };
    stdfd::close_stdout("unlink", out, earned)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn one_operand_is_the_file() {
        assert_eq!(
            parse_args(&argv(&["f"])).unwrap(),
            Request::Unlink("f".into())
        );
        // A lone `-` is a file called `-`, and `--` makes anything one.
        assert_eq!(
            parse_args(&argv(&["-"])).unwrap(),
            Request::Unlink("-".into())
        );
        assert_eq!(
            parse_args(&argv(&["--", "-x"])).unwrap(),
            Request::Unlink("-x".into())
        );
    }

    #[test]
    fn no_operand_is_missing_and_a_second_is_extra() {
        let e = parse_args(&argv(&[])).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "missing operand\nTry 'unlink --help' for more information."
        );
        let e = parse_args(&argv(&["a", "b", "c"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}b\u{2019}\nTry 'unlink --help' for more information."
        );
    }

    #[test]
    fn the_first_option_anywhere_decides() {
        assert_eq!(parse_args(&argv(&["f", "--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--vers"])).unwrap(), Request::Version);
        let e = parse_args(&argv(&["f", "-x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'unlink --help' for more information."
        );
    }
}
