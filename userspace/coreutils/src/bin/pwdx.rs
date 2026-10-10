//! Print the working directory of each process named: procps-ng 4.0.4's,
//! ported. Ubuntu's patches to procps leave `pwdx` alone, so this is the
//! program Ubuntu ships too.
//!
//! ```text
//! pwdx [options] pid...
//! ```
//!
//! A transcription of `src/pwdx.c`. What upstream does and this keeps:
//!
//! - A process is named by its number or by its `/proc/NUMBER` path, read as
//!   `strtol` reads it -- leading white space and a sign allowed, nothing
//!   after the digits -- and refused below 1. The first argument that is not
//!   such a number ends the program, status 1, before any later one is looked
//!   at. What the earlier ones printed is delivered first, because the
//!   refusal goes out through glibc's `error()`, which flushes standard
//!   output before it speaks.
//! - The directory is `/proc/PID/cwd`'s target, and that path is built from
//!   the argument as typed: ` 12`, `+12` and `012` pass the check and then
//!   name no process.
//! - A process that cannot be read is reported after its argument -- `No such
//!   process` for the `ENOENT` that `readlink` gives for a process that is
//!   gone or a zombie, any other failure in its own words -- and the next
//!   argument is tried; the status is 1. That report is `fprintf (stderr,
//!   …)` and not `error()`, so it does *not* flush standard output first: with
//!   both going to one place, it comes out ahead of the directories already
//!   printed but still buffered.
//! - getopt's complaints name the program as `argv[0]` has it; everything
//!   else names it by `argv[0]`'s last component
//!   (`program_invocation_short_name`, which `pwdx` also makes `error()`'s
//!   name). With no `argv[0]` at all, getopt finds nothing and the operand
//!   count comes out at -1, so there is nothing to do and the status is 0.
//! - Standard output is closed as procps' own `close_stdout` closes it: a
//!   reader that has gone is not reported, and the status stands -- see
//!   [`stdfd::close_stdout_procps`].
//!
//! # Deliberately different
//!
//! - `-V`/`--version` names this build: `pwdx from SlateOS coreutils 0.1.0`.
//! - An argument echoed back on standard error cannot rewrite the terminal:
//!   it goes through `escape_unprintable`, which leaves a printable argument
//!   as it is (`free`'s divergence 6). Upstream prints it raw. What goes to
//!   standard output -- the argument and the directory -- is the program's
//!   answer and is printed as it is.

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_bytes, os_from_bytes};
use coreutils::stdfd::{self, Stream};

/// The parser. Its name is never printed: getopt's complaints carry
/// `argv[0]`, and the referral it could add is not used.
const PWDX: Program = Program::new("pwdx", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "Vh";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[("version", Takes::Nothing), ("help", Takes::Nothing)];

/// `ENOENT`, which `pwdx` reports as `ESRCH`.
const ENOENT: i32 = 2;
/// `ESRCH`: "No such process".
const ESRCH: i32 = 3;

/// `program_invocation_short_name`: what follows `argv[0]`'s last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or(argv0)
}

/// The usage text after the program's name: the rest of `usage`'s first
/// line, then procps' `USAGE_OPTIONS`, `USAGE_HELP`, `USAGE_VERSION` and
/// `USAGE_MAN_TAIL ("pwdx(1)")`.
const USAGE_REST: &str = concat!(
    " [options] pid...\n",
    "\n",
    "Options:\n",
    " -h, --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "For more details see pwdx(1).\n",
);

/// `usage (out)`: `USAGE_HEADER`, the name, the rest, and the status `exit`
/// is then given -- 1 when it went to standard error.
fn usage(name: &[u8], to_stdout: bool, out: &mut Stream) -> u8 {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(name);
    text.extend_from_slice(USAGE_REST.as_bytes());
    if to_stdout {
        // `Stream` never fails a write: a failure is recorded for
        // `close_stdout_procps` to report, as stdio's error flag is.
        let _ = out.write_all(&text);
        0
    } else {
        // `fputs (…, stderr)`, which flushes nothing. Nothing is waiting on
        // standard output this early anyway.
        stdfd::diag_bytes_ahead_of_stdout(&text);
        1
    }
}

