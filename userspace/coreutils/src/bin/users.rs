//! `users` — print the user names of users currently logged in.
//!
//! ```text
//! Usage: users [OPTION]... [FILE]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/users.c`. The records are read by
//! [`coreutils::utmp`] -- gnulib's `readutmp`, shared with `who` and `pinky`,
//! over the `utmpfile` parser `last`, `finger` and `uptime` use too -- so this
//! file holds only what `users` does with them.
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
use coreutils::utmp::{self, Record, UTMP_FILE, WTMP_FILE};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const USERS: Program = Program::new("users", 1);

/// `parse_gnu_standard_options_only`'s table.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

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

/// Upstream's `list_entries_users`: the line `users` prints for `records`.
/// Empty when there is nobody, rather than a lone newline. Sessions whose
/// process has gone were left out by [`utmp::read_utmp`] when the live utmp
/// was read, as upstream's `READ_UTMP_CHECK_PIDS` leaves them out.
fn user_line(records: &[Record]) -> Vec<u8> {
    let mut names: Vec<&[u8]> = records
        .iter()
        .filter(|r| utmp::is_user_process(r))
        .map(utmp::trimmed_name)
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
    use coreutils::getopt::Report;
    use coreutils::quote::{os_bytes, quotef};
    use coreutils::stdfd::{self, Stream};
    use coreutils::utmp::{self, Want};
    use std::ffi::OsString;
    use std::io::Write;
    use std::process::ExitCode;

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
                // Upstream's `users (UTMP_FILE, READ_UTMP_CHECK_PIDS)` or
                // `users (file, 0)`, each with `READ_UTMP_USER_PROCESS` added.
                let named = file.is_some();
                let path = file.map_or_else(
                    || UTMP_FILE.as_bytes().to_vec(),
                    |f| os_bytes(&f).into_owned(),
                );
                let want = Want {
                    users_only: true,
                    live_only: !named,
                };
                let records = match utmp::read_utmp(&path, named, want) {
                    Ok(records) => records,
                    Err(e) => {
                        diag!("users: {}: {}", quotef(&path), strerror(&e));
                        return ExitCode::FAILURE;
                    }
                };
                let _ = out.write_all(&user_line(&records));
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
    use coreutils::utmp::{DEAD_PROCESS, LOGIN_PROCESS, USER_PROCESS};

    fn record(record_type: i32, user: &[u8], pid: i32) -> Record {
        Record {
            record_type,
            user: user.to_vec(),
            tty: b"pts/0".to_vec(),
            host: Vec::new(),
            id: Vec::new(),
            pid,
            login_time: 0,
            tv_sec: 0,
            login_usec: 0,
            exit_status: 0,
            session: 0,
            addr_v6: [0; 4],
        }
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
        assert_eq!(user_line(&records), b"Bob alice alice zoe\n");
    }

    #[test]
    fn only_named_user_processes_count() {
        let records = [
            record(LOGIN_PROCESS, b"LOGIN", 1),
            record(DEAD_PROCESS, b"gone", 2),
            record(USER_PROCESS, b"", 3),
            record(USER_PROCESS, b"x", 4),
        ];
        assert_eq!(user_line(&records), b"x\n");
    }

    #[test]
    fn nobody_prints_nothing_at_all() {
        assert_eq!(user_line(&[]), b"");
        assert_eq!(user_line(&[record(DEAD_PROCESS, b"a", 1)]), b"");
    }

    #[test]
    fn trailing_spaces_are_trimmed_and_nothing_else() {
        let records = [record(USER_PROCESS, b" a b  ", 1)];
        assert_eq!(user_line(&records), b" a b\n");
        // A name of spaces alone trims to nothing, and is still a session.
        let records = [
            record(USER_PROCESS, b"  ", 1),
            record(USER_PROCESS, b"b", 2),
        ];
        assert_eq!(user_line(&records), b" b\n");
    }

    // Dropping a session whose process has gone is `coreutils::utmp`'s now,
    // and is tested there.

    #[test]
    fn a_name_is_bytes_not_text() {
        let records = [record(USER_PROCESS, b"caf\xe9", 1)];
        assert_eq!(user_line(&records), b"caf\xe9\n");
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
