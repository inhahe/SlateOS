//! `kill` -- send a signal to a process: procps-ng 4.0.4's `kill.c`, ported,
//! as Ubuntu ships it at `/usr/bin/kill`.
//!
//! ```text
//! kill [options] <pid> [...]
//! kill -l [<signal>]
//! kill -L
//! ```
//!
//! It is the one `kill` (design-decisions §1072, the operator's answer to
//! B-Q22). Until 2026-10-09 there were two, and which one the image got
//! depended on the order the build linked them: this program's predecessor,
//! written here from the manual, and `userspace/kill`, which asked a service
//! that was never written to stop the process and, when that failed -- every
//! time -- ended it on the spot. So a plain `kill PID` was sometimes
//! `kill -9 PID`. Both are gone; `kill PID` is `SIGTERM` everywhere.
//! SlateOS's own ways of asking a program to stop -- *close*, which may ask
//! the user to save, and *terminate* as a message rather than a signal --
//! join this program when the lifecycle protocol exists (§1072).
//!
//! # How upstream reads its command line
//!
//! In two passes, and the first is procps' own:
//!
//! 1. `skill_sig_option` takes the **first** word anywhere after the
//!    program's name that is `-` and a signal -- `-9`, `-KILL`, `-sigterm`,
//!    `-RTMIN+3` -- out of argv. Anywhere: `kill 123 -9` sends `SIGKILL` to
//!    123, and so does `kill -- 123 -9`. Without one the signal is `SIGTERM`.
//! 2. `getopt_long` reads the rest, with `opterr = 0`, so getopt says
//!    nothing itself: a word it cannot use is `invalid argument X` and the
//!    usage, on standard error, unless what it choked on was a digit -- see
//!    the first deliberate difference.
//!
//! `-l` takes its argument attached (`-lTERM`, `--list=TERM`), or else peeks
//! at the next word, which it uses only if that does not begin with `-` --
//! without consuming it, since the program ends there. Whatever it names is
//! translated with `strtosig` (a number to a name, a name to a number, the
//! table's names only), and an unknown one is a warning with status 0. `-s`
//! takes any name `signal_name_to_number` knows; one it does not becomes the
//! signal -1, which `kill(2)` refuses for each process as `Invalid
//! argument`. `-q` sends with `sigqueue` and the value given.
//!
//! Each operand is read with `strtol_or_err` -- the first that is not a
//! number ends the run, after the ones before it were signalled -- and cut to
//! a `pid_t` as C casts it, so `4294967297` is process 1. A failure is
//! reported per process, `kill: (PID): reason`, and makes the status 1.
//!
//! # Deliberate differences
//!
//! 1. **A negative PID that is not after `--` is that process group.**
//!    Upstream's `getopt_long` reads `-1234` as the options `-1`, `-2`...,
//!    stops at the first, and upstream turns the *first digit* into the
//!    process `'0' - '1'`: so `kill -9 -1234` sends `SIGKILL` to process -1,
//!    which is **every process the caller may signal**, and then reports
//!    success as failure and failure as success (`if
//!    (!execute_kill(...)) exitvalue = EXIT_FAILURE`). Measured on Ubuntu's
//!    procps-ng 4.0.4 with the null signal, which sends nothing:
//!    `kill -0 -1234` succeeds silently with status 1, where `kill -0 --
//!    -1234` says `kill: (-1234): No such process`. Here the word is the
//!    operand it was meant to be -- a negative PID, the process group
//!    `1234`, read with `strtol_or_err` like any other -- and the run goes
//!    on with the words after it. Copying a command that kills every
//!    process is not fidelity.
//! 2. **`-l SIG9` is an unknown signal name**, as upstream's `strtosig`
//!    meant it to be: there it strips `SIG` for the digit test but parses
//!    the word with it, finds no number, and frees a pointer three bytes
//!    into its copy, which glibc answers by aborting the program.
//! 3. **`-V` names this build**: `kill from SlateOS coreutils 0.1.0`.
//! 4. **A failed write to standard output is reported**, `kill: write
//!    error: No space left on device`, status 1 -- the listings, the usage,
//!    the version. Upstream registers no `close_stdout`, so it exits as if
//!    all was well; every other procps program here does register it, and
//!    design-decisions §1071 settles the rest.
//! 5. **An empty PID or `-q` value is reported with no reason**: `kill:
//!    failed to parse argument: ''`. Upstream's `strtol_or_err` skips the
//!    conversion for an empty string and reports whatever `errno` an earlier
//!    call left behind -- `No such file or directory` on Ubuntu, from its
//!    start-up -- which is `free`'s and `vmstat`'s divergence too
//!    ([`coreutils::procps::strutils`]).
//!
//! On SlateOS `sigqueue` is not delivered yet, so `-q` reports `Function not
//! implemented` for each process, as `pkill -q` does.
//!
//! `scripts/kill-diff.sh` holds it to Ubuntu's `/usr/bin/kill`, signalling
//! only the harness's own processes, and probing everything else -- the
//! process groups, the broadcast -- with the null signal, which sends
//! nothing.

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Optopt, Program, Takes};
use coreutils::procps::signals::{
    NUMBER_OF_SIGNALS, pretty_print_signals, signal_name_to_number, signal_number_to_name,
    skill_sig_option, unix_print_signals,
};
use coreutils::procps::{scanf, strutils};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};

