//! `arch` — print machine architecture.
//!
//! ```text
//! Usage: arch [OPTION]...
//! ```
//!
//! A port of GNU coreutils 9.4's `arch`, which is not a file of its own:
//! upstream builds it from `src/uname.c` with `uname_mode = UNAME_ARCH`, so it
//! is `uname -m` with no options to choose anything else. The machine name
//! comes from [`coreutils::utsname::MACHINE`], the constant `uname -m` prints,
//! so the two cannot disagree here either.
//!
//! # Replaces `nproc`'s `arch` personality
//!
//! `userspace/nproc` answered to `arch` by reading its own `argv[0]`, and
//! nothing ever started it under that name, so the command did not exist. Its
//! answer was also a guess of its own -- `/proc/cpuinfo` was searched for the
//! string `x86_64`, with `x86_64` as the fallback either way -- rather than
//! `uname -m`'s. The branch is gone; this is the one program with the name.
//!
//! # Options
//!
//! Upstream's `arch_long_options`, read by a full `getopt_long` loop with no
//! short options: every option on the line is looked at, in order, before the
//! operand count is. So `arch x --help` prints the help, `arch --nope x`
//! reports the unknown option rather than the operand, and `arch x` is
//! `extra operand 'x'`.
//!
//! # Checked against GNU
//!
//! `scripts/arch-diff.sh`, which also compares the answer with `uname -m`.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::utsname::MACHINE;
use std::ffi::OsString;

coreutils::guard_std_fds!();

const ARCH: Program = Program::new("arch", 1);

/// Upstream's `arch_long_options`; there are no short options.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Print,
}

fn help_text() -> String {
    "\
Usage: arch [OPTION]...
Print machine architecture.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// `decode_switches` in arch mode: the whole option loop, then the operand
/// count.
///
/// # Errors
///
/// An unknown option, or any operand at all (`extra operand`, naming the
/// first).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut first_operand: Option<OsString> = None;
    for item in ARCH.parse(args, "", LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => {
                if first_operand.is_none() {
                    first_operand = Some(x.clone());
                }
            }
            // Unreachable: the table's two names are handled above.
            Opt::Long(other, _) => {
                return Err(ARCH.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(ARCH.invalid_option(c)),
        }
    }
    match first_operand {
        Some(extra) => Err(ARCH.usage_referring(format!("extra operand {}", quote(&os_bytes(&extra))))),
        None => Ok(Request::Print),
    }
}

/// What `arch` prints: the machine name and a newline.
fn line() -> Vec<u8> {
    let mut out = MACHINE.to_vec();
    out.push(b'\n');
    out
}

fn main() -> std::process::ExitCode {
    use coreutils::stdfd::{self, Stream};
    use std::io::Write;
    use std::process::ExitCode;

    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            ARCH.report(&e);
            return stdfd::close_stderr(ExitCode::FAILURE, 1);
        }
    };
    let mut out = Stream::stdout();
    // Each write is deliberately unread: a failed write is `Stream`'s to
    // remember and `close_stdout`'s to report, once, as upstream's
    // `atexit (close_stdout)` does.
    match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
        }
        Request::Version => {
            let _ = out.write_all(b"arch (SlateOS coreutils) 0.1.0\n");
        }
        Request::Print => {
            let _ = out.write_all(&line());
        }
    }
    stdfd::close_stdout("arch", out, ExitCode::SUCCESS)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_prints_the_machine() {
        assert_eq!(parse_args(&argv(&[])).unwrap(), Request::Print);
        assert_eq!(line(), b"x86_64\n");
    }

    #[test]
    fn the_answer_is_the_one_uname_m_prints() {
        // Not a second constant: the same one, by construction.
        assert_eq!(&line()[..MACHINE.len()], MACHINE);
    }

    #[test]
    fn an_operand_is_an_extra_operand_after_every_option_is_seen() {
        let e = parse_args(&argv(&["x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand ‘x’\nTry 'arch --help' for more information."
        );
        // The loop reaches `--help` past the operand.
        assert_eq!(parse_args(&argv(&["x", "--help"])).unwrap(), Request::Help);
        // ...and an unknown option past it is reported instead of the operand.
        let e = parse_args(&argv(&["x", "--nope"])).unwrap_err();
        assert!(e.message().starts_with("unrecognized option '--nope'"), "{}", e.message());
        // The first operand is the one named.
        let e = parse_args(&argv(&["--", "a", "b"])).unwrap_err();
        assert!(e.message().starts_with("extra operand ‘a’"), "{}", e.message());
    }

    #[test]
    fn there_are_no_short_options() {
        let e = parse_args(&argv(&["-m"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'm'\nTry 'arch --help' for more information."
        );
    }

    #[test]
    fn version_and_prefixes() {
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
        assert_eq!(parse_args(&argv(&["--h"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--v"])).unwrap(), Request::Version);
    }
}
