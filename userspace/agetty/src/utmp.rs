//! `update_utmp`: the `LOGIN_PROCESS` record a getty writes for its line --
//! "LOGIN" waiting there, this process, this session, now -- in `utmp`, over
//! the record `init` wrote for it if there is one, and in `wtmp`. `who -a`
//! and `w` show it, and `login` takes it over.

use libcall::utmp::{self as ut, Utmpx};

use crate::options::Options;

/// `str2memcpy (dest, src, n)`: `src` and its NUL, as far as `dest` holds
/// them -- no NUL when it is full, and what follows in the field left as it
/// was.
fn str2memcpy(dest: &mut [u8], src: &[u8]) {
    let src = src.split(|&b| b == 0).next().unwrap_or_default();
    let n = src.len().saturating_add(1).min(dest.len());
    for (i, d) in dest.iter_mut().take(n).enumerate() {
        *d = src.get(i).copied().unwrap_or(0);
    }
}

/// Whether `record` is this process's own -- `init`'s for it, or one it
/// left: what `update_utmp` looks for.
#[must_use]
pub fn is_ours(record: &Utmpx, pid: i32) -> bool {
    record.ut_pid == pid && (ut::INIT_PROCESS..=ut::DEAD_PROCESS).contains(&record.ut_type)
}

/// The record to write for the line `op` names: `found`, the process's own
/// record, or a new one whose id is the virtual console's number, or the
/// line's last four bytes -- now `LOGIN` waiting on the line, from `host`
/// under `-H`, for process `pid` of session `sid`, at `now`.
#[must_use]
pub fn login_record(found: Option<Utmpx>, op: &Options, pid: i32, sid: i32, now: i64) -> Utmpx {
    let mut rec = found.unwrap_or_else(|| {
        let mut rec = Utmpx::ZERO;
        match op.vcline.as_deref() {
            Some(vc) if !vc.is_empty() => str2memcpy(&mut rec.ut_id, vc),
            _ => {
                let tty = &op.tty;
                let start = tty.len().saturating_sub(ut::UT_IDSIZE);
                str2memcpy(&mut rec.ut_id, tty.get(start..).unwrap_or_default());
            }
        }
        rec
    });
    str2memcpy(&mut rec.ut_user, b"LOGIN");
    str2memcpy(&mut rec.ut_line, &op.tty);
    if let Some(host) = &op.fakehost {
        str2memcpy(&mut rec.ut_host, host);
    }
    rec.ut_tv.tv_sec = now;
    rec.ut_type = ut::LOGIN_PROCESS;
    rec.ut_pid = pid;
    rec.ut_session = sid;
    rec
}

/// `update_utmp (op)`: the record, in `utmp` and `wtmp`. Failures go
/// unreported, as upstream reports none.
pub fn update_utmp(op: &Options) {
    let pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    let sid = libcall::process::session_of(0).unwrap_or(-1);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    let mut file = ut::Utmp::open();
    let found = std::iter::from_fn(|| file.next_record()).find(|r| is_ours(r, pid));
    let rec = login_record(found, op, pid, sid, now);
    let _ = file.put(&rec);
    drop(file);
    ut::append(ut::WTMP_FILE, &rec);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(f: &[u8]) -> &[u8] {
        f.split(|&b| b == 0).next().unwrap_or_default()
    }

    #[test]
    fn str2memcpy_keeps_what_follows_and_drops_the_nul_when_full() {
        let mut f = *b"abcdefgh";
        str2memcpy(&mut f, b"xy");
        assert_eq!(&f, b"xy\0defgh");
        let mut f = [0u8; 4];
        str2memcpy(&mut f, b"ttyS0");
        assert_eq!(&f, b"ttyS");
    }

    #[test]
    fn a_new_record_s_id_is_the_console_s_number_or_the_line_s_tail() {
        let console = Options {
            tty: b"tty3".to_vec(),
            vcline: Some(b"3".to_vec()),
            ..Options::default()
        };
        let rec = login_record(None, &console, 42, 40, 1_700_000_000);
        assert_eq!(&rec.ut_id, b"3\0\0\0");
        assert_eq!(field(&rec.ut_user), b"LOGIN");
        assert_eq!(field(&rec.ut_line), b"tty3");
        assert_eq!(
            (rec.ut_type, rec.ut_pid, rec.ut_session),
            (ut::LOGIN_PROCESS, 42, 40)
        );
        assert_eq!(rec.ut_tv.tv_sec, 1_700_000_000);
        assert_eq!(field(&rec.ut_host), b"");
        let serial = Options {
            tty: b"ttyUSB10".to_vec(),
            fakehost: Some(b"far.example".to_vec()),
            ..Options::default()
        };
        let rec = login_record(None, &serial, 1, 1, 0);
        assert_eq!(&rec.ut_id, b"SB10");
        assert_eq!(field(&rec.ut_host), b"far.example");
        let short = Options {
            tty: b"ab".to_vec(),
            ..Options::default()
        };
        assert_eq!(&login_record(None, &short, 1, 1, 0).ut_id, b"ab\0\0");
    }

    #[test]
    fn init_s_record_is_taken_over() {
        let mut init = Utmpx::ZERO;
        init.ut_type = ut::INIT_PROCESS;
        init.ut_pid = 42;
        init.ut_id = *b"T0\0\0";
        str2memcpy(&mut init.ut_user, b"averylongname");
        init.ut_tv.tv_usec = 7;
        assert!(is_ours(&init, 42));
        assert!(!is_ours(&init, 43));
        let rec = login_record(Some(init), &Options::default(), 42, 42, 5);
        assert_eq!(&rec.ut_id, b"T0\0\0");
        // "LOGIN" and its NUL over the name; the rest of it stays.
        assert_eq!(rec.ut_user.get(..8), Some(&b"LOGIN\0on"[..]));
        assert_eq!((rec.ut_tv.tv_sec, rec.ut_tv.tv_usec), (5, 7));
        let mut user = init;
        user.ut_type = ut::USER_PROCESS;
        assert!(is_ours(&user, 42));
        let mut boot = init;
        boot.ut_type = 2;
        assert!(!is_ours(&boot, 42));
    }
}
