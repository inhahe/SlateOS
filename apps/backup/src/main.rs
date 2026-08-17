//! backup — Slate OS snapshot-based backup system.
//!
//! A CLI tool for creating, managing, and restoring backups with content
//! deduplication via a SHA-256 content-addressed store.
//!
//! Usage:
//!   backup create [--full|--incremental|--differential] --source <PATH> --dest <PATH> [--exclude PATTERN]...
//!   backup restore <BACKUP_ID> --dest <PATH> [--files PATTERN]
//!   backup list [--source <PATH>]
//!   backup verify <BACKUP_ID>
//!   backup prune --keep-last N [--keep-daily N] [--keep-weekly N] [--keep-monthly N]
//!   backup schedule --source <PATH> --dest <PATH> --interval <daily|weekly|monthly>
//!   backup diff <BACKUP_ID1> <BACKUP_ID2>
//!   backup info <BACKUP_ID>

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// SHA-256 (inline, no external crate)
// ============================================================================

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        self.total_len += data.len() as u64;

        if self.buffer_len > 0 {
            let space = 64 - self.buffer_len;
            let copy = space.min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + copy].copy_from_slice(&data[..copy]);
            self.buffer_len += copy;
            offset = copy;

            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }

        while offset + 64 <= data.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[offset..offset + 64]);
            self.compress(&block);
            offset += 64;
        }

        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len * 8;
        let mut padding = vec![0x80u8];
        let pad_len = (55usize.wrapping_sub(self.buffer_len)) % 64;
        padding.resize(1 + pad_len, 0u8);
        padding.extend_from_slice(&bit_len.to_be_bytes());
        self.update(&padding);

        let mut hash = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            hash[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        hash
    }
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

fn sha256_hex(data: &[u8]) -> String {
    let hash = sha256_bytes(data);
    let mut hex = String::with_capacity(64);
    for byte in &hash {
        hex.push(HEX_CHARS[(byte >> 4) as usize]);
        hex.push(HEX_CHARS[(byte & 0x0f) as usize]);
    }
    hex
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in &hash {
        hex.push(HEX_CHARS[(byte >> 4) as usize]);
        hex.push(HEX_CHARS[(byte & 0x0f) as usize]);
    }
    Ok(hex)
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

// ============================================================================
// Glob Pattern Matching
// ============================================================================

/// Matches a path component against a glob pattern.
/// Supports: * (any chars except /), ? (single char except /), ** (any path segments)
fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_match_recursive(pattern.as_bytes(), path.as_bytes())
}

/// The length in bytes of the UTF-8 character starting at `i`, or 1 if the byte
/// there does not begin a well-formed sequence.
///
/// Matching runs over bytes, which is right: our paths are byte strings and are
/// not required to be valid UTF-8. But `?` is documented as "single char except
/// /", and a character is not a byte. Advancing one byte per `?` meant
/// `?.txt` did not match `日.txt` — the `?` ate a third of the kanji and the
/// literal `.` was then compared against a continuation byte. `?` has no
/// backtracking (only `*` does), so the match simply failed.
///
/// In a backup tool that is not cosmetic. These patterns drive include and
/// exclude lists, so a pattern that silently fails to match either backs up a
/// file the user meant to exclude or, worse, skips one they believed was
/// covered — and they find out when they try to restore it.
///
/// Only `?` needs this. `*` is byte-greedy but can only *succeed* on a
/// character boundary, because UTF-8 is self-synchronising: the literal that
/// follows a `*` comes from a `&str`, so it is well-formed, and a well-formed
/// sequence can never match starting inside another one. `/` is likewise safe
/// to test byte-wise, as an ASCII byte cannot occur inside a multi-byte
/// character.
fn utf8_char_len(text: &[u8], i: usize) -> usize {
    let Some(&b) = text.get(i) else {
        return 1;
    };
    let want = if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        // A continuation byte or an invalid lead: not a character start, so
        // consume one byte and let the literal comparison decide.
        return 1;
    };
    // Only treat this as a multi-byte character if the sequence it announces is
    // actually there and well-formed. Clamping to what remains instead would be
    // wrong in a way that matters: for the bytes `[0xE6, b'/']` a lead byte
    // claiming three bytes would consume both, and `?` would have crossed a
    // separator — the one thing it must never do. Falling back to a single byte
    // for an ill-formed sequence keeps that invariant and still guarantees
    // forward progress.
    let follows_are_continuations = (1..want).all(|k| {
        text.get(i.saturating_add(k))
            .is_some_and(|&c| (0x80..=0xBF).contains(&c))
    });
    if follows_are_continuations { want } else { 1 }
}

fn glob_match_recursive(pattern: &[u8], text: &[u8]) -> bool {
    // Check for ** at the start — matches any number of path segments
    if pattern.len() >= 2 && pattern[0] == b'*' && pattern[1] == b'*' {
        let rest = if pattern.len() > 2 && pattern[2] == b'/' {
            &pattern[3..]
        } else if pattern.len() == 2 {
            &pattern[2..]
        } else {
            // "**" followed by something other than "/" — treat as literal
            return glob_match_simple(pattern, text);
        };

        // "**" matches zero or more path segments
        if rest.is_empty() {
            return true;
        }

        // Try matching rest against every suffix of text starting at path boundaries
        for i in 0..=text.len() {
            if (i == 0 || (i > 0 && text[i - 1] == b'/')) && glob_match_recursive(rest, &text[i..])
            {
                return true;
            }
        }
        // Also try without consuming any leading slash
        return glob_match_recursive(rest, text);
    }

    glob_match_simple(pattern, text)
}

fn glob_match_simple(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'?' && text[ti] != b'/' {
            pi += 1;
            // One character, not one byte — see `utf8_char_len`.
            ti += utf8_char_len(text, ti);
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            // Handle ** in the middle of a pattern
            if pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
                // Delegate to recursive handler for **
                return glob_match_recursive(&pattern[pi..], &text[ti..]);
            }
            // Single * — match anything except /
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            // Backtrack to last star
            star_ti += 1;
            if star_ti > text.len() || text[star_ti - 1] == b'/' {
                return false; // * doesn't cross /
            }
            ti = star_ti;
            pi = star_pi + 1;
        } else {
            return false;
        }
    }

    // Consume trailing stars
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Check if a path should be excluded based on exclude patterns.
fn is_excluded(path: &Path, patterns: &[String]) -> bool {
    // Exclusion patterns are UTF-8 text the user typed, so matching against a
    // lossy rendering is the only thing that can be meant. This is a selection
    // heuristic, not an identity check: the ASCII structure a glob keys on
    // (separators, extensions) survives the conversion exactly, and the
    // undecodable bytes a pattern could never have named become U+FFFD, which
    // no pattern contains.
    let path = path.to_string_lossy();
    let path = path.as_ref();
    for pattern in patterns {
        if glob_matches(pattern, path) {
            return true;
        }
        // Also check just the filename component
        if let Some(name) = path.rsplit('/').next()
            && glob_matches(pattern, name)
        {
            return true;
        }
    }
    false
}

// ============================================================================
// JSON Serialization/Deserialization (hand-written, no external crate)
// ============================================================================

/// Simple JSON value type for serialization/deserialization.
#[derive(Clone, Debug, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) => Some(*n as u64),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&Vec<(String, JsonValue)>> {
        match self {
            JsonValue::Object(o) => Some(o),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(entries) => {
                for (k, v) in entries {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            JsonValue::Number(n) => {
                if *n == (*n as u64) as f64 && *n >= 0.0 {
                    write!(f, "{}", *n as u64)
                } else {
                    write!(f, "{}", n)
                }
            }
            JsonValue::Str(s) => {
                write!(f, "\"")?;
                for ch in s.chars() {
                    match ch {
                        '"' => write!(f, "\\\"")?,
                        '\\' => write!(f, "\\\\")?,
                        '\n' => write!(f, "\\n")?,
                        '\r' => write!(f, "\\r")?,
                        '\t' => write!(f, "\\t")?,
                        c if c < '\x20' => write!(f, "\\u{:04x}", c as u32)?,
                        c => write!(f, "{}", c)?,
                    }
                }
                write!(f, "\"")
            }
            JsonValue::Array(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            JsonValue::Object(entries) => {
                write!(f, "{{")?;
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "\"{}\":{}", key, val)?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// Pretty-print JSON with indentation.
fn json_pretty(value: &JsonValue, indent: usize) -> String {
    let mut out = String::new();
    json_pretty_inner(value, indent, 0, &mut out);
    out
}

fn json_pretty_inner(value: &JsonValue, indent: usize, depth: usize, out: &mut String) {
    let prefix = " ".repeat(indent * depth);
    let inner_prefix = " ".repeat(indent * (depth + 1));

    match value {
        JsonValue::Object(entries) if !entries.is_empty() => {
            out.push_str("{\n");
            for (i, (key, val)) in entries.iter().enumerate() {
                out.push_str(&inner_prefix);
                out.push('"');
                out.push_str(key);
                out.push_str("\": ");
                json_pretty_inner(val, indent, depth + 1, out);
                if i + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&prefix);
            out.push('}');
        }
        JsonValue::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&inner_prefix);
                json_pretty_inner(item, indent, depth + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&prefix);
            out.push(']');
        }
        _ => {
            out.push_str(&format!("{}", value));
        }
    }
}

/// Parse a JSON string into a JsonValue.
fn json_parse(input: &str) -> Result<JsonValue, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty input".to_string());
    }
    let (val, rest) = parse_value(trimmed)?;
    if !rest.trim().is_empty() {
        return Err(format!(
            "trailing characters: {:?}",
            &rest[..rest.len().min(20)]
        ));
    }
    Ok(val)
}

fn parse_value(input: &str) -> Result<(JsonValue, &str), String> {
    let s = input.trim_start();
    if s.is_empty() {
        return Err("unexpected end of input".to_string());
    }
    match s.as_bytes()[0] {
        b'"' => parse_string(s),
        b'{' => parse_object(s),
        b'[' => parse_array(s),
        b't' | b'f' => parse_bool(s),
        b'n' => parse_null(s),
        b'-' | b'0'..=b'9' => parse_number(s),
        c => Err(format!("unexpected character: {}", c as char)),
    }
}

/// Parse a JSON string literal, returning the decoded value and the remaining
/// input.
///
/// Literal stretches are copied out as whole `&str` slices rather than a byte
/// at a time. This matters more here than in most JSON readers: the strings in
/// a manifest are **file paths**, and pushing `byte as char` reads each UTF-8
/// byte as the Latin-1 scalar of that value — so a manifest listing
/// `写真/2024.jpg` read back as a mojibake path that no longer names any file,
/// and restore and verify both reported it missing.
fn parse_string(input: &str) -> Result<(JsonValue, &str), String> {
    if !input.starts_with('"') {
        return Err("expected '\"'".to_string());
    }
    let bytes = input.as_bytes();
    let mut result = String::new();
    let mut i = 1;
    // Start of the current run of literal (unescaped) text.
    let mut run_start = i;
    while let Some(&b) = bytes.get(i) {
        // `"` and `\` are ASCII, and an ASCII byte can never occur inside a
        // multi-byte UTF-8 sequence, so `i` is always on a character boundary
        // where a run is cut.
        if b == b'"' {
            result.push_str(input.get(run_start..i).ok_or("string cut mid-character")?);
            let rest = input
                .get(i.saturating_add(1)..)
                .ok_or("string cut mid-character")?;
            return Ok((JsonValue::Str(result), rest));
        }
        if b != b'\\' {
            i = i.saturating_add(1);
            continue;
        }
        result.push_str(input.get(run_start..i).ok_or("string cut mid-character")?);
        let after = i.saturating_add(1);
        // Take a whole character: an unknown escape may be followed by a
        // multi-byte one, and consuming a single byte of it would both corrupt
        // it and strand the scan inside a UTF-8 sequence.
        let esc = input
            .get(after..)
            .and_then(|s| s.chars().next())
            .ok_or("unexpected end in string escape")?;
        let mut next = after.saturating_add(esc.len_utf8());
        match esc {
            '"' => result.push('"'),
            '\\' => result.push('\\'),
            '/' => result.push('/'),
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            'b' => result.push('\u{08}'),
            'f' => result.push('\u{0c}'),
            'u' => {
                let (c, after_escape) = parse_unicode_escape(input, next)?;
                result.push(c);
                next = after_escape;
            }
            other => {
                // Unknown escape: keep it verbatim rather than silently
                // dropping the backslash out of a path.
                result.push('\\');
                result.push(other);
            }
        }
        i = next;
        run_start = i;
    }
    Err("unterminated string".to_string())
}

