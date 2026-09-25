//! Turning mail as it is stored into text a person can read.
//!
//! A message on disk is bytes, and most of what makes it readable is written
//! in encodings stacked on each other: a subject line of `=?UTF-8?B?...?=`
//! words (RFC 2047), a filename given as `filename*=UTF-8''%E2%82%AC.pdf`
//! (RFC 2231), a body in whatever character set its part names, often HTML
//! with no plain alternative, and whole folders of messages run together in
//! one mbox file. The parser in `main.rs` finds the parts; this module decodes
//! them.
//!
//! **Character sets** are the ones mail in the wild is written in and that
//! need no tables: UTF-8 and its ASCII subset, ISO 8859-1, ISO 8859-15 and
//! Windows-1252. Text that claims one of those and is not -- the commonest
//! being 8-bit bytes labelled `us-ascii` -- is shown as Windows-1252 with a
//! note saying so, rather than refused: a reader that can show nothing of a
//! message it has is worse than one that shows it and says what it assumed.

use std::collections::BTreeMap;

/// Text decoded from bytes, and what was assumed to get it, when anything
/// was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// Why the text may not be what was written: the bytes were not the
    /// character set they claimed, or it is one this does not read.
    pub note: Option<String>,
}

/// Windows-1252's 0x80..=0x9F, where it differs from ISO 8859-1; the five
/// positions it leaves undefined map to the control characters 8859-1 has.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
];

/// One byte of Windows-1252.
fn cp1252(b: u8) -> char {
    if (0x80..=0x9F).contains(&b) {
        CP1252_HIGH
            .get(usize::from(b.wrapping_sub(0x80)))
            .copied()
            .unwrap_or(char::from(b))
    } else {
        char::from(b)
    }
}

/// One byte of ISO 8859-15, which is 8859-1 with eight places changed.
fn latin9(b: u8) -> char {
    match b {
        0xA4 => '\u{20AC}',
        0xA6 => '\u{160}',
        0xA8 => '\u{161}',
        0xB4 => '\u{17D}',
        0xB8 => '\u{17E}',
        0xBC => '\u{152}',
        0xBD => '\u{153}',
        0xBE => '\u{178}',
        other => char::from(other),
    }
}

/// `bytes` as text in `charset`.
#[must_use]
pub fn decode_charset(bytes: &[u8], charset: &str) -> Decoded {
    let name = charset.trim().trim_matches('"').to_ascii_lowercase();
    let exact = |text: String| Decoded { text, note: None };
    let as_1252 = |why: String| Decoded {
        text: bytes.iter().map(|&b| cp1252(b)).collect(),
        note: Some(why),
    };
    match name.as_str() {
        "utf-8" | "utf8" | "us-ascii" | "ascii" | "" => match std::str::from_utf8(bytes) {
            Ok(text) => exact(text.to_owned()),
            Err(_) => as_1252(format!(
                "The text is not the {} it says it is; shown as Windows-1252.",
                if name.is_empty() {
                    "ASCII"
                } else {
                    charset.trim()
                }
            )),
        },
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" | "l1" | "iso_8859-1" => {
            exact(bytes.iter().map(|&b| char::from(b)).collect())
        }
        "windows-1252" | "cp1252" | "x-cp1252" => exact(bytes.iter().map(|&b| cp1252(b)).collect()),
        "iso-8859-15" | "iso8859-15" | "latin9" | "latin-9" => {
            exact(bytes.iter().map(|&b| latin9(b)).collect())
        }
        _ => match std::str::from_utf8(bytes) {
            Ok(text) => Decoded {
                text: text.to_owned(),
                note: Some(format!(
                    "The text is in {}, which this does not read; shown as UTF-8.",
                    charset.trim()
                )),
            },
            Err(_) => as_1252(format!(
                "The text is in {}, which this does not read; shown as Windows-1252.",
                charset.trim()
            )),
        },
    }
}

