//! `env` — run a command in a modified environment, or print the environment.
//!
//! ```text
//! env [-i] [-0] [-u NAME]... [-C DIR] [NAME=VALUE]... [COMMAND [ARG]...]
//! ```
//!
//! # Why this program, of all of them, must not hold a `String`
//!
//! An environment variable on this OS is a byte string, exactly like a path:
//! the design allows every byte but `/` and NUL, and the kernel hands `environ`
//! to a new process as bytes. `env`'s entire job is to carry those bytes from
//! the command line into a child's environment and to print them back out
//! again. It is the one utility in the tree whose *subject matter* is the
//! thing that must not be transcoded.
//!
//! The previous version began:
//!
//! ```ignore
//! let args: Vec<String> = env::args().skip(1).collect();
//! ```
//!
//! and printed with `for (key, value) in env::vars()`. Both of those are
//! `unwrap()` in disguise — `std::env::args`'s iterator is literally
//! `self.inner.next().map(|s| s.into_string().unwrap())`, and `vars()` is
//! documented to panic the same way. So:
//!
//! * `env` printing an environment that contained one non-UTF-8 variable
//!   **panicked**, printing a Rust panic message instead of the environment.
//! * `env LANG=$(some byte string) prog` panicked before doing anything.
//!
//! Everything here is `OsString`/`&[u8]` end to end for that reason. See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT` for the
//! other 53 utilities with the same first line.
//!
//! # The options that were missing
//!
//! POSIX defines exactly two, `-i` and `-u`, and this program had neither.
//! `env -i prog` — the standard way to run something in a clean environment,
//! which is what a build script or a privilege boundary reaches for — was
//! parsed as "run the program named `-i`". It failed loudly rather than
//! quietly, which is the one mercy, but `env -i` not working at all is a
//! bigger hole than any single wrong answer.
//!
//! Also added: `-0`/`--null` (NUL-terminate the printed output, so a value
//! containing a newline is still unambiguous — the same reason `find -print0`
//! exists), and `-C`/`--chdir`.
//!
//! Not implemented: `-S`/`--split-string`, GNU's shebang helper. It has a
//! whole quoting grammar of its own and nothing here needs it yet; it is
//! recorded in `todo.txt`.

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};
use std::process::{self, Command, ExitStatus};

use coreutils::errmsg::strerror;
use coreutils::getopt::Program;
use coreutils::quote::{os_bytes, quote_os, quotef_os};

/// `env`'s own failures exit 125, not 1 — GNU reserves 126 and 127 for "found
/// the command but could not run it" and "could not find it", so a third
/// number is needed for "the command line was bad". A caller that tests
/// `[ $? = 127 ]` is asking a question this distinction is the answer to.
const ENV: Program = Program::new("env", 125);

/// `env` itself could not proceed.
const EXIT_CANCELED: i32 = 125;
/// The command was found but could not be invoked.
const EXIT_CANNOT_INVOKE: i32 = 126;
/// The command was not found.
const EXIT_ENOENT: i32 = 127;

/// What terminates each line of printed output.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Sep {
    Newline,
    Nul,
}

impl Sep {
    const fn byte(&self) -> u8 {
        match self {
            Sep::Newline => b'\n',
            Sep::Nul => 0,
        }
    }
}

/// A parsed command line.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Config {
    /// `-i`: start from an empty environment rather than our own.
    ignore_env: bool,
    /// `-u NAME`, in order. Applied after `-i` and before the assignments, so
    /// `env -u FOO FOO=bar` sets `FOO`, matching GNU.
    unset: Vec<OsString>,
    /// `NAME=VALUE` operands, in order.
    assign: Vec<(OsString, OsString)>,
    /// `-C DIR`: chdir before exec.
    chdir: Option<OsString>,
    /// `-0`: NUL-terminate printed output.
    sep: Sep,
    /// The command and its arguments. Empty means "print the environment".
    command: Vec<OsString>,
}

// ---------------------------------------------------------------------------
// Byte views of an `OsStr`
// ---------------------------------------------------------------------------

