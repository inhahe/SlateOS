//! The login records as gnulib's `readutmp` hands them to `who`, `users` and
//! `pinky`: which entries a caller is given, the boot entry gnulib makes up
//! when a Linux utmp has none, and the readings of an entry's fields the
//! three share.
//!
//! The bytes are the `utmpfile` crate's to parse. This is what gnulib does
//! between that parse and the utility.
//!
//! # Which entries a caller gets
//!
//! gnulib's `desirable_utmp_entry`. With [`Want::users_only`], only user
//! processes -- `IS_USER_PROCESS`, a `USER_PROCESS` entry *with a name*; a
//! nameless one is nobody. With [`Want::live_only`], no user process whose
//! process has gone: `kill (pid, 0)` failing with `ESRCH` is the proof, and
//! nothing else is -- `EPERM` means it exists and is someone else's. Upstream
//! asks for that only of the live utmp, where a crashed login leaves its entry
//! behind; a file named on the command line is history and is read whole.
//!
//! # A boot entry, made up when there is none
//!
//! `who -b` prints the `BOOT_TIME` entry, and some systems write none -- Alpine
//! leaves utmp empty, runit and s6 write user entries and no boot -- so on
//! Linux gnulib adds one when the file being read is `/var/run/utmp` itself
//! (compared by name) and the caller has not asked for user processes only.
//! Its time is the modification time of the first of four files only boot
//! touches, `/var/run/utmp` last; or, when none exists, the clock less the
//! kernel's uptime. And a boot entry dated within the first minute of 1970 --
//! a machine with no battery clock, which booted before the network set it --
//! takes the time of the last `runlevel` entry, written once the clock was
//! right. SlateOS writes no utmp yet, so its `who -b` comes from the uptime.
//!
//! # A file that cannot be read
//!
//! As `users` documents and does: GNU on glibc reads through `getutxent`,
//! which cannot report a failure, so a FILE that is missing or unreadable is
//! "nobody logged in", status 0. gnulib's own reader -- the one GNU uses where
//! it reads the file itself, as this does -- reports it. A FILE named on the
//! command line that cannot be read is an error here, and the live utmp counts
//! as empty only when it does not exist. Against GNU on glibc those cases
//! differ, on purpose; each harness records them.

use std::io;

use crate::quote::os_from_bytes;
pub use utmpfile::{
    BOOT_TIME, DEAD_PROCESS, INIT_PROCESS, LOGIN_PROCESS, NEW_TIME, RUN_LVL, Record, USER_PROCESS,
};

/// glibc's `_PATH_UTMP`, which gnulib's `UTMP_FILE` is: who is logged in now.
pub const UTMP_FILE: &str = "/var/run/utmp";

/// glibc's `_PATH_WTMP`: every login since the file was made.
pub const WTMP_FILE: &str = "/var/log/wtmp";

/// The files only boot touches, in gnulib's order -- `get_linux_boot_time_fallback`.
/// `/var/run/utmp` must come last: on several systems it is written at every
/// login too, long after boot.
const BOOT_TOUCHED_FILES: [&str; 4] = [
    "/var/lib/systemd/random-seed",
    "/var/lib/urandom/random-seed",
    "/var/lib/random-seed",
    "/var/run/utmp",
];

/// What a caller asks `read_utmp` for: gnulib's `READ_UTMP_*` bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Want {
    /// `READ_UTMP_USER_PROCESS`: user processes only.
    pub users_only: bool,
    /// `READ_UTMP_CHECK_PIDS`: no user process whose process has gone.
    pub live_only: bool,
}

/// gnulib's `IS_USER_PROCESS`: a `USER_PROCESS` entry with a name in it.
#[must_use]
pub fn is_user_process(record: &Record) -> bool {
    record.record_type == USER_PROCESS && !record.user.is_empty()
}

/// gnulib's `extract_trimmed_name`: the name less any trailing spaces.
#[must_use]
pub fn trimmed_name(record: &Record) -> &[u8] {
    let keep = record
        .user
        .iter()
        .rposition(|&b| b != b' ')
        .map_or(0, |last| last.saturating_add(1));
    record.user.get(..keep).unwrap_or_default()
}