coreutils::guard_std_fds!();

/// The parser's name. It never prints: `opterr` is 0, and every message is
/// the program's own.
const KILL: Program = Program::new("kill", 1);

/// Upstream's option string.
const SHORTS: &str = "l::Ls:hVq:";

/// Upstream's `longopts`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("list", Takes::Optional),
    ("table", Takes::Nothing),
    ("signal", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("queue", Takes::Required),
];

/// `SIGTERM`, the signal with none named.
const SIGTERM: i32 = 15;

/// What the program asks of the system: a fake in the tests, which must
/// signal nothing.
trait System {
    /// `kill (pid, sig)`, `pid` as `kill(2)` takes it.
    fn kill(&self, pid: i32, sig: i32) -> Result<(), i32>;
    /// `sigqueue (pid, sig, value)`.
    fn sigqueue(&self, pid: i32, sig: i32, value: i32) -> Result<(), i32>;
    /// The C library's `SIGRTMIN`.
    fn rtmin(&self) -> i32;
}

/// The C library.
struct Libc;

impl System for Libc {
    fn kill(&self, pid: i32, sig: i32) -> Result<(), i32> {
        libcall::kill_any(pid, sig)
    }

    fn sigqueue(&self, pid: i32, sig: i32, value: i32) -> Result<(), i32> {
        libcall::sigqueue_any(pid, sig, value)
    }

    fn rtmin(&self) -> i32 {
        libcall::sigrtmin()
    }
}

/// The two names upstream prints under: `program_invocation_name` (argv[0]
/// as given), which `error()` -- and so `xwarnx` -- prefixes; and the short
/// name after its last `/`, which the usage and the version use.
struct Names {
    full: Vec<u8>,
    short: Vec<u8>,
}

impl Names {
    fn of(argv0: &[u8]) -> Self {
        let short = match argv0.iter().rposition(|&b| b == b'/') {
            Some(at) => argv0.get(at.saturating_add(1)..).unwrap_or_default(),
            None => argv0,
        };
        Self {
            full: argv0.to_vec(),
            short: short.to_vec(),
        }
    }

    /// `xwarnx (...)`, which is `error (0, 0, ...)`: `name: message`.
    fn warnx(&self, msg: &[u8]) {
        let mut line = self.full.clone();
        line.extend_from_slice(b": ");
        line.extend_from_slice(msg);
        line.push(b'\n');
        stdfd::diag_bytes(&line);
    }
}

/// `print_usage (out)`'s text.
fn usage_text(short: &[u8]) -> Vec<u8> {
    let mut t = b"\nUsage:\n ".to_vec();
    t.extend_from_slice(short);
    t.extend_from_slice(
        b" [options] <pid> [...]\n\
          \nOptions:\n \
          <pid> [...]            send signal to every <pid> listed\n \
          -<signal>, -s, --signal <signal>\n                        \
          specify the <signal> to be sent\n \
          -q, --queue <value>    integer value to be sent with the signal\n \
          -l, --list=[<signal>]  list all signal names, or convert one to a name\n \
          -L, --table            list all signal names in a nice table\n\
          \n -h, --help     display this help and exit\n \
          -V, --version  output version information and exit\n\
          \nFor more details see kill(1).\n",
    );
    t
}