/// Decode a `\u` escape whose four hex digits begin at `start` (just past the
/// `u`), returning the character and the offset just past the escape.
///
/// A leading surrogate is combined with a following `\uXXXX` trailing
/// surrogate, which is how JSON spells anything outside the BMP.
fn parse_unicode_escape(input: &str, start: usize) -> Result<(char, usize), String> {
    let (hi, after_hi) = parse_hex4(input, start)?;
    let bytes = input.as_bytes();
    if (0xD800..0xDC00).contains(&hi)
        && bytes.get(after_hi).copied() == Some(b'\\')
        && bytes.get(after_hi.saturating_add(1)).copied() == Some(b'u')
        && let Ok((lo, after_lo)) = parse_hex4(input, after_hi.saturating_add(2))
        && (0xDC00..0xE000).contains(&lo)
        // Bounded by the two range checks above: at most 0x10000 + 0xFFC00 +
        // 0x3FF = 0x10FFFF, so neither the shift nor the sums can overflow.
        && let Some(c) =
            char::from_u32(0x1_0000 + (hi.saturating_sub(0xD800) << 10) + lo.saturating_sub(0xDC00))
    {
        return Ok((c, after_lo));
    }
    // A lone surrogate has no scalar value. The old code dropped it silently,
    // so an escaped emoji in a path simply vanished from the manifest; U+FFFD
    // at least leaves the loss visible.
    Ok((char::from_u32(hi).unwrap_or('\u{FFFD}'), after_hi))
}

/// Read exactly four ASCII hex digits at `start`.
fn parse_hex4(input: &str, start: usize) -> Result<(u32, usize), String> {
    let end = start.saturating_add(4);
    // `get`, not a slice: the old code sliced these four bytes blindly, so a
    // `\u` followed by multi-byte text (`"\u日本"` cuts at byte 7, inside 本)
    // panicked while merely reading a manifest off disk.
    let hex = input.get(start..end).ok_or("incomplete unicode escape")?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid unicode escape".to_string());
    }
    u32::from_str_radix(hex, 16)
        .map(|v| (v, end))
        .map_err(|_| "invalid unicode escape".to_string())
}

fn parse_object(input: &str) -> Result<(JsonValue, &str), String> {
    let mut s = &input[1..]; // skip '{'
    let mut entries = Vec::new();

    s = s.trim_start();
    if let Some(rest) = s.strip_prefix('}') {
        return Ok((JsonValue::Object(entries), rest));
    }

    loop {
        s = s.trim_start();
        let (key_val, rest) = parse_string(s)?;
        let key = match key_val {
            JsonValue::Str(k) => k,
            _ => return Err("object key must be string".to_string()),
        };
        s = rest.trim_start();
        let Some(after_colon) = s.strip_prefix(':') else {
            return Err("expected ':'".to_string());
        };
        s = after_colon;
        let (val, rest) = parse_value(s)?;
        entries.push((key, val));
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix('}') {
            return Ok((JsonValue::Object(entries), rest));
        }
        let Some(after_comma) = s.strip_prefix(',') else {
            return Err("expected ',' or '}'".to_string());
        };
        s = after_comma;
    }
}

fn parse_array(input: &str) -> Result<(JsonValue, &str), String> {
    let mut s = &input[1..]; // skip '['
    let mut items = Vec::new();

    s = s.trim_start();
    if let Some(rest) = s.strip_prefix(']') {
        return Ok((JsonValue::Array(items), rest));
    }

    loop {
        let (val, rest) = parse_value(s)?;
        items.push(val);
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix(']') {
            return Ok((JsonValue::Array(items), rest));
        }
        let Some(after_comma) = s.strip_prefix(',') else {
            return Err("expected ',' or ']'".to_string());
        };
        s = after_comma;
    }
}

fn parse_bool(input: &str) -> Result<(JsonValue, &str), String> {
    if let Some(rest) = input.strip_prefix("true") {
        Ok((JsonValue::Bool(true), rest))
    } else if let Some(rest) = input.strip_prefix("false") {
        Ok((JsonValue::Bool(false), rest))
    } else {
        Err("expected 'true' or 'false'".to_string())
    }
}

fn parse_null(input: &str) -> Result<(JsonValue, &str), String> {
    if let Some(rest) = input.strip_prefix("null") {
        Ok((JsonValue::Null, rest))
    } else {
        Err("expected 'null'".to_string())
    }
}

fn parse_number(input: &str) -> Result<(JsonValue, &str), String> {
    let mut end = 0;
    let bytes = input.as_bytes();
    if end < bytes.len() && bytes[end] == b'-' {
        end += 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        end += 1;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    let num_str = &input[..end];
    let num: f64 = num_str
        .parse()
        .map_err(|_| format!("invalid number: {}", num_str))?;
    Ok((JsonValue::Number(num), &input[end..]))
}

// ============================================================================
// Data Structures
// ============================================================================

/// Type of backup.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BackupType {
    Full,
    Incremental,
    Differential,
}

impl BackupType {
    fn as_str(&self) -> &'static str {
        match self {
            BackupType::Full => "full",
            BackupType::Incremental => "incremental",
            BackupType::Differential => "differential",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "full" => Some(BackupType::Full),
            "incremental" => Some(BackupType::Incremental),
            "differential" => Some(BackupType::Differential),
            _ => None,
        }
    }
}

impl fmt::Display for BackupType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Manifest format version written into every new manifest.
///
/// Version 1 stored paths as plain JSON strings, which meant they had already
/// been through `to_string_lossy` and could not name a non-UTF-8 file. Version 2
/// stores them percent-encoded (see [`encode_path`]). Absence of the field means
/// version 1, so an existing archive still restores.
const MANIFEST_VERSION: u64 = 2;

/// Metadata about a single file in a backup manifest.
#[derive(Clone, Debug)]
struct FileEntry {
    /// Relative path from backup source root.
    ///
    /// A `PathBuf`, not a `String`: paths on this OS may contain any byte
    /// except `/` and NUL, and a backup that cannot record the name of a file
    /// it copied cannot restore it either.
    path: PathBuf,
    /// File size in bytes.
    size: u64,
    /// Modification time as seconds since UNIX epoch.
    mtime: u64,
    /// SHA-256 hash of file contents.
    hash: String,
    /// Whether this is a symlink.
    is_symlink: bool,
    /// Symlink target (if is_symlink). Also an arbitrary byte string.
    link_target: Option<PathBuf>,
}

impl FileEntry {
    fn to_json(&self) -> JsonValue {
        let mut entries = vec![
            ("path".to_string(), JsonValue::Str(encode_path(&self.path))),
            ("size".to_string(), JsonValue::Number(self.size as f64)),
            ("mtime".to_string(), JsonValue::Number(self.mtime as f64)),
            ("hash".to_string(), JsonValue::Str(self.hash.clone())),
            ("is_symlink".to_string(), JsonValue::Bool(self.is_symlink)),
        ];
        if let Some(ref target) = self.link_target {
            entries.push((
                "link_target".to_string(),
                JsonValue::Str(encode_path(target)),
            ));
        }
        JsonValue::Object(entries)
    }

    /// Read one entry. `encoded` selects the path representation: version 2
    /// manifests percent-encode, version 1 stored the path verbatim.
    fn from_json(val: &JsonValue, encoded: bool) -> Option<Self> {
        let read_path = |s: &str| {
            if encoded {
                decode_path(s)
            } else {
                PathBuf::from(s)
            }
        };
        let path = read_path(val.get("path")?.as_str()?);
        let size = val.get("size")?.as_u64()?;
        let mtime = val.get("mtime")?.as_u64()?;
        let hash = val.get("hash")?.as_str()?.to_string();
        let is_symlink = val
            .get("is_symlink")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let link_target = val
            .get("link_target")
            .and_then(|v| v.as_str())
            .map(&read_path);
        Some(FileEntry {
            path,
            size,
            mtime,
            hash,
            is_symlink,
            link_target,
        })
    }
}

/// Escape a path into printable ASCII, losslessly.
///
/// The manifest is JSON, whose strings are Unicode, but a path is a byte
/// string. `to_string_lossy` would substitute U+FFFD for every undecodable
/// byte — and since the manifest is the only record of what a backup contains,
/// that byte is then gone: restore recreates the file under a different name
/// and verify reports the original as missing. `OsStr::as_encoded_bytes` gives
/// the exact bytes; anything outside printable ASCII, plus `%` itself, is
/// percent-encoded.
fn encode_path(path: &Path) -> String {
    encode_bytes(path.as_os_str().as_encoded_bytes())
}

/// The lossless core of [`encode_path`], on bytes rather than a path.
///
/// Kept separate because this — not the `OsStr` conversion around it — is where
/// the round-trip property lives, and it can be tested on any host.
fn encode_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'%' || !(0x20..0x7f).contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char); // guarded: printable ASCII only
        }
    }
    out
}

/// Reverse of [`encode_path`].
fn decode_path(encoded: &str) -> PathBuf {
    PathBuf::from(os_string_from_bytes(decode_bytes(encoded)))
}

/// Reverse of [`encode_bytes`].
///
/// A `%` not followed by two hex digits is passed through literally rather than
/// dropped: this reads files that may have been hand-edited, and losing a byte
/// silently is worse than keeping one that was never an escape.
fn decode_bytes(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let Some(hex) = encoded.get(i.saturating_add(1)..i.saturating_add(3))
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i = i.saturating_add(3);
            continue;
        }
        out.push(b);
        i = i.saturating_add(1);
    }
    out
}

/// Build an `OsString` from the raw bytes of a path.
///
/// This is where the byte world meets the platform's path type, so it is split
/// per platform rather than papered over with
/// `OsStr::from_encoded_bytes_unchecked`: that function's contract is that the
/// bytes are valid for the platform's `OsStr` encoding, which is true for
/// arbitrary bytes on Unix but *not* on Windows, where `OsStr` is WTF-8. Since
/// our target is `target-family = ["unix"]`, the safe, total conversion below
/// is the one that actually runs; Windows appears only as a test host.
#[cfg(unix)]
fn os_string_from_bytes(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}

