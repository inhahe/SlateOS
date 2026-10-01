//! `backupd` -- the service that runs everyone's scheduled backups: once at
//! startup, and every 15 minutes after.
//!
//! The operator's answer to C-Q21 (design-decisions §1426): a backup set to
//! run daily runs on time whether or not anyone is signed in, and one missed
//! while the machine was off runs as soon as it is on again -- before anyone
//! signs in, and without asking. Lane E's `backup` program already knows how:
//! `backup run-due` reads its user's schedules, runs each one whose time has
//! come (a missed one included, once), and records when it ran
//! (`requests/e-db-the-backup-service-runs-backup-run-due.md`). So this
//! service never reads a schedule. What it knows is *whose* schedules there
//! are, and it runs `backup run-due` for each of them, as them.
//!
//! # One check
//!
//! 1. The user database is read (`getpwent`, the C library's -- the same
//!    accounts every other program sees).
//! 2. A user takes part when `<home>/.config/slateos/backup/schedules.json`
//!    exists: the file `backup schedule` writes, found where `backup` will
//!    look for it, since the run is given `HOME=<home>` and no
//!    `XDG_CONFIG_HOME`.
//! 3. A user whose run from an earlier check is still going is skipped -- a
//!    backup can take longer than the interval -- and the skip is logged.
//! 4. Otherwise `backup run-due` starts **as that user**: the child takes the
//!    user's groups, then their gid, then their uid, in that order, between
//!    `fork` and `exec` (the groups need `CAP_SETGID`, which giving up the
//!    uid would lose). It gets a clean environment -- `HOME`, `USER`,
//!    `LOGNAME`, `PATH` -- and never another user's anything.
//! 5. Every line it prints, and how it ended, goes to the system journal
//!    (`/var/log/syslog.jsonl`, `journalrec`'s records), attributed to the
//!    user.
//!
//! Run as anyone but root, the service can only be that one user, so it
//! considers that account alone and changes no identity.
//!
//! # What the switch does not do yet
//!
//! On SlateOS today a process that changes its uid keeps every capability it
//! held (`requests/d-a-a-process-that-gives-up-root-keeps-roots-authority.md`),
//! so a run started this way is its user to anything that asks who it is, and
//! still holds root's authority. The switch is the POSIX one so that it is
//! right the day that changes; nothing here would need to.
//!
//! Nor is the service installed or started yet: `known-issues.md` ->
//! `D-SCHEDULED-BACKUPS-STILL-DO-NOT-RUN` has what it waits for.
//!
//! # Layout
//!
//! This file is the part that does not touch the operating system -- which
//! accounts take part, when a run may start, what a journal record says --
//! so all of it is tested on the development host. `main.rs` is the other
//! part: the user database, the child's identity change, the output pipes,
//! the clock.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where `backup` keeps a user's schedules, under their home: its
/// `settingsfile::config_dir()` (`$HOME/.config/slateos` when
/// `XDG_CONFIG_HOME` is unset, as it is in the runs this service starts),
/// then `backup/schedules.json`.
pub const SCHEDULES_UNDER_HOME: &str = ".config/slateos/backup/schedules.json";

/// How often a check runs after the first, by default: the request's
/// "every 15 minutes". A shorter interval costs one file test per user when
/// nothing is due.
pub const DEFAULT_INTERVAL_SECS: u64 = 15 * 60;

/// The program run for each user, by default.
pub const DEFAULT_BACKUP: &str = "/bin/backup";

/// The name this service's journal records carry.
pub const SERVICE: &str = "backupd";

/// How long a run's output may stay open after the run itself has ended,
/// before the run is counted as over anyway. Its output closes when every
/// process holding it has exited, and `backup` could leave a helper running
/// that holds it: waiting for that would hold the user's backups back
/// forever, since a user whose run has not ended is skipped.
pub const STREAM_GRACE_SECS: u64 = 5;

/// The longest piece of a run's output one journal record carries. A longer
/// line is journalled in pieces this size, so a run printing without newlines
/// cannot make this service hold its whole output in memory.
pub const MAX_LINE: usize = 4096;

