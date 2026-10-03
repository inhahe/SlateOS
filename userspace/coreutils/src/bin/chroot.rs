//! `chroot` -- run a command with its root directory changed.
//!
//! A port of GNU coreutils 9.4's `src/chroot.c`, measured against the real
//! binary by `scripts/chroot-diff.sh`. It replaces `userspace/chroot`, which
//! was written from the manual rather than the source and, since the kernel
//! had no native `chroot` or `setgroups` when it was written, refused every
//! step with a "not implemented" error. Both exist now
//! (`SYS_PROCESS_CHROOT`, `SYS_PROCESS_SETGROUPS`), and this does all of them.
//!
//! # Users and groups are looked up twice, on purpose
//!
//! `--userspec` and `--groups` name accounts, and the accounts that matter are
//! the new root's: its `/etc/passwd` may give `alice` a different uid from the
//! old root's. So upstream reads them *after* the `chroot`. It also reads them
//! once *before*, silently, for two reasons its comments give: on glibc the
//! lookup plugins (NSS) must be loaded while the old root's libraries are
//! still visible, and a lookup that succeeds outside is the fallback when the
//! same one fails inside -- `--groups` keeps the outside list if the inside
//! cannot read it.
//!
//! The values carry over between the two passes, and that is visible: a uid
//! set outside stays set if the inside pass does not name one, and a group
//! found outside from the account's login group is kept when `--groups` is
//! also given. This port keeps both passes and the carrying-over exactly. Its
//! lookups read the files as SlateOS's library does -- `/etc/passwd` and
//! `/etc/group` under whichever root is current -- and, measured, GNU's
//! lookups report no reason for a failure either, even with no `/etc` at all
//! in the new root.
//!
//! # Then the credentials, in the only safe order
//!
//! Supplementary groups, then the group, then the user -- each a privilege
//! the next one gives up, so any other order leaves a step that can no longer
//! be taken. Any failure stops the run before the command starts: a command
//! that was meant to run as `nobody` must not run as `root` because a call
//! failed.
//!
//! # What `--skip-chdir` is for
//!
//! `chroot` moves the working directory to the new `/`, because the old one
//! may lie outside the new root and would be reachable through `..` if left
//! there. `--skip-chdir` keeps it, and is allowed only when NEWROOT *is* the
//! current root by some other name (`/`, `//`, a symbolic link to `/`): its
//! use is running a command under `--userspec` or `--groups` without moving.

