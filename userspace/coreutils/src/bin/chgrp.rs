//! `chgrp` — change group ownership.
//!
//! ```text
//! Usage: chgrp [OPTION]... GROUP FILE...
//!   or:  chgrp [OPTION]... --reference=RFILE FILE...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/chgrp.c`. POSIX requires the utility,
//! and there was none: `chown :GROUP` does the same job, but scripts and
//! Makefiles say `chgrp`, and got `command not found`.
//!
//! # What is `chgrp`'s own
//!
//! Only the command line and the group operand. The walk -- which files a
//! recursive run reaches, whether a symlink or its target is changed, how `-v`
//! words the result -- is [`coreutils::chowncore`], the module `chown` runs
//! on, as upstream builds both programs over one `chown-core.c`.
//!
//! # The group operand
//!
//! Upstream's `parse_group`: a *name* first (`getgrnam`), and only if no group
//! has that name, a number (`xstrtoumax` with no suffixes, up to `GID_T_MAX`)
//! -- so a group literally named `100` wins over gid 100, and ` 5` and `+5` are
//! both 5 while `5 ` and `0x5` are `invalid group`. The empty string names no
//! group: nothing is changed and the run still succeeds. `4294967295` is
//! `(gid_t) -1`, which is the same "leave it alone".
//!
//! `-v` reports the operand *as typed* -- `chgrp -v 0 f` says `to 0` even on a
//! system where gid 0 is `root` -- and with `--reference` the reference file's
//! group by name.
//!
//! # Checked against GNU
//!
//! `scripts/chgrp-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::chowncore::{Dereference, Traverse, Verbosity, walk_policy};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::xnum::{Status, xstrtoumax};
use pwdb::Db;
use std::ffi::OsString;

coreutils::guard_std_fds!();

const CHGRP: Program = Program::new("chgrp", 1);

/// Upstream's `getopt_long` short string.
const SHORT_OPTIONS: &str = "HLPRcfhv";

