//! `kill` — send a signal to a process or a process group.
//!
//! ```text
//! kill [-s SIGNAL | -n NUMBER | -SIGNAL] PID...
//! kill -l [EXIT_STATUS...]
//! kill -L
//! ```
//!
//! Our OS uses IPC messages rather than Unix signals for process *control*,
//! but it implements POSIX signals for compatibility, so `kill` is a thin
//! wrapper over the POSIX layer's [`kill(2)`]. That layer does the real work,
//! including the whole process-group fanout; see `posix/src/signal.rs`.
//!
//! # The signal spec is the *first* argument, and only the first
//!
//! This is the rule that makes negative PIDs possible, and it is worth stating
//! because the previous implementation did not follow it. That version scanned
//! every argument and treated anything matching `-SOMETHING` as a signal, which
//! meant a PID could never be negative — and a negative PID is not an exotic
//! case, it is POSIX's spelling for "the whole process group":
//!
//! | You type | Old behaviour | Correct behaviour |
//! |---|---|---|
//! | `kill -TERM -1234` | signal silently becomes 1234, then `kill: missing PID` | `SIGTERM` to process group 1234 |
//! | `kill -9 -1234 567` | `kill(567, 1234)` — the wrong signal, to the wrong set | `SIGKILL` to group 1234 *and* to process 567 |
//!
//! The second row is the dangerous one: nothing about the outcome resembles
//! what was asked for, and nothing in the output says so. A shell script that
//! does `kill -TERM -$PGID` — the ordinary way to shut down a job — got a
//! `missing PID` error and a live process group.
//!
//! So: argument 0 may be a signal spec (or `-l`, `-L`, `-s`, `-n`, or `--`).
//! Everything after it is a PID, sign and all. `--` may also appear after a
//! signal spec, for `kill -9 -- -1`.
//!
//! # Which signals exist
//!
//! Exactly the ones this platform can name: the null signal 0, and 1 through
//! 31, matching the `SIGNAL_NAMES` table in `posix/src/signal.rs` that
//! `strsignal` reads. Real-time signals (34–64 on Linux) have no names there,
//! so `kill -l` does not claim they exist and a numeric spec in that range is
//! rejected rather than passed through to fail obscurely later. When the POSIX
//! layer names them, [`SIGNALS`] is the one place to extend.
//!
//! The previous table had twelve entries and was missing, among others,
//! `USR1` and `USR2` — which after `TERM`, `HUP` and `KILL` are the signals
//! scripts send most.

use std::env;
use std::io::{self, ErrorKind, Write};
use std::process;

use coreutils::errmsg::strerror;
use coreutils::quote::quote;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Every signal this platform names, in number order.
///
/// Kept in step with `SIGNAL_NAMES` in `posix/src/signal.rs`: a name here that
/// `strsignal` cannot describe would make `kill -l` a list of things you cannot
/// actually send.
const SIGNALS: &[(i32, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (4, "ILL"),
    (5, "TRAP"),
    (6, "ABRT"),
    (7, "BUS"),
    (8, "FPE"),
    (9, "KILL"),
    (10, "USR1"),
    (11, "SEGV"),
    (12, "USR2"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
    (16, "STKFLT"),
    (17, "CHLD"),
    (18, "CONT"),
    (19, "STOP"),
    (20, "TSTP"),
    (21, "TTIN"),
    (22, "TTOU"),
    (23, "URG"),
    (24, "XCPU"),
    (25, "XFSZ"),
    (26, "VTALRM"),
    (27, "PROF"),
    (28, "WINCH"),
    (29, "IO"),
    (30, "PWR"),
    (31, "SYS"),
];

/// Aliases: names that mean the same number as an entry in [`SIGNALS`], but
/// which `-l` must not list a second time.
///
/// `POLL` is Linux's second name for 29; `IOT` is the historical name for
/// `ABRT`. Both are accepted on input because other systems' scripts use them.
const ALIASES: &[(i32, &str)] = &[(29, "POLL"), (6, "IOT"), (6, "ABRT")];

/// The default signal, when the command line names none.
const SIGTERM: i32 = 15;

/// What the parsed argv asks for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum KillAction {
    /// `-l` / `-L` / `--list` / `--table`. With no operands, list every signal;
    /// with operands, translate each one between name and number.
    List { operands: Vec<String>, table: bool },
    /// Send `signal` to each of `pids`. A negative PID is a process group and
    /// is passed through as written.
    Send { signal: i32, pids: Vec<String> },
}

/// Look up a signal name, with or without a `SIG` prefix, in any case.
///
/// Returns `None` for a name this platform does not have, which the caller
/// turns into `invalid signal` rather than sending something arbitrary.
fn signal_by_name(token: &str) -> Option<i32> {
    let upper = token.to_uppercase();
    let bare = upper.strip_prefix("SIG").unwrap_or(&upper);
    SIGNALS
        .iter()
        .chain(ALIASES.iter())
        .find(|&&(_, n)| n == bare)
        .map(|&(num, _)| num)
}

/// The name for a signal number, without the `SIG` prefix.
fn signal_by_number(num: i32) -> Option<&'static str> {
    SIGNALS
        .iter()
        .find(|&&(n, _)| n == num)
        .map(|&(_, name)| name)
}