use coreutils::canon::{self, Mode, RealFs};
use coreutils::getopt::{self, Opt, Program, Report, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use pwdb::Db;
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

// Recorded before `main`: a descriptor the caller closed reaches the command
// closed, not as the `/dev/null` the runtime puts there.
coreutils::guard_std_fds!();

/// `EXIT_CANCELED`: `chroot` itself failed.
const EXIT_CANCELED: u8 = 125;
/// `EXIT_CANNOT_INVOKE`: the command was found but could not be run.
#[cfg_attr(not(unix), allow(dead_code))]
const EXIT_CANNOT_INVOKE: u8 = 126;
/// `EXIT_ENOENT`: the command was not found.
#[cfg_attr(not(unix), allow(dead_code))]
const EXIT_ENOENT: u8 = 127;

const CHROOT_NAME: &str = "chroot";
const CHROOT: Program = Program::new(CHROOT_NAME, EXIT_CANCELED as i32);

/// Upstream's `"+"`: no short options, and the first operand -- NEWROOT --
/// ends the options, so what follows it is the command's.
const SHORT_OPTIONS: &str = "+";

/// Upstream's `long_opts`, in its order, so that an abbreviation resolves as
/// it does there.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("groups", Takes::Required),
    ("userspec", Takes::Required),
    ("skip-chdir", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `MAXGID`, upstream's `GID_T_MAX`: the largest number `--groups` takes as a
/// group id -- all of a `gid_t`, so 4294967295 is one, where `--userspec`
/// refuses it as `(gid_t) -1`. Both measured.
const MAXGID: u64 = 4_294_967_295;

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum Request {
    Help,
    Version,
    Run(Job),
}

/// A root, a command, and whom to run it as. Off Unix only `newroot` is
/// read, to name the root that cannot be changed to.
#[cfg_attr(test, derive(Debug, PartialEq))]
#[cfg_attr(not(unix), allow(dead_code))]
struct Job {
    /// `--userspec`, with one trailing `:` taken off: upstream treats `user:`
    /// as `user`, because it looks up the login group by default anyway.
    userspec: Option<Vec<u8>>,
    /// `--groups`, as typed. `Some` of an empty list is not `None`: it asks
    /// for the supplementary groups to be cleared.
    groups: Option<Vec<u8>>,
    /// `--skip-chdir`.
    skip_chdir: bool,
    /// NEWROOT.
    newroot: OsString,
    /// The command and its arguments; empty for upstream's interactive shell.
    command: Vec<OsString>,
}

/// GNU 9.4's `--help`, without the block of project links no bin here carries.
const HELP: &str = "\
Usage: chroot [OPTION] NEWROOT [COMMAND [ARG]...]
  or:  chroot OPTION
Run COMMAND with root directory set to NEWROOT.

      --groups=G_LIST        specify supplementary groups as g1,g2,..,gN
      --userspec=USER:GROUP  specify user and group (ID or name) to use
      --skip-chdir           do not change working directory to '/'
      --help        display this help and exit
      --version     output version information and exit

If no command is given, run '\"$SHELL\" -i' (default: '/bin/sh -i').

Exit status:
  125  if the chroot command itself fails
  126  if COMMAND is found but cannot be invoked
  127  if COMMAND cannot be found
  -    the exit status of COMMAND otherwise
";

/// The funnel: a diagnostic that could not be delivered makes the status 125,
/// upstream's `atexit (close_stdout)` with `EXIT_CANCELED` as its failure.
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), EXIT_CANCELED)
}

fn run_main() -> ExitCode {
    // First, before anything can touch a standard descriptor.
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match scan(&args) {
        Ok(request) => request,
        Err(e) => {
            CHROOT.report(&e);
            return ExitCode::from(EXIT_CANCELED);
        }
    };
    match request {
        Request::Help => {
            let mut out = Stream::stdout();
            // Unchecked here: the verdict is `close_stdout_with`'s, below.
            let _ = out.write_all(HELP.as_bytes());
            stdfd::close_stdout_with(CHROOT_NAME, out, ExitCode::SUCCESS, EXIT_CANCELED)
        }
        Request::Version => {
            let mut out = Stream::stdout();
            // As for the help.
            let _ = out.write_all(b"chroot (SlateOS coreutils) 0.1.0\n");
            stdfd::close_stdout_with(CHROOT_NAME, out, ExitCode::SUCCESS, EXIT_CANCELED)
        }
        Request::Run(job) => ExitCode::from(imp::run(&job)),
    }
}

/// `chroot: MESSAGE` on stderr -- upstream's `error (0, 0, …)`.
fn say(message: &str) {
    stdfd::diag_line(&format!("{CHROOT_NAME}: {message}"));
}

/// [`say`] with the reason -- upstream's `error (…, errno, …)`.
#[cfg_attr(not(unix), allow(dead_code))]
fn say_errno(message: &str, errno: i32) {
    say(&format!(
        "{message}: {}",
        coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
    ));
}

// ------------------------------------------------------------- scanning ----

