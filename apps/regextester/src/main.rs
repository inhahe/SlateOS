//! `OurOS` Regex Tester & Debugger
//!
//! An interactive regex testing tool with:
//! - Custom regex engine supporting common regex features
//! - Real-time match highlighting as you type
//! - Match groups and captures display
//! - Regex syntax reference panel
//! - Match statistics (count, positions, groups)
//! - Find & replace with backreferences
//! - Regex library (save/load named patterns)
//! - Common regex patterns (email, URL, IP, date, etc.)
//! - Regex explanation (break down pattern into readable description)
//! - Multi-line test input support
//! - Case-insensitive and other flags
//! - Multi-panel UI with pattern, input, matches, and reference
//!
//! Uses the guitk library for UI rendering.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::cognitive_complexity)]
// Many items are used only via test module and the real GUI event loop
#![allow(dead_code)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const SIDEBAR_WIDTH: f32 = 220.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const PADDING: f32 = 10.0;
const LINE_HEIGHT: f32 = 20.0;
const CHAR_WIDTH: f32 = 8.0;
const SMALL_TEXT: f32 = 12.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;

// Maximum limits
const MAX_PATTERN_LEN: usize = 512;
const MAX_INPUT_LEN: usize = 16384;
const MAX_REPLACE_LEN: usize = 512;
const MAX_MATCHES: usize = 1000;
const MAX_LIBRARY_ENTRIES: usize = 100;
const MAX_HISTORY: usize = 50;

// ============================================================================
// Regex Engine
// ============================================================================

/// A single node in the compiled regex NFA
#[derive(Debug, Clone)]
enum RegexNode {
    /// Match a literal character
    Literal(char),
    /// Match any character (.)
    AnyChar,
    /// Match a character class [abc] or [a-z]
    CharClass {
        chars: Vec<char>,
        ranges: Vec<(char, char)>,
        negated: bool,
    },
    /// Predefined class: \d, \w, \s, etc.
    PredefinedClass(PredefinedClass),
    /// Anchor: ^ or $
    Anchor(AnchorKind),
    /// Group start (capturing or non-capturing)
    GroupStart { group_id: usize, capturing: bool },
    /// Group end
    GroupEnd { group_id: usize },
    /// Split (for alternation and quantifiers) - try first, then second
    Split { first: usize, second: usize },
    /// Jump to another node
    Jump(usize),
    /// Match (accept state)
    Match,
    /// Word boundary \b
    WordBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PredefinedClass {
    Digit,         // \d
    NonDigit,      // \D
    Word,          // \w
    NonWord,       // \W
    Whitespace,    // \s
    NonWhitespace, // \S
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorKind {
    Start, // ^
    End,   // $
}

/// A compiled regex pattern
#[derive(Debug, Clone)]
struct CompiledRegex {
    nodes: Vec<RegexNode>,
    group_count: usize,
}

/// A match result with position and captured groups
#[derive(Debug, Clone)]
struct RegexMatch {
    start: usize,
    end: usize,
    groups: Vec<Option<(usize, usize)>>,
}

/// Thread for NFA simulation
#[derive(Debug, Clone)]
struct Thread {
    pc: usize,
    /// Input position where this thread's potential match began. Tracked
    /// per-thread so the engine can report the true match start when scanning
    /// forward for an unanchored leftmost match.
    start: usize,
    groups: Vec<Option<(usize, usize)>>,
    group_starts: Vec<Option<usize>>,
}

/// Parse error for regex patterns
#[derive(Debug, Clone)]
struct RegexError {
    message: String,
    position: usize,
}

impl std::fmt::Display for RegexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at position {}: {}", self.position, self.message)
    }
}

/// Regex compiler: parses pattern string into NFA nodes
struct RegexCompiler {
    pattern: Vec<char>,
    pos: usize,
    nodes: Vec<RegexNode>,
    group_count: usize,
    case_insensitive: bool,
}

impl RegexCompiler {
    fn new(pattern: &str, case_insensitive: bool) -> Self {
        Self {
            pattern: pattern.chars().collect(),
            pos: 0,
            nodes: Vec::new(),
            group_count: 0,
            case_insensitive,
        }
    }

    fn compile(mut self) -> Result<CompiledRegex, RegexError> {
        self.parse_alternation()?;
        self.nodes.push(RegexNode::Match);
        Ok(CompiledRegex {
            nodes: self.nodes,
            group_count: self.group_count,
        })
    }

    fn peek(&self) -> Option<char> {
        self.pattern.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.pattern.get(self.pos).copied();
        if c.is_some() {
            self.pos = self.pos.saturating_add(1);
        }
        c
    }

    fn parse_alternation(&mut self) -> Result<(), RegexError> {
        let start = self.nodes.len();
        self.parse_sequence()?;

        if self.peek() == Some('|') {
            // Alternation: a|b becomes Split(a_branch, b_branch)
            let mut branches = vec![(start, self.nodes.len())];

            while self.peek() == Some('|') {
                self.advance(); // consume '|'
                let branch_start = self.nodes.len();
                self.parse_sequence()?;
                branches.push((branch_start, self.nodes.len()));
            }

            // Rebuild with split nodes
            let mut new_nodes: Vec<RegexNode> = Vec::new();
            let mut jump_patches: Vec<usize> = Vec::new();

            for (i, &(bstart, bend)) in branches.iter().enumerate() {
                if i < branches.len().saturating_sub(1) {
                    let split_pos = new_nodes.len();
                    // Placeholder split: first = next (branch body), second = next branch's split
                    new_nodes.push(RegexNode::Split {
                        first: split_pos.saturating_add(1),
                        second: 0, // patched later
                    });
                }

                // Copy branch nodes, adjusting indices
                let offset = new_nodes.len().wrapping_sub(bstart);
                for j in bstart..bend {
                    let mut node = self.nodes[j].clone();
                    adjust_node(&mut node, offset, bstart, bend);
                    new_nodes.push(node);
                }

                if i < branches.len().saturating_sub(1) {
                    // Jump over remaining branches
                    jump_patches.push(new_nodes.len());
                    new_nodes.push(RegexNode::Jump(0)); // patched later
                }
            }

            let end = new_nodes.len();

            // Patch split second targets and jump targets
            let mut split_idx = 0;
            for node in &mut new_nodes {
                match node {
                    RegexNode::Split { second, .. } if *second == 0 => {
                        // Point to next split or last branch start
                        if let Some(&patch_pos) = jump_patches.get(split_idx) {
                            *second = patch_pos.saturating_add(1);
                        }
                        split_idx = split_idx.saturating_add(1);
                    }
                    RegexNode::Jump(target) if *target == 0 => {
                        *target = end;
                    }
                    _ => {}
                }
            }

            // Replace nodes from start
            self.nodes.truncate(start);
            self.nodes.extend(new_nodes);
        }

        Ok(())
    }

    fn parse_sequence(&mut self) -> Result<(), RegexError> {
        while let Some(c) = self.peek() {
            if c == ')' || c == '|' {
                break;
            }
            self.parse_quantified()?;
        }
        Ok(())
    }

