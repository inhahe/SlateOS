//! `backupd`'s loop: the clock, the user database, each run's identity and
//! output. What to do is decided in `lib.rs`; this is doing it.
//!
//! ```text
//! backupd [--backup PROGRAM] [--interval SECONDS] [--log FILE] [--once]
//! ```
//!
//! The service manager (`services/init`) starts it at boot and restarts it
//! if it dies. `--once` runs one check, waits for its runs, and exits -- for
//! running it by hand; its exit status is 1 when anything in that check went
//! wrong.
//!
//! # Threads
//!
//! The main thread owns everything that changes: the table of runs, the
//! children, the journal file. Each run's two output streams are read by a
//! thread of their own, so a run never blocks on a full pipe while another
//! is being waited for; those threads only format records and send them
//! here, one channel for all, and the main thread writes them. A thread's
//! records arrive in the order it sent them, so every line a stream carried
//! is journalled before the note that it closed.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ExitCode, ExitStatus};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use backupd::{
    Account, Decision, Level, MAX_LINE, Options, Runs, STILL_OPEN, STREAM_GRACE_SECS, candidates,
    changed, ending, parse_args, record, summary, with_schedules,
};

/// What the output threads tell the main thread.
enum Event {
    /// A journal record, ready to write: one line (or piece of one) a run
    /// printed.
    Record(String),
    /// One of the run `pid`'s two streams reached its end.
    Closed { pid: u32 },
}

/// A run in progress.
struct Running {
    child: Child,
    account: Account,
    /// How many of its two output streams are still open.
    open_streams: u8,
    /// Once it has ended: how (`None` when that could not be learned), and
    /// when the end was seen.
    exited: Option<(Option<ExitStatus>, Instant)>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The journal: appended to, one record a line; stderr when it cannot be.
struct Journal {
    path: std::path::PathBuf,
}

impl Journal {
    fn line(&self, line: &str) {
        let appended = self
            .path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?;
                // One write for the whole record, so another writer's record
                // cannot land inside this one.
                f.write_all(format!("{line}\n").as_bytes())
            });
        if appended.is_err() {
            eprintln!("{line}");
        }
    }

    fn write(&self, level: Level, who: Option<&Account>, text: &[u8], pid: Option<u32>) {
        self.line(&record(now_secs(), level, who, text, pid));
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("backupd: {msg}");
            return ExitCode::from(2);
        }
    };
    let journal = Journal {
        path: opts.log.clone(),
    };
    journal.write(
        Level::Info,
        None,
        format!(
            "started: {} run-due for each account with backup schedules, every {} s",
            quoting::quote(&quoting::os_bytes(opts.backup.as_os_str())),
            opts.interval_secs
        )
        .as_bytes(),
        None,
    );
    if run(&opts, &journal) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The loop. Returns, only with `--once`, whether that one check went
/// without anything going wrong.
fn run(opts: &Options, journal: &Journal) -> bool {
    let (tx, rx) = mpsc::channel::<Event>();
    let mut runs = Runs::new();
    let mut live: Vec<Running> = Vec::new();
    let mut last_with: Option<Vec<u32>> = None;
    let mut all_well = true;
    let interval = Duration::from_secs(opts.interval_secs);
    let mut next = Instant::now();
    let mut check_no: u64 = 0;
    loop {
        if Instant::now() >= next && !(opts.once && check_no >= 1) {
            check_no = check_no.saturating_add(1);
            let ok = check(
                opts,
                journal,
                &tx,
                &mut runs,
                &mut live,
                &mut last_with,
                check_no,
            );
            all_well &= ok;
            // Measured from the end of this check, so a slow check (a large
            // user database) never makes the next one start at once.
            next = Instant::now()
                .checked_add(interval)
                .unwrap_or_else(Instant::now);
        }
        // Records as they come, a second at most between looks at the
        // children: a run's end is seen within a second of it.
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(ev) => handle(journal, &mut live, ev),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
        while let Ok(ev) = rx.try_recv() {
            handle(journal, &mut live, ev);
        }
        all_well &= reap(journal, &mut runs, &mut live);
        if opts.once && check_no >= 1 && live.is_empty() {
            return all_well;
        }
    }
}