/// Test-host fallback. Windows `OsString` cannot hold a byte string that is not
/// WTF-8, so bytes that are not valid UTF-8 cannot survive here. They are not
/// silently mangled: [`decode_bytes`] is still exact, and the tests assert the
/// round-trip at that level, which is the level the manifest is written at.
#[cfg(not(unix))]
fn os_string_from_bytes(bytes: Vec<u8>) -> OsString {
    match String::from_utf8(bytes) {
        Ok(s) => OsString::from(s),
        // Reachable only on a non-Unix host reading a manifest written on the
        // target. Nothing better is representable; `decode_bytes` is the API to
        // use if the exact bytes are needed.
        Err(e) => OsString::from(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

/// Manifest for a single backup — lists all files included.
#[derive(Clone, Debug)]
struct Manifest {
    files: Vec<FileEntry>,
}

impl Manifest {
    fn new() -> Self {
        Self { files: Vec::new() }
    }

    fn to_json(&self) -> JsonValue {
        JsonValue::Object(vec![
            (
                "version".to_string(),
                JsonValue::Number(MANIFEST_VERSION as f64),
            ),
            (
                "files".to_string(),
                JsonValue::Array(self.files.iter().map(|f| f.to_json()).collect()),
            ),
        ])
    }

    fn from_json(val: &JsonValue) -> Option<Self> {
        // A manifest with no version field predates path escaping and stored
        // paths verbatim. Reading it is still worth doing: refusing would
        // strand every backup taken before this change.
        let version = val.get("version").and_then(|v| v.as_u64()).unwrap_or(1);
        let encoded = version >= 2;
        let files_val = val.get("files")?.as_array()?;
        let mut files = Vec::new();
        for fv in files_val {
            files.push(FileEntry::from_json(fv, encoded)?);
        }
        Some(Manifest { files })
    }

    fn serialize(&self) -> String {
        json_pretty(&self.to_json(), 2)
    }

    fn deserialize(input: &str) -> Result<Self, String> {
        let val = json_parse(input)?;
        Self::from_json(&val).ok_or_else(|| "invalid manifest structure".to_string())
    }
}

/// Metadata about a backup.
#[derive(Clone, Debug)]
struct BackupMeta {
    /// Unique backup ID (timestamp-based).
    id: String,
    /// Backup type.
    backup_type: BackupType,
    /// Unix timestamp when backup was created.
    timestamp: u64,
    /// Source path that was backed up.
    source: String,
    /// Parent backup ID (for incremental/differential).
    parent_id: Option<String>,
    /// Number of files in backup.
    file_count: u64,
    /// Total size of all files.
    total_size: u64,
    /// Number of new blobs stored (non-deduplicated).
    new_blobs: u64,
    /// Number of blobs reused via deduplication.
    dedup_blobs: u64,
}

impl BackupMeta {
    fn to_json(&self) -> JsonValue {
        let mut entries = vec![
            ("id".to_string(), JsonValue::Str(self.id.clone())),
            (
                "backup_type".to_string(),
                JsonValue::Str(self.backup_type.as_str().to_string()),
            ),
            (
                "timestamp".to_string(),
                JsonValue::Number(self.timestamp as f64),
            ),
            ("source".to_string(), JsonValue::Str(self.source.clone())),
            (
                "file_count".to_string(),
                JsonValue::Number(self.file_count as f64),
            ),
            (
                "total_size".to_string(),
                JsonValue::Number(self.total_size as f64),
            ),
            (
                "new_blobs".to_string(),
                JsonValue::Number(self.new_blobs as f64),
            ),
            (
                "dedup_blobs".to_string(),
                JsonValue::Number(self.dedup_blobs as f64),
            ),
        ];
        if let Some(ref pid) = self.parent_id {
            entries.push(("parent_id".to_string(), JsonValue::Str(pid.clone())));
        } else {
            entries.push(("parent_id".to_string(), JsonValue::Null));
        }
        JsonValue::Object(entries)
    }

    fn from_json(val: &JsonValue) -> Option<Self> {
        let id = val.get("id")?.as_str()?.to_string();
        let backup_type_str = val.get("backup_type")?.as_str()?;
        let backup_type = BackupType::from_str(backup_type_str)?;
        let timestamp = val.get("timestamp")?.as_u64()?;
        let source = val.get("source")?.as_str()?.to_string();
        let parent_id = val
            .get("parent_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let file_count = val.get("file_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let total_size = val.get("total_size").and_then(|v| v.as_u64()).unwrap_or(0);
        let new_blobs = val.get("new_blobs").and_then(|v| v.as_u64()).unwrap_or(0);
        let dedup_blobs = val.get("dedup_blobs").and_then(|v| v.as_u64()).unwrap_or(0);
        Some(BackupMeta {
            id,
            backup_type,
            timestamp,
            source,
            parent_id,
            file_count,
            total_size,
            new_blobs,
            dedup_blobs,
        })
    }

    fn serialize(&self) -> String {
        json_pretty(&self.to_json(), 2)
    }

    fn deserialize(input: &str) -> Result<Self, String> {
        let val = json_parse(input)?;
        Self::from_json(&val).ok_or_else(|| "invalid backup meta structure".to_string())
    }
}

/// Schedule entry for automated backups.
#[derive(Clone, Debug)]
struct ScheduleEntry {
    source: String,
    dest: String,
    interval: String,
}

impl ScheduleEntry {
    fn to_json(&self) -> JsonValue {
        JsonValue::Object(vec![
            ("source".to_string(), JsonValue::Str(self.source.clone())),
            ("dest".to_string(), JsonValue::Str(self.dest.clone())),
            (
                "interval".to_string(),
                JsonValue::Str(self.interval.clone()),
            ),
        ])
    }

    fn from_json(val: &JsonValue) -> Option<Self> {
        let source = val.get("source")?.as_str()?.to_string();
        let dest = val.get("dest")?.as_str()?.to_string();
        let interval = val.get("interval")?.as_str()?.to_string();
        Some(ScheduleEntry {
            source,
            dest,
            interval,
        })
    }
}

/// Result of comparing two manifests.
#[derive(Debug)]
struct DiffResult {
    added: Vec<FileEntry>,
    modified: Vec<(FileEntry, FileEntry)>, // (old, new)
    deleted: Vec<FileEntry>,
}

impl DiffResult {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

/// Progress tracker for long-running operations.
struct Progress {
    total_files: u64,
    processed_files: u64,
    total_bytes: u64,
    processed_bytes: u64,
    current_file: String,
}

impl Progress {
    fn new() -> Self {
        Self {
            total_files: 0,
            processed_files: 0,
            total_bytes: 0,
            processed_bytes: 0,
            current_file: String::new(),
        }
    }

    fn report(&self) {
        let file_pct = self
            .processed_files
            .checked_mul(100)
            .and_then(|n| n.checked_div(self.total_files))
            .unwrap_or(0);
        let byte_pct = self
            .processed_bytes
            .checked_mul(100)
            .and_then(|n| n.checked_div(self.total_bytes))
            .unwrap_or(0);
        eprintln!(
            "  [{}/{}] files ({}%) | [{}/{}] bytes ({}%) | {}",
            self.processed_files,
            self.total_files,
            file_pct,
            format_size(self.processed_bytes),
            format_size(self.total_bytes),
            byte_pct,
            self.current_file,
        );
    }
}

// ============================================================================
// Path Utilities
// ============================================================================

/// Normalize a path to use forward slashes and remove redundant components.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    if path.starts_with('/') {
        format!("/{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

/// Get relative path of `full` with respect to `base`.
///
/// Byte-exact. The previous implementation went through `to_string_lossy`,
/// which is where a non-UTF-8 filename was destroyed — before the manifest
/// writer ever saw it, so fixing the manifest parser alone was not enough.
///
/// Components are rejoined with `/` rather than by `PathBuf::push` so the
/// manifest records the same text regardless of the host's separator. `/` is
/// ASCII and cannot occur inside a component's encoding, so the join is
/// reversible.
///
/// The root and prefix components are handled separately because their
/// `as_os_str` is already a separator (`/`, or `\` on Windows): joining them
/// like a normal component yields a doubled or mixed separator.
fn relative_path(full: &Path, base: &Path) -> PathBuf {
    let rel = full.strip_prefix(base).unwrap_or(full);
    let mut out: Vec<u8> = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Prefix(prefix) => {
                out.extend_from_slice(prefix.as_os_str().as_encoded_bytes())
            }
            Component::RootDir => out.push(b'/'),
            _ => {
                if !out.is_empty() && !out.ends_with(b"/") {
                    out.push(b'/');
                }
                out.extend_from_slice(comp.as_os_str().as_encoded_bytes());
            }
        }
    }
    PathBuf::from(os_string_from_bytes(out))
}

/// Format byte size as human-readable string.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Get current unix timestamp.
fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Format a unix timestamp as a human-readable date string.
fn format_timestamp(ts: u64) -> String {
    // Simple formatting: YYYY-MM-DD HH:MM:SS (approximate, not accounting for
    // leap seconds perfectly but good enough for display purposes)
    let secs_per_day: u64 = 86400;
    let days = ts / secs_per_day;
    let time_of_day = ts % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to Y/M/D (simplified Gregorian calculation)
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

// ============================================================================
// Content-Addressed Store
// ============================================================================

/// The content-addressed store manages blobs keyed by SHA-256 hash.
struct ContentStore {
    base_path: PathBuf,
}

impl ContentStore {
    fn new(dest: &Path) -> Self {
        let base_path = dest.join("cas");
        Self { base_path }
    }

    /// Get the filesystem path for a blob by its hash.
    fn blob_path(&self, hash: &str) -> PathBuf {
        // Use first 2 chars as directory prefix for filesystem efficiency
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.base_path.join(prefix).join(rest)
    }

    /// Check if a blob exists in the store.
    ///
    /// Existence *is* the deduplication test — there is no hash verification
    /// on this path — so it is only sound if a blob can never appear at its
    /// final name in a partial state. That is why both store methods below go
    /// through `safeio`: a blob is built under a temporary name and renamed
    /// into place, so a blob that exists is a blob that is whole.
    ///
    /// Before that, an interrupted `fs::copy`/`fs::write` left a truncated
    /// blob sitting at the hash's path. Nothing ever rechecked it, so every
    /// later backup containing that content deduplicated against the damaged
    /// copy and `store_*` cheerfully reported "already have it". One
    /// interrupted write silently poisoned that content for every future
    /// backup — in the one application whose entire purpose is that the data
    /// survives.
    fn has_blob(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    /// Store a file in the CAS. Returns true if the blob was new (not deduplicated).
    fn store_file(&self, source_path: &Path, hash: &str) -> io::Result<bool> {
        if self.has_blob(hash) {
            return Ok(false); // Already exists — deduplicated
        }

        let blob_path = self.blob_path(hash);
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        safeio::copy_atomically(source_path, &blob_path)?;
        Ok(true)
    }

    /// Store raw bytes in the CAS.
    fn store_bytes(&self, data: &[u8], hash: &str) -> io::Result<bool> {
        if self.has_blob(hash) {
            return Ok(false);
        }

        let blob_path = self.blob_path(hash);
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        safeio::write_atomically(&blob_path, data)?;
        Ok(true)
    }

    /// Retrieve a blob's contents.
    fn read_blob(&self, hash: &str) -> io::Result<Vec<u8>> {
        fs::read(self.blob_path(hash))
    }

    /// Verify a blob's integrity.
    fn verify_blob(&self, hash: &str) -> io::Result<bool> {
        let data = self.read_blob(hash)?;
        let actual = sha256_hex(&data);
        Ok(actual == hash)
    }

    /// Remove a blob from the store.
    fn remove_blob(&self, hash: &str) -> io::Result<()> {
        let path = self.blob_path(hash);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Get all blob hashes currently in the store.
    fn all_blobs(&self) -> io::Result<Vec<String>> {
        let mut blobs = Vec::new();
        if !self.base_path.exists() {
            return Ok(blobs);
        }
        for prefix_entry in fs::read_dir(&self.base_path)? {
            let prefix_entry = prefix_entry?;
            if !prefix_entry.file_type()?.is_dir() {
                continue;
            }
            let prefix = prefix_entry.file_name().to_string_lossy().to_string();
            for blob_entry in fs::read_dir(prefix_entry.path())? {
                let blob_entry = blob_entry?;
                let rest = blob_entry.file_name().to_string_lossy().to_string();
                blobs.push(format!("{}{}", prefix, rest));
            }
        }
        Ok(blobs)
    }
}

// ============================================================================
// File Scanner
// ============================================================================

/// Scan a directory tree and collect file entries.
fn scan_directory(
    source: &Path,
    exclude_patterns: &[String],
    follow_symlinks: bool,
    progress: &mut Progress,
) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    scan_dir_recursive(
        source,
        source,
        exclude_patterns,
        follow_symlinks,
        &mut entries,
        progress,
    )?;
    Ok(entries)
}

fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    excludes: &[String],
    follow_symlinks: bool,
    entries: &mut Vec<FileEntry>,
    progress: &mut Progress,
) -> io::Result<()> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("warning: cannot read directory {:?}: {}", dir, e);
            return Ok(());
        }
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("warning: directory entry error: {}", e);
                continue;
            }
        };

        let path = entry.path();
        let rel = relative_path(&path, root);

        // Check exclusions
        if is_excluded(&rel, excludes) {
            continue;
        }

        let meta = if follow_symlinks {
            match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("warning: cannot stat {:?}: {}", path, e);
                    continue;
                }
            }
        } else {
            match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("warning: cannot stat {:?}: {}", path, e);
                    continue;
                }
            }
        };

        if meta.is_dir() {
            scan_dir_recursive(root, &path, excludes, follow_symlinks, entries, progress)?;
        } else if meta.is_file() {
            let mtime = meta
                .modified()
                .unwrap_or(UNIX_EPOCH)
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let hash = match sha256_file(&path) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("warning: cannot hash {:?}: {}", path, e);
                    continue;
                }
            };

            progress.processed_files += 1;
            progress.processed_bytes += meta.len();
            progress.current_file = rel.display().to_string();

            // Report progress every 100 files
            if progress.processed_files.is_multiple_of(100) {
                progress.report();
            }

            entries.push(FileEntry {
                path: rel,
                size: meta.len(),
                mtime,
                hash,
                is_symlink: false,
                link_target: None,
            });
        } else if meta.file_type().is_symlink() {
            // A symlink target is an arbitrary byte string too, and it is
            // what restore recreates, so it must not be flattened either.
            let target = fs::read_link(&path).unwrap_or_default();
            let hash = sha256_hex(target.as_os_str().as_encoded_bytes());

            entries.push(FileEntry {
                path: rel,
                size: 0,
                mtime: 0,
                hash,
                is_symlink: true,
                link_target: Some(target),
            });
        }
    }

    Ok(())
}

/// Pre-scan to estimate total files and bytes (for progress reporting).
fn estimate_scan(source: &Path, excludes: &[String]) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    estimate_recursive(source, source, excludes, &mut files, &mut bytes);
    (files, bytes)
}

fn estimate_recursive(
    root: &Path,
    dir: &Path,
    excludes: &[String],
    files: &mut u64,
    bytes: &mut u64,
) {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let rel = relative_path(&path, root);
        if is_excluded(&rel, excludes) {
            continue;
        }
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if meta.is_dir() {
                estimate_recursive(root, &path, excludes, files, bytes);
            } else if meta.is_file() {
                *files += 1;
                *bytes += meta.len();
            }
        }
    }
}

// ============================================================================
// Change Detection
// ============================================================================