/// Decode base64, skipping what is not base64 (line breaks, padding).
#[must_use]
pub fn base64(input: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for b in input.bytes() {
        let value = match b {
            b'A'..=b'Z' => b.wrapping_sub(b'A'),
            b'a'..=b'z' => b.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => b.wrapping_sub(b'0').wrapping_add(52),
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buf = (buf << 6) | u32::from(value);
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            out.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
            buf &= (1_u32 << bits).wrapping_sub(1);
        }
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(b.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(b.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// An encoded word's "Q" text: `_` a space, `=XX` a byte.
fn q_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'_' => out.push(b' '),
            b'=' => {
                if let (Some(hi), Some(lo)) = (
                    bytes.get(i.saturating_add(1)).copied().and_then(hex),
                    bytes.get(i.saturating_add(2)).copied().and_then(hex),
                ) {
                    out.push(hi.wrapping_shl(4) | lo);
                    i = i.saturating_add(3);
                    continue;
                }
                out.push(b);
            }
            _ => out.push(b),
        }
        i = i.saturating_add(1);
    }
    out
}

/// One encoded word at the start of `s` -- `=?charset?B|Q?text?=` -- as its
/// text, and how many bytes of `s` it took.
fn encoded_word(s: &str) -> Option<(String, usize)> {
    let body = s.strip_prefix("=?")?;
    let charset_end = body.find('?')?;
    let charset = body.get(..charset_end)?;
    let rest = body.get(charset_end.saturating_add(1)..)?;
    let encoding_end = rest.find('?')?;
    let encoding = rest.get(..encoding_end)?;
    let rest = rest.get(encoding_end.saturating_add(1)..)?;
    let text_end = rest.find("?=")?;
    let text = rest.get(..text_end)?;
    if text.contains(char::is_whitespace) || charset.is_empty() {
        return None;
    }
    let bytes = match encoding {
        "B" | "b" => base64(text),
        "Q" | "q" => q_decode(text),
        _ => return None,
    };
    // RFC 2231 lets a language ride after a star: `utf-8*en`.
    let charset = charset.split('*').next().unwrap_or(charset);
    let used = 2_usize
        .saturating_add(charset_end)
        .saturating_add(1)
        .saturating_add(encoding_end)
        .saturating_add(1)
        .saturating_add(text_end)
        .saturating_add(2);
    Some((decode_charset(&bytes, charset).text, used))
}

/// A header value with its RFC 2047 encoded words decoded. The white space
/// between two encoded words is not part of the text -- it is how a long
/// word is split -- and is dropped; everything else is kept as written.
#[must_use]
pub fn header_words(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    let mut after_word = false;
    while let Some(at) = rest.find("=?") {
        let before = rest.get(..at).unwrap_or("");
        let from = rest.get(at..).unwrap_or("");
        if let Some((text, used)) = encoded_word(from) {
            if !(after_word && before.chars().all(char::is_whitespace)) {
                out.push_str(before);
            }
            out.push_str(&text);
            after_word = true;
            rest = from.get(used..).unwrap_or("");
        } else {
            out.push_str(before);
            out.push_str("=?");
            after_word = false;
            rest = from.get(2..).unwrap_or("");
        }
    }
    out.push_str(rest);
    out
}

/// Split a parameter list at the semicolons outside quotes, unquoting quoted
/// values (a backslash escapes the next character).
fn raw_params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut piece = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut pieces = Vec::new();
    for c in s.chars() {
        if escaped {
            piece.push(c);
            escaped = false;
        } else if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if c == ';' && !quoted {
            pieces.push(std::mem::take(&mut piece));
        } else {
            piece.push(c);
        }
    }
    pieces.push(piece);
    for piece in pieces {
        if let Some((key, value)) = piece.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            if !key.is_empty() {
                out.push((key, value.trim().to_owned()));
            }
        }
    }
    out
}