fn handle(journal: &Journal, live: &mut [Running], ev: Event) {
    match ev {
        Event::Record(line) => journal.line(&line),
        Event::Closed { pid } => {
            // A run already counted as over (its grace ran out) is no longer
            // in the table; its streams closing now changes nothing.
            if let Some(r) = live.iter_mut().find(|r| r.child.id() == pid) {
                r.open_streams = r.open_streams.saturating_sub(1);
            }
        }
    }
}

/// Journal the ending of each run that is over: its process has exited, and
/// its output has closed or has had [`STREAM_GRACE_SECS`] to. Returns
/// whether every run that ended here ended well.
fn reap(journal: &Journal, runs: &mut Runs, live: &mut Vec<Running>) -> bool {
    let grace = Duration::from_secs(STREAM_GRACE_SECS);
    let mut all_well = true;
    let mut i = 0usize;
    while let Some(r) = live.get_mut(i) {
        if r.exited.is_none() {
            match r.child.try_wait() {
                Ok(Some(status)) => r.exited = Some((Some(status), Instant::now())),
                Ok(None) => {}
                // The process cannot be waited for -- something else reaped
                // it. Asking again would fail again, every second, and hold
                // the user's backups back for good; it is over, and how is
                // unknown.
                Err(e) => {
                    journal.write(
                        Level::Err,
                        Some(&r.account),
                        format!("cannot wait for backup run-due: {e}").as_bytes(),
                        Some(r.child.id()),
                    );
                    r.exited = Some((None, Instant::now()));
                }
            }
        }
        let over = match r.exited {
            Some((_, at)) => r.open_streams == 0 || at.elapsed() >= grace,
            None => false,
        };
        if !over {
            i = i.saturating_add(1);
            continue;
        }
        let r = live.swap_remove(i);
        let pid = r.child.id();
        if let Some((status, _)) = r.exited {
            let (level, words) = match status {
                Some(s) => ending(s.code(), signal_of(s)),
                None => ending(None, None),
            };
            all_well &= level != Level::Err;
            journal.write(level, Some(&r.account), words.as_bytes(), Some(pid));
        }
        if r.open_streams > 0 {
            journal.write(
                Level::Warning,
                Some(&r.account),
                STILL_OPEN.as_bytes(),
                Some(pid),
            );
        }
        runs.ended(r.account.uid);
    }
    all_well
}

#[cfg(unix)]
fn signal_of(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_status: ExitStatus) -> Option<i32> {
    None
}

/// One check: whose schedules there are, and a run for each of them that is
/// not already going. Returns whether it went without anything going wrong.
fn check(
    opts: &Options,
    journal: &Journal,
    tx: &mpsc::Sender<Event>,
    runs: &mut Runs,
    live: &mut Vec<Running>,
    last_with: &mut Option<Vec<u32>>,
    check_no: u64,
) -> bool {
    let all = match users::all() {
        Ok(a) => a,
        Err(e) => {
            journal.write(
                Level::Err,
                None,
                format!("cannot read the user database: {e}").as_bytes(),
                None,
            );
            return false;
        }
    };
    let cands = candidates(&all, users::our_uid());
    let with = with_schedules(&cands, Path::exists);
    if changed(last_with.as_deref(), &with) {
        journal.write(Level::Info, None, summary(&with).as_bytes(), None);
        *last_with = Some(with.iter().map(|a| a.uid).collect());
    }
    let mut all_well = true;
    for account in with {
        match runs.decide(account.uid) {
            Decision::StillRunning => {
                let since = runs.since(account.uid).unwrap_or(0);
                journal.write(
                    Level::Info,
                    Some(account),
                    format!("skipped: the run from check {since} is still going").as_bytes(),
                    None,
                );
            }
            Decision::Start => match users::spawn_as(&opts.backup, account) {
                Ok(mut child) => {
                    let pid = child.id();
                    journal.write(
                        Level::Info,
                        Some(account),
                        b"backup run-due started",
                        Some(pid),
                    );
                    let mut open_streams = 0u8;
                    if let Some(out) = child.stdout.take() {
                        open_streams = open_streams.saturating_add(1);
                        pump(tx.clone(), account.clone(), pid, Level::Info, out);
                    }
                    if let Some(err) = child.stderr.take() {
                        open_streams = open_streams.saturating_add(1);
                        pump(tx.clone(), account.clone(), pid, Level::Warning, err);
                    }
                    runs.started(account.uid, check_no);
                    live.push(Running {
                        child,
                        account: account.clone(),
                        open_streams,
                        exited: None,
                    });
                }
                Err(e) => {
                    all_well = false;
                    journal.write(
                        Level::Err,
                        Some(account),
                        format!("could not start backup run-due: {e}").as_bytes(),
                        None,
                    );
                }
            },
        }
    }
    all_well
}

