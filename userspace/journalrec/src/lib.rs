//! One spelling of a journal record, shared by everything that writes one.
//!
//! `journalctl` reads JSON-lines records: `ts`, `level`, `service`, `msg`
//! and `pid`, from `/var/log/syslog.jsonl` (and from `/var/log/journal/`,
//! which nothing currently writes). `syslogd` produces them; `systemd-cat`
//! needs to, and the two must escape identically or the reader mis-parses
//! one of them.
//!
//! Escaping is the part that has to be shared rather than re-derived. A log
//! message is attacker-shaped text: unescaped, a `"` ends the field and a
//! newline ends the *record*, so anything that can get a line into the log
//! can forge entries around it. There is one escaper here and `syslogd`'s
//! private copy is gone.
//!
//! No dependencies and a `&str`/`String` API, so this stays a formatter and
//! not a second libc -- §768's exemption.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The file `journalctl` reads first among its fallbacks, and the one
/// `syslogd` writes. A second writer belongs here rather than in
/// `/var/log/journal/`: `journalctl` consults that directory *first* and
/// only falls back when it yields nothing, so a lone record there would
/// hide every syslog entry on the machine.
pub const MAIN_LOG_PATH: &str = "/var/log/syslog.jsonl";

/// Escape `s` for a JSON string body.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(8));
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}
/// One journal record, in the fields `journalctl` reads.
#[derive(Debug, Clone)]
pub struct Record {
    /// Seconds since the epoch.
    pub ts: u64,
    /// `emerg`..`debug`. `journalctl` reads this as the priority.
    pub level: String,
    /// The originating identifier -- `systemd-cat -t` sets it. Read by
    /// `journalctl` as the unit, which also accepts a `unit` key; this
    /// writes `service`, the spelling the documented example uses.
    pub service: String,
    /// The line itself.
    pub msg: String,
    /// The process the line is attributed to, when one is known.
    pub pid: Option<u32>,
}

impl Record {
    /// The record as one JSON-lines entry, without the trailing newline.
    #[must_use]
    pub fn to_json_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("\"ts\":{}", self.ts));
        parts.push(format!("\"level\":\"{}\"", escape(&self.level)));
        parts.push(format!("\"service\":\"{}\"", escape(&self.service)));
        parts.push(format!("\"msg\":\"{}\"", escape(&self.msg)));
        if let Some(pid) = self.pid {
            parts.push(format!("\"pid\":{pid}"));
        }
        let mut out = String::from("{");
        out.push_str(&parts.join(","));
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    /// The reason this is one function and not two. A message is
    /// attacker-shaped text: a bare quote ends the field and a bare newline
    /// ends the record, so either would let whoever produced the line write
    /// journal entries around it.
    #[test]
    fn a_quote_cannot_end_the_field() {
        assert_eq!(escape("a\"b"), "a\\\"b");
    }

    #[test]
    fn a_newline_cannot_end_the_record() {
        let escaped = escape("a\nb");
        assert_eq!(escaped, "a\\nb");
        assert!(!escaped.contains(char::from(10)), "a real newline survived");
    }

    #[test]
    fn a_backslash_is_doubled_not_dropped() {
        assert_eq!(escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn a_control_byte_becomes_a_unicode_escape() {
        assert_eq!(escape("\u{1}"), "\\u0001");
    }

    #[test]
    fn a_record_carries_the_fields_journalctl_reads() {
        let r = Record {
            ts: 1_716_000_000,
            level: "info".to_string(),
            service: "net.dhcp".to_string(),
            msg: "lease renewed".to_string(),
            pid: Some(42),
        };
        assert_eq!(
            r.to_json_line(),
            r#"{"ts":1716000000,"level":"info","service":"net.dhcp","msg":"lease renewed","pid":42}"#
        );
    }

    /// A record with no pid omits the key rather than writing 0, which
    /// would attribute the line to the kernel's idle task.
    #[test]
    fn an_unknown_pid_is_absent_not_zero() {
        let r = Record {
            ts: 1,
            level: "info".to_string(),
            service: "x".to_string(),
            msg: "y".to_string(),
            pid: None,
        };
        assert!(!r.to_json_line().contains("pid"));
    }
}
