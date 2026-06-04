//! OurOS stream editor (`sed`)
//!
//! A POSIX-compatible stream editor that reads input line by line, applies
//! editing commands (substitution, deletion, insertion, etc.), and writes the
//! result to stdout. Includes a built-in basic/extended regex engine.

use std::cell::Cell;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

// ---------------------------------------------------------------------------
// Regex engine -- simple backtracking matcher supporting basic & extended mode
// ---------------------------------------------------------------------------

/// A compiled regex node in the NFA-like pattern list.
#[derive(Debug, Clone)]
enum ReNode {
    /// Literal character.
    Literal(u8),
    /// `.` -- any character except newline.
    AnyChar,
    /// `^` -- start of line anchor.
    StartAnchor,
    /// `$` -- end of line anchor.
    EndAnchor,
    /// Character class `[...]` -- set of bytes, negated flag.
    CharClass { bytes: Vec<u8>, negated: bool },
    /// `*` -- zero or more of the previous node.
    Star(Box<ReNode>),
    /// `+` -- one or more of the previous node.
    Plus(Box<ReNode>),
    /// `?` -- zero or one of the previous node.
    Question(Box<ReNode>),
    /// Capture group open (1-based index).
    GroupStart(usize),
    /// Capture group close (1-based index).
    GroupEnd(usize),
}

/// A compiled regex pattern.
#[derive(Debug, Clone)]
struct Regex {
    nodes: Vec<ReNode>,
    /// Whether case-insensitive matching is enabled.
    ignore_case: bool,
}

/// A single match result with capture groups.
#[derive(Debug, Clone)]
struct MatchResult {
    /// Start offset in the haystack.
    start: usize,
    /// End offset (exclusive) in the haystack.
    end: usize,
    /// Capture groups (1-based index -> (start, end)).
    groups: Vec<Option<(usize, usize)>>,
}

/// Parse a character class body (everything between `[` and `]`).
/// Returns the set of bytes that belong to the class, and whether it is negated.
fn parse_char_class(pattern: &[u8], pos: &mut usize) -> Result<(Vec<u8>, bool), String> {
    let mut bytes = Vec::new();
    let negated = if *pos < pattern.len() && pattern[*pos] == b'^' {
        *pos += 1;
        true
    } else {
        false
    };
    // A `]` immediately after `[` or `[^` is treated as a literal.
    if *pos < pattern.len() && pattern[*pos] == b']' {
        bytes.push(b']');
        *pos += 1;
    }
    while *pos < pattern.len() && pattern[*pos] != b']' {
        // Range: a-z
        if *pos + 2 < pattern.len() && pattern[*pos + 1] == b'-' && pattern[*pos + 2] != b']' {
            let lo = pattern[*pos];
            let hi = pattern[*pos + 2];
            if lo <= hi {
                for c in lo..=hi {
                    bytes.push(c);
                }
            }
            *pos += 3;
        } else if pattern[*pos] == b'\\' && *pos + 1 < pattern.len() {
            *pos += 1;
            bytes.push(pattern[*pos]);
            *pos += 1;
        } else {
            bytes.push(pattern[*pos]);
            *pos += 1;
        }
    }
    if *pos < pattern.len() && pattern[*pos] == b']' {
        *pos += 1; // consume `]`
    } else {
        return Err("unterminated character class".into());
    }
    Ok((bytes, negated))
}

/// Compile a regex pattern string into a `Regex`.
fn compile_regex(pattern: &[u8], extended: bool, ignore_case: bool) -> Result<Regex, String> {
    let mut nodes: Vec<ReNode> = Vec::new();
    let mut pos = 0;
    let mut group_counter: usize = 0;

    while pos < pattern.len() {
        let ch = pattern[pos];
        match ch {
            b'^' => {
                nodes.push(ReNode::StartAnchor);
                pos += 1;
            }
            b'$' => {
                nodes.push(ReNode::EndAnchor);
                pos += 1;
            }
            b'.' => {
                nodes.push(ReNode::AnyChar);
                pos += 1;
            }
            b'[' => {
                pos += 1;
                let (bytes, negated) = parse_char_class(pattern, &mut pos)?;
                nodes.push(ReNode::CharClass { bytes, negated });
            }
            b'*' => {
                if let Some(prev) = nodes.pop() {
                    nodes.push(ReNode::Star(Box::new(prev)));
                }
                // else: leading * is literal in some implementations, ignore here
                pos += 1;
            }
            b'\\' => {
                pos += 1;
                if pos >= pattern.len() {
                    return Err("trailing backslash".into());
                }
                let next = pattern[pos];
                if !extended && next == b'(' {
                    group_counter += 1;
                    nodes.push(ReNode::GroupStart(group_counter));
                    pos += 1;
                } else if !extended && next == b')' {
                    // Find the matching group number by scanning back.
                    let gn = find_open_group(&nodes);
                    nodes.push(ReNode::GroupEnd(gn));
                    pos += 1;
                } else if !extended && next == b'+' {
                    // \+ in basic mode = one or more
                    if let Some(prev) = nodes.pop() {
                        nodes.push(ReNode::Plus(Box::new(prev)));
                    }
                    pos += 1;
                } else if !extended && next == b'?' {
                    if let Some(prev) = nodes.pop() {
                        nodes.push(ReNode::Question(Box::new(prev)));
                    }
                    pos += 1;
                } else if next == b'n' {
                    nodes.push(ReNode::Literal(b'\n'));
                    pos += 1;
                } else if next == b't' {
                    nodes.push(ReNode::Literal(b'\t'));
                    pos += 1;
                } else {
                    // Escaped literal (e.g. \., \\, \/)
                    nodes.push(ReNode::Literal(next));
                    pos += 1;
                }
            }
            b'(' if extended => {
                group_counter += 1;
                nodes.push(ReNode::GroupStart(group_counter));
                pos += 1;
            }
            b')' if extended => {
                let gn = find_open_group(&nodes);
                nodes.push(ReNode::GroupEnd(gn));
                pos += 1;
            }
            b'+' if extended => {
                if let Some(prev) = nodes.pop() {
                    nodes.push(ReNode::Plus(Box::new(prev)));
                }
                pos += 1;
            }
            b'?' if extended => {
                if let Some(prev) = nodes.pop() {
                    nodes.push(ReNode::Question(Box::new(prev)));
                }
                pos += 1;
            }
            _ => {
                nodes.push(ReNode::Literal(ch));
                pos += 1;
            }
        }
    }
    Ok(Regex { nodes, ignore_case })
}

