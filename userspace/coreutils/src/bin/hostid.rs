//! `hostid` — print the numeric identifier for the current host.
//!
//! ```text
//! Usage: hostid [OPTION]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/hostid.c`: `gethostid()`, the low 32
//! bits, as eight hex digits. The number is the C library's to decide -- here
//! `posix`'s `gethostid`, which returns what `sethostid` stored or else a
//! value derived from the system's host name -- so this program adds only the
//! command line and the formatting.
//!
//! # Checked against GNU
//!
//! `scripts/hostid-diff.sh`, which on Linux compares glibc's answer printed by
//! both programs.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const HOSTID: Program = Program::new("hostid", 1);

/// `parse_gnu_standard_options_only`'s table.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Print,
}

fn help_text() -> String {
    "\
Usage: hostid [OPTION]
Print the numeric identifier (in hexadecimal) for the current host.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// gnulib's single `getopt_long` call, then upstream's operand check.
///
/// # Errors
///
/// An unknown option, or any operand (`extra operand`, naming the first).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut operands: Vec<OsString> = Vec::new();
    for item in HOSTID.parse(args, "", LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => operands.push(x.clone()),
            // Unreachable: the table's two names are handled above.
            Opt::Long(other, _) => {
                return Err(HOSTID.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(HOSTID.invalid_option(c)),
        }
    }
    match operands.first() {
        Some(extra) => {
            Err(HOSTID.usage_referring(format!("extra operand {}", quote(&os_bytes(extra)))))
        }
        None => Ok(Request::Print),
    }
}

/// `printf ("%08x\n", id & 0xffffffff)`: POSIX says the identifier is 32
/// bits and is silent on sign extension through `long`, so it is masked.
fn line(id: i64) -> String {
    format!("{:08x}\n", id & 0xffff_ffff)
}

#[cfg(unix)]
mod imp {
    use super::{HOSTID, Request, help_text, line, parse_args};
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::io::Write;
    use std::process::ExitCode;

    unsafe extern "C" {
        fn gethostid() -> i64;
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(e) => {
                HOSTID.report(&e);
                return ExitCode::FAILURE;
            }
        };
        let mut out = Stream::stdout();
        // Each write is deliberately unread: a failed write is `Stream`'s to
        // remember and `close_stdout`'s to report, once.
        match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
            }
            Request::Version => {
                let _ = out.write_all(b"hostid (SlateOS coreutils) 0.1.0\n");
            }
            Request::Print => {
                // SAFETY: `gethostid` takes nothing and cannot fail.
                let id = unsafe { gethostid() };
                let _ = out.write_all(line(id).as_bytes());
            }
        }
        stdfd::close_stdout("hostid", out, ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no `gethostid`.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("hostid: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn eight_hex_digits_of_the_low_word() {
        assert_eq!(line(0x007f_0101), "007f0101\n");
        assert_eq!(line(0), "00000000\n");
        // A negative `long` is not sign-extended into the output.
        assert_eq!(line(-1), "ffffffff\n");
        assert_eq!(line(0x1_2345_6789), "23456789\n");
    }

    #[test]
    fn options() {
        assert_eq!(parse_args(&argv(&[])).unwrap(), Request::Print);
        assert_eq!(parse_args(&argv(&["x", "--help"])).unwrap(), Request::Help);
        let e = parse_args(&argv(&["x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand ‘x’\nTry 'hostid --help' for more information."
        );
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'hostid --help' for more information."
        );
    }
}
