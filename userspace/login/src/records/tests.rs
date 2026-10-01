//! The records `login` writes, field by field, and `lastlog` against a real
//! file in a scratch directory. The C library's `utmp` calls themselves only
//! run on SlateOS (`libcall::utmp`), so what is checked here is what `login`
//! hands them -- the same records util-linux's `login` builds.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use scratchdir::ScratchDir;

fn field(f: &[u8]) -> &[u8] {
    let end = f.iter().position(|&b| b == 0).unwrap_or(f.len());
    &f[..end]
}

const T: Timeval = Timeval {
    tv_sec: 1_759_300_000,
    tv_usec: 250,
};

#[test]
fn a_terminal_is_named_as_util_linux_names_it() {
    let t = Terminal::from_path(b"/dev/tty1");
    assert_eq!(
        (t.line.as_slice(), t.number.as_deref()),
        (&b"tty1"[..], Some(&b"1"[..]))
    );
    let t = Terminal::from_path(b"/dev/pts/13");
    assert_eq!(
        (t.line.as_slice(), t.number.as_deref()),
        (&b"pts/13"[..], Some(&b"13"[..]))
    );
    let t = Terminal::from_path(b"/dev/ttyS0");
    assert_eq!(t.number.as_deref(), Some(&b"0"[..]));
    // No digit, no number: util-linux then leaves `ut_id` as it found it.
    let t = Terminal::from_path(b"/dev/console");
    assert_eq!((t.line.as_slice(), t.number), (&b"console"[..], None));
    // A name `ttyname` gave without /dev/ is kept whole.
    assert_eq!(Terminal::from_path(b"tty3").line, b"tty3");
}

#[test]
fn a_new_session_record_has_every_field_login_knows() {
    let term = Terminal::from_path(b"/dev/tty2");
    let ut = session_record(
        Utmpx::ZERO,
        Some(&term),
        b"alice",
        Some(b"example.org"),
        4242,
        T,
    );
    assert_eq!(ut.ut_type, USER_PROCESS);
    assert_eq!(ut.ut_pid, 4242);
    assert_eq!(field(&ut.ut_line), b"tty2");
    assert_eq!(ut.ut_id, *b"2\0\0\0");
    assert_eq!(field(&ut.ut_user), b"alice");
    assert_eq!(field(&ut.ut_host), b"example.org");
    assert_eq!(ut.ut_tv, T);
}

#[test]
fn a_local_session_has_no_host() {
    // This `login` used to record "localhost" for a login with no -h, which
    // made every console login look like a network one.
    let term = Terminal::from_path(b"/dev/tty2");
    let ut = session_record(Utmpx::ZERO, Some(&term), b"alice", None, 1, T);
    assert_eq!(ut.ut_host, [0; utmp::UT_HOSTSIZE]);
}

#[test]
fn a_record_a_getty_left_keeps_its_id_and_session() {
    let mut getty = Utmpx::ZERO;
    getty.ut_type = LOGIN_PROCESS;
    fill_field(&mut getty.ut_id, b"c1");
    getty.ut_session = 77;
    let term = Terminal::from_path(b"/dev/tty1");
    let ut = session_record(getty, Some(&term), b"bob", None, 9, T);
    assert_eq!(ut.ut_id, *b"c1\0\0", "the getty's id, not the tty number");
    assert_eq!(ut.ut_session, 77);
    assert_eq!(ut.ut_type, USER_PROCESS);
}

#[test]
fn a_name_that_fills_its_field_is_cut_not_refused() {
    let long = [b'x'; 40];
    let ut = session_record(Utmpx::ZERO, None, &long, None, 1, T);
    assert_eq!(ut.ut_user, [b'x'; utmp::UT_NAMESIZE]);
}

#[test]
fn the_logout_record_is_the_session_ended() {
    let term = Terminal::from_path(b"/dev/pts/4");
    let session = session_record(Utmpx::ZERO, Some(&term), b"alice", Some(b"h"), 50, T);
    let later = Timeval {
        tv_sec: T.tv_sec + 60,
        tv_usec: 0,
    };
    let ut = logout_record(&session, later);
    assert_eq!(ut.ut_type, DEAD_PROCESS);
    assert_eq!(ut.ut_user, [0; utmp::UT_NAMESIZE]);
    assert_eq!(ut.ut_host, [0; utmp::UT_HOSTSIZE]);
    // What `getutxid` finds it by, and what `last` pairs it with, is kept.
    assert_eq!(ut.ut_id, session.ut_id);
    assert_eq!(ut.ut_line, session.ut_line);
    assert_eq!(ut.ut_pid, 50);
    assert_eq!(ut.ut_tv, later);
}