/// Read the command line as `chroot.c`'s `main` does.
///
/// # Errors
///
/// A getopt diagnostic, or `missing operand` -- each with the referral.
fn scan(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut userspec = None;
    let mut groups = None;
    let mut skip_chdir = false;
    let mut parser = CHROOT.parse(args, SHORT_OPTIONS, LONG_OPTIONS);
    let mut first_operand = None;
    // `while let` rather than `for`: the operand arm needs `optind`, which a
    // `for` loop's borrow of the parser would not let it ask for.
    #[allow(clippy::while_let_on_iterator)]
    while let Some(item) = parser.next() {
        match item? {
            Opt::Long("userspec", value) => {
                let mut spec = os_bytes(&value.unwrap_or_default()).into_owned();
                // "Treat 'user:' just like 'user'" -- one colon, and only at
                // the very end.
                if spec.last() == Some(&b':') {
                    spec.pop();
                }
                userspec = Some(spec);
            }
            Opt::Long("groups", value) => {
                groups = Some(os_bytes(&value.unwrap_or_default()).into_owned());
            }
            Opt::Long("skip-chdir", _) => skip_chdir = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(_) => {
                // The `+` has stopped the parser, one word past NEWROOT.
                first_operand = Some(parser.optind().saturating_sub(1));
                break;
            }
            // Unreachable: there are no short options, and the parser yields
            // only the long ones declared above. Refused rather than ignored,
            // so one added to the table without a handler fails loudly.
            Opt::Short(other, _) => return Err(CHROOT.invalid_option(other)),
            Opt::Long(other, _) => {
                return Err(CHROOT.usage_referring(format!("option '--{other}' is unhandled")));
            }
        }
    }
    // No operand met: the end of argv, or `--` (after which `optind` points).
    let start = first_operand.unwrap_or_else(|| parser.optind());
    let Some([newroot, command @ ..]) = args.get(start..) else {
        return Err(CHROOT.usage_referring("missing operand".to_string()));
    };
    Ok(Request::Run(Job {
        userspec,
        groups,
        skip_chdir,
        newroot: newroot.clone(),
        command: command.to_vec(),
    }))
}

// -------------------------------------------------------------- lookups ----

/// Upstream's `is_root`: whether `dir` is the current root under another
/// name. Its canonical form, every component existing, is `/` -- compared as
/// a name rather than by device and inode, because "/" could be bind mounted
/// elsewhere.
#[cfg_attr(not(unix), allow(dead_code))]
fn is_root(dir: &[u8]) -> bool {
    canon::canonicalize(&RealFs, dir, Mode::Existing).is_ok_and(|name| name == b"/")
}

/// Upstream's `parse_additional_groups`: `--groups`' comma-separated names
/// and numbers, as group ids.
///
/// A word that reads as a number (`xstrtoumax`, the whole word, up to
/// `MAXGID`) is that number -- unless a group is *called* that, in which case
/// it is that group, the way a numeric name is handled throughout; a `+` in
/// front skips the name lookup. Anything else is a group name. Empty words,
/// between two commas or at either end, are skipped, as `strtok` skips them.
///
/// With `show_errors`, every word that names no group is reported, and then
/// the list as a whole if it held no words at all. Without, the first failure
/// ends it silently -- that is the pass outside the new root, whose failures
/// are not the user's to see.
///
/// # Errors
///
/// `Err(())` for any word that names no group, or for a list with no words.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_additional_groups(groups: &[u8], db: &Db, show_errors: bool) -> Result<Vec<u32>, ()> {
    let mut gids = Vec::new();
    let mut failed = false;
    for word in groups.split(|&b| b == b',').filter(|w| !w.is_empty()) {
        let found = match xnum::xstrtoumax_base(word, 10, Some(b"")) {
            (value, Status::Ok) if value <= MAXGID => {
                // "Handle the case where the name is numeric." The name looked
                // up is the word without its leading white space, which the
                // number was read past too -- C's `isspace`, vertical tab
                // included, where Rust's ASCII trim leaves it.
                let name = c_trim_start(word);
                let by_name = if name.first() == Some(&b'+') {
                    None
                } else {
                    db.group_by_name(name).map(|g| g.gid)
                };
                // In range, by the guard above.
                by_name.or_else(|| u32::try_from(value).ok())
            }
            _ => db.group_by_name(word).map(|g| g.gid),
        };
        if let Some(gid) = found {
            gids.push(gid);
        } else {
            failed = true;
            if !show_errors {
                break;
            }
            say(&format!("invalid group {}", quote(word)));
        }
    }
    if !failed && gids.is_empty() {
        if show_errors {
            say(&format!("invalid group list {}", quote(groups)));
        }
        failed = true;
    }
    if failed { Err(()) } else { Ok(gids) }
}