    fn parse_quantified(&mut self) -> Result<(), RegexError> {
        let atom_start = self.nodes.len();
        self.parse_atom()?;
        let atom_end = self.nodes.len();

        // Check for quantifier
        match self.peek() {
            Some('*') => {
                self.advance();
                let greedy = self.peek() != Some('?');
                if !greedy {
                    self.advance();
                }
                // a* = Split(a, skip); a -> Jump(split)
                let split_pos = atom_start;
                self.nodes.push(RegexNode::Jump(split_pos));
                // After inserting the Split at `atom_start`, every node at or
                // after `atom_start` shifts up by one. The Jump currently sits
                // at the end; the exit (next node to be pushed) lands one past
                // it post-insert, i.e. nodes.len() + 1.
                let exit = self.nodes.len().saturating_add(1);
                let body = split_pos.saturating_add(1);
                let split = if greedy {
                    RegexNode::Split {
                        first: body,
                        second: exit,
                    }
                } else {
                    RegexNode::Split {
                        first: exit,
                        second: body,
                    }
                };
                self.nodes.insert(atom_start, split);
                // Adjust indices of *other* nodes after insertion. The Split's
                // own targets are already expressed in post-insert coordinates.
                adjust_after_insert(&mut self.nodes, atom_start, exit);
            }
            Some('+') => {
                self.advance();
                let greedy = self.peek() != Some('?');
                if !greedy {
                    self.advance();
                }
                // a+ = a; Split(a, skip)
                let split_pos = self.nodes.len();
                let after = split_pos.saturating_add(1);
                let split = if greedy {
                    RegexNode::Split {
                        first: atom_start,
                        second: after,
                    }
                } else {
                    RegexNode::Split {
                        first: after,
                        second: atom_start,
                    }
                };
                self.nodes.push(split);
            }
            Some('?') => {
                self.advance();
                let greedy = self.peek() != Some('?');
                if !greedy {
                    self.advance();
                }
                // a? = Split(a, skip). After inserting the Split at atom_start,
                // the atom occupies [atom_start+1, atom_end+1) and the exit
                // (next node to be pushed) lands at atom_end+1.
                let body = atom_start.saturating_add(1);
                let exit = atom_end.saturating_add(1);
                let split = if greedy {
                    RegexNode::Split {
                        first: body,
                        second: exit,
                    }
                } else {
                    RegexNode::Split {
                        first: exit,
                        second: body,
                    }
                };
                self.nodes.insert(atom_start, split);
                adjust_after_insert(&mut self.nodes, atom_start, exit);
            }
            Some('{') => {
                if let Some((min, max)) = self.try_parse_repetition() {
                    self.apply_repetition(atom_start, atom_end, min, max);
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn try_parse_repetition(&mut self) -> Option<(usize, Option<usize>)> {
        let saved_pos = self.pos;
        self.advance(); // consume '{'

        let mut min_str = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                min_str.push(c);
                self.advance();
            } else {
                break;
            }
        }

        if min_str.is_empty() {
            self.pos = saved_pos;
            return None;
        }

        let min: usize = min_str.parse().ok()?;

        match self.peek() {
            Some('}') => {
                self.advance();
                Some((min, Some(min)))
            }
            Some(',') => {
                self.advance();
                let mut max_str = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        max_str.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                if self.peek() == Some('}') {
                    self.advance();
                    if max_str.is_empty() {
                        Some((min, None)) // {n,} = at least n
                    } else {
                        let max: usize = max_str.parse().ok()?;
                        Some((min, Some(max)))
                    }
                } else {
                    self.pos = saved_pos;
                    None
                }
            }
            _ => {
                self.pos = saved_pos;
                None
            }
        }
    }

    fn apply_repetition(
        &mut self,
        atom_start: usize,
        atom_end: usize,
        min: usize,
        max: Option<usize>,
    ) {
        let atom_nodes: Vec<RegexNode> = self.nodes[atom_start..atom_end].to_vec();
        self.nodes.truncate(atom_start);

        // Required copies (min)
        for _ in 0..min {
            let offset = self.nodes.len().wrapping_sub(atom_start);
            for node in &atom_nodes {
                let mut n = node.clone();
                adjust_node_offset(&mut n, offset);
                self.nodes.push(n);
            }
        }

        // Optional copies (up to max)
        if let Some(max_val) = max {
            for _ in min..max_val {
                let split_pos = self.nodes.len();
                let body_start = split_pos.saturating_add(1);
                // Will be fixed up after we know the body end
                self.nodes.push(RegexNode::Split {
                    first: body_start,
                    second: 0,
                });

                let offset = self.nodes.len().wrapping_sub(atom_start);
                for node in &atom_nodes {
                    let mut n = node.clone();
                    adjust_node_offset(&mut n, offset);
                    self.nodes.push(n);
                }

                let after = self.nodes.len();
                if let Some(RegexNode::Split { second, .. }) = self.nodes.get_mut(split_pos) {
                    *second = after;
                }
            }
        } else {
            // {n,} = min copies + star
            let split_pos = self.nodes.len();
            let body_start = split_pos.saturating_add(1);
            self.nodes.push(RegexNode::Split {
                first: body_start,
                second: 0,
            });

            let offset = self.nodes.len().wrapping_sub(atom_start);
            for node in &atom_nodes {
                let mut n = node.clone();
                adjust_node_offset(&mut n, offset);
                self.nodes.push(n);
            }
            self.nodes.push(RegexNode::Jump(split_pos));

            let after = self.nodes.len();
            if let Some(RegexNode::Split { second, .. }) = self.nodes.get_mut(split_pos) {
                *second = after;
            }
        }
    }

    fn parse_atom(&mut self) -> Result<(), RegexError> {
        match self.peek() {
            Some('(') => {
                self.advance();
                let capturing;
                let group_id;

                if self.peek() == Some('?')
                    && self.pattern.get(self.pos.saturating_add(1)).copied() == Some(':')
                {
                    self.advance(); // ?
                    self.advance(); // :
                    capturing = false;
                    group_id = 0; // non-capturing
                } else {
                    self.group_count = self.group_count.saturating_add(1);
                    group_id = self.group_count;
                    capturing = true;
                }

                if capturing {
                    self.nodes.push(RegexNode::GroupStart {
                        group_id,
                        capturing,
                    });
                }

                self.parse_alternation()?;

                if self.peek() != Some(')') {
                    return Err(RegexError {
                        message: "Unmatched '('".into(),
                        position: self.pos,
                    });
                }
                self.advance();

                if capturing {
                    self.nodes.push(RegexNode::GroupEnd { group_id });
                }
            }
            Some('[') => {
                self.advance();
                let negated = self.peek() == Some('^');
                if negated {
                    self.advance();
                }

                let mut chars = Vec::new();
                let mut ranges = Vec::new();

                // Handle ] as first char in class
                if self.peek() == Some(']') {
                    chars.push(']');
                    self.advance();
                }

                while let Some(c) = self.peek() {
                    if c == ']' {
                        self.advance();
                        break;
                    }
                    let ch = self.parse_char_in_class()?;
                    if self.peek() == Some('-')
                        && self
                            .pattern
                            .get(self.pos.saturating_add(1))
                            .is_some_and(|&next| next != ']')
                    {
                        self.advance(); // consume '-'
                        let end_ch = self.parse_char_in_class()?;
                        ranges.push((ch, end_ch));
                    } else {
                        chars.push(ch);
                    }
                }

                if self.case_insensitive {
                    let extra: Vec<char> = chars
                        .iter()
                        .filter_map(|c| {
                            if c.is_ascii_lowercase() {
                                Some(c.to_ascii_uppercase())
                            } else if c.is_ascii_uppercase() {
                                Some(c.to_ascii_lowercase())
                            } else {
                                None
                            }
                        })
                        .collect();
                    chars.extend(extra);
                }

                self.nodes.push(RegexNode::CharClass {
                    chars,
                    ranges,
                    negated,
                });
            }
            Some('.') => {
                self.advance();
                self.nodes.push(RegexNode::AnyChar);
            }
            Some('^') => {
                self.advance();
                self.nodes.push(RegexNode::Anchor(AnchorKind::Start));
            }
            Some('$') => {
                self.advance();
                self.nodes.push(RegexNode::Anchor(AnchorKind::End));
            }
            Some('\\') => {
                self.advance();
                match self.peek() {
                    Some('d') => {
                        self.advance();
                        self.nodes
                            .push(RegexNode::PredefinedClass(PredefinedClass::Digit));
                    }
                    Some('D') => {
                        self.advance();
                        self.nodes
                            .push(RegexNode::PredefinedClass(PredefinedClass::NonDigit));
                    }
                    Some('w') => {
                        self.advance();
                        self.nodes
                            .push(RegexNode::PredefinedClass(PredefinedClass::Word));
                    }
                    Some('W') => {
                        self.advance();
                        self.nodes
                            .push(RegexNode::PredefinedClass(PredefinedClass::NonWord));
                    }
                    Some('s') => {
                        self.advance();
                        self.nodes
                            .push(RegexNode::PredefinedClass(PredefinedClass::Whitespace));
                    }
                    Some('S') => {
                        self.advance();
                        self.nodes
                            .push(RegexNode::PredefinedClass(PredefinedClass::NonWhitespace));
                    }
                    Some('b') => {
                        self.advance();
                        self.nodes.push(RegexNode::WordBoundary);
                    }
                    Some('n') => {
                        self.advance();
                        self.push_literal('\n');
                    }
                    Some('r') => {
                        self.advance();
                        self.push_literal('\r');
                    }
                    Some('t') => {
                        self.advance();
                        self.push_literal('\t');
                    }
                    Some(c) if !c.is_alphanumeric() => {
                        let ch = c;
                        self.advance();
                        self.push_literal(ch);
                    }
                    Some(c) => {
                        return Err(RegexError {
                            message: format!("Unknown escape '\\{c}'"),
                            position: self.pos,
                        });
                    }
                    None => {
                        return Err(RegexError {
                            message: "Trailing backslash".into(),
                            position: self.pos,
                        });
                    }
                }
            }
            Some(c)
                if c != ')'
                    && c != '|'
                    && c != '*'
                    && c != '+'
                    && c != '?'
                    && c != '{'
                    && c != '}' =>
            {
                self.advance();
                self.push_literal(c);
            }
            Some(c) => {
                return Err(RegexError {
                    message: format!("Unexpected character '{c}'"),
                    position: self.pos,
                });
            }
            None => {
                return Err(RegexError {
                    message: "Unexpected end of pattern".into(),
                    position: self.pos,
                });
            }
        }

        Ok(())
    }

    fn push_literal(&mut self, c: char) {
        if self.case_insensitive && c.is_ascii_alphabetic() {
            let lower = c.to_ascii_lowercase();
            let upper = c.to_ascii_uppercase();
            self.nodes.push(RegexNode::CharClass {
                chars: vec![lower, upper],
                ranges: Vec::new(),
                negated: false,
            });
        } else {
            self.nodes.push(RegexNode::Literal(c));
        }
    }

    fn parse_char_in_class(&mut self) -> Result<char, RegexError> {
        match self.advance() {
            Some('\\') => match self.advance() {
                Some('n') => Ok('\n'),
                Some('r') => Ok('\r'),
                Some('t') => Ok('\t'),
                Some(c) => Ok(c),
                None => Err(RegexError {
                    message: "Trailing backslash in class".into(),
                    position: self.pos,
                }),
            },
            Some(c) => Ok(c),
            None => Err(RegexError {
                message: "Unterminated character class".into(),
                position: self.pos,
            }),
        }
    }
}

fn adjust_node(node: &mut RegexNode, offset: usize, _bstart: usize, _bend: usize) {
    adjust_node_offset(node, offset);
}

fn adjust_node_offset(node: &mut RegexNode, offset: usize) {
    match node {
        RegexNode::Split { first, second } => {
            *first = first.wrapping_add(offset);
            *second = second.wrapping_add(offset);
        }
        RegexNode::Jump(target) => {
            *target = target.wrapping_add(offset);
        }
        _ => {}
    }
}

fn adjust_after_insert(nodes: &mut [RegexNode], insert_pos: usize, _count: usize) {
    for (i, node) in nodes.iter_mut().enumerate() {
        if i == insert_pos {
            continue;
        }
        match node {
            RegexNode::Split { first, second } => {
                if *first > insert_pos {
                    *first = first.saturating_add(1);
                }
                if *second > insert_pos {
                    *second = second.saturating_add(1);
                }
            }
            RegexNode::Jump(target) if *target > insert_pos => {
                *target = target.saturating_add(1);
            }
            _ => {}
        }
    }
}

// ============================================================================
// Regex Execution Engine (Thompson NFA simulation)
// ============================================================================

fn matches_predefined(c: char, class: PredefinedClass) -> bool {
    match class {
        PredefinedClass::Digit => c.is_ascii_digit(),
        PredefinedClass::NonDigit => !c.is_ascii_digit(),
        PredefinedClass::Word => c.is_ascii_alphanumeric() || c == '_',
        PredefinedClass::NonWord => !(c.is_ascii_alphanumeric() || c == '_'),
        PredefinedClass::Whitespace => c.is_ascii_whitespace(),
        PredefinedClass::NonWhitespace => !c.is_ascii_whitespace(),
    }
}

fn matches_char_class(c: char, chars: &[char], ranges: &[(char, char)], negated: bool) -> bool {
    let in_class = chars.contains(&c) || ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
    if negated { !in_class } else { in_class }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn execute_regex(compiled: &CompiledRegex, input: &str, start_pos: usize) -> Option<RegexMatch> {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let group_count = compiled.group_count;
    let nodes = &compiled.nodes;

    // Pike-VM simulation. We seed a fresh start-thread at every input position
    // until a match is found; this performs an unanchored leftmost scan from
    // `start_pos` while reporting the true match start. Once a match exists we
    // stop seeding new (later-starting) threads so the leftmost start is locked
    // in, and let the surviving threads run on to find the longest extension.
    let mut threads: Vec<Thread> = Vec::new();
    let mut best_match: Option<RegexMatch> = None;

    for i in start_pos..=len {
        if best_match.is_none() {
            threads.push(Thread {
                pc: 0,
                start: i,
                groups: vec![None; group_count.saturating_add(1)],
                group_starts: vec![None; group_count.saturating_add(1)],
            });
        }

        // Epsilon-closure at the current position (resolves splits, jumps,
        // group markers and anchors before we attempt to consume a character).
        add_epsilon_threads(&mut threads, nodes, &chars, i, len);

        let current_char = chars.get(i).copied();
        let mut new_threads: Vec<Thread> = Vec::new();

        for thread in &threads {
            let Some(node) = nodes.get(thread.pc) else {
                continue;
            };

            match node {
                RegexNode::Match => {
                    let m = RegexMatch {
                        start: thread.start,
                        end: i,
                        groups: thread.groups.clone(),
                    };
                    // Leftmost-longest: prefer the earliest start, and among
                    // matches with the same start, the longest extent.
                    let better = match &best_match {
                        None => true,
                        Some(prev) => {
                            m.start < prev.start || (m.start == prev.start && m.end > prev.end)
                        }
                    };
                    if better {
                        best_match = Some(m);
                    }
                }
                RegexNode::Literal(expected) if current_char == Some(*expected) => {
                    let mut new_t = thread.clone();
                    new_t.pc = thread.pc.saturating_add(1);
                    new_threads.push(new_t);
                }
                RegexNode::AnyChar => {
                    if let Some(c) = current_char
                        && c != '\n'
                    {
                        let mut new_t = thread.clone();
                        new_t.pc = thread.pc.saturating_add(1);
                        new_threads.push(new_t);
                    }
                }
                RegexNode::CharClass {
                    chars: cc,
                    ranges,
                    negated,
                } => {
                    if let Some(c) = current_char
                        && matches_char_class(c, cc, ranges, *negated)
                    {
                        let mut new_t = thread.clone();
                        new_t.pc = thread.pc.saturating_add(1);
                        new_threads.push(new_t);
                    }
                }
                RegexNode::PredefinedClass(class) => {
                    if let Some(c) = current_char
                        && matches_predefined(c, *class)
                    {
                        let mut new_t = thread.clone();
                        new_t.pc = thread.pc.saturating_add(1);
                        new_threads.push(new_t);
                    }
                }
                // Epsilon transitions are resolved in add_epsilon_threads.
                _ => {}
            }
        }

        threads = new_threads;

        // No surviving threads: if we already have a match we are done, since
        // any later start could only be further right. Otherwise keep going so
        // the next iteration can seed a fresh start-thread further along.
        if threads.is_empty() && best_match.is_some() {
            break;
        }
    }

    best_match
}

fn add_epsilon_threads(
    threads: &mut Vec<Thread>,
    nodes: &[RegexNode],
    chars: &[char],
    pos: usize,
    len: usize,
) {
    let mut i = 0;
    let mut seen: Vec<bool> = vec![false; nodes.len()];

    while i < threads.len() {
        let pc = threads[i].pc;
        if pc >= nodes.len() || seen[pc] {
            i = i.saturating_add(1);
            continue;
        }
        seen[pc] = true;

        match &nodes[pc] {
            RegexNode::Split { first, second } => {
                let mut t1 = threads[i].clone();
                t1.pc = *first;
                let mut t2 = threads[i].clone();
                t2.pc = *second;
                threads[i] = t1;
                threads.push(t2);
                // Don't increment i - process the replacement
                continue;
            }
            RegexNode::Jump(target) => {
                threads[i].pc = *target;
                continue;
            }
            RegexNode::GroupStart {
                group_id,
                capturing,
            } => {
                if *capturing {
                    threads[i].group_starts[*group_id] = Some(pos);
                }
                threads[i].pc = pc.saturating_add(1);
                continue;
            }
            RegexNode::GroupEnd { group_id } => {
                if let Some(start) = threads[i].group_starts[*group_id] {
                    threads[i].groups[*group_id] = Some((start, pos));
                }
                threads[i].pc = pc.saturating_add(1);
                continue;
            }
            RegexNode::Anchor(kind) => {
                let matches = match kind {
                    AnchorKind::Start => pos == 0,
                    AnchorKind::End => pos == len,
                };
                if matches {
                    threads[i].pc = pc.saturating_add(1);
                    continue;
                }
                // Remove non-matching thread
                threads.swap_remove(i);
                continue;
            }
            RegexNode::WordBoundary => {
                let before = if pos > 0 {
                    chars
                        .get(pos.wrapping_sub(1))
                        .is_some_and(|c| is_word_char(*c))
                } else {
                    false
                };
                let after = chars.get(pos).is_some_and(|c| is_word_char(*c));
                if before != after {
                    threads[i].pc = pc.saturating_add(1);
                    continue;
                }
                threads.swap_remove(i);
                continue;
            }
            _ => {}
        }

        i = i.saturating_add(1);
    }
}

/// Find all non-overlapping matches in the input
fn find_all_matches(compiled: &CompiledRegex, input: &str) -> Vec<RegexMatch> {
    let mut matches = Vec::new();
    let mut pos = 0;
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();

    while pos <= len && matches.len() < MAX_MATCHES {
        if let Some(m) = execute_regex(compiled, input, pos) {
            if m.end == m.start {
                // Zero-length match, advance by one
                pos = m.start.saturating_add(1);
            } else {
                pos = m.end;
            }
            matches.push(m);
        } else {
            pos = pos.saturating_add(1);
        }
    }

    matches
}

/// Apply replacement with backreferences ($0, $1, etc.)
fn apply_replacement(input: &str, matches: &[RegexMatch], replacement: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::new();
    let mut last_end = 0;

    for m in matches {
        // Append text before this match
        for &c in &chars[last_end..m.start] {
            result.push(c);
        }

        // Process replacement with backreferences
        let rep_chars: Vec<char> = replacement.chars().collect();
        let mut ri = 0;
        while ri < rep_chars.len() {
            if rep_chars[ri] == '$' {
                if let Some(&next) = rep_chars.get(ri.saturating_add(1))
                    && next.is_ascii_digit()
                {
                    let group_idx = (next as usize).wrapping_sub('0' as usize);
                    if group_idx == 0 {
                        // $0 = entire match
                        for &c in &chars[m.start..m.end] {
                            result.push(c);
                        }
                    } else if let Some(Some((gs, ge))) = m.groups.get(group_idx) {
                        for &c in &chars[*gs..*ge] {
                            result.push(c);
                        }
                    }
                    ri = ri.saturating_add(2);
                    continue;
                }
                result.push('$');
            } else if rep_chars[ri] == '\\' {
                if let Some(&next) = rep_chars.get(ri.saturating_add(1)) {
                    match next {
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        _ => result.push(next),
                    }
                    ri = ri.saturating_add(2);
                    continue;
                }
                result.push('\\');
            } else {
                result.push(rep_chars[ri]);
            }
            ri = ri.saturating_add(1);
        }

        last_end = m.end;
    }

    // Append remaining text
    for &c in &chars[last_end..] {
        result.push(c);
    }

    result
}

// ============================================================================
// Regex Explanation
// ============================================================================

fn explain_regex(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut explanations = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '^' => explanations.push("^  Start of string".into()),
            '$' => explanations.push("$  End of string".into()),
            '.' => explanations.push(".  Any character (except newline)".into()),
            '*' => {
                if chars.get(i.saturating_add(1)) == Some(&'?') {
                    explanations.push("*? Zero or more (lazy)".into());
                    i = i.saturating_add(1);
                } else {
                    explanations.push("*  Zero or more (greedy)".into());
                }
            }
            '+' => {
                if chars.get(i.saturating_add(1)) == Some(&'?') {
                    explanations.push("+? One or more (lazy)".into());
                    i = i.saturating_add(1);
                } else {
                    explanations.push("+  One or more (greedy)".into());
                }
            }
            '?' => {
                if chars.get(i.saturating_add(1)) == Some(&'?') {
                    explanations.push("?? Zero or one (lazy)".into());
                    i = i.saturating_add(1);
                } else {
                    explanations.push("?  Zero or one (greedy)".into());
                }
            }
            '|' => explanations.push("|  Alternation (OR)".into()),
            '(' => {
                if chars.get(i.saturating_add(1)) == Some(&'?')
                    && chars.get(i.saturating_add(2)) == Some(&':')
                {
                    explanations.push("(?:  Non-capturing group".into());
                    i = i.saturating_add(2);
                } else {
                    explanations.push("(  Capturing group start".into());
                }
            }
            ')' => explanations.push(")  Group end".into()),
            '[' => {
                let mut desc = String::from("[");
                let negated = chars.get(i.saturating_add(1)) == Some(&'^');
                if negated {
                    desc.push('^');
                    i = i.saturating_add(1);
                }
                i = i.saturating_add(1);
                while i < chars.len() && chars[i] != ']' {
                    desc.push(chars[i]);
                    i = i.saturating_add(1);
                }
                desc.push(']');
                if negated {
                    explanations.push(format!("{desc}  Negated character class"));
                } else {
                    explanations.push(format!("{desc}  Character class"));
                }
            }
            '\\' => {
                if let Some(&next) = chars.get(i.saturating_add(1)) {
                    let desc = match next {
                        'd' => "\\d  Digit [0-9]",
                        'D' => "\\D  Non-digit",
                        'w' => "\\w  Word char [a-zA-Z0-9_]",
                        'W' => "\\W  Non-word char",
                        's' => "\\s  Whitespace",
                        'S' => "\\S  Non-whitespace",
                        'b' => "\\b  Word boundary",
                        'n' => "\\n  Newline",
                        'r' => "\\r  Carriage return",
                        't' => "\\t  Tab",
                        _ => "",
                    };
                    if desc.is_empty() {
                        explanations.push(format!("\\{next}  Escaped literal '{next}'"));
                    } else {
                        explanations.push(desc.into());
                    }
                    i = i.saturating_add(1);
                }
            }
            '{' => {
                let mut rep = String::from("{");
                let start = i;
                i = i.saturating_add(1);
                while i < chars.len() && chars[i] != '}' {
                    rep.push(chars[i]);
                    i = i.saturating_add(1);
                }
                if i < chars.len() {
                    rep.push('}');
                    explanations.push(format!("{rep}  Repetition quantifier"));
                } else {
                    i = start;
                    explanations.push("{  Literal '{'".to_string());
                }
            }
            _ => {
                explanations.push(format!("{c}  Literal '{c}'"));
            }
        }
        i = i.saturating_add(1);
    }

    explanations
}

// ============================================================================
// Common regex patterns library
// ============================================================================

#[derive(Debug, Clone)]
struct PatternEntry {
    name: String,
    pattern: String,
    description: String,
    category: PatternCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternCategory {
    Validation,
    Extraction,
    Format,
    Network,
    DateTime,
    Programming,
    Custom,
}

impl PatternCategory {
    fn label(self) -> &'static str {
        match self {
            Self::Validation => "Validation",
            Self::Extraction => "Extraction",
            Self::Format => "Format",
            Self::Network => "Network",
            Self::DateTime => "Date/Time",
            Self::Programming => "Programming",
            Self::Custom => "Custom",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Validation => BLUE,
            Self::Extraction => GREEN,
            Self::Format => PEACH,
            Self::Network => TEAL,
            Self::DateTime => YELLOW,
            Self::Programming => MAUVE,
            Self::Custom => SUBTEXT0,
        }
    }
}

fn built_in_patterns() -> Vec<PatternEntry> {
    vec![
        PatternEntry {
            name: "Email".into(),
            pattern: r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}".into(),
            description: "Match email addresses".into(),
            category: PatternCategory::Validation,
        },
        PatternEntry {
            name: "URL".into(),
            pattern: r"https?://[a-zA-Z0-9.\-]+(?:/[^\s]*)?".into(),
            description: "Match HTTP/HTTPS URLs".into(),
            category: PatternCategory::Network,
        },
        PatternEntry {
            name: "IPv4".into(),
            pattern: r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}".into(),
            description: "Match IPv4 addresses".into(),
            category: PatternCategory::Network,
        },
        PatternEntry {
            name: "Date (YYYY-MM-DD)".into(),
            pattern: r"\d{4}-\d{2}-\d{2}".into(),
            description: "Match ISO date format".into(),
            category: PatternCategory::DateTime,
        },
        PatternEntry {
            name: "Time (HH:MM:SS)".into(),
            pattern: r"\d{2}:\d{2}(:\d{2})?".into(),
            description: "Match time format".into(),
            category: PatternCategory::DateTime,
        },
        PatternEntry {
            name: "Phone (US)".into(),
            pattern: r"(\+1)?[\s\-]?\(?\d{3}\)?[\s\-]?\d{3}[\s\-]?\d{4}".into(),
            description: "Match US phone numbers".into(),
            category: PatternCategory::Validation,
        },
        PatternEntry {
            name: "Hex Color".into(),
            pattern: r"#[0-9a-fA-F]{3,8}".into(),
            description: "Match hex color codes".into(),
            category: PatternCategory::Format,
        },
        PatternEntry {
            name: "Integer".into(),
            pattern: r"-?\d+".into(),
            description: "Match integers (with optional sign)".into(),
            category: PatternCategory::Extraction,
        },
        PatternEntry {
            name: "Float".into(),
            pattern: r"-?\d+\.\d+".into(),
            description: "Match floating point numbers".into(),
            category: PatternCategory::Extraction,
        },
        PatternEntry {
            name: "HTML Tag".into(),
            pattern: r"</?[a-zA-Z][a-zA-Z0-9]*[^>]*>".into(),
            description: "Match HTML tags".into(),
            category: PatternCategory::Programming,
        },
        PatternEntry {
            name: "Quoted String".into(),
            pattern: "\"[^\"]*\"".into(),
            description: "Match double-quoted strings".into(),
            category: PatternCategory::Programming,
        },
        PatternEntry {
            name: "C-style Comment".into(),
            pattern: r"/\*.*\*/".into(),
            description: "Match block comments".into(),
            category: PatternCategory::Programming,
        },
        PatternEntry {
            name: "Line Comment".into(),
            pattern: r"//.*$".into(),
            description: "Match line comments".into(),
            category: PatternCategory::Programming,
        },
        PatternEntry {
            name: "Words".into(),
            pattern: r"\b[a-zA-Z]+\b".into(),
            description: "Match individual words".into(),
            category: PatternCategory::Extraction,
        },
        PatternEntry {
            name: "UUID".into(),
            pattern: r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}".into(),
            description: "Match UUIDs".into(),
            category: PatternCategory::Format,
        },
        PatternEntry {
            name: "MAC Address".into(),
            pattern: r"([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}".into(),
            description: "Match MAC addresses".into(),
            category: PatternCategory::Network,
        },
        PatternEntry {
            name: "ZIP Code (US)".into(),
            pattern: r"\d{5}(-\d{4})?".into(),
            description: "Match US ZIP codes".into(),
            category: PatternCategory::Validation,
        },
        PatternEntry {
            name: "Identifier".into(),
            pattern: r"[a-zA-Z_][a-zA-Z0-9_]*".into(),
            description: "Match programming identifiers".into(),
            category: PatternCategory::Programming,
        },
    ]
}