/// One account, as the user database gives it: what a run needs to become
/// that user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Login name, as bytes: a name need not be text.
    pub name: Vec<u8>,
    /// User id.
    pub uid: u32,
    /// Login group.
    pub gid: u32,
    /// Home directory.
    pub home: PathBuf,
}

impl Account {
    /// The schedules file `backup` reads for this account.
    #[must_use]
    pub fn schedules(&self) -> PathBuf {
        self.home.join(SCHEDULES_UNDER_HOME)
    }

    /// The name as a journal can show it: `quoting`'s rendering of bytes
    /// that are not printable text, never a lossy decode.
    #[must_use]
    pub fn shown_name(&self) -> String {
        quoting::escape_unprintable(&self.name)
    }
}

/// The accounts a check considers, of those the user database lists.
///
/// Running as root, every account; as anyone else, only the account whose
/// uid is ours -- we could not become any other. A uid listed twice is
/// considered once, as the first entry (`getpwuid`'s answer), since two
/// entries for one uid are one set of files. An account whose home is empty
/// or relative has nowhere `backup` would look, and is left out.  (`has_root`
/// rather than `is_absolute`: on the target they agree, and on the Windows
/// host where these tests run, `/home/alice` has a root and no drive.)
#[must_use]
pub fn candidates(all: &[Account], our_uid: u32) -> Vec<Account> {
    let mut seen = std::collections::HashSet::new();
    all.iter()
        .filter(|a| our_uid == 0 || a.uid == our_uid)
        .filter(|a| a.home.has_root())
        .filter(|a| seen.insert(a.uid))
        .cloned()
        .collect()
}

/// The accounts that have schedules, of the candidates: those whose
/// schedules file exists, as `exists` reports it.
pub fn with_schedules(
    candidates: &[Account],
    mut exists: impl FnMut(&Path) -> bool,
) -> Vec<&Account> {
    candidates
        .iter()
        .filter(|a| exists(&a.schedules()))
        .collect()
}

/// What one check decided for one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Start `backup run-due` for it.
    Start,
    /// Its run from an earlier check is still going: leave it.
    StillRunning,
}

/// Which accounts have a run going, by uid, and since which check.
///
/// The one rule it keeps is the request's fourth: never two runs at once
/// for one user.
#[derive(Debug, Default)]
pub struct Runs {
    running: HashMap<u32, u64>,
}

impl Runs {
    /// An empty table: nothing running.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What to do for `uid` at the check numbered `check`.
    #[must_use]
    pub fn decide(&self, uid: u32) -> Decision {
        if self.running.contains_key(&uid) {
            Decision::StillRunning
        } else {
            Decision::Start
        }
    }

    /// A run for `uid` started at check `check`.
    pub fn started(&mut self, uid: u32, check: u64) {
        self.running.insert(uid, check);
    }

    /// The run for `uid` ended (or never got going).
    pub fn ended(&mut self, uid: u32) {
        self.running.remove(&uid);
    }

    /// The check at which `uid`'s current run started, if one is going.
    #[must_use]
    pub fn since(&self, uid: u32) -> Option<u64> {
        self.running.get(&uid).copied()
    }

    /// Whether anything is running.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }
}

/// A journal priority, in `journalrec`'s spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Something went wrong: a run that could not start, or ended badly.
    Err,
    /// Worth noticing: a line a run wrote to its error stream.
    Warning,
    /// Ordinary progress.
    Info,
}