/// `%XX` escapes to bytes.
fn percent_bytes(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i.saturating_add(1)).copied().and_then(hex),
                bytes.get(i.saturating_add(2)).copied().and_then(hex),
            )
        {
            out.push(hi.wrapping_shl(4) | lo);
            i = i.saturating_add(3);
            continue;
        }
        out.push(b);
        i = i.saturating_add(1);
    }
    out
}

/// The parameters of a `Content-Type` or `Content-Disposition` value, after
/// its first `;`: names lower-cased, quoted values unquoted, and the RFC 2231
/// forms a name that is not ASCII arrives in -- `name*=charset'lang'%XX`, and
/// `name*0*=`, `name*1=` pieces -- put together and decoded. A plain value
/// with encoded words in it (not the standard, but what many mailers send
/// for a filename) is decoded as a header would be.
#[must_use]
pub fn params(s: &str) -> BTreeMap<String, String> {
    let mut plain: BTreeMap<String, String> = BTreeMap::new();
    // name -> (index, encoded?, raw value)
    let mut pieces: BTreeMap<String, Vec<(u32, bool, String)>> = BTreeMap::new();
    for (key, value) in raw_params(s) {
        let (base, encoded) = match key.strip_suffix('*') {
            Some(base) => (base.to_owned(), true),
            None => (key.clone(), false),
        };
        match base.split_once('*') {
            Some((name, index)) => {
                if let Ok(index) = index.parse::<u32>() {
                    pieces
                        .entry(name.to_owned())
                        .or_default()
                        .push((index, encoded, value));
                }
            }
            None if encoded => pieces.entry(base).or_default().push((0, true, value)),
            None => {
                plain.insert(base, header_words(&value));
            }
        }
    }
    for (name, mut parts) in pieces {
        parts.sort_by_key(|(index, _, _)| *index);
        let mut charset = String::from("us-ascii");
        let mut bytes = Vec::new();
        for (i, (_, encoded, value)) in parts.iter().enumerate() {
            if *encoded {
                let mut value = value.as_str();
                if i == 0 {
                    // charset'language'text
                    let mut split = value.splitn(3, '\'');
                    if let (Some(cs), Some(_lang), Some(text)) =
                        (split.next(), split.next(), split.next())
                    {
                        cs.clone_into(&mut charset);
                        value = text;
                    }
                }
                bytes.extend(percent_bytes(value));
            } else {
                bytes.extend_from_slice(value.as_bytes());
            }
        }
        plain.insert(name, decode_charset(&bytes, &charset).text);
    }
    plain
}

