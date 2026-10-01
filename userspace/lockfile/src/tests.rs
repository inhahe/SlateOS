// A test panics on a broken expectation by design: that is how it reports.
// The fake system's counters (a clock, inode numbers, link counts) are
// small and cannot overflow in a test's handful of operations.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use super::*;
use std::collections::{BTreeMap, BTreeSet};

const EACCES: i32 = 13;

#[derive(Clone, Debug)]
struct Node {
    nlink: u64,
    mtime: i64,
    mode: u32,
    content: Vec<u8>,
}

/// A file handle: the inode it is open on.
struct FakeFile(u64);

/// An in-memory system: names to inodes, a clock that moves only when
/// lockfile sleeps, and switches for the awkward cases.
struct Fake {
    names: BTreeMap<Vec<u8>, u64>,
    nodes: BTreeMap<u64, Node>,
    dirs: BTreeSet<Vec<u8>>,
    next_ino: u64,
    clock: i64,
    stderr: Vec<u8>,
    sleeps: Vec<u32>,
    uid: u32,
    setuid_works: bool,
    pid: u64,
    nodename: Vec<u8>,
    logname: Option<Vec<u8>>,
    users: Vec<(u32, Vec<u8>)>,
    exitflag: i32,
    /// Every name `create_excl` made, in order.
    created: Vec<Vec<u8>>,
    /// The longest final path component the filesystem takes.
    name_max: Option<usize>,
    /// `link` fails with this errno, without linking.
    link_errno: Option<i32>,
    /// `link` links, then reports EIO anyway -- as NFS can.
    link_lies: bool,
    /// Names whose unlink is refused.
    undeletable: BTreeSet<Vec<u8>>,
    /// After this many sleeps, this name is removed (another holder lets go).
    release_after: Option<(usize, Vec<u8>)>,
    /// After this many sleeps, a terminating signal arrives.
    signal_after: Option<usize>,
}

impl Fake {
    fn new() -> Self {
        Self {
            names: BTreeMap::new(),
            nodes: BTreeMap::new(),
            dirs: BTreeSet::new(),
            next_ino: 100,
            clock: 64,
            stderr: Vec::new(),
            sleeps: Vec::new(),
            uid: 1000,
            setuid_works: true,
            pid: 1,
            nodename: b"host".to_vec(),
            logname: None,
            users: vec![(1000, b"me".to_vec())],
            exitflag: 0,
            created: Vec::new(),
            name_max: None,
            link_errno: None,
            link_lies: false,
            undeletable: BTreeSet::new(),
            release_after: None,
            signal_after: None,
        }
    }

    /// A lock someone else holds, made `age` seconds ago.
    fn held(&mut self, name: &str, age: i64) {
        let ino = self.new_node(0o444, self.clock - age);
        self.nodes.get_mut(&ino).unwrap().content = b"0".to_vec();
        self.names.insert(name.as_bytes().to_vec(), ino);
    }

    fn new_node(&mut self, mode: u32, mtime: i64) -> u64 {
        self.next_ino += 1;
        let ino = self.next_ino;
        self.nodes.insert(
            ino,
            Node {
                nlink: 1,
                mtime,
                mode,
                content: Vec::new(),
            },
        );
        ino
    }

    fn check_path(&self, path: &[u8]) -> Result<(), i32> {
        let dir = &path[..lastdirsep(path)];
        if !dir.is_empty() && !self.dirs.contains(dir) {
            return Err(ENOENT);
        }
        if let Some(max) = self.name_max {
            if path.len() - lastdirsep(path) > max {
                return Err(ENAMETOOLONG);
            }
        }
        Ok(())
    }

    fn stat_of(&self, ino: u64) -> Stat {
        let n = &self.nodes[&ino];
        Stat {
            dev: 1,
            ino,
            uid: self.uid,
            gid: 100,
            nlink: n.nlink,
            size: n.content.len() as u64,
            mtime: n.mtime,
            mode: 0o100_000 | n.mode,
        }
    }

    fn file(&self, name: &str) -> Option<&Node> {
        self.names.get(name.as_bytes()).map(|ino| &self.nodes[ino])
    }

    fn listing(&self) -> Vec<String> {
        self.names
            .keys()
            .map(|k| String::from_utf8_lossy(k).into_owned())
            .collect()
    }

