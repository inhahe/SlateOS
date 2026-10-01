//! The login records `login` keeps: `utmp` (who is on now), `wtmp` (the
//! history `last` reads), `btmp` (the failed attempts `lastb` reads) and
//! `lastlog` (each user's last login) -- kept as util-linux 2.39.3's `login`
//! keeps them (`login-utils/login.c`: `log_utmp`, `log_btmp`, `log_lastlog`),
//! plus the logout record util-linux leaves to `init`, which this `login` can
//! write itself because it outlives the shell.
//!
//! Until 2026-10-01 `login` kept none of them as they are kept. It appended
//! text lines -- `user:tty:host` -- to `/var/log/lastlog` and
//! `/var/log/faillog`, which are binary files of fixed-size records indexed
//! by uid (our own `lastlog`, in `userspace/last`, reads 292-byte records and
//! would have read those lines as garbage), and it wrote nothing at all to
//! `utmp` or `wtmp`, so `who`, `w`, `last` and `getlogin` never saw a session
//! (lane D's `requests/d-b-utmp-is-a-real-file-now-create-it-and-write-logins.md`).
//! The failure log is now `btmp`, as util-linux's: `faillog` is shadow's,
//! nothing here reads it, and `authlib`'s tally is what rate-limits guessing.
//!
//! The `utmp`, `wtmp` and `btmp` writes go through the C library's
//! `pututxline`/`updwtmpx` (`libcall::utmp`), which is the one implementation
//! of their locking and search rules; `lastlog` has no C call, and is written
//! here as util-linux writes it, with `lseek` and `write`.
//!
//! One part of util-linux's record is not filled: `ut_addr_v6`, the remote
//! host's address, which it resolves from `-h` with `getaddrinfo`. The name
//! is recorded; `last -i` would show the address as zero.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use libcall::utmp::{
    self, BTMP_FILE, DEAD_PROCESS, INIT_PROCESS, LOGIN_PROCESS, Timeval, USER_PROCESS, Utmp, Utmpx,
    WTMP_FILE, fill_field,
};

/// The terminal a login is on, as the records name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Terminal {
    /// Its path without `/dev/` -- `tty1`, `pts/3` -- which is `ut_line`.
    pub(crate) line: Vec<u8>,
    /// Its name from the first digit on -- `1`, `3` -- util-linux's
    /// `tty_number`, which becomes `ut_id`. `None` for a terminal with no
    /// digit in its name (`console`).
    pub(crate) number: Option<Vec<u8>>,
}

impl Terminal {
    /// Name the terminal at `path` (`ttyname`'s answer) as util-linux's
    /// `init_tty` does: `/dev/` taken off, the number found.
    pub(crate) fn from_path(path: &[u8]) -> Self {
        let line = path.strip_prefix(b"/dev/").unwrap_or(path).to_vec();
        let number = line
            .iter()
            .position(u8::is_ascii_digit)
            .and_then(|i| line.get(i..))
            .map(<[u8]>::to_vec);
        Self { line, number }
    }
}

/// Now, as `gettimeofday` gives it; the epoch if the clock is before it.
pub(crate) fn now() -> Timeval {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Timeval {
        tv_sec: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        tv_usec: i64::from(since.subsec_micros()),
    }
}

/// util-linux's `log_utmp`: this login's `USER_PROCESS` record, written to
/// `utmp` and appended to `wtmp`.
///
/// The record a `getty` left for the terminal is reused if there is one --
/// found by this process's pid, then by the terminal's line, then by its
/// number -- so its `ut_id` and session carry over; otherwise a new one is
/// made. Returned with the result of the `utmp` write, because the logout
/// record is made from it whatever that result was: `wtmp`, which the
/// history comes from, was appended to either way.
pub(crate) fn log_utmp(
    term: Option<&Terminal>,
    user: &[u8],
    host: Option<&[u8]>,
    pid: i32,
    when: Timeval,
) -> (Utmpx, Result<(), i32>) {
    let mut db = Utmp::open();

    let mut found = None;
    while let Some(ut) = db.next_record() {
        if ut.ut_pid == pid && (INIT_PROCESS..=DEAD_PROCESS).contains(&ut.ut_type) {
            found = Some(ut);
            break;
        }
    }
    if found.is_none()
        && let Some(t) = term
    {
        db.rewind();
        let mut wanted = Utmpx::ZERO;
        wanted.ut_type = LOGIN_PROCESS;
        fill_field(&mut wanted.ut_line, &t.line);
        found = db.find_line(&wanted);
    }
    if found.is_none()
        && let Some(n) = term.and_then(|t| t.number.as_deref())
    {
        db.rewind();
        let mut wanted = Utmpx::ZERO;
        wanted.ut_type = DEAD_PROCESS;
        fill_field(&mut wanted.ut_id, n);
        found = db.find_id(&wanted);
    }

    let record = session_record(found.unwrap_or(Utmpx::ZERO), term, user, host, pid, when);
    let written = db.put(&record);
    drop(db);
    utmp::append(WTMP_FILE, &record);
    (record, written)
}