/// `word` without its leading C white space: space, tab, newline, vertical
/// tab, form feed and carriage return, as `isspace` has them in the C locale.
#[cfg_attr(not(unix), allow(dead_code))]
fn c_trim_start(word: &[u8]) -> &[u8] {
    let skip = word
        .iter()
        .take_while(|&&b| matches!(b, b' ' | 0x09..=0x0d))
        .count();
    word.get(skip..).unwrap_or_default()
}

/// What the lookups have settled so far: upstream's `uid`, `gid`, `username`
/// and the supplementary list, which survive from the pass outside the new
/// root into the pass inside.
#[derive(Default)]
#[cfg_attr(not(unix), allow(dead_code))]
struct Ids {
    uid: Option<u32>,
    gid: Option<u32>,
    username: Option<Vec<u8>>,
    /// The supplementary groups, once a list has been found.
    gids: Option<Vec<u32>>,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl Ids {
    /// Fold in a parsed `--userspec`: each half replaces what is there only if
    /// the spec has it, as gnulib's `parse_with_separator` starts from the
    /// caller's values and writes back only what it read.
    fn take(&mut self, spec: &userspec::Spec) {
        self.uid = spec.uid.or(self.uid);
        self.gid = spec.gid.or(self.gid);
    }

    /// "If no gid is supplied or looked up, do so now. Also lookup the
    /// username for use with getgroups." -- skipped when there is no uid, or
    /// when `--groups` was given and a gid is already known.
    ///
    /// Returns `false` only when the lookup was made and found no account.
    fn login_group(&mut self, db: &Db, groups_given: bool) -> bool {
        let Some(uid) = self.uid else {
            return true;
        };
        if groups_given && self.gid.is_some() {
            return true;
        }
        let Some(account) = db.user_by_uid(uid) else {
            return false;
        };
        if self.gid.is_none() {
            self.gid = Some(account.gid);
        }
        self.username = Some(account.name.clone());
        true
    }
}

// ------------------------------------------------------------------ unix ----

/// Changing root and credentials, and becoming the command.
#[cfg(unix)]
mod imp {
    use super::{
        CHROOT, EXIT_CANCELED, EXIT_CANNOT_INVOKE, EXIT_ENOENT, Ids, Job, is_root,
        parse_additional_groups, say, say_errno,
    };
    use coreutils::getopt::Report;
    use coreutils::quote::{os_bytes, quote, quoteaf};
    use libcall::process;
    use pwdb::Db;
    use std::ffi::{CStr, CString, OsString};

    /// Run the job: upstream's `main` after the option loop. Returns only when
    /// the command could not be started, with the status to exit with.
    pub fn run(job: &Job) -> u8 {
        let newroot = os_bytes(&job.newroot).into_owned();
        let is_oldroot = is_root(&newroot);
        if !is_oldroot && job.skip_chdir {
            CHROOT.report(&CHROOT.usage_referring(format!(
                "option --skip-chdir only permitted if NEWROOT is old {}",
                quoteaf(b"/")
            )));
            return EXIT_CANCELED;
        }

        let mut ids = Ids::default();
        if !is_oldroot {
            look_up_outside(job, &mut ids);
        }

        let refused = |e: i32| {
            say_errno(
                &format!("cannot change root directory to {}", quoteaf(&newroot)),
                e,
            );
            EXIT_CANCELED
        };
        // A word of argv cannot hold a NUL, so this cannot fail on one.
        let Ok(path) = CString::new(newroot.clone()) else {
            return refused(libcall::EINVAL);
        };
        if let Err(e) = process::change_root(&path) {
            return refused(e);
        }
        if !job.skip_chdir
            && let Err(e) = std::env::set_current_dir("/")
        {
            say_errno(
                "cannot chdir to root directory",
                e.raw_os_error().unwrap_or(libcall::EINVAL),
            );
            return EXIT_CANCELED;
        }

        // No command: "Run an interactive shell."
        let argv: Vec<OsString> = if job.command.is_empty() {
            let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
            vec![shell, OsString::from("-i")]
        } else {
            job.command.clone()
        };

        if let Err(status) = look_up_inside(job, &mut ids) {
            return status;
        }
        if let Err(status) = set_credentials(job, &ids) {
            return status;
        }
        exec(&argv)
    }

