//! A datagram from `/dev/log`, read as systemd-journald reads one.
//!
//! journald owns `/dev/log` on the Linux every program here is measured
//! against, so what it makes of a frame is what a log reader there sees
//! (design-decisions §1063). This is systemd 255's `journald-syslog.c` --
//! `syslog_parse_priority`, `syslog_skip_timestamp`, `syslog_parse_identifier`
//! and the trimming `server_process_syslog_message` does first -- checked
//! against WSL's journald by `scripts/syslogd-diff.sh`.
//!
//! journald works on a NUL-terminated copy of the datagram, so its parsing
//! stops at the first NUL byte; the message it files ends there too, and the
//! whole datagram is kept beside it (`SYSLOG_RAW`). Those are measured
//! behaviours, reproduced as they are.

/// `LOG_USER | LOG_INFO`: what a frame with no usable `<PRI>` is filed as.
pub const DEFAULT_PRIORITY: u32 = (1 << 3) | 6;

/// `LOG_PRIMASK`: the level bits of a priority.
pub const LOG_PRIMASK: u32 = 0x07;

/// journald's `WHITESPACE`.
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// One frame, taken apart.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Facility and level together, as `<PRI>` gave them, or
    /// [`DEFAULT_PRIORITY`]. Not range-checked: `<999>` is facility 124.
    pub priority: u32,
    /// `Mmm dd hh:mm:ss ` -- with its trailing space, as journald keeps it --
    /// when the frame had exactly that.
    pub timestamp: Option<&'a [u8]>,
    /// The first word, when it ended in `:`, without its `[pid]` and colon.
    /// May be empty: `[99]: x` has an empty identifier and pid 99.
    pub identifier: Option<&'a [u8]>,
    /// What stood between `[` and `]` before the colon, unchecked: `abc` and
    /// the empty string are both kept, as journald keeps them.
    pub pid: Option<&'a [u8]>,
    /// The rest, one separator after the identifier's colon consumed.
    pub message: &'a [u8],
    /// Whether the whole datagram should be kept beside the parts, as
    /// journald's `SYSLOG_RAW`: whitespace was trimmed from either end, the
    /// datagram held a NUL, or it had no timestamp.
    pub keep_raw: bool,
}

/// Take `raw` apart. Never fails: what is not understood stays in the
/// message.
#[must_use]
pub fn parse(raw: &[u8]) -> Frame<'_> {
    // journald trims trailing whitespace by length -- and in that scan a NUL
    // counts as whitespace, since `strchr (WHITESPACE, '\0')` finds the
    // string's own terminator -- then leading whitespace as a C string.
    let mut end = raw.len();
    while end > 0
        && raw
            .get(end.saturating_sub(1))
            .is_some_and(|&b| is_ws(b) || b == 0)
    {
        end = end.saturating_sub(1);
    }
    let trimmed_end = end != raw.len();
    let cstr_end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let has_nul = cstr_end < raw.len();
    let leading = raw
        .get(..cstr_end)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| is_ws(b))
        .count();
    // What is parsed is the C string between the two trims.
    let body_end = end.min(cstr_end).max(leading);
    let mut text = raw.get(leading..body_end).unwrap_or_default();

    let mut priority = DEFAULT_PRIORITY;
    if let Some((value, rest)) = parse_priority(text) {
        priority = value;
        text = rest;
    }

    let (timestamp, after_ts) = skip_timestamp(text);
    let mut keep_raw = leading > 0 || trimmed_end || has_nul;
    if timestamp.is_none() {
        keep_raw = true;
    }
    text = after_ts;

    let (identifier, pid, message) = parse_identifier(text);
    Frame {
        priority,
        timestamp,
        identifier,
        pid,
        message,
        keep_raw,
    }
}

/// `syslog_parse_priority (&p, &priority, true)`: `<` one to three digits
/// `>`, read as decimal with no range check. Anything else is not a
/// priority and leaves the text as it was.
fn parse_priority(text: &[u8]) -> Option<(u32, &[u8])> {
    if text.first() != Some(&b'<') {
        return None;
    }
    let close = text.iter().position(|&b| b == b'>')?;
    let digits = text.get(1..close)?;
    if digits.is_empty() || digits.len() > 3 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // Every byte is an ASCII digit, checked above, so no subtraction wraps.
    let value = digits.iter().fold(0u32, |n, &d| {
        n.saturating_mul(10)
            .saturating_add(u32::from(d.saturating_sub(b'0')))
    });
    Some((
        value,
        text.get(close.saturating_add(1)..).unwrap_or_default(),
    ))
}