/// Is `num` something `kill()` can be asked to send?
///
/// 0 is included: it is the null signal, POSIX's "does this process exist and
/// may I signal it" probe, and the POSIX layer implements it as exactly that.
fn is_sendable(num: i32) -> bool {
    num == 0 || signal_by_number(num).is_some()
}

/// Resolve a signal spec — the text after `-`, or the argument to `-s`/`-n`.
///
/// Accepts a decimal number or a name. A number outside the range this
/// platform names is rejected here rather than handed to `kill()`, which would
/// fail with `EINVAL` and a diagnostic that named the PID rather than the
/// signal.
fn resolve_signal(token: &str) -> Option<i32> {
    if let Ok(n) = token.parse::<i32>() {
        return is_sendable(n).then_some(n);
    }
    signal_by_name(token)
}

/// Parse kill's argv. The error string is wordable as `kill: {e}`.
fn parse_args(args: &[String]) -> Result<KillAction, String> {
    let Some(first) = args.first() else {
        return Err("missing operand".to_string());
    };
    let rest = args.get(1..).unwrap_or(&[]);

    // Options, all of which may only appear first. `-` alone is not an option;
    // it falls through to be parsed (and rejected) as a PID, which is what GNU
    // does and what keeps the "everything after argv[0] is a PID" rule true.
    let (signal, operands): (i32, &[String]) = match first.as_str() {
        "-l" | "--list" => {
            return Ok(KillAction::List {
                operands: rest.to_vec(),
                table: false,
            });
        }
        "-L" | "--table" => {
            return Ok(KillAction::List {
                operands: rest.to_vec(),
                table: true,
            });
        }
        "--" => (SIGTERM, rest),
        "-s" | "-n" | "--signal" => {
            let Some(spec) = rest.first() else {
                return Err(format!("option {first} requires an argument"));
            };
            let Some(sig) = resolve_signal(spec) else {
                return Err(format!("{}: invalid signal", quote(spec.as_bytes())));
            };
            (sig, rest.get(1..).unwrap_or(&[]))
        }
        _ if first.starts_with("--signal=") => {
            let spec = first.strip_prefix("--signal=").unwrap_or_default();
            let Some(sig) = resolve_signal(spec) else {
                return Err(format!("{}: invalid signal", quote(spec.as_bytes())));
            };
            (sig, rest)
        }
        // Every long option this program has was matched above, so anything
        // else starting with `--` is a typo, not a signal named `-foo`.
        _ if first.starts_with("--") => {
            return Err(format!("unrecognized option {}", quote(first.as_bytes())));
        }
        _ => {
            match first.strip_prefix('-').filter(|body| !body.is_empty()) {
                Some(body) => {
                    // The whole body is tried as a signal spec *first*, so
                    // `-segv` is SIGSEGV. Only if that fails is it re-read as
                    // an attached option argument (`-s9`, `-nTERM`). getopt's
                    // rule is the other way round, which silently turns
                    // `kill -segv $$` into `kill -s egv $$` — a spelling a
                    // user can reach by accident, since this program accepts
                    // lower-case signal names everywhere else.
                    let sig = resolve_signal(body)
                        .or_else(|| body.strip_prefix('s').and_then(resolve_signal))
                        .or_else(|| body.strip_prefix('n').and_then(resolve_signal));
                    let Some(sig) = sig else {
                        return Err(format!("{}: invalid signal", quote(body.as_bytes())));
                    };
                    (sig, rest)
                }
                // No signal spec at all: every argument, this one included,
                // is a PID. (A bare `-` reaches here and is rejected later as
                // an invalid process id, which is what it is.)
                None => (SIGTERM, args),
            }
        }
    };

    // `kill -9 -- -1`: a `--` may separate the signal from the PIDs too.
    let pids = match operands.first() {
        Some(sep) if sep == "--" => operands.get(1..).unwrap_or(&[]),
        _ => operands,
    };

    if pids.is_empty() {
        return Err("missing operand".to_string());
    }

    Ok(KillAction::Send {
        signal,
        pids: pids.to_vec(),
    })
}

