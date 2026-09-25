//! `users` — print the user names of users currently logged in.
//!
//! ```text
//! Usage: users [OPTION]... [FILE]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/users.c`. The records are read by the
//! shared `utmpfile` crate -- the one `who`, `last`, `finger` and `uptime`
//! read them with -- so this file holds only what `users` does with them.
//!
//! # What it prints
//!
//! One word per *session*, not per person: a user logged in twice is listed
//! twice, which is what makes `users | wc -w` a session count. A record counts
//! if it is a `USER_PROCESS` with a non-empty name; the name loses trailing
//! spaces (gnulib's `extract_trimmed_name`); the list is sorted by byte value
//! (`strcmp`) and printed on one line. With nobody logged in, nothing at all is
//! printed -- not even the newline.
//!
//! With no FILE, `/var/run/utmp` is read and a session whose process no longer
//! exists (`kill(pid, 0)` failing with `ESRCH`) is left out, since a crashed
//! login leaves its record behind. A FILE named on the command line is taken
//! as it is, dead sessions included -- `users /var/log/wtmp` is history.
//!
//! # A file that cannot be read is not a file with nobody in it
//!
//! GNU on Linux reads through glibc's `utmpxname`/`getutxent`, which cannot
//! report a failure: a FILE that does not exist, cannot be opened, or is a
//! directory yields no records, and `users` prints nothing and exits 0 --
//! "nobody is logged in", which is not what happened. gnulib's own reader,
//! the one GNU uses where it reads the file itself as this does, reports it:
//! a named FILE it cannot open or read is `users: FILE: <reason>`, status 1,
//! and only a *missing* default utmp counts as no sessions. That is what this
//! does, with the default file read by `optionalfile`, so that one which is
//! there but unreadable is an error too rather than an empty machine. Against
//! GNU on glibc those cases differ, on purpose.
//!
//! A torn record at the end of the file is dropped, as both readers drop it.
//!
//! # Replaces `nproc`'s `users` personality
//!
//! `userspace/nproc` answered to `users` by reading its own `argv[0]`, and
//! nothing ever started it under that name, so the command did not exist. What
//! it would have printed was not the logged-in users: it read `/var/log/wtmp`
//! (every login since the file was created) instead of utmp, took each name
//! from byte 8 -- which is `ut_line`, the terminal, so a session on `pts/0`
//! reported a user called `pts/0` -- and removed duplicates, which turns a
//! session count into a head count. The branch is gone; this is the one
//! program with the name.
//!
//! # Checked against GNU
//!
//! `scripts/users-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use std::ffi::OsString;
use utmpfile::{Record, USER_PROCESS};

coreutils::guard_std_fds!();

const USERS: Program = Program::new("users", 1);

/// `parse_gnu_standard_options_only`'s table.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// glibc's `_PATH_UTMP`, which gnulib's `UTMP_FILE` is.
const UTMP_FILE: &str = "/var/run/utmp";

/// glibc's `_PATH_WTMP`, named only in the help.
const WTMP_FILE: &str = "/var/log/wtmp";

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The file named, or `None` for the live utmp.
    List(Option<OsString>),
}

fn help_text() -> String {
    format!(
        "\
Usage: users [OPTION]... [FILE]
Output who is currently logged in according to FILE.
If FILE is not specified, use {UTMP_FILE}.  {WTMP_FILE} as FILE is common.

      --help        display this help and exit
      --version     output version information and exit
"
    )
}

/// gnulib's single `getopt_long` call, then upstream's operand count.
///
/// # Errors
///
/// An unknown option, or a second FILE (`extra operand`, naming it).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut operands: Vec<OsString> = Vec::new();
    for item in USERS.parse(args, "", LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => operands.push(x.clone()),
            // Unreachable: the table's two names are handled above.
            Opt::Long(other, _) => {
                return Err(USERS.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(USERS.invalid_option(c)),
        }
    }
    let mut it = operands.into_iter();
    let file = it.next();
    if let Some(extra) = it.next() {
        return Err(USERS.usage_referring(format!("extra operand {}", quote(&os_bytes(&extra)))));
    }
    Ok(Request::List(file))
}

/// gnulib's `IS_USER_PROCESS`: a live-session record with a name in it.
fn is_user_process(record: &Record) -> bool {
    record.record_type == USER_PROCESS && !record.user.is_empty()
}

/// gnulib's `extract_trimmed_name`: the name less any trailing spaces.
fn trimmed_name(record: &Record) -> &[u8] {
    let keep = record
        .user
        .iter()
        .rposition(|&b| b != b' ')
        .map_or(0, |last| last.saturating_add(1));
    record.user.get(..keep).unwrap_or_default()
}

/// Upstream's `list_entries_users`: the line `users` prints for `records`,
/// keeping a session only if `still_running(pid)` says so. Empty when there is
/// nobody, rather than a lone newline.
fn user_line(records: &[Record], still_running: impl Fn(i32) -> bool) -> Vec<u8> {
    let mut names: Vec<&[u8]> = records
        .iter()
        .filter(|r| is_user_process(r))
        // gnulib's `READ_UTMP_CHECK_PIDS` asks only about a positive pid.
        .filter(|r| r.pid <= 0 || still_running(r.pid))
        .map(trimmed_name)
        .collect();
    // `qsort` with `strcmp`: byte order, which is `[u8]`'s `Ord`.
    names.sort_unstable();
    let mut line = names.join(&b' ');
    // Upstream ends the last name with the newline, so no name means no
    // newline -- but a name that trimmed to nothing still gets one.
    if !names.is_empty() {
        line.push(b'\n');
    }
    line
}