/// Compare current scan against a previous manifest to detect changes.
fn detect_changes(current: &[FileEntry], previous: &Manifest) -> DiffResult {
    let prev_map: BTreeMap<&Path, &FileEntry> = previous
        .files
        .iter()
        .map(|f| (f.path.as_path(), f))
        .collect();

    let curr_map: BTreeMap<&Path, &FileEntry> =
        current.iter().map(|f| (f.path.as_path(), f)).collect();

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    // A path present now but not before is an addition; a path present in both
    // whose content hash differs is a modification. Size and mtime are not
    // consulted: the hash is authoritative, and a file can be rewritten with
    // identical size and a preserved mtime.
    for entry in current {
        match prev_map.get(entry.path.as_path()) {
            None => added.push(entry.clone()),
            Some(prev_entry) => {
                if entry.hash != prev_entry.hash {
                    modified.push(((*prev_entry).clone(), entry.clone()));
                }
            }
        }
    }

    // Find deleted files
    for prev_entry in &previous.files {
        if !curr_map.contains_key(prev_entry.path.as_path()) {
            deleted.push(prev_entry.clone());
        }
    }

    DiffResult {
        added,
        modified,
        deleted,
    }
}

// ============================================================================
// Backup Repository Operations
// ============================================================================

/// Get the path to the backups directory within the destination.
fn backups_dir(dest: &Path) -> PathBuf {
    dest.join("backups")
}

/// Get the path to the schedules file.
fn schedules_path(dest: &Path) -> PathBuf {
    dest.join("schedules.json")
}

/// List all backup metadata in the destination, sorted by timestamp.
fn list_backups(dest: &Path) -> io::Result<Vec<BackupMeta>> {
    let bdir = backups_dir(dest);
    if !bdir.exists() {
        return Ok(Vec::new());
    }

    let mut metas = Vec::new();
    for entry in fs::read_dir(&bdir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let meta_path = entry.path().join("meta.json");
        if meta_path.exists() {
            let content = fs::read_to_string(&meta_path)?;
            if let Ok(meta) = BackupMeta::deserialize(&content) {
                metas.push(meta);
            }
        }
    }

    metas.sort_by_key(|m| m.timestamp);
    Ok(metas)
}

/// Find the most recent backup of a given type (or any type if None).
fn find_latest_backup(
    dest: &Path,
    backup_type: Option<BackupType>,
) -> io::Result<Option<BackupMeta>> {
    let metas = list_backups(dest)?;
    Ok(metas
        .into_iter()
        .rev()
        .find(|m| backup_type.is_none() || Some(m.backup_type) == backup_type))
}

/// Find the most recent full backup.
fn find_latest_full_backup(dest: &Path) -> io::Result<Option<BackupMeta>> {
    find_latest_backup(dest, Some(BackupType::Full))
}

/// Load a manifest for a given backup ID.
fn load_manifest(dest: &Path, backup_id: &str) -> io::Result<Manifest> {
    let manifest_path = backups_dir(dest).join(backup_id).join("manifest.json");
    let content = fs::read_to_string(&manifest_path)?;
    Manifest::deserialize(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Load metadata for a given backup ID.
fn load_meta(dest: &Path, backup_id: &str) -> io::Result<BackupMeta> {
    let meta_path = backups_dir(dest).join(backup_id).join("meta.json");
    let content = fs::read_to_string(&meta_path)?;
    BackupMeta::deserialize(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// ============================================================================
// Command: create
// ============================================================================

struct CreateOptions {
    backup_type: BackupType,
    source: PathBuf,
    dest: PathBuf,
    exclude: Vec<String>,
    follow_symlinks: bool,
}

fn cmd_create(opts: CreateOptions) -> io::Result<()> {
    let source = opts.source.canonicalize().map_err(|e| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("source path {:?}: {}", opts.source, e),
        )
    })?;

    // Ensure destination exists
    fs::create_dir_all(&opts.dest)?;
    fs::create_dir_all(backups_dir(&opts.dest))?;

    let store = ContentStore::new(&opts.dest);

    // Determine parent backup for incremental/differential
    let parent_manifest = match opts.backup_type {
        BackupType::Full => None,
        BackupType::Incremental => {
            // Parent is the most recent backup of any type
            match find_latest_backup(&opts.dest, None)? {
                Some(meta) => Some((meta.id.clone(), load_manifest(&opts.dest, &meta.id)?)),
                None => {
                    eprintln!("No previous backup found. Performing full backup instead.");
                    None
                }
            }
        }
        BackupType::Differential => {
            // Parent is the most recent full backup
            match find_latest_full_backup(&opts.dest)? {
                Some(meta) => Some((meta.id.clone(), load_manifest(&opts.dest, &meta.id)?)),
                None => {
                    eprintln!("No previous full backup found. Performing full backup instead.");
                    None
                }
            }
        }
    };

    let effective_type = if parent_manifest.is_none() && opts.backup_type != BackupType::Full {
        BackupType::Full
    } else {
        opts.backup_type
    };

    println!(
        "Creating {} backup of {:?} -> {:?}",
        effective_type, source, opts.dest
    );

    // Estimate for progress
    eprintln!("Scanning directory tree...");
    let (est_files, est_bytes) = estimate_scan(&source, &opts.exclude);
    eprintln!(
        "  Estimated: {} files, {}",
        est_files,
        format_size(est_bytes)
    );

    let mut progress = Progress::new();
    progress.total_files = est_files;
    progress.total_bytes = est_bytes;

    // Scan source directory
    let all_files = scan_directory(&source, &opts.exclude, opts.follow_symlinks, &mut progress)?;

    // Determine which files to include based on backup type
    let files_to_backup = match (&parent_manifest, effective_type) {
        (Some((_, prev_manifest)), BackupType::Incremental | BackupType::Differential) => {
            let diff = detect_changes(&all_files, prev_manifest);
            let mut to_backup = diff.added;
            to_backup.extend(diff.modified.into_iter().map(|(_, new)| new));
            to_backup
        }
        _ => all_files.clone(),
    };

    // Store blobs and build manifest
    let mut manifest = Manifest::new();
    let mut new_blobs: u64 = 0;
    let mut dedup_blobs: u64 = 0;
    let mut total_size: u64 = 0;

    for entry in &files_to_backup {
        total_size += entry.size;

        if entry.is_symlink {
            // Symlinks don't need blob storage
            manifest.files.push(entry.clone());
            continue;
        }

        // Store blob in CAS
        let file_path = source.join(&entry.path);
        match store.store_file(&file_path, &entry.hash) {
            Ok(true) => new_blobs += 1,
            Ok(false) => dedup_blobs += 1,
            Err(e) => {
                eprintln!("warning: failed to store {:?}: {}", entry.path, e);
                continue;
            }
        }

        manifest.files.push(entry.clone());
    }

    // For incremental/differential, also include unchanged files in manifest
    // so that restore can reconstruct the full state
    if effective_type == BackupType::Full {
        // manifest already has all files
    }

    // Create backup directory
    let timestamp = now_timestamp();
    let backup_id = format!("{}-{}", timestamp, effective_type.as_str());
    let backup_dir = backups_dir(&opts.dest).join(&backup_id);
    fs::create_dir_all(&backup_dir)?;

    // Write manifest.
    //
    // The order of these two writes is load-bearing and must not be swapped:
    // `list_backups` only counts a directory as a backup if `meta.json` is
    // present *and* parses, so writing meta.json last makes it the completion
    // marker. An interrupted backup leaves a directory with no meta, which is
    // correctly ignored rather than offered to the user as restorable.
    //
    // Both go through `safeio` as well, so that neither file can exist in a
    // half-written state even momentarily. That is belt-and-braces given the
    // ordering above, and cheap enough to be worth not having the safety rest
    // on a subtle argument that a future edit could invalidate silently.
    let manifest_str = manifest.serialize();
    safeio::write_str_atomically(&backup_dir.join("manifest.json"), &manifest_str)?;

    // Write metadata
    let parent_id = parent_manifest.map(|(id, _)| id);
    let meta = BackupMeta {
        id: backup_id.clone(),
        backup_type: effective_type,
        timestamp,
        source: source.to_string_lossy().to_string(),
        parent_id,
        file_count: manifest.files.len() as u64,
        total_size,
        new_blobs,
        dedup_blobs,
    };
    let meta_str = meta.serialize();
    safeio::write_str_atomically(&backup_dir.join("meta.json"), &meta_str)?;

    println!("\nBackup complete: {}", backup_id);
    println!("  Type: {}", effective_type);
    println!("  Files: {}", manifest.files.len());
    println!("  Total size: {}", format_size(total_size));
    println!("  New blobs: {}", new_blobs);
    println!("  Deduplicated: {}", dedup_blobs);
    if dedup_blobs > 0 {
        let saved_pct = (dedup_blobs * 100) / (new_blobs + dedup_blobs);
        println!("  Dedup ratio: {}%", saved_pct);
    }

    Ok(())
}

// ============================================================================
// Command: restore
// ============================================================================

struct RestoreOptions {
    backup_id: String,
    backup_dest: PathBuf,  // Where backups are stored
    restore_dest: PathBuf, // Where to restore files to
    file_pattern: Option<String>,
}

fn cmd_restore(opts: RestoreOptions) -> io::Result<()> {
    let manifest = load_manifest(&opts.backup_dest, &opts.backup_id)?;
    let meta = load_meta(&opts.backup_dest, &opts.backup_id)?;
    let store = ContentStore::new(&opts.backup_dest);

    println!(
        "Restoring backup {} to {:?}",
        opts.backup_id, opts.restore_dest
    );
    println!("  Type: {}", meta.backup_type);
    println!("  Files in manifest: {}", manifest.files.len());

    // For incremental/differential, we need to reconstruct the full file set
    // by walking back through parent manifests
    let full_files = if meta.backup_type != BackupType::Full {
        reconstruct_full_manifest(&opts.backup_dest, &meta, &manifest)?
    } else {
        manifest.files.clone()
    };

    let files_to_restore: Vec<&FileEntry> = if let Some(ref pattern) = opts.file_pattern {
        full_files
            .iter()
            // The pattern is UTF-8 text the user typed, so matching a lossy
            // rendering of the stored path is a selection heuristic, not an
            // identity check. The path itself is still restored byte-exactly.
            .filter(|f| glob_matches(pattern, &f.path.to_string_lossy()))
            .collect()
    } else {
        full_files.iter().collect()
    };

    println!("  Restoring {} files", files_to_restore.len());

    fs::create_dir_all(&opts.restore_dest)?;

    let mut restored = 0u64;
    let mut errors = 0u64;

    for entry in &files_to_restore {
        let dest_path = opts.restore_dest.join(&entry.path);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if entry.is_symlink {
            if let Some(ref target) = entry.link_target {
                // Create symlink (platform-dependent, best-effort)
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(target, &dest_path).ok();
                }
                #[cfg(not(unix))]
                {
                    // On non-Unix, write the target path as file content
                    fs::write(&dest_path, target.as_os_str().as_encoded_bytes())?;
                }
            }
            restored += 1;
            continue;
        }

        // Read blob from CAS and write to destination
        match store.read_blob(&entry.hash) {
            Ok(data) => {
                // Verify hash
                let actual_hash = sha256_hex(&data);
                if actual_hash != entry.hash {
                    eprintln!(
                        "error: hash mismatch for {:?}: expected {}, got {}",
                        entry.path, entry.hash, actual_hash
                    );
                    errors += 1;
                    continue;
                }
                // Crash-safe: a restore usually writes *over* a file the user
                // still has. `fs::write` truncates first, so an interrupted
                // restore destroyed the existing file and failed to supply
                // the replacement — the one outcome a restore must never
                // produce.
                safeio::write_atomically(&dest_path, &data)?;
                restored += 1;
            }
            Err(e) => {
                eprintln!("error: cannot read blob for {:?}: {}", entry.path, e);
                errors += 1;
            }
        }
    }

    println!("\nRestore complete:");
    println!("  Restored: {} files", restored);
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Reconstruct the full file list by walking back through parent backups.
fn reconstruct_full_manifest(
    dest: &Path,
    meta: &BackupMeta,
    manifest: &Manifest,
) -> io::Result<Vec<FileEntry>> {
    let mut file_map: BTreeMap<PathBuf, FileEntry> = BTreeMap::new();

    // Walk back to find the full base
    let mut chain = vec![(meta.clone(), manifest.clone())];
    let mut current_meta = meta.clone();
    while let Some(ref pid) = current_meta.parent_id {
        let parent_meta = load_meta(dest, pid)?;
        let parent_manifest = load_manifest(dest, pid)?;
        chain.push((parent_meta.clone(), parent_manifest));
        if parent_meta.backup_type == BackupType::Full {
            break;
        }
        current_meta = parent_meta;
    }

    // Apply from oldest (full) to newest (incremental)
    for (_, m) in chain.iter().rev() {
        for entry in &m.files {
            file_map.insert(entry.path.clone(), entry.clone());
        }
    }

    Ok(file_map.into_values().collect())
}

// ============================================================================
// Command: list
// ============================================================================

fn cmd_list(dest: &Path, source_filter: Option<&str>) -> io::Result<()> {
    let metas = list_backups(dest)?;

    if metas.is_empty() {
        println!("No backups found.");
        return Ok(());
    }

    let filtered: Vec<&BackupMeta> = if let Some(source) = source_filter {
        metas.iter().filter(|m| m.source.contains(source)).collect()
    } else {
        metas.iter().collect()
    };

    println!(
        "{:<30} {:<12} {:<20} {:>8} {:>12}",
        "BACKUP ID", "TYPE", "DATE", "FILES", "SIZE"
    );
    println!("{}", "-".repeat(84));

    for meta in &filtered {
        println!(
            "{:<30} {:<12} {:<20} {:>8} {:>12}",
            meta.id,
            meta.backup_type.as_str(),
            format_timestamp(meta.timestamp),
            meta.file_count,
            format_size(meta.total_size),
        );
    }

    println!("\nTotal: {} backups", filtered.len());

    Ok(())
}

// ============================================================================
// Command: verify
// ============================================================================

fn cmd_verify(dest: &Path, backup_id: &str) -> io::Result<()> {
    let manifest = load_manifest(dest, backup_id)?;
    let meta = load_meta(dest, backup_id)?;
    let store = ContentStore::new(dest);

    println!("Verifying backup: {}", backup_id);
    println!("  Type: {}", meta.backup_type);
    println!("  Files: {}", manifest.files.len());
    println!();

    let mut ok = 0u64;
    let mut missing = 0u64;
    let mut corrupt = 0u64;

    for entry in &manifest.files {
        if entry.is_symlink {
            ok += 1;
            continue;
        }

        if !store.has_blob(&entry.hash) {
            eprintln!("  MISSING: {} (hash: {})", entry.path.display(), entry.hash);
            missing += 1;
            continue;
        }

        match store.verify_blob(&entry.hash) {
            Ok(true) => ok += 1,
            Ok(false) => {
                eprintln!("  CORRUPT: {} (hash: {})", entry.path.display(), entry.hash);
                corrupt += 1;
            }
            Err(e) => {
                eprintln!("  ERROR: {} — {}", entry.path.display(), e);
                corrupt += 1;
            }
        }
    }

    println!("\nVerification results:");
    println!("  OK: {}", ok);
    if missing > 0 {
        println!("  Missing blobs: {}", missing);
    }
    if corrupt > 0 {
        println!("  Corrupt blobs: {}", corrupt);
    }

    if missing == 0 && corrupt == 0 {
        println!("  Status: PASSED");
        Ok(())
    } else {
        println!("  Status: FAILED");
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "backup verification failed",
        ))
    }
}