/// Render `-l`'s full listing: every name, space-separated, one line.
///
/// This is GNU's format — `HUP INT QUIT …` — not one signal per line. Scripts
/// read it with `$(kill -l)` and expect a word list.
fn format_signal_list() -> String {
    let names: Vec<&str> = SIGNALS.iter().map(|&(_, name)| name).collect();
    let mut s = names.join(" ");
    s.push('\n');
    s
}

/// Render `-L`'s table: `NUM) SIGNAME`, four to a line.
fn format_signal_table() -> String {
    let mut s = String::new();
    for (i, &(num, name)) in SIGNALS.iter().enumerate() {
        s.push_str(&format!("{num:>2}) SIG{name:<9}"));
        if i % 4 == 3 {
            // Trailing blanks on a padded final column are noise.
            while s.ends_with(' ') {
                s.pop();
            }
            s.push('\n');
        }
    }
    if !s.ends_with('\n') {
        while s.ends_with(' ') {
            s.pop();
        }
        s.push('\n');
    }
    s
}

/// Translate one `-l` operand.
///
/// A number becomes a name, a name becomes a number. A number of 128 or more
/// is a wait status rather than a signal — `$?` for a process killed by signal
/// *n* is `128 + n` — so 128 is subtracted first. That is the whole reason the
/// synopsis says `EXIT_STATUS` and not `SIGNAL`: the usual call is
/// `kill -l $?`.
fn translate_operand(operand: &str) -> Result<String, String> {
    if let Ok(n) = operand.parse::<i32>() {
        let sig = if n >= 128 { n.saturating_sub(128) } else { n };
        return signal_by_number(sig)
            .map(ToString::to_string)
            .ok_or_else(|| format!("{}: invalid signal", quote(operand.as_bytes())));
    }
    signal_by_name(operand)
        .map(|n| n.to_string())
        .ok_or_else(|| format!("{}: invalid signal", quote(operand.as_bytes())))
}

/// The `strerror` text for an errno that `kill()` can report.
///
/// These three are the only ones `posix::signal::kill` produces, and none of
/// them survives std's `ErrorKind` normalisation intact — `ESRCH` in
/// particular has no `ErrorKind` at all, so the generic path would print the
/// host's "Uncategorized" wording for the single most common outcome of
/// running `kill`. The previous implementation printed one fixed string,
/// `No such process or permission denied`, for every failure, which spared
/// itself the choice by refusing to make it: a script could not tell a dead
/// PID from one it lacked the authority to touch.
fn errno_text(raw: Option<i32>, err: &io::Error) -> String {
    match raw {
        Some(1) => "Operation not permitted".to_string(),
        Some(3) => "No such process".to_string(),
        Some(22) => "Invalid argument".to_string(),
        _ => strerror(err),
    }
}

