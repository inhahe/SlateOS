//! `link` — call the link function to create a link named FILE2 to FILE1.
//!
//! ```text
//! Usage: link FILE1 FILE2
//!   or:  link OPTION
//! ```
//!
//! A port of GNU coreutils 9.4's `src/link.c`. POSIX requires the utility and
//! there was none. It is `ln` with every decision taken away: one `link(2)`,
//! no `-f` to remove what is in the way, no `-s`, no target-directory form, and
//! no treating FILE2 as a directory to put the link inside. A script uses it
//! precisely to get that one call and its one answer.
//!
//! # Symlinks are linked, not followed
//!
//! On Linux `link(2)` makes a hard link to a symlink itself rather than to
//! what it names, and upstream relies on that. `std::fs::hard_link` is
//! `linkat(AT_FDCWD, FILE1, AT_FDCWD, FILE2, 0)` -- no `AT_SYMLINK_FOLLOW` -- so
//! it does the same, as `ln` here already does.
//!
//! # Options
//!
//! gnulib's `parse_gnu_standard_options_only`, as in `unlink`: `--help`,
//! `--version`, prefixes, one `getopt_long` call.
//!
//! # Checked against GNU
//!
//! `scripts/unlink-diff.sh`, which covers both programs.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf_os};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const LINK: Program = Program::new("link", 1);

/// `parse_gnu_standard_options_only`'s table, in the order it registers them.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// `link(existing, new)`.
    Link(OsString, OsString),
}

fn help_text() -> String {
    "\
Usage: link FILE1 FILE2
  or:  link OPTION
Call the link function to create a link named FILE2 to an existing FILE1.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// gnulib's single `getopt_long` call, then upstream's operand count.
///
/// # Errors
///
/// An unknown option; fewer than two operands (`missing operand`, or `missing
/// operand after 'FILE1'`); or more than two (`extra operand`, naming the
/// third).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut operands: Vec<OsString> = Vec::new();
    for item in LINK.parse(args, "", LONG_OPTIONS) {
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the table has two names, both handled.
            Opt::Long(other, _) => {
                return Err(LINK.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(LINK.invalid_option(c)),
        }
    }
    let mut it = operands.into_iter();
    let (Some(existing), second) = (it.next(), it.next()) else {
        return Err(LINK.usage_referring("missing operand".to_string()));
    };
    let Some(new) = second else {
        return Err(LINK.usage_referring(format!(
            "missing operand after {}",
            quote(&os_bytes(&existing))
        )));
    };
    if let Some(extra) = it.next() {
        return Err(LINK.usage_referring(format!("extra operand {}", quote(&os_bytes(&extra)))));
    }
    Ok(Request::Link(existing, new))
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
            LINK.report(&e);
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
            let _ = out.write_all(b"link (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Link(existing, new) => match std::fs::hard_link(&existing, &new) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                // New name first, then the existing one: upstream's
                // `quoteaf_n (0, argv[optind + 1]), quoteaf_n (1, argv[optind])`.
                diag!(
                    "link: cannot create link {} to {}: {}",
                    quoteaf_os(&new),
                    quoteaf_os(&existing),
                    strerror(&e)
                );
                ExitCode::FAILURE
            }
        },
    };
    stdfd::close_stdout("link", out, earned)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn two_operands_are_the_existing_file_and_the_new_name() {
        assert_eq!(
            parse_args(&argv(&["a", "b"])).unwrap(),
            Request::Link("a".into(), "b".into())
        );
        assert_eq!(
            parse_args(&argv(&["--", "-a", "-b"])).unwrap(),
            Request::Link("-a".into(), "-b".into())
        );
    }

    #[test]
    fn missing_operands_are_counted_and_the_first_named() {
        let e = parse_args(&argv(&[])).unwrap_err();
        assert_eq!(
            e.message(),
            "missing operand\nTry 'link --help' for more information."
        );
        let e = parse_args(&argv(&["a"])).unwrap_err();
        assert_eq!(
            e.message(),
            "missing operand after \u{2018}a\u{2019}\nTry 'link --help' for more information."
        );
    }

    #[test]
    fn a_third_operand_is_extra() {
        let e = parse_args(&argv(&["a", "b", "c", "d"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}c\u{2019}\nTry 'link --help' for more information."
        );
    }

    #[test]
    fn the_first_option_anywhere_decides() {
        assert_eq!(
            parse_args(&argv(&["a", "--version"])).unwrap(),
            Request::Version
        );
        assert_eq!(parse_args(&argv(&["--h"])).unwrap(), Request::Help);
    }
}
