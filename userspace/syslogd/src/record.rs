//! One datagram from `/dev/log` as one journal record: what systemd-journald
//! would file for it (design-decisions §1063), in this journal's JSON-lines
//! spelling.
//!
//! The record's own fields are the five every journal writer fills:
//!
//! | field | from |
//! |---|---|
//! | `ts` | the time the datagram was received, as journald stamps it |
//! | `level` | the priority's level |
//! | `service` | the frame's identifier; failing that, the sender's command name, which is what `journalctl` on Linux shows in its place |
//! | `msg` | the message |
//! | `pid` | the frame's `[pid]` when it is a number; failing that, the sender's |
//!
//! and after them, journald's own fields, under its names, wherever journald
//! would store one: `SYSLOG_FACILITY` (with `facility`, the name `logger`'s
//! records carry), `SYSLOG_TIMESTAMP`, a `SYSLOG_PID` that is not a number,
//! `SYSLOG_RAW`, the sender's `_PID`, `_UID`, `_GID` and `_COMM` as the kernel
//! vouches for them, and `_TRANSPORT`. Every value goes through
//! [`journalrec::json_value`], so bytes that are not text are kept exactly.

use crate::frame::{self, Frame};

/// Who sent a datagram, as the kernel vouches for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Creds {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

/// The name glibc gives facility number `facility`, if it gives one --
/// `journalrec`'s, which every writer of a `facility` field shares.
#[must_use]
pub fn facility_name(facility: u32) -> Option<&'static str> {
    journalrec::facility_name(facility)
}

/// A `[pid]` that is a process number: ASCII digits only, as journald's
/// consumers read `SYSLOG_PID`, and one that fits.
fn pid_number(text: &[u8]) -> Option<u32> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse().ok()
}

/// The journal line, without its newline, for the datagram `raw` received at
/// `ts` from a sender the kernel describes as `creds`, whose command name is
/// `comm`.
#[must_use]
pub fn line(raw: &[u8], ts: u64, creds: Option<Creds>, comm: Option<&[u8]>) -> String {
    let f: Frame<'_> = frame::parse(raw);
    let level_index = usize::try_from(f.priority & frame::LOG_PRIMASK).unwrap_or(6);
    let level = journalrec::PRIORITY_NAMES
        .get(level_index)
        .copied()
        .unwrap_or("info");
    let service = match f.identifier {
        Some(id) if !id.is_empty() => id,
        _ => comm.unwrap_or_default(),
    };
    let claimed = f.pid.and_then(pid_number);
    let sender_pid = creds
        .and_then(|c| u32::try_from(c.pid).ok())
        .filter(|&p| p != 0);
    let record = journalrec::ByteRecord {
        ts,
        level,
        service,
        msg: f.message,
        pid: claimed.or(sender_pid),
    };

    let facility = f.priority.checked_shr(3).unwrap_or(0);
    let facility_text = facility.to_string();
    let pid_text;
    let uid_text;
    let gid_text;
    let mut extra: Vec<(&str, &[u8])> = Vec::new();
    // journald files `SYSLOG_FACILITY` only for a facility other than the
    // kernel's, number zero.
    if facility != 0 {
        if let Some(name) = facility_name(facility) {
            extra.push(("facility", name.as_bytes()));
        }
        extra.push(("SYSLOG_FACILITY", facility_text.as_bytes()));
    }
    if let Some(ts_text) = f.timestamp {
        extra.push(("SYSLOG_TIMESTAMP", ts_text));
    }
    if let Some(pid) = f.pid
        && claimed.is_none()
    {
        extra.push(("SYSLOG_PID", pid));
    }
    if f.keep_raw {
        extra.push(("SYSLOG_RAW", raw));
    }
    if let Some(c) = creds {
        pid_text = c.pid.to_string();
        uid_text = c.uid.to_string();
        gid_text = c.gid.to_string();
        extra.push(("_PID", pid_text.as_bytes()));
        extra.push(("_UID", uid_text.as_bytes()));
        extra.push(("_GID", gid_text.as_bytes()));
    }
    if let Some(name) = comm {
        extra.push(("_COMM", name));
    }
    extra.push(("_TRANSPORT", b"syslog"));
    record.to_json_line_with(&extra)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Creds, facility_name, line};

    const ME: Creds = Creds {
        pid: 4242,
        uid: 1000,
        gid: 1000,
    };

    #[test]
    fn the_local_form_files_its_parts_and_the_senders() {
        assert_eq!(
            line(
                b"<30>Oct  7 16:30:00 ntpd[123]: step",
                7,
                Some(ME),
                Some(&b"ntpd"[..])
            ),
            concat!(
                r#"{"ts":7,"level":"info","service":"ntpd","msg":"step","pid":123,"#,
                r#""facility":"daemon","SYSLOG_FACILITY":"3","SYSLOG_TIMESTAMP":"Oct  7 16:30:00 ","#,
                r#""_PID":"4242","_UID":"1000","_GID":"1000","_COMM":"ntpd","_TRANSPORT":"syslog"}"#
            )
        );
    }

    #[test]
    fn with_no_identifier_the_command_name_stands_in() {
        let got = line(
            b"<13>Oct  7 16:30:00 host t[1]: x",
            1,
            Some(ME),
            Some(&b"sh"[..]),
        );
        assert!(got.contains(r#""service":"sh""#), "{got}");
        assert!(got.contains(r#""msg":"host t[1]: x""#), "{got}");
        assert!(got.contains(r#""pid":4242"#), "the sender's: {got}");
    }

    #[test]
    fn a_pid_that_is_not_a_number_is_kept_beside_the_senders() {
        let got = line(b"<13>Oct  7 16:30:00 t[abc]: x", 1, Some(ME), None);
        assert!(got.contains(r#""pid":4242"#), "{got}");
        assert!(got.contains(r#""SYSLOG_PID":"abc""#), "{got}");
    }

    #[test]
    fn the_kernels_facility_is_not_filed_and_a_nameless_one_is_a_number() {
        let kern = line(b"<0>Oct  7 16:30:00 t: x", 1, None, None);
        assert!(!kern.contains("FACILITY"), "{kern}");
        assert!(kern.contains(r#""level":"emerg""#), "{kern}");
        let big = line(b"<999>Oct  7 16:30:00 t: x", 1, None, None);
        assert!(big.contains(r#""SYSLOG_FACILITY":"124""#), "{big}");
        assert!(!big.contains(r#""facility""#), "{big}");
        assert!(big.contains(r#""level":"debug""#), "{big}");
    }

    #[test]
    fn what_journald_keeps_raw_is_kept_raw_and_bytes_stay_bytes() {
        let got = line(b"<13>Oct  7 16:30:00 t: bad \xff\n", 1, None, None);
        assert!(got.contains(r#""msg":[98,97,100,32,255]"#), "{got}");
        assert!(got.contains(r#""SYSLOG_RAW":[60,"#), "{got}");
    }

    #[test]
    fn no_priority_is_user_info() {
        let got = line(b"plain", 1, None, None);
        assert!(got.contains(r#""level":"info""#), "{got}");
        assert!(got.contains(r#""facility":"user""#), "{got}");
        assert!(got.contains(r#""msg":"plain""#), "{got}");
    }

    #[test]
    fn facility_names_are_glibcs() {
        assert_eq!(facility_name(3), Some("daemon"));
        assert_eq!(facility_name(10), Some("authpriv"));
        assert_eq!(facility_name(12), None);
        assert_eq!(facility_name(23), Some("local7"));
        assert_eq!(facility_name(124), None);
    }
}
