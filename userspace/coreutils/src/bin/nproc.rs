//! `nproc` — print the number of processing units available.
//!
//! ```text
//! Usage: nproc [OPTION]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/nproc.c` and of the gnulib
//! `num_processors` it prints (`lib/nproc.c`, the Linux/glibc paths).
//!
//! # What the number is
//!
//! Without `--all`, the processing units *this process* may run on:
//!
//! 1. `OMP_NUM_THREADS`, if it holds a positive number, capped by
//!    `OMP_THREAD_LIMIT` -- gnulib honours the OpenMP variables because every
//!    OpenMP program does, and a build script asking `nproc` how many jobs to
//!    start should get the answer the compiler's runtime will act on;
//! 2. otherwise the CPUs in the affinity mask (`sched_getaffinity`), or if
//!    that fails the online count (`sysconf(_SC_NPROCESSORS_ONLN)`), again
//!    capped by `OMP_THREAD_LIMIT`.
//!
//! With `--all`, the installed processors (`sysconf(_SC_NPROCESSORS_CONF)`),
//! raised to the affinity count when the configured count is 1 or 2 and the
//! mask shows more -- glibc answers 1 or 2 when `/sys` is not mounted, and
//! gnulib guarantees `--all` is never less than the plain answer. The OpenMP
//! variables do not apply to `--all`.
//!
//! `--ignore=N` subtracts N, but never below 1.
//!
//! # Replaces the `nproc` crate
//!
//! `userspace/nproc` was this program plus four personalities, of which it
//! had already lost `tty` and `logname` to the `coreutils` bins of those names
//! and has now lost `arch`, `pathchk` and `users` too; design-decisions.md
//! §1005 makes `coreutils` the one home. What was left was not GNU's `nproc`
//! either: it counted `processor` lines in `/proc/cpuinfo` and ranges in
//! `/sys/devices/system/cpu/online` instead of asking the scheduler, so a
//! process pinned to two CPUs of sixteen was told sixteen; it read
//! `OMP_NUM_THREADS` without `OMP_THREAD_LIMIT` and only as a fallback when
//! `/sys` was missing; and it printed `--help` to stderr. The crate is gone.
//!
//! # Checked against GNU
//!
//! `scripts/nproc-diff.sh`, including pinned CPU sets (`taskset`) and the
//! OpenMP variables.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::nproc::Query;
use coreutils::quote::{os_bytes, quote};
use coreutils::xnum::xdectoumax;
use std::ffi::OsString;

coreutils::guard_std_fds!();

const NPROC: Program = Program::new("nproc", 1);

/// Upstream's `longopts[]`; there are no short options.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("ignore", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Count { query: Query, ignore: u64 },
}

/// Why a command line was refused.
#[cfg_attr(test, derive(Debug))]
enum Refusal {
    /// getopt's refusal, with the `Try ... --help` line.
    Usage(getopt::Error),
    /// `--ignore`'s argument, reported as upstream's `xdectoumax` does: the
    /// message alone, status 1.
    Number(String),
}

fn help_text() -> String {
    "\
Usage: nproc [OPTION]...
Print the number of processing units available to the current process,
which may be less than the number of online processors

      --all      print the number of installed processors
      --ignore=N  if possible, exclude N processing units
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// Upstream's option loop, then its operand check.
///
/// # Errors
///
/// An unknown option; `--ignore` with an argument that is not a number (which
/// upstream reports from inside the loop, so it preempts every option after
/// it); or any operand (`extra operand`, naming the first).
fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut query = Query::CurrentOverridable;
    let mut ignore = 0u64;
    let mut first_operand: Option<OsString> = None;
    for item in NPROC.parse(args, "", LONG_OPTIONS) {
        match item.map_err(Refusal::Usage)? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Long("all", _) => query = Query::All,
            Opt::Long("ignore", value) => {
                let text = value.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
                ignore = xdectoumax(&text, 0, u64::MAX, Some(b""), "invalid number")
                    .map_err(Refusal::Number)?;
            }
            Opt::Operand(x) => {
                if first_operand.is_none() {
                    first_operand = Some(x.clone());
                }
            }
            // Unreachable: every name in the table is handled above.
            Opt::Long(other, _) => {
                return Err(Refusal::Usage(
                    NPROC.usage_referring(format!("option '--{other}' is unhandled")),
                ));
            }
            Opt::Short(c, _) => return Err(Refusal::Usage(NPROC.invalid_option(c))),
        }
    }
    if let Some(extra) = first_operand {
        return Err(Refusal::Usage(NPROC.usage_referring(format!(
            "extra operand {}",
            quote(&os_bytes(&extra))
        ))));
    }
    Ok(Request::Count { query, ignore })
}

/// Upstream's last three lines: subtract `--ignore`, but never below 1.
fn after_ignoring(count: u64, ignore: u64) -> u64 {
    if ignore < count {
        count.saturating_sub(ignore)
    } else {
        1
    }
}

#[cfg(unix)]
mod imp {
    use super::{NPROC, Refusal, Request, after_ignoring, help_text, parse_args};
    use coreutils::diag;
    use coreutils::getopt::Report;
    use coreutils::nproc;
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::io::Write;
    use std::process::ExitCode;

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(Refusal::Usage(e)) => {
                NPROC.report(&e);
                return ExitCode::FAILURE;
            }
            Err(Refusal::Number(message)) => {
                diag!("nproc: {message}");
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
                let _ = out.write_all(b"nproc (SlateOS coreutils) 0.1.0\n");
            }
            Request::Count { query, ignore } => {
                let count = nproc::now(query);
                let _ = writeln!(out, "{}", after_ignoring(count, ignore));
            }
        }
        stdfd::close_stdout("nproc", out, ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has neither `sched_getaffinity` nor these `sysconf` names.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("nproc: unix-only utility; not supported on this platform");
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
    fn ignore_subtracts_but_never_below_one() {
        assert_eq!(after_ignoring(4, 0), 4);
        assert_eq!(after_ignoring(4, 3), 1);
        assert_eq!(after_ignoring(4, 4), 1);
        assert_eq!(after_ignoring(4, u64::MAX), 1);
    }

    #[test]
    fn options() {
        assert_eq!(
            parse_args(&argv(&[])).unwrap(),
            Request::Count {
                query: Query::CurrentOverridable,
                ignore: 0
            }
        );
        assert_eq!(
            parse_args(&argv(&["--all", "--ignore=2"])).unwrap(),
            Request::Count {
                query: Query::All,
                ignore: 2
            }
        );
        assert_eq!(
            parse_args(&argv(&["--ignore", "3", "--a"])).unwrap(),
            Request::Count {
                query: Query::All,
                ignore: 3
            }
        );
        match parse_args(&argv(&["--ignore=x", "--nope"])).unwrap_err() {
            Refusal::Number(m) => assert_eq!(m, "invalid number: ‘x’"),
            Refusal::Usage(e) => panic!("{}", e.message()),
        }
        match parse_args(&argv(&["x"])).unwrap_err() {
            Refusal::Usage(e) => assert_eq!(
                e.message(),
                "extra operand ‘x’\nTry 'nproc --help' for more information."
            ),
            Refusal::Number(m) => panic!("{m}"),
        }
        match parse_args(&argv(&["-a"])).unwrap_err() {
            Refusal::Usage(e) => assert_eq!(
                e.message(),
                "invalid option -- 'a'\nTry 'nproc --help' for more information."
            ),
            Refusal::Number(m) => panic!("{m}"),
        }
    }
}