    fn err(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

impl System for Fake {
    type File = FakeFile;

    fn elog(&mut self, bytes: &[u8]) {
        self.stderr.extend_from_slice(bytes);
    }
    fn getuid(&mut self) -> u32 {
        self.uid
    }
    fn geteuid(&mut self) -> u32 {
        self.uid
    }
    fn setuid(&mut self, _uid: u32) -> bool {
        self.setuid_works
    }
    fn getgid(&mut self) -> u32 {
        100
    }
    fn getegid(&mut self) -> u32 {
        100
    }
    fn setgid(&mut self, _gid: u32) -> bool {
        true
    }
    fn getpid(&mut self) -> u64 {
        self.pid
    }
    fn time(&mut self) -> i64 {
        self.clock
    }
    fn nodename(&mut self) -> Vec<u8> {
        self.nodename.clone()
    }
    fn logname(&mut self) -> Option<Vec<u8>> {
        self.logname.clone()
    }
    fn user_by_name(&mut self, name: &[u8]) -> Option<(u32, Vec<u8>)> {
        self.users.iter().find(|(_, n)| n == name).cloned()
    }
    fn user_by_uid(&mut self, uid: u32) -> Option<Vec<u8>> {
        self.users
            .iter()
            .find(|(u, _)| *u == uid)
            .map(|(_, n)| n.clone())
    }
    fn lstat(&mut self, path: &[u8]) -> Result<Stat, i32> {
        self.check_path(path)?;
        self.names
            .get(path)
            .map(|&ino| self.stat_of(ino))
            .ok_or(ENOENT)
    }
    fn stat(&mut self, path: &[u8]) -> Result<Stat, i32> {
        self.lstat(path)
    }
    fn create_excl(&mut self, path: &[u8], mode: u32) -> Result<FakeFile, i32> {
        self.check_path(path)?;
        if self.names.contains_key(path) {
            return Err(EEXIST);
        }
        let ino = self.new_node(mode & 0o7777, self.clock);
        self.names.insert(path.to_vec(), ino);
        self.created.push(path.to_vec());
        Ok(FakeFile(ino))
    }
    fn fstat(&mut self, file: &FakeFile) -> Result<Stat, i32> {
        Ok(self.stat_of(file.0))
    }
    fn write(&mut self, file: &mut FakeFile, bytes: &[u8]) {
        self.nodes
            .get_mut(&file.0)
            .unwrap()
            .content
            .extend_from_slice(bytes);
    }
    fn close(&mut self, _file: FakeFile) {}
    fn link(&mut self, old: &[u8], new: &[u8]) -> Result<(), i32> {
        if let Some(e) = self.link_errno {
            return Err(e);
        }
        self.check_path(new)?;
        if self.names.contains_key(new) {
            return Err(EEXIST);
        }
        let ino = *self.names.get(old).ok_or(ENOENT)?;
        self.names.insert(new.to_vec(), ino);
        self.nodes.get_mut(&ino).unwrap().nlink += 1;
        if self.link_lies { Err(EIO) } else { Ok(()) }
    }
    fn unlink(&mut self, path: &[u8]) -> Result<(), i32> {
        if self.undeletable.contains(path) {
            return Err(EACCES);
        }
        let ino = self.names.remove(path).ok_or(ENOENT)?;
        let node = self.nodes.get_mut(&ino).unwrap();
        node.nlink -= 1;
        if node.nlink == 0 {
            self.nodes.remove(&ino);
        }
        Ok(())
    }
    fn sleep(&mut self, seconds: u32) {
        self.sleeps.push(seconds);
        self.clock += i64::from(seconds);
        if let Some((after, name)) = self.release_after.clone() {
            if self.sleeps.len() == after {
                self.names.remove(&name);
            }
        }
        if self.signal_after == Some(self.sleeps.len()) {
            self.exitflag = 2;
        }
    }
    fn exitflag(&mut self) -> i32 {
        self.exitflag
    }
    fn catch_signals(&mut self) {}
    fn ignore_sigpipe(&mut self) {}
}

fn run(fake: &mut Fake, words: &[&str]) -> i32 {
    let argv = std::iter::once("lockfile")
        .chain(words.iter().copied())
        .map(|w| w.as_bytes().to_vec())
        .collect();
    Lockfile::new(fake, argv).run()
}

fn usage() -> String {
    String::from_utf8(USAGE.to_vec()).unwrap()
}

// ---------------------------------------------------------------------------
// Taking a lock
// ---------------------------------------------------------------------------

#[test]
fn a_lock_is_read_only_holds_0_and_leaves_no_temporary() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["a.lock"]), 0);
    assert_eq!(f.listing(), ["a.lock"]);
    let node = f.file("a.lock").unwrap();
    assert_eq!(node.mode, LOCK_PERM);
    assert_eq!(node.content, b"0");
    assert_eq!(node.nlink, 1);
    assert_eq!(f.err(), "");
}