/// `check_pid_argument`, inverted: whether `input` -- `/proc/NUMBER` or
/// `NUMBER` -- is a process number of 1 or more, as `strtol` reads one.
fn pid_argument_is_valid(input: &[u8]) -> bool {
    let digits = input.strip_prefix(b"/proc/").unwrap_or(input);
    let (pid, used, overflow) = cstrtol::strtol_overflow(digits, 10);
    // `errno` (an overflow), no digits at all, or anything after them.
    if overflow || used == 0 || used != digits.len() {
        return false;
    }
    pid >= 1
}

/// The symlink naming the process's working directory: `/proc/%s/cwd`, or
/// `%s/cwd` for an argument that is already a `/proc` path. Every argument
/// that reaches here is one of the two, so its first byte says which.
fn cwd_link(arg: &[u8]) -> Vec<u8> {
    let mut link = if arg.first() == Some(&b'/') {
        Vec::new()
    } else {
        b"/proc/".to_vec()
    };
    link.extend_from_slice(arg);
    link.extend_from_slice(b"/cwd");
    link
}

/// `strerror (errno)`, as glibc words it.
fn strerror(errno: i32) -> String {
    coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

/// Everything `main` does before `close_stdout`, as the status it reaches.
fn run(argv: &[OsString], out: &mut Stream) -> u8 {
    let Some(argv0) = argv.first() else {
        // `argc == 0`: glibc's getopt returns -1 at once and leaves `optind`
        // at 1, so `argc -= optind` is -1 -- not the 0 that would mean usage
        // -- and the loop over the operands runs no times.
        return 0;
    };
    let argv0 = os_bytes(argv0);
    let name = short_name(&argv0);
    // Never empty-handed: `argv[0]` is there, so `1..` is in range.
    let words = argv.get(1..).unwrap_or(&[]);

    let mut operands: Vec<&OsString> = Vec::new();
    for item in PWDX.parse(words, SHORT_OPTIONS, LONG_OPTIONS) {
        match item {
            Ok(Opt::Short(b'V', _) | Opt::Long("version", _)) => {
                // `printf (PROCPS_NG_VERSION)`; see "Deliberately different".
                let mut v = name.to_vec();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                // `Stream` never fails a write; see `usage`.
                let _ = out.write_all(&v);
                return 0;
            }
            Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => return usage(name, true, out),
            Ok(Opt::Operand(o)) => operands.push(o),
            // Nothing else is in the tables.
            Ok(Opt::Short(..) | Opt::Long(..)) => return usage(name, false, out),
            Err(e) => {
                // getopt's own complaint, `argv[0]` and all, then
                // `usage (stderr)` -- not the `Try` referral.
                let mut m = argv0.to_vec();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                stdfd::diag_bytes_ahead_of_stdout(&m);
                return usage(name, false, out);
            }
        }
    }
    if operands.is_empty() {
        return usage(name, false, out);
    }

    let mut status = 0u8;
    for arg in operands {
        let arg = os_bytes(arg);
        if !pid_argument_is_valid(&arg) {
            // `xerrx (EXIT_FAILURE, …)`: glibc's `error()`, which delivers
            // standard output first, as `diag_bytes` does.
            let mut m = name.to_vec();
            m.extend_from_slice(b": invalid process id: ");
            m.extend_from_slice(escape_unprintable(&arg).as_bytes());
            m.push(b'\n');
            stdfd::diag_bytes(&m);
            return 1;
        }
        match std::fs::read_link(os_from_bytes(&cwd_link(&arg))) {
            Ok(target) => {
                let mut line = arg.to_vec();
                line.extend_from_slice(b": ");
                line.extend_from_slice(&os_bytes(target.as_os_str()));
                line.push(b'\n');
                // `Stream` never fails a write; see `usage`.
                let _ = out.write_all(&line);
            }
            Err(e) => {
                // An error with no errno cannot come from `readlink`; it is
                // given the errno the kernel would have used for a path
                // that names nothing, which is what such an error means.
                let errno = e.raw_os_error().unwrap_or(ENOENT);
                let shown = if errno == ENOENT { ESRCH } else { errno };
                status = 1;
                let mut m = escape_unprintable(&arg).into_bytes();
                m.extend_from_slice(b": ");
                m.extend_from_slice(strerror(shown).as_bytes());
                m.push(b'\n');
                stdfd::diag_bytes_ahead_of_stdout(&m);
            }
        }
    }
    status
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = Stream::stdout();
    let status = run(&argv, &mut out);
    // `close_stdout`'s `error()` names the program as every diagnostic here
    // does. With no `argv[0]` nothing was written, so it cannot speak.
    let name = argv
        .first()
        .map_or_else(Vec::new, |a| short_name(&os_bytes(a)).to_vec());
    stdfd::close_stdout_procps(&name, out, ExitCode::from(status))
}

#[cfg(test)]
mod tests {
    use super::{cwd_link, pid_argument_is_valid, run, short_name};
    use coreutils::stdfd::Stream;

    /// `argc == 0`, which Linux since 5.18 no longer lets a program see (it
    /// substitutes one empty argument) and so no harness can produce: getopt
    /// finds nothing, the operand count comes out at -1, and nothing is done.
    #[test]
    fn with_no_argv_at_all_there_is_nothing_to_do() {
        let mut out = Stream::stdout();
        assert_eq!(run(&[], &mut out), 0);
        assert!(!out.errored());
    }

    #[test]
    fn process_ids_are_read_as_strtol_reads_them() {
        assert!(pid_argument_is_valid(b"1"));
        assert!(pid_argument_is_valid(b"/proc/42"));
        assert!(pid_argument_is_valid(b" 12"), "leading white space");
        assert!(pid_argument_is_valid(b"\t\n12"), "any of C's six spaces");
        assert!(pid_argument_is_valid(b"+12"), "a sign");
        assert!(pid_argument_is_valid(b"012"), "a leading zero is decimal");
        assert!(pid_argument_is_valid(b"4294967297"), "a long, not an int");
        assert!(!pid_argument_is_valid(b"0"));
        assert!(!pid_argument_is_valid(b"-3"));
        assert!(!pid_argument_is_valid(b"-0"));
        assert!(!pid_argument_is_valid(b""));
        assert!(!pid_argument_is_valid(b" "), "no digits");
        assert!(!pid_argument_is_valid(b"+"), "a sign and no digits");
        assert!(!pid_argument_is_valid(b"12x"), "trailing bytes");
        assert!(!pid_argument_is_valid(b"12 "), "trailing white space");
        assert!(!pid_argument_is_valid(b"0x10"));
        assert!(!pid_argument_is_valid(b"/proc/"));
        assert!(!pid_argument_is_valid(b"/proc/12/"));
        assert!(!pid_argument_is_valid(b"/proc//12"), "one prefix only");
        assert!(!pid_argument_is_valid(b"proc/12"));
        assert!(!pid_argument_is_valid(b"99999999999999999999"), "overflow");
    }

    #[test]
    fn the_link_is_built_from_the_argument_as_typed() {
        assert_eq!(cwd_link(b"42"), b"/proc/42/cwd");
        assert_eq!(cwd_link(b"/proc/42"), b"/proc/42/cwd");
        assert_eq!(cwd_link(b" 42"), b"/proc/ 42/cwd");
        assert_eq!(cwd_link(b"+42"), b"/proc/+42/cwd");
    }

    #[test]
    fn the_name_is_what_follows_the_last_slash() {
        assert_eq!(short_name(b"/usr/bin/pwdx"), b"pwdx");
        assert_eq!(short_name(b"pwdx"), b"pwdx");
        assert_eq!(short_name(b"bin/"), b"");
        assert_eq!(short_name(b""), b"");
    }
}
