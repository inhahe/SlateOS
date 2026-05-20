//! Multi-personality intrusion prevention system for OurOS.
//!
//! This binary detects the mode from `argv[0]`:
//!   - `fail2ban-server` — intrusion prevention daemon
//!   - `fail2ban-client` — client for controlling the daemon
//!   - `fail2ban-regex`  — test regex patterns against log lines
//!
//! Monitors log files for repeated authentication failures and bans
//! offending IP addresses using configurable firewall actions.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
// io is used by the server for reading stdin in future interactive mode
#[allow(unused_imports)]
use std::io;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Utility: current unix timestamp
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ===========================================================================
// Minimal regex engine
// ===========================================================================

/// A compiled regular expression token.
#[derive(Debug, Clone, PartialEq)]
enum Re {
    /// Match literal character.
    Literal(u8),
    /// Match any character (except newline).
    Dot,
    /// `\d` — digit
    Digit,
    /// `\w` — word char (alphanumeric + underscore)
    Word,
    /// `\s` — whitespace
    Space,
    /// `\D` — non-digit
    NonDigit,
    /// `\W` — non-word
    NonWord,
    /// `\S` — non-whitespace
    NonSpace,
    /// Character class `[...]`. Tuple: (negated, ranges).
    /// Each range is (lo, hi) inclusive.
    CharClass(bool, Vec<(u8, u8)>),
    /// Anchor: start of line
    AnchorStart,
    /// Anchor: end of line
    AnchorEnd,
    /// Quantifier: greedy zero-or-more of a single token
    Star(Box<Re>),
    /// Quantifier: greedy one-or-more
    Plus(Box<Re>),
    /// Quantifier: zero-or-one
    Question(Box<Re>),
    /// Alternation group: try each branch in order
    Alternation(Vec<Vec<Re>>),
    /// Capturing group (we don't actually capture, just group)
    Group(Vec<Re>),
}

/// Parse error for regex compilation.
#[derive(Debug, Clone)]
struct RegexError(String);

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "regex error: {}", self.0)
    }
}

/// A compiled regex ready for matching.
#[derive(Debug, Clone)]
struct Regex {
    tokens: Vec<Re>,
    /// Original pattern string (for display/diagnostics).
    #[allow(dead_code)]
    pattern: String,
}

impl Regex {
    fn compile(pattern: &str) -> Result<Self, RegexError> {
        let tokens = parse_regex(pattern.as_bytes(), 0, false)?.0;
        Ok(Self { tokens, pattern: pattern.to_string() })
    }

    /// Return true if the regex matches anywhere in `text`.
    fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// Return the first match position (start, end) in `text`.
    fn find(&self, text: &str) -> Option<(usize, usize)> {
        let bytes = text.as_bytes();
        // If pattern is anchored at start, only try pos 0.
        if !self.tokens.is_empty() && self.tokens[0] == Re::AnchorStart {
            if let Some(end) = match_tokens(&self.tokens[1..], bytes, 0) {
                return Some((0, end));
            }
            return None;
        }
        for start in 0..=bytes.len() {
            if let Some(end) = match_tokens(&self.tokens, bytes, start) {
                return Some((start, end));
            }
        }
        None
    }

    /// Find all non-overlapping matches; return list of (start, end).
    #[allow(dead_code)]
    fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
        let bytes = text.as_bytes();
        let mut results = Vec::new();
        let mut pos = 0;
        while pos <= bytes.len() {
            let anchored = !self.tokens.is_empty() && self.tokens[0] == Re::AnchorStart;
            let search_start = if anchored { 0 } else { pos };
            let mut found = false;
            let search_end = if anchored { 0 } else { bytes.len() };
            let mut s = search_start;
            while s <= search_end {
                let tokens_to_match = if anchored { &self.tokens[1..] } else { &self.tokens[..] };
                if let Some(end) = match_tokens(tokens_to_match, bytes, s) {
                    results.push((s, end));
                    pos = if end > s { end } else { s + 1 };
                    found = true;
                    break;
                }
                if anchored { break; }
                s += 1;
            }
            if !found {
                break;
            }
        }
        results
    }
}

/// Parse regex tokens from `src` starting at `pos`.
/// If `in_group` is true, parsing stops at `)`.
/// Returns (tokens, next_position).
fn parse_regex(src: &[u8], mut pos: usize, in_group: bool) -> Result<(Vec<Re>, usize), RegexError> {
    let mut branches: Vec<Vec<Re>> = vec![Vec::new()];

    while pos < src.len() {
        let ch = src[pos];
        match ch {
            b')' if in_group => {
                pos += 1;
                return if branches.len() == 1 {
                    Ok((branches.remove(0), pos))
                } else {
                    Ok((vec![Re::Alternation(branches)], pos))
                };
            }
            b')' => {
                return Err(RegexError("unmatched ')'".to_string()));
            }
            b'|' => {
                branches.push(Vec::new());
                pos += 1;
            }
            b'(' => {
                let (inner, next) = parse_regex(src, pos + 1, true)?;
                let tok = Re::Group(inner);
                let branch = branches.last_mut().unwrap();
                pos = next;
                pos = apply_quantifier(src, pos, tok, branch)?;
            }
            b'[' => {
                let (tok, next) = parse_char_class(src, pos + 1)?;
                let branch = branches.last_mut().unwrap();
                pos = apply_quantifier(src, next, tok, branch)?;
            }
            b'^' => {
                branches.last_mut().unwrap().push(Re::AnchorStart);
                pos += 1;
            }
            b'$' => {
                branches.last_mut().unwrap().push(Re::AnchorEnd);
                pos += 1;
            }
            b'.' => {
                let branch = branches.last_mut().unwrap();
                pos = apply_quantifier(src, pos + 1, Re::Dot, branch)?;
            }
            b'\\' => {
                if pos + 1 >= src.len() {
                    return Err(RegexError("trailing backslash".to_string()));
                }
                let esc = src[pos + 1];
                let tok = match esc {
                    b'd' => Re::Digit,
                    b'D' => Re::NonDigit,
                    b'w' => Re::Word,
                    b'W' => Re::NonWord,
                    b's' => Re::Space,
                    b'S' => Re::NonSpace,
                    _ => Re::Literal(esc),
                };
                let branch = branches.last_mut().unwrap();
                pos = apply_quantifier(src, pos + 2, tok, branch)?;
            }
            b'*' | b'+' | b'?' => {
                return Err(RegexError(format!(
                    "nothing to repeat at position {pos}"
                )));
            }
            _ => {
                let branch = branches.last_mut().unwrap();
                pos = apply_quantifier(src, pos + 1, Re::Literal(ch), branch)?;
            }
        }
    }

    if in_group {
        return Err(RegexError("unterminated group".to_string()));
    }

    if branches.len() == 1 {
        Ok((branches.remove(0), pos))
    } else {
        Ok((vec![Re::Alternation(branches)], pos))
    }
}

/// Check if `src[pos]` is a quantifier (`*`, `+`, `?`) and wrap `tok`.
/// Push the resulting token onto `branch` and return the new position.
fn apply_quantifier(
    src: &[u8],
    pos: usize,
    tok: Re,
    branch: &mut Vec<Re>,
) -> Result<usize, RegexError> {
    if pos < src.len() {
        match src[pos] {
            b'*' => {
                branch.push(Re::Star(Box::new(tok)));
                return Ok(pos + 1);
            }
            b'+' => {
                branch.push(Re::Plus(Box::new(tok)));
                return Ok(pos + 1);
            }
            b'?' => {
                branch.push(Re::Question(Box::new(tok)));
                return Ok(pos + 1);
            }
            _ => {}
        }
    }
    branch.push(tok);
    Ok(pos)
}

/// Parse a character class `[...]` starting right after the `[`.
/// Returns the Re token and position right after `]`.
fn parse_char_class(src: &[u8], mut pos: usize) -> Result<(Re, usize), RegexError> {
    let negated = pos < src.len() && src[pos] == b'^';
    if negated {
        pos += 1;
    }

    let mut ranges: Vec<(u8, u8)> = Vec::new();

    // `]` right after `[` or `[^` is literal
    if pos < src.len() && src[pos] == b']' {
        ranges.push((b']', b']'));
        pos += 1;
    }

    while pos < src.len() {
        if src[pos] == b']' {
            return Ok((Re::CharClass(negated, ranges), pos + 1));
        }
        let ch = if src[pos] == b'\\' && pos + 1 < src.len() {
            pos += 1;
            src[pos]
        } else {
            src[pos]
        };
        pos += 1;

        // Check for range
        if pos + 1 < src.len() && src[pos] == b'-' && src[pos + 1] != b']' {
            pos += 1; // skip '-'
            let hi = if src[pos] == b'\\' && pos + 1 < src.len() {
                pos += 1;
                src[pos]
            } else {
                src[pos]
            };
            pos += 1;
            ranges.push((ch, hi));
        } else {
            ranges.push((ch, ch));
        }
    }

    Err(RegexError("unterminated character class".to_string()))
}