/// The device a `ut_line` names, relative to `/dev` unless absolute: the part
/// after its first space, if it has one ("If ut_line contains a space, the
/// device name starts after the space"). `None` for an empty name, which
/// upstream's `fstatat` refuses -- joined to `/dev/` it would stat `/dev`.
#[must_use]
pub fn tty_device(line: &[u8]) -> Option<Vec<u8>> {
    let name = match line.iter().position(|&c| c == b' ') {
        Some(at) => line.get(at.saturating_add(1)..).unwrap_or_default(),
        None => line,
    };
    if name.is_empty() {
        return None;
    }
    if name.first() == Some(&b'/') {
        return Some(name.to_vec());
    }
    let mut path = b"/dev/".to_vec();
    path.extend_from_slice(name);
    Some(path)
}

/// A `ut_host` split at its first `:` into the host and the X display after
/// it, as `who` and `pinky` both print them.
#[must_use]
pub fn split_display(host: &[u8]) -> (&[u8], Option<&[u8]>) {
    match host.iter().position(|&c| c == b':') {
        Some(at) => (
            host.get(..at).unwrap_or_default(),
            Some(host.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (host, None),
    }
}

/// gnulib's `canon_host`: the resolver's canonical name for `host`, for
/// `who --lookup` and `pinky`. `None` when the resolver does not know the name
/// -- the caller then prints `host` as it is -- and `host` itself when it does
/// and names no canonical form, as gnulib returns it.
///
/// Through the C library's `getaddrinfo`, because that is the resolver:
/// `/etc/hosts`, DNS and whatever `nsswitch.conf` adds.
#[must_use]
pub fn canon_host(host: &[u8]) -> Option<Vec<u8>> {
    // A name with a NUL in it cannot be passed as a C string; `ut_host` is cut
    // at its first NUL, so this cannot happen, and a refusal is the honest
    // answer if it ever did.
    let name = std::ffi::CString::new(host).ok()?;
    let found = libcall::netdb::AddrInfo::lookup(&name, 0, libcall::netdb::AI_CANONNAME).ok()?;
    Some(found.canonical_name().unwrap_or(host).to_vec())
}

/// The answers the reading needs from the running system, so that the rules
/// above can be tested against a system that is not this one.
pub trait Machine {
    /// Whether process `pid` exists: `kill (pid, 0)` not failing with `ESRCH`.
    fn alive(&self, pid: i32) -> bool;
    /// The modification time of the first of the files only boot touches
    /// that exists, as seconds and nanoseconds.
    fn boot_touched_time(&self) -> Option<(i64, i64)>;
    /// The clock less the kernel's uptime, as seconds and nanoseconds.
    fn boot_from_uptime(&self) -> Option<(i64, i64)>;
}

/// The system this runs on.
pub struct ThisMachine;

impl Machine for ThisMachine {
    fn alive(&self, pid: i32) -> bool {
        // `kill` refuses a pid that is not one process; only a positive one
        // is ever asked about here, as upstream asks.
        !matches!(libcall::kill(pid, 0), Err(libcall::ESRCH))
    }

    fn boot_touched_time(&self) -> Option<(i64, i64)> {
        BOOT_TOUCHED_FILES.iter().find_map(|name| {
            let modified = std::fs::metadata(name).ok()?.modified().ok()?;
            Some(since_epoch(modified))
        })
    }

    fn boot_from_uptime(&self) -> Option<(i64, i64)> {
        let (up_sec, up_nsec) = libcall::clock::since_boot().ok()?;
        let (mut sec, mut nsec) = since_epoch(std::time::SystemTime::now());
        // gnulib's borrow, then the subtraction.
        if nsec < up_nsec {
            nsec = nsec.saturating_add(1_000_000_000);
            sec = sec.saturating_sub(1);
        }
        Some((sec.saturating_sub(up_sec), nsec.saturating_sub(up_nsec)))
    }
}

/// A time as seconds and nanoseconds since the epoch, negative before it.
fn since_epoch(t: std::time::SystemTime) -> (i64, i64) {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(after) => (
            i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            i64::from(after.subsec_nanos()),
        ),
        Err(before) => {
            let before = before.duration();
            let secs = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            match before.subsec_nanos() {
                0 => (secs.saturating_neg(), 0),
                n => (
                    secs.saturating_neg().saturating_sub(1),
                    1_000_000_000_i64.saturating_sub(i64::from(n)),
                ),
            }
        }
    }
}

/// The bytes of a utmp file: a `named` one must be readable; the live one,
/// when not named, is no sessions at all if it does not exist.
///
/// # Errors
///
/// Whatever reading the file failed with, as described above.
pub fn read_bytes(file: &[u8], named: bool) -> io::Result<Vec<u8>> {
    let path = os_from_bytes(file);
    if named {
        std::fs::read(path)
    } else {
        optionalfile::read_bytes_or_empty(std::path::Path::new(&path))
    }
}

/// gnulib's `read_utmp`: the entries of `file` the caller wants, with the boot
/// entry made up as described above.
///
/// # Errors
///
/// As [`read_bytes`].
pub fn read_utmp(file: &[u8], named: bool, want: Want) -> io::Result<Vec<Record>> {
    let data = read_bytes(file, named)?;
    Ok(entries(
        &utmpfile::parse(&data),
        file == UTMP_FILE.as_bytes(),
        want,
        &ThisMachine,
    ))
}

/// What [`read_utmp`] does with the parsed records -- the part with no I/O of
/// its own, given the system's answers through `machine`.
#[must_use]
pub fn entries(
    records: &[Record],
    file_is_utmp: bool,
    want: Want,
    machine: &impl Machine,
) -> Vec<Record> {
    // The last `runlevel` entry's time, over every entry read, wanted or not.
    let runlevel = if file_is_utmp {
        records
            .iter()
            .rev()
            .find(|r| r.user == b"runlevel" && r.tty == b"~")
            .map(|r| (r.tv_sec, r.login_usec))
    } else {
        None
    };
    let mut out: Vec<Record> = records
        .iter()
        .filter(|r| desirable(r, want, machine))
        .cloned()
        .collect();
    if want.users_only || !file_is_utmp {
        return out;
    }
    // The battery-less clock: a boot within 1970's first minute takes the
    // runlevel entry's time, if there is a runlevel entry with a time.
    if let Some(boot) = out.iter_mut().find(|r| r.record_type == BOOT_TIME)
        && boot.tv_sec <= 60
        && let Some((sec, usec)) = runlevel.filter(|(sec, _)| *sec != 0)
    {
        boot.tv_sec = sec;
        boot.login_time = u64::try_from(sec).unwrap_or(0);
        boot.login_usec = usec;
    }
    // A boot-touched file's time first; the uptime only if there is none.
    if !out.iter().any(|r| r.record_type == BOOT_TIME)
        && let Some(time) = machine
            .boot_touched_time()
            .or_else(|| machine.boot_from_uptime())
    {
        out.push(made_up_boot(time));
    }
    out
}

/// gnulib's `desirable_utmp_entry`, for the two options asked of it here.
fn desirable(record: &Record, want: Want, machine: &impl Machine) -> bool {
    let user_process = is_user_process(record);
    if want.users_only && !user_process {
        return false;
    }
    !(want.live_only && user_process && record.pid > 0 && !machine.alive(record.pid))
}

/// The entry gnulib adds: `reboot` on `~`, a `BOOT_TIME` at `time`.
fn made_up_boot((sec, nsec): (i64, i64)) -> Record {
    Record {
        record_type: BOOT_TIME,
        user: b"reboot".to_vec(),
        tty: b"~".to_vec(),
        host: Vec::new(),
        id: Vec::new(),
        pid: 0,
        login_time: u64::try_from(sec).unwrap_or(0),
        tv_sec: sec,
        login_usec: u32::try_from(nsec / 1000).unwrap_or(0),
        exit_status: 0,
        session: 0,
        addr_v6: [0; 4],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A system whose answers the test chooses.
    struct Fake {
        dead: Vec<i32>,
        touched: Option<(i64, i64)>,
        uptime: Option<(i64, i64)>,
    }

    impl Machine for Fake {
        fn alive(&self, pid: i32) -> bool {
            !self.dead.contains(&pid)
        }
        fn boot_touched_time(&self) -> Option<(i64, i64)> {
            self.touched
        }
        fn boot_from_uptime(&self) -> Option<(i64, i64)> {
            self.uptime
        }
    }

    const NOTHING: Fake = Fake {
        dead: Vec::new(),
        touched: None,
        uptime: None,
    };

    fn rec(record_type: i32, user: &[u8], tty: &[u8], pid: i32, sec: i64) -> Record {
        Record {
            record_type,
            user: user.to_vec(),
            tty: tty.to_vec(),
            host: Vec::new(),
            id: Vec::new(),
            pid,
            login_time: u64::try_from(sec).unwrap_or(0),
            tv_sec: sec,
            login_usec: 0,
            exit_status: 0,
            session: 0,
            addr_v6: [0; 4],
        }
    }

    fn users(records: &[Record]) -> Vec<&[u8]> {
        records.iter().map(|r| r.user.as_slice()).collect()
    }

    #[test]
    fn a_user_process_has_a_name() {
        assert!(is_user_process(&rec(
            USER_PROCESS,
            b"alice",
            b"pts/0",
            1,
            0
        )));
        assert!(!is_user_process(&rec(USER_PROCESS, b"", b"pts/0", 1, 0)));
        assert!(!is_user_process(&rec(
            LOGIN_PROCESS,
            b"LOGIN",
            b"tty1",
            1,
            0
        )));
    }

    #[test]
    fn only_user_processes_when_asked() {
        let records = [
            rec(BOOT_TIME, b"reboot", b"~", 0, 5),
            rec(USER_PROCESS, b"alice", b"pts/0", 10, 100),
            rec(USER_PROCESS, b"", b"pts/1", 11, 100),
            rec(DEAD_PROCESS, b"", b"pts/2", 12, 100),
        ];
        let want = Want {
            users_only: true,
            live_only: false,
        };
        assert_eq!(
            users(&entries(&records, true, want, &NOTHING)),
            [&b"alice"[..]]
        );
        // Asked for everything, everything comes, nameless sessions included.
        assert_eq!(entries(&records, false, Want::default(), &NOTHING).len(), 4);
    }

    /// `kill (pid, 0)` failing with `ESRCH` drops a session -- and only a
    /// session, and only one with a positive pid.
    #[test]
    fn a_session_whose_process_is_gone_is_dropped_when_asked() {
        let fake = Fake {
            dead: vec![10, 0, -3, 30],
            ..NOTHING
        };
        let records = [
            rec(USER_PROCESS, b"gone", b"pts/0", 10, 100),
            rec(USER_PROCESS, b"here", b"pts/1", 20, 100),
            rec(USER_PROCESS, b"zero", b"pts/2", 0, 100),
            rec(USER_PROCESS, b"negative", b"pts/3", -3, 100),
            rec(LOGIN_PROCESS, b"LOGIN", b"tty1", 30, 100),
        ];
        let live = Want {
            users_only: false,
            live_only: true,
        };
        assert_eq!(
            users(&entries(&records, false, live, &fake)),
            [&b"here"[..], b"zero", b"negative", b"LOGIN"]
        );
        assert_eq!(entries(&records, false, Want::default(), &fake).len(), 5);
    }

    /// A boot within 1970's first minute takes the last runlevel entry's time.
    #[test]
    fn a_boot_before_the_clock_was_set_takes_the_runlevel_time() {
        let records = [
            rec(BOOT_TIME, b"reboot", b"~", 0, 5),
            rec(RUN_LVL, b"runlevel", b"~", 0, 1_000),
            rec(RUN_LVL, b"runlevel", b"~", 0, 2_000),
        ];
        let got = entries(&records, true, Want::default(), &NOTHING);
        assert_eq!(got[0].tv_sec, 2_000);
        assert_eq!(got[0].login_time, 2_000);
        // Not for a boot after the first minute, nor for a file that is not
        // the live utmp, nor without a runlevel entry to take a time from.
        let late = [rec(BOOT_TIME, b"reboot", b"~", 0, 61), records[1].clone()];
        assert_eq!(
            entries(&late, true, Want::default(), &NOTHING)[0].tv_sec,
            61
        );
        assert_eq!(
            entries(&records, false, Want::default(), &NOTHING)[0].tv_sec,
            5
        );
        let alone = [rec(BOOT_TIME, b"reboot", b"~", 0, 5)];
        assert_eq!(
            entries(&alone, true, Want::default(), &NOTHING)[0].tv_sec,
            5
        );
    }

    /// No boot entry in the live utmp: one is made up, from a boot-touched
    /// file's time if there is one, else from the uptime.
    #[test]
    fn a_missing_boot_entry_is_made_up_for_the_live_utmp() {
        let records = [rec(USER_PROCESS, b"alice", b"pts/0", 1, 100)];
        let touched = Fake {
            touched: Some((1_000, 5)),
            uptime: Some((2_000, 0)),
            ..NOTHING
        };
        let got = entries(&records, true, Want::default(), &touched);
        assert_eq!(got.len(), 2);
        let boot = &got[1];
        assert_eq!(boot.record_type, BOOT_TIME);
        assert_eq!(
            (boot.user.as_slice(), boot.tty.as_slice()),
            (&b"reboot"[..], &b"~"[..])
        );
        assert_eq!(boot.tv_sec, 1_000);
        let uptime = Fake {
            uptime: Some((2_000, 0)),
            ..NOTHING
        };
        assert_eq!(
            entries(&records, true, Want::default(), &uptime)[1].tv_sec,
            2_000
        );
        // Neither answer: nothing is made up.
        assert_eq!(entries(&records, true, Want::default(), &NOTHING).len(), 1);
        // Not for another file, and not when only user processes are wanted.
        assert_eq!(entries(&records, false, Want::default(), &touched).len(), 1);
        let users_only = Want {
            users_only: true,
            live_only: false,
        };
        assert_eq!(entries(&records, true, users_only, &touched).len(), 1);
    }

    #[test]
    fn the_shared_field_readings() {
        let spaced = rec(USER_PROCESS, b"bob  ", b"pts/0", 1, 0);
        assert_eq!(trimmed_name(&spaced), b"bob");
        assert_eq!(trimmed_name(&rec(USER_PROCESS, b"   ", b"", 1, 0)), b"");
        assert_eq!(tty_device(b"pts/3"), Some(b"/dev/pts/3".to_vec()));
        assert_eq!(tty_device(b"/abs/tty"), Some(b"/abs/tty".to_vec()));
        assert_eq!(tty_device(b"tag pts/4"), Some(b"/dev/pts/4".to_vec()));
        assert_eq!(tty_device(b""), None);
        assert_eq!(tty_device(b"tag "), None);
        assert_eq!(split_display(b"host:0"), (&b"host"[..], Some(&b"0"[..])));
        assert_eq!(split_display(b":1"), (&b""[..], Some(&b"1"[..])));
        assert_eq!(split_display(b"host"), (&b"host"[..], None));
    }

    #[test]
    fn a_time_before_the_epoch_keeps_its_sign() {
        let before = std::time::UNIX_EPOCH - std::time::Duration::new(1, 250_000_000);
        assert_eq!(since_epoch(before), (-2, 750_000_000));
        let exact = std::time::UNIX_EPOCH - std::time::Duration::from_secs(3);
        assert_eq!(since_epoch(exact), (-3, 0));
    }
}