    /// The silent pass, before the `chroot`: what the old root knows, kept as
    /// the fallback for what the new one does not.
    fn look_up_outside(job: &Job, ids: &mut Ids) {
        let db = Db::load();
        if let Some(spec) = &job.userspec
            && let Ok((parsed, _)) = userspec::parse_user_spec(spec, &db)
        {
            ids.take(&parsed);
        }
        // Unchecked: a uid the old root does not know is the inside pass's to
        // report, if the new root does not know it either.
        let _ = ids.login_group(&db, job.groups.is_some());
        match &job.groups {
            Some(list) if !list.is_empty() => {
                ids.gids = parse_additional_groups(list, &db, false).ok();
            }
            None => {
                if let (Some(gid), Some(name)) = (ids.gid, ids.username.as_deref()) {
                    ids.gids = Some(db.group_list(name, gid));
                }
            }
            Some(_) => {}
        }
    }

    /// The pass that counts, inside the new root. `Err` is the status to exit
    /// with, the reason already said.
    fn look_up_inside(job: &Job, ids: &mut Ids) -> Result<(), u8> {
        let db = Db::load();
        if let Some(spec) = &job.userspec {
            match userspec::parse_user_spec(spec, &db) {
                Ok((parsed, dot)) => {
                    ids.take(&parsed);
                    if dot {
                        // A warning, and the run goes on: `error (0, …)`.
                        say("warning: '.' should be ':'");
                    }
                }
                Err(message) => {
                    say(message);
                    return Err(EXIT_CANCELED);
                }
            }
        }
        if !ids.login_group(&db, job.groups.is_some()) && ids.gid.is_none() {
            // A uid with no account, and no group to fall back on. Upstream
            // appends `errno` here, which no file lookup sets: measured, no
            // reason follows the message.
            say(&format!(
                "no group specified for unknown uid: {}",
                ids.uid.unwrap_or_default()
            ));
            return Err(EXIT_CANCELED);
        }
        match &job.groups {
            Some(list) if !list.is_empty() => {
                // Its errors are shown only when there is no outside list to
                // fall back on -- and with one, that is used instead.
                match parse_additional_groups(list, &db, ids.gids.is_none()) {
                    Ok(inside) => ids.gids = Some(inside),
                    Err(()) if ids.gids.is_none() => return Err(EXIT_CANCELED),
                    Err(()) => {}
                }
            }
            None => {
                // `getgrouplist` counts the group it is given, so the list is
                // never empty, and upstream's "failed to get supplemental
                // groups" cannot be reached through it.
                if let (Some(gid), Some(name)) = (ids.gid, ids.username.as_deref()) {
                    ids.gids = Some(db.group_list(name, gid));
                }
            }
            Some(_) => {}
        }
        Ok(())
    }

    /// "Attempt to set all three: supplementary groups, group ID, user ID.
    /// Diagnose any failures. If any have failed, exit before execvp."
    fn set_credentials(job: &Job, ids: &Ids) -> Result<(), u8> {
        if ids.uid.is_some() || job.groups.is_some() {
            let gids = ids.gids.as_deref().unwrap_or_default();
            if let Err(e) = process::set_groups(gids) {
                say_errno("failed to set supplemental groups", e);
                return Err(EXIT_CANCELED);
            }
        }
        if let Some(gid) = ids.gid
            && let Err(e) = process::set_gid(gid)
        {
            say_errno("failed to set group-ID", e);
            return Err(EXIT_CANCELED);
        }
        if let Some(uid) = ids.uid
            && let Err(e) = process::set_uid(uid)
        {
            say_errno("failed to set user-ID", e);
            return Err(EXIT_CANCELED);
        }
        Ok(())
    }