impl Level {
    /// The name `journalctl` reads.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Err => "err",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One journal record from this service, as a JSON line without its newline.
///
/// `who` is the account a record is about, shown first in the message -- the
/// journal's records carry no user field, and a line from `backup` means
/// nothing without whose backup it was. `text` is bytes because a run's
/// output is; it is rendered by `quoting`, never decoded lossily, and
/// `journalrec` escapes what is left, so no line a run prints can end the
/// record or forge another.
#[must_use]
pub fn record(
    ts: u64,
    level: Level,
    who: Option<&Account>,
    text: &[u8],
    pid: Option<u32>,
) -> String {
    let body = quoting::escape_unprintable(text);
    let msg = match who {
        Some(a) => format!("{}: {body}", a.shown_name()),
        None => body,
    };
    journalrec::Record {
        ts,
        level: level.name().to_owned(),
        service: SERVICE.to_owned(),
        msg,
        pid,
    }
    .to_json_line()
}

/// How a run ended, in words for the journal, and at what level.
///
/// `backup run-due` exits 0 when nothing was due or everything due ran whole,
/// and 1 when something due did not run whole or the schedule file did not
/// read -- each case already explained by the lines it printed. Anything else
/// is a failure of the program itself.
#[must_use]
pub fn ending(code: Option<i32>, signal: Option<i32>) -> (Level, String) {
    match (code, signal) {
        (Some(0), _) => (Level::Info, "backup run-due finished".to_owned()),
        (Some(1), _) => (
            Level::Warning,
            "backup run-due finished, and something due did not run whole (said above)".to_owned(),
        ),
        (Some(c), _) => (
            Level::Err,
            format!("backup run-due failed with exit status {c}"),
        ),
        (None, Some(s)) => (
            Level::Err,
            format!("backup run-due was ended by signal {s}"),
        ),
        (None, None) => (
            Level::Err,
            "backup run-due ended, and how is not known".to_owned(),
        ),
    }
}

/// The journal line naming the accounts that have schedules, written when
/// that set changes (and at the first check), so the journal says whose
/// backups this service is looking after without repeating it every 15
/// minutes.
#[must_use]
pub fn summary(with: &[&Account]) -> String {
    if with.is_empty() {
        return "no account has backup schedules".to_owned();
    }
    let names: Vec<String> = with.iter().map(|a| a.shown_name()).collect();
    format!(
        "{} with backup schedules: {}",
        if with.len() == 1 {
            "1 account".to_owned()
        } else {
            format!("{} accounts", with.len())
        },
        names.join(", ")
    )
}

/// Whether the set of accounts with schedules has changed since the last
/// check (`None`: there was none), by uid.
#[must_use]
pub fn changed(last: Option<&[u32]>, now: &[&Account]) -> bool {
    let now: Vec<u32> = now.iter().map(|a| a.uid).collect();
    last != Some(now.as_slice())
}

/// The note journalled with a run's ending when its output was still open:
/// something it started holds it, and whatever that prints is journalled as
/// it arrives.
pub const STILL_OPEN: &str = "its output is still open -- a process it started holds it -- so the run is counted as over; any later lines are journalled as they come";

/// The command line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The program to run for each user.
    pub backup: PathBuf,
    /// Seconds between checks.
    pub interval_secs: u64,
    /// One check, wait for its runs, then exit -- for testing it by hand.
    pub once: bool,
    /// The journal file.
    pub log: PathBuf,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            backup: PathBuf::from(DEFAULT_BACKUP),
            interval_secs: DEFAULT_INTERVAL_SECS,
            once: false,
            log: PathBuf::from(journalrec::MAIN_LOG_PATH),
        }
    }
}

/// The usage line.
pub const USAGE: &str =
    "usage: backupd [--backup PROGRAM] [--interval SECONDS] [--log FILE] [--once]";

/// Parse the arguments after the program name. An option this does not know,
/// or one missing its value, or an interval that is not a whole number of
/// seconds from 1 up, is an error naming it.
pub fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let a_bytes = quoting::os_bytes(a);
        match a_bytes.as_ref() {
            b"--backup" => {
                o.backup = PathBuf::from(it.next().ok_or("--backup needs a program")?);
            }
            b"--log" => {
                o.log = PathBuf::from(it.next().ok_or("--log needs a file")?);
            }
            b"--interval" => {
                let v = it.next().ok_or("--interval needs a number of seconds")?;
                let v = quoting::os_bytes(v);
                let secs = std::str::from_utf8(&v)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|&s| s > 0)
                    .ok_or_else(|| {
                        format!(
                            "--interval: {} is not a whole number of seconds, 1 or more",
                            quoting::quote(&v)
                        )
                    })?;
                o.interval_secs = secs;
            }
            b"--once" => o.once = true,
            b"--help" | b"-h" => return Err(USAGE.to_owned()),
            other => return Err(format!("unknown option {}\n{USAGE}", quoting::quote(other))),
        }
    }
    Ok(o)
}