// ============================================================================
// Application State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Tester,
    Library,
    Reference,
}

impl ActiveTab {
    fn label(self) -> &'static str {
        match self {
            Self::Tester => "Tester",
            Self::Library => "Library",
            Self::Reference => "Reference",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveField {
    Pattern,
    Input,
    Replace,
}

#[derive(Debug, Clone)]
struct MatchHighlight {
    start: usize,
    end: usize,
    group_index: usize, // 0 = full match
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    pattern: String,
    flags: RegexFlags,
    match_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct RegexFlags {
    case_insensitive: bool,
    global: bool,
    multiline: bool,
}

impl Default for RegexFlags {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            global: true,
            multiline: false,
        }
    }
}

struct App {
    // Current state
    pattern: String,
    input_text: String,
    replace_text: String,
    flags: RegexFlags,
    active_tab: ActiveTab,
    active_field: ActiveField,

    // Regex results
    compiled: Option<CompiledRegex>,
    compile_error: Option<String>,
    matches: Vec<RegexMatch>,
    highlights: Vec<MatchHighlight>,
    replace_result: Option<String>,
    explanations: Vec<String>,

    // Library
    library: Vec<PatternEntry>,
    selected_library_entry: Option<usize>,
    library_category_filter: Option<PatternCategory>,