/// A thread reading one of a run's streams to its end -- bytes, never
/// decoded -- a line at a time, and at most [`MAX_LINE`] bytes of one per
/// record, so neither a full pipe nor an endless line can stall anything.
fn pump<R: Read + Send + 'static>(
    tx: mpsc::Sender<Event>,
    who: Account,
    pid: u32,
    level: Level,
    stream: R,
) {
    // Rendered now, while `who` is still here to render it from; sent only if
    // the thread cannot be started.
    let failed = record(
        now_secs(),
        Level::Err,
        Some(&who),
        b"could not start a thread to read the run's output; it is not journalled",
        Some(pid),
    );
    let spawned = std::thread::Builder::new()
        .name(format!("backupd-out-{pid}"))
        .spawn({
            let tx = tx.clone();
            move || {
                let mut reader = BufReader::new(stream);
                let mut buf = Vec::with_capacity(256);
                let cap = u64::try_from(MAX_LINE).unwrap_or(u64::MAX);
                loop {
                    buf.clear();
                    match (&mut reader).take(cap).read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if buf.last() == Some(&b'\n') {
                                buf.pop();
                            }
                            let line = record(now_secs(), level, Some(&who), &buf, Some(pid));
                            if tx.send(Event::Record(line)).is_err() {
                                break;
                            }
                        }
                    }
                }
                // The main thread may have stopped listening (it only does when
                // `--once` is done); there is no one else to tell.
                let _ = tx.send(Event::Closed { pid });
            }
        });
    if spawned.is_err() {
        // No thread, so nothing will read this stream: the run may block on
        // it, and its output is lost. Say so, and count the stream closed so
        // the run is not held open by it.
        // Sent to our own receiver, which is alive: this runs on the main
        // thread, which holds it.
        let _ = tx.send(Event::Record(failed));
        let _ = tx.send(Event::Closed { pid });
    }
}