    /// Become the command: `execvp`, and upstream's report if that fails.
    fn exec(argv: &[OsString]) -> u8 {
        let name = argv
            .first()
            .map(|word| os_bytes(word).into_owned())
            .unwrap_or_default();
        let failed = |errno: i32| {
            say_errno(&format!("failed to run command {}", quote(&name)), errno);
            if errno == libcall::ENOENT {
                EXIT_ENOENT
            } else {
                EXIT_CANNOT_INVOKE
            }
        };
        // A word of argv cannot hold a NUL, so this cannot fail on one.
        let Ok(words) = argv
            .iter()
            .map(|word| CString::new(os_bytes(word).into_owned()))
            .collect::<Result<Vec<CString>, _>>()
        else {
            return failed(libcall::EINVAL);
        };
        let refs: Vec<&CStr> = words.iter().map(CString::as_c_str).collect();
        let mut slots = vec![std::ptr::null::<u8>(); refs.len().saturating_add(1)];
        failed(process::execvp(&refs, &mut slots))
    }
}

/// Off Unix there is no root to change.
#[cfg(not(unix))]
mod imp {
    use super::{EXIT_CANCELED, Job, say};

    pub fn run(job: &Job) -> u8 {
        say(&format!(
            "cannot change root directory to {}: {}",
            coreutils::quote::quoteaf_os(&job.newroot),
            coreutils::errmsg::strerror(&std::io::Error::from(std::io::ErrorKind::Unsupported))
        ));
        EXIT_CANCELED
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn job(words: &[&str]) -> Job {
        match scan(&argv(words)) {
            Ok(Request::Run(job)) => job,
            other => panic!("wanted a job, got {other:?}"),
        }
    }

    fn refused(words: &[&str]) -> String {
        scan(&argv(words)).unwrap_err().message()
    }

    /// A fixture account database: `fix` (4321) in `fixg` (4322), and a group
    /// whose *name* is a number.
    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0::/:/bin/sh\nfix:x:4321:4322::/:/bin/sh\n",
            b"root:x:0:\nfixg:x:4322:fix\nother:x:4400:fix\n4401:x:4402:fix\n",
        )
    }

    #[test]
    fn newroot_and_command_and_the_options_before_them() {
        let j = job(&[
            "--userspec=fix:",
            "--groups=a,b",
            "--skip-chdir",
            "/r",
            "ls",
            "-l",
        ]);
        assert_eq!(
            j.userspec.as_deref(),
            Some(&b"fix"[..]),
            "one colon dropped"
        );
        assert_eq!(j.groups.as_deref(), Some(&b"a,b"[..]));
        assert!(j.skip_chdir);
        assert_eq!(j.newroot, OsString::from("/r"));
        assert_eq!(j.command, argv(&["ls", "-l"]));
        // Only one trailing colon goes.
        assert_eq!(
            job(&["--userspec=u::", "/r"]).userspec.as_deref(),
            Some(&b"u:"[..])
        );
        // No command is the interactive shell's case, and NEWROOT ends the
        // options: `--help` after it is the command's.
        let j = job(&["/r", "--help"]);
        assert_eq!(j.command, argv(&["--help"]));
        assert!(job(&["/r"]).command.is_empty());
        assert_eq!(job(&["--", "/r"]).newroot, OsString::from("/r"));
        assert_eq!(job(&["--groups=", "/r"]).groups.as_deref(), Some(&b""[..]));
    }

    #[test]
    fn the_command_line_is_refused_as_upstream_refuses_it() {
        let try_help = "\nTry 'chroot --help' for more information.";
        assert_eq!(refused(&[]), format!("missing operand{try_help}"));
        assert_eq!(
            refused(&["--skip-chdir"]),
            format!("missing operand{try_help}")
        );
        assert_eq!(refused(&["--"]), format!("missing operand{try_help}"));
        assert_eq!(refused(&["-x"]), format!("invalid option -- 'x'{try_help}"));
        assert_eq!(
            refused(&["--userspec"]),
            format!("option '--userspec' requires an argument{try_help}")
        );
        assert!(refused(&["--bogus"]).starts_with("unrecognized option '--bogus'"));
        assert_eq!(scan(&argv(&["--help", "--bogus"])), Ok(Request::Help));
        assert_eq!(scan(&argv(&["--version", "/r"])), Ok(Request::Version));
        assert_eq!(scan(&argv(&["--h"])), Ok(Request::Help));
    }