/// The `USER_PROCESS` record `log_utmp` writes, built on `base` -- the
/// record a `getty` left, or an empty one.
pub(crate) fn session_record(
    base: Utmpx,
    term: Option<&Terminal>,
    user: &[u8],
    host: Option<&[u8]>,
    pid: i32,
    when: Timeval,
) -> Utmpx {
    let mut ut = base;
    if let Some(n) = term.and_then(|t| t.number.as_deref())
        && ut.ut_id[0] == 0
    {
        fill_field(&mut ut.ut_id, n);
    }
    fill_field(&mut ut.ut_user, user);
    if let Some(t) = term {
        fill_field(&mut ut.ut_line, &t.line);
    }
    ut.ut_tv = when;
    ut.ut_type = USER_PROCESS;
    ut.ut_pid = pid;
    if let Some(h) = host {
        fill_field(&mut ut.ut_host, h);
    }
    ut
}

/// The end of the session: `session` again as `DEAD_PROCESS`, its user and
/// host cleared and stamped `when` -- what glibc's `logout(3)` leaves in
/// `utmp` and `logwtmp(3)` appends to `wtmp`.
///
/// # Errors
///
/// The `errno` of the `utmp` write; `wtmp` is appended to regardless.
pub(crate) fn log_logout(session: &Utmpx, when: Timeval) -> Result<(), i32> {
    let record = logout_record(session, when);
    let written = Utmp::open().put(&record);
    utmp::append(WTMP_FILE, &record);
    written
}

/// The record [`log_logout`] writes.
pub(crate) fn logout_record(session: &Utmpx, when: Timeval) -> Utmpx {
    let mut ut = *session;
    ut.ut_type = DEAD_PROCESS;
    ut.ut_user = [0; utmp::UT_NAMESIZE];
    ut.ut_host = [0; utmp::UT_HOSTSIZE];
    ut.ut_tv = when;
    ut
}

/// util-linux's `log_btmp`: a failed attempt, appended to `btmp`.
///
/// The name is the one that was typed, account or not -- what `lastb`
/// shows is what was tried.
pub(crate) fn log_btmp(
    term: Option<&Terminal>,
    user: &[u8],
    host: Option<&[u8]>,
    pid: i32,
    when: Timeval,
) {
    utmp::append(BTMP_FILE, &btmp_record(term, user, host, pid, when));
}

/// The record [`log_btmp`] appends.
pub(crate) fn btmp_record(
    term: Option<&Terminal>,
    user: &[u8],
    host: Option<&[u8]>,
    pid: i32,
    when: Timeval,
) -> Utmpx {
    let mut ut = Utmpx::ZERO;
    fill_field(
        &mut ut.ut_user,
        if user.is_empty() { b"(unknown)" } else { user },
    );
    if let Some(t) = term {
        if let Some(n) = &t.number {
            fill_field(&mut ut.ut_id, n);
        }
        fill_field(&mut ut.ut_line, &t.line);
    }
    ut.ut_tv = when;
    // util-linux's own comment on this line: "XXX doesn't matter".
    ut.ut_type = LOGIN_PROCESS;
    ut.ut_pid = pid;
    if let Some(h) = host {
        fill_field(&mut ut.ut_host, h);
    }
    ut
}

/// Bytes in one `lastlog` record: glibc's x86-64 `struct lastlog`, a 32-bit
/// time then the line and the host.
pub(crate) const LASTLOG_RECORD: usize = 4 + utmp::UT_LINESIZE + utmp::UT_HOSTSIZE;