/// Rebuild an `OsString` from bytes.
///
/// The inverse of [`os_bytes`], and it carries the same caveat for the same
/// reason: on the target an `OsStr` *is* bytes and this round-trips exactly,
/// while on a Windows development host there is no byte view that round-trips
/// at all. That only affects the machine the tests run on, never a running
/// SlateOS — and the alternative, refusing to split an argument into a name
/// and a value without valid UTF-8, would make the program wrong on the target
/// in order to be tidy on the host.
#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(b).to_os_string()
}

#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

/// Split `NAME=VALUE` at its first `=`.
///
/// Returns `None` when there is no `=`, or when the name would be empty —
/// `=foo` is not an assignment, because there is no such variable, so GNU
/// treats it as the command. The *value* may contain further `=` signs and any
/// bytes at all; only the first separator counts.
fn split_assignment(arg: &OsStr) -> Option<(OsString, OsString)> {
    let bytes = os_bytes(arg);
    let eq = bytes.iter().position(|&b| b == b'=')?;
    if eq == 0 {
        return None;
    }
    let name = bytes.get(..eq)?;
    let value = bytes.get(eq.saturating_add(1)..)?;
    Some((os_from_bytes(name), os_from_bytes(value)))
}

// ---------------------------------------------------------------------------
// Exit status
// ---------------------------------------------------------------------------

/// The status `env` exits with when the command could not be started.
///
/// GNU distinguishes these two and scripts rely on it: 127 means "there is no
/// such command, check the spelling or the PATH", 126 means "it is there but
/// you cannot run it", which is a permissions or a format problem. The old
/// code returned 127 for both, so a non-executable file looked like a missing
/// one.
const fn spawn_failure_status(kind: ErrorKind) -> i32 {
    match kind {
        ErrorKind::NotFound => EXIT_ENOENT,
        _ => EXIT_CANNOT_INVOKE,
    }
}

/// The status `env` exits with once the command has run.
///
/// A child killed by a signal has no exit code, and the old code turned that
/// into a flat `1` via `status.code().unwrap_or(1)` — so a command killed by
/// SIGKILL was indistinguishable from one that returned failure. The shell
/// convention is `128 + signal`, which is what `$?` says when the shell runs
/// the same command without `env` in front of it; `env` must not change the
/// answer merely by being in the way.
const fn child_status(code: Option<i32>, signal: Option<i32>) -> i32 {
    match (code, signal) {
        (Some(c), _) => c,
        (None, Some(sig)) => 128_i32.saturating_add(sig),
        // Neither: nothing sensible to report, and `env` did run something.
        (None, None) => EXIT_CANNOT_INVOKE,
    }
}

#[cfg(unix)]
fn exit_status_code(status: &ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    child_status(status.code(), status.signal())
}