/// A plain reading of an HTML body: tags out, line breaks where blocks and
/// `<br>` put them, list items bulleted, the common entities decoded, scripts
/// and styles dropped, and white space run together as a browser runs it.
#[must_use]
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len().checked_div(2).unwrap_or(0));
    let mut chars = html.chars().peekable();
    let mut space = false;
    let push_break = |out: &mut String| {
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('\n');
    };
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let mut tag = String::new();
                let mut quote: Option<char> = None;
                for t in chars.by_ref() {
                    match quote {
                        Some(q) if t == q => quote = None,
                        Some(_) => {}
                        None if t == '"' || t == '\'' => quote = Some(t),
                        None if t == '>' => break,
                        None => {}
                    }
                    tag.push(t);
                }
                let name: String = tag
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '!')
                    .collect::<String>()
                    .to_ascii_lowercase();
                let closing = tag.starts_with('/');
                if !closing && (name == "script" || name == "style") {
                    // Everything up to the closing tag is code, not text.
                    let end = format!("</{name}");
                    let mut seen = String::new();
                    for t in chars.by_ref() {
                        seen.push(t.to_ascii_lowercase());
                        if seen.ends_with(&end) {
                            break;
                        }
                    }
                    for t in chars.by_ref() {
                        if t == '>' {
                            break;
                        }
                    }
                    continue;
                }
                match name.as_str() {
                    "br" => {
                        push_break(&mut out);
                        space = false;
                    }
                    "p" | "div" | "tr" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    | "blockquote" | "ul" | "ol" | "pre" | "hr" => {
                        // A paragraph, a list, a heading: set apart by a blank
                        // line on both sides, as they are drawn. A division or
                        // a table row is only a new line.
                        push_break(&mut out);
                        if name != "div" && name != "tr" {
                            push_break(&mut out);
                        }
                        space = false;
                    }
                    "li" if !closing => {
                        push_break(&mut out);
                        out.push_str("\u{2022} ");
                        space = false;
                    }
                    "td" | "th" if closing => {
                        out.push('\t');
                        space = false;
                    }
                    _ => {}
                }
            }
            '&' => {
                let mut entity = String::new();
                while let Some(&t) = chars.peek() {
                    if t == ';' || entity.len() > 10 || t.is_whitespace() || t == '<' {
                        break;
                    }
                    entity.push(t);
                    chars.next();
                }
                let decoded = match entity.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{A0}'),
                    e if e.starts_with("#x") || e.starts_with("#X") => e
                        .get(2..)
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .and_then(char::from_u32),
                    e if e.starts_with('#') => e
                        .get(1..)
                        .and_then(|d| d.parse::<u32>().ok())
                        .and_then(char::from_u32),
                    _ => None,
                };
                if let Some(d) = decoded {
                    if chars.peek() == Some(&';') {
                        chars.next();
                    }
                    if space {
                        out.push(' ');
                        space = false;
                    }
                    out.push(d);
                } else {
                    out.push('&');
                    out.push_str(&entity);
                }
            }
            c if c.is_whitespace() => {
                space = !out.is_empty() && !out.ends_with('\n');
            }
            c => {
                if space {
                    out.push(' ');
                    space = false;
                }
                out.push(c);
            }
        }
    }
    // No more than one blank line in a row, and none at either end.
    let mut folded = String::with_capacity(out.len());
    let mut blank = 0_u32;
    for line in out.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank = blank.saturating_add(1);
            if blank > 1 || folded.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        folded.push_str(line);
        folded.push('\n');
    }
    folded.trim_end().to_owned()
}

/// Whether `line` is an mbox envelope line: `From `, a sender, and a date --
/// four digits of a year, or a time, somewhere after the sender.
///
/// A writer should have escaped a body line that begins `From `, and most
/// do; one that did not leaves "From the start of the talk..." after a blank
/// line, which only its lack of a date tells from the next envelope.
#[must_use]
pub fn is_envelope(line: &[u8]) -> bool {
    let Some(rest) = line.strip_prefix(b"From ") else {
        return false;
    };
    let text: String = rest.iter().map(|&b| char::from(b)).collect();
    let mut words = text.split_whitespace();
    let has_sender = words.next().is_some();
    let dated = words.any(|w| {
        let digits = w.chars().filter(char::is_ascii_digit).count();
        (w.len() == 4 && digits == 4) || (w.contains(':') && digits >= 3)
    });
    has_sender && dated
}

