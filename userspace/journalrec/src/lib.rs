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
/// The eight syslog priorities, in the spellings `journalctl` prints.
///
/// Indexed by the numeric priority, so `PRIORITY_NAMES[3]` is `err`.
pub const PRIORITY_NAMES: [&str; 8] = [
    "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug",
];

/// A `-p` argument -- a number 0..=7 or a name -- as the canonical name.
///
/// The aliases are the ones `journalctl`'s own `Priority::from_name`
/// accepts, because a record this writes has to be one that reader
/// understands. Both tables are the same table; if one gains a spelling the
/// other must too, and the test below is where that shows up.
#[must_use]
pub fn priority_name(spec: &str) -> Option<&'static str> {
    let lower = spec.trim().to_ascii_lowercase();
    let index = match lower.as_str() {
        "emerg" | "emergency" | "0" => 0,
        "alert" | "1" => 1,
        "crit" | "critical" | "2" => 2,
        "err" | "error" | "3" => 3,
        "warning" | "warn" | "4" => 4,
        "notice" | "5" => 5,
        "info" | "6" => 6,
        "debug" | "7" => 7,
        _ => return None,
    };
    PRIORITY_NAMES.get(index).copied()
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

    #[test]
    fn a_priority_is_accepted_by_number_and_by_name() {
        assert_eq!(priority_name("3"), Some("err"));
        assert_eq!(priority_name("err"), Some("err"));
        assert_eq!(priority_name("error"), Some("err"));
        assert_eq!(priority_name("ERR"), Some("err"));
        assert_eq!(priority_name(" warn "), Some("warning"));
        assert_eq!(priority_name("0"), Some("emerg"));
        assert_eq!(priority_name("7"), Some("debug"));
    }

    /// A value outside the set is `None` rather than a default. Silently
    /// meaning `info` by `-p bogus` is the shape of defect this tree has
    /// been pulling out of option parsers all week.
    #[test]
    fn an_unknown_priority_is_not_quietly_info() {
        assert_eq!(priority_name("8"), None);
        assert_eq!(priority_name("-1"), None);
        assert_eq!(priority_name("chatty"), None);
        assert_eq!(priority_name(""), None);
    }

    /// Every canonical name resolves to itself, so the table and the
    /// resolver cannot disagree about what the eight are.
    #[test]
    fn the_canonical_names_round_trip() {
        for (i, name) in PRIORITY_NAMES.iter().enumerate() {
            assert_eq!(priority_name(name), Some(*name));
            let n = alloc::format!("{i}");
            assert_eq!(priority_name(&n), Some(*name));
        }
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