/// One user's `lastlog` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LastLog {
    /// When, in seconds since the epoch (0: never).
    pub(crate) time: i32,
    /// The terminal, `ut_line`'s way.
    pub(crate) line: [u8; utmp::UT_LINESIZE],
    /// The remote host, or empty.
    pub(crate) host: [u8; utmp::UT_HOSTSIZE],
}

impl LastLog {
    /// This login's record. The time is stored as glibc stores it, in 32
    /// bits, so it wraps in 2038 as it does there.
    pub(crate) fn new(time: i64, term: Option<&Terminal>, host: Option<&[u8]>) -> Self {
        let mut rec = Self {
            // The format's own 32 bits: truncation is what glibc does too.
            time: time as i32,
            line: [0; utmp::UT_LINESIZE],
            host: [0; utmp::UT_HOSTSIZE],
        };
        if let Some(t) = term {
            fill_field(&mut rec.line, &t.line);
        }
        if let Some(h) = host {
            fill_field(&mut rec.host, h);
        }
        rec
    }

    fn to_bytes(&self) -> [u8; LASTLOG_RECORD] {
        let mut out = [0u8; LASTLOG_RECORD];
        let (time, rest) = out.split_at_mut(4);
        time.copy_from_slice(&self.time.to_le_bytes());
        let (line, host) = rest.split_at_mut(utmp::UT_LINESIZE);
        line.copy_from_slice(&self.line);
        host.copy_from_slice(&self.host);
        out
    }

    fn from_bytes(b: &[u8; LASTLOG_RECORD]) -> Self {
        let [t0, t1, t2, t3, rest @ ..] = b;
        let (line, host) = rest.split_at(utmp::UT_LINESIZE);
        let mut rec = Self {
            time: i32::from_le_bytes([*t0, *t1, *t2, *t3]),
            line: [0; utmp::UT_LINESIZE],
            host: [0; utmp::UT_HOSTSIZE],
        };
        rec.line.copy_from_slice(line);
        rec.host.copy_from_slice(host);
        rec
    }
}

/// util-linux's `log_lastlog`: read `uid`'s previous record (unless `quiet`),
/// then write `record` in its place.
///
/// The file is not created: whether the system keeps a `lastlog` is decided
/// by its existence, as util-linux decides it. `Ok(None)` is no file, or no
/// previous login, or `quiet`.
///
/// # Errors
///
/// Seeking or writing the file failed, or it could not be opened for a
/// reason other than its absence.
pub(crate) fn log_lastlog(
    path: &Path,
    uid: u32,
    record: &LastLog,
    quiet: bool,
) -> io::Result<Option<LastLog>> {
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let at = u64::from(uid).saturating_mul(LASTLOG_RECORD as u64);
    file.seek(SeekFrom::Start(at))?;
    let mut previous = None;
    if !quiet {
        let mut buf = [0u8; LASTLOG_RECORD];
        // A short read is a uid past the end of the file: no record yet.
        if read_full(&mut file, &mut buf)? {
            let prev = LastLog::from_bytes(&buf);
            if prev.time != 0 {
                previous = Some(prev);
            }
        }
        file.seek(SeekFrom::Start(at))?;
    }
    file.write_all(&record.to_bytes())?;
    Ok(previous)
}

/// Fill `buf` from `r`; `Ok(false)` if the file ended first.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut got = 0;
    while let Some(rest) = buf.get_mut(got..).filter(|r| !r.is_empty()) {
        match r.read(rest)? {
            0 => return Ok(false),
            n => got = got.saturating_add(n),
        }
    }
    Ok(true)
}

/// The line util-linux prints from the previous record:
/// `Last login: Wed Oct  1 03:44:12 from host` (or `on tty1` for a local
/// login) -- `ctime`'s first 19 characters, in local time.
pub(crate) fn last_login_line(prev: &LastLog, zone: &localtime::Zone) -> Vec<u8> {
    let tm = zone.localtime(i64::from(prev.time), 0);
    let mut out = b"Last login: ".to_vec();
    out.extend(localtime::strftime(b"%a %b %e %H:%M:%S", &tm));
    let field = |f: &[u8]| -> Vec<u8> {
        let end = f.iter().position(|&b| b == 0).unwrap_or(f.len());
        f.get(..end).unwrap_or_default().to_vec()
    };
    let host = field(&prev.host);
    if host.is_empty() {
        out.extend_from_slice(b" on ");
        out.extend(field(&prev.line));
    } else {
        out.extend_from_slice(b" from ");
        out.extend(host);
    }
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests;