/// The messages of an mbox file.
///
/// Each begins at an envelope line (`From sender date`, which is not part of
/// the message) at the start of the file or after a blank line. A line inside
/// a message that starts with one or more `>` before `From ` loses one `>`:
/// that is how both mbox variants keep a body line from reading as the next
/// envelope. A file that does not begin with an envelope line is one message.
#[must_use]
pub fn split_mbox(bytes: &[u8]) -> Vec<Vec<u8>> {
    let first = bytes.split(|b| *b == b'\n').next().unwrap_or_default();
    if !is_envelope(first) {
        return if bytes.is_empty() {
            Vec::new()
        } else {
            vec![bytes.to_vec()]
        };
    }
    let mut messages: Vec<Vec<u8>> = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    let mut previous_blank = true;
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let blank = line == b"\n" || line == b"\r\n";
        if previous_blank && is_envelope(line) {
            if let Some(mut done) = current.take() {
                // The blank line before an envelope belongs to the envelope.
                if done.ends_with(b"\r\n") {
                    done.truncate(done.len().saturating_sub(2));
                } else if done.ends_with(b"\n") {
                    done.truncate(done.len().saturating_sub(1));
                }
                messages.push(done);
            }
            current = Some(Vec::new());
            previous_blank = false;
            continue;
        }
        if let Some(message) = current.as_mut() {
            let quoted = line.iter().take_while(|b| **b == b'>').count();
            if quoted > 0 && line.get(quoted..).is_some_and(|l| l.starts_with(b"From ")) {
                message.extend_from_slice(line.get(1..).unwrap_or_default());
            } else {
                message.extend_from_slice(line);
            }
        }
        previous_blank = blank;
    }
    if let Some(done) = current {
        messages.push(done);
    }
    messages
}

/// A message's `Date:` header as seconds since 1970, UTC -- for sorting, and
/// `None` when it is not a date this reads. RFC 5322's form, with the day of
/// the week optional and the obsolete zone names.
#[must_use]
pub fn parse_date(value: &str) -> Option<i64> {
    let value = value.split_once(',').map_or(value, |(_, rest)| rest);
    let mut words = value.split_whitespace();
    let day: u32 = words.next()?.parse().ok()?;
    let month = match words.next()?.to_ascii_lowercase().get(..3)? {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let year: i32 = words.next()?.parse().ok()?;
    // Two-digit years, as the obsolete syntax has them.
    let year = match year {
        0..=49 => year.saturating_add(2000),
        50..=999 => year.saturating_add(1900),
        y => y,
    };
    let mut clock = words.next()?.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next().map_or(Some(0), |s| s.parse().ok())?;
    let zone = words.next().unwrap_or("+0000");
    let offset_minutes: i64 = match zone.to_ascii_uppercase().as_str() {
        "UT" | "GMT" | "Z" => 0,
        "EST" => -300,
        "EDT" => -240,
        "CST" => -360,
        "CDT" => -300,
        "MST" => -420,
        "MDT" => -360,
        "PST" => -480,
        "PDT" => -420,
        z => {
            let behind = match z.get(..1)? {
                "+" => false,
                "-" => true,
                _ => return None,
            };
            let digits = z.get(1..5)?;
            let hh: i64 = digits.get(..2)?.parse().ok()?;
            let mm: i64 = digits.get(2..)?.parse().ok()?;
            let minutes = hh.saturating_mul(60).saturating_add(mm);
            if behind {
                minutes.saturating_neg()
            } else {
                minutes
            }
        }
    };
    if !(1..=12).contains(&month)
        || day == 0
        || day > guitk::date::days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let date = guitk::date::Date::from_ymd(year, month, day);
    Some(
        date.unix_secs_utc()
            .saturating_add(hour.saturating_mul(3600))
            .saturating_add(minute.saturating_mul(60))
            .saturating_add(second)
            .saturating_sub(offset_minutes.saturating_mul(60)),
    )
}

// ---------------------------------------------------------------------------
// Writing: the same codings, the other way
// ---------------------------------------------------------------------------

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64, unwrapped.
#[must_use]
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(
        data.len()
            .saturating_add(2)
            .checked_div(3)
            .unwrap_or(0)
            .saturating_mul(4),
    );
    for chunk in data.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let triple = (b(0) << 16) | (b(1) << 8) | b(2);
        let sextet = |shift: u32| {
            BASE64
                .get(usize::try_from((triple >> shift) & 0x3F).unwrap_or(0))
                .copied()
                .map_or('A', char::from)
        };
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// Text as quoted-printable (RFC 2045), with CRLF line ends.
///
/// Printable ASCII but `=` is written as it is; `=`, every byte of a
/// character that is not ASCII, and a space or tab ending a line are
/// written `=XX`; and a line longer than 76 characters is broken with a soft
/// line break (`=` at the end), which a reader joins back up.
#[must_use]
pub fn quoted_printable(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len().saturating_add(16));
    let normalised = text.replace("\r\n", "\n");
    let mut lines = normalised.split('\n').peekable();
    while let Some(line) = lines.next() {
        let bytes = line.as_bytes();
        let mut width = 0_usize;
        for (i, &b) in bytes.iter().enumerate() {
            let last = i.saturating_add(1) == bytes.len();
            let literal =
                ((b == b' ' || b == b'\t') && !last) || ((33..=126).contains(&b) && b != b'=');
            let piece_len = if literal { 1 } else { 3 };
            // Room for the piece and, unless it ends the line, a soft break.
            let limit = if last { 76 } else { 75 };
            if width.saturating_add(piece_len) > limit {
                out.push_str("=\r\n");
                width = 0;
            }
            if literal {
                out.push(char::from(b));
            } else {
                out.push('=');
                out.push(
                    HEX.get(usize::from(b >> 4))
                        .copied()
                        .map_or('0', char::from),
                );
                out.push(
                    HEX.get(usize::from(b & 0x0F))
                        .copied()
                        .map_or('0', char::from),
                );
            }
            width = width.saturating_add(piece_len);
        }
        if lines.peek().is_some() {
            out.push_str("\r\n");
        }
    }
    out
}