/// Upstream's `long_options[]`, in its declaration order -- which an
/// ambiguous prefix's list of candidates shows.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("recursive", Takes::Nothing),
    ("changes", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-preserve-root", Takes::Nothing),
    ("preserve-root", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("reference", Takes::Required),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--quiet` and `--silent` are one option, so `--s` is not ambiguous.
const LONG_ALIASES: &[(&str, &str)] = &[("silent", "quiet")];

/// Where the new group comes from.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Source {
    /// A GROUP operand, still unparsed: resolving it needs the account
    /// database, and parsing the command line touches no file.
    Group(OsString),
    /// `--reference=RFILE`: whatever group that file turns out to have.
    Reference(OsString),
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Settings {
    recursive: bool,
    /// The traversal in effect, after [`walk_policy`].
    traverse: Traverse,
    /// Change a symlink's target rather than the link, after [`walk_policy`].
    affect_referent: bool,
    verbosity: Verbosity,
    force_silent: bool,
    preserve_root: bool,
    source: Source,
    files: Vec<OsString>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

fn help_text() -> String {
    "\
Usage: chgrp [OPTION]... GROUP FILE...
  or:  chgrp [OPTION]... --reference=RFILE FILE...
Change the group of each FILE to GROUP.
With --reference, change the group of each FILE to that of RFILE.

  -c, --changes          like verbose but report only when a change is made
  -f, --silent, --quiet  suppress most error messages
  -v, --verbose          output a diagnostic for every file processed
      --dereference      affect the referent of each symbolic link (this is
                         the default), rather than the symbolic link itself
  -h, --no-dereference   affect symbolic links instead of any referenced file
                         (useful only on systems that can change the
                         ownership of a symlink)
      --no-preserve-root  do not treat '/' specially (the default)
      --preserve-root    fail to operate recursively on '/'
      --reference=RFILE  use RFILE's group rather than specifying a GROUP.
                         RFILE is always dereferenced if a symbolic link.
  -R, --recursive        operate on files and directories recursively

The following options modify how a hierarchy is traversed when the -R
option is also specified.  If more than one is specified, only the final
one takes effect.

  -H                     if a command line argument is a symbolic link
                         to a directory, traverse it
  -L                     traverse every symbolic link to a directory
                         encountered
  -P                     do not traverse any symbolic links (default)

      --help        display this help and exit
      --version     output version information and exit

Examples:
  chgrp staff /u      Change the group of /u to \"staff\".
  chgrp -hR staff /u  Change the group of /u and subfiles to \"staff\".
"
    .to_string()
}

/// Upstream's option loop, the `-R --dereference` check, then the operand
/// count -- in that order, which is the order their messages can appear in.
///
/// # Errors
///
/// An unknown or ambiguous option; `-R --dereference` without `-H` or `-L`;
/// too few operands.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut recursive = false;
    let mut traverse = Traverse::default();
    let mut dereference = Dereference::default();
    let mut verbosity = Verbosity::Off;
    let mut force_silent = false;
    let mut preserve_root = false;
    let mut reference: Option<OsString> = None;
    let mut operands: Vec<OsString> = Vec::new();

    for item in CHGRP.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, LONG_ALIASES) {
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Short(b'H', _) => traverse = Traverse::CommandLine,
            Opt::Short(b'L', _) => traverse = Traverse::Logical,
            Opt::Short(b'P', _) => traverse = Traverse::Physical,
            Opt::Short(b'h', _) | Opt::Long("no-dereference", _) => dereference = Dereference::Link,
            Opt::Long("dereference", _) => dereference = Dereference::Referent,
            Opt::Long("no-preserve-root", _) => preserve_root = false,
            Opt::Long("preserve-root", _) => preserve_root = true,
            Opt::Long("reference", value) => reference = value,
            Opt::Short(b'R', _) | Opt::Long("recursive", _) => recursive = true,
            Opt::Short(b'c', _) | Opt::Long("changes", _) => verbosity = Verbosity::ChangesOnly,
            // Both spellings: an exact long option resolves to the name typed.
            Opt::Short(b'f', _) | Opt::Long("quiet" | "silent", _) => force_silent = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => verbosity = Verbosity::High,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: every table entry is handled above.
            Opt::Long(other, _) => {
                return Err(CHGRP.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(CHGRP.invalid_option(other)),
        }
    }

    // `error (EXIT_FAILURE, …)`: status 1 and no referral.
    let (traverse, affect_referent) = walk_policy(recursive, traverse, dereference)
        .map_err(|message| CHGRP.usage(message.to_string()))?;

    let wanted = if reference.is_some() { 1 } else { 2 };
    if operands.len() < wanted {
        let message = match operands.last() {
            None => "missing operand".to_string(),
            Some(last) => format!("missing operand after {}", quote(&os_bytes(last))),
        };
        return Err(CHGRP.usage_referring(message));
    }

    let mut files = operands;
    let source = match reference {
        Some(rfile) => Source::Reference(rfile),
        // The group is the first word and the files are the rest.
        None => Source::Group(files.remove(0)),
    };
    Ok(Request::Run(Box::new(Settings {
        recursive,
        traverse,
        affect_referent,
        verbosity,
        force_silent,
        preserve_root,
        source,
        files,
    })))
}

/// Upstream's `parse_group`: the gid to set, or `None` for "leave it alone".
///
/// # Errors
///
/// `invalid group: 'NAME'` for a name no group has that is not a number in
/// range either.
fn parse_group(name: &[u8], db: &Db) -> Result<Option<u32>, String> {
    if name.is_empty() {
        return Ok(None);
    }
    if let Some(group) = db.group_by_name(name) {
        return Ok(Some(group.gid));
    }
    match xstrtoumax(name, Some(b"")) {
        (value, Status::Ok) => match u32::try_from(value) {
            // `(gid_t) -1` is what chown(2) takes to mean "unchanged".
            Ok(u32::MAX) => Ok(None),
            Ok(gid) => Ok(Some(gid)),
            Err(_) => Err(format!("invalid group: {}", quote(name))),
        },
        _ => Err(format!("invalid group: {}", quote(name))),
    }
}

#[cfg(unix)]
mod imp {
    use super::{CHGRP, Request, Source, help_text, parse_args, parse_group};
    use coreutils::chowncore::{Ids, Options, chown_files};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, quoteaf_os};
    use coreutils::stdfd::{self, Stream};
    use coreutils::userspec::gid_to_name;
    use pwdb::Db;
    use std::ffi::OsString;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::process::ExitCode;

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let settings = match parse_args(&args) {
            Ok(Request::Run(settings)) => *settings,
            Ok(Request::Help) => {
                let mut out = Stream::stdout();
                // Deliberately unread: `close_stdout` reports a failed write.
                let _ = out.write_all(help_text().as_bytes());
                return stdfd::close_stdout("chgrp", out, ExitCode::SUCCESS);
            }
            Ok(Request::Version) => {
                let mut out = Stream::stdout();
                let _ = out.write_all(b"chgrp (SlateOS coreutils) 0.1.0\n");
                return stdfd::close_stdout("chgrp", out, ExitCode::SUCCESS);
            }
            Err(e) => {
                CHGRP.report(&e);
                return ExitCode::FAILURE;
            }
        };

        let db = Db::load();
        let (gid, group_name) = match &settings.source {
            Source::Reference(rfile) => match fs::metadata(rfile) {
                // Always dereferenced, as the help text says.
                Ok(meta) => (Some(meta.gid()), Some(gid_to_name(&db, meta.gid()))),
                Err(e) => {
                    diag!(
                        "chgrp: failed to get attributes of {}: {}",
                        quoteaf_os(rfile),
                        strerror(&e)
                    );
                    return ExitCode::FAILURE;
                }
            },
            Source::Group(text) => {
                let name = os_bytes(text);
                match parse_group(&name, &db) {
                    // `-v` reports the operand as typed, not the name the
                    // number resolves to; the empty operand names nothing.
                    Ok(gid) => (gid, (!name.is_empty()).then(|| name.to_vec())),
                    Err(message) => {
                        diag!("chgrp: {message}");
                        return ExitCode::FAILURE;
                    }
                }
            }
        };

        let root_dev_ino = if settings.recursive && settings.preserve_root {
            match fs::metadata("/") {
                Ok(meta) => Some((meta.dev(), meta.ino())),
                Err(e) => {
                    diag!(
                        "chgrp: failed to get attributes of {}: {}",
                        quoteaf_os("/"),
                        strerror(&e)
                    );
                    return ExitCode::FAILURE;
                }
            }
        } else {
            None
        };

        let options = Options {
            program: "chgrp",
            recursive: settings.recursive,
            traverse: settings.traverse,
            affect_referent: settings.affect_referent,
            verbosity: settings.verbosity,
            force_silent: settings.force_silent,
            root_dev_ino,
            user_name: None,
            group_name,
        };
        let mut out = Stream::stdout();
        let ok = chown_files(
            &settings.files,
            Ids { uid: None, gid },
            Ids::default(),
            &options,
            &db,
            &mut out,
        );
        let earned = if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
        stdfd::close_stdout("chgrp", out, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no `chown(2)`.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("chgrp: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n",
            b"root:x:0:\nstaff:x:50:\n100:x:7:\n",
        )
    }

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn run(args: &[&str]) -> Settings {
        match parse_args(&argv(args)).unwrap() {
            Request::Run(settings) => *settings,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_name_wins_over_a_number() {
        assert_eq!(parse_group(b"staff", &db()), Ok(Some(50)));
        // A group literally named `100` is gid 7, not gid 100.
        assert_eq!(parse_group(b"100", &db()), Ok(Some(7)));
        assert_eq!(parse_group(b"200", &db()), Ok(Some(200)));
    }

    #[test]
    fn numbers_are_read_as_xstrtoumax_reads_them() {
        assert_eq!(parse_group(b" 5", &db()), Ok(Some(5)));
        assert_eq!(parse_group(b"+5", &db()), Ok(Some(5)));
        assert_eq!(
            parse_group(b"5 ", &db()),
            Err("invalid group: ‘5 ’".to_string())
        );
        assert!(parse_group(b"0x5", &db()).is_err());
        assert!(parse_group(b"-5", &db()).is_err());
        assert!(parse_group(b"4294967296", &db()).is_err());
        assert!(parse_group(b"nosuch", &db()).is_err());
    }

    #[test]
    fn the_empty_group_and_minus_one_leave_the_group_alone() {
        assert_eq!(parse_group(b"", &db()), Ok(None));
        assert_eq!(parse_group(b"4294967295", &db()), Ok(None));
    }

    #[test]
    fn the_first_operand_is_the_group() {
        let s = run(&["staff", "a", "b"]);
        assert_eq!(s.source, Source::Group("staff".into()));
        assert_eq!(s.files, argv(&["a", "b"]));
        let s = run(&["--reference=r", "a"]);
        assert_eq!(s.source, Source::Reference("r".into()));
        assert_eq!(s.files, argv(&["a"]));
    }

    #[test]
    fn the_symlink_options_reduce_as_upstream_reduces_them() {
        let s = run(&["g", "f"]);
        assert_eq!((s.traverse, s.affect_referent), (Traverse::Physical, true));
        let s = run(&["-h", "g", "f"]);
        assert!(!s.affect_referent);
        let s = run(&["-R", "g", "f"]);
        assert_eq!((s.traverse, s.affect_referent), (Traverse::Physical, false));
        let s = run(&["-RH", "g", "f"]);
        assert_eq!(
            (s.traverse, s.affect_referent),
            (Traverse::CommandLine, true)
        );
        let s = run(&["-RLh", "g", "f"]);
        assert_eq!((s.traverse, s.affect_referent), (Traverse::Logical, false));
        // The last of -H/-L/-P wins.
        let s = run(&["-R", "-L", "-P", "g", "f"]);
        assert_eq!(s.traverse, Traverse::Physical);
    }

    #[test]
    fn dereference_under_r_needs_h_or_l_and_is_checked_before_the_operands() {
        let e = parse_args(&argv(&["-R", "--dereference"])).unwrap_err();
        assert_eq!(e.message(), "-R --dereference requires either -H or -L");
        assert!(parse_args(&argv(&["-RH", "--dereference", "g", "f"])).is_ok());
    }

    #[test]
    fn missing_operands() {
        let e = parse_args(&argv(&[])).unwrap_err();
        assert_eq!(
            e.message(),
            "missing operand\nTry 'chgrp --help' for more information."
        );
        let e = parse_args(&argv(&["staff"])).unwrap_err();
        assert_eq!(
            e.message(),
            "missing operand after ‘staff’\nTry 'chgrp --help' for more information."
        );
        assert!(parse_args(&argv(&["--reference=r"])).is_err());
    }
}