#[cfg(unix)]
mod imp {
    use super::{Request, USERS, UTMP_FILE, help_text, parse_args, user_line};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::quotef_os;
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::path::Path;
    use std::process::ExitCode;

    /// gnulib's `READ_UTMP_CHECK_PIDS` test, inverted: a session is dropped
    /// only when `kill(pid, 0)` says there is no such process. `EPERM` means it
    /// exists and is somebody else's, and anything else is not proof of death.
    fn still_running(pid: i32) -> bool {
        /// `ESRCH`: 3 on Linux, and `posix::errno::ESRCH` is 3.
        const ESRCH: i32 = 3;

        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        // SAFETY: signal 0 performs the existence and permission checks and
        // delivers nothing. The call takes two integers and touches no memory
        // of ours.
        if unsafe { kill(pid, 0) } == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() != Some(ESRCH)
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(e) => {
                USERS.report(&e);
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
                let _ = out.write_all(b"users (SlateOS coreutils) 0.1.0\n");
            }
            Request::List(file) => {
                let (path, check_pids) = match file {
                    None => (OsString::from(UTMP_FILE), true),
                    Some(f) => (f, false),
                };
                let read = if check_pids {
                    // The live utmp: a system that has none has nobody logged
                    // in, but one that is there and cannot be read does not.
                    optionalfile::read_bytes_or_empty(Path::new(&path))
                } else {
                    // A file named on the command line must be readable, as
                    // gnulib's own reader requires.
                    std::fs::read(&path)
                };
                let data = match read {
                    Ok(data) => data,
                    Err(e) => {
                        diag!("users: {}: {}", quotef_os(&path), strerror(&e));
                        return ExitCode::FAILURE;
                    }
                };
                let records = utmpfile::parse(&data);
                let line = user_line(&records, |pid| !check_pids || still_running(pid));
                let _ = out.write_all(&line);
            }
        }
        stdfd::close_stdout("users", out, ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no utmp and no `kill(2)`.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("users: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use utmpfile::{DEAD_PROCESS, LOGIN_PROCESS};

    fn record(record_type: i32, user: &[u8], pid: i32) -> Record {
        Record {
            record_type,
            user: user.to_vec(),
            tty: b"pts/0".to_vec(),
            host: Vec::new(),
            id: Vec::new(),
            pid,
            login_time: 0,
            login_usec: 0,
            exit_status: 0,
            session: 0,
            addr_v6: [0; 4],
        }
    }

    fn everyone(_: i32) -> bool {
        true
    }

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn sessions_are_sorted_by_byte_and_duplicates_stay() {
        let records = [
            record(USER_PROCESS, b"zoe", 10),
            record(USER_PROCESS, b"alice", 11),
            record(USER_PROCESS, b"Bob", 12),
            record(USER_PROCESS, b"alice", 13),
        ];
        // `B` sorts before `a`: byte order, not a collation.
        assert_eq!(user_line(&records, everyone), b"Bob alice alice zoe\n");
    }

    #[test]
    fn only_named_user_processes_count() {
        let records = [
            record(LOGIN_PROCESS, b"LOGIN", 1),
            record(DEAD_PROCESS, b"gone", 2),
            record(USER_PROCESS, b"", 3),
            record(USER_PROCESS, b"x", 4),
        ];
        assert_eq!(user_line(&records, everyone), b"x\n");
    }

    #[test]
    fn nobody_prints_nothing_at_all() {
        assert_eq!(user_line(&[], everyone), b"");
        assert_eq!(user_line(&[record(DEAD_PROCESS, b"a", 1)], everyone), b"");
    }

    #[test]
    fn trailing_spaces_are_trimmed_and_nothing_else() {
        let records = [record(USER_PROCESS, b" a b  ", 1)];
        assert_eq!(user_line(&records, everyone), b" a b\n");
        // A name of spaces alone trims to nothing, and is still a session.
        let records = [
            record(USER_PROCESS, b"  ", 1),
            record(USER_PROCESS, b"b", 2),
        ];
        assert_eq!(user_line(&records, everyone), b" b\n");
    }

    #[test]
    fn a_dead_session_is_dropped_only_when_asked_and_only_for_a_real_pid() {
        let records = [
            record(USER_PROCESS, b"live", 100),
            record(USER_PROCESS, b"dead", 200),
            record(USER_PROCESS, b"nopid", 0),
        ];
        let only_100 = |pid: i32| pid == 100;
        assert_eq!(user_line(&records, only_100), b"live nopid\n");
        assert_eq!(user_line(&records, everyone), b"dead live nopid\n");
    }

    #[test]
    fn a_name_is_bytes_not_text() {
        let records = [record(USER_PROCESS, b"caf\xe9", 1)];
        assert_eq!(user_line(&records, everyone), b"caf\xe9\n");
    }

    #[test]
    fn options() {
        assert_eq!(parse_args(&argv(&[])).unwrap(), Request::List(None));
        assert_eq!(
            parse_args(&argv(&["/var/log/wtmp"])).unwrap(),
            Request::List(Some("/var/log/wtmp".into()))
        );
        assert_eq!(parse_args(&argv(&["f", "--help"])).unwrap(), Request::Help);
        let e = parse_args(&argv(&["a", "b", "c"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand ‘b’\nTry 'users --help' for more information."
        );
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'users --help' for more information."
        );
    }

    #[test]
    fn the_help_names_the_two_files() {
        let help = help_text();
        assert!(help.contains("use /var/run/utmp.  /var/log/wtmp as FILE is common."));
    }
}