    #[test]
    fn a_group_list_reads_names_and_numbers_as_upstream_does() {
        let db = db();
        let read = |s: &str| parse_additional_groups(s.as_bytes(), &db, false);
        assert_eq!(read("fixg"), Ok(vec![4322]));
        assert_eq!(read("fixg,other"), Ok(vec![4322, 4400]));
        assert_eq!(
            read(",fixg,,other,"),
            Ok(vec![4322, 4400]),
            "strtok skips empties"
        );
        assert_eq!(read("4322"), Ok(vec![4322]));
        assert_eq!(read("4401"), Ok(vec![4402]), "a group called 4401 wins");
        assert_eq!(
            read("+4401"),
            Ok(vec![4401]),
            "...unless a + asks for the number"
        );
        assert_eq!(read(" 4322"), Ok(vec![4322]));
        assert_eq!(
            read("\x0b4401"),
            Ok(vec![4402]),
            "isspace includes vertical tab"
        );
        assert_eq!(read("4294967295"), Ok(vec![u32::MAX]), "all of a gid_t");
        assert_eq!(read("4294967296"), Err(()));
        assert_eq!(read("-1"), Err(()));
        assert_eq!(read("0x10"), Err(()));
        assert_eq!(read("nosuch"), Err(()));
        assert_eq!(read("fixg,nosuch"), Err(()));
        assert_eq!(read(","), Err(()), "no words at all");
        assert_eq!(read(""), Err(()));
    }

    #[test]
    fn a_userspec_fills_only_what_it_names() {
        let db = db();
        let mut ids = Ids {
            uid: Some(1),
            gid: Some(2),
            ..Ids::default()
        };
        let (spec, _) = userspec::parse_user_spec(b":fixg", &db).unwrap();
        ids.take(&spec);
        assert_eq!((ids.uid, ids.gid), (Some(1), Some(4322)), "the uid kept");
        let (spec, _) = userspec::parse_user_spec(b"fix", &db).unwrap();
        ids.take(&spec);
        assert_eq!((ids.uid, ids.gid), (Some(4321), Some(4322)), "the gid kept");
    }

    #[test]
    fn the_login_group_is_looked_up_only_when_upstream_looks() {
        let db = db();
        // A uid with no gid: the account's group, and its name.
        let mut ids = Ids {
            uid: Some(4321),
            ..Ids::default()
        };
        assert!(ids.login_group(&db, false));
        assert_eq!(ids.gid, Some(4322));
        assert_eq!(ids.username.as_deref(), Some(&b"fix"[..]));
        // --groups given and a gid known: no lookup at all.
        let mut ids = Ids {
            uid: Some(4321),
            gid: Some(7),
            ..Ids::default()
        };
        assert!(ids.login_group(&db, true));
        assert_eq!((ids.gid, ids.username), (Some(7), None));
        // A uid nobody has.
        let mut ids = Ids {
            uid: Some(12345),
            ..Ids::default()
        };
        assert!(!ids.login_group(&db, false));
        // No uid: nothing to look up.
        assert!(Ids::default().login_group(&db, false));
    }

    #[test]
    fn the_help_is_upstream_s_text() {
        assert!(HELP.starts_with("Usage: chroot [OPTION] NEWROOT [COMMAND [ARG]...]\n"));
        assert!(HELP.contains("run '\"$SHELL\" -i' (default: '/bin/sh -i')."));
        assert!(HELP.ends_with("  -    the exit status of COMMAND otherwise\n"));
    }

    #[cfg(unix)]
    #[test]
    fn only_the_root_by_another_name_is_the_root() {
        assert!(is_root(b"/"));
        assert!(is_root(b"//"));
        assert!(is_root(b"/."));
        assert!(!is_root(b"/nonexistent"));
        assert!(!is_root(b""));
    }
}