/// `_`, the pid, a separator, the time, `.`, the host -- the numbers in
/// procmail's base 64, least significant digit first. pid 1 is `B`; the
/// time 64 is `AB`.
#[test]
fn the_temporary_is_named_as_procmail_names_it() {
    let mut f = Fake::new();
    run(&mut f, &["dir/a.lock"]);
    assert!(f.err().contains("Try praying"), "{}", f.err()); // no dir/ yet
    let mut f = Fake::new();
    f.dirs.insert(b"dir/".to_vec());
    assert_eq!(run(&mut f, &["dir/a.lock"]), 0);
    assert_eq!(f.created, [b"dir/_B.AB.host".to_vec()]);
}

#[test]
fn a_long_host_name_is_cut_to_fit_the_temporary() {
    let mut f = Fake::new();
    f.nodename = b"a-very-long-host-name.example.org".to_vec();
    assert_eq!(run(&mut f, &["x"]), 0);
    let name = &f.created[0];
    assert_eq!(name.len(), UNIQ_NAME_LEN - 1);
    assert!(
        name.starts_with(b"_B.AB.a-very-long"),
        "{}",
        String::from_utf8_lossy(name)
    );
}

#[test]
fn a_held_lock_with_no_retries_is_sorry_and_73() {
    let mut f = Fake::new();
    f.held("a.lock", 0);
    assert_eq!(run(&mut f, &["-r0", "a.lock"]), EX_CANTCREAT);
    assert_eq!(f.err(), "lockfile: Sorry, giving up on \"a.lock\"\n");
    assert_eq!(f.file("a.lock").unwrap().content, b"0"); // left alone
}

#[test]
fn retries_sleep_the_interval_between_attempts() {
    let mut f = Fake::new();
    f.held("a.lock", 0);
    assert_eq!(run(&mut f, &["-3", "-r", "2", "a.lock"]), EX_CANTCREAT);
    assert_eq!(f.sleeps, [3, 3]);
}

#[test]
fn it_waits_and_takes_the_lock_when_it_is_released() {
    let mut f = Fake::new();
    f.held("a.lock", 0);
    f.release_after = Some((2, b"a.lock".to_vec()));
    assert_eq!(run(&mut f, &["-1", "a.lock"]), 0);
    assert_eq!(f.sleeps, [1, 1]);
    assert_eq!(f.file("a.lock").unwrap().mode, LOCK_PERM);
}

#[test]
fn a_lock_older_than_l_seconds_is_forced() {
    let mut f = Fake::new();
    f.held("a.lock", 100);
    assert_eq!(run(&mut f, &["-l", "10", "-s", "2", "a.lock"]), 0);
    assert_eq!(f.err(), "lockfile: Forcing lock on \"a.lock\"\n");
    assert_eq!(f.sleeps, [2]);
}

#[test]
fn a_forced_unlock_that_is_refused_says_so_and_waits() {
    let mut f = Fake::new();
    f.held("a.lock", 100);
    f.undeletable.insert(b"a.lock".to_vec());
    f.signal_after = Some(2);
    assert_eq!(run(&mut f, &["-l", "10", "-s", "1", "a.lock"]), EX_TEMPFAIL);
    assert!(
        f.err()
            .starts_with("lockfile: Forced unlock denied on \"a.lock\"\n"),
        "{}",
        f.err()
    );
}

#[test]
fn a_young_lock_is_not_forced() {
    let mut f = Fake::new();
    f.held("a.lock", 5);
    assert_eq!(run(&mut f, &["-l", "10", "-r0", "a.lock"]), EX_CANTCREAT);
    assert_eq!(f.err(), "lockfile: Sorry, giving up on \"a.lock\"\n");
}

#[test]
fn a_missing_directory_gives_up_at_once() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["nodir/x.lock"]), EX_UNAVAILABLE);
    assert_eq!(
        f.err(),
        "lockfile: Try praying, giving up on \"nodir/x.lock\"\n"
    );
    assert!(f.sleeps.is_empty());
}

#[test]
fn a_signal_ends_the_wait() {
    let mut f = Fake::new();
    f.held("a.lock", 0);
    f.signal_after = Some(1);
    assert_eq!(run(&mut f, &["-1", "a.lock"]), EX_TEMPFAIL);
    assert_eq!(
        f.err(),
        "lockfile: Signal received, giving up on \"a.lock\"\n"
    );
}