/// Find the most recent unclosed group number by scanning the node list.
fn find_open_group(nodes: &[ReNode]) -> usize {
    let mut open_groups = Vec::new();
    for node in nodes {
        match node {
            ReNode::GroupStart(n) => open_groups.push(*n),
            ReNode::GroupEnd(n) => {
                if let Some(idx) = open_groups.iter().rposition(|g| g == n) {
                    open_groups.remove(idx);
                }
            }
            _ => {}
        }
    }
    open_groups.last().copied().unwrap_or(1)
}

impl Regex {
    /// Try to match at every position in `text`, returning the first match.
    fn find(&self, text: &[u8]) -> Option<MatchResult> {
        // If pattern starts with ^, only try at position 0.
        if matches!(self.nodes.first(), Some(ReNode::StartAnchor)) {
            let mut groups = vec![None; 10];
            if let Some(end) = self.match_at(text, 0, 1, &mut groups) {
                return Some(MatchResult {
                    start: 0,
                    end,
                    groups,
                });
            }
            return None;
        }
        for start in 0..=text.len() {
            let mut groups = vec![None; 10];
            if let Some(end) = self.match_at(text, start, 0, &mut groups) {
                return Some(MatchResult {
                    start,
                    end,
                    groups,
                });
            }
        }
        None
    }