/// Where `print_usage` writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum To {
    Out,
    Err,
}

/// `strtosig (s)`: a number becomes the table's name for it, a name the
/// table's number for it; anything else, or a number the table does not
/// have, is `None`. Case does not matter, and `SIG` may lead a name.
///
/// Upstream tests for a digit *after* `SIG` but parses the word *with* it,
/// so `SIG9` is no number; what it does next is free a pointer into the
/// middle of its copy, and abort. Here `SIG9` is simply unknown -- the
/// second deliberate difference.
fn strtosig(s: &[u8]) -> Option<Vec<u8>> {
    let upper = s.to_ascii_uppercase();
    let p = upper.strip_prefix(b"SIG").unwrap_or(&upper);
    let mut numsignal: i32 = 0;
    if p.first().is_some_and(u8::is_ascii_digit) {
        // `strtol (s, &endp, 10)` on the word as given, which must be wholly
        // a number, into an `int`: cut as C cuts it, so 4294967305 is 9.
        let (value, used) = scanf::strtol(s);
        if used == 0 || used != s.len() {
            return None;
        }
        numsignal = scanf::low_i32(value);
    }
    if numsignal != 0 {
        // `get_sigtable_num (i)` for each row: the rows hold 1 to 31.
        (1..=NUMBER_OF_SIGNALS)
            .contains(&numsignal)
            .then(|| signal_number_to_name(numsignal, 0).into_bytes())
    } else {
        (1..=NUMBER_OF_SIGNALS)
            .find(|&n| signal_number_to_name(n, 0).as_bytes() == p)
            .map(|n| n.to_string().into_bytes())
    }
}