#[cfg(not(unix))]
fn exit_status_code(status: &ExitStatus) -> i32 {
    child_status(status.code(), None)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// A command line `env` will not run, and the status to exit with.
struct Failure {
    message: String,
    status: i32,
}

/// Parse `env`'s argv.
///
/// Option parsing stops at the first operand, GNU-style: after `FOO=bar` or a
/// command name, a `-i` is an argument to the command rather than an option to
/// `env`. That is not a simplification — `env FOO=1 prog -i` must pass `-i` to
/// `prog`, and there is no way to tell that case from `env FOO=1 -i` except by
/// the rule that options come first.
fn parse_args(args: &[OsString]) -> Result<Config, Failure> {
    let mut cfg = Config {
        ignore_env: false,
        unset: Vec::new(),
        assign: Vec::new(),
        chdir: None,
        sep: Sep::Newline,
        command: Vec::new(),
    };

    let mut i = 0usize;
    // --- options ---
    while let Some(arg) = args.get(i) {
        let bytes = os_bytes(arg);
        let Some(body) = bytes.strip_prefix(b"-") else {
            break; // an operand; options are over
        };
        if body.is_empty() {
            // A bare `-` is GNU's historical synonym for `-i`.
            cfg.ignore_env = true;
            i = i.saturating_add(1);
            continue;
        }
        if let Some(long) = body.strip_prefix(b"-") {
            if long.is_empty() {
                i = i.saturating_add(1); // `--` ends the options
                break;
            }
            i = parse_long(long, args, i, &mut cfg)?;
            continue;
        }
        i = parse_shorts(body, args, i, &mut cfg)?;
    }

    // --- NAME=VALUE operands ---
    while let Some(arg) = args.get(i) {
        let Some(pair) = split_assignment(arg) else {
            break;
        };
        cfg.assign.push(pair);
        i = i.saturating_add(1);
    }

    // --- the command, and everything after it verbatim ---
    cfg.command = args.get(i..).unwrap_or(&[]).to_vec();
    Ok(cfg)
}

/// Handle one `--long` option. Returns the index of the next argument.
fn parse_long(
    long: &[u8],
    args: &[OsString],
    i: usize,
    cfg: &mut Config,
) -> Result<usize, Failure> {
    // Split `--name=value` before matching, so the name is matched alone.
    let (name, inline) = match long.iter().position(|&b| b == b'=') {
        Some(eq) => (
            long.get(..eq).unwrap_or_default(),
            long.get(eq.saturating_add(1)..),
        ),
        None => (long, None),
    };

    /// Take `--opt=value` if present, else the next argument. `resolved` is
    /// the option's own spelling, for the diagnostic when there is neither.
    fn value(
        inline: Option<&[u8]>,
        args: &[OsString],
        i: usize,
        resolved: &str,
    ) -> Result<(OsString, usize), Failure> {
        if let Some(v) = inline {
            return Ok((os_from_bytes(v), i.saturating_add(1)));
        }
        match args.get(i.saturating_add(1)) {
            Some(v) => Ok((v.clone(), i.saturating_add(2))),
            None => Err(fail(ENV.long_missing_argument(resolved))),
        }
    }

    match name {
        b"ignore-environment" => {
            cfg.ignore_env = true;
            Ok(i.saturating_add(1))
        }
        b"null" => {
            cfg.sep = Sep::Nul;
            Ok(i.saturating_add(1))
        }
        b"unset" => {
            let (v, next) = value(inline, args, i, "unset")?;
            cfg.unset.push(v);
            Ok(next)
        }
        b"chdir" => {
            let (v, next) = value(inline, args, i, "chdir")?;
            cfg.chdir = Some(v);
            Ok(next)
        }
        // `whole` is the option exactly as typed, `--` included, because there
        // is no resolution to name instead.
        _ => {
            let mut whole = b"--".to_vec();
            whole.extend_from_slice(long);
            Err(fail(ENV.unrecognized_option(&whole)))
        }
    }
}

/// Handle a bundle of short options (`-i`, `-i0`, `-u NAME`, `-uNAME`).
fn parse_shorts(
    body: &[u8],
    args: &[OsString],
    i: usize,
    cfg: &mut Config,
) -> Result<usize, Failure> {
    for (pos, &c) in body.iter().enumerate() {
        match c {
            b'i' => cfg.ignore_env = true,
            b'0' => cfg.sep = Sep::Nul,
            b'u' | b'C' => {
                // The rest of this token is the value; if there is no rest,
                // the next argument is. `-uFOO`, `-u FOO` and `-iuFOO` all
                // reach here with the same meaning.
                let rest = body.get(pos.saturating_add(1)..).unwrap_or_default();
                let (v, next) = if rest.is_empty() {
                    match args.get(i.saturating_add(1)) {
                        Some(v) => (v.clone(), i.saturating_add(2)),
                        None => return Err(fail(ENV.short_missing_argument(c))),
                    }
                } else {
                    (os_from_bytes(rest), i.saturating_add(1))
                };
                if c == b'u' {
                    cfg.unset.push(v);
                } else {
                    cfg.chdir = Some(v);
                }
                return Ok(next);
            }
            _ => return Err(fail(ENV.invalid_option(c))),
        }
    }
    Ok(i.saturating_add(1))
}

fn fail(e: coreutils::getopt::Error) -> Failure {
    Failure {
        message: e.message(),
        status: e.status,
    }
}

// ---------------------------------------------------------------------------
// Doing it
// ---------------------------------------------------------------------------

/// The environment `cfg` describes, as name/value pairs in application order.
///
/// Returned rather than applied so the print path and the exec path build the
/// same thing from the same code — the old version applied assignments with
/// `set_var` on one path and `Command::env` on the other, which is two chances
/// to disagree about a question (does `-u FOO FOO=bar` set `FOO`?) that has
/// one answer.
fn effective_env(cfg: &Config, inherited: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = if cfg.ignore_env {
        Vec::new()
    } else {
        inherited
    };
    for name in &cfg.unset {
        out.retain(|(k, _)| k != name);
    }
    for (name, value) in &cfg.assign {
        match out.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value.clone(),
            None => out.push((name.clone(), value.clone())),
        }
    }
    out
}