#[test]
fn a_name_too_long_is_shortened_one_byte_at_a_time() {
    let mut f = Fake::new();
    f.name_max = Some(20);
    let long = "a".repeat(23);
    assert_eq!(run(&mut f, &[long.as_str()]), 0);
    assert_eq!(f.listing(), ["a".repeat(20)]);
    assert_eq!(
        f.err().matches("and retrying lock").count(),
        3,
        "{}",
        f.err()
    );
    assert!(f.err().starts_with(&format!(
        "lockfile: Truncating \"{long}\" and retrying lock\n"
    )));
}

/// NFS can report failure for a link it made: the inodes say it worked.
#[test]
fn a_link_that_reports_failure_but_was_made_counts() {
    let mut f = Fake::new();
    f.link_lies = true;
    assert_eq!(run(&mut f, &["a.lock"]), 0);
    assert_eq!(f.listing(), ["a.lock"]);
    assert_eq!(f.file("a.lock").unwrap().nlink, 1);
}

/// Where the filesystem has no hard links, the lock is made by an exclusive
/// create, and is left empty -- procmail's fallback, kept.
#[test]
fn without_hard_links_the_lock_is_created_exclusively_and_empty() {
    let mut f = Fake::new();
    f.link_errno = Some(EXDEV);
    assert_eq!(run(&mut f, &["a.lock"]), 0);
    assert_eq!(f.listing(), ["a.lock"]);
    assert_eq!(f.file("a.lock").unwrap().content, b"");
}

#[test]
fn a_caller_who_cannot_drop_privileges_is_refused() {
    let mut f = Fake::new();
    f.setuid_works = false;
    assert_eq!(run(&mut f, &["a.lock"]), EX_OSERR);
    assert_eq!(f.err(), "lockfile: Unable to give up special permissions");
}

// ---------------------------------------------------------------------------
// The second pass, and the status
// ---------------------------------------------------------------------------

#[test]
fn a_failure_releases_the_locks_taken_before_it() {
    let mut f = Fake::new();
    f.held("held.lock", 0);
    assert_eq!(
        run(&mut f, &["-r0", "a.lock", "held.lock", "b.lock"]),
        EX_CANTCREAT
    );
    assert_eq!(f.listing(), ["held.lock"]);
}

#[test]
fn invert_turns_success_into_73_and_73_into_success() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-!", "a.lock"]), EX_CANTCREAT);
    let mut f = Fake::new();
    f.held("a.lock", 0);
    assert_eq!(run(&mut f, &["-!", "-r0", "a.lock"]), 0);
}

/// The second pass parses only the arguments before the failure, and its
/// `-!` is the one that counts.
#[test]
fn invert_after_the_failing_argument_does_not_count() {
    let mut f = Fake::new();
    f.held("a.lock", 0);
    assert_eq!(run(&mut f, &["-r0", "a.lock", "-!"]), EX_CANTCREAT);
}

#[test]
fn no_arguments_is_the_usage_line() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &[]), EX_USAGE);
    assert_eq!(f.err(), usage());
}

#[test]
fn options_alone_are_the_usage_line() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-5"]), EX_USAGE);
    assert_eq!(f.err(), usage());
}

/// Once per pass: the first pass fails at `x`, the second re-reads `-r` and
/// finds its number missing.
#[test]
fn a_bad_number_prints_the_usage_line_twice() {
    for words in [&["-r", "x"][..], &["-s", "-1"][..]] {
        let mut f = Fake::new();
        assert_eq!(run(&mut f, words), EX_USAGE, "{words:?}");
        assert_eq!(f.err(), usage().repeat(2), "{words:?}");
    }
}

#[test]
fn an_unknown_option_is_the_usage_line_and_creates_nothing() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-x", "a.lock"]), EX_USAGE);
    assert_eq!(f.err(), usage());
    assert!(f.listing().is_empty());
}

#[test]
fn a_usage_error_is_never_inverted() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-!", "-r", "x"]), EX_USAGE);
}

#[test]
fn help_and_version_go_to_stderr_with_status_64() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-h"]), EX_USAGE);
    assert_eq!(
        f.err(),
        format!("{}{}", usage(), String::from_utf8_lossy(HELP))
    );

    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-v"]), EX_USAGE);
    let want = format!(
        "lockfile{}\nYour system mailbox's lockfile:\t/var/mail/me.lock\n",
        String::from_utf8_lossy(VERSION)
    );
    assert_eq!(f.err(), want);
}