/// The run: argv with its name first, on `sys`, writing standard output to
/// `out`. Answers the status `main` exits with.
fn run(mut argv: Vec<Vec<u8>>, sys: &dyn System, out: &mut dyn Write) -> u8 {
    let names = Names::of(argv.first().map_or(b"kill".as_slice(), Vec::as_slice));
    let rtmin = sys.rtmin();
    let usage = |to: To, out: &mut dyn Write| -> u8 {
        let text = usage_text(&names.short);
        match to {
            To::Out => {
                // `main`'s writer is a `Stream`, whose writes never fail: a
                // failure is kept for the close.
                let _ = out.write_all(&text);
                0
            }
            To::Err => {
                stdfd::diag_bytes(&text);
                1
            }
        }
    };

    if argv.len() < 2 {
        return usage(To::Err, out);
    }

    let mut signo = skill_sig_option(&mut argv, rtmin);
    if signo < 0 {
        signo = SIGTERM;
    }
    let words: Vec<OsString> = argv
        .iter()
        .skip(1)
        .map(|w| coreutils::quote::os_from_bytes(w))
        .collect();

    let mut queue: Option<i32> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut parser = KILL.parse(&words, SHORTS, LONGS).keep_going(true);
    while let Some(item) = parser.next() {
        let opt = match item {
            Ok(opt) => opt,
            Err(_) => {
                let optopt = match parser.optopt() {
                    Optopt::Short(c) => c,
                    Optopt::Long(name) => long_val(name),
                    Optopt::None => 0,
                };
                if optopt.is_ascii_digit() {
                    // The first deliberate difference: the word is a
                    // negative PID, and the walk goes on after it.
                    if let Some(word) = parser.current_word() {
                        operands.push(os_bytes(word).into_owned());
                    }
                    parser.abandon_word();
                    continue;
                }
                let mut msg = b"invalid argument ".to_vec();
                // `%c` of `optopt`, which is 0 -- a NUL byte -- for a long
                // option that names nothing.
                msg.push(optopt);
                names.warnx(&msg);
                return usage(To::Err, out);
            }
        };
        match opt {
            Opt::Operand(word) => operands.push(os_bytes(word).into_owned()),
            Opt::Short(b'l', value) | Opt::Long("list", value) => {
                let sig_option = match value {
                    Some(v) => Some(os_bytes(&v).into_owned()),
                    None => words
                        .get(parser.optind())
                        .map(|w| os_bytes(w).into_owned())
                        .filter(|w| w.first() != Some(&b'-')),
                };
                match sig_option {
                    Some(name) => match strtosig(&name) {
                        Some(converted) => {
                            let _ = out.write_all(&converted);
                            let _ = out.write_all(b"\n");
                        }
                        None => {
                            let mut msg = b"unknown signal name ".to_vec();
                            msg.extend_from_slice(&name);
                            names.warnx(&msg);
                        }
                    },
                    None => {
                        let _ = out.write_all(&unix_print_signals(rtmin));
                    }
                }
                return 0;
            }
            Opt::Short(b'L', _) | Opt::Long("table", _) => {
                let _ = out.write_all(&pretty_print_signals(rtmin));
                return 0;
            }
            Opt::Short(b's', value) | Opt::Long("signal", value) => {
                let name = value.as_ref().map(|v| os_bytes(v).into_owned());
                signo = signal_name_to_number(name.as_deref().unwrap_or_default(), rtmin);
            }
            Opt::Short(b'h', _) | Opt::Long("help", _) => return usage(To::Out, out),
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                let mut text = names.short.clone();
                text.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                let _ = out.write_all(&text);
                return 0;
            }
            Opt::Short(b'q', value) | Opt::Long("queue", value) => {
                let text = value.as_ref().map(|v| os_bytes(v).into_owned());
                let text = text.unwrap_or_default();
                match strutils::strtol(&text) {
                    // `sigval.sival_int = strtol_or_err (...)`: a `long` cut
                    // to `int`.
                    Ok(v) => queue = Some(scanf::low_i32(v)),
                    Err(fault) => {
                        error_exit(
                            &names,
                            b"must be an integer value to be passed with the signal.",
                            &text,
                            fault,
                        );
                        return 1;
                    }
                }
            }
            // Unreachable: every option in the tables has its case above.
            Opt::Short(..) | Opt::Long(..) => return usage(To::Err, out),
        }
    }

    if operands.is_empty() {
        return usage(To::Err, out);
    }

    let mut exitvalue = 0;
    for word in &operands {
        let pid = match strutils::strtol(word) {
            Ok(pid) => pid,
            Err(fault) => {
                error_exit(&names, b"failed to parse argument", word, fault);
                return 1;
            }
        };
        let sent = match queue {
            Some(value) => sys.sigqueue(scanf::low_i32(pid), signo, value),
            None => sys.kill(scanf::low_i32(pid), signo),
        };
        if let Err(errno) = sent {
            // `error (0, errno, "(%ld)", pid)`: the `long`, not the cut
            // `pid_t` the call was made with.
            let mut line = names.full.clone();
            line.extend_from_slice(format!(": ({pid}): ").as_bytes());
            line.extend_from_slice(
                coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno)).as_bytes(),
            );
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            exitvalue = 1;
        }
    }
    exitvalue
}

/// A long option's `val`, which `optopt` holds after an error about it:
/// upstream's table maps each to its short letter.
fn long_val(name: &str) -> u8 {
    match name {
        "list" => b'l',
        "table" => b'L',
        "signal" => b's',
        "help" => b'h',
        "version" => b'V',
        "queue" => b'q',
        _ => 0,
    }
}