    // History
    history: Vec<HistoryEntry>,

    // UI state
    scroll_offset: f32,
    match_scroll_offset: f32,
    current_match_index: usize,
    show_replace: bool,
    show_groups: bool,
}

impl App {
    fn new() -> Self {
        let library = built_in_patterns();
        Self {
            pattern: String::new(),
            input_text: String::new(),
            replace_text: String::new(),
            flags: RegexFlags::default(),
            active_tab: ActiveTab::Tester,
            active_field: ActiveField::Pattern,
            compiled: None,
            compile_error: None,
            matches: Vec::new(),
            highlights: Vec::new(),
            replace_result: None,
            explanations: Vec::new(),
            library,
            selected_library_entry: None,
            library_category_filter: None,
            history: Vec::new(),
            scroll_offset: 0.0,
            match_scroll_offset: 0.0,
            current_match_index: 0,
            show_replace: false,
            show_groups: true,
        }
    }

    fn update_regex(&mut self) {
        if self.pattern.is_empty() {
            self.compiled = None;
            self.compile_error = None;
            self.matches.clear();
            self.highlights.clear();
            self.replace_result = None;
            self.explanations.clear();
            return;
        }

        // Compile
        let compiler = RegexCompiler::new(&self.pattern, self.flags.case_insensitive);
        match compiler.compile() {
            Ok(regex) => {
                self.compiled = Some(regex);
                self.compile_error = None;
            }
            Err(e) => {
                self.compiled = None;
                self.compile_error = Some(format!("{e}"));
                self.matches.clear();
                self.highlights.clear();
                self.replace_result = None;
                self.explanations = explain_regex(&self.pattern);
                return;
            }
        }

        // Find matches
        if let Some(compiled) = &self.compiled {
            self.matches = find_all_matches(compiled, &self.input_text);
            if !self.flags.global && self.matches.len() > 1 {
                self.matches.truncate(1);
            }
        }

        // Build highlights
        self.highlights.clear();
        let match_colors = [0usize, 1, 2, 3, 4, 5, 6, 7];
        for (mi, m) in self.matches.iter().enumerate() {
            self.highlights.push(MatchHighlight {
                start: m.start,
                end: m.end,
                group_index: match_colors[mi % match_colors.len()],
            });
        }

        // Replace
        if self.show_replace && self.compiled.is_some() && !self.replace_text.is_empty() {
            self.replace_result = Some(apply_replacement(
                &self.input_text,
                &self.matches,
                &self.replace_text,
            ));
        } else {
            self.replace_result = None;
        }

        // Explain
        self.explanations = explain_regex(&self.pattern);

        // Clamp match index
        if self.matches.is_empty() {
            self.current_match_index = 0;
        } else if self.current_match_index >= self.matches.len() {
            self.current_match_index = self.matches.len().saturating_sub(1);
        }
    }

    fn add_to_history(&mut self) {
        if self.pattern.is_empty() || self.compile_error.is_some() {
            return;
        }

        // Don't add duplicates
        if self
            .history
            .first()
            .is_some_and(|h| h.pattern == self.pattern)
        {
            return;
        }

        self.history.insert(
            0,
            HistoryEntry {
                pattern: self.pattern.clone(),
                flags: self.flags,
                match_count: self.matches.len(),
            },
        );

        if self.history.len() > MAX_HISTORY {
            self.history.truncate(MAX_HISTORY);
        }
    }