#[cfg(test)]
// A test's `unwrap` is its assertion: a wrong answer should stop it there.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn acct(name: &str, uid: u32, home: &str) -> Account {
        Account {
            name: name.as_bytes().to_vec(),
            uid,
            gid: uid,
            home: PathBuf::from(home),
        }
    }

    /// The file is the one `backup schedule` writes, under the home the run
    /// is given.
    #[test]
    fn the_schedules_file_is_under_the_home() {
        let a = acct("alice", 1000, "/home/alice");
        assert_eq!(
            a.schedules(),
            Path::new("/home/alice").join(".config/slateos/backup/schedules.json")
        );
    }

    /// Root considers everyone; anyone else only themselves -- they could not
    /// become anybody else.
    #[test]
    fn root_considers_every_account_and_a_user_only_itself() {
        let all = [
            acct("root", 0, "/root"),
            acct("alice", 1000, "/home/alice"),
            acct("bob", 1001, "/home/bob"),
        ];
        assert_eq!(candidates(&all, 0).len(), 3);
        let mine = candidates(&all, 1000);
        assert_eq!(mine, [acct("alice", 1000, "/home/alice")]);
        assert!(candidates(&all, 4242).is_empty());
    }

    /// A uid twice is one set of files: the first entry, as `getpwuid`
    /// answers. A home that is empty or relative is nowhere to look.
    #[test]
    fn duplicates_and_homeless_accounts_are_left_out() {
        let all = [
            acct("alice", 1000, "/home/alice"),
            acct("alias", 1000, "/home/alias"),
            acct("nohome", 1002, ""),
            acct("relative", 1003, "home/relative"),
        ];
        let c = candidates(&all, 0);
        assert_eq!(c.len(), 1);
        assert_eq!(c.first().map(|a| a.name.as_slice()), Some(&b"alice"[..]));
    }

    /// Only the accounts whose schedules file exists take part.
    #[test]
    fn only_accounts_with_a_schedules_file_take_part() {
        let all = [
            acct("alice", 1000, "/home/alice"),
            acct("bob", 1001, "/home/bob"),
        ];
        let have = with_schedules(&all, |p| p.starts_with("/home/bob"));
        assert_eq!(have.len(), 1);
        assert_eq!(have.first().map(|a| a.uid), Some(1001));
    }

    /// The request's fourth rule: never two runs at once for one user, and a
    /// finished run frees the user for the next check.
    #[test]
    fn a_user_whose_run_is_going_is_skipped_until_it_ends() {
        let mut runs = Runs::new();
        assert_eq!(runs.decide(1000), Decision::Start);
        runs.started(1000, 1);
        assert_eq!(runs.decide(1000), Decision::StillRunning);
        assert_eq!(
            runs.decide(1001),
            Decision::Start,
            "another user is not held up"
        );
        assert_eq!(runs.since(1000), Some(1));
        runs.ended(1000);
        assert_eq!(runs.decide(1000), Decision::Start);
        assert!(runs.is_empty());
    }

    /// A record is `journalrec`'s, the user first in the message.
    #[test]
    fn a_record_is_a_journal_line_naming_the_user() {
        let a = acct("alice", 1000, "/home/alice");
        let line = record(
            1_716_000_000,
            Level::Info,
            Some(&a),
            b"Nothing is due.",
            Some(42),
        );
        assert_eq!(
            line,
            r#"{"ts":1716000000,"level":"info","service":"backupd","msg":"alice: Nothing is due.","pid":42}"#
        );
        let line = record(
            7,
            Level::Err,
            None,
            b"could not read the user database",
            None,
        );
        assert_eq!(
            line,
            r#"{"ts":7,"level":"err","service":"backupd","msg":"could not read the user database"}"#
        );
    }

    /// What a run prints cannot end the record or forge another: a newline is
    /// rendered, a quote escaped, and bytes that are not text shown as
    /// escapes, never decoded lossily.
    #[test]
    fn a_line_from_a_run_cannot_forge_a_record() {
        let a = acct("alice", 1000, "/home/alice");
        let line = record(
            1,
            Level::Warning,
            Some(&a),
            b"a\n{\"ts\":0} \"q\" \xff",
            None,
        );
        assert!(!line.contains('\n'), "a raw newline survived: {line}");
        assert!(line.contains(r"\\012"), "the newline, rendered: {line}");
        assert!(line.contains(r#"\"q\""#), "the quotes, escaped: {line}");
        assert!(line.contains(r"\\377"), "the byte, as an escape: {line}");
        // A name that is not text is shown, not mangled.
        let odd = Account {
            name: b"caf\xe9".to_vec(),
            ..acct("x", 1, "/h")
        };
        assert!(record(1, Level::Info, Some(&odd), b"x", None).contains(r"caf\\351: x"));
    }

    /// How a run ended, in words, at the level it deserves: exit 1 is
    /// `backup`'s "something due did not run whole", already explained.
    #[test]
    fn a_runs_ending_is_named() {
        assert_eq!(ending(Some(0), None).0, Level::Info);
        assert_eq!(ending(Some(1), None).0, Level::Warning);
        let (l, m) = ending(Some(3), None);
        assert_eq!(
            (l, m.as_str()),
            (Level::Err, "backup run-due failed with exit status 3")
        );
        let (l, m) = ending(None, Some(9));
        assert_eq!(
            (l, m.as_str()),
            (Level::Err, "backup run-due was ended by signal 9")
        );
    }

    /// The summary names the accounts, and says so when there are none.
    #[test]
    fn the_summary_names_whose_backups_are_looked_after() {
        let (a, b) = (
            acct("alice", 1000, "/home/alice"),
            acct("bob", 1001, "/home/bob"),
        );
        assert_eq!(summary(&[]), "no account has backup schedules");
        assert_eq!(summary(&[&a]), "1 account with backup schedules: alice");
        assert_eq!(
            summary(&[&a, &b]),
            "2 accounts with backup schedules: alice, bob"
        );
    }

    /// The summary is repeated only when the set changes: at the first check,
    /// and when an account gains or loses its schedules.
    #[test]
    fn the_summary_is_repeated_only_when_the_set_changes() {
        let (a, b) = (
            acct("alice", 1000, "/home/alice"),
            acct("bob", 1001, "/home/bob"),
        );
        assert!(changed(None, &[]), "the first check always says");
        assert!(!changed(Some(&[]), &[]));
        assert!(!changed(Some(&[1000]), &[&a]));
        assert!(changed(Some(&[1000]), &[&a, &b]));
        assert!(changed(Some(&[1000, 1001]), &[&b]));
    }

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_defaults_are_the_requests() {
        let o = parse_args(&[]).unwrap();
        assert_eq!(o.backup, Path::new("/bin/backup"));
        assert_eq!(o.interval_secs, 900);
        assert!(!o.once);
        assert_eq!(o.log, Path::new("/var/log/syslog.jsonl"));
    }

    #[test]
    fn every_option_is_read() {
        let o = parse_args(&args(&[
            "--backup",
            "/mnt/bin/backup",
            "--interval",
            "60",
            "--log",
            "/tmp/j",
            "--once",
        ]))
        .unwrap();
        assert_eq!(
            o,
            Options {
                backup: PathBuf::from("/mnt/bin/backup"),
                interval_secs: 60,
                once: true,
                log: PathBuf::from("/tmp/j"),
            }
        );
    }

    /// A bad or missing value is refused by name, never replaced by a
    /// default.
    #[test]
    fn a_bad_option_is_refused_by_name() {
        let e = parse_args(&args(&["--interval", "0"])).unwrap_err();
        assert!(e.contains("\u{2018}0\u{2019} is not a whole number"), "{e}");
        let e = parse_args(&args(&["--interval", "15m"])).unwrap_err();
        assert!(e.contains("\u{2018}15m\u{2019}"), "{e}");
        let e = parse_args(&args(&["--interval"])).unwrap_err();
        assert!(e.contains("needs a number"), "{e}");
        let e = parse_args(&args(&["--frobnicate"])).unwrap_err();
        assert!(
            e.contains("unknown option \u{2018}--frobnicate\u{2019}"),
            "{e}"
        );
        assert!(e.contains(USAGE));
    }
}