/// What one position of the timestamp must hold.
#[derive(Clone, Copy)]
enum Want {
    Letter,
    Space,
    Number,
    SpaceOrNumber,
    Colon,
}

/// `Mmm dd hh:mm:ss `, character class by character class.
const TIMESTAMP: [Want; 16] = [
    Want::Letter,
    Want::Letter,
    Want::Letter,
    Want::Space,
    Want::SpaceOrNumber,
    Want::Number,
    Want::Space,
    Want::SpaceOrNumber,
    Want::Number,
    Want::Colon,
    Want::SpaceOrNumber,
    Want::Number,
    Want::Colon,
    Want::SpaceOrNumber,
    Want::Number,
    Want::Space,
];

/// `syslog_skip_timestamp`: the timestamp and what follows it, or none and
/// the text as it was.
fn skip_timestamp(text: &[u8]) -> (Option<&[u8]>, &[u8]) {
    for (i, want) in TIMESTAMP.iter().enumerate() {
        let Some(&b) = text.get(i) else {
            return (None, text);
        };
        let ok = match want {
            Want::Letter => b.is_ascii_alphabetic(),
            Want::Space => b == b' ',
            Want::Number => b.is_ascii_digit(),
            Want::SpaceOrNumber => b == b' ' || b.is_ascii_digit(),
            Want::Colon => b == b':',
        };
        if !ok {
            return (None, text);
        }
    }
    let (ts, rest) = text.split_at(TIMESTAMP.len());
    (Some(ts), rest)
}