/// Write `text` to stdout, treating a closed pipe as success.
///
/// `kill -l | head -1` closes the pipe under us. That is the reader's choice,
/// not our failure, and it is the one write error that must not become a
/// diagnostic — but every *other* one must, which is why this is not a bare
/// `print!`. `print!` panics on a write error, so the old code turned a full
/// disk into a panic message and a closed pipe into one too.
fn write_out(text: &str) -> i32 {
    let mut out = io::stdout().lock();
    let write = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    match write {
        Ok(()) => 0,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("kill: write error: {}", strerror(&e));
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let action = match parse_args(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("kill: {e}");
            eprintln!("Usage: kill [-s SIGNAL | -SIGNAL] PID...");
            eprintln!("       kill -l [EXIT_STATUS...]");
            process::exit(1);
        }
    };

    let status = match action {
        KillAction::List {
            operands,
            table: true,
        } if operands.is_empty() => write_out(&format_signal_table()),
        KillAction::List { operands, .. } if operands.is_empty() => {
            write_out(&format_signal_list())
        }
        KillAction::List { operands, .. } => {
            let mut status = 0;
            let mut lines = String::new();
            for operand in &operands {
                match translate_operand(operand) {
                    Ok(text) => {
                        lines.push_str(&text);
                        lines.push('\n');
                    }
                    Err(e) => {
                        eprintln!("kill: {e}");
                        status = 1;
                    }
                }
            }
            write_out(&lines).max(status)
        }
        KillAction::Send { signal, pids } => send_all(signal, &pids),
    };
    process::exit(status);
}

/// Send `signal` to every PID, reporting each failure and continuing.
///
/// Continuing matters: `kill -9 111 222` must still try 222 after 111 turns
/// out to be gone. The exit status is 1 if *any* target failed, which is what
/// a caller testing `if kill …` is asking about.
fn send_all(signal: i32, pids: &[String]) -> i32 {
    let mut status = 0;
    for pid_str in pids {
        let Ok(pid) = pid_str.parse::<i32>() else {
            eprintln!("kill: {}: invalid process id", quote(pid_str.as_bytes()));
            status = 1;
            continue;
        };
        if let Err(err) = send_one(pid, signal) {
            eprintln!(
                "kill: {}: {}",
                quote(pid_str.as_bytes()),
                errno_text(err.raw_os_error(), &err)
            );
            status = 1;
        }
    }
    status
}