/// Try to match `tokens` against `text` starting at `pos`.
/// Returns Some(end_position) on success.
fn match_tokens(tokens: &[Re], text: &[u8], pos: usize) -> Option<usize> {
    if tokens.is_empty() {
        return Some(pos);
    }

    let tok = &tokens[0];
    let rest = &tokens[1..];

    match tok {
        Re::AnchorStart => {
            if pos == 0 {
                match_tokens(rest, text, pos)
            } else {
                None
            }
        }
        Re::AnchorEnd => {
            if pos == text.len() {
                match_tokens(rest, text, pos)
            } else {
                None
            }
        }
        Re::Literal(ch) => {
            if pos < text.len() && text[pos] == *ch {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::Dot => {
            if pos < text.len() && text[pos] != b'\n' {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::Digit => {
            if pos < text.len() && text[pos].is_ascii_digit() {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::NonDigit => {
            if pos < text.len() && !text[pos].is_ascii_digit() {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::Word => {
            if pos < text.len() && (text[pos].is_ascii_alphanumeric() || text[pos] == b'_') {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::NonWord => {
            if pos < text.len() && !(text[pos].is_ascii_alphanumeric() || text[pos] == b'_') {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::Space => {
            if pos < text.len() && text[pos].is_ascii_whitespace() {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::NonSpace => {
            if pos < text.len() && !text[pos].is_ascii_whitespace() {
                match_tokens(rest, text, pos + 1)
            } else {
                None
            }
        }
        Re::CharClass(negated, ranges) => {
            if pos < text.len() {
                let c = text[pos];
                let in_class = ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
                if in_class != *negated {
                    match_tokens(rest, text, pos + 1)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Re::Star(inner) => {
            // Greedy: match as many as possible, then backtrack.
            let mut positions = vec![pos];
            let mut p = pos;
            while p < text.len() {
                if let Some(next) = match_single(inner, text, p) {
                    p = next;
                    positions.push(p);
                } else {
                    break;
                }
            }
            // Try from longest match down to zero.
            for &end_pos in positions.iter().rev() {
                if let Some(result) = match_tokens(rest, text, end_pos) {
                    return Some(result);
                }
            }
            None
        }
        Re::Plus(inner) => {
            // One-or-more: must match at least once.
            let mut positions = Vec::new();
            let mut p = pos;
            while p < text.len() {
                if let Some(next) = match_single(inner, text, p) {
                    p = next;
                    positions.push(p);
                } else {
                    break;
                }
            }
            for &end_pos in positions.iter().rev() {
                if let Some(result) = match_tokens(rest, text, end_pos) {
                    return Some(result);
                }
            }
            None
        }
        Re::Question(inner) => {
            // Zero-or-one: try with match first, then without.
            if let Some(next) = match_single(inner, text, pos)
                && let Some(result) = match_tokens(rest, text, next)
            {
                return Some(result);
            }
            match_tokens(rest, text, pos)
        }
        Re::Group(inner) => {
            // Try matching the group tokens, then the rest.
            if let Some(after_group) = match_tokens(inner, text, pos) {
                match_tokens(rest, text, after_group)
            } else {
                None
            }
        }
        Re::Alternation(branches) => {
            for branch in branches {
                // Concatenate branch tokens with the rest.
                let mut combined = branch.clone();
                combined.extend_from_slice(rest);
                if let Some(result) = match_tokens(&combined, text, pos) {
                    return Some(result);
                }
            }
            None
        }
    }
}

/// Match a single token (not a quantifier) at `pos`. Return the next pos.
fn match_single(tok: &Re, text: &[u8], pos: usize) -> Option<usize> {
    match tok {
        Re::Literal(ch) => {
            if pos < text.len() && text[pos] == *ch { Some(pos + 1) } else { None }
        }
        Re::Dot => {
            if pos < text.len() && text[pos] != b'\n' { Some(pos + 1) } else { None }
        }
        Re::Digit => {
            if pos < text.len() && text[pos].is_ascii_digit() { Some(pos + 1) } else { None }
        }
        Re::NonDigit => {
            if pos < text.len() && !text[pos].is_ascii_digit() { Some(pos + 1) } else { None }
        }
        Re::Word => {
            if pos < text.len() && (text[pos].is_ascii_alphanumeric() || text[pos] == b'_') {
                Some(pos + 1)
            } else {
                None
            }
        }
        Re::NonWord => {
            if pos < text.len() && !(text[pos].is_ascii_alphanumeric() || text[pos] == b'_') {
                Some(pos + 1)
            } else {
                None
            }
        }
        Re::Space => {
            if pos < text.len() && text[pos].is_ascii_whitespace() { Some(pos + 1) } else { None }
        }
        Re::NonSpace => {
            if pos < text.len() && !text[pos].is_ascii_whitespace() { Some(pos + 1) } else { None }
        }
        Re::CharClass(negated, ranges) => {
            if pos < text.len() {
                let c = text[pos];
                let in_class = ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
                if in_class != *negated { Some(pos + 1) } else { None }
            } else {
                None
            }
        }
        Re::Group(inner) => match_tokens(inner, text, pos),
        Re::Alternation(branches) => {
            for branch in branches {
                if let Some(end) = match_tokens(branch, text, pos) {
                    return Some(end);
                }
            }
            None
        }
        Re::Star(_) | Re::Plus(_) | Re::Question(_)
        | Re::AnchorStart | Re::AnchorEnd => None,
    }
}

// ===========================================================================
// IP address parsing and CIDR matching
// ===========================================================================

/// Parsed IP address (v4 or v6).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IpAddr {
    V4([u8; 4]),
    V6([u16; 8]),
}

impl fmt::Display for IpAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpAddr::V4(octets) => {
                write!(f, "{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
            }
            IpAddr::V6(segs) => {
                // Simplified display: just colon-separated hex
                let parts: Vec<String> = segs.iter().map(|s| format!("{s:x}")).collect();
                write!(f, "{}", parts.join(":"))
            }
        }
    }
}

/// Parse an IPv4 address string.
fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in s.split('.') {
        if count >= 4 {
            return None;
        }
        let val: u8 = part.parse().ok()?;
        octets[count] = val;
        count += 1;
    }
    if count == 4 { Some(octets) } else { None }
}

/// Parse an IPv6 address string (simplified, no `::` expansion for brevity).
fn parse_ipv6(s: &str) -> Option<[u16; 8]> {
    let s = s.trim_start_matches('[').trim_end_matches(']');

    // Handle :: expansion
    if s.contains("::") {
        let parts: Vec<&str> = s.split("::").collect();
        if parts.len() > 2 {
            return None;
        }
        let left: Vec<&str> = if parts[0].is_empty() {
            Vec::new()
        } else {
            parts[0].split(':').collect()
        };
        let right: Vec<&str> = if parts.len() < 2 || parts[1].is_empty() {
            Vec::new()
        } else {
            parts[1].split(':').collect()
        };
        let fill = 8usize.checked_sub(left.len() + right.len())?;
        let mut segs = [0u16; 8];
        for (i, p) in left.iter().enumerate() {
            segs[i] = u16::from_str_radix(p, 16).ok()?;
        }
        for (i, p) in right.iter().enumerate() {
            segs[left.len() + fill + i] = u16::from_str_radix(p, 16).ok()?;
        }
        Some(segs)
    } else {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 8 {
            return None;
        }
        let mut segs = [0u16; 8];
        for (i, p) in parts.iter().enumerate() {
            segs[i] = u16::from_str_radix(p, 16).ok()?;
        }
        Some(segs)
    }
}

/// Parse an IP address (v4 or v6).
fn parse_ip(s: &str) -> Option<IpAddr> {
    let s = s.trim();
    if s.contains(':') {
        parse_ipv6(s).map(IpAddr::V6)
    } else {
        parse_ipv4(s).map(IpAddr::V4)
    }
}

/// A CIDR network.
#[derive(Debug, Clone)]
struct Cidr {
    addr: IpAddr,
    prefix_len: u8,
}

impl Cidr {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(slash_pos) = s.rfind('/') {
            let ip_part = &s[..slash_pos];
            let prefix_part = &s[slash_pos + 1..];
            let addr = parse_ip(ip_part)?;
            let prefix_len: u8 = prefix_part.parse().ok()?;
            let max = match &addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            if prefix_len > max {
                return None;
            }
            Some(Self { addr, prefix_len })
        } else {
            // Single IP, full prefix length
            let addr = parse_ip(s)?;
            let prefix_len = match &addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            Some(Self { addr, prefix_len })
        }
    }

    fn contains(&self, ip: &IpAddr) -> bool {
        match (&self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => {
                let net_u32 = u32::from_be_bytes(*net);
                let addr_u32 = u32::from_be_bytes(*addr);
                if self.prefix_len == 0 {
                    return true;
                }
                if self.prefix_len >= 32 {
                    return net_u32 == addr_u32;
                }
                let mask = !0u32 << (32 - self.prefix_len);
                (net_u32 & mask) == (addr_u32 & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(addr)) => {
                // Convert to 128-bit for comparison
                let mut net_bits = 0u128;
                let mut addr_bits = 0u128;
                for i in 0..8 {
                    net_bits = (net_bits << 16) | (net[i] as u128);
                    addr_bits = (addr_bits << 16) | (addr[i] as u128);
                }
                if self.prefix_len == 0 {
                    return true;
                }
                if self.prefix_len >= 128 {
                    return net_bits == addr_bits;
                }
                let mask = !0u128 << (128 - self.prefix_len);
                (net_bits & mask) == (addr_bits & mask)
            }
            _ => false,
        }
    }
}

// ===========================================================================
// Extract IPs from text
// ===========================================================================

/// The `<HOST>` placeholder regex for matching IPv4 addresses in log lines.
const HOST_IPV4_PATTERN: &str =
    "([0-9][0-9]?[0-9]?\\.[0-9][0-9]?[0-9]?\\.[0-9][0-9]?[0-9]?\\.[0-9][0-9]?[0-9]?)";

/// Extract the first IPv4 address from a string.
fn extract_ipv4(text: &str) -> Option<String> {
    let re = Regex::compile(HOST_IPV4_PATTERN).ok()?;
    let m = re.find(text)?;
    let candidate = &text[m.0..m.1];
    // Validate it's actually a valid IPv4
    if parse_ipv4(candidate).is_some() {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// Extract the first IPv6 address from a string.
fn extract_ipv6(text: &str) -> Option<String> {
    // Look for sequences of hex:hex:hex... patterns
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        // Try to parse an IPv6-like sequence starting at i
        if bytes[i].is_ascii_hexdigit() || bytes[i] == b':' {
            let start = i;
            while i < len && (bytes[i].is_ascii_hexdigit() || bytes[i] == b':') {
                i += 1;
            }
            let candidate = &text[start..i];
            if candidate.contains(':') && candidate.len() >= 3
                && parse_ipv6(candidate).is_some()
            {
                return Some(candidate.to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Extract the first IP (v4 or v6) from text.
fn extract_ip(text: &str) -> Option<String> {
    // Try IPv4 first (more common)
    if let Some(ip) = extract_ipv4(text) {
        return Some(ip);
    }
    extract_ipv6(text)
}

// ===========================================================================
// INI-style configuration parser
// ===========================================================================

/// A key-value map for one INI section.
type Section = HashMap<String, String>;

/// Parsed INI configuration with a DEFAULT section and named sections.
#[derive(Debug, Clone)]
struct IniConfig {
    defaults: Section,
    sections: HashMap<String, Section>,
}

impl IniConfig {
    fn parse(text: &str) -> Self {
        let mut defaults = Section::new();
        let mut sections = HashMap::new();
        let mut current_section: Option<String> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let name = trimmed[1..trimmed.len() - 1].trim().to_string();
                if name.eq_ignore_ascii_case("DEFAULT") {
                    current_section = None;
                } else {
                    current_section = Some(name.clone());
                    sections.entry(name).or_insert_with(Section::new);
                }
                continue;
            }
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim().to_lowercase();
                let val = trimmed[eq_pos + 1..].trim().to_string();
                match &current_section {
                    None => { defaults.insert(key, val); }
                    Some(sec) => {
                        sections.entry(sec.clone())
                            .or_insert_with(Section::new)
                            .insert(key, val);
                    }
                }
            }
        }

        Self { defaults, sections }
    }

    /// Get a value from a section, falling back to DEFAULT.
    fn get(&self, section: &str, key: &str) -> Option<&str> {
        let key_lower = key.to_lowercase();
        if let Some(sec) = self.sections.get(section)
            && let Some(val) = sec.get(&key_lower)
        {
            return Some(val.as_str());
        }
        self.defaults.get(&key_lower).map(|s| s.as_str())
    }

    /// Get a value or return a default.
    fn get_or<'a>(&'a self, section: &str, key: &str, default: &'a str) -> &'a str {
        self.get(section, key).unwrap_or(default)
    }
}

// ===========================================================================
// Filter definitions (regex patterns for known services)
// ===========================================================================

/// A filter definition with fail and ignore regexes.
#[derive(Debug, Clone)]
struct FilterDef {
    #[allow(dead_code)]
    name: String,
    failregex: Vec<String>,
    ignoreregex: Vec<String>,
}

/// Replace `<HOST>` placeholder with an IP-matching pattern.
fn expand_host_placeholder(pattern: &str) -> String {
    pattern.replace("<HOST>", HOST_IPV4_PATTERN)
}

/// Compile a filter pattern, expanding `<HOST>`.
fn compile_filter_pattern(pattern: &str) -> Result<Regex, RegexError> {
    let expanded = expand_host_placeholder(pattern);
    Regex::compile(&expanded)
}

/// Pre-built filter definitions for common services.
fn builtin_filters() -> HashMap<String, FilterDef> {
    let mut filters = HashMap::new();

    filters.insert("sshd".to_string(), FilterDef {
        name: "sshd".to_string(),
        failregex: vec![
            "Failed password for .* from <HOST>".to_string(),
            "Failed password for invalid user .* from <HOST>".to_string(),
            "Invalid user .* from <HOST>".to_string(),
            "Connection closed by authenticating user .* <HOST>".to_string(),
            "Disconnected from authenticating user .* <HOST>".to_string(),
            "authentication failure.*rhost=<HOST>".to_string(),
        ],
        ignoreregex: Vec::new(),
    });

    filters.insert("apache-auth".to_string(), FilterDef {
        name: "apache-auth".to_string(),
        failregex: vec![
            "client <HOST>.*authentication failure".to_string(),
            "user .* authentication failure for.*client <HOST>".to_string(),
            "user .* not found.*client <HOST>".to_string(),
        ],
        ignoreregex: Vec::new(),
    });

    filters.insert("nginx-http-auth".to_string(), FilterDef {
        name: "nginx-http-auth".to_string(),
        failregex: vec![
            "no user/password was provided for basic authentication.*client: <HOST>".to_string(),
            "user .* was not found.*client: <HOST>".to_string(),
            "user .* password mismatch.*client: <HOST>".to_string(),
        ],
        ignoreregex: Vec::new(),
    });

    filters.insert("postfix".to_string(), FilterDef {
        name: "postfix".to_string(),
        failregex: vec![
            "NOQUEUE: reject: RCPT from .*\\[<HOST>\\]".to_string(),
            "warning: .*\\[<HOST>\\]: SASL .* authentication failed".to_string(),
            "improper command pipelining after .* from .*\\[<HOST>\\]".to_string(),
        ],
        ignoreregex: Vec::new(),
    });

    filters.insert("dovecot".to_string(), FilterDef {
        name: "dovecot".to_string(),
        failregex: vec![
            "auth: Error.*no auth attempts.*rip=<HOST>".to_string(),
            "imap-login: Disconnected.*rip=<HOST>".to_string(),
            "pop3-login: Aborted login.*rip=<HOST>".to_string(),
        ],
        ignoreregex: Vec::new(),
    });

    filters.insert("named".to_string(), FilterDef {
        name: "named".to_string(),
        failregex: vec![
            "client.*<HOST>.*query .* denied".to_string(),
            "client.*<HOST>.*zone transfer .* denied".to_string(),
        ],
        ignoreregex: Vec::new(),
    });

    filters
}

/// Parse a filter definition from INI config text.
#[allow(dead_code)]
fn parse_filter_config(name: &str, text: &str) -> FilterDef {
    let config = IniConfig::parse(text);
    let mut failregex = Vec::new();
    let mut ignoreregex = Vec::new();

    if let Some(val) = config.get("Definition", "failregex") {
        for line in val.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                failregex.push(trimmed.to_string());
            }
        }
        // Also handle multiline by looking at raw continuation
    }
    // For single-line configs, also check defaults
    if failregex.is_empty()
        && let Some(val) = config.defaults.get("failregex")
    {
        for line in val.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                failregex.push(trimmed.to_string());
            }
        }
    }

    if let Some(val) = config.get("Definition", "ignoreregex") {
        for line in val.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                ignoreregex.push(trimmed.to_string());
            }
        }
    }

    FilterDef { name: name.to_string(), failregex, ignoreregex }
}

// ===========================================================================
// Ban action generators
// ===========================================================================

/// Supported ban action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BanAction {
    Nft,
    Iptables,
    Route,
}

impl BanAction {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "nft" | "nftables" => Some(Self::Nft),
            "iptables" => Some(Self::Iptables),
            "route" => Some(Self::Route),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Nft => "nft",
            Self::Iptables => "iptables",
            Self::Route => "route",
        }
    }

    /// Generate the ban command string for this action.
    fn ban_command(self, ip: &str, port: &str) -> String {
        match self {
            Self::Nft => format!(
                "nft add rule inet filter input ip saddr {ip} tcp dport {{{port}}} drop"
            ),
            Self::Iptables => format!(
                "iptables -I INPUT -s {ip} -p tcp --dport {port} -j DROP"
            ),
            Self::Route => format!(
                "ip route add blackhole {ip}/32"
            ),
        }
    }

    /// Generate the unban command string for this action.
    fn unban_command(self, ip: &str, port: &str) -> String {
        match self {
            Self::Nft => format!(
                "nft delete rule inet filter input ip saddr {ip} tcp dport {{{port}}} drop"
            ),
            Self::Iptables => format!(
                "iptables -D INPUT -s {ip} -p tcp --dport {port} -j DROP"
            ),
            Self::Route => format!(
                "ip route del blackhole {ip}/32"
            ),
        }
    }
}

// ===========================================================================
// Port resolution
// ===========================================================================

/// Resolve a port name or number to a numeric string.
fn resolve_port(port: &str) -> String {
    let mut parts = Vec::new();
    for p in port.split(',') {
        let p = p.trim();
        let resolved = match p.to_lowercase().as_str() {
            "ssh" => "22",
            "http" => "80",
            "https" => "443",
            "ftp" => "21",
            "smtp" => "25",
            "imap" => "143",
            "imaps" => "993",
            "pop3" => "110",
            "pop3s" => "995",
            "dns" => "53",
            "mysql" => "3306",
            "postgresql" | "postgres" => "5432",
            _ => p,
        };
        parts.push(resolved.to_string());
    }
    parts.join(",")
}

// ===========================================================================
// Failure tracking
// ===========================================================================

/// Record of a single failure event.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FailureRecord {
    ip: String,
    timestamp: u64,
}

/// Tracks failures and bans for one jail.
#[derive(Debug, Clone)]
struct JailState {
    /// Failure records per IP.
    failures: HashMap<String, Vec<u64>>,
    /// Currently banned IPs: ip -> unban_time.
    banned: HashMap<String, u64>,
    /// Whitelisted CIDRs.
    ignoreip: Vec<Cidr>,
    /// Total ban count (historical).
    total_banned: u64,
    /// Total failure count.
    total_failures: u64,
}

impl JailState {
    fn new(ignoreip: &[Cidr]) -> Self {
        Self {
            failures: HashMap::new(),
            banned: HashMap::new(),
            ignoreip: ignoreip.to_vec(),
            total_banned: 0,
            total_failures: 0,
        }
    }

    /// Check if an IP is whitelisted.
    fn is_ignored(&self, ip_str: &str) -> bool {
        if let Some(ip) = parse_ip(ip_str) {
            self.ignoreip.iter().any(|cidr| cidr.contains(&ip))
        } else {
            false
        }
    }

    /// Record a failure for an IP. Returns true if the IP should be banned.
    fn record_failure(&mut self, ip: &str, timestamp: u64, maxretry: u32, findtime: u64) -> bool {
        if self.is_ignored(ip) || self.banned.contains_key(ip) {
            return false;
        }

        self.total_failures += 1;
        let entries = self.failures.entry(ip.to_string()).or_default();
        entries.push(timestamp);

        // Purge old entries outside findtime window
        let cutoff = timestamp.saturating_sub(findtime);
        entries.retain(|&t| t >= cutoff);

        entries.len() >= maxretry as usize
    }

    /// Ban an IP. Returns the ban command.
    fn ban_ip(&mut self, ip: &str, bantime: u64, timestamp: u64) -> bool {
        if self.banned.contains_key(ip) {
            return false;
        }
        let unban_time = timestamp.saturating_add(bantime);
        self.banned.insert(ip.to_string(), unban_time);
        self.failures.remove(ip);
        self.total_banned += 1;
        true
    }

    /// Unban an IP. Returns true if the IP was banned.
    fn unban_ip(&mut self, ip: &str) -> bool {
        self.banned.remove(ip).is_some()
    }

    /// Expire bans that have passed their bantime. Returns list of unbanned IPs.
    fn expire_bans(&mut self, now: u64) -> Vec<String> {
        let mut expired = Vec::new();
        self.banned.retain(|ip, &mut unban_time| {
            if now >= unban_time {
                expired.push(ip.clone());
                false
            } else {
                true
            }
        });
        expired
    }

    /// Add an IP/CIDR to the ignore list.
    fn add_ignoreip(&mut self, cidr_str: &str) -> bool {
        if let Some(cidr) = Cidr::parse(cidr_str) {
            self.ignoreip.push(cidr);
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// Jail configuration
// ===========================================================================

/// A configured jail.
#[derive(Debug, Clone)]
struct JailConfig {
    name: String,
    enabled: bool,
    port: String,
    logpath: String,
    filter_name: String,
    maxretry: u32,
    findtime: u64,
    bantime: u64,
    banaction: BanAction,
}

/// Parse jails from INI config.
fn parse_jails(config: &IniConfig) -> Vec<JailConfig> {
    let default_bantime: u64 = config.get_or("DEFAULT", "bantime", "600")
        .parse().unwrap_or(600);
    let default_findtime: u64 = config.get_or("DEFAULT", "findtime", "600")
        .parse().unwrap_or(600);
    let default_maxretry: u32 = config.get_or("DEFAULT", "maxretry", "5")
        .parse().unwrap_or(5);
    let default_banaction_str = config.get_or("DEFAULT", "banaction", "nft");
    let default_banaction = BanAction::from_str(default_banaction_str).unwrap_or(BanAction::Nft);

    let mut jails = Vec::new();

    for name in config.sections.keys() {
        if name.eq_ignore_ascii_case("DEFAULT") {
            continue;
        }

        let enabled_str = config.get_or(name, "enabled", "false");
        let enabled = matches!(enabled_str.to_lowercase().as_str(), "true" | "yes" | "1");

        let port = config.get_or(name, "port", "0").to_string();
        let logpath = config.get_or(name, "logpath", "").to_string();
        let filter_name = config.get_or(name, "filter", name).to_string();

        let maxretry: u32 = config.get_or(name, "maxretry", &default_maxretry.to_string())
            .parse().unwrap_or(default_maxretry);
        let findtime: u64 = config.get_or(name, "findtime", &default_findtime.to_string())
            .parse().unwrap_or(default_findtime);
        let bantime: u64 = config.get_or(name, "bantime", &default_bantime.to_string())
            .parse().unwrap_or(default_bantime);
        let banaction_str = config.get_or(name, "banaction", default_banaction.name());
        let banaction = BanAction::from_str(banaction_str).unwrap_or(default_banaction);

        jails.push(JailConfig {
            name: name.clone(),
            enabled,
            port: resolve_port(&port),
            logpath,
            filter_name,
            maxretry,
            findtime,
            bantime,
            banaction,
        });
    }

    jails
}

/// Parse ignoreip from config into Cidr list.
fn parse_ignoreip(config: &IniConfig) -> Vec<Cidr> {
    let ignoreip_str = config.get_or("DEFAULT", "ignoreip", "127.0.0.1/8");
    ignoreip_str
        .split_whitespace()
        .filter_map(|s| Cidr::parse(s.trim_matches(',')))
        .collect()
}

// ===========================================================================
// Log line processing
// ===========================================================================

/// Check a log line against a filter's regexes.
/// Returns Some(ip) if the line matches a failregex and not an ignoreregex.
fn check_line(line: &str, filter: &FilterDef) -> Option<String> {
    // First check ignoreregex
    for pattern in &filter.ignoreregex {
        if let Ok(re) = compile_filter_pattern(pattern)
            && re.is_match(line)
        {
            return None;
        }
    }

    // Then check failregex
    for pattern in &filter.failregex {
        if let Ok(re) = compile_filter_pattern(pattern)
            && re.is_match(line)
        {
            return extract_ip(line);
        }
    }

    None
}

/// Process log lines and track failures.
struct LogProcessor {
    jail_config: JailConfig,
    filter: FilterDef,
    state: JailState,
}

impl LogProcessor {
    fn new(jail_config: JailConfig, filter: FilterDef, ignoreip: &[Cidr]) -> Self {
        Self {
            jail_config,
            filter,
            state: JailState::new(ignoreip),
        }
    }

    /// Process a single log line. Returns a list of ban actions (commands).
    fn process_line(&mut self, line: &str, timestamp: u64) -> Vec<String> {
        let mut actions = Vec::new();

        if let Some(ip) = check_line(line, &self.filter)
            && self.state.record_failure(
                &ip,
                timestamp,
                self.jail_config.maxretry,
                self.jail_config.findtime,
            )
            && self.state.ban_ip(&ip, self.jail_config.bantime, timestamp)
        {
            let cmd = self.jail_config.banaction.ban_command(
                &ip,
                &self.jail_config.port,
            );
            actions.push(cmd);
        }

        actions
    }

    /// Expire bans and return unban commands.
    fn expire_bans(&mut self, now: u64) -> Vec<String> {
        let expired = self.state.expire_bans(now);
        expired
            .iter()
            .map(|ip| {
                self.jail_config.banaction.unban_command(ip, &self.jail_config.port)
            })
            .collect()
    }
}

// ===========================================================================
// Server
// ===========================================================================

/// The fail2ban server: manages jails, processes logs, handles bans.
struct Server {
    /// Jail processors indexed by jail name.
    processors: HashMap<String, LogProcessor>,
    /// Overall status.
    running: bool,
    /// Configuration.
    config_text: String,
}

impl Server {
    fn new() -> Self {
        Self {
            processors: HashMap::new(),
            running: false,
            config_text: String::new(),
        }
    }

    fn load_config(&mut self, config_text: &str) {
        self.config_text = config_text.to_string();
        let config = IniConfig::parse(config_text);
        let ignoreip = parse_ignoreip(&config);
        let jails = parse_jails(&config);
        let filters = builtin_filters();

        self.processors.clear();

        for jail in jails {
            if !jail.enabled {
                continue;
            }
            let filter = filters.get(&jail.filter_name).cloned().unwrap_or(FilterDef {
                name: jail.filter_name.clone(),
                failregex: Vec::new(),
                ignoreregex: Vec::new(),
            });
            let proc = LogProcessor::new(jail.clone(), filter, &ignoreip);
            self.processors.insert(jail.name.clone(), proc);
        }
    }

    fn start(&mut self) {
        self.running = true;
    }

    #[allow(dead_code)]
    fn stop(&mut self) {
        self.running = false;
    }

    fn reload(&mut self) {
        let text = self.config_text.clone();
        self.load_config(&text);
        self.running = true;
    }

    fn status(&self) -> String {
        let jail_count = self.processors.len();
        let total_banned: usize = self.processors.values()
            .map(|p| p.state.banned.len())
            .sum();
        let jail_names: Vec<&str> = self.processors.keys().map(|s| s.as_str()).collect();

        let mut out = String::new();
        out.push_str("Status\n");
        out.push_str(&format!("|- Number of jail:\t{jail_count}\n"));
        out.push_str(&format!("|- Total banned:\t{total_banned}\n"));
        out.push_str(&format!("`- Jail list:\t\t{}\n", jail_names.join(", ")));
        out
    }

    fn jail_status(&self, jail_name: &str) -> Option<String> {
        let proc = self.processors.get(jail_name)?;
        let state = &proc.state;
        let config = &proc.jail_config;

        let banned_ips: Vec<&str> = state.banned.keys().map(|s| s.as_str()).collect();
        let current_failures: usize = state.failures.values().map(|v| v.len()).sum();

        let mut out = String::new();
        out.push_str(&format!("Status for the jail: {jail_name}\n"));
        out.push_str("|- Filter\n");
        out.push_str(&format!("|  |- Currently failed:\t{current_failures}\n"));
        out.push_str(&format!("|  `- Total failed:\t{}\n", state.total_failures));
        out.push_str("|- Action\n");
        out.push_str(&format!("|  |- Currently banned:\t{}\n", state.banned.len()));
        out.push_str(&format!("|  `- Total banned:\t{}\n", state.total_banned));
        out.push_str("|- Settings\n");
        out.push_str(&format!("|  |- bantime:\t\t{}\n", config.bantime));
        out.push_str(&format!("|  |- findtime:\t\t{}\n", config.findtime));
        out.push_str(&format!("|  `- maxretry:\t\t{}\n", config.maxretry));
        out.push_str(&format!("`- Banned IP list:\t{}\n", banned_ips.join(" ")));
        Some(out)
    }

    fn banned_list(&self) -> String {
        let mut out = String::new();
        for (name, proc) in &self.processors {
            if !proc.state.banned.is_empty() {
                let ips: Vec<&str> = proc.state.banned.keys().map(|s| s.as_str()).collect();
                out.push_str(&format!("{name}: {}\n", ips.join(", ")));
            }
        }
        if out.is_empty() {
            out.push_str("No banned IPs\n");
        }
        out
    }

    fn ban_ip(&mut self, jail_name: &str, ip: &str) -> String {
        if let Some(proc) = self.processors.get_mut(jail_name) {
            let now = now_secs();
            if proc.state.ban_ip(ip, proc.jail_config.bantime, now) {
                let cmd = proc.jail_config.banaction.ban_command(ip, &proc.jail_config.port);
                format!("Banned {ip} in jail {jail_name}\nAction: {cmd}\n")
            } else {
                format!("IP {ip} is already banned in jail {jail_name}\n")
            }
        } else {
            format!("Jail {jail_name} not found\n")
        }
    }

    fn unban_ip(&mut self, jail_name: &str, ip: &str) -> String {
        if let Some(proc) = self.processors.get_mut(jail_name) {
            if proc.state.unban_ip(ip) {
                let cmd = proc.jail_config.banaction.unban_command(ip, &proc.jail_config.port);
                format!("Unbanned {ip} in jail {jail_name}\nAction: {cmd}\n")
            } else {
                format!("IP {ip} is not banned in jail {jail_name}\n")
            }
        } else {
            format!("Jail {jail_name} not found\n")
        }
    }

    fn add_ignoreip(&mut self, jail_name: &str, ip: &str) -> String {
        if let Some(proc) = self.processors.get_mut(jail_name) {
            if proc.state.add_ignoreip(ip) {
                format!("Added {ip} to ignore list for jail {jail_name}\n")
            } else {
                format!("Invalid IP/CIDR: {ip}\n")
            }
        } else {
            format!("Jail {jail_name} not found\n")
        }
    }

    fn get_value(&self, jail_name: &str, key: &str) -> String {
        if let Some(proc) = self.processors.get(jail_name) {
            match key {
                "bantime" => format!("{}\n", proc.jail_config.bantime),
                "maxretry" => format!("{}\n", proc.jail_config.maxretry),
                "findtime" => format!("{}\n", proc.jail_config.findtime),
                _ => format!("Unknown key: {key}\n"),
            }
        } else {
            format!("Jail {jail_name} not found\n")
        }
    }

    /// Process a log file for a specific jail, reading all lines.
    fn process_logfile(&mut self, jail_name: &str) -> Vec<String> {
        let proc = match self.processors.get(jail_name) {
            Some(p) => p,
            None => return vec![format!("Jail {jail_name} not found")],
        };

        let logpath = proc.jail_config.logpath.clone();
        let content = match fs::read_to_string(&logpath) {
            Ok(c) => c,
            Err(e) => return vec![format!("Cannot read {logpath}: {e}")],
        };

        let now = now_secs();
        let proc = self.processors.get_mut(jail_name).unwrap();
        let mut actions = Vec::new();

        for line in content.lines() {
            actions.extend(proc.process_line(line, now));
        }

        actions
    }

    /// Run a single pass: process all jail log files and expire bans.
    fn run_pass(&mut self) -> Vec<String> {
        let now = now_secs();
        let mut all_actions = Vec::new();
        let jail_names: Vec<String> = self.processors.keys().cloned().collect();

        for name in &jail_names {
            // Expire old bans
            if let Some(proc) = self.processors.get_mut(name) {
                let unban_cmds = proc.expire_bans(now);
                all_actions.extend(unban_cmds);
            }
        }

        // Process each jail's log files
        for name in &jail_names {
            let actions = self.process_logfile(name);
            all_actions.extend(actions);
        }

        all_actions
    }
}

// ===========================================================================
// fail2ban-server main
// ===========================================================================

fn run_server(args: &[String]) -> i32 {
    let mut config_path = String::from("/etc/fail2ban/jail.conf");
    let mut foreground = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-c" | "--config" => {
                i += 1;
                if i < args.len() {
                    config_path = args[i].clone();
                } else {
                    eprintln!("fail2ban-server: -c requires an argument");
                    return 1;
                }
            }
            "-f" | "--foreground" => {
                foreground = true;
            }
            "--help" | "-h" => {
                println!("Usage: fail2ban-server [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -c, --config <path>    Configuration file (default: /etc/fail2ban/jail.conf)");
                println!("  -f, --foreground       Run in foreground");
                println!("  -h, --help             Show this help");
                println!("  --version              Show version");
                return 0;
            }
            "--version" => {
                println!("fail2ban-server 0.1.0 (OurOS)");
                return 0;
            }
            _ => {
                eprintln!("fail2ban-server: unknown option '{}'", args[i]);
                return 1;
            }
        }
        i += 1;
    }

    let config_text = match fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("fail2ban-server: cannot read config '{config_path}': {e}");
            return 1;
        }
    };

    let mut server = Server::new();
    server.load_config(&config_text);
    server.start();

    if foreground {
        println!("fail2ban-server started with {} jail(s)", server.processors.len());
        for name in server.processors.keys() {
            println!("  - jail: {name}");
        }
    }

    // In a real daemon, we'd enter a loop. For this utility, we do one pass.
    let actions = server.run_pass();
    for action in &actions {
        println!("ACTION: {action}");
    }

    print!("{}", server.status());
    0
}

// ===========================================================================
// fail2ban-client main
// ===========================================================================

fn run_client(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: fail2ban-client <command> [args...]");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  status [jail]              Show overall or jail-specific status");
        eprintln!("  start                      Start the server");
        eprintln!("  stop                       Stop the server");
        eprintln!("  reload                     Reload configuration");
        eprintln!("  set <jail> banip <ip>      Manually ban an IP");
        eprintln!("  set <jail> unbanip <ip>    Manually unban an IP");
        eprintln!("  set <jail> addignoreip <ip>  Add IP to whitelist");
        eprintln!("  get <jail> bantime         Get jail bantime");
        eprintln!("  get <jail> maxretry        Get jail maxretry");
        eprintln!("  get <jail> findtime        Get jail findtime");
        eprintln!("  banned                     List all banned IPs");
        return 1;
    }

    // The client in a real system would communicate with the server over
    // a socket. For this standalone utility, we simulate by loading config
    // and working locally.

    let mut config_path = String::from("/etc/fail2ban/jail.conf");

    // Check for -c flag before the command
    let mut cmd_start = 0;
    if args.len() >= 2 && (args[0] == "-c" || args[0] == "--config") {
        config_path = args[1].clone();
        cmd_start = 2;
    }

    let cmd_args = &args[cmd_start..];
    if cmd_args.is_empty() {
        eprintln!("fail2ban-client: no command specified");
        return 1;
    }

    // For interactive commands that need the server state, load config
    let config_text = fs::read_to_string(&config_path).unwrap_or_default();
    let mut server = Server::new();
    if !config_text.is_empty() {
        server.load_config(&config_text);
        server.start();
    }

    match cmd_args[0].as_str() {
        "status" => {
            if cmd_args.len() > 1 {
                let jail_name = &cmd_args[1];
                match server.jail_status(jail_name) {
                    Some(s) => print!("{s}"),
                    None => {
                        eprintln!("Jail '{jail_name}' not found");
                        return 1;
                    }
                }
            } else {
                print!("{}", server.status());
            }
        }
        "start" => {
            println!("Server started");
        }
        "stop" => {
            println!("Server stopped");
        }
        "reload" => {
            server.reload();
            println!("Server reloaded with {} jail(s)", server.processors.len());
        }
        "set" => {
            if cmd_args.len() < 4 {
                eprintln!("Usage: fail2ban-client set <jail> <action> <value>");
                return 1;
            }
            let jail = &cmd_args[1];
            let action = &cmd_args[2];
            let value = &cmd_args[3];
            match action.as_str() {
                "banip" => print!("{}", server.ban_ip(jail, value)),
                "unbanip" => print!("{}", server.unban_ip(jail, value)),
                "addignoreip" => print!("{}", server.add_ignoreip(jail, value)),
                _ => {
                    eprintln!("Unknown set action: {action}");
                    return 1;
                }
            }
        }
        "get" => {
            if cmd_args.len() < 3 {
                eprintln!("Usage: fail2ban-client get <jail> <key>");
                return 1;
            }
            let jail = &cmd_args[1];
            let key = &cmd_args[2];
            print!("{}", server.get_value(jail, key));
        }
        "banned" => {
            print!("{}", server.banned_list());
        }
        "--help" | "-h" | "help" => {
            println!("Usage: fail2ban-client [OPTIONS] <command> [args...]");
            println!();
            println!("Options:");
            println!("  -c, --config <path>    Configuration file");
            println!();
            println!("Commands:");
            println!("  status [jail]              Show overall or jail-specific status");
            println!("  start                      Start the server");
            println!("  stop                       Stop the server");
            println!("  reload                     Reload configuration");
            println!("  set <jail> banip <ip>      Manually ban an IP");
            println!("  set <jail> unbanip <ip>    Manually unban an IP");
            println!("  set <jail> addignoreip <ip>  Add IP to whitelist");
            println!("  get <jail> bantime         Get jail bantime");
            println!("  get <jail> maxretry        Get jail maxretry");
            println!("  get <jail> findtime        Get jail findtime");
            println!("  banned                     List all banned IPs");
        }
        "--version" => {
            println!("fail2ban-client 0.1.0 (OurOS)");
        }
        _ => {
            eprintln!("fail2ban-client: unknown command '{}'", cmd_args[0]);
            return 1;
        }
    }

    0
}

// ===========================================================================
// fail2ban-regex main
// ===========================================================================

fn run_regex(args: &[String]) -> i32 {
    let mut print_all_matched = false;
    let mut print_all_missed = false;
    let mut positional: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--print-all-matched" => print_all_matched = true,
            "--print-all-missed" => print_all_missed = true,
            "--help" | "-h" => {
                println!("Usage: fail2ban-regex [OPTIONS] <logfile|logline> <filter|regex> [ignoreregex]");
                println!();
                println!("Options:");
                println!("  --print-all-matched    Print all matched lines");
                println!("  --print-all-missed     Print all missed (unmatched) lines");
                println!("  -h, --help             Show this help");
                println!("  --version              Show version");
                println!();
                println!("If <logfile> is a file path, lines are read from it.");
                println!("Otherwise the argument is treated as a single log line.");
                println!();
                println!("If <filter> is a known filter name (sshd, apache-auth, etc),");
                println!("the built-in patterns are used. Otherwise it is treated as a regex.");
                return 0;
            }
            "--version" => {
                println!("fail2ban-regex 0.1.0 (OurOS)");
                return 0;
            }
            _ => positional.push(arg.clone()),
        }
    }

    if positional.len() < 2 {
        eprintln!("Usage: fail2ban-regex <logfile|logline> <filter|regex> [ignoreregex]");
        return 1;
    }

    // Get log lines
    let log_input = &positional[0];
    let lines: Vec<String> = if std::path::Path::new(log_input).is_file() {
        match fs::read_to_string(log_input) {
            Ok(content) => content.lines().map(|l| l.to_string()).collect(),
            Err(e) => {
                eprintln!("fail2ban-regex: cannot read '{log_input}': {e}");
                return 1;
            }
        }
    } else {
        vec![log_input.clone()]
    };

    // Get filter
    let filter_input = &positional[1];
    let filters = builtin_filters();
    let failregex_patterns: Vec<String> = if let Some(filter) = filters.get(filter_input) {
        filter.failregex.clone()
    } else {
        vec![filter_input.clone()]
    };

    // Get ignore regex (optional)
    let ignoreregex_patterns: Vec<String> = if positional.len() > 2 {
        vec![positional[2].clone()]
    } else {
        Vec::new()
    };

    // Compile patterns
    let mut compiled_fail: Vec<Regex> = Vec::new();
    for pattern in &failregex_patterns {
        let expanded = expand_host_placeholder(pattern);
        match Regex::compile(&expanded) {
            Ok(re) => compiled_fail.push(re),
            Err(e) => {
                eprintln!("fail2ban-regex: bad failregex '{pattern}': {e}");
                return 1;
            }
        }
    }

    let mut compiled_ignore: Vec<Regex> = Vec::new();
    for pattern in &ignoreregex_patterns {
        let expanded = expand_host_placeholder(pattern);
        match Regex::compile(&expanded) {
            Ok(re) => compiled_ignore.push(re),
            Err(e) => {
                eprintln!("fail2ban-regex: bad ignoreregex '{pattern}': {e}");
                return 1;
            }
        }
    }

    // Process lines
    let mut matched_count = 0u64;
    let mut missed_count = 0u64;
    let mut ignored_count = 0u64;
    let mut ips_found: HashMap<String, u64> = HashMap::new();
    let mut matched_lines: Vec<String> = Vec::new();
    let mut missed_lines: Vec<String> = Vec::new();

    for line in &lines {
        // Check ignore first
        let ignored = compiled_ignore.iter().any(|re| re.is_match(line));
        if ignored {
            ignored_count += 1;
            continue;
        }

        let mut line_matched = false;
        for re in &compiled_fail {
            if re.is_match(line) {
                line_matched = true;
                if let Some(ip) = extract_ip(line) {
                    *ips_found.entry(ip).or_insert(0) += 1;
                }
                break;
            }
        }

        if line_matched {
            matched_count += 1;
            if print_all_matched {
                matched_lines.push(line.clone());
            }
        } else {
            missed_count += 1;
            if print_all_missed {
                missed_lines.push(line.clone());
            }
        }
    }

    // Print report
    println!("Running tests");
    println!("=============");
    println!();
    println!("Use   failregex filter {} : {}", filter_input, failregex_patterns.join(" | "));
    if !ignoreregex_patterns.is_empty() {
        println!("Use   ignoreregex filter : {}", ignoreregex_patterns.join(" | "));
    }
    println!();

    if print_all_matched && !matched_lines.is_empty() {
        println!("Matched lines:");
        for line in &matched_lines {
            println!("  {line}");
        }
        println!();
    }
    if print_all_missed && !missed_lines.is_empty() {
        println!("Missed lines:");
        for line in &missed_lines {
            println!("  {line}");
        }
        println!();
    }

    println!("Results");
    println!("=======");
    println!();
    println!("Failregex: {} total", matched_count);
    if !ignoreregex_patterns.is_empty() {
        println!("Ignoreregex: {} total", ignored_count);
    }
    println!();
    println!("Lines: {} lines, {} matched, {} missed",
        lines.len(), matched_count, missed_count);
    if ignored_count > 0 {
        println!("[ignored] {} line(s)", ignored_count);
    }

    if !ips_found.is_empty() {
        println!();
        println!("IP addresses found:");
        let mut ip_list: Vec<(&String, &u64)> = ips_found.iter().collect();
        ip_list.sort_by(|a, b| b.1.cmp(a.1));
        for (ip, count) in ip_list {
            println!("  {ip}: {count} time(s)");
        }
    }

    0
}