/// A header value as the ASCII a header must be: as it is when it already
/// is, and otherwise UTF-8 encoded words (RFC 2047) of at most 75 characters
/// each, folded onto continuation lines.
#[must_use]
pub fn header_word(value: &str) -> String {
    if value.bytes().all(|b| (32..=126).contains(&b)) {
        return value.to_owned();
    }
    // 45 bytes of text is 60 of base64, which with `=?UTF-8?B?` and `?=` is
    // 72: under the 75 a word may be.
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in value.chars() {
        if chunk.len().saturating_add(c.len_utf8()) > 45 {
            words.push(format!("=?UTF-8?B?{}?=", base64_encode(chunk.as_bytes())));
            chunk.clear();
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        words.push(format!("=?UTF-8?B?{}?=", base64_encode(chunk.as_bytes())));
    }
    words.join("\r\n ")
}

/// A `Content-Type` or `Content-Disposition` parameter: `name="value"` for
/// ASCII, and RFC 2231's `name*=UTF-8''%XX...` for anything else, which is
/// the form a reader decodes back to the same name.
#[must_use]
pub fn param(name: &str, value: &str) -> String {
    if value.bytes().all(|b| (32..=126).contains(&b)) {
        let mut quoted = String::with_capacity(value.len().saturating_add(2));
        quoted.push('"');
        for c in value.chars() {
            if c == '"' || c == '\\' {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        return format!("{name}={quoted}");
    }
    let mut encoded = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{name}*=UTF-8''{encoded}")
}

/// Seconds since 1970 as an RFC 5322 date, in UTC: "Thu, 25 Sep 2026
/// 12:03:07 +0000".
#[must_use]
pub fn rfc5322_date(secs: i64) -> String {
    let date = guitk::date::Date::from_unix_utc(secs);
    let (y, m, d) = date.ymd();
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(usize::try_from(m.saturating_sub(1)).unwrap_or(0))
    .copied()
    .unwrap_or("Jan");
    let weekday = date.weekday().short_name();
    let day_secs = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (
        day_secs.checked_div(3_600).unwrap_or(0),
        day_secs
            .checked_div(60)
            .unwrap_or(0)
            .checked_rem(60)
            .unwrap_or(0),
        day_secs.checked_rem(60).unwrap_or(0),
    );
    format!("{weekday}, {d} {month} {y} {hh:02}:{mm:02}:{ss:02} +0000")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn character_sets_are_decoded_and_a_wrong_label_is_said() {
        let bytes = [b'c', b'a', b'f', 0xE9];
        assert_eq!(decode_charset(&bytes, "ISO-8859-1").text, "caf\u{e9}");
        assert_eq!(decode_charset(&bytes, "windows-1252").text, "caf\u{e9}");
        assert_eq!(decode_charset(&[0x80], "cp1252").text, "\u{20AC}");
        assert_eq!(decode_charset(&[0xA4], "iso-8859-15").text, "\u{20AC}");
        assert_eq!(
            decode_charset("caf\u{e9}".as_bytes(), "utf-8"),
            Decoded {
                text: String::from("caf\u{e9}"),
                note: None
            }
        );
        let mislabelled = decode_charset(&bytes, "us-ascii");
        assert_eq!(mislabelled.text, "caf\u{e9}");
        assert!(mislabelled.note.unwrap().contains("not the us-ascii"));
        let unknown = decode_charset(b"plain", "koi8-r");
        assert_eq!(unknown.text, "plain");
        assert!(unknown.note.unwrap().contains("koi8-r"));
    }

    #[test]
    fn encoded_words_are_decoded_and_joined() {
        assert_eq!(header_words("=?UTF-8?B?Q2Fmw6k=?="), "Caf\u{e9}");
        assert_eq!(
            header_words("=?iso-8859-1?Q?caf=E9_cr=E8me?="),
            "caf\u{e9} cr\u{e8}me"
        );
        assert_eq!(
            header_words("=?utf-8?q?one?= \r\n =?utf-8?q?two?= three"),
            "onetwo three",
            "the space between two encoded words is not text; after one it is"
        );
        assert_eq!(
            header_words("Re: plain =? not a word"),
            "Re: plain =? not a word"
        );
        assert_eq!(
            header_words("=?utf-8*en?Q?x?="),
            "x",
            "a language after the charset"
        );
        assert_eq!(header_words("=?utf-8?X?abc?="), "=?utf-8?X?abc?=");
    }

    #[test]
    fn parameters_are_unquoted_and_rfc_2231_is_put_together() {
        let p = params(" charset=\"utf-8\"; name=\"a;b.pdf\"; format=flowed");
        assert_eq!(p["charset"], "utf-8");
        assert_eq!(p["name"], "a;b.pdf", "a semicolon inside quotes");
        assert_eq!(p["format"], "flowed");
        let p = params(" filename*=UTF-8''%E2%82%AC%20rates.pdf");
        assert_eq!(p["filename"], "\u{20AC} rates.pdf");
        let p = params(" filename*0*=utf-8''caf%C3%A9; filename*1=\"-menu.txt\"");
        assert_eq!(p["filename"], "caf\u{e9}-menu.txt");
        let p = params(" filename=\"=?UTF-8?B?Q2Fmw6kucGRm?=\"");
        assert_eq!(p["filename"], "Caf\u{e9}.pdf");
        let p = params(" filename=\"q\\\"uote.txt\"");
        assert_eq!(p["filename"], "q\"uote.txt");
    }

    #[test]
    fn html_reads_as_text() {
        let html = "<html><head><style>p { color: red }</style></head><body>\
                    <p>Hello&nbsp;<b>world</b> &amp; all</p><script>alert(1)</script>\
                    <ul><li>one</li><li>two &#8364; &#x41;</li></ul>line<br>break\
                    </body></html>";
        let text = html_to_text(html);
        assert_eq!(
            text,
            "Hello\u{a0}world & all\n\n\u{2022} one\n\u{2022} two \u{20AC} A\n\nline\nbreak"
        );
        assert!(!text.contains("color"), "a style sheet was read as text");
        assert!(!text.contains("alert"), "a script was read as text");
    }

    /// Envelope lines are told from body lines that begin "From" by the date
    /// an envelope carries.
    #[test]
    fn an_mbox_splits_into_its_messages() {
        let mbox = b"From a@example.com Thu Sep 25 10:00:00 2026\n\
            Subject: one\n\nbody one\n>From the start\n\n\
            From b@example.com Thu Sep 25 11:00:00 2026\r\n\
            Subject: two\r\n\r\nFrom inside, not after a blank\r\n>>From quoted twice\r\n";
        let messages = split_mbox(mbox);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], b"Subject: one\n\nbody one\nFrom the start\n");
        assert_eq!(
            messages[1],
            b"Subject: two\r\n\r\nFrom inside, not after a blank\r\n>From quoted twice\r\n"
        );
        assert_eq!(split_mbox(b"Subject: x\n\nnot an mbox").len(), 1);
        assert!(split_mbox(b"").is_empty());
    }

    /// Quoted-printable written here reads back as the same text, `=` and
    /// characters that are not ASCII included, with no line over 76.
    #[test]
    fn quoted_printable_round_trips() {
        let text = "x=41 and caf\u{e9}\ntrailing space \n".to_owned() + &"long ".repeat(40);
        let encoded = quoted_printable(&text);
        assert!(encoded.contains("x=3D41"), "{encoded}");
        assert!(encoded.contains("caf=C3=A9"), "{encoded}");
        assert!(encoded.contains("space=20\r\n"), "{encoded}");
        assert!(encoded.lines().all(|l| l.len() <= 76), "{encoded}");
        assert!(encoded.bytes().all(|b| b.is_ascii()));
        let back = crate::quoted_printable_decode(&encoded);
        assert_eq!(String::from_utf8(back).unwrap(), text.replace('\n', "\r\n"));
    }

    /// A header value that is not ASCII becomes encoded words that decode
    /// back to it; one that is ASCII stays as it is.
    #[test]
    fn header_words_round_trip() {
        assert_eq!(header_word("Plain subject"), "Plain subject");
        let long = "R\u{e9}sum\u{e9} ".repeat(12);
        let words = header_word(&long);
        assert!(words.bytes().all(|b| b.is_ascii()));
        assert!(words.split("\r\n ").all(|w| w.len() <= 75), "{words}");
        assert_eq!(header_words(&words.replace("\r\n", "")), long);
    }

    /// A parameter that is not ASCII is written in RFC 2231 form, and reads
    /// back as the same name.
    #[test]
    fn parameters_round_trip() {
        assert_eq!(
            param("filename", "a \"b\".txt"),
            "filename=\"a \\\"b\\\".txt\""
        );
        let written = param("filename", "\u{20AC} totals.pdf");
        assert_eq!(written, "filename*=UTF-8''%E2%82%AC%20totals.pdf");
        assert_eq!(
            params(&format!("; {written}"))["filename"],
            "\u{20AC} totals.pdf"
        );
    }

    #[test]
    fn a_date_is_written_as_rfc_5322_and_read_back() {
        let written = rfc5322_date(1_790_337_787);
        assert_eq!(written, "Fri, 25 Sep 2026 12:03:07 +0000");
        assert_eq!(parse_date(&written), Some(1_790_337_787));
    }

    #[test]
    fn dates_are_read_to_seconds() {
        assert_eq!(
            parse_date("Thu, 25 Sep 2026 14:03:07 +0200"),
            Some(1_790_337_787)
        );
        assert_eq!(parse_date("25 Sep 2026 12:03:07 GMT"), Some(1_790_337_787));
        assert_eq!(
            parse_date("Thu, 25 Sep 26 07:03:07 EST"),
            Some(1_790_337_787)
        );
        assert_eq!(
            parse_date("Thu, 25 Sep 2026 12:03 +0000"),
            Some(1_790_337_780)
        );
        assert_eq!(parse_date("yesterday"), None);
        assert_eq!(parse_date("31 Feb 2026 10:00:00 +0000"), None);
    }
}