/// The user database and the switch into an account, by the C library.
#[cfg(unix)]
mod users {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};

    use backupd::Account;

    /// `struct passwd`: the field order POSIX's `<pwd.h>` implementations
    /// share (ours, musl's and glibc's alike -- `posix/src/pwd.rs`).
    #[repr(C)]
    struct Passwd {
        pw_name: *const u8,
        pw_passwd: *const u8,
        pw_uid: u32,
        pw_gid: u32,
        pw_gecos: *const u8,
        pw_dir: *const u8,
        pw_shell: *const u8,
    }

    unsafe extern "C" {
        fn setpwent();
        fn getpwent() -> *const Passwd;
        fn endpwent();
        fn getgrouplist(user: *const u8, group: u32, groups: *mut u32, ngroups: *mut i32) -> i32;
        fn setgroups(size: usize, list: *const u32) -> i32;
        fn setgid(gid: u32) -> i32;
        fn setuid(uid: u32) -> i32;
        fn getuid() -> u32;
        fn __errno_location() -> *mut i32;
    }

    /// `ENOENT`: no user database at all, which lists no one rather than
    /// failing.
    const ENOENT: i32 = 2;

    pub fn our_uid() -> u32 {
        // SAFETY: no arguments, no memory.
        unsafe { getuid() }
    }

    fn bytes(s: *const u8) -> Vec<u8> {
        if s.is_null() {
            return Vec::new();
        }
        // SAFETY: a non-null field of the entry `getpwent` just returned,
        // which the C library NUL-terminates; read before the next call.
        unsafe { CStr::from_ptr(s.cast()) }.to_bytes().to_vec()
    }

    /// Every account the C library's user database lists, copied out.
    ///
    /// `getpwent` answers NULL both at the end and on an error; `errno`,
    /// cleared first, tells them apart.
    pub fn all() -> std::io::Result<Vec<Account>> {
        let mut out = Vec::new();
        // SAFETY: the enumeration's calls, all on this thread (the only one
        // that enumerates), with each entry copied out before the next
        // `getpwent` reuses its storage.
        let err = unsafe {
            setpwent();
            let err = loop {
                *__errno_location() = 0;
                let p = getpwent();
                if p.is_null() {
                    break *__errno_location();
                }
                let e = &*p;
                out.push(Account {
                    name: bytes(e.pw_name),
                    uid: e.pw_uid,
                    gid: e.pw_gid,
                    home: PathBuf::from(OsStr::from_bytes(&bytes(e.pw_dir))),
                });
            };
            endpwent();
            err
        };
        if err != 0 && err != ENOENT {
            return Err(std::io::Error::from_raw_os_error(err));
        }
        Ok(out)
    }

    /// The account's groups, as `initgroups` would set them: its login
    /// group, then each group listing it.
    fn groups_of(a: &Account) -> std::io::Result<Vec<u32>> {
        let mut name = a.name.clone();
        name.push(0);
        let mut n: i32 = 0;
        // SAFETY: the count query: a NULL vector with room for none; the
        // call writes the count it needs into `n`.
        unsafe { getgrouplist(name.as_ptr(), a.gid, std::ptr::null_mut(), &raw mut n) };
        let mut groups = vec![0u32; usize::try_from(n).unwrap_or(0).max(1)];
        let mut got = i32::try_from(groups.len()).unwrap_or(i32::MAX);
        // SAFETY: `groups` has room for `got` entries, and `name` is
        // NUL-terminated.
        if unsafe { getgrouplist(name.as_ptr(), a.gid, groups.as_mut_ptr(), &raw mut got) } < 0 {
            return Err(std::io::Error::other(
                "the group database changed while it was being read",
            ));
        }
        groups.truncate(usize::try_from(got).unwrap_or(0));
        Ok(groups)
    }

    /// `backup run-due`, started as `a`: a clean environment, the account's
    /// home as its directory, its output piped back.
    ///
    /// When this service runs as root, the child takes the account's
    /// supplementary groups, gid and uid, in that order, between `fork` and
    /// `exec` -- the groups first, since setting them needs the privilege
    /// that giving up the uid gives up. The list is made here, in the
    /// parent: the child may not allocate, because another thread of this
    /// process could hold the allocator's lock at the moment of the `fork`.
    pub fn spawn_as(backup: &Path, a: &Account) -> std::io::Result<Child> {
        let mut cmd = Command::new(backup);
        cmd.arg("run-due")
            .env_clear()
            .env("HOME", &a.home)
            .env("USER", OsStr::from_bytes(&a.name))
            .env("LOGNAME", OsStr::from_bytes(&a.name))
            .env("PATH", "/bin:/usr/bin")
            .current_dir(&a.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if our_uid() == 0 && a.uid != 0 {
            let groups = groups_of(a)?;
            let (uid, gid) = (a.uid, a.gid);
            // SAFETY: the closure runs in the child between `fork` and
            // `exec` and makes three calls that are each one system call
            // over memory it owns (the list was allocated in the parent),
            // allocating nothing and taking no lock.
            unsafe {
                cmd.pre_exec(move || {
                    if setgroups(groups.len(), groups.as_ptr()) != 0
                        || setgid(gid) != 0
                        || setuid(uid) != 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        cmd.spawn()
    }
}

/// The development host has no user database to read or identity to take;
/// this keeps the crate building there, so `lib.rs`'s tests run.
#[cfg(not(unix))]
mod users {
    use std::path::Path;
    use std::process::Child;

    use backupd::Account;

    pub fn our_uid() -> u32 {
        0
    }

    pub fn all() -> std::io::Result<Vec<Account>> {
        Err(std::io::Error::other(
            "there is no user database on this host",
        ))
    }

    pub fn spawn_as(_backup: &Path, _a: &Account) -> std::io::Result<Child> {
        Err(std::io::Error::other(
            "there is no identity to take on this host",
        ))
    }
}