/// `strtol_or_err`'s failure: `error (EXIT_FAILURE, errno, "%s: '%s'",
/// errmesg, str)`.
fn error_exit(names: &Names, errmesg: &[u8], text: &[u8], fault: strutils::NumFault) {
    let mut line = names.full.clone();
    line.extend_from_slice(b": ");
    line.extend_from_slice(errmesg);
    line.extend_from_slice(b": '");
    line.extend_from_slice(text);
    line.push(b'\'');
    line.extend_from_slice(fault.suffix().as_bytes());
    line.push(b'\n');
    stdfd::diag_bytes(&line);
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<Vec<u8>> = std::env::args_os()
        .map(|a| os_bytes(&a).into_owned())
        .collect();
    // Upstream's other complaints are `error ()`'s, under argv[0] as given:
    // `kill` does not set `program_invocation_name` to the short name, as
    // most procps programs do. The fourth deliberate difference speaks the
    // same way.
    let name = Names::of(argv.first().map_or(b"kill".as_slice(), Vec::as_slice)).full;
    let mut out = Stream::stdout();
    let status = run(argv, &Libc, &mut out);
    stdfd::close_stdout_procps(&name, out, ExitCode::from(status))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::{System, run, strtosig};
    use std::cell::RefCell;

    /// One send: the call, the PID, the signal, and `sigqueue`'s value.
    type Sent = (&'static str, i32, i32, Option<i32>);

    /// A system that records what it was asked to send and sends nothing.
    /// `ESRCH` for PID 999999, as for a process that does not exist.
    #[derive(Default)]
    struct Fake {
        sent: RefCell<Vec<Sent>>,
    }

    impl System for Fake {
        fn kill(&self, pid: i32, sig: i32) -> Result<(), i32> {
            self.sent.borrow_mut().push(("kill", pid, sig, None));
            if sig < 0 {
                Err(22)
            } else if pid == 999_999 {
                Err(3)
            } else {
                Ok(())
            }
        }
        fn sigqueue(&self, pid: i32, sig: i32, value: i32) -> Result<(), i32> {
            self.sent
                .borrow_mut()
                .push(("sigqueue", pid, sig, Some(value)));
            Ok(())
        }
        fn rtmin(&self) -> i32 {
            34
        }
    }

    /// The status, what was sent, and standard output.
    fn go(words: &[&str]) -> (u8, Vec<Sent>, String) {
        let sys = Fake::default();
        let mut argv = vec![b"kill".to_vec()];
        argv.extend(words.iter().map(|w| w.as_bytes().to_vec()));
        let mut out = Vec::new();
        let status = run(argv, &sys, &mut out);
        let sent = sys.sent.borrow().clone();
        (status, sent, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_pid_alone_is_sigterm() {
        let (status, sent, _) = go(&["123"]);
        assert_eq!(status, 0);
        assert_eq!(sent, vec![("kill", 123, 15, None)]);
    }

    #[test]
    fn the_signal_word_is_taken_from_anywhere() {
        assert_eq!(go(&["-9", "123"]).1, vec![("kill", 123, 9, None)]);
        assert_eq!(go(&["123", "-9"]).1, vec![("kill", 123, 9, None)]);
        assert_eq!(go(&["--", "123", "-KILL"]).1, vec![("kill", 123, 9, None)]);
        assert_eq!(go(&["-sigint", "5"]).1, vec![("kill", 5, 2, None)]);
        assert_eq!(go(&["-RTMIN+1", "5"]).1, vec![("kill", 5, 35, None)]);
        // The first one only: the second is an option getopt refuses.
        let (status, sent, _) = go(&["-9", "-HUP", "5"]);
        assert_eq!((status, sent.len()), (1, 0));
    }

    #[test]
    fn s_takes_any_name_and_an_unknown_one_is_minus_one() {
        assert_eq!(
            go(&["-s", "HUP", "5", "6"]).1,
            vec![("kill", 5, 1, None), ("kill", 6, 1, None)]
        );
        assert_eq!(go(&["--signal=USR1", "5"]).1, vec![("kill", 5, 10, None)]);
        assert_eq!(go(&["-s", "IO", "5"]).1, vec![("kill", 5, 29, None)]);
        // kill(5, -1) fails, Invalid argument, and the status is 1.
        let (status, sent, _) = go(&["-s", "FOO", "5"]);
        assert_eq!((status, sent), (1, vec![("kill", 5, -1, None)]));
    }

    #[test]
    fn a_failure_is_per_process_and_the_status_is_one() {
        let (status, sent, _) = go(&["5", "999999", "6"]);
        assert_eq!(status, 1);
        assert_eq!(sent.len(), 3, "the PIDs after the failure are signalled");
    }

    #[test]
    fn a_pid_that_is_no_number_ends_the_run_where_it_stands() {
        let (status, sent, _) = go(&["5", "abc", "6"]);
        assert_eq!(status, 1);
        assert_eq!(sent, vec![("kill", 5, 15, None)]);
        assert_eq!(go(&["abc"]).0, 1);
        assert_eq!(go(&[""]).0, 1);
    }

    #[test]
    fn a_pid_is_cut_to_a_pid_t() {
        assert_eq!(go(&["4294967297"]).1, vec![("kill", 1, 15, None)]);
    }

    /// The first deliberate difference: upstream sends to -1 -- every
    /// process -- for `kill -9 -1234`; here it is the group 1234, and the
    /// words after it are read too.
    #[test]
    fn a_negative_pid_is_its_process_group_with_or_without_dashes() {
        assert_eq!(go(&["-9", "-1234"]).1, vec![("kill", -1234, 9, None)]);
        assert_eq!(go(&["-9", "--", "-1234"]).1, vec![("kill", -1234, 9, None)]);
        assert_eq!(
            go(&["-0", "-12", "-34", "56"]).1,
            vec![
                ("kill", -12, 0, None),
                ("kill", -34, 0, None),
                ("kill", 56, 0, None)
            ]
        );
        // And never -1 unless asked for in so many words.
        assert!(go(&["-9", "-1234"]).1.iter().all(|s| s.1 != -1));
    }

    #[test]
    fn q_sends_with_sigqueue_and_its_value_cut_to_an_int() {
        assert_eq!(go(&["-q", "5", "7"]).1, vec![("sigqueue", 7, 15, Some(5))]);
        assert_eq!(
            go(&["--queue=4294967297", "-s", "USR1", "7"]).1,
            vec![("sigqueue", 7, 10, Some(1))]
        );
        let (status, sent, _) = go(&["-q", "x", "7"]);
        assert_eq!((status, sent.len()), (1, 0));
    }

    #[test]
    fn l_lists_or_translates_and_never_signals() {
        let (status, sent, out) = go(&["-l"]);
        assert_eq!((status, sent.len()), (0, 0));
        assert!(out.starts_with("HUP INT QUIT"), "{out}");
        assert_eq!(go(&["-l", "9"]).2, "KILL\n");
        assert_eq!(go(&["-lTERM"]).2, "15\n");
        assert_eq!(go(&["--list=sigkill"]).2, "9\n");
        assert_eq!(go(&["--list", "kill"]).2, "9\n");
        // The next word is used only if it does not begin with `-`.
        assert!(go(&["-l", "--", "9"]).2.starts_with("HUP"));
        // An unknown name is a warning, and the status is still 0.
        let (status, _, out) = go(&["-l", "nosuch"]);
        assert_eq!((status, out.as_str()), (0, ""));
        // `-L` is the table.
        assert!(go(&["-L"]).2.starts_with(" 1 HUP      2 INT"));
    }

    #[test]
    fn usage_errors_and_help() {
        assert_eq!(go(&[]).0, 1);
        assert_eq!(go(&["-9"]).0, 1, "a signal and no PID");
        assert_eq!(go(&["-Z", "5"]), (1, vec![], String::new()));
        assert_eq!(go(&["--nosuch", "5"]).0, 1);
        assert_eq!(go(&["-s"]).0, 1);
        let (status, _, out) = go(&["-h"]);
        assert_eq!(status, 0);
        assert!(
            out.starts_with("\nUsage:\n kill [options] <pid> [...]\n"),
            "{out}"
        );
        assert!(out.ends_with("\nFor more details see kill(1).\n"), "{out}");
        assert_eq!(go(&["-V"]).2, "kill from SlateOS coreutils 0.1.0\n");
    }

    #[test]
    fn strtosig_is_procps_but_never_frees_what_it_should_not() {
        let s = |w: &str| strtosig(w.as_bytes()).map(|v| String::from_utf8(v).unwrap());
        assert_eq!(s("9").as_deref(), Some("KILL"));
        assert_eq!(s("15").as_deref(), Some("TERM"));
        assert_eq!(s("kill").as_deref(), Some("9"));
        assert_eq!(s("SIGkill").as_deref(), Some("9"));
        assert_eq!(s("POLL").as_deref(), Some("29"));
        assert_eq!(
            s("4294967305").as_deref(),
            Some("KILL"),
            "an int, as C cuts it"
        );
        for unknown in [
            "0", "32", "SIG9", "IO", "CLD", "RTMIN", "9x", " 9", "", "SIG",
        ] {
            assert_eq!(s(unknown), None, "{unknown:?}");
        }
    }
}