/// `syslog_parse_identifier`: the first word, if it ends in `:`, with a
/// `[pid]` before the colon; then one whitespace separator. Otherwise no
/// identifier, and the text as it was.
fn parse_identifier(text: &[u8]) -> (Option<&[u8]>, Option<&[u8]>, &[u8]) {
    let start = text.iter().take_while(|&&b| is_ws(b)).count();
    let word = text.get(start..).unwrap_or_default();
    let len = word.iter().take_while(|&&b| !is_ws(b)).count();
    if len == 0 || word.get(len.saturating_sub(1)) != Some(&b':') {
        return (None, None, text);
    }
    // Without the colon.
    let mut id_end = len.saturating_sub(1);
    let mut pid = None;
    if id_end > 0 && word.get(id_end.saturating_sub(1)) == Some(&b']') {
        // The `[` nearest the end, searching back from the `]`.
        let close = id_end.saturating_sub(1);
        if let Some(open) = word
            .get(..close)
            .and_then(|w| w.iter().rposition(|&b| b == b'['))
        {
            pid = word.get(open.saturating_add(1)..close);
            id_end = open;
        }
    }
    let identifier = word.get(..id_end);
    // One separator, if the word is followed by whitespace.
    let mut after = len;
    if word.get(after).is_some_and(|&b| is_ws(b)) {
        after = after.saturating_add(1);
    }
    (identifier, pid, word.get(after..).unwrap_or_default())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::{DEFAULT_PRIORITY, Frame, parse};

    fn frame(raw: &[u8]) -> Frame<'_> {
        parse(raw)
    }

    // Each case below was sent to WSL's systemd 255 journald on 2026-10-07
    // and read back with `journalctl -o json`; the expectations are what it
    // stored.

    #[test]
    fn the_local_form() {
        let f = frame(b"<13>Oct  7 16:30:00 tag[123]: hello local");
        assert_eq!(f.priority, 13);
        assert_eq!(f.timestamp, Some(&b"Oct  7 16:30:00 "[..]));
        assert_eq!(f.identifier, Some(&b"tag"[..]));
        assert_eq!(f.pid, Some(&b"123"[..]));
        assert_eq!(f.message, b"hello local");
        assert!(!f.keep_raw);
    }

    #[test]
    fn no_pid() {
        let f = frame(b"<13>Oct  7 16:30:00 tag: no pid");
        assert_eq!((f.identifier, f.pid), (Some(&b"tag"[..]), None));
        assert_eq!(f.message, b"no pid");
    }

    #[test]
    fn a_hostname_is_not_understood_and_stays_in_the_message() {
        let f = frame(b"<13>Oct  7 16:30:00 myhost tag[123]: with a hostname");
        assert_eq!(f.identifier, None);
        assert_eq!(f.message, b"myhost tag[123]: with a hostname");
        assert!(!f.keep_raw);
    }

    #[test]
    fn an_rfc5424_frame_is_not_understood_either() {
        let raw = b"<34>1 2026-10-07T16:30:00Z myhost app 456 ID47 - msg";
        let f = frame(raw);
        assert_eq!(f.priority, 34);
        assert_eq!(f.timestamp, None);
        assert_eq!(f.identifier, None);
        assert_eq!(f.message, &raw[4..]);
        assert!(f.keep_raw, "no timestamp: the raw frame is kept");
    }

    #[test]
    fn an_identifier_needs_its_colon_at_the_end_of_its_word() {
        let f = frame(b"<13>Oct  7 16:30:00 tag[12]:msg without space");
        assert_eq!(f.identifier, None);
        assert_eq!(f.message, b"tag[12]:msg without space");
        let f = frame(b"<14>Oct  7 16:30:00 tag[ 77 ]: spaced pid");
        assert_eq!(f.identifier, None);
        assert_eq!(f.priority, 14);
    }

    #[test]
    fn the_pid_is_whatever_the_brackets_hold() {
        assert_eq!(
            frame(b"<13>Oct  7 16:30:00 t[abc]: x").pid,
            Some(&b"abc"[..])
        );
        let f = frame(b"<13>Oct  7 16:30:00 t[]: x");
        assert_eq!((f.identifier, f.pid), (Some(&b"t"[..]), Some(&b""[..])));
        let f = frame(b"<13>Oct  7 16:30:00 [99]: no identifier");
        assert_eq!((f.identifier, f.pid), (Some(&b""[..]), Some(&b"99"[..])));
    }

    #[test]
    fn one_separator_is_consumed_and_no_more() {
        assert_eq!(frame(b"<13>Oct  7 16:30:00 t:   three").message, b"  three");
        assert_eq!(frame(b"<13>Oct  7 16:30:00 t:\ttab").message, b"tab");
        assert_eq!(frame(b"<13>Oct  7 16:30:00 t:").message, b"");
    }

    #[test]
    fn whitespace_at_either_end_is_trimmed_and_the_raw_frame_kept() {
        let f = frame(b"  <13>Oct  7 16:30:00 t: leading");
        assert_eq!((f.priority, f.message), (13, &b"leading"[..]));
        assert!(f.keep_raw);
        let f = frame(b"<13>Oct  7 16:30:00 t: end   ");
        assert_eq!(f.message, b"end");
        assert!(f.keep_raw);
        let f = frame(b"<13>Oct  7 16:30:00 t: newline\n");
        assert_eq!(f.message, b"newline");
        assert!(f.keep_raw);
    }

    #[test]
    fn a_nul_ends_the_message_and_keeps_the_raw_frame() {
        let f = frame(b"<13>Oct  7 16:30:00 t: nul\0after");
        assert_eq!(f.message, b"nul");
        assert!(f.keep_raw);
    }

    #[test]
    fn a_malformed_priority_is_the_default_and_stays_in_the_text() {
        for raw in [
            &b"<13t: unclosed"[..],
            b"<abc>t: letters",
            b"<1234>t: four digits",
        ] {
            let f = frame(raw);
            assert_eq!(f.priority, DEFAULT_PRIORITY, "{raw:?}");
            assert!(f.identifier.unwrap().starts_with(b"<"), "{raw:?}");
            assert!(f.keep_raw, "{raw:?}");
        }
    }

    #[test]
    fn the_priority_is_not_range_checked() {
        let f = frame(b"<999>Oct  7 16:30:00 t: big");
        assert_eq!((f.priority >> 3, f.priority & 7), (124, 7));
        assert_eq!(frame(b"<0>Oct  7 16:30:00 t: kern").priority, 0);
    }

    #[test]
    fn the_timestamp_is_exactly_that_shape() {
        assert!(
            frame(b"<13>Oct 07 16:30:00 t: zero day")
                .timestamp
                .is_some()
        );
        let f = frame(b"<13>Oct  7 16:30:00t: no space after");
        assert_eq!(f.timestamp, None);
        assert_eq!(f.identifier, None, "the first word is `Oct`");
        assert_eq!(f.message, b"Oct  7 16:30:00t: no space after");
        let f = frame(b"<13>t: no timestamp");
        assert_eq!((f.timestamp, f.identifier), (None, Some(&b"t"[..])));
        assert!(f.keep_raw);
    }

    #[test]
    fn a_control_byte_or_a_bad_byte_is_kept_as_it_is() {
        assert_eq!(frame(b"<13>Oct  7 16:30:00 t: \x01ctl").message, b"\x01ctl");
        assert_eq!(
            frame(b"<13>Oct  7 16:30:00 t: bad \xff").message,
            b"bad \xff"
        );
        assert_eq!(
            frame(b"<13>Oct  7 16:30:00 t: multi\nline").message,
            b"multi\nline"
        );
    }
}
