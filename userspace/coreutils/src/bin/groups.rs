//! `groups` — print the groups a user is in.
//!
//! ```text
//! Usage: groups [OPTION]... [USERNAME]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/groups.c`. There was no `groups` on the
//! system; `id -Gn` answers the same question, but scripts and people ask it
//! as `groups`, and got `command not found`.
//!
//! # One group list, shared with `id`
//!
//! The list itself is [`coreutils::grouplist`], the module `id -G` prints
//! through -- upstream compiles `group-list.c` into both programs, and the two
//! printing different groups for the same user would be a bug in one of them
//! that neither could see. What is `groups`' own is only the framing:
//!
//! * **No operand** reports the current process: its real gid, its effective
//!   gid when that differs, then the kernel's supplementary list.
//! * **An operand** reports that account, as `NAME : g1 g2 …`, from the
//!   account database.
//!
//! # An operand is a name, never a number
//!
//! Upstream looks each operand up with `getpwnam` -- not with the
//! `parse_user_spec` `id` uses -- so `groups 0` is `‘0’: no such user` unless
//! an account is literally named `0`, and there is no `+UID` form. A name that
//! is not found is reported and the rest are still answered; the status is 1
//! if any was not found.
//!
//! # Checked against GNU
//!
//! `scripts/id-diff.sh`, which covers `id` and `groups` together.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::grouplist::{Ids, Output, print_group_list};
use pwdb::Db;
use std::ffi::OsString;

coreutils::guard_std_fds!();

const GROUPS: Program = Program::new("groups", 1);

/// Upstream's `longopts[]`; there are no short options.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The operands, in order; none means the current process.
    Run(Vec<OsString>),
}

fn help_text() -> String {
    "\
Usage: groups [OPTION]... [USERNAME]...
Print group memberships for each USERNAME or, if no USERNAME is specified, for
the current process (which may differ if the groups database has changed).
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// Upstream's full `getopt_long` loop -- unlike `id`'s neighbours that stop
/// at the first option, every option is looked at.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut users = Vec::new();
    for item in GROUPS.parse(args, "", LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(u) => users.push(u.clone()),
            // Unreachable: the table's two names are handled above.
            Opt::Long(other, _) => {
                return Err(GROUPS.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(GROUPS.invalid_option(c)),
        }
    }
    Ok(Request::Run(users))
}

/// One operand's report: `NAME : g1 g2 …\n`, or `None` for an account the
/// database does not have -- upstream's `getpwnam` returning null.
fn report_user(name: &[u8], db: &Db) -> Option<Output> {
    let user = db.user_by_name(name)?;
    let ids = Ids {
        ruid: user.uid,
        euid: user.uid,
        rgid: user.gid,
        egid: user.gid,
    };
    let mut out = Output::new();
    out.out.extend_from_slice(name);
    out.out.extend_from_slice(b" : ");
    // The process list is not consulted when there is a username: the
    // database answers instead. See `grouplist`.
    print_group_list(&mut out, Some(name), ids, true, b' ', db, &[]);
    out.out.push(b'\n');
    Some(out)
}

/// The current process's report: the groups alone, no `NAME : ` in front.
fn report_process(ids: Ids, db: &Db, process_groups: &[u32]) -> Output {
    let mut out = Output::new();
    print_group_list(&mut out, None, ids, true, b' ', db, process_groups);
    out.out.push(b'\n');
    out
}

#[cfg(unix)]
mod imp {
    use super::{Request, help_text, parse_args, report_process, report_user};
    use coreutils::diag;
    use coreutils::grouplist::{Output, current_ids, process_groups};
    use coreutils::quote::{os_bytes, quote};
    use coreutils::stdfd::{self, Stream};
    use pwdb::Db;
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::process::ExitCode;

    /// Flush one report: diagnostics first, then the line, as `id` does.
    fn drain(out: &mut Output, sink: &mut impl Write) -> io::Result<()> {
        for message in out.errors.drain(..) {
            diag!("groups: {message}");
        }
        sink.write_all(&out.out)
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(e) => {
                super::GROUPS.report(&e);
                return ExitCode::FAILURE;
            }
        };
        let mut stdout = Stream::stdout();
        let earned = match request {
            Request::Help => {
                let _ = stdout.write_all(help_text().as_bytes());
                ExitCode::SUCCESS
            }
            Request::Version => {
                let _ = stdout.write_all(b"groups (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Run(users) => {
                let db = Db::load();
                let mut ok = true;
                if users.is_empty() {
                    let mut out = report_process(current_ids(), &db, &process_groups());
                    ok &= out.ok;
                    // Deliberately unread: a failed write is `Stream`'s to
                    // remember and `close_stdout`'s to report, once.
                    let _ = drain(&mut out, &mut stdout);
                } else {
                    for user in &users {
                        let name = os_bytes(user);
                        match report_user(&name, &db) {
                            Some(mut out) => {
                                ok &= out.ok;
                                let _ = drain(&mut out, &mut stdout);
                            }
                            None => {
                                diag!("groups: {}: no such user", quote(&name));
                                ok = false;
                            }
                        }
                    }
                }
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
        };
        stdfd::close_stdout("groups", stdout, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has neither `getgroups` nor an account database of ours.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("groups: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:1000:Alice:/home/alice:/bin/sh\n\
              orphan:x:1002:9999:no group line:/:/bin/sh\n",
            b"root:x:0:\n\
              alice:x:1000:\n\
              staff:x:2000:alice\n\
              wheel:x:3000:alice\n",
        )
    }

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn an_account_is_named_then_its_groups() {
        let out = report_user(b"alice", &db()).unwrap();
        assert_eq!(out.out, b"alice : alice staff wheel\n");
        assert!(out.ok && out.errors.is_empty());
    }

    #[test]
    fn an_unknown_account_is_none_not_an_empty_line() {
        assert!(report_user(b"nosuch", &db()).is_none());
        // A number is looked up as a NAME, never as a uid.
        assert!(report_user(b"0", &db()).is_none());
    }

    #[test]
    fn a_login_group_with_no_name_is_printed_as_a_number_and_fails_the_run() {
        let out = report_user(b"orphan", &db()).unwrap();
        assert_eq!(out.out, b"orphan : 9999\n");
        assert!(!out.ok);
        assert_eq!(out.errors, vec!["cannot find name for group ID 9999"]);
    }

    #[test]
    fn the_process_report_has_no_name_in_front() {
        let ids = Ids {
            ruid: 1000,
            euid: 1000,
            rgid: 1000,
            egid: 1000,
        };
        assert_eq!(report_process(ids, &db(), &[2000]).out, b"alice staff\n");
    }

    #[test]
    fn options() {
        assert_eq!(parse_args(&argv(&[])).unwrap(), Request::Run(vec![]));
        assert_eq!(
            parse_args(&argv(&["a", "b"])).unwrap(),
            Request::Run(argv(&["a", "b"]))
        );
        assert_eq!(parse_args(&argv(&["a", "--help"])).unwrap(), Request::Help);
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'groups --help' for more information."
        );
    }
}