    /// Try to match at a specific position, starting from a given node index.
    /// Returns the end position if successful.
    // Vec is required: backtracking saves/restores `groups` via .clone(),
    // which only works on Vec, not slices.
    #[allow(clippy::ptr_arg)]
    fn match_at(
        &self,
        text: &[u8],
        pos: usize,
        node_idx: usize,
        groups: &mut Vec<Option<(usize, usize)>>,
    ) -> Option<usize> {
        if node_idx >= self.nodes.len() {
            return Some(pos);
        }
        let node = &self.nodes[node_idx];
        match node {
            ReNode::StartAnchor => {
                if pos == 0 {
                    self.match_at(text, pos, node_idx + 1, groups)
                } else {
                    None
                }
            }
            ReNode::EndAnchor => {
                if pos == text.len() {
                    self.match_at(text, pos, node_idx + 1, groups)
                } else {
                    None
                }
            }
            ReNode::Literal(ch) => {
                if pos < text.len() && self.char_eq(text[pos], *ch) {
                    self.match_at(text, pos + 1, node_idx + 1, groups)
                } else {
                    None
                }
            }
            ReNode::AnyChar => {
                if pos < text.len() && text[pos] != b'\n' {
                    self.match_at(text, pos + 1, node_idx + 1, groups)
                } else {
                    None
                }
            }
            ReNode::CharClass { bytes, negated } => {
                if pos < text.len() {
                    let ch = text[pos];
                    let in_class = if self.ignore_case {
                        bytes
                            .iter()
                            .any(|&b| b.eq_ignore_ascii_case(&ch))
                    } else {
                        bytes.contains(&ch)
                    };
                    let matches = if *negated { !in_class } else { in_class };
                    if matches {
                        self.match_at(text, pos + 1, node_idx + 1, groups)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ReNode::Star(inner) => {
                // Greedy: try matching as many as possible, then backtrack.
                let mut positions = vec![pos];
                let mut p = pos;
                while p < text.len() {
                    let mut dummy = groups.clone();
                    if self.match_single(inner, text, p, &mut dummy).is_some() {
                        p += match self.match_single(inner, text, p, &mut dummy) {
                            Some(end) => {
                                if end == p {
                                    break;
                                }
                                end - p
                            }
                            None => break,
                        };
                        positions.push(p);
                    } else {
                        break;
                    }
                }
                // Try from longest match to shortest.
                for &try_pos in positions.iter().rev() {
                    if let Some(end) = self.match_at(text, try_pos, node_idx + 1, groups) {
                        return Some(end);
                    }
                }
                None
            }
            ReNode::Plus(inner) => {
                // One or more: must match at least once.
                let mut positions = Vec::new();
                let mut p = pos;
                while p < text.len() {
                    let mut dummy = groups.clone();
                    if let Some(end) = self.match_single(inner, text, p, &mut dummy) {
                        if end == p {
                            break;
                        }
                        p = end;
                        positions.push(p);
                    } else {
                        break;
                    }
                }
                for &try_pos in positions.iter().rev() {
                    if let Some(end) = self.match_at(text, try_pos, node_idx + 1, groups) {
                        return Some(end);
                    }
                }
                None
            }
            ReNode::Question(inner) => {
                // Try matching once first (greedy), then zero times.
                let mut dummy = groups.clone();
                if let Some(end1) = self.match_single(inner, text, pos, &mut dummy)
                    && let Some(end) = self.match_at(text, end1, node_idx + 1, groups) {
                        return Some(end);
                    }
                self.match_at(text, pos, node_idx + 1, groups)
            }
            ReNode::GroupStart(n) => {
                let n = *n;
                if n < groups.len() {
                    let old = groups[n];
                    // Record the start position; the end will be set by GroupEnd.
                    groups[n] = Some((pos, pos));
                    if let Some(end) = self.match_at(text, pos, node_idx + 1, groups) {
                        return Some(end);
                    }
                    groups[n] = old;
                    None
                } else {
                    self.match_at(text, pos, node_idx + 1, groups)
                }
            }
            ReNode::GroupEnd(n) => {
                let n = *n;
                if n < groups.len() {
                    let old = groups[n];
                    if let Some((start, _)) = groups[n] {
                        groups[n] = Some((start, pos));
                    }
                    if let Some(end) = self.match_at(text, pos, node_idx + 1, groups) {
                        return Some(end);
                    }
                    groups[n] = old;
                    None
                } else {
                    self.match_at(text, pos, node_idx + 1, groups)
                }
            }
        }
    }

    /// Try to match a single node at the given position.
    // See match_at: Vec is required for backtracking via .clone().
    #[allow(clippy::ptr_arg)]
    fn match_single(
        &self,
        node: &ReNode,
        text: &[u8],
        pos: usize,
        groups: &mut Vec<Option<(usize, usize)>>,
    ) -> Option<usize> {
        match node {
            ReNode::Literal(ch) => {
                if pos < text.len() && self.char_eq(text[pos], *ch) {
                    Some(pos + 1)
                } else {
                    None
                }
            }
            ReNode::AnyChar => {
                if pos < text.len() && text[pos] != b'\n' {
                    Some(pos + 1)
                } else {
                    None
                }
            }
            ReNode::CharClass { bytes, negated } => {
                if pos < text.len() {
                    let ch = text[pos];
                    let in_class = if self.ignore_case {
                        bytes
                            .iter()
                            .any(|&b| b.eq_ignore_ascii_case(&ch))
                    } else {
                        bytes.contains(&ch)
                    };
                    let matches = if *negated { !in_class } else { in_class };
                    if matches {
                        Some(pos + 1)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ReNode::GroupStart(n) => {
                let n = *n;
                if n < groups.len() {
                    groups[n] = Some((pos, pos));
                }
                Some(pos)
            }
            ReNode::GroupEnd(n) => {
                let n = *n;
                if n < groups.len()
                    && let Some((start, _)) = groups[n] {
                        groups[n] = Some((start, pos));
                    }
                Some(pos)
            }
            _ => None,
        }
    }

    /// Compare two bytes, respecting case-insensitivity.
    fn char_eq(&self, a: u8, b: u8) -> bool {
        if self.ignore_case {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    }

    /// Check if the pattern matches anywhere in the text.
    fn is_match(&self, text: &[u8]) -> bool {
        self.find(text).is_some()
    }
}

// ---------------------------------------------------------------------------
// Sed command parsing
// ---------------------------------------------------------------------------

/// An address that determines which lines a command applies to.
#[derive(Debug, Clone)]
enum Address {
    /// A specific line number.
    Line(usize),
    /// The last line (`$`).
    Last,
    /// Lines matching a regex.
    Pattern(Regex),
}

/// An address range for a command.
#[derive(Debug, Clone)]
enum AddressRange {
    /// No address -- applies to every line.
    All,
    /// Single address.
    Single(Address),
    /// Two-address range (inclusive).
    Range(Address, Address),
}

/// Substitution flags.
#[derive(Debug, Clone)]
#[derive(Default)]
struct SubFlags {
    /// Replace all occurrences.
    global: bool,
    /// Print after substitution.
    print: bool,
    /// Case-insensitive matching.
    ignore_case: bool,
    /// Replace only the Nth occurrence (0 = not set).
    nth: usize,
}


/// A sed editing command.
#[derive(Debug, Clone)]
enum Command {
    /// `s/pattern/replacement/flags`
    Substitute {
        pattern: Regex,
        replacement: Vec<u8>,
        flags: SubFlags,
    },
    /// `d` -- delete pattern space, restart.
    Delete,
    /// `p` -- print pattern space.
    Print,
    /// `q` -- quit.
    Quit,
    /// `a\text` -- append text after output.
    Append(Vec<u8>),
    /// `i\text` -- insert text before output.
    Insert(Vec<u8>),
    /// `c\text` -- replace line with text.
    Change(Vec<u8>),
    /// `y/src/dst/` -- transliterate.
    Transliterate { src: Vec<u8>, dst: Vec<u8> },
    /// `=` -- print line number.
    LineNumber,
    /// `{ commands }` -- grouped commands.
    Group(Vec<SedCommand>),
}

/// A complete sed command with optional address, negation, and the command itself.
#[derive(Debug, Clone)]
struct SedCommand {
    address: AddressRange,
    negated: bool,
    command: Command,
}

/// Parse a sed script string into a list of commands.
fn parse_script(script: &str, extended: bool) -> Result<Vec<SedCommand>, String> {
    let bytes = script.as_bytes();
    let mut pos = 0;
    let mut commands = Vec::new();

    while pos < bytes.len() {
        skip_whitespace_and_semicolons(bytes, &mut pos);
        if pos >= bytes.len() {
            break;
        }
        if bytes[pos] == b'#' {
            // Comment: skip to end of line.
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        if bytes[pos] == b'}' {
            // End of group -- handled by parse_group.
            break;
        }
        let cmd = parse_one_command(bytes, &mut pos, extended)?;
        commands.push(cmd);
    }
    Ok(commands)
}

fn skip_whitespace_and_semicolons(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len()
        && (bytes[*pos] == b' '
            || bytes[*pos] == b'\t'
            || bytes[*pos] == b'\n'
            || bytes[*pos] == b'\r'
            || bytes[*pos] == b';')
    {
        *pos += 1;
    }
}

fn skip_whitespace(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && (bytes[*pos] == b' ' || bytes[*pos] == b'\t') {
        *pos += 1;
    }
}

/// Parse a single address (line number, `$`, or `/regex/`).
fn parse_address(bytes: &[u8], pos: &mut usize, extended: bool) -> Result<Option<Address>, String> {
    if *pos >= bytes.len() {
        return Ok(None);
    }
    let ch = bytes[*pos];
    if ch.is_ascii_digit() {
        let start = *pos;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        let num_str = std::str::from_utf8(&bytes[start..*pos]).map_err(|e| e.to_string())?;
        let num: usize = num_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        Ok(Some(Address::Line(num)))
    } else if ch == b'$' {
        *pos += 1;
        Ok(Some(Address::Last))
    } else if ch == b'/' {
        *pos += 1; // skip opening /
        let pat = read_delimited(bytes, pos, b'/')?;
        let re = compile_regex(&pat, extended, false)?;
        Ok(Some(Address::Pattern(re)))
    } else if ch == b'\\' && *pos + 1 < bytes.len() {
        // \cREGEXPc form -- arbitrary delimiter
        *pos += 1;
        let delim = bytes[*pos];
        *pos += 1;
        let pat = read_delimited(bytes, pos, delim)?;
        let re = compile_regex(&pat, extended, false)?;
        Ok(Some(Address::Pattern(re)))
    } else {
        Ok(None)
    }
}

/// Read bytes until the delimiter, handling `\` escapes. Consumes the delimiter.
fn read_delimited(bytes: &[u8], pos: &mut usize, delim: u8) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    while *pos < bytes.len() && bytes[*pos] != delim {
        if bytes[*pos] == b'\\' && *pos + 1 < bytes.len() {
            let next = bytes[*pos + 1];
            if next == delim {
                result.push(delim);
                *pos += 2;
            } else {
                result.push(b'\\');
                result.push(next);
                *pos += 2;
            }
        } else {
            result.push(bytes[*pos]);
            *pos += 1;
        }
    }
    if *pos < bytes.len() && bytes[*pos] == delim {
        *pos += 1; // consume delimiter
    }
    Ok(result)
}

/// Parse a complete command, including address and command letter.
fn parse_one_command(
    bytes: &[u8],
    pos: &mut usize,
    extended: bool,
) -> Result<SedCommand, String> {
    skip_whitespace(bytes, pos);

    // Parse first address.
    let addr1 = parse_address(bytes, pos, extended)?;
    skip_whitespace(bytes, pos);

    // Check for comma (range).
    let address = if let Some(a1) = addr1 {
        if *pos < bytes.len() && bytes[*pos] == b',' {
            *pos += 1;
            skip_whitespace(bytes, pos);
            let addr2 = parse_address(bytes, pos, extended)?;
            match addr2 {
                Some(a2) => AddressRange::Range(a1, a2),
                None => return Err("expected second address after comma".into()),
            }
        } else {
            AddressRange::Single(a1)
        }
    } else {
        AddressRange::All
    };

    skip_whitespace(bytes, pos);

    // Check for negation.
    let negated = if *pos < bytes.len() && bytes[*pos] == b'!' {
        *pos += 1;
        skip_whitespace(bytes, pos);
        true
    } else {
        false
    };

    // Parse the command character.
    if *pos >= bytes.len() {
        return Err("expected command".into());
    }

    let cmd_char = bytes[*pos];
    *pos += 1;

    let command = match cmd_char {
        b's' => parse_substitute(bytes, pos, extended)?,
        b'd' => Command::Delete,
        b'p' => Command::Print,
        b'q' => Command::Quit,
        b'=' => Command::LineNumber,
        b'a' => {
            let text = parse_text_argument(bytes, pos);
            Command::Append(text)
        }
        b'i' => {
            let text = parse_text_argument(bytes, pos);
            Command::Insert(text)
        }
        b'c' => {
            let text = parse_text_argument(bytes, pos);
            Command::Change(text)
        }
        b'y' => parse_transliterate(bytes, pos)?,
        b'{' => {
            let group_cmds = parse_group(bytes, pos, extended)?;
            Command::Group(group_cmds)
        }
        other => {
            return Err(format!(
                "unknown command: '{}'",
                char::from(other)
            ));
        }
    };

    Ok(SedCommand {
        address,
        negated,
        command,
    })
}

/// Parse the text argument for a, i, c commands.
/// Handles `a\text` and `a text` forms.
fn parse_text_argument(bytes: &[u8], pos: &mut usize) -> Vec<u8> {
    // Skip optional backslash and whitespace/newline.
    if *pos < bytes.len() && bytes[*pos] == b'\\' {
        *pos += 1;
    }
    if *pos < bytes.len() && (bytes[*pos] == b'\n' || bytes[*pos] == b' ' || bytes[*pos] == b'\t')
    {
        *pos += 1;
    }
    let mut text = Vec::new();
    while *pos < bytes.len() && bytes[*pos] != b'\n' && bytes[*pos] != b';' {
        if bytes[*pos] == b'\\' && *pos + 1 < bytes.len() && bytes[*pos + 1] == b'n' {
            text.push(b'\n');
            *pos += 2;
        } else {
            text.push(bytes[*pos]);
            *pos += 1;
        }
    }
    text
}

/// Parse a substitute command: `s/pattern/replacement/flags`.
fn parse_substitute(bytes: &[u8], pos: &mut usize, extended: bool) -> Result<Command, String> {
    if *pos >= bytes.len() {
        return Err("s command requires a delimiter".into());
    }
    let delim = bytes[*pos];
    *pos += 1;
    let pattern_bytes = read_delimited(bytes, pos, delim)?;
    let replacement = read_delimited(bytes, pos, delim)?;

    // Parse flags.
    let mut flags = SubFlags::default();
    while *pos < bytes.len()
        && bytes[*pos] != b';'
        && bytes[*pos] != b'\n'
        && bytes[*pos] != b'}'
        && bytes[*pos] != b' '
        && bytes[*pos] != b'\t'
    {
        match bytes[*pos] {
            b'g' => flags.global = true,
            b'p' => flags.print = true,
            b'i' | b'I' => flags.ignore_case = true,
            d if d.is_ascii_digit() => {
                let start = *pos;
                while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                    *pos += 1;
                }
                let num_str =
                    std::str::from_utf8(&bytes[start..*pos]).map_err(|e| e.to_string())?;
                flags.nth = num_str
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?;
                continue; // already advanced pos
            }
            _ => break,
        }
        *pos += 1;
    }

    let pattern = compile_regex(&pattern_bytes, extended, flags.ignore_case)?;
    Ok(Command::Substitute {
        pattern,
        replacement,
        flags,
    })
}

/// Parse the `y/src/dst/` transliterate command.
fn parse_transliterate(bytes: &[u8], pos: &mut usize) -> Result<Command, String> {
    if *pos >= bytes.len() {
        return Err("y command requires a delimiter".into());
    }
    let delim = bytes[*pos];
    *pos += 1;
    let src = read_delimited(bytes, pos, delim)?;
    let dst = read_delimited(bytes, pos, delim)?;
    if src.len() != dst.len() {
        return Err(format!(
            "y: source and dest must have equal length ({} vs {})",
            src.len(),
            dst.len()
        ));
    }
    Ok(Command::Transliterate { src, dst })
}

/// Parse a `{ ... }` group of commands.
fn parse_group(
    bytes: &[u8],
    pos: &mut usize,
    extended: bool,
) -> Result<Vec<SedCommand>, String> {
    let mut commands = Vec::new();
    loop {
        skip_whitespace_and_semicolons(bytes, pos);
        if *pos >= bytes.len() {
            return Err("unterminated group (missing })".into());
        }
        if bytes[*pos] == b'}' {
            *pos += 1;
            break;
        }
        let cmd = parse_one_command(bytes, pos, extended)?;
        commands.push(cmd);
    }
    Ok(commands)
}

// ---------------------------------------------------------------------------
// Sed execution engine
// ---------------------------------------------------------------------------

/// Runtime state for the sed engine.
struct SedEngine {
    /// The list of commands to apply.
    commands: Vec<SedCommand>,
    /// Suppress auto-print (`-n`).
    quiet: bool,
    /// Current line number (1-based).
    line_number: usize,
    /// Whether we are currently inside a range for each Range command.
    /// Uses `Cell` for interior mutability so `matches_address` can update
    /// range state while borrowing `&self` (needed because commands are
    /// also borrowed from self during iteration).
    range_active: Vec<Cell<bool>>,
    /// Total lines (only known for files, not stdin).
    total_lines: Option<usize>,
}

impl SedEngine {
    fn new(commands: Vec<SedCommand>, quiet: bool) -> Self {
        let range_count = count_ranges(&commands);
        Self {
            commands,
            quiet,
            line_number: 0,
            range_active: vec![Cell::new(false); range_count],
            total_lines: None,
        }
    }

    /// Process all lines from a reader, writing output to the given writer.
    fn process<R: BufRead, W: Write>(
        &mut self,
        reader: R,
        writer: &mut W,
    ) -> io::Result<()> {
        let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
        self.total_lines = Some(lines.len());

        for line in &lines {
            self.line_number += 1;
            let is_last = self.line_number == lines.len();
            let mut pattern_space = line.as_bytes().to_vec();
            let mut range_idx = 0;

            let result = self.execute_commands(
                &self.commands.clone(),
                &mut pattern_space,
                &mut range_idx,
                is_last,
                writer,
            )?;

            match result {
                ExecResult::Continue => {
                    if !self.quiet {
                        writer.write_all(&pattern_space)?;
                        writer.write_all(b"\n")?;
                    }
                }
                ExecResult::Delete => {
                    // Don't print anything.
                }
                ExecResult::Quit => {
                    if !self.quiet {
                        writer.write_all(&pattern_space)?;
                        writer.write_all(b"\n")?;
                    }
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Execute a list of commands on the pattern space.
    fn execute_commands<W: Write>(
        &mut self,
        commands: &[SedCommand],
        pattern_space: &mut Vec<u8>,
        range_idx: &mut usize,
        is_last: bool,
        writer: &mut W,
    ) -> io::Result<ExecResult> {
        for cmd in commands {
            let matches_addr = self.matches_address(
                &cmd.address,
                pattern_space,
                is_last,
                range_idx,
            );
            let should_exec = if cmd.negated {
                !matches_addr
            } else {
                matches_addr
            };
            // Advance range_idx for range addresses.
            if matches!(cmd.address, AddressRange::Range(_, _)) {
                *range_idx += 1;
            }

            if !should_exec {
                continue;
            }

            match &cmd.command {
                Command::Substitute {
                    pattern,
                    replacement,
                    flags,
                } => {
                    let changed =
                        apply_substitute(pattern, replacement, flags, pattern_space);
                    if changed && flags.print {
                        writer.write_all(pattern_space)?;
                        writer.write_all(b"\n")?;
                    }
                }
                Command::Delete => {
                    return Ok(ExecResult::Delete);
                }
                Command::Print => {
                    writer.write_all(pattern_space)?;
                    writer.write_all(b"\n")?;
                }
                Command::Quit => {
                    return Ok(ExecResult::Quit);
                }
                Command::Append(text) => {
                    // Append happens after the current line is printed.
                    // We handle this by printing the pattern space now if not quiet,
                    // then the appended text, then returning Delete to avoid double-print.
                    if !self.quiet {
                        writer.write_all(pattern_space)?;
                        writer.write_all(b"\n")?;
                    }
                    writer.write_all(text)?;
                    writer.write_all(b"\n")?;
                    return Ok(ExecResult::Delete);
                }
                Command::Insert(text) => {
                    writer.write_all(text)?;
                    writer.write_all(b"\n")?;
                }
                Command::Change(text) => {
                    writer.write_all(text)?;
                    writer.write_all(b"\n")?;
                    return Ok(ExecResult::Delete);
                }
                Command::Transliterate { src, dst } => {
                    for byte in pattern_space.iter_mut() {
                        if let Some(idx) = src.iter().position(|&s| s == *byte)
                            && let Some(&replacement) = dst.get(idx) {
                                *byte = replacement;
                            }
                    }
                }
                Command::LineNumber => {
                    writeln!(writer, "{}", self.line_number)?;
                }
                Command::Group(sub_commands) => {
                    let sub_cmds = sub_commands.clone();
                    let result = self.execute_commands(
                        &sub_cmds,
                        pattern_space,
                        range_idx,
                        is_last,
                        writer,
                    )?;
                    if !matches!(result, ExecResult::Continue) {
                        return Ok(result);
                    }
                }
            }
        }
        Ok(ExecResult::Continue)
    }

    /// Check whether the current line matches an address.
    fn matches_address(
        &self,
        address: &AddressRange,
        pattern_space: &[u8],
        is_last: bool,
        range_idx: &mut usize,
    ) -> bool {
        match address {
            AddressRange::All => true,
            AddressRange::Single(addr) => self.matches_single_address(addr, pattern_space, is_last),
            AddressRange::Range(start, end) => {
                let idx = *range_idx;
                let active = self
                    .range_active
                    .get(idx)
                    .is_some_and(|c| c.get());

                if active {
                    // We're inside the range. Check if end matches.
                    if self.matches_single_address(end, pattern_space, is_last)
                        && let Some(cell) = self.range_active.get(idx) {
                            cell.set(false);
                        }
                    true
                } else if self.matches_single_address(start, pattern_space, is_last) {
                    if let Some(cell) = self.range_active.get(idx) {
                        cell.set(true);
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    fn matches_single_address(
        &self,
        addr: &Address,
        pattern_space: &[u8],
        is_last: bool,
    ) -> bool {
        match addr {
            Address::Line(n) => self.line_number == *n,
            Address::Last => is_last,
            Address::Pattern(re) => re.is_match(pattern_space),
        }
    }
}

/// Result of executing a command on a line.
enum ExecResult {
    /// Continue processing normally (auto-print unless -n).
    Continue,
    /// Delete the pattern space (don't print).
    Delete,
    /// Quit after printing.
    Quit,
}

/// Count the number of Range addresses in the command list (recursively).
fn count_ranges(commands: &[SedCommand]) -> usize {
    let mut count = 0;
    for cmd in commands {
        if matches!(cmd.address, AddressRange::Range(_, _)) {
            count += 1;
        }
        if let Command::Group(sub) = &cmd.command {
            count += count_ranges(sub);
        }
    }
    count
}

/// Apply a substitution command to the pattern space.
/// Returns true if any substitution was made.
fn apply_substitute(
    pattern: &Regex,
    replacement: &[u8],
    flags: &SubFlags,
    pattern_space: &mut Vec<u8>,
) -> bool {
    let mut changed = false;
    let mut occurrence = 0;

    if flags.global {
        // Global replacement: replace all non-overlapping matches.
        let mut result = Vec::new();
        let mut search_start = 0;

        while search_start <= pattern_space.len() {
            // Search from search_start.
            let search_slice = &pattern_space[search_start..];
            let m = pattern.find(search_slice);
            match m {
                Some(mat) => {
                    occurrence += 1;
                    // Copy text before the match.
                    result.extend_from_slice(&pattern_space[search_start..search_start + mat.start]);
                    // Build replacement.
                    let repl = build_replacement(replacement, search_slice, &mat);
                    result.extend_from_slice(&repl);
                    changed = true;
                    let advance = mat.end.max(mat.start + 1);
                    search_start += advance;
                }
                None => {
                    result.extend_from_slice(&pattern_space[search_start..]);
                    break;
                }
            }
        }
        if changed {
            *pattern_space = result;
        }
    } else if flags.nth > 0 {
        // Replace only the Nth occurrence.
        let target = flags.nth;
        let mut result = Vec::new();
        let mut search_start = 0;

        while search_start <= pattern_space.len() {
            let search_slice = &pattern_space[search_start..];
            let m = pattern.find(search_slice);
            match m {
                Some(mat) => {
                    occurrence += 1;
                    if occurrence == target {
                        result.extend_from_slice(
                            &pattern_space[search_start..search_start + mat.start],
                        );
                        let repl = build_replacement(replacement, search_slice, &mat);
                        result.extend_from_slice(&repl);
                        // Copy rest unchanged.
                        let rest_start = search_start + mat.end;
                        result.extend_from_slice(&pattern_space[rest_start..]);
                        changed = true;
                        break;
                    }
                    result.extend_from_slice(
                        &pattern_space[search_start..search_start + mat.end],
                    );
                    search_start += mat.end.max(mat.start + 1);
                }
                None => {
                    result.extend_from_slice(&pattern_space[search_start..]);
                    break;
                }
            }
        }
        if changed {
            *pattern_space = result;
        }
    } else {
        // Replace first occurrence only.
        if let Some(mat) = pattern.find(pattern_space) {
            let repl = build_replacement(replacement, pattern_space, &mat);
            let mut result = Vec::new();
            result.extend_from_slice(&pattern_space[..mat.start]);
            result.extend_from_slice(&repl);
            result.extend_from_slice(&pattern_space[mat.end..]);
            *pattern_space = result;
            changed = true;
        }
    }
    let _ = occurrence; // suppress unused warning when neither global nor nth
    changed
}

/// Build the replacement string, expanding `&`, `\1`..`\9`, `\n`, `\t`.
fn build_replacement(template: &[u8], text: &[u8], mat: &MatchResult) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < template.len() {
        if template[i] == b'&' {
            // Entire match.
            if mat.start < text.len() {
                let end = mat.end.min(text.len());
                result.extend_from_slice(&text[mat.start..end]);
            }
            i += 1;
        } else if template[i] == b'\\' && i + 1 < template.len() {
            let next = template[i + 1];
            if next.is_ascii_digit() && next != b'0' {
                let group_num = (next - b'0') as usize;
                if group_num < mat.groups.len()
                    && let Some((gs, ge)) = mat.groups[group_num]
                        && gs < text.len() {
                            let end = ge.min(text.len());
                            result.extend_from_slice(&text[gs..end]);
                        }
                i += 2;
            } else if next == b'n' {
                result.push(b'\n');
                i += 2;
            } else if next == b't' {
                result.push(b'\t');
                i += 2;
            } else if next == b'\\' {
                result.push(b'\\');
                i += 2;
            } else {
                result.push(next);
                i += 2;
            }
        } else {
            result.push(template[i]);
            i += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

struct Args {
    /// `-n` / `--quiet`
    quiet: bool,
    /// `-E` / `-r` / `--regexp-extended`
    extended: bool,
    /// In-place edit suffix. `Some("")` means no backup, `Some(".bak")` means backup.
    in_place: Option<String>,
    /// Script expressions from `-e`.
    expressions: Vec<String>,
    /// Script files from `-f`.
    script_files: Vec<String>,
    /// Input files (positional args after options).
    input_files: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = env::args().collect();
    let mut args = Args {
        quiet: false,
        extended: false,
        in_place: None,
        expressions: Vec::new(),
        script_files: Vec::new(),
        input_files: Vec::new(),
    };

    let mut i = 1;
    let mut found_double_dash = false;

    while i < argv.len() {
        if found_double_dash {
            args.input_files.push(argv[i].clone());
            i += 1;
            continue;
        }
        let arg = &argv[i];
        if arg == "--" {
            found_double_dash = true;
            i += 1;
            continue;
        }
        if arg == "-n" || arg == "--quiet" || arg == "--silent" {
            args.quiet = true;
            i += 1;
        } else if arg == "-E" || arg == "-r" || arg == "--regexp-extended" {
            args.extended = true;
            i += 1;
        } else if arg == "-e" || arg == "--expression" {
            i += 1;
            if i >= argv.len() {
                return Err("-e requires an argument".into());
            }
            args.expressions.push(argv[i].clone());
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("--expression=") {
            args.expressions.push(rest.to_string());
            i += 1;
        } else if arg == "-f" || arg == "--file" {
            i += 1;
            if i >= argv.len() {
                return Err("-f requires an argument".into());
            }
            args.script_files.push(argv[i].clone());
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("--file=") {
            args.script_files.push(rest.to_string());
            i += 1;
        } else if arg == "-i" || arg == "--in-place" {
            args.in_place = Some(String::new());
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("--in-place=") {
            args.in_place = Some(rest.to_string());
            i += 1;
        } else if arg.starts_with("-i") && arg.len() > 2 {
            // -iSUFFIX form
            let suffix = &arg[2..];
            args.in_place = Some(suffix.to_string());
            i += 1;
        } else if arg.starts_with("-") && arg.len() > 1 && !arg.starts_with("--") {
            // Combined short flags, e.g. -nE, -ne 'script'
            let flag_chars: Vec<u8> = arg.as_bytes()[1..].to_vec();
            let mut j = 0;
            while j < flag_chars.len() {
                match flag_chars[j] {
                    b'n' => args.quiet = true,
                    b'E' | b'r' => args.extended = true,
                    b'e' => {
                        // Rest of this arg or next arg is the expression.
                        if j + 1 < flag_chars.len() {
                            let rest =
                                String::from_utf8_lossy(&flag_chars[j + 1..]).to_string();
                            args.expressions.push(rest);
                            j = flag_chars.len(); // consumed
                            continue;
                        }
                        i += 1;
                        if i >= argv.len() {
                            return Err("-e requires an argument".into());
                        }
                        args.expressions.push(argv[i].clone());
                    }
                    b'f' => {
                        if j + 1 < flag_chars.len() {
                            let rest =
                                String::from_utf8_lossy(&flag_chars[j + 1..]).to_string();
                            args.script_files.push(rest);
                            j = flag_chars.len();
                            continue;
                        }
                        i += 1;
                        if i >= argv.len() {
                            return Err("-f requires an argument".into());
                        }
                        args.script_files.push(argv[i].clone());
                    }
                    _ => {
                        return Err(format!("unknown option: -{}", char::from(flag_chars[j])));
                    }
                }
                j += 1;
            }
            i += 1;
        } else if arg.starts_with("--") {
            return Err(format!("unknown option: {arg}"));
        } else {
            // Not an option. If we haven't collected any script yet, the first
            // non-option arg is the script (bare `sed 'script' files...`).
            if args.expressions.is_empty() && args.script_files.is_empty() {
                args.expressions.push(arg.clone());
            } else {
                args.input_files.push(arg.clone());
            }
            i += 1;
        }
    }

    if args.expressions.is_empty() && args.script_files.is_empty() {
        return Err("no script specified. Usage: sed [OPTIONS] 'script' [file...]".into());
    }

    Ok(args)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn run() -> Result<(), String> {
    let args = parse_args()?;

    // Collect the full script from all -e and -f sources.
    let mut full_script = String::new();
    for expr in &args.expressions {
        if !full_script.is_empty() {
            full_script.push('\n');
        }
        full_script.push_str(expr);
    }
    for file in &args.script_files {
        let contents = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        if !full_script.is_empty() {
            full_script.push('\n');
        }
        full_script.push_str(&contents);
    }

    let commands = parse_script(&full_script, args.extended)?;

    if let Some(ref suffix) = args.in_place {
        // In-place mode: process each file individually.
        if args.input_files.is_empty() {
            return Err("-i requires input files (cannot use stdin)".into());
        }
        for file_path in &args.input_files {
            process_in_place(file_path, suffix, &commands, args.quiet)?;
        }
    } else if args.input_files.is_empty() {
        // Read from stdin.
        let stdin = io::stdin();
        let reader = stdin.lock();
        let stdout = io::stdout();
        let mut writer = io::BufWriter::new(stdout.lock());
        let mut engine = SedEngine::new(commands.clone(), args.quiet);
        engine.process(reader, &mut writer).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    } else {
        // Process listed files in order.
        let stdout = io::stdout();
        let mut writer = io::BufWriter::new(stdout.lock());
        let mut engine = SedEngine::new(commands.clone(), args.quiet);
        for file_path in &args.input_files {
            let contents = fs::read(file_path).map_err(|e| format!("{file_path}: {e}"))?;
            let reader = io::Cursor::new(contents);
            engine.process(reader, &mut writer).map_err(|e| e.to_string())?;
        }
        writer.flush().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Process a file in-place, with optional backup.
fn process_in_place(
    path: &str,
    suffix: &str,
    commands: &[SedCommand],
    quiet: bool,
) -> Result<(), String> {
    let contents = fs::read(path).map_err(|e| format!("{path}: {e}"))?;

    // Create backup if suffix is non-empty.
    if !suffix.is_empty() {
        let backup_path = format!("{path}{suffix}");
        fs::copy(path, &backup_path).map_err(|e| format!("backup {backup_path}: {e}"))?;
    }

    let reader = io::Cursor::new(contents);
    let mut output = Vec::new();
    let mut engine = SedEngine::new(commands.to_vec(), quiet);
    engine
        .process(reader, &mut output)
        .map_err(|e| e.to_string())?;

    fs::write(path, &output).map_err(|e| format!("writing {path}: {e}"))?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        let _ = writeln!(io::stderr(), "sed: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Regex tests --

    #[test]
    fn test_literal_match() {
        let re = compile_regex(b"hello", false, false).unwrap();
        assert!(re.is_match(b"say hello world"));
        assert!(!re.is_match(b"say helo world"));
    }

    #[test]
    fn test_dot_match() {
        let re = compile_regex(b"h.llo", false, false).unwrap();
        assert!(re.is_match(b"hello"));
        assert!(re.is_match(b"hallo"));
        assert!(!re.is_match(b"hllo"));
    }

    #[test]
    fn test_star_match() {
        let re = compile_regex(b"ab*c", false, false).unwrap();
        assert!(re.is_match(b"ac"));
        assert!(re.is_match(b"abc"));
        assert!(re.is_match(b"abbc"));
        assert!(!re.is_match(b"adc"));
    }

    #[test]
    fn test_plus_extended() {
        let re = compile_regex(b"ab+c", true, false).unwrap();
        assert!(!re.is_match(b"ac"));
        assert!(re.is_match(b"abc"));
        assert!(re.is_match(b"abbc"));
    }

    #[test]
    fn test_question_extended() {
        let re = compile_regex(b"ab?c", true, false).unwrap();
        assert!(re.is_match(b"ac"));
        assert!(re.is_match(b"abc"));
        assert!(!re.is_match(b"abbc"));
    }

    #[test]
    fn test_anchors() {
        let re_start = compile_regex(b"^hello", false, false).unwrap();
        assert!(re_start.is_match(b"hello world"));
        assert!(!re_start.is_match(b"say hello"));

        let re_end = compile_regex(b"world$", false, false).unwrap();
        assert!(re_end.is_match(b"hello world"));
        assert!(!re_end.is_match(b"world peace"));
    }

    #[test]
    fn test_char_class() {
        let re = compile_regex(b"[abc]", false, false).unwrap();
        assert!(re.is_match(b"a"));
        assert!(re.is_match(b"b"));
        assert!(!re.is_match(b"d"));

        let re_neg = compile_regex(b"[^abc]", false, false).unwrap();
        assert!(!re_neg.is_match(b"a"));
        assert!(re_neg.is_match(b"d"));
    }

    #[test]
    fn test_char_class_range() {
        let re = compile_regex(b"[a-z]", false, false).unwrap();
        assert!(re.is_match(b"m"));
        assert!(!re.is_match(b"M"));
    }

    #[test]
    fn test_case_insensitive() {
        let re = compile_regex(b"hello", false, true).unwrap();
        assert!(re.is_match(b"HELLO"));
        assert!(re.is_match(b"Hello"));
        assert!(re.is_match(b"hello"));
    }

    #[test]
    fn test_capture_groups_basic() {
        let re = compile_regex(b"\\(hel\\)lo", false, false).unwrap();
        let m = re.find(b"hello").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
        assert_eq!(m.groups[1], Some((0, 3)));
    }

    #[test]
    fn test_capture_groups_extended() {
        let re = compile_regex(b"(hel)lo", true, false).unwrap();
        let m = re.find(b"hello").unwrap();
        assert_eq!(m.groups[1], Some((0, 3)));
    }

    // -- Substitution tests --

    #[test]
    fn test_substitute_simple() {
        let re = compile_regex(b"foo", false, false).unwrap();
        let flags = SubFlags::default();
        let mut ps = b"foo bar foo".to_vec();
        let changed = apply_substitute(&re, b"baz", &flags, &mut ps);
        assert!(changed);
        assert_eq!(ps, b"baz bar foo");
    }

    #[test]
    fn test_substitute_global() {
        let re = compile_regex(b"foo", false, false).unwrap();
        let flags = SubFlags {
            global: true,
            ..Default::default()
        };
        let mut ps = b"foo bar foo".to_vec();
        apply_substitute(&re, b"baz", &flags, &mut ps);
        assert_eq!(ps, b"baz bar baz");
    }

    #[test]
    fn test_substitute_backreference() {
        let re = compile_regex(b"\\(f..\\)", false, false).unwrap();
        let flags = SubFlags::default();
        let mut ps = b"foo bar".to_vec();
        apply_substitute(&re, b"[\\1]", &flags, &mut ps);
        assert_eq!(ps, b"[foo] bar");
    }

    #[test]
    fn test_substitute_ampersand() {
        let re = compile_regex(b"foo", false, false).unwrap();
        let flags = SubFlags::default();
        let mut ps = b"foo bar".to_vec();
        apply_substitute(&re, b"[&]", &flags, &mut ps);
        assert_eq!(ps, b"[foo] bar");
    }

    #[test]
    fn test_substitute_nth() {
        let re = compile_regex(b"x", false, false).unwrap();
        let flags = SubFlags {
            nth: 2,
            ..Default::default()
        };
        let mut ps = b"axbxcxd".to_vec();
        apply_substitute(&re, b"Y", &flags, &mut ps);
        assert_eq!(ps, b"axbYcxd");
    }

    // -- Transliterate test --

    #[test]
    fn test_transliterate() {
        let src = b"abc".to_vec();
        let dst = b"xyz".to_vec();
        let mut ps = b"abcdef".to_vec();
        for byte in ps.iter_mut() {
            if let Some(idx) = src.iter().position(|&s| s == *byte) {
                if let Some(&repl) = dst.get(idx) {
                    *byte = repl;
                }
            }
        }
        assert_eq!(ps, b"xyzdef");
    }

    // -- Script parsing tests --

    #[test]
    fn test_parse_substitute_command() {
        let cmds = parse_script("s/foo/bar/g", false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0].command {
            Command::Substitute { flags, .. } => {
                assert!(flags.global);
                assert!(!flags.print);
            }
            _ => panic!("expected Substitute"),
        }
    }

    #[test]
    fn test_parse_delete_with_address() {
        let cmds = parse_script("3d", false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0].address {
            AddressRange::Single(Address::Line(3)) => {}
            _ => panic!("expected line 3 address"),
        }
        assert!(matches!(cmds[0].command, Command::Delete));
    }

    #[test]
    fn test_parse_regex_address() {
        let cmds = parse_script("/^#/d", false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0].address {
            AddressRange::Single(Address::Pattern(_)) => {}
            _ => panic!("expected pattern address"),
        }
    }

    #[test]
    fn test_parse_range_address() {
        let cmds = parse_script("2,5d", false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0].address {
            AddressRange::Range(Address::Line(2), Address::Line(5)) => {}
            _ => panic!("expected range 2,5"),
        }
    }

    #[test]
    fn test_parse_negated() {
        let cmds = parse_script("/keep/!d", false).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].negated);
        assert!(matches!(cmds[0].command, Command::Delete));
    }

    #[test]
    fn test_parse_multiple_commands() {
        let cmds = parse_script("s/a/b/;s/c/d/", false).unwrap();
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn test_parse_y_command() {
        let cmds = parse_script("y/abc/xyz/", false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0].command {
            Command::Transliterate { src, dst } => {
                assert_eq!(src, b"abc");
                assert_eq!(dst, b"xyz");
            }
            _ => panic!("expected Transliterate"),
        }
    }

    // -- Engine integration tests --

    fn run_sed(script: &str, input: &str, quiet: bool, extended: bool) -> String {
        let commands = parse_script(script, extended).unwrap();
        let mut engine = SedEngine::new(commands, quiet);
        let reader = io::Cursor::new(input.as_bytes().to_vec());
        let mut output = Vec::new();
        engine.process(reader, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn test_engine_simple_substitute() {
        let out = run_sed("s/hello/world/", "hello there\nhello again\n", false, false);
        assert_eq!(out, "world there\nworld again\n");
    }

    #[test]
    fn test_engine_delete() {
        let out = run_sed("2d", "line1\nline2\nline3\n", false, false);
        assert_eq!(out, "line1\nline3\n");
    }

    #[test]
    fn test_engine_quiet_print() {
        let out = run_sed("2p", "line1\nline2\nline3\n", true, false);
        assert_eq!(out, "line2\n");
    }

    #[test]
    fn test_engine_line_number() {
        let out = run_sed("=", "a\nb\n", true, false);
        assert_eq!(out, "1\n2\n");
    }

    #[test]
    fn test_engine_quit() {
        let out = run_sed("2q", "a\nb\nc\nd\n", false, false);
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn test_engine_insert() {
        let out = run_sed("2i\\INSERTED", "a\nb\nc\n", false, false);
        assert_eq!(out, "a\nINSERTED\nb\nc\n");
    }

    #[test]
    fn test_engine_change() {
        let out = run_sed("2c\\CHANGED", "a\nb\nc\n", false, false);
        assert_eq!(out, "a\nCHANGED\nc\n");
    }

    #[test]
    fn test_engine_append() {
        let out = run_sed("2a\\AFTER", "a\nb\nc\n", false, false);
        assert_eq!(out, "a\nb\nAFTER\nc\n");
    }

    #[test]
    fn test_engine_transliterate() {
        let out = run_sed("y/abc/ABC/", "aXbYcZ\n", false, false);
        assert_eq!(out, "AXBYCZ\n");
    }

    #[test]
    fn test_engine_regex_address() {
        let out = run_sed("/^#/d", "#comment\ncode\n#another\n", false, false);
        assert_eq!(out, "code\n");
    }

    #[test]
    fn test_engine_range() {
        let out = run_sed("2,3d", "a\nb\nc\nd\n", false, false);
        assert_eq!(out, "a\nd\n");
    }

    #[test]
    fn test_engine_last_line() {
        let out = run_sed("$d", "a\nb\nc\n", false, false);
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn test_engine_negated_delete() {
        let out = run_sed("/keep/!d", "drop\nkeep this\ndrop too\nkeep me\n", false, false);
        assert_eq!(out, "keep this\nkeep me\n");
    }

    #[test]
    fn test_engine_empty_input() {
        let out = run_sed("s/a/b/", "", false, false);
        assert_eq!(out, "");
    }

    #[test]
    fn test_engine_global_sub_with_dot_star() {
        let out = run_sed("s/.*/(&)/", "hello\n", false, false);
        assert_eq!(out, "(hello)\n");
    }

    #[test]
    fn test_engine_extended_regex() {
        let out = run_sed("s/a+/X/g", "aaa bb aa\n", false, true);
        assert_eq!(out, "X bb X\n");
    }

    #[test]
    fn test_engine_group_commands() {
        let out = run_sed("2{s/b/B/;p}", "a\nb\nc\n", true, false);
        assert_eq!(out, "B\n");
    }

    #[test]
    fn test_engine_substitute_case_insensitive() {
        let out = run_sed("s/hello/world/i", "Hello There\n", false, false);
        assert_eq!(out, "world There\n");
    }

    #[test]
    fn test_engine_pattern_range() {
        let out = run_sed("/start/,/end/d", "before\nstart\nmiddle\nend\nafter\n", false, false);
        assert_eq!(out, "before\nafter\n");
    }

    #[test]
    fn test_arg_parse_basic() {
        // This tests the parsing logic indirectly -- can't easily set argv,
        // but we can verify script parsing from a string.
        let cmds = parse_script("s/a/b/g;3d;/x/p", false).unwrap();
        assert_eq!(cmds.len(), 3);
    }
}