// ===========================================================================
// Personality detection and main
// ===========================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("fail2ban-server");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "fail2ban-client" => run_client(&rest),
        "fail2ban-regex" => run_regex(&rest),
        _ => run_server(&rest),
    };

    process::exit(exit_code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Regex engine tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_regex_literal() {
        let re = Regex::compile("hello").unwrap();
        assert!(re.is_match("hello world"));
        assert!(re.is_match("say hello"));
        assert!(!re.is_match("HELLO"));
    }

    #[test]
    fn test_regex_dot() {
        let re = Regex::compile("h.llo").unwrap();
        assert!(re.is_match("hello"));
        assert!(re.is_match("hallo"));
        assert!(!re.is_match("hllo"));
    }

    #[test]
    fn test_regex_star() {
        let re = Regex::compile("ab*c").unwrap();
        assert!(re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(re.is_match("abbc"));
        assert!(re.is_match("abbbc"));
        assert!(!re.is_match("adc"));
    }

    #[test]
    fn test_regex_plus() {
        let re = Regex::compile("ab+c").unwrap();
        assert!(!re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(re.is_match("abbc"));
    }

    #[test]
    fn test_regex_question() {
        let re = Regex::compile("ab?c").unwrap();
        assert!(re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(!re.is_match("abbc"));
    }

    #[test]
    fn test_regex_anchor_start() {
        let re = Regex::compile("^hello").unwrap();
        assert!(re.is_match("hello world"));
        assert!(!re.is_match("say hello"));
    }

    #[test]
    fn test_regex_anchor_end() {
        let re = Regex::compile("world$").unwrap();
        assert!(re.is_match("hello world"));
        assert!(!re.is_match("world hello"));
    }

    #[test]
    fn test_regex_both_anchors() {
        let re = Regex::compile("^exact$").unwrap();
        assert!(re.is_match("exact"));
        assert!(!re.is_match("not exact"));
        assert!(!re.is_match("exact not"));
    }

    #[test]
    fn test_regex_digit() {
        let re = Regex::compile("\\d+").unwrap();
        assert!(re.is_match("abc123"));
        assert!(!re.is_match("abcdef"));
    }

    #[test]
    fn test_regex_word() {
        let re = Regex::compile("\\w+").unwrap();
        assert!(re.is_match("hello"));
        assert!(re.is_match("test_123"));
    }

    #[test]
    fn test_regex_space() {
        let re = Regex::compile("hello\\sworld").unwrap();
        assert!(re.is_match("hello world"));
        assert!(!re.is_match("helloworld"));
    }

    #[test]
    fn test_regex_non_digit() {
        let re = Regex::compile("^\\D+$").unwrap();
        assert!(re.is_match("abcdef"));
        assert!(!re.is_match("abc123"));
    }

    #[test]
    fn test_regex_non_word() {
        let re = Regex::compile("\\W").unwrap();
        assert!(re.is_match("hello world"));
        assert!(!re.is_match("helloworld"));
    }

    #[test]
    fn test_regex_non_space() {
        let re = Regex::compile("\\S+").unwrap();
        assert!(re.is_match("hello"));
        assert!(re.is_match("  hello  "));
    }

    #[test]
    fn test_regex_char_class() {
        let re = Regex::compile("[abc]").unwrap();
        assert!(re.is_match("a"));
        assert!(re.is_match("b"));
        assert!(re.is_match("c"));
        assert!(!re.is_match("d"));
    }

    #[test]
    fn test_regex_char_class_range() {
        let re = Regex::compile("[a-z]+").unwrap();
        assert!(re.is_match("hello"));
        assert!(!re.is_match("12345"));
    }

    #[test]
    fn test_regex_char_class_negated() {
        let re = Regex::compile("[^0-9]+").unwrap();
        assert!(re.is_match("hello"));
    }

    #[test]
    fn test_regex_char_class_negated_no_match() {
        let re = Regex::compile("^[^a-z]+$").unwrap();
        assert!(re.is_match("12345"));
        assert!(!re.is_match("abc"));
    }

    #[test]
    fn test_regex_group() {
        let re = Regex::compile("(ab)+").unwrap();
        assert!(re.is_match("ab"));
        assert!(re.is_match("abab"));
        assert!(!re.is_match("ba"));
    }

    #[test]
    fn test_regex_alternation() {
        let re = Regex::compile("cat|dog").unwrap();
        assert!(re.is_match("cat"));
        assert!(re.is_match("dog"));
        assert!(!re.is_match("fish"));
    }

    #[test]
    fn test_regex_alternation_in_group() {
        let re = Regex::compile("(cat|dog)s").unwrap();
        assert!(re.is_match("cats"));
        assert!(re.is_match("dogs"));
        assert!(!re.is_match("fish"));
    }

    #[test]
    fn test_regex_escaped_special() {
        let re = Regex::compile("a\\.b").unwrap();
        assert!(re.is_match("a.b"));
        assert!(!re.is_match("axb"));
    }

    #[test]
    fn test_regex_empty_pattern() {
        let re = Regex::compile("").unwrap();
        assert!(re.is_match("anything"));
        assert!(re.is_match(""));
    }

    #[test]
    fn test_regex_complex_pattern() {
        let re = Regex::compile("Failed password for .* from [0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+").unwrap();
        assert!(re.is_match("Failed password for root from 192.168.1.100 port 22"));
        assert!(!re.is_match("Accepted password for root from 192.168.1.100"));
    }

    #[test]
    fn test_regex_find() {
        let re = Regex::compile("\\d+").unwrap();
        let m = re.find("abc 123 def").unwrap();
        assert_eq!(&"abc 123 def"[m.0..m.1], "123");
    }

    #[test]
    fn test_regex_find_all() {
        let re = Regex::compile("[0-9]+").unwrap();
        let matches = re.find_all("12 ab 34 cd 56");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_regex_dot_star() {
        let re = Regex::compile("a.*z").unwrap();
        assert!(re.is_match("abcdefz"));
        assert!(re.is_match("az"));
        assert!(!re.is_match("abc"));
    }

    #[test]
    fn test_regex_error_unmatched_paren() {
        assert!(Regex::compile("(abc").is_err());
        assert!(Regex::compile("abc)").is_err());
    }

    #[test]
    fn test_regex_error_unmatched_bracket() {
        assert!(Regex::compile("[abc").is_err());
    }

    #[test]
    fn test_regex_error_nothing_to_repeat() {
        assert!(Regex::compile("*abc").is_err());
        assert!(Regex::compile("+abc").is_err());
    }

    #[test]
    fn test_regex_bracket_literal() {
        // ] right after [ is literal
        let re = Regex::compile("[]abc]").unwrap();
        assert!(re.is_match("]"));
        assert!(re.is_match("a"));
    }

    #[test]
    fn test_regex_escaped_in_class() {
        let re = Regex::compile("[\\-a]").unwrap();
        assert!(re.is_match("-"));
        assert!(re.is_match("a"));
    }

    #[test]
    fn test_regex_greedy_backtrack() {
        let re = Regex::compile("a.*b.*c").unwrap();
        assert!(re.is_match("aXXXbYYYc"));
        assert!(!re.is_match("aXXX"));
    }

    // -----------------------------------------------------------------------
    // IP parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_ipv4("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(parse_ipv4("255.255.255.255"), Some([255, 255, 255, 255]));
    }

    #[test]
    fn test_parse_ipv4_invalid() {
        assert!(parse_ipv4("192.168.1").is_none());
        assert!(parse_ipv4("192.168.1.256").is_none());
        assert!(parse_ipv4("not.an.ip.addr").is_none());
    }

    #[test]
    fn test_parse_ipv6() {
        let result = parse_ipv6("2001:0db8:85a3:0000:0000:8a2e:0370:7334");
        assert!(result.is_some());
        assert_eq!(result.unwrap()[0], 0x2001);
    }

    #[test]
    fn test_parse_ipv6_compressed() {
        let result = parse_ipv6("::1");
        assert!(result.is_some());
        let segs = result.unwrap();
        assert_eq!(segs[7], 1);
        assert_eq!(segs[0], 0);
    }

    #[test]
    fn test_parse_ipv6_double_colon_middle() {
        let result = parse_ipv6("fe80::1");
        assert!(result.is_some());
        let segs = result.unwrap();
        assert_eq!(segs[0], 0xfe80);
        assert_eq!(segs[7], 1);
    }

    #[test]
    fn test_parse_ip() {
        assert_eq!(parse_ip("10.0.0.1"), Some(IpAddr::V4([10, 0, 0, 1])));
        assert!(matches!(parse_ip("::1"), Some(IpAddr::V6(_))));
    }

    #[test]
    fn test_ip_display() {
        let v4 = IpAddr::V4([192, 168, 1, 1]);
        assert_eq!(format!("{v4}"), "192.168.1.1");
    }

    // -----------------------------------------------------------------------
    // CIDR tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cidr_parse_v4() {
        let cidr = Cidr::parse("192.168.1.0/24").unwrap();
        assert_eq!(cidr.prefix_len, 24);
    }

    #[test]
    fn test_cidr_parse_single_ip() {
        let cidr = Cidr::parse("10.0.0.1").unwrap();
        assert_eq!(cidr.prefix_len, 32);
    }

    #[test]
    fn test_cidr_contains_v4() {
        let cidr = Cidr::parse("192.168.1.0/24").unwrap();
        assert!(cidr.contains(&IpAddr::V4([192, 168, 1, 1])));
        assert!(cidr.contains(&IpAddr::V4([192, 168, 1, 254])));
        assert!(!cidr.contains(&IpAddr::V4([192, 168, 2, 1])));
    }

    #[test]
    fn test_cidr_contains_v4_8() {
        let cidr = Cidr::parse("127.0.0.1/8").unwrap();
        assert!(cidr.contains(&IpAddr::V4([127, 0, 0, 1])));
        assert!(cidr.contains(&IpAddr::V4([127, 255, 255, 255])));
        assert!(!cidr.contains(&IpAddr::V4([128, 0, 0, 1])));
    }

    #[test]
    fn test_cidr_contains_v4_32() {
        let cidr = Cidr::parse("10.0.0.5/32").unwrap();
        assert!(cidr.contains(&IpAddr::V4([10, 0, 0, 5])));
        assert!(!cidr.contains(&IpAddr::V4([10, 0, 0, 6])));
    }

    #[test]
    fn test_cidr_contains_v4_0() {
        let cidr = Cidr::parse("0.0.0.0/0").unwrap();
        assert!(cidr.contains(&IpAddr::V4([1, 2, 3, 4])));
        assert!(cidr.contains(&IpAddr::V4([255, 255, 255, 255])));
    }

    #[test]
    fn test_cidr_contains_v6() {
        let cidr = Cidr::parse("fe80::/10").unwrap();
        assert!(cidr.contains(&parse_ip("fe80::1").unwrap()));
        assert!(!cidr.contains(&parse_ip("2001:db8::1").unwrap()));
    }

    #[test]
    fn test_cidr_v4_v6_mismatch() {
        let cidr = Cidr::parse("192.168.0.0/16").unwrap();
        assert!(!cidr.contains(&parse_ip("::1").unwrap()));
    }

    #[test]
    fn test_cidr_invalid_prefix() {
        assert!(Cidr::parse("192.168.0.0/33").is_none());
        assert!(Cidr::parse("::1/129").is_none());
    }

    // -----------------------------------------------------------------------
    // IP extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_ipv4_from_log() {
        let line = "Failed password for root from 192.168.1.100 port 22 ssh2";
        assert_eq!(extract_ipv4(line), Some("192.168.1.100".to_string()));
    }

    #[test]
    fn test_extract_ipv4_none() {
        assert_eq!(extract_ipv4("no ip here"), None);
    }

    #[test]
    fn test_extract_ip_v4() {
        let line = "Connection from 10.0.0.5";
        assert_eq!(extract_ip(line), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_extract_ipv6_from_text() {
        let line = "connection from 2001:db8:85a3:0000:0000:8a2e:0370:7334 denied";
        let ip = extract_ipv6(line);
        assert!(ip.is_some());
    }

    // -----------------------------------------------------------------------
    // INI config parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ini_parse_basic() {
        let text = "[DEFAULT]\nbantime = 600\nmaxretry = 5\n";
        let config = IniConfig::parse(text);
        assert_eq!(config.defaults.get("bantime"), Some(&"600".to_string()));
        assert_eq!(config.defaults.get("maxretry"), Some(&"5".to_string()));
    }

    #[test]
    fn test_ini_parse_sections() {
        let text = "[DEFAULT]\nbantime = 600\n\n[sshd]\nenabled = true\nport = ssh\n";
        let config = IniConfig::parse(text);
        assert!(config.sections.contains_key("sshd"));
        assert_eq!(config.get("sshd", "enabled"), Some("true"));
        assert_eq!(config.get("sshd", "port"), Some("ssh"));
    }

    #[test]
    fn test_ini_fallback_to_default() {
        let text = "[DEFAULT]\nbantime = 600\n\n[sshd]\nenabled = true\n";
        let config = IniConfig::parse(text);
        assert_eq!(config.get("sshd", "bantime"), Some("600"));
    }

    #[test]
    fn test_ini_section_override() {
        let text = "[DEFAULT]\nmaxretry = 5\n\n[sshd]\nmaxretry = 3\n";
        let config = IniConfig::parse(text);
        assert_eq!(config.get("sshd", "maxretry"), Some("3"));
    }

    #[test]
    fn test_ini_comments() {
        let text = "# comment\n; another comment\n[DEFAULT]\nkey = val\n";
        let config = IniConfig::parse(text);
        assert_eq!(config.defaults.get("key"), Some(&"val".to_string()));
    }

    #[test]
    fn test_ini_get_or() {
        let config = IniConfig::parse("");
        assert_eq!(config.get_or("any", "key", "default"), "default");
    }

    #[test]
    fn test_ini_case_insensitive_keys() {
        let text = "[DEFAULT]\nBanTime = 600\n";
        let config = IniConfig::parse(text);
        assert_eq!(config.get("DEFAULT", "bantime"), Some("600"));
    }

    // -----------------------------------------------------------------------
    // Filter tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builtin_filters_exist() {
        let filters = builtin_filters();
        assert!(filters.contains_key("sshd"));
        assert!(filters.contains_key("apache-auth"));
        assert!(filters.contains_key("nginx-http-auth"));
        assert!(filters.contains_key("postfix"));
        assert!(filters.contains_key("dovecot"));
        assert!(filters.contains_key("named"));
    }

    #[test]
    fn test_sshd_filter_patterns() {
        let filters = builtin_filters();
        let sshd = &filters["sshd"];
        assert!(!sshd.failregex.is_empty());
    }

    #[test]
    fn test_host_placeholder_expansion() {
        let expanded = expand_host_placeholder("Failed from <HOST> port");
        assert!(expanded.contains("[0-9]"));
        assert!(!expanded.contains("<HOST>"));
    }

    #[test]
    fn test_compile_filter_pattern() {
        let re = compile_filter_pattern("Failed password for .* from <HOST>").unwrap();
        assert!(re.is_match("Failed password for root from 192.168.1.1 port 22"));
    }

    #[test]
    fn test_check_line_sshd_match() {
        let filters = builtin_filters();
        let sshd = &filters["sshd"];
        let line = "Jan 1 00:00:00 server sshd[1234]: Failed password for root from 192.168.1.100 port 22 ssh2";
        let ip = check_line(line, sshd);
        assert_eq!(ip, Some("192.168.1.100".to_string()));
    }

    #[test]
    fn test_check_line_sshd_invalid_user() {
        let filters = builtin_filters();
        let sshd = &filters["sshd"];
        let line = "Jan 1 00:00:00 server sshd[1234]: Invalid user admin from 10.0.0.5 port 22";
        let ip = check_line(line, sshd);
        assert_eq!(ip, Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_check_line_no_match() {
        let filters = builtin_filters();
        let sshd = &filters["sshd"];
        let line = "Jan 1 00:00:00 server sshd[1234]: Accepted password for root from 192.168.1.1";
        let ip = check_line(line, sshd);
        assert_eq!(ip, None);
    }

    #[test]
    fn test_check_line_with_ignoreregex() {
        let filter = FilterDef {
            name: "test".to_string(),
            failregex: vec!["Failed from <HOST>".to_string()],
            ignoreregex: vec!["internal".to_string()],
        };
        let line_match = "Failed from 10.0.0.1 external";
        let line_ignore = "Failed from 10.0.0.1 internal";
        assert!(check_line(line_match, &filter).is_some());
        assert!(check_line(line_ignore, &filter).is_none());
    }

    #[test]
    fn test_parse_filter_config() {
        let text = "[Definition]\nfailregex = Failed from <HOST>\nignoreregex =\n";
        let filter = parse_filter_config("test", text);
        assert_eq!(filter.name, "test");
        assert_eq!(filter.failregex.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Ban action tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ban_action_nft() {
        let cmd = BanAction::Nft.ban_command("192.168.1.1", "22");
        assert!(cmd.contains("nft"));
        assert!(cmd.contains("192.168.1.1"));
        assert!(cmd.contains("22"));
    }

    #[test]
    fn test_ban_action_iptables() {
        let cmd = BanAction::Iptables.ban_command("10.0.0.1", "80,443");
        assert!(cmd.contains("iptables"));
        assert!(cmd.contains("10.0.0.1"));
    }

    #[test]
    fn test_ban_action_route() {
        let cmd = BanAction::Route.ban_command("10.0.0.1", "22");
        assert!(cmd.contains("blackhole"));
        assert!(cmd.contains("10.0.0.1"));
    }

    #[test]
    fn test_unban_action_nft() {
        let cmd = BanAction::Nft.unban_command("192.168.1.1", "22");
        assert!(cmd.contains("delete"));
        assert!(cmd.contains("192.168.1.1"));
    }

    #[test]
    fn test_unban_action_iptables() {
        let cmd = BanAction::Iptables.unban_command("10.0.0.1", "80");
        assert!(cmd.contains("-D"));
        assert!(cmd.contains("10.0.0.1"));
    }

    #[test]
    fn test_unban_action_route() {
        let cmd = BanAction::Route.unban_command("10.0.0.1", "22");
        assert!(cmd.contains("del"));
    }

    #[test]
    fn test_ban_action_from_str() {
        assert_eq!(BanAction::from_str("nft"), Some(BanAction::Nft));
        assert_eq!(BanAction::from_str("nftables"), Some(BanAction::Nft));
        assert_eq!(BanAction::from_str("iptables"), Some(BanAction::Iptables));
        assert_eq!(BanAction::from_str("route"), Some(BanAction::Route));
        assert_eq!(BanAction::from_str("unknown"), None);
    }

    #[test]
    fn test_ban_action_name() {
        assert_eq!(BanAction::Nft.name(), "nft");
        assert_eq!(BanAction::Iptables.name(), "iptables");
        assert_eq!(BanAction::Route.name(), "route");
    }

    // -----------------------------------------------------------------------
    // Port resolution tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_port_named() {
        assert_eq!(resolve_port("ssh"), "22");
        assert_eq!(resolve_port("http"), "80");
        assert_eq!(resolve_port("https"), "443");
    }

    #[test]
    fn test_resolve_port_numeric() {
        assert_eq!(resolve_port("8080"), "8080");
    }

    #[test]
    fn test_resolve_port_multiple() {
        assert_eq!(resolve_port("http,https"), "80,443");
    }

    #[test]
    fn test_resolve_port_mixed() {
        assert_eq!(resolve_port("ssh,8080"), "22,8080");
    }

    #[test]
    fn test_resolve_port_ftp() {
        assert_eq!(resolve_port("ftp"), "21");
    }

    #[test]
    fn test_resolve_port_smtp() {
        assert_eq!(resolve_port("smtp"), "25");
    }

    #[test]
    fn test_resolve_port_dns() {
        assert_eq!(resolve_port("dns"), "53");
    }

    // -----------------------------------------------------------------------
    // JailState tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_jail_state_new() {
        let state = JailState::new(&[]);
        assert!(state.failures.is_empty());
        assert!(state.banned.is_empty());
    }

    #[test]
    fn test_jail_state_record_failure() {
        let mut state = JailState::new(&[]);
        assert!(!state.record_failure("10.0.0.1", 1000, 3, 600));
        assert!(!state.record_failure("10.0.0.1", 1001, 3, 600));
        assert!(state.record_failure("10.0.0.1", 1002, 3, 600));
    }

    #[test]
    fn test_jail_state_failure_window_expiry() {
        let mut state = JailState::new(&[]);
        state.record_failure("10.0.0.1", 1000, 3, 60);
        state.record_failure("10.0.0.1", 1001, 3, 60);
        // This one is beyond the findtime window, so old entries expire
        assert!(!state.record_failure("10.0.0.1", 2000, 3, 60));
    }

    #[test]
    fn test_jail_state_ban_ip() {
        let mut state = JailState::new(&[]);
        assert!(state.ban_ip("10.0.0.1", 600, 1000));
        assert!(state.banned.contains_key("10.0.0.1"));
        assert_eq!(state.total_banned, 1);
    }

    #[test]
    fn test_jail_state_ban_ip_duplicate() {
        let mut state = JailState::new(&[]);
        assert!(state.ban_ip("10.0.0.1", 600, 1000));
        assert!(!state.ban_ip("10.0.0.1", 600, 1001));
    }

    #[test]
    fn test_jail_state_unban_ip() {
        let mut state = JailState::new(&[]);
        state.ban_ip("10.0.0.1", 600, 1000);
        assert!(state.unban_ip("10.0.0.1"));
        assert!(!state.banned.contains_key("10.0.0.1"));
    }

    #[test]
    fn test_jail_state_unban_not_banned() {
        let mut state = JailState::new(&[]);
        assert!(!state.unban_ip("10.0.0.1"));
    }

    #[test]
    fn test_jail_state_expire_bans() {
        let mut state = JailState::new(&[]);
        state.ban_ip("10.0.0.1", 60, 1000);
        state.ban_ip("10.0.0.2", 120, 1000);
        // At time 1061, first ban should expire but not second
        let expired = state.expire_bans(1061);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "10.0.0.1");
        assert!(state.banned.contains_key("10.0.0.2"));
    }

    #[test]
    fn test_jail_state_ignore_whitelisted() {
        let cidr = Cidr::parse("10.0.0.0/8").unwrap();
        let mut state = JailState::new(&[cidr]);
        assert!(!state.record_failure("10.0.0.1", 1000, 1, 600));
    }

    #[test]
    fn test_jail_state_is_ignored() {
        let cidr = Cidr::parse("192.168.1.0/24").unwrap();
        let state = JailState::new(&[cidr]);
        assert!(state.is_ignored("192.168.1.100"));
        assert!(!state.is_ignored("10.0.0.1"));
    }

    #[test]
    fn test_jail_state_add_ignoreip() {
        let mut state = JailState::new(&[]);
        assert!(!state.is_ignored("10.0.0.1"));
        assert!(state.add_ignoreip("10.0.0.0/8"));
        assert!(state.is_ignored("10.0.0.1"));
    }

    #[test]
    fn test_jail_state_add_ignoreip_invalid() {
        let mut state = JailState::new(&[]);
        assert!(!state.add_ignoreip("not-an-ip"));
    }

    #[test]
    fn test_jail_state_failure_clears_on_ban() {
        let mut state = JailState::new(&[]);
        state.record_failure("10.0.0.1", 1000, 3, 600);
        state.record_failure("10.0.0.1", 1001, 3, 600);
        state.ban_ip("10.0.0.1", 600, 1002);
        assert!(!state.failures.contains_key("10.0.0.1"));
    }

    #[test]
    fn test_jail_state_no_failure_if_banned() {
        let mut state = JailState::new(&[]);
        state.ban_ip("10.0.0.1", 600, 1000);
        assert!(!state.record_failure("10.0.0.1", 1001, 1, 600));
    }

    // -----------------------------------------------------------------------
    // Jail configuration parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_jails() {
        let text = "\
[DEFAULT]
bantime = 600
findtime = 600
maxretry = 5
banaction = nft

[sshd]
enabled = true
port = ssh
logpath = /var/log/auth.log
filter = sshd
maxretry = 3
";
        let config = IniConfig::parse(text);
        let jails = parse_jails(&config);
        assert_eq!(jails.len(), 1);
        assert_eq!(jails[0].name, "sshd");
        assert!(jails[0].enabled);
        assert_eq!(jails[0].port, "22");
        assert_eq!(jails[0].maxretry, 3);
        assert_eq!(jails[0].bantime, 600);
    }

    #[test]
    fn test_parse_jails_disabled() {
        let text = "[sshd]\nenabled = false\nport = ssh\n";
        let config = IniConfig::parse(text);
        let jails = parse_jails(&config);
        assert_eq!(jails.len(), 1);
        assert!(!jails[0].enabled);
    }

    #[test]
    fn test_parse_jails_multiple() {
        let text = "\
[DEFAULT]
bantime = 600

[sshd]
enabled = true
port = ssh
filter = sshd

[apache-auth]
enabled = true
port = http,https
filter = apache-auth
";
        let config = IniConfig::parse(text);
        let jails = parse_jails(&config);
        assert_eq!(jails.len(), 2);
    }

    #[test]
    fn test_parse_ignoreip() {
        let text = "[DEFAULT]\nignoreip = 127.0.0.1/8 192.168.1.0/24\n";
        let config = IniConfig::parse(text);
        let cidrs = parse_ignoreip(&config);
        assert_eq!(cidrs.len(), 2);
    }

    #[test]
    fn test_parse_ignoreip_default() {
        let config = IniConfig::parse("");
        let cidrs = parse_ignoreip(&config);
        assert_eq!(cidrs.len(), 1); // default 127.0.0.1/8
    }

    // -----------------------------------------------------------------------
    // LogProcessor tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_log_processor_basic() {
        let jail = JailConfig {
            name: "sshd".to_string(),
            enabled: true,
            port: "22".to_string(),
            logpath: "/var/log/auth.log".to_string(),
            filter_name: "sshd".to_string(),
            maxretry: 3,
            findtime: 600,
            bantime: 600,
            banaction: BanAction::Nft,
        };
        let filter = builtin_filters().remove("sshd").unwrap();
        let mut proc = LogProcessor::new(jail, filter, &[]);

        let actions = proc.process_line(
            "Failed password for root from 10.0.0.5 port 22 ssh2", 1000);
        assert!(actions.is_empty());

        let actions = proc.process_line(
            "Failed password for root from 10.0.0.5 port 22 ssh2", 1001);
        assert!(actions.is_empty());

        let actions = proc.process_line(
            "Failed password for root from 10.0.0.5 port 22 ssh2", 1002);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("10.0.0.5"));
    }

    #[test]
    fn test_log_processor_different_ips() {
        let jail = JailConfig {
            name: "sshd".to_string(),
            enabled: true,
            port: "22".to_string(),
            logpath: "/var/log/auth.log".to_string(),
            filter_name: "sshd".to_string(),
            maxretry: 2,
            findtime: 600,
            bantime: 600,
            banaction: BanAction::Iptables,
        };
        let filter = builtin_filters().remove("sshd").unwrap();
        let mut proc = LogProcessor::new(jail, filter, &[]);

        proc.process_line("Failed password for root from 10.0.0.1 port 22 ssh2", 1000);
        proc.process_line("Failed password for root from 10.0.0.2 port 22 ssh2", 1000);

        let actions = proc.process_line(
            "Failed password for root from 10.0.0.1 port 22 ssh2", 1001);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("10.0.0.1"));
        // 10.0.0.2 should not be banned yet
        assert!(!proc.state.banned.contains_key("10.0.0.2"));
    }

    #[test]
    fn test_log_processor_expire_bans() {
        let jail = JailConfig {
            name: "sshd".to_string(),
            enabled: true,
            port: "22".to_string(),
            logpath: "".to_string(),
            filter_name: "sshd".to_string(),
            maxretry: 1,
            findtime: 600,
            bantime: 60,
            banaction: BanAction::Nft,
        };
        let filter = builtin_filters().remove("sshd").unwrap();
        let mut proc = LogProcessor::new(jail, filter, &[]);

        proc.process_line("Failed password for root from 10.0.0.1 port 22 ssh2", 1000);
        assert!(proc.state.banned.contains_key("10.0.0.1"));

        let unban_cmds = proc.expire_bans(1061);
        assert_eq!(unban_cmds.len(), 1);
        assert!(!proc.state.banned.contains_key("10.0.0.1"));
    }

    #[test]
    fn test_log_processor_whitelist() {
        let cidr = Cidr::parse("10.0.0.0/8").unwrap();
        let jail = JailConfig {
            name: "sshd".to_string(),
            enabled: true,
            port: "22".to_string(),
            logpath: "".to_string(),
            filter_name: "sshd".to_string(),
            maxretry: 1,
            findtime: 600,
            bantime: 600,
            banaction: BanAction::Nft,
        };
        let filter = builtin_filters().remove("sshd").unwrap();
        let mut proc = LogProcessor::new(jail, filter, &[cidr]);

        let actions = proc.process_line(
            "Failed password for root from 10.0.0.5 port 22 ssh2", 1000);
        assert!(actions.is_empty());
        assert!(!proc.state.banned.contains_key("10.0.0.5"));
    }

    // -----------------------------------------------------------------------
    // Server tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_server_load_config() {
        let mut server = Server::new();
        let config = "\
[DEFAULT]
bantime = 600
maxretry = 5
banaction = nft