/// Render an environment for printing, one `NAME=VALUE` per separator.
fn render(vars: &[(OsString, OsString)], sep: &Sep) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in vars {
        out.extend_from_slice(&os_bytes(name));
        out.push(b'=');
        out.extend_from_slice(&os_bytes(value));
        out.push(sep.byte());
    }
    out
}

/// Write to stdout, treating a closed pipe as success and anything else as the
/// failure it is. `println!` panics on a write error; `env | head -1` must not
/// produce a panic message.
fn write_out(bytes: &[u8]) -> i32 {
    let mut out = io::stdout().lock();
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("env: write error: {}", strerror(&e));
            EXIT_CANCELED
        }
    }
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("env: {}", e.message);
            process::exit(e.status);
        }
    };

    let vars = effective_env(&cfg, env::vars_os().collect());

    let Some(program) = cfg.command.first() else {
        // No command: `-C` still has to happen, because `env -C /tmp` with no
        // command is how a caller asks what the environment looks like there.
        if let Some(dir) = &cfg.chdir
            && let Err(e) = env::set_current_dir(dir)
        {
            eprintln!(
                "env: cannot change directory to {}: {}",
                quote_os(dir),
                strerror(&e)
            );
            process::exit(EXIT_CANCELED);
        }
        process::exit(write_out(&render(&vars, &cfg.sep)));
    };

    let mut cmd = Command::new(program);
    cmd.args(cfg.command.get(1..).unwrap_or(&[]));
    cmd.env_clear();
    cmd.envs(vars);
    if let Some(dir) = &cfg.chdir {
        cmd.current_dir(dir);
    }

    match cmd.status() {
        Ok(status) => process::exit(exit_status_code(&status)),
        Err(e) => {
            eprintln!("env: {}: {}", quotef_os(program), strerror(&e));
            process::exit(spawn_failure_status(e.kind()));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn o(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        items
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn cfg(args: &[&str]) -> Config {
        match parse_args(&o(args)) {
            Ok(c) => c,
            Err(e) => panic!("expected a parse, got {}", e.message),
        }
    }

    // ---------------- assignments ----------------

    #[test]
    fn assignments_then_command() {
        let c = cfg(&["FOO=bar", "ls", "-la"]);
        assert_eq!(c.assign, pairs(&[("FOO", "bar")]));
        assert_eq!(c.command, o(&["ls", "-la"]));
    }

    #[test]
    fn every_argument_an_assignment_means_print() {
        let c = cfg(&["A=1", "B=2"]);
        assert_eq!(c.assign, pairs(&[("A", "1"), ("B", "2")]));
        assert!(c.command.is_empty());
    }

    #[test]
    fn only_the_first_equals_splits() {
        let c = cfg(&["KEY=a=b=c"]);
        assert_eq!(c.assign, pairs(&[("KEY", "a=b=c")]));
    }

    #[test]
    fn an_empty_value_is_still_an_assignment() {
        let c = cfg(&["FOO="]);
        assert_eq!(c.assign, pairs(&[("FOO", "")]));
        assert!(c.command.is_empty());
    }

    #[test]
    fn a_leading_equals_is_the_command_not_an_assignment() {
        // There is no variable with an empty name to assign to.
        let c = cfg(&["=foo", "bar"]);
        assert!(c.assign.is_empty());
        assert_eq!(c.command, o(&["=foo", "bar"]));
    }

    #[test]
    fn an_assignment_after_the_command_belongs_to_the_command() {
        let c = cfg(&["FOO=bar", "ls", "BAR=baz"]);
        assert_eq!(c.assign, pairs(&[("FOO", "bar")]));
        assert_eq!(c.command, o(&["ls", "BAR=baz"]));
    }

    // ---------------- -i ----------------

    #[test]
    fn dash_i_is_an_option_not_a_program_name() {
        // The old parser had no options at all, so this ran a program called
        // `-i`. `env -i` is the standard way to get a clean environment.
        let c = cfg(&["-i", "prog"]);
        assert!(c.ignore_env);
        assert_eq!(c.command, o(&["prog"]));
    }

    #[test]
    fn a_bare_dash_means_the_same_as_dash_i() {
        assert!(cfg(&["-", "prog"]).ignore_env);
    }

    #[test]
    fn long_ignore_environment() {
        assert!(cfg(&["--ignore-environment", "prog"]).ignore_env);
    }

    #[test]
    fn ignore_env_drops_everything_inherited() {
        let c = cfg(&["-i", "PATH=/bin", "prog"]);
        let got = effective_env(&c, pairs(&[("HOME", "/root"), ("PATH", "/usr/bin")]));
        assert_eq!(got, pairs(&[("PATH", "/bin")]));
    }

    // ---------------- -u ----------------

    #[test]
    fn dash_u_separate_and_attached() {
        assert_eq!(cfg(&["-u", "FOO", "prog"]).unset, o(&["FOO"]));
        assert_eq!(cfg(&["-uFOO", "prog"]).unset, o(&["FOO"]));
        assert_eq!(cfg(&["--unset=FOO", "prog"]).unset, o(&["FOO"]));
        assert_eq!(cfg(&["--unset", "FOO", "prog"]).unset, o(&["FOO"]));
    }

    #[test]
    fn several_unsets_accumulate() {
        let c = cfg(&["-u", "A", "-u", "B", "prog"]);
        assert_eq!(c.unset, o(&["A", "B"]));
        let got = effective_env(&c, pairs(&[("A", "1"), ("B", "2"), ("C", "3")]));
        assert_eq!(got, pairs(&[("C", "3")]));
    }

    #[test]
    fn unset_then_assign_sets_it() {
        // GNU's order: `-u` first, assignments after, so this leaves FOO=bar.
        let c = cfg(&["-u", "FOO", "FOO=bar", "prog"]);
        let got = effective_env(&c, pairs(&[("FOO", "old")]));
        assert_eq!(got, pairs(&[("FOO", "bar")]));
    }

    #[test]
    fn an_assignment_replaces_rather_than_duplicates() {
        // Two entries with the same name is not an environment, it is a bug
        // that surfaces as "the variable has the wrong value, sometimes".
        let c = cfg(&["PATH=/bin", "prog"]);
        let got = effective_env(&c, pairs(&[("PATH", "/usr/bin"), ("HOME", "/root")]));
        assert_eq!(got, pairs(&[("PATH", "/bin"), ("HOME", "/root")]));
    }

    // ---------------- -0, -C, bundling ----------------

    #[test]
    fn dash_zero_selects_nul() {
        assert_eq!(cfg(&["-0"]).sep, Sep::Nul);
        assert_eq!(cfg(&["--null"]).sep, Sep::Nul);
        assert_eq!(cfg(&[]).sep, Sep::Newline);
    }

    #[test]
    fn dash_c_takes_a_directory() {
        assert_eq!(
            cfg(&["-C", "/tmp", "prog"]).chdir,
            Some(OsString::from("/tmp"))
        );
        assert_eq!(cfg(&["-C/tmp", "prog"]).chdir, Some(OsString::from("/tmp")));
        assert_eq!(
            cfg(&["--chdir=/tmp", "prog"]).chdir,
            Some(OsString::from("/tmp"))
        );
    }

    #[test]
    fn short_options_bundle() {
        let c = cfg(&["-i0", "prog"]);
        assert!(c.ignore_env);
        assert_eq!(c.sep, Sep::Nul);
        let c = cfg(&["-iuFOO", "prog"]);
        assert!(c.ignore_env);
        assert_eq!(c.unset, o(&["FOO"]));
    }

    // ---------------- option termination ----------------

    #[test]
    fn double_dash_ends_the_options() {
        // Without it there is no way to run a program actually called `-i`.
        let c = cfg(&["--", "-i"]);
        assert!(!c.ignore_env);
        assert_eq!(c.command, o(&["-i"]));
    }

    #[test]
    fn options_stop_at_the_first_operand() {
        // `-i` here is an argument to `prog`, not an option to `env`. GNU
        // stops option parsing at the first operand for exactly this reason.
        let c = cfg(&["FOO=1", "prog", "-i"]);
        assert!(!c.ignore_env);
        assert_eq!(c.command, o(&["prog", "-i"]));
    }

    #[test]
    fn a_command_that_looks_like_an_assignment_later_is_left_alone() {
        let c = cfg(&["prog", "A=1"]);
        assert!(c.assign.is_empty());
        assert_eq!(c.command, o(&["prog", "A=1"]));
    }

    // ---------------- diagnostics ----------------

    #[test]
    fn an_unknown_option_is_rejected_with_gnu_wording_and_status_125() {
        let e = parse_args(&o(&["-Z", "prog"])).unwrap_err();
        assert!(e.message.contains("invalid option -- 'Z'"), "{}", e.message);
        // 125, not 1: 126 and 127 already mean "could not run the command",
        // so `env`'s own failure needs a number of its own.
        assert_eq!(e.status, EXIT_CANCELED);
    }

    #[test]
    fn an_unknown_long_option_is_rejected() {
        let e = parse_args(&o(&["--zzz"])).unwrap_err();
        assert!(e.message.contains("unrecognized option"), "{}", e.message);
        assert!(e.message.contains("--zzz"), "{}", e.message);
    }

    #[test]
    fn an_option_missing_its_argument_is_rejected() {
        assert!(
            parse_args(&o(&["-u"]))
                .unwrap_err()
                .message
                .contains("requires an argument")
        );
        assert!(
            parse_args(&o(&["--chdir"]))
                .unwrap_err()
                .message
                .contains("requires an argument")
        );
    }

    // ---------------- exit status ----------------

    #[test]
    fn missing_and_unrunnable_commands_get_different_statuses() {
        // The old code returned 127 for both, so a file that exists but is not
        // executable was reported as one that does not exist.
        assert_eq!(spawn_failure_status(ErrorKind::NotFound), 127);
        assert_eq!(spawn_failure_status(ErrorKind::PermissionDenied), 126);
    }

    #[test]
    fn a_signalled_child_reports_128_plus_the_signal() {
        // `status.code().unwrap_or(1)` made a SIGKILLed command look like one
        // that merely returned 1. The shell says 137; `env` must not change
        // the answer by being in the way.
        assert_eq!(child_status(None, Some(9)), 137);
        assert_eq!(child_status(None, Some(15)), 143);
        assert_eq!(child_status(Some(3), None), 3);
        assert_eq!(child_status(Some(0), None), 0);
    }

    // ---------------- rendering ----------------

    #[test]
    fn render_uses_the_chosen_separator() {
        let vars = pairs(&[("A", "1"), ("B", "2")]);
        assert_eq!(render(&vars, &Sep::Newline), b"A=1\nB=2\n");
        assert_eq!(render(&vars, &Sep::Nul), b"A=1\0B=2\0");
    }

    #[test]
    fn a_value_containing_a_newline_survives() {
        // This is what `-0` is for: with newline separators the two lines are
        // indistinguishable from two variables.
        let vars = pairs(&[("A", "one\ntwo")]);
        assert_eq!(render(&vars, &Sep::Nul), b"A=one\ntwo\0");
    }

    // ---------------- bytes ----------------

    #[test]
    fn an_assignment_splits_on_bytes_not_characters() {
        // The name and the value are both byte strings on this OS. The split
        // must land on the first `=` byte and preserve everything else, which
        // is why this program never builds a `String`.
        let (name, value) = split_assignment(OsStr::new("Kü=vä=lue")).unwrap();
        assert_eq!(name, OsString::from("Kü"));
        assert_eq!(value, OsString::from("vä=lue"));
    }

    #[test]
    fn something_with_no_equals_is_not_an_assignment() {
        assert!(split_assignment(OsStr::new("plain")).is_none());
        assert!(split_assignment(OsStr::new("=novalue")).is_none());
    }
}