// ============================================================================
// Command: prune
// ============================================================================

struct PruneOptions {
    dest: PathBuf,
    keep_last: Option<u64>,
    keep_daily: Option<u64>,
    keep_weekly: Option<u64>,
    keep_monthly: Option<u64>,
}

fn cmd_prune(opts: PruneOptions) -> io::Result<()> {
    let metas = list_backups(&opts.dest)?;

    if metas.is_empty() {
        println!("No backups to prune.");
        return Ok(());
    }

    let keep_set = compute_retention(&metas, &opts);

    let to_remove: Vec<&BackupMeta> = metas.iter().filter(|m| !keep_set.contains(&m.id)).collect();

    if to_remove.is_empty() {
        println!("No backups to prune (all match retention policy).");
        return Ok(());
    }

    println!("Pruning {} backups:", to_remove.len());
    for meta in &to_remove {
        println!(
            "  - {} ({}, {})",
            meta.id,
            meta.backup_type,
            format_timestamp(meta.timestamp)
        );
    }

    // Remove backup directories
    for meta in &to_remove {
        let backup_dir = backups_dir(&opts.dest).join(&meta.id);
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir)?;
        }
    }

    // Clean up unreferenced blobs
    let remaining_metas = list_backups(&opts.dest)?;
    let mut referenced_hashes = std::collections::HashSet::new();
    for meta in &remaining_metas {
        if let Ok(manifest) = load_manifest(&opts.dest, &meta.id) {
            for entry in &manifest.files {
                referenced_hashes.insert(entry.hash.clone());
            }
        }
    }

    let store = ContentStore::new(&opts.dest);
    let all_blobs = store.all_blobs()?;
    let mut removed_blobs = 0u64;
    for blob_hash in &all_blobs {
        if !referenced_hashes.contains(blob_hash) {
            store.remove_blob(blob_hash)?;
            removed_blobs += 1;
        }
    }

    println!("\nPrune complete:");
    println!("  Backups removed: {}", to_remove.len());
    println!("  Orphan blobs removed: {}", removed_blobs);

    Ok(())
}

/// Compute which backup IDs to keep based on retention policy.
fn compute_retention(
    metas: &[BackupMeta],
    opts: &PruneOptions,
) -> std::collections::HashSet<String> {
    let mut keep = std::collections::HashSet::new();

    // Keep last N
    if let Some(n) = opts.keep_last {
        for meta in metas.iter().rev().take(n as usize) {
            keep.insert(meta.id.clone());
        }
    }

    // Keep daily: one backup per day for last N days
    if let Some(n) = opts.keep_daily {
        let mut days_seen = std::collections::HashSet::new();
        for meta in metas.iter().rev() {
            let day = meta.timestamp / 86400;
            if days_seen.len() < n as usize && days_seen.insert(day) {
                keep.insert(meta.id.clone());
            }
        }
    }

    // Keep weekly: one backup per week for last N weeks
    if let Some(n) = opts.keep_weekly {
        let mut weeks_seen = std::collections::HashSet::new();
        for meta in metas.iter().rev() {
            let week = meta.timestamp / (86400 * 7);
            if weeks_seen.len() < n as usize && weeks_seen.insert(week) {
                keep.insert(meta.id.clone());
            }
        }
    }

    // Keep monthly: one backup per month for last N months
    if let Some(n) = opts.keep_monthly {
        let mut months_seen = std::collections::HashSet::new();
        for meta in metas.iter().rev() {
            let month = meta.timestamp / (86400 * 30); // approximate
            if months_seen.len() < n as usize && months_seen.insert(month) {
                keep.insert(meta.id.clone());
            }
        }
    }

    // If no retention policy specified, keep everything
    if opts.keep_last.is_none()
        && opts.keep_daily.is_none()
        && opts.keep_weekly.is_none()
        && opts.keep_monthly.is_none()
    {
        for meta in metas {
            keep.insert(meta.id.clone());
        }
    }

    keep
}

// ============================================================================
// Command: schedule
// ============================================================================

fn cmd_schedule(dest: &Path, source: &str, interval: &str) -> io::Result<()> {
    let sched_path = schedules_path(dest);
    let mut schedules: Vec<ScheduleEntry> = if sched_path.exists() {
        let content = fs::read_to_string(&sched_path)?;
        if let Ok(val) = json_parse(&content) {
            if let Some(arr) = val.as_array() {
                arr.iter().filter_map(ScheduleEntry::from_json).collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Validate interval
    match interval {
        "daily" | "weekly" | "monthly" => {}
        _ => {
            eprintln!(
                "error: invalid interval '{}'. Must be daily, weekly, or monthly.",
                interval
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid interval",
            ));
        }
    }

    let entry = ScheduleEntry {
        source: source.to_string(),
        dest: dest.to_string_lossy().to_string(),
        interval: interval.to_string(),
    };

    // Remove existing schedule for same source/dest
    schedules.retain(|s| !(s.source == entry.source && s.dest == entry.dest));
    schedules.push(entry);

    // Save
    let json_arr = JsonValue::Array(schedules.iter().map(|s| s.to_json()).collect());
    fs::create_dir_all(dest)?;
    // Crash-safe: this rewrites the whole schedule list, so a truncated write
    // does not lose the one schedule being added — it loses all of them.
    safeio::write_str_atomically(&sched_path, &json_pretty(&json_arr, 2))?;

    println!(
        "Schedule saved: {} -> {} ({})",
        source,
        dest.display(),
        interval
    );
    Ok(())
}

// ============================================================================
// Command: diff
// ============================================================================

fn cmd_diff(dest: &Path, id1: &str, id2: &str) -> io::Result<()> {
    let manifest1 = load_manifest(dest, id1)?;
    let manifest2 = load_manifest(dest, id2)?;
    let meta1 = load_meta(dest, id1)?;
    let meta2 = load_meta(dest, id2)?;

    println!("Comparing backups:");
    println!("  [1] {} ({})", id1, format_timestamp(meta1.timestamp));
    println!("  [2] {} ({})", id2, format_timestamp(meta2.timestamp));
    println!();

    let diff = detect_changes(&manifest2.files, &manifest1);

    if diff.is_empty() {
        println!("No differences found.");
        return Ok(());
    }

    if !diff.added.is_empty() {
        println!("Added ({}):", diff.added.len());
        for f in &diff.added {
            println!("  + {} ({})", f.path.display(), format_size(f.size));
        }
        println!();
    }

    if !diff.modified.is_empty() {
        println!("Modified ({}):", diff.modified.len());
        for (old, new) in &diff.modified {
            let size_change = new.size as i64 - old.size as i64;
            let sign = if size_change >= 0 { "+" } else { "" };
            println!("  ~ {} ({}{} bytes)", new.path.display(), sign, size_change);
        }
        println!();
    }

    if !diff.deleted.is_empty() {
        println!("Deleted ({}):", diff.deleted.len());
        for f in &diff.deleted {
            println!("  - {} ({})", f.path.display(), format_size(f.size));
        }
        println!();
    }

    // Summary
    let added_size: u64 = diff.added.iter().map(|f| f.size).sum();
    let deleted_size: u64 = diff.deleted.iter().map(|f| f.size).sum();
    let modified_new_size: u64 = diff.modified.iter().map(|(_, n)| n.size).sum();
    let modified_old_size: u64 = diff.modified.iter().map(|(o, _)| o.size).sum();

    println!("Summary:");
    println!(
        "  Added: {} files (+{})",
        diff.added.len(),
        format_size(added_size)
    );
    println!(
        "  Modified: {} files (was {}, now {})",
        diff.modified.len(),
        format_size(modified_old_size),
        format_size(modified_new_size),
    );
    println!(
        "  Deleted: {} files (-{})",
        diff.deleted.len(),
        format_size(deleted_size)
    );

    Ok(())
}

// ============================================================================
// Command: info
// ============================================================================

fn cmd_info(dest: &Path, backup_id: &str) -> io::Result<()> {
    let meta = load_meta(dest, backup_id)?;
    let manifest = load_manifest(dest, backup_id)?;

    println!("Backup: {}", meta.id);
    println!("  Type:       {}", meta.backup_type);
    println!("  Created:    {}", format_timestamp(meta.timestamp));
    println!("  Source:     {}", meta.source);
    println!(
        "  Parent:     {}",
        meta.parent_id.as_deref().unwrap_or("(none)")
    );
    println!("  Files:      {}", meta.file_count);
    println!("  Total size: {}", format_size(meta.total_size));
    println!("  New blobs:  {}", meta.new_blobs);
    println!("  Dedup:      {}", meta.dedup_blobs);
    println!();

    // File type breakdown
    let mut by_ext: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for entry in &manifest.files {
        // `Path::extension` (unlike splitting on '.') correctly reports no
        // extension for "README" and for a dotfile like ".gitignore".
        let ext = entry.path.extension().map_or_else(
            || "(no ext)".to_string(),
            |e| e.to_string_lossy().into_owned(),
        );
        let (count, size) = by_ext.entry(ext).or_insert((0, 0));
        *count += 1;
        *size += entry.size;
    }

    if !by_ext.is_empty() {
        println!("  File types:");
        let mut sorted: Vec<_> = by_ext.into_iter().collect();
        sorted.sort_by_key(|item| std::cmp::Reverse(item.1.1));
        for (ext, (count, size)) in sorted.iter().take(10) {
            println!(
                "    .{:<12} {:>6} files  {:>12}",
                ext,
                count,
                format_size(*size)
            );
        }
        if sorted.len() > 10 {
            println!("    ... and {} more types", sorted.len() - 10);
        }
    }

    Ok(())
}

// ============================================================================
// Argument Parsing
// ============================================================================

enum Command {
    Create(CreateOptions),
    Restore(RestoreOptions),
    List {
        dest: PathBuf,
        source: Option<String>,
    },
    Verify {
        dest: PathBuf,
        backup_id: String,
    },
    Prune(PruneOptions),
    Schedule {
        dest: PathBuf,
        source: String,
        interval: String,
    },
    Diff {
        dest: PathBuf,
        id1: String,
        id2: String,
    },
    Info {
        dest: PathBuf,
        backup_id: String,
    },
    Help,
}

fn parse_args() -> Result<Command, String> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Ok(Command::Help);
    }

    match args[1].as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "create" => parse_create_args(&args[2..]),
        "restore" => parse_restore_args(&args[2..]),
        "list" => parse_list_args(&args[2..]),
        "verify" => parse_verify_args(&args[2..]),
        "prune" => parse_prune_args(&args[2..]),
        "schedule" => parse_schedule_args(&args[2..]),
        "diff" => parse_diff_args(&args[2..]),
        "info" => parse_info_args(&args[2..]),
        cmd => Err(format!("unknown command: {}", cmd)),
    }
}