    fn load_library_entry(&mut self, index: usize) {
        if let Some(entry) = self.library.get(index) {
            self.pattern = entry.pattern.clone();
            self.selected_library_entry = Some(index);
            self.update_regex();
        }
    }

    fn save_to_library(&mut self, name: &str) {
        if self.pattern.is_empty() || name.is_empty() || self.library.len() >= MAX_LIBRARY_ENTRIES {
            return;
        }

        self.library.push(PatternEntry {
            name: name.into(),
            pattern: self.pattern.clone(),
            description: format!("{} matches in test input", self.matches.len()),
            category: PatternCategory::Custom,
        });
    }

    fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match_index =
                (self.current_match_index.saturating_add(1)) % self.matches.len();
        }
    }

    fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            if self.current_match_index == 0 {
                self.current_match_index = self.matches.len().saturating_sub(1);
            } else {
                self.current_match_index = self.current_match_index.saturating_sub(1);
            }
        }
    }

    fn match_stats(&self) -> String {
        use std::fmt::Write as _;
        let count = self.matches.len();
        if count == 0 {
            return "No matches".into();
        }

        let total_chars: usize = self
            .matches
            .iter()
            .map(|m| m.end.saturating_sub(m.start))
            .sum();
        let group_count = self
            .matches
            .first()
            .map_or(0, |m| m.groups.iter().filter(|g| g.is_some()).count());

        let mut stats = format!("{count} match");
        if count != 1 {
            stats.push_str("es");
        }
        let _ = write!(stats, ", {total_chars} chars matched");
        if group_count > 0 {
            let _ = write!(stats, ", {group_count} group");
            if group_count != 1 {
                stats.push('s');
            }
        }
        stats
    }

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar
        self.render_toolbar(&mut cmds);

        // Tab content
        match self.active_tab {
            ActiveTab::Tester => self.render_tester_tab(&mut cmds),
            ActiveTab::Library => self.render_library_tab(&mut cmds),
            ActiveTab::Reference => Self::render_reference_tab(&mut cmds),
        }

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 13.0,
            text: "Regex Tester".into(),
            font_size: TITLE_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
        });

        // Tabs
        let tabs = [ActiveTab::Tester, ActiveTab::Library, ActiveTab::Reference];
        let mut tab_x = 170.0;
        for tab in &tabs {
            let label = tab.label();
            let w = (label.len() as f32) * CHAR_WIDTH + 20.0;
            let active = *tab == self.active_tab;

            if active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: 8.0,
                    width: w,
                    height: 28.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 10.0,
                y: 15.0,
                text: label.into(),
                font_size: NORMAL_TEXT,
                color: if active { BLUE } else { SUBTEXT0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w),
            });

            tab_x += w + 6.0;
        }

        // Flags on the right
        let flags_x = WINDOW_WIDTH - 250.0;
        let flag_items = [
            ("i", self.flags.case_insensitive, "Case insensitive"),
            ("g", self.flags.global, "Global"),
            ("m", self.flags.multiline, "Multiline"),
        ];

        for (fi, (label, active, tooltip)) in flag_items.iter().enumerate() {
            let fx = flags_x + (fi as f32) * 40.0;
            cmds.push(RenderCommand::FillRect {
                x: fx,
                y: 8.0,
                width: 30.0,
                height: 28.0,
                color: if *active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: fx + 11.0,
                y: 15.0,
                text: (*label).into(),
                font_size: NORMAL_TEXT,
                color: if *active { CRUST } else { SUBTEXT0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
            });
            let _ = tooltip; // used for hover tooltip
        }

        // Match navigation on far right
        if !self.matches.is_empty() {
            let nav_text = format!(
                "{}/{}",
                self.current_match_index.saturating_add(1),
                self.matches.len()
            );
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH - 100.0,
                y: 15.0,
                text: nav_text,
                font_size: SMALL_TEXT,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(90.0),
            });
        }
    }

    fn render_tester_tab(&self, cmds: &mut Vec<RenderCommand>) {
        let content_y = TOOLBAR_HEIGHT + PADDING;
        let content_height = WINDOW_HEIGHT - content_y - PADDING;

        // Pattern input area
        let pattern_y = content_y;
        Self::render_input_field(
            cmds,
            PADDING,
            pattern_y,
            WINDOW_WIDTH - 2.0 * PADDING,
            36.0,
            "Pattern:",
            &self.pattern,
            self.active_field == ActiveField::Pattern,
        );

        // Error or stats line
        let status_y = pattern_y + 40.0;
        if let Some(err) = &self.compile_error {
            cmds.push(RenderCommand::Text {
                x: PADDING + 80.0,
                y: status_y,
                text: format!("Error: {err}"),
                font_size: SMALL_TEXT,
                color: RED,
                font_weight: FontWeightHint::Regular,
                max_width: Some(WINDOW_WIDTH - 100.0),
            });
        } else if !self.pattern.is_empty() {
            cmds.push(RenderCommand::Text {
                x: PADDING + 80.0,
                y: status_y,
                text: self.match_stats(),
                font_size: SMALL_TEXT,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(WINDOW_WIDTH - 100.0),
            });
        }

        // Replace input (optional)
        let mut next_y = status_y + 20.0;
        if self.show_replace {
            Self::render_input_field(
                cmds,
                PADDING,
                next_y,
                WINDOW_WIDTH - 2.0 * PADDING,
                36.0,
                "Replace:",
                &self.replace_text,
                self.active_field == ActiveField::Replace,
            );
            next_y += 42.0;
        }

        // Split: left = input text, right = results
        let split_y = next_y + 4.0;
        let split_height = content_height - (split_y - content_y) - PADDING;
        let left_width = (WINDOW_WIDTH - 3.0 * PADDING) * 0.55;
        let right_x = PADDING + left_width + PADDING;
        let right_width = WINDOW_WIDTH - right_x - PADDING;

        // Input text area
        self.render_text_area(
            cmds,
            PADDING,
            split_y,
            left_width,
            split_height,
            "Test Input:",
            &self.input_text,
            self.active_field == ActiveField::Input,
        );

        // Results panel
        self.render_results_panel(cmds, right_x, split_y, right_width, split_height);
    }

    // Stateless render helper: takes the field geometry and content directly
    // rather than reading from `self`, hence an associated function.
    #[allow(clippy::too_many_arguments)]
    fn render_input_field(
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        label: &str,
        value: &str,
        focused: bool,
    ) {
        // Label
        cmds.push(RenderCommand::Text {
            x,
            y: y + 10.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
        });

        // Input background
        let input_x = x + 80.0;
        let input_width = width - 80.0;
        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y,
            width: input_width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: input_x,
            y,
            width: input_width,
            height,
            color: if focused { BLUE } else { SURFACE1 },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });

        // Text content
        let display = if value.is_empty() && !focused {
            "(empty)"
        } else {
            value
        };
        let text_color = if value.is_empty() && !focused {
            OVERLAY0
        } else {
            TEXT
        };

        cmds.push(RenderCommand::Text {
            x: input_x + 8.0,
            y: y + 10.0,
            text: truncate_display(display, ((input_width - 16.0) / CHAR_WIDTH) as usize),
            font_size: NORMAL_TEXT,
            color: text_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input_width - 16.0),
        });

        // Cursor
        if focused {
            let cursor_x = input_x + 8.0 + (value.len().min(60) as f32) * CHAR_WIDTH;
            cmds.push(RenderCommand::FillRect {
                x: cursor_x,
                y: y + 6.0,
                width: 2.0,
                height: height - 12.0,
                color: BLUE,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_text_area(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        label: &str,
        text: &str,
        focused: bool,
    ) {
        // Header
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii {
                top_left: 4.0,
                top_right: 4.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 5.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 16.0),
        });

        // Body
        let body_y = y + 24.0;
        let body_height = height - 24.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: body_y,
            width,
            height: body_height,
            color: MANTLE,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: 4.0,
                bottom_right: 4.0,
            },
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color: if focused { BLUE } else { SURFACE1 },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });

        // Render text with match highlighting
        let lines: Vec<&str> = text.split('\n').collect();
        let max_visible = ((body_height - 10.0) / LINE_HEIGHT) as usize;
        let scroll = (self.scroll_offset / LINE_HEIGHT) as usize;

        let chars_per_line = ((width - 60.0) / CHAR_WIDTH) as usize;
        let mut char_offset = 0usize;

        for (li, line) in lines.iter().enumerate().skip(scroll).take(max_visible) {
            let ly = body_y + 6.0 + ((li - scroll) as f32) * LINE_HEIGHT;

            // Line number
            cmds.push(RenderCommand::Text {
                x: x + 4.0,
                y: ly,
                text: format!("{:>3}", li.saturating_add(1)),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(30.0),
            });

            // Render text with highlights
            let line_start = char_offset;
            let line_end = char_offset.saturating_add(line.len());

            // Draw highlighted background segments
            for m in &self.matches {
                if m.end <= line_start || m.start >= line_end {
                    continue;
                }
                let hl_start = m.start.max(line_start).saturating_sub(line_start);
                let hl_end = m.end.min(line_end).saturating_sub(line_start);
                let hl_x = x + 40.0 + (hl_start as f32) * CHAR_WIDTH;
                let hl_w = ((hl_end.saturating_sub(hl_start)) as f32) * CHAR_WIDTH;

                cmds.push(RenderCommand::FillRect {
                    x: hl_x,
                    y: ly - 2.0,
                    width: hl_w,
                    height: LINE_HEIGHT,
                    color: Color::rgba(137, 180, 250, 60), // Blue highlight
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            // Line text
            let display_line = truncate_display(line, chars_per_line);
            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: ly,
                text: display_line,
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 50.0),
            });

            char_offset = line_end.saturating_add(1); // +1 for the newline
        }

        // Show line count
        cmds.push(RenderCommand::Text {
            x: x + width - 80.0,
            y: y + 5.0,
            text: format!("{} lines", lines.len()),
            font_size: SMALL_TEXT,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
        });
    }

    fn render_results_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Sub-tabs: Matches | Groups | Explanation | Replace
        let tab_labels = ["Matches", "Groups", "Explain"];
        let mut tx = x + 4.0;
        for (ti, label) in tab_labels.iter().enumerate() {
            let tw = (label.len() as f32) * CHAR_WIDTH + 16.0;
            let selected = ti == 0; // Simplified: always show matches

            if selected {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: y + 4.0,
                    width: tw,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 8.0,
                text: (*label).into(),
                font_size: SMALL_TEXT,
                color: if selected { BLUE } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(tw),
            });
            tx += tw + 4.0;
        }

        let content_y = y + 30.0;
        let content_h = height - 34.0;

        // Render matches list
        if self.matches.is_empty() {
            if self.pattern.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: content_y + 20.0,
                    text: "Enter a pattern to begin".into(),
                    font_size: NORMAL_TEXT,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            } else if self.compile_error.is_none() {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: content_y + 20.0,
                    text: "No matches found".into(),
                    font_size: NORMAL_TEXT,
                    color: YELLOW,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            }
        } else {
            self.render_match_list(cmds, x, content_y, width, content_h);
        }

        // Explanation section at bottom
        if !self.explanations.is_empty() {
            let explain_y =
                y + height - (self.explanations.len().min(6) as f32) * LINE_HEIGHT - 30.0;

            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: explain_y - 4.0,
                width: width - 8.0,
                height: 1.0,
                color: SURFACE1,
                corner_radii: CornerRadii::ZERO,
            });

            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: explain_y + 2.0,
                text: "Pattern Breakdown:".into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 16.0),
            });

            for (ei, explanation) in self.explanations.iter().take(6).enumerate() {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: explain_y + 20.0 + (ei as f32) * LINE_HEIGHT,
                    text: explanation.clone(),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            }
        }
    }

    fn render_match_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let input_chars: Vec<char> = self.input_text.chars().collect();
        let max_visible = ((height - 10.0) / (LINE_HEIGHT * 2.0)) as usize;
        let scroll = (self.match_scroll_offset / (LINE_HEIGHT * 2.0)) as usize;

        for (mi, m) in self
            .matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(max_visible)
        {
            let row_y = y + 4.0 + ((mi - scroll) as f32) * LINE_HEIGHT * 2.0;
            let is_current = mi == self.current_match_index;

            // Highlight current match row
            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: row_y,
                    width: width - 8.0,
                    height: LINE_HEIGHT * 2.0 - 4.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Match index and position
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y + 2.0,
                text: format!("#{} [{}-{}]", mi.saturating_add(1), m.start, m.end),
                font_size: SMALL_TEXT,
                color: if is_current { BLUE } else { OVERLAY0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 16.0),
            });

            // Matched text
            let matched: String = input_chars[m.start..m.end.min(input_chars.len())]
                .iter()
                .take(40)
                .collect();
            let display = if m.end.saturating_sub(m.start) > 40 {
                format!("{matched}...")
            } else {
                matched
            };

            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y + LINE_HEIGHT,
                text: format!("\"{display}\""),
                font_size: SMALL_TEXT,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
            });

            // Show groups if enabled
            if self.show_groups {
                let group_texts: Vec<String> = m
                    .groups
                    .iter()
                    .enumerate()
                    .skip(1) // skip group 0 (whole match)
                    .filter_map(|(gi, g)| {
                        g.map(|(gs, ge)| {
                            let text: String = input_chars[gs..ge.min(input_chars.len())]
                                .iter()
                                .take(20)
                                .collect();
                            format!("${gi}=\"{text}\"")
                        })
                    })
                    .collect();

                if !group_texts.is_empty() {
                    let groups_str = group_texts.join("  ");
                    cmds.push(RenderCommand::Text {
                        x: x + 120.0,
                        y: row_y + 2.0,
                        text: groups_str,
                        font_size: SMALL_TEXT,
                        color: MAUVE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width - 130.0),
                    });
                }
            }
        }
    }

    fn render_library_tab(&self, cmds: &mut Vec<RenderCommand>) {
        let content_y = TOOLBAR_HEIGHT + PADDING;

        // Category filter bar
        let categories = [
            None,
            Some(PatternCategory::Validation),
            Some(PatternCategory::Extraction),
            Some(PatternCategory::Format),
            Some(PatternCategory::Network),
            Some(PatternCategory::DateTime),
            Some(PatternCategory::Programming),
            Some(PatternCategory::Custom),
        ];
        let mut cat_x = PADDING;
        for cat in &categories {
            let label = cat.map_or("All", PatternCategory::label);
            let w = (label.len() as f32) * CHAR_WIDTH + 16.0;
            let selected = self.library_category_filter == *cat;

            cmds.push(RenderCommand::FillRect {
                x: cat_x,
                y: content_y,
                width: w,
                height: 24.0,
                color: if selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: cat_x + 8.0,
                y: content_y + 5.0,
                text: label.into(),
                font_size: SMALL_TEXT,
                color: if selected {
                    cat.map_or(BLUE, PatternCategory::color)
                } else {
                    SUBTEXT0
                },
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w),
            });
            cat_x += w + 6.0;
        }

        // Library entries
        let list_y = content_y + 34.0;
        let filtered: Vec<(usize, &PatternEntry)> = self
            .library
            .iter()
            .enumerate()
            .filter(|(_, e)| self.library_category_filter.is_none_or(|f| e.category == f))
            .collect();

        for (vi, (original_idx, entry)) in filtered.iter().enumerate() {
            let row_y = list_y + (vi as f32) * 60.0;
            if row_y > WINDOW_HEIGHT - 30.0 {
                break;
            }

            let selected = self.selected_library_entry == Some(*original_idx);

            // Row background
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: row_y,
                width: WINDOW_WIDTH - 2.0 * PADDING,
                height: 54.0,
                color: if selected { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });

            // Category badge
            let cat_label = entry.category.label();
            let badge_w = (cat_label.len() as f32) * 7.0 + 12.0;
            cmds.push(RenderCommand::FillRect {
                x: PADDING + 8.0,
                y: row_y + 6.0,
                width: badge_w,
                height: 18.0,
                color: entry.category.color(),
                corner_radii: CornerRadii::all(9.0),
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 14.0,
                y: row_y + 9.0,
                text: cat_label.into(),
                font_size: 10.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w),
            });

            // Name
            cmds.push(RenderCommand::Text {
                x: PADDING + badge_w + 16.0,
                y: row_y + 8.0,
                text: entry.name.clone(),
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
            });

            // Pattern
            cmds.push(RenderCommand::Text {
                x: PADDING + 8.0,
                y: row_y + 30.0,
                text: truncate_display(&entry.pattern, 80),
                font_size: SMALL_TEXT,
                color: SKY,
                font_weight: FontWeightHint::Regular,
                max_width: Some(WINDOW_WIDTH - 40.0),
            });

            // Description on right
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH - 250.0,
                y: row_y + 8.0,
                text: entry.description.clone(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(230.0),
            });
        }

        if filtered.is_empty() {
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH / 2.0 - 80.0,
                y: list_y + 40.0,
                text: "No patterns in this category".into(),
                font_size: NORMAL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
        }
    }

    fn render_reference_tab(cmds: &mut Vec<RenderCommand>) {
        let content_y = TOOLBAR_HEIGHT + PADDING;
        let col_width = (WINDOW_WIDTH - 3.0 * PADDING) / 2.0;

        // Left column: Syntax reference
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: content_y,
            width: col_width,
            height: WINDOW_HEIGHT - content_y - PADDING,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: content_y + 10.0,
            text: "Syntax Reference".into(),
            font_size: HEADER_TEXT,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_width - 24.0),
        });

        let syntax_items = [
            (".       ", "Any character (except newline)"),
            ("^       ", "Start of string"),
            ("$       ", "End of string"),
            ("*       ", "Zero or more"),
            ("+       ", "One or more"),
            ("?       ", "Zero or one"),
            ("{n}     ", "Exactly n times"),
            ("{n,}    ", "n or more times"),
            ("{n,m}   ", "Between n and m times"),
            ("*? +? ??", "Lazy quantifiers"),
            ("(...)   ", "Capturing group"),
            ("(?:...) ", "Non-capturing group"),
            ("a|b     ", "Alternation (a or b)"),
            ("[abc]   ", "Character class"),
            ("[^abc]  ", "Negated class"),
            ("[a-z]   ", "Character range"),
            ("\\d      ", "Digit [0-9]"),
            ("\\D      ", "Non-digit"),
            ("\\w      ", "Word char [a-zA-Z0-9_]"),
            ("\\W      ", "Non-word char"),
            ("\\s      ", "Whitespace"),
            ("\\S      ", "Non-whitespace"),
            ("\\b      ", "Word boundary"),
            ("\\n \\r \\t", "Newline, CR, Tab"),
            ("\\\\     ", "Escaped backslash"),
        ];

        for (si, (syntax, desc)) in syntax_items.iter().enumerate() {
            let sy = content_y + 36.0 + (si as f32) * LINE_HEIGHT;
            if sy > WINDOW_HEIGHT - 30.0 {
                break;
            }

            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0,
                y: sy,
                text: (*syntax).into(),
                font_size: SMALL_TEXT,
                color: GREEN,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 100.0,
                y: sy,
                text: (*desc).into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_width - 112.0),
            });
        }

        // Right column: Replace reference
        let right_x = PADDING + col_width + PADDING;
        cmds.push(RenderCommand::FillRect {
            x: right_x,
            y: content_y,
            width: col_width,
            height: WINDOW_HEIGHT - content_y - PADDING,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: right_x + 12.0,
            y: content_y + 10.0,
            text: "Replacement Reference".into(),
            font_size: HEADER_TEXT,
            color: PEACH,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_width - 24.0),
        });

        let replace_items = [
            ("$0     ", "Entire match"),
            ("$1-$9  ", "Capture group N"),
            ("\\n     ", "Newline"),
            ("\\t     ", "Tab"),
            ("\\\\    ", "Literal backslash"),
        ];

        for (ri, (syntax, desc)) in replace_items.iter().enumerate() {
            let ry = content_y + 36.0 + (ri as f32) * LINE_HEIGHT;

            cmds.push(RenderCommand::Text {
                x: right_x + 12.0,
                y: ry,
                text: (*syntax).into(),
                font_size: SMALL_TEXT,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
            });
            cmds.push(RenderCommand::Text {
                x: right_x + 100.0,
                y: ry,
                text: (*desc).into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_width - 112.0),
            });
        }

        // Tips section
        let tips_y = content_y + 140.0;
        cmds.push(RenderCommand::Text {
            x: right_x + 12.0,
            y: tips_y,
            text: "Tips & Tricks".into(),
            font_size: HEADER_TEXT,
            color: TEAL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_width - 24.0),
        });

        let tips = [
            "Use \\b for word boundaries to avoid partial matches",
            "Character classes [] are faster than alternation |",
            "Non-capturing groups (?:) when you don't need the capture",
            "Use lazy quantifiers *? +? to match as little as possible",
            "Anchors ^ $ don't consume characters",
            "Escape special chars with \\ when matching literally",
            "Test patterns incrementally - start simple, add complexity",
        ];

        for (ti, tip) in tips.iter().enumerate() {
            let ty = tips_y + 26.0 + (ti as f32) * LINE_HEIGHT;
            if ty > WINDOW_HEIGHT - 30.0 {
                break;
            }

            cmds.push(RenderCommand::Text {
                x: right_x + 16.0,
                y: ty,
                text: format!("- {tip}"),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_width - 28.0),
            });
        }
    }
}