#[test]
fn a_failed_attempt_records_what_was_typed() {
    let term = Terminal::from_path(b"/dev/tty5");
    let ut = btmp_record(Some(&term), b"nosuchuser", None, 3, T);
    assert_eq!(ut.ut_type, LOGIN_PROCESS);
    assert_eq!(field(&ut.ut_user), b"nosuchuser");
    assert_eq!(field(&ut.ut_line), b"tty5");
    assert_eq!(ut.ut_id, *b"5\0\0\0");
    // Nothing typed at all is "(unknown)", as util-linux writes it.
    assert_eq!(
        field(&btmp_record(None, b"", None, 3, T).ut_user),
        b"(unknown)"
    );
}

#[cfg(not(target_vendor = "slateos"))]
#[test]
fn on_a_host_the_session_is_built_but_nothing_is_written() {
    let term = Terminal::from_path(b"/dev/tty1");
    let (ut, written) = log_utmp(Some(&term), b"alice", None, 7, T);
    assert_eq!(ut.ut_type, USER_PROCESS);
    assert_eq!(written, Err(libcall::ENOSYS));
    assert_eq!(log_logout(&ut, T), Err(libcall::ENOSYS));
    log_btmp(Some(&term), b"x", None, 7, T);
}

#[test]
fn lastlog_is_written_at_the_uid_and_reports_the_previous_login() {
    let scratch = ScratchDir::new("login-lastlog");
    let path = scratch.path("lastlog");
    std::fs::write(&path, b"").unwrap();

    let term = Terminal::from_path(b"/dev/tty1");
    let first = LastLog::new(31_536_000, Some(&term), None);
    assert_eq!(log_lastlog(&path, 1000, &first, false).unwrap(), None);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        bytes.len(),
        1001 * LASTLOG_RECORD,
        "record 1000 of a sparse file"
    );
    let rec = &bytes[1000 * LASTLOG_RECORD..];
    assert_eq!(&rec[..4], &31_536_000i32.to_le_bytes());
    assert_eq!(field(&rec[4..36]), b"tty1");

    let second = LastLog::new(31_536_100, Some(&term), Some(b"example.org"));
    assert_eq!(
        log_lastlog(&path, 1000, &second, false).unwrap(),
        Some(first)
    );
    // Quiet (a hushlogin) reads nothing back, and still records.
    let third = LastLog::new(31_536_200, None, None);
    assert_eq!(log_lastlog(&path, 1000, &third, true).unwrap(), None);
    assert_eq!(
        log_lastlog(&path, 1000, &second, false).unwrap(),
        Some(third)
    );
    // Another uid is another record.
    assert_eq!(log_lastlog(&path, 0, &second, false).unwrap(), None);
}

#[test]
fn lastlog_is_not_created_when_the_system_keeps_none() {
    let scratch = ScratchDir::new("login-lastlog-absent");
    let path = scratch.path("lastlog");
    let rec = LastLog::new(1, None, None);
    assert_eq!(log_lastlog(&path, 1000, &rec, false).unwrap(), None);
    assert!(!path.exists());
}

#[test]
fn the_last_login_line_is_util_linuxs() {
    let utc = localtime::Zone::utc();
    let term = Terminal::from_path(b"/dev/tty1");
    // 1971-01-01 00:00:00 UTC, a Friday.
    let local = LastLog::new(31_536_000, Some(&term), None);
    assert_eq!(
        last_login_line(&local, &utc),
        b"Last login: Fri Jan  1 00:00:00 on tty1\n"
    );
    let remote = LastLog::new(31_536_000, Some(&term), Some(b"example.org"));
    assert_eq!(
        last_login_line(&remote, &utc),
        b"Last login: Fri Jan  1 00:00:00 from example.org\n"
    );
}