fn parse_create_args(args: &[String]) -> Result<Command, String> {
    let mut backup_type = BackupType::Full;
    let mut source: Option<PathBuf> = None;
    let mut dest: Option<PathBuf> = None;
    let mut exclude: Vec<String> = Vec::new();
    let mut follow_symlinks = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--full" => backup_type = BackupType::Full,
            "--incremental" => backup_type = BackupType::Incremental,
            "--differential" => backup_type = BackupType::Differential,
            "--follow-symlinks" => follow_symlinks = true,
            "--source" => {
                i += 1;
                source = Some(PathBuf::from(
                    args.get(i).ok_or("--source requires a path")?,
                ));
            }
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            "--exclude" => {
                i += 1;
                exclude.push(args.get(i).ok_or("--exclude requires a pattern")?.clone());
            }
            other => return Err(format!("unknown option for create: {}", other)),
        }
        i += 1;
    }

    let source = source.ok_or("--source is required")?;
    let dest = dest.ok_or("--dest is required")?;

    Ok(Command::Create(CreateOptions {
        backup_type,
        source,
        dest,
        exclude,
        follow_symlinks,
    }))
}

fn parse_restore_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("restore requires a BACKUP_ID".to_string());
    }

    let backup_id = args[0].clone();
    let mut backup_dest: Option<PathBuf> = None;
    let mut restore_dest: Option<PathBuf> = None;
    let mut file_pattern: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dest" => {
                i += 1;
                restore_dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            "--from" => {
                i += 1;
                backup_dest = Some(PathBuf::from(args.get(i).ok_or("--from requires a path")?));
            }
            "--files" => {
                i += 1;
                file_pattern = Some(args.get(i).ok_or("--files requires a pattern")?.clone());
            }
            other => return Err(format!("unknown option for restore: {}", other)),
        }
        i += 1;
    }

    let backup_dest = backup_dest.ok_or("--from is required (backup repository path)")?;
    let restore_dest = restore_dest.ok_or("--dest is required (restore destination)")?;

    Ok(Command::Restore(RestoreOptions {
        backup_id,
        backup_dest,
        restore_dest,
        file_pattern,
    }))
}

fn parse_list_args(args: &[String]) -> Result<Command, String> {
    let mut dest: Option<PathBuf> = None;
    let mut source: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            "--source" => {
                i += 1;
                source = Some(args.get(i).ok_or("--source requires a path")?.clone());
            }
            other => {
                // Positional: treat as dest
                if dest.is_none() {
                    dest = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unknown option for list: {}", other));
                }
            }
        }
        i += 1;
    }

    let dest = dest.ok_or("destination path is required")?;
    Ok(Command::List { dest, source })
}

fn parse_verify_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("verify requires a BACKUP_ID".to_string());
    }

    let backup_id = args[0].clone();
    let mut dest: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            other => {
                if dest.is_none() {
                    dest = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unknown option for verify: {}", other));
                }
            }
        }
        i += 1;
    }

    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Verify { dest, backup_id })
}

fn parse_prune_args(args: &[String]) -> Result<Command, String> {
    let mut dest: Option<PathBuf> = None;
    let mut keep_last: Option<u64> = None;
    let mut keep_daily: Option<u64> = None;
    let mut keep_weekly: Option<u64> = None;
    let mut keep_monthly: Option<u64> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            "--keep-last" => {
                i += 1;
                keep_last = Some(
                    args.get(i)
                        .ok_or("--keep-last requires a number")?
                        .parse::<u64>()
                        .map_err(|_| "--keep-last must be a number")?,
                );
            }
            "--keep-daily" => {
                i += 1;
                keep_daily = Some(
                    args.get(i)
                        .ok_or("--keep-daily requires a number")?
                        .parse::<u64>()
                        .map_err(|_| "--keep-daily must be a number")?,
                );
            }
            "--keep-weekly" => {
                i += 1;
                keep_weekly = Some(
                    args.get(i)
                        .ok_or("--keep-weekly requires a number")?
                        .parse::<u64>()
                        .map_err(|_| "--keep-weekly must be a number")?,
                );
            }
            "--keep-monthly" => {
                i += 1;
                keep_monthly = Some(
                    args.get(i)
                        .ok_or("--keep-monthly requires a number")?
                        .parse::<u64>()
                        .map_err(|_| "--keep-monthly must be a number")?,
                );
            }
            other => {
                if dest.is_none() {
                    dest = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unknown option for prune: {}", other));
                }
            }
        }
        i += 1;
    }

    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Prune(PruneOptions {
        dest,
        keep_last,
        keep_daily,
        keep_weekly,
        keep_monthly,
    }))
}

fn parse_schedule_args(args: &[String]) -> Result<Command, String> {
    let mut source: Option<String> = None;
    let mut dest: Option<PathBuf> = None;
    let mut interval: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--source" => {
                i += 1;
                source = Some(args.get(i).ok_or("--source requires a path")?.clone());
            }
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            "--interval" => {
                i += 1;
                interval = Some(args.get(i).ok_or("--interval requires a value")?.clone());
            }
            other => return Err(format!("unknown option for schedule: {}", other)),
        }
        i += 1;
    }

    let source = source.ok_or("--source is required")?;
    let dest = dest.ok_or("--dest is required")?;
    let interval = interval.ok_or("--interval is required")?;

    Ok(Command::Schedule {
        dest,
        source,
        interval,
    })
}

fn parse_diff_args(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("diff requires two BACKUP_IDs".to_string());
    }

    let id1 = args[0].clone();
    let id2 = args[1].clone();
    let mut dest: Option<PathBuf> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            other => {
                if dest.is_none() {
                    dest = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unknown option for diff: {}", other));
                }
            }
        }
        i += 1;
    }

    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Diff { dest, id1, id2 })
}

fn parse_info_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("info requires a BACKUP_ID".to_string());
    }

    let backup_id = args[0].clone();
    let mut dest: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dest" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            other => {
                if dest.is_none() {
                    dest = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unknown option for info: {}", other));
                }
            }
        }
        i += 1;
    }

    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Info { dest, backup_id })
}

// ============================================================================
// Help Text
// ============================================================================