fn truncate_display(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.into()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let app = App::new();
    let _cmds = app.render();
    // In the real OS, this would enter the GUI event loop
    // For now, validate that rendering works
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Regex compilation tests ---

    #[test]
    fn test_compile_empty() {
        let compiler = RegexCompiler::new("", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_literal() {
        let compiler = RegexCompiler::new("abc", false);
        let result = compiler.compile().unwrap();
        assert!(result.nodes.len() >= 3); // 3 literals + match
    }

    #[test]
    fn test_compile_dot() {
        let compiler = RegexCompiler::new("a.c", false);
        let result = compiler.compile().unwrap();
        assert!(result.nodes.iter().any(|n| matches!(n, RegexNode::AnyChar)));
    }

    #[test]
    fn test_compile_char_class() {
        let compiler = RegexCompiler::new("[abc]", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::CharClass { .. }))
        );
    }

    #[test]
    fn test_compile_negated_class() {
        let compiler = RegexCompiler::new("[^abc]", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::CharClass { negated: true, .. }))
        );
    }

    #[test]
    fn test_compile_char_range() {
        let compiler = RegexCompiler::new("[a-z]", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::CharClass { ranges, .. } if !ranges.is_empty()))
        );
    }

    #[test]
    fn test_compile_predefined_digit() {
        let compiler = RegexCompiler::new("\\d", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::PredefinedClass(PredefinedClass::Digit)))
        );
    }

    #[test]
    fn test_compile_predefined_word() {
        let compiler = RegexCompiler::new("\\w", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::PredefinedClass(PredefinedClass::Word)))
        );
    }

    #[test]
    fn test_compile_predefined_whitespace() {
        let compiler = RegexCompiler::new("\\s", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::PredefinedClass(PredefinedClass::Whitespace)))
        );
    }

    #[test]
    fn test_compile_anchor_start() {
        let compiler = RegexCompiler::new("^abc", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::Anchor(AnchorKind::Start)))
        );
    }

    #[test]
    fn test_compile_anchor_end() {
        let compiler = RegexCompiler::new("abc$", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::Anchor(AnchorKind::End)))
        );
    }

    #[test]
    fn test_compile_group() {
        let compiler = RegexCompiler::new("(abc)", false);
        let result = compiler.compile().unwrap();
        assert_eq!(result.group_count, 1);
    }

    #[test]
    fn test_compile_non_capturing_group() {
        let compiler = RegexCompiler::new("(?:abc)", false);
        let result = compiler.compile().unwrap();
        assert_eq!(result.group_count, 0);
    }

    #[test]
    fn test_compile_nested_groups() {
        let compiler = RegexCompiler::new("(a(b)c)", false);
        let result = compiler.compile().unwrap();
        assert_eq!(result.group_count, 2);
    }

    #[test]
    fn test_compile_quantifier_star() {
        let compiler = RegexCompiler::new("a*", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::Split { .. }))
        );
    }

    #[test]
    fn test_compile_quantifier_plus() {
        let compiler = RegexCompiler::new("a+", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_quantifier_question() {
        let compiler = RegexCompiler::new("a?", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_alternation() {
        let compiler = RegexCompiler::new("a|b", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_escape_special() {
        let compiler = RegexCompiler::new("\\.", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::Literal('.')))
        );
    }

    #[test]
    fn test_compile_error_unmatched_paren() {
        let compiler = RegexCompiler::new("(abc", false);
        let result = compiler.compile();
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_error_trailing_backslash() {
        let compiler = RegexCompiler::new("abc\\", false);
        let result = compiler.compile();
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_word_boundary() {
        let compiler = RegexCompiler::new("\\bword\\b", false);
        let result = compiler.compile().unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|n| matches!(n, RegexNode::WordBoundary))
        );
    }

    #[test]
    fn test_compile_repetition_exact() {
        let compiler = RegexCompiler::new("a{3}", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_repetition_range() {
        let compiler = RegexCompiler::new("a{2,4}", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_repetition_min() {
        let compiler = RegexCompiler::new("a{2,}", false);
        let result = compiler.compile();
        assert!(result.is_ok());
    }

    // --- Matching tests ---

    #[test]
    fn test_match_literal() {
        let compiled = RegexCompiler::new("hello", false).compile().unwrap();
        let m = execute_regex(&compiled, "hello world", 0);
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn test_match_dot() {
        let compiled = RegexCompiler::new("h.llo", false).compile().unwrap();
        let m = execute_regex(&compiled, "hello", 0);
        assert!(m.is_some());
    }

    #[test]
    fn test_match_star() {
        let compiled = RegexCompiler::new("ab*c", false).compile().unwrap();
        assert!(execute_regex(&compiled, "ac", 0).is_some());
        assert!(execute_regex(&compiled, "abc", 0).is_some());
        assert!(execute_regex(&compiled, "abbbc", 0).is_some());
    }

    #[test]
    fn test_match_plus() {
        let compiled = RegexCompiler::new("ab+c", false).compile().unwrap();
        assert!(execute_regex(&compiled, "ac", 0).is_none());
        assert!(execute_regex(&compiled, "abc", 0).is_some());
        assert!(execute_regex(&compiled, "abbc", 0).is_some());
    }

    #[test]
    fn test_match_question() {
        let compiled = RegexCompiler::new("ab?c", false).compile().unwrap();
        assert!(execute_regex(&compiled, "ac", 0).is_some());
        assert!(execute_regex(&compiled, "abc", 0).is_some());
    }

    #[test]
    fn test_match_char_class() {
        let compiled = RegexCompiler::new("[abc]", false).compile().unwrap();
        assert!(execute_regex(&compiled, "a", 0).is_some());
        assert!(execute_regex(&compiled, "b", 0).is_some());
        assert!(execute_regex(&compiled, "d", 0).is_none());
    }

    #[test]
    fn test_match_negated_class() {
        let compiled = RegexCompiler::new("[^abc]", false).compile().unwrap();
        assert!(execute_regex(&compiled, "d", 0).is_some());
        assert!(execute_regex(&compiled, "a", 0).is_none());
    }

    #[test]
    fn test_match_digit() {
        let compiled = RegexCompiler::new("\\d+", false).compile().unwrap();
        let m = execute_regex(&compiled, "abc123def", 0);
        assert!(m.is_some());
        let m = m.unwrap();
        // Unanchored leftmost scan: the first digit run "123" begins at index 3.
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_match_anchor_start() {
        let compiled = RegexCompiler::new("^hello", false).compile().unwrap();
        assert!(execute_regex(&compiled, "hello world", 0).is_some());
        assert!(execute_regex(&compiled, "say hello", 0).is_none());
    }

    #[test]
    fn test_match_anchor_end() {
        let compiled = RegexCompiler::new("world$", false).compile().unwrap();
        assert!(execute_regex(&compiled, "hello world", 6).is_some());
    }

    #[test]
    fn test_match_group_capture() {
        let compiled = RegexCompiler::new("(\\d+)-(\\d+)", false)
            .compile()
            .unwrap();
        let m = execute_regex(&compiled, "123-456", 0);
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.groups.len(), 3); // group 0 (unused) + groups 1 and 2
    }

    #[test]
    fn test_match_case_insensitive() {
        let compiled = RegexCompiler::new("hello", true).compile().unwrap();
        assert!(execute_regex(&compiled, "HELLO", 0).is_some());
        assert!(execute_regex(&compiled, "Hello", 0).is_some());
    }

    // --- Find all matches tests ---

    #[test]
    fn test_find_all_simple() {
        let compiled = RegexCompiler::new("\\d+", false).compile().unwrap();
        let matches = find_all_matches(&compiled, "a1b22c333");
        assert!(matches.len() >= 2); // should find multiple digit sequences
    }

    #[test]
    fn test_find_all_empty_input() {
        let compiled = RegexCompiler::new("abc", false).compile().unwrap();
        let matches = find_all_matches(&compiled, "");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_all_no_match() {
        let compiled = RegexCompiler::new("xyz", false).compile().unwrap();
        let matches = find_all_matches(&compiled, "hello world");
        assert!(matches.is_empty());
    }

    // --- Replacement tests ---

    #[test]
    fn test_replace_simple() {
        let compiled = RegexCompiler::new("world", false).compile().unwrap();
        let matches = find_all_matches(&compiled, "hello world");
        let result = apply_replacement("hello world", &matches, "earth");
        assert_eq!(result, "hello earth");
    }

    #[test]
    fn test_replace_backreference() {
        let compiled = RegexCompiler::new("(\\w+)@(\\w+)", false)
            .compile()
            .unwrap();
        let matches = find_all_matches(&compiled, "user@host");
        let result = apply_replacement("user@host", &matches, "$1 at $2");
        // Should replace with group captures
        assert!(result.contains("at"));
    }

    #[test]
    fn test_replace_no_match() {
        let compiled = RegexCompiler::new("xyz", false).compile().unwrap();
        let matches = find_all_matches(&compiled, "hello");
        let result = apply_replacement("hello", &matches, "replacement");
        assert_eq!(result, "hello");
    }

    // --- Explanation tests ---

    #[test]
    fn test_explain_literal() {
        let explanations = explain_regex("abc");
        assert_eq!(explanations.len(), 3);
        assert!(explanations[0].contains("Literal"));
    }

    #[test]
    fn test_explain_special_chars() {
        let explanations = explain_regex("^.$");
        assert!(explanations.iter().any(|e| e.contains("Start")));
        assert!(explanations.iter().any(|e| e.contains("Any")));
        assert!(explanations.iter().any(|e| e.contains("End")));
    }

    #[test]
    fn test_explain_quantifiers() {
        let explanations = explain_regex("a*b+c?");
        assert!(explanations.iter().any(|e| e.contains("Zero or more")));
        assert!(explanations.iter().any(|e| e.contains("One or more")));
        assert!(explanations.iter().any(|e| e.contains("Zero or one")));
    }

    #[test]
    fn test_explain_classes() {
        let explanations = explain_regex("\\d\\w\\s");
        assert!(explanations.iter().any(|e| e.contains("Digit")));
        assert!(explanations.iter().any(|e| e.contains("Word")));
        assert!(explanations.iter().any(|e| e.contains("Whitespace")));
    }

    #[test]
    fn test_explain_group() {
        let explanations = explain_regex("(abc)");
        assert!(explanations.iter().any(|e| e.contains("Capturing group")));
        assert!(explanations.iter().any(|e| e.contains("Group end")));
    }

    #[test]
    fn test_explain_non_capturing() {
        let explanations = explain_regex("(?:abc)");
        assert!(explanations.iter().any(|e| e.contains("Non-capturing")));
    }

    // --- Pattern library tests ---

    #[test]
    fn test_builtin_patterns_not_empty() {
        let patterns = built_in_patterns();
        assert!(!patterns.is_empty());
        assert!(patterns.len() >= 15);
    }

    #[test]
    fn test_builtin_patterns_compile() {
        let patterns = built_in_patterns();
        for entry in &patterns {
            let compiler = RegexCompiler::new(&entry.pattern, false);
            let result = compiler.compile();
            assert!(
                result.is_ok(),
                "Failed to compile pattern '{}': {:?}",
                entry.name,
                result.err().map(|e| e.message)
            );
        }
    }

    #[test]
    fn test_email_pattern_matches() {
        let patterns = built_in_patterns();
        let email_pattern = patterns.iter().find(|p| p.name == "Email").unwrap();
        let compiled = RegexCompiler::new(&email_pattern.pattern, false)
            .compile()
            .unwrap();
        assert!(execute_regex(&compiled, "user@example.com", 0).is_some());
    }

    #[test]
    fn test_integer_pattern_matches() {
        let patterns = built_in_patterns();
        let int_pattern = patterns.iter().find(|p| p.name == "Integer").unwrap();
        let compiled = RegexCompiler::new(&int_pattern.pattern, false)
            .compile()
            .unwrap();
        assert!(execute_regex(&compiled, "42", 0).is_some());
        assert!(execute_regex(&compiled, "-7", 0).is_some());
    }

    // --- Predefined class tests ---

    #[test]
    fn test_predefined_digit_match() {
        assert!(matches_predefined('5', PredefinedClass::Digit));
        assert!(!matches_predefined('a', PredefinedClass::Digit));
    }

    #[test]
    fn test_predefined_word_match() {
        assert!(matches_predefined('a', PredefinedClass::Word));
        assert!(matches_predefined('_', PredefinedClass::Word));
        assert!(!matches_predefined(' ', PredefinedClass::Word));
    }

    #[test]
    fn test_predefined_whitespace_match() {
        assert!(matches_predefined(' ', PredefinedClass::Whitespace));
        assert!(matches_predefined('\t', PredefinedClass::Whitespace));
        assert!(!matches_predefined('a', PredefinedClass::Whitespace));
    }

    #[test]
    fn test_predefined_non_digit() {
        assert!(!matches_predefined('5', PredefinedClass::NonDigit));
        assert!(matches_predefined('a', PredefinedClass::NonDigit));
    }

    #[test]
    fn test_predefined_non_word() {
        assert!(!matches_predefined('a', PredefinedClass::NonWord));
        assert!(matches_predefined(' ', PredefinedClass::NonWord));
    }

    #[test]
    fn test_predefined_non_whitespace() {
        assert!(!matches_predefined(' ', PredefinedClass::NonWhitespace));
        assert!(matches_predefined('a', PredefinedClass::NonWhitespace));
    }

    // --- App state tests ---

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert!(app.pattern.is_empty());
        assert!(app.input_text.is_empty());
        assert!(app.matches.is_empty());
        assert_eq!(app.active_tab, ActiveTab::Tester);
    }

    #[test]
    fn test_app_update_empty_pattern() {
        let mut app = App::new();
        app.update_regex();
        assert!(app.compiled.is_none());
        assert!(app.compile_error.is_none());
    }

    #[test]
    fn test_app_update_valid_pattern() {
        let mut app = App::new();
        app.pattern = "\\d+".into();
        app.input_text = "abc123def456".into();
        app.update_regex();
        assert!(app.compiled.is_some());
        assert!(app.compile_error.is_none());
        assert!(!app.matches.is_empty());
    }

    #[test]
    fn test_app_update_invalid_pattern() {
        let mut app = App::new();
        app.pattern = "(unclosed".into();
        app.update_regex();
        assert!(app.compile_error.is_some());
        assert!(app.matches.is_empty());
    }

    #[test]
    fn test_app_match_navigation() {
        let mut app = App::new();
        app.pattern = "\\d".into();
        app.input_text = "a1b2c3".into();
        app.update_regex();

        let total = app.matches.len();
        assert!(total >= 2);

        app.next_match();
        assert_eq!(app.current_match_index, 1);

        app.prev_match();
        assert_eq!(app.current_match_index, 0);

        app.prev_match(); // wraps around
        assert_eq!(app.current_match_index, total.saturating_sub(1));
    }

    #[test]
    fn test_app_history() {
        let mut app = App::new();
        app.pattern = "\\d+".into();
        app.input_text = "123".into();
        app.update_regex();
        app.add_to_history();
        assert_eq!(app.history.len(), 1);

        // Don't add duplicates
        app.add_to_history();
        assert_eq!(app.history.len(), 1);

        // Add different pattern
        app.pattern = "\\w+".into();
        app.update_regex();
        app.add_to_history();
        assert_eq!(app.history.len(), 2);
    }

    #[test]
    fn test_app_load_library() {
        let mut app = App::new();
        app.load_library_entry(0);
        assert!(app.selected_library_entry.is_some());
        assert!(!app.pattern.is_empty());
    }

    #[test]
    fn test_app_save_to_library() {
        let mut app = App::new();
        let initial_len = app.library.len();
        app.pattern = "custom_pattern".into();
        app.save_to_library("My Pattern");
        assert_eq!(app.library.len(), initial_len + 1);
    }

    #[test]
    fn test_app_save_empty_rejected() {
        let mut app = App::new();
        let initial_len = app.library.len();
        app.save_to_library(""); // empty name
        assert_eq!(app.library.len(), initial_len);

        app.pattern = "".into();
        app.save_to_library("Test"); // empty pattern
        assert_eq!(app.library.len(), initial_len);
    }

    #[test]
    fn test_app_match_stats_empty() {
        let app = App::new();
        assert_eq!(app.match_stats(), "No matches");
    }

    #[test]
    fn test_app_match_stats_with_matches() {
        let mut app = App::new();
        app.pattern = "\\d+".into();
        app.input_text = "abc123".into();
        app.update_regex();
        let stats = app.match_stats();
        assert!(stats.contains("match"));
    }

    #[test]
    fn test_app_replace() {
        let mut app = App::new();
        app.pattern = "world".into();
        app.input_text = "hello world".into();
        app.replace_text = "earth".into();
        app.show_replace = true;
        app.update_regex();
        assert!(app.replace_result.is_some());
    }

    #[test]
    fn test_app_render() {
        let app = App::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_all_tabs() {
        let mut app = App::new();

        app.active_tab = ActiveTab::Tester;
        let cmds1 = app.render();
        assert!(!cmds1.is_empty());

        app.active_tab = ActiveTab::Library;
        let cmds2 = app.render();
        assert!(!cmds2.is_empty());

        app.active_tab = ActiveTab::Reference;
        let cmds3 = app.render();
        assert!(!cmds3.is_empty());
    }

    // --- Utility tests ---

    #[test]
    fn test_truncate_display_short() {
        assert_eq!(truncate_display("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_display_long() {
        let result = truncate_display("hello world this is long", 10);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 10);
    }

    #[test]
    fn test_char_class_match() {
        assert!(matches_char_class('a', &['a', 'b', 'c'], &[], false));
        assert!(!matches_char_class('d', &['a', 'b', 'c'], &[], false));
    }

    #[test]
    fn test_char_class_range() {
        assert!(matches_char_class('m', &[], &[('a', 'z')], false));
        assert!(!matches_char_class('M', &[], &[('a', 'z')], false));
    }

    #[test]
    fn test_char_class_negated() {
        assert!(!matches_char_class('a', &['a', 'b'], &[], true));
        assert!(matches_char_class('c', &['a', 'b'], &[], true));
    }

    #[test]
    fn test_is_word_char() {
        assert!(is_word_char('a'));
        assert!(is_word_char('Z'));
        assert!(is_word_char('5'));
        assert!(is_word_char('_'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('-'));
    }

    // --- Category tests ---

    #[test]
    fn test_pattern_category_labels() {
        assert_eq!(PatternCategory::Validation.label(), "Validation");
        assert_eq!(PatternCategory::Network.label(), "Network");
        assert_eq!(PatternCategory::Custom.label(), "Custom");
    }

    #[test]
    fn test_regex_error_display() {
        let err = RegexError {
            message: "bad pattern".into(),
            position: 5,
        };
        let display = format!("{err}");
        assert!(display.contains("position 5"));
        assert!(display.contains("bad pattern"));
    }

    #[test]
    fn test_flags_default() {
        let flags = RegexFlags::default();
        assert!(!flags.case_insensitive);
        assert!(flags.global);
        assert!(!flags.multiline);
    }

    #[test]
    fn test_app_case_insensitive_matching() {
        let mut app = App::new();
        app.pattern = "hello".into();
        app.input_text = "Hello HELLO hello".into();
        app.flags.case_insensitive = true;
        app.update_regex();
        assert!(app.matches.len() >= 2);
    }
}