/// Send one signal, or report why not.
///
/// This function is the *entire* platform-specific part of the program, and it
/// is this small on purpose. When the diagnostic-building lived inside the
/// `cfg` too, everything that shaped an error message was invisible to a build
/// on the Windows development host — including the tests for it, which is how
/// the old one-size-fits-all `No such process or permission denied` string
/// survived. Everything above and below this is compiled, and tested,
/// everywhere.
#[cfg(target_os = "linux")]
fn send_one(pid: i32, signal: i32) -> io::Result<()> {
    // SAFETY: `kill` is the POSIX layer's own function; both arguments are
    // plain integers and it dereferences nothing.
    let ret = unsafe { kill(pid, signal) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn send_one(_pid: i32, _signal: i32) -> io::Result<()> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "no signal interface on this platform",
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    /// The signal and PID list of a `Send`, or a panic naming what came back.
    fn sent(args: &[&str]) -> (i32, Vec<String>) {
        match parse_args(&s(args)).unwrap() {
            KillAction::Send { signal, pids } => (signal, pids),
            other => panic!("expected a Send, got {other:?}"),
        }
    }

    // ---------------- the table itself ----------------

    #[test]
    fn signal_numbers_match_the_posix_layer() {
        // Spot-checks against `posix/src/signal.rs`. These are the constants
        // the platform's own headers define; a `kill` that disagreed would
        // send a different signal than the name it was given.
        assert_eq!(signal_by_name("HUP"), Some(1));
        assert_eq!(signal_by_name("USR1"), Some(10));
        assert_eq!(signal_by_name("SEGV"), Some(11));
        assert_eq!(signal_by_name("USR2"), Some(12));
        assert_eq!(signal_by_name("TERM"), Some(15));
        assert_eq!(signal_by_name("CHLD"), Some(17));
        assert_eq!(signal_by_name("CONT"), Some(18));
        assert_eq!(signal_by_name("STOP"), Some(19));
        assert_eq!(signal_by_name("TSTP"), Some(20));
        assert_eq!(signal_by_name("WINCH"), Some(28));
        assert_eq!(signal_by_name("SYS"), Some(31));
    }

    #[test]
    fn the_table_is_dense_and_ordered() {
        // 1..=31 with no gaps and no repeats: the platform names all of them.
        let nums: Vec<i32> = SIGNALS.iter().map(|&(n, _)| n).collect();
        assert_eq!(nums, (1..=31).collect::<Vec<i32>>());
    }

    #[test]
    fn usr1_and_usr2_are_present() {
        // The old twelve-entry table had neither, so `kill -USR1` — a routine
        // way to ask a daemon to reopen its logs — was `unknown signal`.
        assert_eq!(resolve_signal("USR1"), Some(10));
        assert_eq!(resolve_signal("SIGUSR2"), Some(12));
    }

    // ---------------- resolve_signal ----------------

    #[test]
    fn resolve_number() {
        assert_eq!(resolve_signal("9"), Some(9));
        assert_eq!(resolve_signal("15"), Some(15));
    }

    #[test]
    fn resolve_zero_is_the_null_signal() {
        assert_eq!(resolve_signal("0"), Some(0));
    }

    #[test]
    fn resolve_name_bare_and_prefixed_and_lowercase() {
        assert_eq!(resolve_signal("KILL"), Some(9));
        assert_eq!(resolve_signal("SIGKILL"), Some(9));
        assert_eq!(resolve_signal("sigterm"), Some(15));
        assert_eq!(resolve_signal("Hup"), Some(1));
    }

    #[test]
    fn resolve_aliases() {
        assert_eq!(resolve_signal("POLL"), Some(29));
        assert_eq!(resolve_signal("IOT"), Some(6));
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert_eq!(resolve_signal("NOPE"), None);
        assert_eq!(resolve_signal("SIGNOPE"), None);
        assert_eq!(resolve_signal(""), None);
    }

    #[test]
    fn out_of_range_numbers_are_rejected_here_not_by_the_kernel() {
        // The old code accepted any integer, so `kill -1234 $$` reached
        // `kill()`, failed with EINVAL, and was reported as though the *PID*
        // were at fault.
        assert_eq!(resolve_signal("32"), None);
        assert_eq!(resolve_signal("64"), None);
        assert_eq!(resolve_signal("1234"), None);
        assert_eq!(resolve_signal("-1"), None);
    }

    // ---------------- negative PIDs: the headline fix ----------------

    #[test]
    fn a_negative_pid_is_a_process_group_not_a_signal() {
        // The whole bug in one assertion. `kill -TERM -1234` means "SIGTERM to
        // process group 1234"; the old parser made the signal 1234 and then
        // complained there was no PID.
        assert_eq!(sent(&["-TERM", "-1234"]), (15, s(&["-1234"])));
    }

    #[test]
    fn a_negative_pid_mixed_with_a_positive_one() {
        assert_eq!(sent(&["-9", "-1234", "567"]), (9, s(&["-1234", "567"])));
    }

    #[test]
    fn a_lone_leading_negative_number_is_a_signal_not_a_pid() {
        // `kill -1` has meant "send SIGHUP" for forty years, so a bare
        // leading `-N` cannot also mean "process group N" — the grammar has
        // no room for both. `--` is how you say you meant the group, which is
        // exactly why POSIX gives `kill` a `--`.
        assert!(
            parse_args(&s(&["-1234"]))
                .unwrap_err()
                .contains("invalid signal")
        );
        assert_eq!(sent(&["-1", "99"]), (1, s(&["99"])));
        assert_eq!(sent(&["--", "-1234"]), (15, s(&["-1234"])));
    }

    #[test]
    fn pid_zero_is_the_callers_own_group() {
        assert_eq!(sent(&["-HUP", "0"]), (1, s(&["0"])));
    }

    #[test]
    fn only_the_first_argument_can_be_a_signal() {
        // The old implementation scanned every argument, so this parsed as
        // "signal 9, PIDs 100 and 200" and a test asserted that it did. It is
        // the reason a negative PID was unreachable, and POSIX puts the signal
        // first precisely so the two cannot be confused.
        assert_eq!(sent(&["100", "-9", "200"]), (15, s(&["100", "-9", "200"])));
    }

    // ---------------- the `--` separator ----------------

    #[test]
    fn double_dash_alone_introduces_pids() {
        assert_eq!(sent(&["--", "-1"]), (15, s(&["-1"])));
    }

    #[test]
    fn double_dash_may_follow_a_signal() {
        assert_eq!(sent(&["-9", "--", "-1"]), (9, s(&["-1"])));
    }

    // ---------------- -s / -n / --signal ----------------

    #[test]
    fn dash_s_takes_a_separate_argument() {
        // POSIX's own spelling — `kill -s TERM pid` — and the old code read
        // the `s` as a signal name and died with `unknown signal: s`.
        assert_eq!(sent(&["-s", "TERM", "123"]), (15, s(&["123"])));
        assert_eq!(sent(&["-s", "KILL", "1", "2"]), (9, s(&["1", "2"])));
    }

    #[test]
    fn dash_s_takes_an_attached_argument() {
        assert_eq!(sent(&["-sTERM", "123"]), (15, s(&["123"])));
    }

    #[test]
    fn dash_n_takes_a_number() {
        assert_eq!(sent(&["-n", "9", "123"]), (9, s(&["123"])));
        assert_eq!(sent(&["-n9", "123"]), (9, s(&["123"])));
    }

    #[test]
    fn long_signal_option() {
        assert_eq!(sent(&["--signal", "HUP", "5"]), (1, s(&["5"])));
        assert_eq!(sent(&["--signal=HUP", "5"]), (1, s(&["5"])));
    }

    #[test]
    fn dash_s_without_an_argument_errors() {
        let err = parse_args(&s(&["-s"])).unwrap_err();
        assert!(err.contains("requires an argument"), "{err}");
    }

    #[test]
    fn dash_s_with_a_bad_signal_errors() {
        let err = parse_args(&s(&["-s", "NOPE", "1"])).unwrap_err();
        assert!(err.contains("invalid signal"), "{err}");
    }

    // ---------------- ordinary parsing ----------------

    #[test]
    fn parse_empty_errors() {
        assert!(parse_args(&s(&[])).unwrap_err().contains("missing operand"));
    }

    #[test]
    fn default_signal_is_term() {
        assert_eq!(sent(&["1234"]), (15, s(&["1234"])));
    }

    #[test]
    fn numeric_and_named_signals() {
        assert_eq!(sent(&["-9", "1234"]).0, 9);
        assert_eq!(sent(&["-KILL", "1234"]).0, 9);
        assert_eq!(sent(&["-SIGTERM", "1"]).0, 15);
        assert_eq!(sent(&["-0", "1"]).0, 0);
    }

    #[test]
    fn multiple_pids() {
        assert_eq!(sent(&["-INT", "100", "200", "300"]).1, s(&["100", "200", "300"]));
    }

    #[test]
    fn unknown_signal_errors() {
        assert!(
            parse_args(&s(&["-NOPE", "1"]))
                .unwrap_err()
                .contains("invalid signal")
        );
    }

    #[test]
    fn a_signal_with_no_pid_errors() {
        assert!(parse_args(&s(&["-9"])).unwrap_err().contains("missing operand"));
    }

    #[test]
    fn a_mistyped_long_option_is_not_a_signal() {
        let err = parse_args(&s(&["--kill", "1"])).unwrap_err();
        assert!(err.contains("unrecognized option"), "{err}");
    }

    // ---------------- -l ----------------

    #[test]
    fn dash_l_with_no_operands_lists_everything() {
        match parse_args(&s(&["-l"])).unwrap() {
            KillAction::List { operands, table } => {
                assert!(operands.is_empty());
                assert!(!table);
            }
            other => panic!("expected a List, got {other:?}"),
        }
    }

    #[test]
    fn dash_capital_l_asks_for_the_table() {
        match parse_args(&s(&["-L"])).unwrap() {
            KillAction::List { table, .. } => assert!(table),
            other => panic!("expected a List, got {other:?}"),
        }
    }

    #[test]
    fn dash_l_keeps_its_operands() {
        // The old code took `-l` as the whole command and threw away the rest,
        // so `kill -l 9` printed all twelve names instead of `KILL`.
        match parse_args(&s(&["-l", "9", "TERM"])).unwrap() {
            KillAction::List { operands, .. } => assert_eq!(operands, s(&["9", "TERM"])),
            other => panic!("expected a List, got {other:?}"),
        }
    }

    #[test]
    fn translate_number_to_name_and_back() {
        assert_eq!(translate_operand("9").unwrap(), "KILL");
        assert_eq!(translate_operand("KILL").unwrap(), "9");
        assert_eq!(translate_operand("sigterm").unwrap(), "15");
    }

    #[test]
    fn translate_subtracts_128_from_a_wait_status() {
        // `kill -l $?` after a process died of SIGKILL: `$?` is 137.
        assert_eq!(translate_operand("137").unwrap(), "KILL");
        assert_eq!(translate_operand("143").unwrap(), "TERM");
    }

    #[test]
    fn translate_rejects_what_it_cannot_name() {
        assert!(translate_operand("200").is_err());
        assert!(translate_operand("NOPE").is_err());
    }

    #[test]
    fn signal_list_is_one_space_separated_line() {
        let listing = format_signal_list();
        assert_eq!(listing.lines().count(), 1);
        let words: Vec<&str> = listing.trim_end().split(' ').collect();
        assert_eq!(words.len(), SIGNALS.len());
        assert_eq!(words.first(), Some(&"HUP"));
        assert_eq!(words.last(), Some(&"SYS"));
    }

    #[test]
    fn signal_table_names_every_signal_with_its_number() {
        let table = format_signal_table();
        for &(num, name) in SIGNALS {
            assert!(table.contains(&format!("SIG{name}")), "missing SIG{name}");
            assert!(
                table.contains(&format!("{num:>2}) SIG{name}")),
                "missing {num}) SIG{name}"
            );
        }
        assert!(table.ends_with('\n'));
        assert!(!table.contains(" \n"), "trailing blanks before a newline");
    }

    // ---------------- diagnostics ----------------

    #[test]
    fn each_errno_gets_its_own_sentence() {
        // One fixed string for all three was the old behaviour, and it made a
        // dead PID indistinguishable from one we lacked authority over.
        let any = io::Error::from(ErrorKind::Other);
        assert_eq!(errno_text(Some(1), &any), "Operation not permitted");
        assert_eq!(errno_text(Some(3), &any), "No such process");
        assert_eq!(errno_text(Some(22), &any), "Invalid argument");
    }

    #[test]
    fn an_unexpected_errno_falls_back_to_strerror() {
        let e = io::Error::from(ErrorKind::PermissionDenied);
        assert_eq!(errno_text(None, &e), "Permission denied");
    }
}