fn print_help() {
    println!("backup — Slate OS snapshot-based backup system");
    println!();
    println!("USAGE:");
    println!("  backup <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("  create     Create a new backup");
    println!("  restore    Restore files from a backup");
    println!("  list       List available backups");
    println!("  verify     Verify backup integrity");
    println!("  prune      Remove old backups per retention policy");
    println!("  schedule   Set up a backup schedule");
    println!("  diff       Compare two backups");
    println!("  info       Show detailed backup information");
    println!("  help       Show this help message");
    println!();
    println!("CREATE OPTIONS:");
    println!("  --full             Full backup (default)");
    println!("  --incremental      Only files changed since last backup");
    println!("  --differential     Only files changed since last full backup");
    println!("  --source <PATH>    Source directory to back up (required)");
    println!("  --dest <PATH>      Destination backup repository (required)");
    println!("  --exclude <PAT>    Exclude files matching glob pattern (repeatable)");
    println!("  --follow-symlinks  Follow symbolic links");
    println!();
    println!("RESTORE OPTIONS:");
    println!("  backup restore <BACKUP_ID> --from <REPO> --dest <PATH> [--files <PATTERN>]");
    println!();
    println!("PRUNE OPTIONS:");
    println!("  --keep-last N      Keep the N most recent backups");
    println!("  --keep-daily N     Keep one backup per day for N days");
    println!("  --keep-weekly N    Keep one backup per week for N weeks");
    println!("  --keep-monthly N   Keep one backup per month for N months");
    println!();
    println!("EXAMPLES:");
    println!("  backup create --full --source /home/user --dest /mnt/backup");
    println!(
        "  backup create --incremental --source /home/user --dest /mnt/backup --exclude '*.tmp'"
    );
    println!("  backup list --dest /mnt/backup");
    println!("  backup restore 1700000000-full --from /mnt/backup --dest /tmp/restore");
    println!("  backup verify 1700000000-full --dest /mnt/backup");
    println!("  backup prune --dest /mnt/backup --keep-last 5 --keep-weekly 4");
    println!("  backup diff 1700000000-full 1700100000-incremental --dest /mnt/backup");
    println!("  backup info 1700000000-full --dest /mnt/backup");
    println!("  backup schedule --source /home/user --dest /mnt/backup --interval daily");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let cmd = match parse_args() {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("Try 'backup help' for usage information.");
            process::exit(1);
        }
    };

    let result = match cmd {
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Create(opts) => cmd_create(opts),
        Command::Restore(opts) => cmd_restore(opts),
        Command::List { dest, source } => cmd_list(&dest, source.as_deref()),
        Command::Verify { dest, backup_id } => cmd_verify(&dest, &backup_id),
        Command::Prune(opts) => cmd_prune(opts),
        Command::Schedule {
            dest,
            source,
            interval,
        } => cmd_schedule(&dest, &source, &interval),
        Command::Diff { dest, id1, id2 } => cmd_diff(&dest, &id1, &id2),
        Command::Info { dest, backup_id } => cmd_info(&dest, &backup_id),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- SHA-256 Tests ---

    #[test]
    fn test_sha256_empty() {
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_abc() {
        let hash = sha256_hex(b"abc");
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_longer() {
        let hash = sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            hash,
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_sha256_multiblock() {
        // 64 bytes exactly — one full block
        let data = vec![0x61u8; 64]; // 'a' * 64
        let hash = sha256_hex(&data);
        assert_eq!(
            hash,
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    #[test]
    fn test_sha256_incremental() {
        // Build hash incrementally vs all-at-once
        let data = b"The quick brown fox jumps over the lazy dog";
        let hash_once = sha256_hex(data);

        let mut hasher = Sha256::new();
        hasher.update(&data[..10]);
        hasher.update(&data[10..20]);
        hasher.update(&data[20..]);
        let hash_parts = {
            let h = hasher.finalize();
            let mut hex = String::with_capacity(64);
            for byte in &h {
                hex.push(HEX_CHARS[(byte >> 4) as usize]);
                hex.push(HEX_CHARS[(byte & 0x0f) as usize]);
            }
            hex
        };

        assert_eq!(hash_once, hash_parts);
    }

    // --- Glob Pattern Matching Tests ---

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("*.txt", "file.txt"));
        assert!(glob_matches("*.txt", "long.name.txt"));
        assert!(!glob_matches("*.txt", "file.rs"));
        assert!(!glob_matches("*.txt", "dir/file.txt")); // * doesn't cross /
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_matches("file?.txt", "file1.txt"));
        assert!(glob_matches("file?.txt", "fileA.txt"));
        assert!(!glob_matches("file?.txt", "file12.txt"));
        assert!(!glob_matches("file?.txt", "file.txt"));
    }

    #[test]
    fn a_question_mark_matches_one_character_not_one_byte() {
        // `?` is documented as "single char except /". Every one of these is a
        // single character, and every one of them is more than a single byte.
        for ch in ["\u{e9}", "\u{65e5}", "\u{03b1}", "\u{0440}", "\u{1f600}"] {
            assert!(
                glob_matches("?.txt", &format!("{ch}.txt")),
                "?.txt should match {ch}.txt"
            );
            assert!(
                glob_matches("file?.txt", &format!("file{ch}.txt")),
                "file?.txt should match file{ch}.txt"
            );
            // ...and still exactly one of them.
            assert!(
                !glob_matches("?.txt", &format!("{ch}{ch}.txt")),
                "?.txt should not match two characters"
            );
            assert!(
                glob_matches("??.txt", &format!("{ch}{ch}.txt")),
                "??.txt should match two characters"
            );
        }
        // Mixed widths in one name, and `?` against an ASCII character still
        // consumes exactly one byte.
        assert!(glob_matches("?x?.txt", "\u{65e5}x\u{672c}.txt"));
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "ab"));
        // `?` must not swallow a separator, whatever its width.
        assert!(!glob_matches("a?c", "a/c"));
    }

    #[test]
    fn a_non_ascii_name_is_matched_and_excluded_consistently() {
        // The two entry points a pattern actually reaches: whole-path matching
        // and the filename-only fallback in `is_excluded`.
        let patterns = vec!["**/?.log".to_string()];
        assert!(is_excluded(Path::new("var/\u{65e5}.log"), &patterns));
        assert!(!is_excluded(
            Path::new("var/\u{65e5}\u{672c}.log"),
            &patterns
        ));
        let patterns = vec!["\u{65e5}*".to_string()];
        assert!(is_excluded(
            Path::new("dir/\u{65e5}\u{672c}\u{8a9e}.txt"),
            &patterns
        ));
    }

    #[test]
    fn a_truncated_utf8_sequence_does_not_run_past_the_end() {
        // Paths are byte strings and need not be well-formed UTF-8. An
        // ill-formed sequence falls back to one byte per `?`, so a lead byte
        // whose continuations are missing is just a byte.
        assert!(glob_match_recursive(b"?", &[0xE6]));
        assert!(!glob_match_recursive(b"?", &[0xE6, 0x97]));
        assert!(glob_match_recursive(b"??", &[0xE6, 0x97]));
        // A complete sequence is one character.
        assert!(glob_match_recursive(b"?", &[0xE6, 0x97, 0xA5]));
        // A stray continuation byte is consumed one byte at a time.
        assert!(glob_match_recursive(b"?", &[0x97]));
        assert!(glob_match_recursive(b"??", &[0x97, 0xA5]));
    }

    #[test]
    fn a_question_mark_never_crosses_a_separator() {
        // The invariant that made clamping-to-what-remains the wrong fallback:
        // a lead byte announcing three bytes, followed by a separator, must not
        // let `?` consume the separator.
        // This is the case that actually pins it. A lead byte claiming three
        // bytes, with only a separator behind it: consuming "what remains"
        // would swallow the `/` and report a match for a name that contains
        // one. Validating the continuations rejects it.
        assert!(!glob_match_recursive(b"?", &[0xE6, b'/']));
        assert!(!glob_match_recursive(b"?c", &[0xE6, b'/', b'c']));
        assert!(!glob_match_recursive(b"??c", &[0xE6, b'/', b'c']));
        // Well-formed input, same rule.
        assert!(!glob_matches("a?c", "a/c"));
        assert!(!glob_matches("?", "/"));
    }

    #[test]
    fn test_glob_doublestar() {
        assert!(glob_matches("**/*.txt", "file.txt"));
        assert!(glob_matches("**/*.txt", "dir/file.txt"));
        assert!(glob_matches("**/*.txt", "a/b/c/file.txt"));
        assert!(!glob_matches("**/*.rs", "file.txt"));
    }

    #[test]
    fn test_glob_exact() {
        assert!(glob_matches("Makefile", "Makefile"));
        assert!(!glob_matches("Makefile", "makefile"));
        assert!(!glob_matches("Makefile", "dir/Makefile"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_matches("src/**/*.rs", "src/main.rs"));
        assert!(glob_matches("src/**/*.rs", "src/sub/mod.rs"));
        assert!(!glob_matches("src/**/*.rs", "lib/main.rs"));
    }

    #[test]
    fn test_glob_star_prefix() {
        assert!(glob_matches("test_*", "test_foo"));
        assert!(glob_matches("test_*", "test_"));
        assert!(!glob_matches("test_*", "test"));
    }

    // --- Change Detection Tests ---

    #[test]
    fn test_detect_no_changes() {
        let files = vec![FileEntry {
            path: PathBuf::from("a.txt"),
            size: 100,
            mtime: 1000,
            hash: "abc123".to_string(),
            is_symlink: false,
            link_target: None,
        }];
        let manifest = Manifest {
            files: files.clone(),
        };
        let diff = detect_changes(&files, &manifest);
        assert!(diff.added.is_empty());
        assert!(diff.modified.is_empty());
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detect_added() {
        let prev = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("a.txt"),
                size: 100,
                mtime: 1000,
                hash: "aaa".to_string(),
                is_symlink: false,
                link_target: None,
            }],
        };
        let current = vec![
            FileEntry {
                path: PathBuf::from("a.txt"),
                size: 100,
                mtime: 1000,
                hash: "aaa".to_string(),
                is_symlink: false,
                link_target: None,
            },
            FileEntry {
                path: PathBuf::from("b.txt"),
                size: 200,
                mtime: 2000,
                hash: "bbb".to_string(),
                is_symlink: false,
                link_target: None,
            },
        ];
        let diff = detect_changes(&current, &prev);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].path, Path::new("b.txt"));
        assert!(diff.modified.is_empty());
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detect_modified() {
        let prev = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("a.txt"),
                size: 100,
                mtime: 1000,
                hash: "old_hash".to_string(),
                is_symlink: false,
                link_target: None,
            }],
        };
        let current = vec![FileEntry {
            path: PathBuf::from("a.txt"),
            size: 150,
            mtime: 2000,
            hash: "new_hash".to_string(),
            is_symlink: false,
            link_target: None,
        }];
        let diff = detect_changes(&current, &prev);
        assert!(diff.added.is_empty());
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.modified[0].0.hash, "old_hash");
        assert_eq!(diff.modified[0].1.hash, "new_hash");
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detect_deleted() {
        let prev = Manifest {
            files: vec![
                FileEntry {
                    path: PathBuf::from("a.txt"),
                    size: 100,
                    mtime: 1000,
                    hash: "aaa".to_string(),
                    is_symlink: false,
                    link_target: None,
                },
                FileEntry {
                    path: PathBuf::from("b.txt"),
                    size: 200,
                    mtime: 2000,
                    hash: "bbb".to_string(),
                    is_symlink: false,
                    link_target: None,
                },
            ],
        };
        let current = vec![FileEntry {
            path: PathBuf::from("a.txt"),
            size: 100,
            mtime: 1000,
            hash: "aaa".to_string(),
            is_symlink: false,
            link_target: None,
        }];
        let diff = detect_changes(&current, &prev);
        assert!(diff.added.is_empty());
        assert!(diff.modified.is_empty());
        assert_eq!(diff.deleted.len(), 1);
        assert_eq!(diff.deleted[0].path, Path::new("b.txt"));
    }

    // --- Manifest Serialization Tests ---

    #[test]
    fn test_manifest_roundtrip() {
        let manifest = Manifest {
            files: vec![
                FileEntry {
                    path: PathBuf::from("src/main.rs"),
                    size: 1024,
                    mtime: 1700000000,
                    hash: "abcdef0123456789".to_string(),
                    is_symlink: false,
                    link_target: None,
                },
                FileEntry {
                    path: PathBuf::from("README.md"),
                    size: 512,
                    mtime: 1699999000,
                    hash: "9876543210fedcba".to_string(),
                    is_symlink: false,
                    link_target: None,
                },
            ],
        };

        let serialized = manifest.serialize();
        let deserialized = Manifest::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.files.len(), 2);
        assert_eq!(deserialized.files[0].path, Path::new("src/main.rs"));
        assert_eq!(deserialized.files[0].size, 1024);
        assert_eq!(deserialized.files[0].hash, "abcdef0123456789");
        assert_eq!(deserialized.files[1].path, Path::new("README.md"));
    }

    #[test]
    fn test_manifest_with_symlink() {
        let manifest = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("link"),
                size: 0,
                mtime: 0,
                hash: "linkhash".to_string(),
                is_symlink: true,
                link_target: Some(PathBuf::from("/usr/bin/target")),
            }],
        };

        let serialized = manifest.serialize();
        let deserialized = Manifest::deserialize(&serialized).unwrap();

        assert!(deserialized.files[0].is_symlink);
        assert_eq!(
            deserialized.files[0].link_target.as_deref(),
            Some(Path::new("/usr/bin/target"))
        );
    }

    #[test]
    fn test_meta_roundtrip() {
        let meta = BackupMeta {
            id: "1700000000-full".to_string(),
            backup_type: BackupType::Full,
            timestamp: 1700000000,
            source: "/home/user".to_string(),
            parent_id: None,
            file_count: 42,
            total_size: 1048576,
            new_blobs: 40,
            dedup_blobs: 2,
        };

        let serialized = meta.serialize();
        let deserialized = BackupMeta::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.id, "1700000000-full");
        assert_eq!(deserialized.backup_type, BackupType::Full);
        assert_eq!(deserialized.timestamp, 1700000000);
        assert_eq!(deserialized.source, "/home/user");
        assert!(deserialized.parent_id.is_none());
        assert_eq!(deserialized.file_count, 42);
    }

    #[test]
    fn test_meta_with_parent() {
        let meta = BackupMeta {
            id: "1700100000-incremental".to_string(),
            backup_type: BackupType::Incremental,
            timestamp: 1700100000,
            source: "/home/user".to_string(),
            parent_id: Some("1700000000-full".to_string()),
            file_count: 5,
            total_size: 4096,
            new_blobs: 3,
            dedup_blobs: 2,
        };

        let serialized = meta.serialize();
        let deserialized = BackupMeta::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.backup_type, BackupType::Incremental);
        assert_eq!(deserialized.parent_id.as_deref(), Some("1700000000-full"));
    }

    // --- Pruning Retention Policy Tests ---

    #[test]
    fn test_prune_keep_last() {
        let metas = vec![
            BackupMeta {
                id: "1".to_string(),
                backup_type: BackupType::Full,
                timestamp: 100,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
            BackupMeta {
                id: "2".to_string(),
                backup_type: BackupType::Full,
                timestamp: 200,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
            BackupMeta {
                id: "3".to_string(),
                backup_type: BackupType::Full,
                timestamp: 300,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
        ];

        let opts = PruneOptions {
            dest: PathBuf::from("/tmp"),
            keep_last: Some(2),
            keep_daily: None,
            keep_weekly: None,
            keep_monthly: None,
        };

        let keep = compute_retention(&metas, &opts);
        assert!(!keep.contains("1"));
        assert!(keep.contains("2"));
        assert!(keep.contains("3"));
    }

    #[test]
    fn test_prune_keep_daily() {
        let day = 86400u64;
        let metas = vec![
            BackupMeta {
                id: "d1_a".to_string(),
                backup_type: BackupType::Full,
                timestamp: day * 10 + 100,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
            BackupMeta {
                id: "d1_b".to_string(),
                backup_type: BackupType::Incremental,
                timestamp: day * 10 + 200,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
            BackupMeta {
                id: "d2_a".to_string(),
                backup_type: BackupType::Full,
                timestamp: day * 11 + 100,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
            BackupMeta {
                id: "d3_a".to_string(),
                backup_type: BackupType::Full,
                timestamp: day * 12 + 100,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
        ];

        let opts = PruneOptions {
            dest: PathBuf::from("/tmp"),
            keep_last: None,
            keep_daily: Some(2),
            keep_weekly: None,
            keep_monthly: None,
        };

        let keep = compute_retention(&metas, &opts);
        // Should keep the most recent backup from the 2 most recent days
        assert!(keep.contains("d3_a")); // day 12
        assert!(keep.contains("d2_a")); // day 11
        // day 10 should not be kept (only 2 days retained)
        assert!(!keep.contains("d1_a"));
        assert!(!keep.contains("d1_b"));
    }

    #[test]
    fn test_prune_no_policy_keeps_all() {
        let metas = vec![
            BackupMeta {
                id: "1".to_string(),
                backup_type: BackupType::Full,
                timestamp: 100,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
            BackupMeta {
                id: "2".to_string(),
                backup_type: BackupType::Full,
                timestamp: 200,
                source: "/src".to_string(),
                parent_id: None,
                file_count: 0,
                total_size: 0,
                new_blobs: 0,
                dedup_blobs: 0,
            },
        ];

        let opts = PruneOptions {
            dest: PathBuf::from("/tmp"),
            keep_last: None,
            keep_daily: None,
            keep_weekly: None,
            keep_monthly: None,
        };

        let keep = compute_retention(&metas, &opts);
        assert!(keep.contains("1"));
        assert!(keep.contains("2"));
    }

    // --- Path Normalization Tests ---

    #[test]
    fn test_normalize_simple() {
        assert_eq!(normalize_path("a/b/c"), "a/b/c");
        assert_eq!(normalize_path("/a/b/c"), "/a/b/c");
    }

    #[test]
    fn test_normalize_dots() {
        assert_eq!(normalize_path("a/./b/c"), "a/b/c");
        assert_eq!(normalize_path("a/b/../c"), "a/c");
        assert_eq!(normalize_path("/a/b/../c/./d"), "/a/c/d");
    }

    #[test]
    fn test_normalize_redundant_slashes() {
        assert_eq!(normalize_path("a//b///c"), "a/b/c");
        assert_eq!(normalize_path("/a//b/c"), "/a/b/c");
    }

    #[test]
    fn test_normalize_leading_dotdot() {
        // Can't go above root
        assert_eq!(normalize_path("../a/b"), "a/b");
        assert_eq!(normalize_path("../../a"), "a");
    }

    // --- JSON Parser Tests ---

    #[test]
    fn test_json_parse_string() {
        let val = json_parse(r#""hello""#).unwrap();
        assert_eq!(val.as_str(), Some("hello"));
    }

    /// The strings in a manifest are file paths. A byte-at-a-time `as char`
    /// read turns every non-ASCII path into one that names no file, so restore
    /// and verify both report it missing.
    #[test]
    fn a_non_ascii_string_survives_the_json_round_trip() {
        for text in [
            "写真/2024.jpg",
            "Musique/Café/piste.flac",
            "Ωμέγα.txt",
            "🚀/launch.log",
            "D:/Δοκιμή/日本語/файл.bin",
        ] {
            let encoded = JsonValue::Str(text.to_string()).to_string();
            let val =
                json_parse(&encoded).unwrap_or_else(|e| panic!("failed to parse {encoded}: {e}"));
            assert_eq!(val.as_str(), Some(text), "path changed: {encoded}");
        }
    }

    /// A whole manifest, not just one string: this is the path the bug actually
    /// took, since `Manifest::serialize` is what writes the file on disk.
    #[test]
    fn a_manifest_of_non_ascii_paths_round_trips() {
        let mut manifest = Manifest::new();
        for path in ["写真/2024.jpg", "Ωμέγα.txt", "plain.txt"] {
            manifest.files.push(FileEntry {
                path: PathBuf::from(path),
                size: 7,
                mtime: 11,
                hash: "abc".to_string(),
                is_symlink: false,
                link_target: None,
            });
        }
        let serialized = manifest.serialize();
        let back = Manifest::deserialize(&serialized).expect("manifest should parse");
        let got: Vec<&Path> = back.files.iter().map(|f| f.path.as_path()).collect();
        assert_eq!(
            got,
            [
                Path::new("写真/2024.jpg"),
                Path::new("Ωμέγα.txt"),
                Path::new("plain.txt")
            ]
        );
    }

    /// The write side. Fixing the manifest *parser* was not enough: the entry
    /// handed to the writer had already been through `to_string_lossy` in
    /// `relative_path`, so a byte the filesystem allowed was destroyed before
    /// the manifest ever saw it, and restore recreated the file under a
    /// different name.
    ///
    /// Asserted on bytes because that is what the manifest stores; on the
    /// Windows test host an `OsString` cannot hold a non-WTF-8 byte string at
    /// all, so routing through `PathBuf` would test the host, not the format.
    #[test]
    fn a_path_byte_the_filesystem_allows_survives_the_manifest() {
        let raw = b"photos/caf\xE9.jpg";
        let encoded = encode_bytes(raw);
        assert_eq!(encoded, "photos/caf%E9.jpg");
        assert_eq!(
            decode_bytes(&encoded),
            raw,
            "a lone 0xE9 must come back as 0xE9, not as U+FFFD"
        );
    }

    #[test]
    fn every_byte_value_round_trips_through_the_path_encoding() {
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(decode_bytes(&encode_bytes(&all)), all);
    }

    #[test]
    fn a_percent_in_a_filename_is_not_mistaken_for_an_escape() {
        // "100% done.txt" is a legal filename. Round-tripping it must not eat
        // the "20", and a `%` that is not a valid escape is kept verbatim.
        let raw = b"100% done.txt";
        assert_eq!(encode_bytes(raw), "100%25 done.txt");
        assert_eq!(decode_bytes(&encode_bytes(raw)), raw);
        assert_eq!(decode_bytes("a%zz"), b"a%zz");
    }

    /// `relative_path` produces every `FileEntry.path`, so its output is what
    /// the manifest records. It must strip the base and normalise separators
    /// without going anywhere near a string conversion.
    #[test]
    fn relative_path_strips_the_base_and_joins_with_forward_slashes() {
        let base = PathBuf::from("/srv/data");
        assert_eq!(
            relative_path(Path::new("/srv/data/a/b/c.txt"), &base),
            PathBuf::from("a/b/c.txt")
        );
        // A path that is not under the base is kept whole rather than silently
        // becoming empty — an empty relative path would collide with every
        // other such file in the manifest. The root component must survive as a
        // single leading `/`, not as a doubled or host-flavoured separator.
        assert_eq!(
            relative_path(Path::new("/elsewhere/x.txt"), &base),
            PathBuf::from("/elsewhere/x.txt")
        );
        // Host separators are normalised to `/` so the manifest reads the same
        // whichever machine wrote it.
        assert_eq!(
            relative_path(Path::new(r"a\b\c.txt"), Path::new("")),
            PathBuf::from(if cfg!(windows) {
                "a/b/c.txt"
            } else {
                r"a\b\c.txt"
            })
        );
        // The base itself is relative to nothing.
        assert_eq!(relative_path(&base, &base), PathBuf::new());
    }

    /// The defect this whole change exists for, at the point it happened:
    /// `relative_path` produced the name the manifest recorded, and it went
    /// through `to_string_lossy`, so a byte the filesystem allowed became
    /// U+FFFD before anything could escape it.
    ///
    /// Unix-only because a Windows `OsString` cannot hold such a path at all —
    /// and our target is `target-family = ["unix"]`, so this is the platform
    /// that matters.
    #[cfg(unix)]
    #[test]
    fn relative_path_does_not_mangle_a_non_utf8_name() {
        use std::os::unix::ffi::OsStrExt;
        let full = Path::new(std::ffi::OsStr::from_bytes(b"/srv/data/photos/caf\xE9.jpg"));
        let rel = relative_path(full, Path::new("/srv/data"));
        assert_eq!(
            rel.as_os_str().as_bytes(),
            b"photos/caf\xE9.jpg",
            "the 0xE9 must reach the manifest writer intact"
        );
    }

    /// Version 1 manifests stored paths verbatim. Refusing to read them would
    /// strand every backup taken before path escaping existed.
    #[test]
    fn a_version_1_manifest_is_still_readable() {
        let legacy = r#"{
          "files": [
            {"path": "src/main.rs", "size": 10, "mtime": 1, "hash": "aa", "is_symlink": false},
            {"path": "50%20off.txt", "size": 20, "mtime": 2, "hash": "bb", "is_symlink": false}
          ]
        }"#;
        let back = Manifest::deserialize(legacy).expect("a v1 manifest should still parse");
        let got: Vec<&Path> = back.files.iter().map(|f| f.path.as_path()).collect();
        assert_eq!(
            got,
            [Path::new("src/main.rs"), Path::new("50%20off.txt")],
            "v1 paths are literal: `%20` is part of the name, not an escape"
        );
    }

    /// The version marker is what tells the two formats apart, so a manifest we
    /// write must carry it.
    #[test]
    fn a_manifest_we_write_declares_its_version() {
        let mut manifest = Manifest::new();
        manifest.files.push(FileEntry {
            path: PathBuf::from("100% done.txt"),
            size: 1,
            mtime: 2,
            hash: "aa".to_string(),
            is_symlink: false,
            link_target: None,
        });
        let text = manifest.serialize();
        let val = json_parse(&text).expect("our own output should parse");
        assert_eq!(val.get("version").and_then(JsonValue::as_u64), Some(2));
        // Written escaped, and read back as the original name.
        assert!(text.contains("100%25 done.txt"), "escaped on disk: {text}");
        let back = Manifest::deserialize(&text).expect("round trip");
        assert_eq!(back.files[0].path, PathBuf::from("100% done.txt"));
    }

    #[test]
    fn a_symlink_target_is_escaped_like_any_other_path() {
        let mut manifest = Manifest::new();
        manifest.files.push(FileEntry {
            path: PathBuf::from("link"),
            size: 0,
            mtime: 0,
            hash: "aa".to_string(),
            is_symlink: true,
            link_target: Some(PathBuf::from("/opt/50% off/bin")),
        });
        let back = Manifest::deserialize(&manifest.serialize()).expect("round trip");
        assert_eq!(
            back.files[0].link_target.as_deref(),
            Some(Path::new("/opt/50% off/bin"))
        );
    }

    /// `\uXXXX` is what our own writer emits for control characters, and what
    /// any other JSON writer may emit for anything at all. The old reader
    /// dropped an astral character silently rather than pairing surrogates.
    #[test]
    fn unicode_escapes_including_surrogate_pairs_are_decoded() {
        for (encoded, want) in [
            (r#""\u0041""#, "A"),
            (r#""\u00e9""#, "é"),
            (r#""\u5199\u771f""#, "写真"),
            (r#""\ud83d\ude80/launch.log""#, "🚀/launch.log"),
            (r#""a\u0001b""#, "a\u{01}b"),
        ] {
            let val = json_parse(encoded).unwrap_or_else(|e| panic!("{encoded}: {e}"));
            assert_eq!(val.as_str(), Some(want), "wrong decode of {encoded}");
        }
    }

    /// A `\u` whose four bytes ran into a multi-byte character used to be
    /// sliced blindly, so reading a manifest could panic on a char boundary.
    #[test]
    fn a_malformed_unicode_escape_is_an_error_not_a_panic() {
        for bad in [r#""\u日本""#, r#""\u12""#, r#""\uzzzz""#, r#""\u""#] {
            assert!(
                json_parse(bad).is_err(),
                "{bad} should be rejected, not accepted or panic"
            );
        }
    }

    /// Control: the ASCII path is byte-for-byte what it always was.
    #[test]
    fn ascii_json_strings_are_unchanged() {
        for (encoded, want) in [
            (r#""hello""#, "hello"),
            (r#""a\"b""#, "a\"b"),
            (r#""a\\b""#, "a\\b"),
            (r#""a\nb\tc\/d""#, "a\nb\tc/d"),
            (r#""""#, ""),
        ] {
            let val = json_parse(encoded).unwrap_or_else(|e| panic!("{encoded}: {e}"));
            assert_eq!(val.as_str(), Some(want), "wrong parse of {encoded}");
        }
    }

    #[test]
    fn test_json_parse_number() {
        let val = json_parse("42").unwrap();
        assert_eq!(val.as_u64(), Some(42));
    }

    #[test]
    fn test_json_parse_object() {
        let val = json_parse(r#"{"key": "value", "num": 123}"#).unwrap();
        assert_eq!(val.get("key").unwrap().as_str(), Some("value"));
        assert_eq!(val.get("num").unwrap().as_u64(), Some(123));
    }

    #[test]
    fn test_json_parse_array() {
        let val = json_parse(r#"[1, 2, 3]"#).unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_u64(), Some(1));
    }

    #[test]
    fn test_json_parse_nested() {
        let val = json_parse(r#"{"files": [{"path": "a.txt", "size": 100}]}"#).unwrap();
        let files = val.get("files").unwrap().as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].get("path").unwrap().as_str(), Some("a.txt"));
        assert_eq!(files[0].get("size").unwrap().as_u64(), Some(100));
    }

    #[test]
    fn test_json_escape_roundtrip() {
        let val = JsonValue::Str("hello\nworld\t\"quoted\"".to_string());
        let serialized = format!("{}", val);
        let parsed = json_parse(&serialized).unwrap();
        assert_eq!(parsed.as_str(), Some("hello\nworld\t\"quoted\""));
    }

    // --- Format/Display Tests ---

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1048576), "1.0 MiB");
        assert_eq!(format_size(1073741824), "1.0 GiB");
    }

    #[test]
    fn test_format_timestamp() {
        // 2023-11-14 22:13:20 UTC = 1700000000
        let ts = format_timestamp(1700000000);
        assert!(ts.starts_with("2023-"));
        assert!(ts.contains(":"));
    }

    // --- Exclusion Tests ---

    #[test]
    fn test_is_excluded_simple() {
        let patterns = vec!["*.tmp".to_string(), "*.log".to_string()];
        assert!(is_excluded(Path::new("file.tmp"), &patterns));
        assert!(is_excluded(Path::new("debug.log"), &patterns));
        assert!(!is_excluded(Path::new("file.txt"), &patterns));
    }

    #[test]
    fn test_is_excluded_directory_pattern() {
        let patterns = vec!["**/node_modules/**".to_string()];
        assert!(is_excluded(
            Path::new("project/node_modules/pkg/index.js"),
            &patterns
        ));
    }

    #[test]
    fn test_is_excluded_filename_fallback() {
        // Pattern matches just the filename component
        let patterns = vec![".gitignore".to_string()];
        assert!(is_excluded(Path::new("project/.gitignore"), &patterns));
    }
}