// ---------------------------------------------------------------------------
// The mailbox
// ---------------------------------------------------------------------------

#[test]
fn ml_locks_the_callers_mailbox_and_mu_unlocks_it() {
    let mut f = Fake::new();
    f.dirs.insert(b"/var/mail/".to_vec());
    assert_eq!(run(&mut f, &["-ml"]), 0);
    assert_eq!(f.listing(), ["/var/mail/me.lock"]);
    assert_eq!(run(&mut f, &["-mu"]), 0);
    assert!(f.listing().is_empty());
}

#[test]
fn logname_names_the_mailbox_only_when_its_uid_is_the_callers() {
    let mut f = Fake::new();
    f.dirs.insert(b"/var/mail/".to_vec());
    f.users.push((1000, b"alias".to_vec()));
    f.users.push((2000, b"other".to_vec()));
    f.logname = Some(b"alias".to_vec());
    assert_eq!(run(&mut f, &["-ml"]), 0);
    assert_eq!(f.listing(), ["/var/mail/alias.lock"]);

    let mut f = Fake::new();
    f.dirs.insert(b"/var/mail/".to_vec());
    f.users.push((2000, b"other".to_vec()));
    f.logname = Some(b"other".to_vec());
    assert_eq!(run(&mut f, &["-ml"]), 0);
    assert_eq!(f.listing(), ["/var/mail/me.lock"]);
}

#[test]
fn a_caller_with_no_passwd_entry_has_no_mailbox() {
    let mut f = Fake::new();
    f.users.clear();
    assert_eq!(run(&mut f, &["-ml"]), EX_USAGE);
    assert_eq!(
        f.err(),
        format!(
            "lockfile: Can't determine your mailbox, who are you?\n{}",
            usage()
        )
    );
}

#[test]
fn mu_that_cannot_unlock_says_so() {
    let mut f = Fake::new();
    assert_eq!(run(&mut f, &["-mu"]), EX_USAGE);
    assert_eq!(
        f.err(),
        format!("lockfile: Can't unlock \"/var/mail/me.lock\"\n{}", usage())
    );
}

/// The second pass releases the mailbox lock as it releases files.
#[test]
fn a_failure_after_ml_releases_the_mailbox() {
    let mut f = Fake::new();
    f.dirs.insert(b"/var/mail/".to_vec());
    f.held("held.lock", 0);
    assert_eq!(run(&mut f, &["-r0", "-ml", "held.lock"]), EX_CANTCREAT);
    assert_eq!(f.listing(), ["held.lock"]);
}

// ---------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------

#[test]
fn strtol_reads_as_c_does() {
    assert_eq!(strtol(b"12"), (12, 2));
    assert_eq!(strtol(b"  -7x"), (-7, 4));
    assert_eq!(strtol(b"+5"), (5, 2));
    assert_eq!(strtol(b"x"), (0, 0));
    assert_eq!(strtol(b"+"), (0, 0));
    assert_eq!(strtol(b"-"), (0, 0));
    assert_eq!(strtol(b""), (0, 0));
    // A long that does not fit an int keeps its low 32 bits.
    assert_eq!(strtol(b"99999999999"), (1_215_752_191, 11));
    assert_eq!(strtol(b"4294967296"), (0, 10));
}

#[test]
fn ultoan_is_base_64_least_significant_first() {
    let mut buf = [0xffu8; 8];
    assert_eq!(ultoan(0, &mut buf, 0), 1);
    assert_eq!(cstr(&buf), b"A");
    assert_eq!(ultoan(63, &mut buf, 0), 1);
    assert_eq!(cstr(&buf), b"_");
    assert_eq!(ultoan(62, &mut buf, 0), 1);
    assert_eq!(cstr(&buf), b"-");
    assert_eq!(ultoan(64, &mut buf, 0), 2);
    assert_eq!(cstr(&buf), b"AB");
    assert_eq!(ultoan(1234, &mut buf, 0), 2);
    assert_eq!(cstr(&buf), b"ST");
}

#[test]
fn safehost_writes_unsafe_characters_in_octal() {
    assert_eq!(safehost(b"a/b:c\\d"), b"a\\057b\\072c\\134d");
    assert_eq!(safehost(b"plain"), b"plain");
}

#[test]
fn lastdirsep_finds_the_name_after_the_last_slash() {
    assert_eq!(lastdirsep(b"a/b/c"), 4);
    assert_eq!(lastdirsep(b"abc"), 0);
    assert_eq!(lastdirsep(b"/"), 1);
}