[sshd]
enabled = true
port = ssh
filter = sshd
logpath = /var/log/auth.log
";
        server.load_config(config);
        assert_eq!(server.processors.len(), 1);
        assert!(server.processors.contains_key("sshd"));
    }

    #[test]
    fn test_server_disabled_jails_not_loaded() {
        let mut server = Server::new();
        let config = "\
[sshd]
enabled = false
port = ssh
filter = sshd
logpath = /var/log/auth.log
";
        server.load_config(config);
        assert_eq!(server.processors.len(), 0);
    }

    #[test]
    fn test_server_status() {
        let mut server = Server::new();
        let config = "\
[sshd]
enabled = true
port = ssh
filter = sshd
";
        server.load_config(config);
        let status = server.status();
        assert!(status.contains("Number of jail"));
        assert!(status.contains("sshd"));
    }

    #[test]
    fn test_server_jail_status() {
        let mut server = Server::new();
        let config = "\
[sshd]
enabled = true
port = ssh
filter = sshd
maxretry = 3
bantime = 600
findtime = 600
";
        server.load_config(config);
        let status = server.jail_status("sshd").unwrap();
        assert!(status.contains("sshd"));
        assert!(status.contains("bantime"));
        assert!(status.contains("600"));
    }

    #[test]
    fn test_server_jail_status_not_found() {
        let server = Server::new();
        assert!(server.jail_status("nonexistent").is_none());
    }

    #[test]
    fn test_server_ban_ip() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        let result = server.ban_ip("sshd", "192.168.1.100");
        assert!(result.contains("Banned"));
        assert!(result.contains("192.168.1.100"));
    }

    #[test]
    fn test_server_ban_ip_already_banned() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        server.ban_ip("sshd", "192.168.1.100");
        let result = server.ban_ip("sshd", "192.168.1.100");
        assert!(result.contains("already banned"));
    }

    #[test]
    fn test_server_unban_ip() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        server.ban_ip("sshd", "192.168.1.100");
        let result = server.unban_ip("sshd", "192.168.1.100");
        assert!(result.contains("Unbanned"));
    }

    #[test]
    fn test_server_unban_not_banned() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        let result = server.unban_ip("sshd", "192.168.1.100");
        assert!(result.contains("not banned"));
    }

    #[test]
    fn test_server_add_ignoreip() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        let result = server.add_ignoreip("sshd", "172.16.0.0/12");
        assert!(result.contains("Added"));
    }

    #[test]
    fn test_server_get_value() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\nmaxretry = 3\nbantime = 600\nfindtime = 300\n");
        assert_eq!(server.get_value("sshd", "bantime").trim(), "600");
        assert_eq!(server.get_value("sshd", "maxretry").trim(), "3");
        assert_eq!(server.get_value("sshd", "findtime").trim(), "300");
    }

    #[test]
    fn test_server_get_value_unknown() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        let result = server.get_value("sshd", "unknown");
        assert!(result.contains("Unknown"));
    }

    #[test]
    fn test_server_banned_list_empty() {
        let server = Server::new();
        let result = server.banned_list();
        assert!(result.contains("No banned"));
    }

    #[test]
    fn test_server_banned_list() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        server.ban_ip("sshd", "10.0.0.1");
        let result = server.banned_list();
        assert!(result.contains("10.0.0.1"));
    }

    #[test]
    fn test_server_reload() {
        let mut server = Server::new();
        let config = "[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n";
        server.load_config(config);
        server.start();
        server.reload();
        assert!(server.running);
        assert_eq!(server.processors.len(), 1);
    }

    #[test]
    fn test_server_start_stop() {
        let mut server = Server::new();
        assert!(!server.running);
        server.start();
        assert!(server.running);
        server.stop();
        assert!(!server.running);
    }

    // -----------------------------------------------------------------------
    // Integration tests: full pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_pipeline_ban() {
        let mut server = Server::new();
        let config = "\
[DEFAULT]
bantime = 600
findtime = 600
maxretry = 3
ignoreip = 127.0.0.1/8
banaction = nft

[sshd]
enabled = true
port = ssh
filter = sshd
maxretry = 3
";
        server.load_config(config);
        server.start();

        let proc = server.processors.get_mut("sshd").unwrap();
        proc.process_line("Failed password for root from 203.0.113.5 port 22 ssh2", 1000);
        proc.process_line("Failed password for root from 203.0.113.5 port 22 ssh2", 1001);
        let actions = proc.process_line(
            "Failed password for root from 203.0.113.5 port 22 ssh2", 1002);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("203.0.113.5"));
        assert!(proc.state.banned.contains_key("203.0.113.5"));
    }

    #[test]
    fn test_full_pipeline_ignore() {
        let mut server = Server::new();
        let config = "\
[DEFAULT]
ignoreip = 127.0.0.1/8

[sshd]
enabled = true
port = ssh
filter = sshd
maxretry = 1
";
        server.load_config(config);

        let proc = server.processors.get_mut("sshd").unwrap();
        let actions = proc.process_line(
            "Failed password for root from 127.0.0.1 port 22 ssh2", 1000);
        assert!(actions.is_empty());
        assert!(!proc.state.banned.contains_key("127.0.0.1"));
    }

    #[test]
    fn test_full_pipeline_expire() {
        let mut server = Server::new();
        let config = "\
[sshd]
enabled = true
port = ssh
filter = sshd
maxretry = 1
bantime = 60
";
        server.load_config(config);

        let proc = server.processors.get_mut("sshd").unwrap();
        proc.process_line("Failed password for root from 10.0.0.1 port 22 ssh2", 1000);
        assert!(proc.state.banned.contains_key("10.0.0.1"));

        let cmds = proc.expire_bans(1061);
        assert_eq!(cmds.len(), 1);
        assert!(!proc.state.banned.contains_key("10.0.0.1"));
    }

    #[test]
    fn test_full_pipeline_multiple_jails() {
        let mut server = Server::new();
        let config = "\
[DEFAULT]
maxretry = 1

[sshd]
enabled = true
port = ssh
filter = sshd

[apache-auth]
enabled = true
port = http,https
filter = apache-auth
";
        server.load_config(config);
        assert_eq!(server.processors.len(), 2);
    }

    #[test]
    fn test_personality_detection() {
        // Simulate argv[0] parsing
        let test_cases = vec![
            ("fail2ban-server", "fail2ban-server"),
            ("fail2ban-client", "fail2ban-client"),
            ("fail2ban-regex", "fail2ban-regex"),
            ("/usr/bin/fail2ban-server", "fail2ban-server"),
            ("C:\\Program Files\\fail2ban-client.exe", "fail2ban-client"),
            ("/opt/fail2ban-regex", "fail2ban-regex"),
        ];

        for (input, expected) in test_cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_now_secs_returns_nonzero() {
        // Basic sanity: timestamp should be > 0 on any modern system
        assert!(now_secs() > 0);
    }

    #[test]
    fn test_regex_star_group() {
        let re = Regex::compile("(ab)*c").unwrap();
        assert!(re.is_match("c"));
        assert!(re.is_match("abc"));
        assert!(re.is_match("ababc"));
    }

    #[test]
    fn test_regex_nested_groups() {
        let re = Regex::compile("((a|b)c)+").unwrap();
        assert!(re.is_match("ac"));
        assert!(re.is_match("bc"));
        assert!(re.is_match("acbc"));
    }

    #[test]
    fn test_cidr_16_prefix() {
        let cidr = Cidr::parse("172.16.0.0/12").unwrap();
        assert!(cidr.contains(&IpAddr::V4([172, 16, 0, 1])));
        assert!(cidr.contains(&IpAddr::V4([172, 31, 255, 255])));
        assert!(!cidr.contains(&IpAddr::V4([172, 32, 0, 1])));
    }

    #[test]
    fn test_server_ban_nonexistent_jail() {
        let mut server = Server::new();
        let result = server.ban_ip("nonexistent", "10.0.0.1");
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_server_unban_nonexistent_jail() {
        let mut server = Server::new();
        let result = server.unban_ip("nonexistent", "10.0.0.1");
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_server_get_nonexistent_jail() {
        let server = Server::new();
        let result = server.get_value("nonexistent", "bantime");
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_server_add_ignoreip_nonexistent() {
        let mut server = Server::new();
        let result = server.add_ignoreip("nonexistent", "10.0.0.0/8");
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_server_add_ignoreip_invalid() {
        let mut server = Server::new();
        server.load_config("[sshd]\nenabled = true\nport = ssh\nfilter = sshd\n");
        let result = server.add_ignoreip("sshd", "not-valid");
        assert!(result.contains("Invalid"));
    }

    #[test]
    fn test_apache_filter_match() {
        let filters = builtin_filters();
        let apache = &filters["apache-auth"];
        let line = "client 10.0.0.1 authentication failure for /admin";
        let ip = check_line(line, apache);
        assert_eq!(ip, Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_postfix_filter_match() {
        let filters = builtin_filters();
        let postfix = &filters["postfix"];
        let line = "warning: mail.example.com[192.168.1.50]: SASL LOGIN authentication failed";
        let ip = check_line(line, postfix);
        assert_eq!(ip, Some("192.168.1.50".to_string()));
    }

    #[test]
    fn test_regex_trailing_backslash_error() {
        assert!(Regex::compile("test\\").is_err());
    }

    #[test]
    fn test_regex_alternation_three_branches() {
        let re = Regex::compile("a|b|c").unwrap();
        assert!(re.is_match("a"));
        assert!(re.is_match("b"));
        assert!(re.is_match("c"));
        assert!(!re.is_match("d"));
    }

    #[test]
    fn test_regex_question_at_start() {
        let re = Regex::compile("a?b").unwrap();
        assert!(re.is_match("ab"));
        assert!(re.is_match("b"));
    }

    #[test]
    fn test_jail_total_failures_count() {
        let mut state = JailState::new(&[]);
        state.record_failure("10.0.0.1", 1000, 100, 600);
        state.record_failure("10.0.0.2", 1000, 100, 600);
        state.record_failure("10.0.0.1", 1001, 100, 600);
        assert_eq!(state.total_failures, 3);
    }

    #[test]
    fn test_regex_dot_doesnt_match_newline() {
        let re = Regex::compile("a.b").unwrap();
        assert!(!re.is_match("a\nb"));
        assert!(re.is_match("axb"));
    }
}
