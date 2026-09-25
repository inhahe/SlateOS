//! `Slate OS` Regex Tester & Debugger
//!
//! An interactive regex testing tool with:
//! - Its own engine (a Pike VM) with the common syntax: classes, `\d \w \s
//!   \b`, lazy quantifiers, non-capturing groups
//! - Matches highlighted in the text as the pattern is typed, the current one
//!   more strongly
//! - Each match's capture groups, and the current match's in a table
//! - A breakdown of the pattern, piece by piece
//! - Find & replace with `$0`-`$9`, and the result shown
//! - A library of common patterns, and the user's own, saved with their flags
//!   and kept between sessions (`settingsfile`, `regextester.yaml`)
//! - A multi-line test input with a caret, a selection and a clipboard
//! - Case-insensitive, global and multiline flags
//! - A syntax reference
//!
//! Every control answers the pointer -- the renderer records a hit box where
//! it draws each one (`guitk::frame::Frame`) -- and every key is on the F1
//! card.

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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::theme::with_alpha;
use guitk::wheel;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const PADDING: f32 = 10.0;
const LINE_HEIGHT: f32 = 20.0;
const SMALL_TEXT: f32 = 12.0;
/// Font size of the category badge on a library row.
const BADGE_TEXT: f32 = 10.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;

// Maximum limits
const MAX_PATTERN_LEN: usize = 512;
const MAX_INPUT_LEN: usize = 16384;
const MAX_REPLACE_LEN: usize = 512;
const MAX_MATCHES: usize = 1000;
const _: () = assert!(
    MAX_INPUT_LEN > MAX_PATTERN_LEN,
    "the text being searched is expected to dwarf the pattern it is searched with; if that stops being true the field limits want rethinking rather than swapping"
);

/// How many saved patterns the library will hold.
///
/// Unenforced, because nothing saves one: `save_to_library` has no caller.
/// See `todo.txt`.
#[allow(dead_code, reason = "the library has no control that reaches it")]
const MAX_LIBRARY_ENTRIES: usize = 100;
/// How many past patterns to remember. Unenforced for the same reason:
/// `add_to_history` has no caller, so the history is always empty.
#[allow(dead_code, reason = "nothing writes to the history")]
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
    /// Whether `^` and `$` also match at line boundaries.
    ///
    /// This was a field on the *app* that the engine never read: the window
    /// drew an `m` button, coloured it by the flag, and the matcher's anchor
    /// arm was `pos == 0` and `pos == len` regardless. A toggle for a flag
    /// with no effect is worse than no toggle, because the button is a claim.
    multiline: bool,
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
    multiline: bool,
}

impl RegexCompiler {
    fn new(pattern: &str, case_insensitive: bool) -> Self {
        Self {
            pattern: pattern.chars().collect(),
            pos: 0,
            nodes: Vec::new(),
            group_count: 0,
            case_insensitive,
            multiline: false,
        }
    }

    /// Compile with `^` and `$` matching at every line boundary.
    ///
    /// A setter rather than a third argument to `new`, because `new` has 47
    /// call sites and every one of them is a test that does not care about
    /// this flag. Widening the signature would have edited 47 lines to say
    /// `false`.
    fn multiline(mut self, on: bool) -> Self {
        self.multiline = on;
        self
    }

    fn compile(mut self) -> Result<CompiledRegex, RegexError> {
        self.parse_alternation()?;
        self.nodes.push(RegexNode::Match);
        Ok(CompiledRegex {
            nodes: self.nodes,
            group_count: self.group_count,
            multiline: self.multiline,
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
                    let Some(mut node) = self.nodes.get(j).cloned() else {
                        continue;
                    };
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
        let atom_nodes: Vec<RegexNode> = self
            .nodes
            .get(atom_start..atom_end)
            .map(<[RegexNode]>::to_vec)
            .unwrap_or_default();
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

/// Run `compiled` over `input` from `start_pos`. **Tests only**: a search
/// collects the characters once and calls [`execute_regex_chars`].
#[cfg(test)]
fn execute_regex(compiled: &CompiledRegex, input: &str, start_pos: usize) -> Option<RegexMatch> {
    let chars: Vec<char> = input.chars().collect();
    execute_regex_chars(compiled, &chars, start_pos)
}

/// [`execute_regex`] over the input's characters, collected by the caller.
///
/// `find_all_matches` called `execute_regex` once per match and each call
/// collected the whole input into a new `Vec<char>`: with the input at its
/// 16 384-character limit and a thousand matches, one keystroke copied
/// sixteen million characters before it matched any.
fn execute_regex_chars(
    compiled: &CompiledRegex,
    chars: &[char],
    start_pos: usize,
) -> Option<RegexMatch> {
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
        add_epsilon_threads(&mut threads, nodes, chars, i, len, compiled.multiline);

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

// The NFA simulation indexes by *its own* program counter and thread index,
// both of which it produces and bounds itself: `pc >= nodes.len()` is checked
// at the head of the loop below, and every `pc` written afterwards is either
// that one plus a step or a successor index the compiler emitted. Converting
// each access to `get` would force an `else` branch at every step whose only
// honest content is "this cannot happen because the loop head just checked" —
// which is noise that hides the one place the invariant is actually
// established. Same argument as `apps/paint`'s rasterizer.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn add_epsilon_threads(
    threads: &mut Vec<Thread>,
    nodes: &[RegexNode],
    chars: &[char],
    pos: usize,
    len: usize,
    multiline: bool,
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
                // In multiline mode a line boundary is a start and an end, so
                // `^` matches after every newline and `$` before every one.
                // `chars.get` rather than indexing: `pos` runs to `len`
                // inclusive, so `pos` is a valid index only when it is not the
                // end, and the end is exactly where `$` matches anyway.
                let matches = match kind {
                    AnchorKind::Start => {
                        pos == 0
                            || (multiline
                                && pos.checked_sub(1).and_then(|p| chars.get(p)) == Some(&'\n'))
                    }
                    AnchorKind::End => pos == len || (multiline && chars.get(pos) == Some(&'\n')),
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
        if let Some(m) = execute_regex_chars(compiled, &chars, pos) {
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
        for &c in chars.get(last_end..m.start).unwrap_or_default() {
            result.push(c);
        }

        // Process replacement with backreferences
        let rep_chars: Vec<char> = replacement.chars().collect();
        let mut ri = 0;
        while ri < rep_chars.len() {
            if rep_chars.get(ri) == Some(&'$') {
                if let Some(&next) = rep_chars.get(ri.saturating_add(1))
                    && next.is_ascii_digit()
                {
                    let group_idx = (next as usize).wrapping_sub('0' as usize);
                    if group_idx == 0 {
                        // $0 = entire match
                        for &c in chars.get(m.start..m.end).unwrap_or_default() {
                            result.push(c);
                        }
                    } else if let Some(Some((gs, ge))) = m.groups.get(group_idx) {
                        for &c in chars.get(*gs..*ge).unwrap_or_default() {
                            result.push(c);
                        }
                    }
                    ri = ri.saturating_add(2);
                    continue;
                }
                result.push('$');
            } else if rep_chars.get(ri) == Some(&'\\') {
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
            } else if let Some(&c) = rep_chars.get(ri) {
                result.push(c);
            }
            ri = ri.saturating_add(1);
        }

        last_end = m.end;
    }

    // Append remaining text
    for &c in chars.get(last_end..).unwrap_or_default() {
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
        let Some(&c) = chars.get(i) else {
            break;
        };
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
                while let Some(&c) = chars.get(i).filter(|c| **c != ']') {
                    desc.push(c);
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
                while let Some(&c) = chars.get(i).filter(|c| **c != '}') {
                    rep.push(c);
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
    /// The flags a saved pattern was saved with, which loading it restores:
    /// what a pattern matches depends on them. `None` for the built-in ones,
    /// which leave the flags as they are.
    flags: Option<RegexFlags>,
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

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Validation => pal.blue,
            Self::Extraction => pal.green,
            Self::Format => pal.peach,
            Self::Network => pal.teal,
            Self::DateTime => pal.yellow,
            Self::Programming => pal.mauve,
            Self::Custom => pal.subtext0,
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
            flags: None,
        },
        PatternEntry {
            name: "URL".into(),
            pattern: r"https?://[a-zA-Z0-9.\-]+(?:/[^\s]*)?".into(),
            description: "Match HTTP/HTTPS URLs".into(),
            category: PatternCategory::Network,
            flags: None,
        },
        PatternEntry {
            name: "IPv4".into(),
            pattern: r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}".into(),
            description: "Match IPv4 addresses".into(),
            category: PatternCategory::Network,
            flags: None,
        },
        PatternEntry {
            name: "Date (YYYY-MM-DD)".into(),
            pattern: r"\d{4}-\d{2}-\d{2}".into(),
            description: "Match ISO date format".into(),
            category: PatternCategory::DateTime,
            flags: None,
        },
        PatternEntry {
            name: "Time (HH:MM:SS)".into(),
            pattern: r"\d{2}:\d{2}(:\d{2})?".into(),
            description: "Match time format".into(),
            category: PatternCategory::DateTime,
            flags: None,
        },
        PatternEntry {
            name: "Phone (US)".into(),
            pattern: r"(\+1)?[\s\-]?\(?\d{3}\)?[\s\-]?\d{3}[\s\-]?\d{4}".into(),
            description: "Match US phone numbers".into(),
            category: PatternCategory::Validation,
            flags: None,
        },
        PatternEntry {
            name: "Hex Color".into(),
            pattern: r"#[0-9a-fA-F]{3,8}".into(),
            description: "Match hex color codes".into(),
            category: PatternCategory::Format,
            flags: None,
        },
        PatternEntry {
            name: "Integer".into(),
            pattern: r"-?\d+".into(),
            description: "Match integers (with optional sign)".into(),
            category: PatternCategory::Extraction,
            flags: None,
        },
        PatternEntry {
            name: "Float".into(),
            pattern: r"-?\d+\.\d+".into(),
            description: "Match floating point numbers".into(),
            category: PatternCategory::Extraction,
            flags: None,
        },
        PatternEntry {
            name: "HTML Tag".into(),
            pattern: r"</?[a-zA-Z][a-zA-Z0-9]*[^>]*>".into(),
            description: "Match HTML tags".into(),
            category: PatternCategory::Programming,
            flags: None,
        },
        PatternEntry {
            name: "Quoted String".into(),
            pattern: "\"[^\"]*\"".into(),
            description: "Match double-quoted strings".into(),
            category: PatternCategory::Programming,
            flags: None,
        },
        PatternEntry {
            name: "C-style Comment".into(),
            pattern: r"/\*.*\*/".into(),
            description: "Match block comments".into(),
            category: PatternCategory::Programming,
            flags: None,
        },
        PatternEntry {
            name: "Line Comment".into(),
            pattern: r"//.*$".into(),
            description: "Match line comments".into(),
            category: PatternCategory::Programming,
            flags: None,
        },
        PatternEntry {
            name: "Words".into(),
            pattern: r"\b[a-zA-Z]+\b".into(),
            description: "Match individual words".into(),
            category: PatternCategory::Extraction,
            flags: None,
        },
        PatternEntry {
            name: "UUID".into(),
            pattern: r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}".into(),
            description: "Match UUIDs".into(),
            category: PatternCategory::Format,
            flags: None,
        },
        PatternEntry {
            name: "MAC Address".into(),
            pattern: r"([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}".into(),
            description: "Match MAC addresses".into(),
            category: PatternCategory::Network,
            flags: None,
        },
        PatternEntry {
            name: "ZIP Code (US)".into(),
            pattern: r"\d{5}(-\d{4})?".into(),
            description: "Match US ZIP codes".into(),
            category: PatternCategory::Validation,
            flags: None,
        },
        PatternEntry {
            name: "Identifier".into(),
            pattern: r"[a-zA-Z_][a-zA-Z0-9_]*".into(),
            description: "Match programming identifiers".into(),
            category: PatternCategory::Programming,
            flags: None,
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
    const ALL: [Self; 3] = [Self::Tester, Self::Library, Self::Reference];

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

/// What the results panel beside the test input shows.
///
/// The panel drew a strip of three sub-tabs -- Matches, Groups, Explain --
/// with the first always lit ("Simplified: always show matches") and nothing
/// behind the other two. The breakdown was drawn under the matches instead,
/// cut at six lines, so a longer pattern's explanation could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultsView {
    Matches,
    Groups,
    Explain,
}

impl ResultsView {
    const ALL: [Self; 3] = [Self::Matches, Self::Groups, Self::Explain];

    fn label(self) -> &'static str {
        match self {
            Self::Matches => "Matches",
            Self::Groups => "Groups",
            Self::Explain => "Explain",
        }
    }

    /// How tall one of its rows is.
    fn row_height(self) -> f32 {
        match self {
            Self::Matches => LINE_HEIGHT * 2.0,
            Self::Groups | Self::Explain => LINE_HEIGHT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl RegexFlags {
    /// The flags as the letters the toolbar labels them with, in its order:
    /// how a saved pattern records them.
    fn letters(self) -> String {
        [
            (self.case_insensitive, 'i'),
            (self.global, 'g'),
            (self.multiline, 'm'),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, c)| *c)
        .collect()
    }

    /// The flags a saved pattern recorded; a letter this does not know is
    /// ignored rather than refusing the pattern.
    fn from_letters(letters: &str) -> Self {
        Self {
            case_insensitive: letters.contains('i'),
            global: letters.contains('g'),
            multiline: letters.contains('m'),
        }
    }

    fn get(self, flag: Flag) -> bool {
        match flag {
            Flag::CaseInsensitive => self.case_insensitive,
            Flag::Global => self.global,
            Flag::Multiline => self.multiline,
        }
    }

    fn toggle(&mut self, flag: Flag) {
        match flag {
            Flag::CaseInsensitive => self.case_insensitive = !self.case_insensitive,
            Flag::Global => self.global = !self.global,
            Flag::Multiline => self.multiline = !self.multiline,
        }
    }
}

/// One of the three flags, as the toolbar draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flag {
    CaseInsensitive,
    Global,
    Multiline,
}

impl Flag {
    const ALL: [Self; 3] = [Self::CaseInsensitive, Self::Global, Self::Multiline];

    fn letter(self) -> &'static str {
        match self {
            Self::CaseInsensitive => "i",
            Self::Global => "g",
            Self::Multiline => "m",
        }
    }

    /// What the button does, with its key, for the status bar.
    fn tip(self) -> &'static str {
        match self {
            Self::CaseInsensitive => "Case insensitive (Ctrl+I)",
            Self::Global => "Global: every match, not only the first (Ctrl+G)",
            Self::Multiline => "Multiline: ^ and $ match at each line (Ctrl+M)",
        }
    }
}

/// The chips along the top of the Library tab, in the order they are drawn
/// and the order `Ctrl+L` steps through them.
///
/// One list, not two. The renderer had this array inline and the key that
/// steps it did not exist; adding the key with its own copy of the order is
/// how the two drift apart, which is the defect this crate has now produced
/// four times in four readings.
const LIBRARY_FILTERS: [Option<PatternCategory>; 8] = [
    None,
    Some(PatternCategory::Validation),
    Some(PatternCategory::Extraction),
    Some(PatternCategory::Format),
    Some(PatternCategory::Network),
    Some(PatternCategory::DateTime),
    Some(PatternCategory::Programming),
    Some(PatternCategory::Custom),
];

/// Every key this program answers, and what it does.
///
/// There is no `?` here and there cannot be: every printable character is
/// typed into whichever field has focus, which is what makes this a tester
/// rather than a viewer. So the list is raised by `F1`, as in
/// `apps/spreadsheet` and `apps/hexeditor` for the same reason.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`.
const SHORTCUTS: &[(&str, &str)] = &[
    (
        "Ctrl+1 / Ctrl+2 / Ctrl+3",
        "The tester / the library / the reference",
    ),
    (
        "Tab / Shift+Tab",
        "Move between the pattern, the text and the replacement",
    ),
    ("F3 / Shift+F3", "Next / previous match"),
    (
        "Enter",
        "Next match, from the pattern; a new line, in the text",
    ),
    (
        "Arrows / Home / End",
        "Move the caret; Up and Down step through the matches from the pattern",
    ),
    (
        "Backspace / Delete",
        "Delete a character from the focused field",
    ),
    (
        "Ctrl+A / Ctrl+C / Ctrl+X / Ctrl+V",
        "Select all / copy / cut / paste",
    ),
    ("Ctrl+I", "Case insensitive"),
    ("Ctrl+G", "Global"),
    ("Ctrl+M", "Multiline"),
    ("Ctrl+R", "Show or hide the replacement box"),
    (
        "Ctrl+Shift+G",
        "Show or hide the capture groups in the match list",
    ),
    ("Ctrl+S", "Save the pattern to the library"),
    ("Ctrl+L", "Next category, in the library"),
    ("Delete", "Delete a saved pattern, in the library"),
    ("F1", "This list"),
];

/// Where the user's own patterns are kept (`settingsfile`), under `library`,
/// one map per name holding `pattern` and `flags`.
const CONFIG_NAME: &str = "regextester";
const LIBRARY_KEY: &str = "library";

/// How many characters a library name holds.
const MAX_NAME_LEN: usize = 60;

/// Height of the status bar at the foot of the window.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of a library row.
const LIBRARY_ROW_HEIGHT: f32 = 60.0;
/// Left gutter of the test input, where the line numbers go.
const GUTTER: f32 = 40.0;

// ============================================================================
// The test input: a text field of any number of lines
// ============================================================================

/// The test input: a text field of any number of lines.
///
/// The toolkit's `TextInput` is one line, and the test input has to hold a
/// log excerpt, a file, a list -- the thing a multi-line pattern is tried
/// against. It could not take a newline at all: Enter's text is a control
/// character and the typing path dropped every control character, so the
/// multiline flag had nothing to act on, and the text could only be edited
/// at its end.
#[derive(Debug, Clone, Default)]
struct TextArea {
    text: String,
    /// The caret, as a byte offset on a character boundary.
    caret: usize,
    /// Where a selection started, when there is one.
    anchor: Option<usize>,
    /// Where Up and Down aim, in pixels from the line's start: kept across a
    /// run of them, so passing a short line does not pull the caret left.
    goal_x: Option<f32>,
}

impl TextArea {
    fn text(&self) -> &str {
        &self.text
    }

    /// Replace the text, with the caret at its end. **Tests only.**
    #[cfg(test)]
    fn set_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
        self.caret = self.text.len();
        self.anchor = None;
        self.goal_x = None;
    }

    /// The selected bytes, in order, when any are selected.
    fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }

    fn selected_text(&self) -> &str {
        self.selection()
            .and_then(|(a, b)| self.text.get(a..b))
            .unwrap_or("")
    }

    /// Where the line holding byte `at` starts.
    fn line_start(&self, at: usize) -> usize {
        self.text
            .get(..at)
            .and_then(|head| head.rfind('\n'))
            .map_or(0, |nl| nl.saturating_add(1))
    }

    /// Where the line holding byte `at` ends: its newline, or the end.
    fn line_end(&self, at: usize) -> usize {
        self.text
            .get(at..)
            .and_then(|tail| tail.find('\n'))
            .map_or(self.text.len(), |nl| at.saturating_add(nl))
    }

    /// Which line byte `at` is on, counting from zero.
    fn line_index(&self, at: usize) -> usize {
        self.text
            .get(..at)
            .map_or(0, |head| head.bytes().filter(|b| *b == b'\n').count())
    }

    /// Where line `index` starts, or the start of the last line past the end.
    fn start_of_line(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }
        self.text
            .match_indices('\n')
            .nth(index.saturating_sub(1))
            .map_or_else(
                || self.line_start(self.text.len()),
                |(nl, _)| nl.saturating_add(1),
            )
    }

    fn line_count(&self) -> usize {
        self.text.matches('\n').count().saturating_add(1)
    }

    /// Put the caret at `at`, extending the selection when `shift` is held.
    fn move_to(&mut self, at: usize, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = at.min(self.text.len());
    }

    fn left(&mut self, shift: bool) {
        self.goal_x = None;
        if !shift && let Some((from, _)) = self.selection() {
            self.move_to(from, false);
            return;
        }
        let at = self
            .text
            .get(..self.caret)
            .and_then(|head| head.chars().next_back())
            .map_or(0, |c| self.caret.saturating_sub(c.len_utf8()));
        self.move_to(at, shift);
    }

    fn right(&mut self, shift: bool) {
        self.goal_x = None;
        if !shift && let Some((_, to)) = self.selection() {
            self.move_to(to, false);
            return;
        }
        let at = self
            .text
            .get(self.caret..)
            .and_then(|tail| tail.chars().next())
            .map_or(self.caret, |c| self.caret.saturating_add(c.len_utf8()));
        self.move_to(at, shift);
    }

    fn home(&mut self, shift: bool) {
        self.goal_x = None;
        self.move_to(self.line_start(self.caret), shift);
    }

    fn end(&mut self, shift: bool) {
        self.goal_x = None;
        self.move_to(self.line_end(self.caret), shift);
    }

    /// Up (`down == false`) or down by `lines` lines, aiming at the column
    /// the caret was at when the run of vertical moves began.
    fn vertical(&mut self, down: bool, lines: usize, shift: bool) {
        let start = self.line_start(self.caret);
        let here = self.line_index(self.caret);
        let goal = self.goal_x.unwrap_or_else(|| {
            let line = self.text.get(start..self.caret).unwrap_or("");
            text::measure(line, NORMAL_TEXT, FontWeightHint::Regular)
        });
        let last = self.line_count().saturating_sub(1);
        let target = if down {
            here.saturating_add(lines).min(last)
        } else {
            here.saturating_sub(lines)
        };
        if target == here {
            // Past the first or the last line: to its start or its end, as
            // every text box does.
            let at = if down {
                self.line_end(self.caret)
            } else {
                start
            };
            self.move_to(at, shift);
            self.goal_x = None;
            return;
        }
        let from = self.start_of_line(target);
        let line = self.text.get(from..self.line_end(from)).unwrap_or("");
        let within = text::cursor_at(line, goal, NORMAL_TEXT, FontWeightHint::Regular).byte;
        self.move_to(from.saturating_add(within), shift);
        self.goal_x = Some(goal);
    }

    fn select_all(&mut self) {
        self.goal_x = None;
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Take the selection out. Returns whether there was one.
    fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            return false;
        };
        self.text.replace_range(from..to, "");
        self.caret = from;
        self.anchor = None;
        true
    }

    /// Put `typed` where the caret is, over any selection, taking no more
    /// than leaves the field at `capacity` characters. Returns whether the
    /// text changed.
    fn insert(&mut self, typed: &str, capacity: usize) -> bool {
        self.goal_x = None;
        let removed = self.delete_selection();
        let room = capacity.saturating_sub(self.text.chars().count());
        // Line breaks and tabs are text here; other control characters --
        // a paste's carriage returns among them -- are not.
        let taken: String = typed
            .chars()
            .filter(|c| matches!(c, '\n' | '\t') || !c.is_control())
            .take(room)
            .collect();
        if taken.is_empty() {
            return removed;
        }
        self.text.insert_str(self.caret, &taken);
        self.caret = self.caret.saturating_add(taken.len());
        true
    }

    fn backspace(&mut self) -> bool {
        self.goal_x = None;
        if self.delete_selection() {
            return true;
        }
        let Some(c) = self
            .text
            .get(..self.caret)
            .and_then(|h| h.chars().next_back())
        else {
            return false;
        };
        let from = self.caret.saturating_sub(c.len_utf8());
        self.text.replace_range(from..self.caret, "");
        self.caret = from;
        true
    }

    fn delete(&mut self) -> bool {
        self.goal_x = None;
        if self.delete_selection() {
            return true;
        }
        let Some(c) = self.text.get(self.caret..).and_then(|t| t.chars().next()) else {
            return false;
        };
        let to = self.caret.saturating_add(c.len_utf8());
        self.text.replace_range(self.caret..to, "");
        true
    }

    /// Put the caret on line `line`, at `x` pixels from where the line's
    /// text starts.
    fn click(&mut self, line: usize, x: f32, shift: bool) {
        self.goal_x = None;
        let line = line.min(self.line_count().saturating_sub(1));
        let from = self.start_of_line(line);
        let text = self.text.get(from..self.line_end(from)).unwrap_or("");
        let within = text::cursor_at(text, x, NORMAL_TEXT, FontWeightHint::Regular).byte;
        self.move_to(from.saturating_add(within), shift);
    }
}

// ============================================================================
// Pointer targets
// ============================================================================

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The tester drew three tabs, three flag buttons, three fields, a match list,
/// three sub-tabs, eight library chips and a library of rows, and handled no
/// pointer event (`known-issues.md` →
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). A library
/// row is named by its index in the library, not by where it is drawn: the
/// chips filter the list and the wheel scrolls it, and neither renumbers the
/// library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Tab(ActiveTab),
    Flag(Flag),
    MatchPrev,
    MatchNext,
    ReplaceToggle,
    SavePattern,
    PatternField,
    ReplaceField,
    /// The test input's text, where a press puts the caret.
    InputArea,
    /// The replacement's result, which scrolls.
    ResultArea,
    ResultTab(ResultsView),
    /// Whether the match list shows each match's groups.
    GroupsInline,
    MatchRow(usize),
    /// The results panel's body, which scrolls.
    ResultsBody,
    Chip(usize),
    LibraryRow(usize),
    LibraryUse(usize),
    LibraryDelete(usize),
    /// The library's list, which scrolls.
    LibraryBody,
    /// The reference, which scrolls.
    ReferenceBody,
    SaveName,
    SaveConfirm,
    SaveCancel,
    /// Everything behind the save dialog: a press there does nothing.
    ModalBackdrop,
    HelpCard,
}

impl Target {
    /// What pressing this does, with its key, for the status bar.
    fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Flag(flag) => flag.tip(),
            Self::MatchPrev => "Previous match (Shift+F3)",
            Self::MatchNext => "Next match (F3)",
            Self::ReplaceToggle => "Show or hide the replacement (Ctrl+R)",
            Self::SavePattern => "Save the pattern to the library (Ctrl+S)",
            Self::GroupsInline => "Show or hide each match's groups (Ctrl+Shift+G)",
            Self::LibraryUse(_) => "Try this pattern in the tester (Enter)",
            Self::LibraryDelete(_) => "Delete this saved pattern (Delete)",
            Self::Chip(_) => "Show one category (Ctrl+L steps through them)",
            _ => return None,
        })
    }
}

/// Something that scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Input,
    Result,
    Results(ResultsView),
    Library,
    Reference,
}

impl Pane {
    /// The pane a wheel over `target` scrolls.
    fn of(target: Target) -> Option<Self> {
        Some(match target {
            Target::InputArea => Self::Input,
            Target::ResultArea => Self::Result,
            Target::ResultsBody | Target::MatchRow(_) => Self::Results(ResultsView::Matches),
            Target::LibraryBody
            | Target::LibraryRow(_)
            | Target::LibraryUse(_)
            | Target::LibraryDelete(_) => Self::Library,
            Target::ReferenceBody => Self::Reference,
            _ => return None,
        })
    }
}

/// Where the tester's parts go, at the window's size: one statement of the
/// layout, read by the drawing, the pointer and the scrolling alike.
#[derive(Debug, Clone, Copy)]
struct TesterLayout {
    pattern: Rect,
    status_y: f32,
    replace: Option<Rect>,
    result: Option<Rect>,
    /// The test input, header included.
    input: Rect,
    results: Rect,
}

impl TesterLayout {
    /// The test input's text area: the box under its header.
    fn input_body(&self) -> Rect {
        Rect::new(
            self.input.x,
            self.input.y + 24.0,
            self.input.w,
            (self.input.h - 24.0).max(0.0),
        )
    }

    /// Where the input's lines are drawn: inside the body, right of the
    /// gutter.
    fn input_text(&self) -> Rect {
        let body = self.input_body();
        Rect::new(
            body.x + GUTTER,
            body.y + 6.0,
            (body.w - GUTTER - 8.0).max(0.0),
            (body.h - 10.0).max(0.0),
        )
    }

    /// The results panel under its strip of sub-tabs.
    fn results_body(&self) -> Rect {
        Rect::new(
            self.results.x,
            self.results.y + 30.0,
            self.results.w,
            (self.results.h - 34.0).max(0.0),
        )
    }
}

struct App {
    /// The window size as the compositor granted it.
    ///
    /// A field rather than the `WINDOW_WIDTH`/`WINDOW_HEIGHT` constants the
    /// layout used to read directly: a compositor may grant a size that was
    /// never requested, and the first frame is drawn before any `Resize`
    /// arrives — so an app that lays out against a constant draws its first
    /// frame at the wrong width and never notices a resize at all.
    window_width: f32,
    /// See [`Self::window_width`].
    window_height: f32,

    pattern: TextInput,
    input: TextArea,
    replace: TextInput,
    flags: RegexFlags,
    /// Whether the shortcut list is up.
    show_help: bool,
    active_tab: ActiveTab,
    active_field: ActiveField,

    // Regex results
    compiled: Option<CompiledRegex>,
    compile_error: Option<String>,
    matches: Vec<RegexMatch>,
    replace_result: Option<String>,
    explanations: Vec<String>,

    // Library
    library: Vec<PatternEntry>,
    selected_library_entry: Option<usize>,
    library_category_filter: Option<PatternCategory>,
    /// The first row the library list shows, as a position in the filtered
    /// list.
    library_scroll: usize,

    /// The first line the test input shows.
    input_scroll: usize,
    /// The first line the replacement's result shows.
    result_scroll: usize,
    /// The first row each results view shows, by [`ResultsView::index`].
    results_scroll: [usize; 3],
    /// The first row the reference shows.
    reference_scroll: usize,
    current_match_index: usize,
    results_view: ResultsView,
    show_replace: bool,
    show_groups: bool,

    /// The name being typed for a pattern about to be saved, while the dialog
    /// asking for it is up.
    save_name: Option<TextInput>,
    /// Why the last save could not be made, shown in the dialog.
    save_error: Option<String>,
    /// What the last save, delete or copy said.
    status: String,
    /// What was last copied or cut, from any field: one clipboard for the
    /// window, so a pattern can be pasted into the text and back.
    clipboard: String,
    /// Whether a press in the test input is being dragged into a selection.
    dragging: bool,
    /// The wheel's remainder, so a trackpad's small turns add up.
    wheel: wheel::Accumulator,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl App {
    fn new() -> Self {
        Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            pattern: TextInput::new(),
            input: TextArea::default(),
            replace: TextInput::new(),
            flags: RegexFlags::default(),
            active_tab: ActiveTab::Tester,
            active_field: ActiveField::Pattern,
            compiled: None,
            compile_error: None,
            matches: Vec::new(),
            replace_result: None,
            explanations: Vec::new(),
            library: built_in_patterns(),
            selected_library_entry: None,
            library_category_filter: None,
            library_scroll: 0,
            input_scroll: 0,
            result_scroll: 0,
            results_scroll: [0; 3],
            reference_scroll: 0,
            current_match_index: 0,
            results_view: ResultsView::Matches,
            show_replace: false,
            show_groups: true,
            save_name: None,
            save_error: None,
            status: String::new(),
            clipboard: String::new(),
            dragging: false,
            wheel: wheel::Accumulator::default(),
            hover: None,
            last_hits: Vec::new(),
        }
    }

    // -- the three fields ---------------------------------------------------------

    /// Put `text` in the pattern and run it. **Tests only.**
    #[cfg(test)]
    fn set_pattern(&mut self, text: &str) {
        self.pattern.set_text(text);
        self.update_regex();
    }

    /// Put `text` in the test input and run the pattern over it. **Tests only.**
    #[cfg(test)]
    fn set_input(&mut self, text: &str) {
        self.input.set_text(text);
        self.update_regex();
    }

    /// Put `text` in the replacement and apply it. **Tests only.**
    #[cfg(test)]
    fn set_replacement(&mut self, text: &str) {
        self.replace.set_text(text);
        self.update_regex();
    }

    fn update_regex(&mut self) {
        if self.pattern.text().is_empty() {
            self.compiled = None;
            self.compile_error = None;
            self.matches.clear();
            self.replace_result = None;
            self.explanations.clear();
            self.current_match_index = 0;
            return;
        }

        let compiler = RegexCompiler::new(self.pattern.text(), self.flags.case_insensitive)
            .multiline(self.flags.multiline);
        match compiler.compile() {
            Ok(regex) => {
                self.compiled = Some(regex);
                self.compile_error = None;
            }
            Err(e) => {
                self.compiled = None;
                self.compile_error = Some(format!("{e}"));
                self.matches.clear();
                self.replace_result = None;
                self.explanations = explain_regex(self.pattern.text());
                self.current_match_index = 0;
                return;
            }
        }

        if let Some(compiled) = &self.compiled {
            self.matches = find_all_matches(compiled, self.input.text());
            if !self.flags.global && self.matches.len() > 1 {
                self.matches.truncate(1);
            }
        }

        if self.show_replace && self.compiled.is_some() && !self.replace.text().is_empty() {
            self.replace_result = Some(apply_replacement(
                self.input.text(),
                &self.matches,
                self.replace.text(),
            ));
        } else {
            self.replace_result = None;
        }

        self.explanations = explain_regex(self.pattern.text());

        if self.matches.is_empty() {
            self.current_match_index = 0;
        } else if self.current_match_index >= self.matches.len() {
            self.current_match_index = self.matches.len().saturating_sub(1);
        }
    }

    fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match_index = self
                .current_match_index
                .saturating_add(1)
                .checked_rem(self.matches.len())
                .unwrap_or(0);
            self.reveal_current_match();
        }
    }

    fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            if self.current_match_index == 0 {
                self.current_match_index = self.matches.len().saturating_sub(1);
            } else {
                self.current_match_index = self.current_match_index.saturating_sub(1);
            }
            self.reveal_current_match();
        }
    }

    /// Scroll the match list and the test input to the current match.
    ///
    /// Stepping through matches changed a counter and a row's colour, and
    /// neither list moved: the thousandth match could be current and on
    /// neither screen.
    fn reveal_current_match(&mut self) {
        let rows = self.results_rows(ResultsView::Matches);
        let current = self.current_match_index;
        let slot = self.pane_scroll(Pane::Results(ResultsView::Matches));
        *slot = keep_in_view(*slot, current, rows);
        if let Some(m) = self.matches.get(self.current_match_index) {
            let byte = char_to_byte(self.input.text(), m.start);
            let line = self.input.line_index(byte);
            self.input_scroll = keep_in_view(self.input_scroll, line, self.input_rows());
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
        let group_count = self.matches.first().map_or(0, |m| {
            m.groups.iter().skip(1).filter(|g| g.is_some()).count()
        });

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

    // -- the library ---------------------------------------------------------------

    /// Read the user's saved patterns from their settings.
    ///
    /// A saved entry with no pattern is not shown; it stays in the file, which
    /// is the user's, and a later save or delete does not touch it -- each
    /// writes only the entry it is about.
    fn load_library(&mut self, doc: &yamldoc::Document) {
        for name in doc.keys(&[LIBRARY_KEY]) {
            if self.library.len() >= MAX_LIBRARY_ENTRIES {
                break;
            }
            let Some(pattern) = doc.get_str(&[LIBRARY_KEY, &name, "pattern"]) else {
                continue;
            };
            let flags = doc
                .get_str(&[LIBRARY_KEY, &name, "flags"])
                .map_or_else(RegexFlags::default, |f| RegexFlags::from_letters(&f));
            self.library
                .retain(|e| !(e.category == PatternCategory::Custom && e.name == name));
            self.library.push(custom_entry(&name, &pattern, flags));
        }
    }

    /// The entries the chips leave, with their indices in the library.
    fn visible_library(&self) -> Vec<(usize, &PatternEntry)> {
        self.library
            .iter()
            .enumerate()
            .filter(|(_, e)| self.library_category_filter.is_none_or(|f| e.category == f))
            .collect()
    }

    /// Put library entry `index` in the tester: its pattern, and the flags it
    /// was saved with if it was saved. `load_library_entry` existed with no
    /// caller, so no pattern in the library could be tried.
    fn use_library_entry(&mut self, index: usize) -> bool {
        let Some(entry) = self.library.get(index) else {
            return false;
        };
        let (name, pattern, flags) = (entry.name.clone(), entry.pattern.clone(), entry.flags);
        self.selected_library_entry = Some(index);
        if let Some(flags) = flags {
            self.flags = flags;
        }
        self.pattern.set_text(&pattern);
        self.active_tab = ActiveTab::Tester;
        self.active_field = ActiveField::Pattern;
        self.update_regex();
        self.status = format!("Trying {name}");
        true
    }

    /// Put the dialog up that asks what to call the pattern.
    fn ask_to_save(&mut self) -> bool {
        if self.pattern.text().is_empty() {
            self.status = "Nothing to save: the pattern is empty".to_string();
            return true;
        }
        let mut name = TextInput::new();
        // Offered the name it was loaded under, when it was a saved one.
        if let Some(entry) = self
            .selected_library_entry
            .and_then(|i| self.library.get(i))
            .filter(|e| e.category == PatternCategory::Custom && e.pattern == self.pattern.text())
        {
            name.set_text(&entry.name);
            name.select_all();
        }
        self.save_name = Some(name);
        self.save_error = None;
        true
    }

    /// Save the pattern under the name typed in the dialog, replacing a saved
    /// pattern of that name. The library was a list in memory with
    /// `save_to_library` never called and no way to type a name; a saved
    /// pattern now outlives the window.
    fn confirm_save(&mut self) -> bool {
        let Some(name) = self.save_name.as_ref().map(|n| n.text().trim().to_string()) else {
            return false;
        };
        if name.is_empty() {
            self.save_error = Some("Give the pattern a name".to_string());
            return true;
        }
        let existing = self
            .library
            .iter()
            .position(|e| e.category == PatternCategory::Custom && e.name == name);
        if existing.is_none() && self.library.len() >= MAX_LIBRARY_ENTRIES {
            self.save_error = Some(format!(
                "The library is full ({MAX_LIBRARY_ENTRIES} patterns); delete one first"
            ));
            return true;
        }
        let entry = custom_entry(&name, self.pattern.text(), self.flags);
        let index = if let Some(i) = existing {
            if let Some(slot) = self.library.get_mut(i) {
                *slot = entry;
            }
            i
        } else {
            self.library.push(entry);
            self.library.len().saturating_sub(1)
        };
        self.selected_library_entry = Some(index);
        self.save_name = None;
        self.save_error = None;
        let mut doc = settingsfile::load(CONFIG_NAME);
        doc.set_str(&[LIBRARY_KEY, &name, "pattern"], self.pattern.text());
        doc.set_str(&[LIBRARY_KEY, &name, "flags"], &self.flags.letters());
        self.status = match settingsfile::store(CONFIG_NAME, &doc) {
            Ok(()) => format!("Saved {name} to the library"),
            Err(e) => format!("{name} is in the library until the window closes: {e}"),
        };
        true
    }

    /// Delete saved pattern `index`. The built-in ones cannot be deleted.
    fn delete_library_entry(&mut self, index: usize) -> bool {
        let Some(entry) = self
            .library
            .get(index)
            .filter(|e| e.category == PatternCategory::Custom)
        else {
            return false;
        };
        let name = entry.name.clone();
        self.library.remove(index);
        self.selected_library_entry = match self.selected_library_entry {
            Some(s) if s == index => None,
            Some(s) if s > index => Some(s.saturating_sub(1)),
            other => other,
        };
        let mut doc = settingsfile::load(CONFIG_NAME);
        doc.remove(&[LIBRARY_KEY, &name]);
        self.status = match settingsfile::store(CONFIG_NAME, &doc) {
            Ok(()) => format!("Deleted {name}"),
            Err(e) => format!("Deleted {name} from the list, but not from your settings: {e}"),
        };
        let shown = self.visible_library().len();
        self.library_scroll = self.library_scroll.min(shown.saturating_sub(1));
        true
    }

    /// Move the library's selection by `delta` rows within what the chips
    /// show.
    fn step_library(&mut self, delta: isize) -> bool {
        let shown: Vec<usize> = self.visible_library().iter().map(|(i, _)| *i).collect();
        if shown.is_empty() {
            return false;
        }
        let at = self
            .selected_library_entry
            .and_then(|s| shown.iter().position(|i| *i == s));
        let next = match at {
            None => 0,
            Some(p) => p
                .saturating_add_signed(delta)
                .min(shown.len().saturating_sub(1)),
        };
        let Some(&index) = shown.get(next) else {
            return false;
        };
        if Some(index) == self.selected_library_entry {
            return false;
        }
        self.selected_library_entry = Some(index);
        self.library_scroll = keep_in_view(self.library_scroll, next, self.library_rows());
        true
    }

    /// Show one category of the library.
    fn set_library_filter(&mut self, filter: Option<PatternCategory>) -> bool {
        if self.library_category_filter == filter {
            return false;
        }
        self.library_category_filter = filter;
        self.library_scroll = 0;
        true
    }

    // -- layout ------------------------------------------------------------------------

    /// Where everything under the toolbar and over the status bar goes.
    fn content_rect(&self) -> Rect {
        let top = TOOLBAR_HEIGHT + PADDING;
        Rect::new(
            0.0,
            top,
            self.window_width,
            (self.window_height - top - STATUS_BAR_HEIGHT - PADDING).max(0.0),
        )
    }

    fn tester_layout(&self) -> TesterLayout {
        let content = self.content_rect();
        let width = (self.window_width - 2.0 * PADDING).max(0.0);
        let pattern = Rect::new(PADDING + 80.0, content.y, (width - 80.0).max(0.0), 36.0);
        let status_y = pattern.bottom() + 6.0;
        let mut next_y = status_y + 20.0;
        let (replace, result) = if self.show_replace {
            let replace = Rect::new(PADDING + 80.0, next_y, (width - 80.0).max(0.0), 36.0);
            next_y = replace.bottom() + 6.0;
            let result = Rect::new(PADDING, next_y, width, 72.0);
            next_y = result.bottom() + 6.0;
            (Some(replace), Some(result))
        } else {
            (None, None)
        };
        let split_y = next_y + 4.0;
        let split_h = (content.bottom() - split_y).max(0.0);
        let left_w = ((self.window_width - 3.0 * PADDING) * 0.55).max(0.0);
        let right_x = PADDING + left_w + PADDING;
        TesterLayout {
            pattern,
            status_y,
            replace,
            result,
            input: Rect::new(PADDING, split_y, left_w, split_h),
            results: Rect::new(
                right_x,
                split_y,
                (self.window_width - right_x - PADDING).max(0.0),
                split_h,
            ),
        }
    }

    /// How many lines of the test input fit.
    fn input_rows(&self) -> usize {
        rows_in(self.tester_layout().input_text().h, LINE_HEIGHT)
    }

    /// How many rows of a results view fit.
    fn results_rows(&self, view: ResultsView) -> usize {
        rows_in(self.tester_layout().results_body().h, view.row_height())
    }

    /// The library's list, under its chips.
    fn library_list_rect(&self) -> Rect {
        let content = self.content_rect();
        Rect::new(
            PADDING,
            content.y + 34.0,
            (self.window_width - 2.0 * PADDING).max(0.0),
            (content.h - 34.0).max(0.0),
        )
    }

    fn library_rows(&self) -> usize {
        rows_in(self.library_list_rect().h, LIBRARY_ROW_HEIGHT)
    }

    /// How many rows the results view `view` has.
    fn results_len(&self, view: ResultsView) -> usize {
        match view {
            ResultsView::Matches => self.matches.len(),
            ResultsView::Groups => self
                .matches
                .get(self.current_match_index)
                .map_or(0, |m| m.groups.len()),
            ResultsView::Explain => self.explanations.len(),
        }
    }

    /// How far the test input is scrolled sideways, so the caret -- or, while
    /// the keyboard is elsewhere, the current match -- is in view. Worked out
    /// fresh from the point it follows, as a single-line field does
    /// (`textedit::horizontal_scroll`), so it cannot go stale.
    fn input_hscroll(&self, width: f32) -> f32 {
        let text = self.input.text();
        let at = if self.active_field == ActiveField::Input {
            Some(self.input.caret)
        } else {
            self.matches
                .get(self.current_match_index)
                .map(|m| char_to_byte(text, m.start))
        };
        let Some(at) = at else {
            return 0.0;
        };
        let start = self.input.line_start(at);
        let line = text.get(start..self.input.line_end(at)).unwrap_or("");
        let caret_px = text::measure(
            text.get(start..at).unwrap_or(""),
            NORMAL_TEXT,
            FontWeightHint::Regular,
        );
        let line_w = text::measure(line, NORMAL_TEXT, FontWeightHint::Regular);
        textedit::horizontal_scroll(line_w, width, caret_px)
    }

    /// Scroll the test input so its caret is on screen.
    fn keep_caret_visible(&mut self) {
        let line = self.input.line_index(self.input.caret);
        self.input_scroll = keep_in_view(self.input_scroll, line, self.input_rows());
    }

    // -- drawing -------------------------------------------------------------------

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    fn frame(&self) -> Frame<Target> {
        let mut f = Frame::new(self.window_width, self.window_height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.draw_toolbar(&mut f);
        match self.active_tab {
            ActiveTab::Tester => self.draw_tester(&mut f),
            ActiveTab::Library => self.draw_library(&mut f),
            ActiveTab::Reference => self.draw_reference(&mut f),
        }
        self.draw_status_bar(&mut f);
        if self.save_name.is_some() {
            self.draw_save_dialog(&mut f);
        }
        // Over everything, because it is the one thing a reader asked for.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (self.window_width, self.window_height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(
                Target::HelpCard,
                Rect::new(0.0, 0.0, self.window_width, self.window_height),
            );
        }
        f
    }

    /// The drawn commands. **Tests only**: the window's `render` takes the
    /// frame itself, because it keeps the frame's boxes for the pointer.
    ///
    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`.
    #[cfg(test)]
    fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// A compact button, lit while it is on or the pointer is on it.
    fn button(&self, f: &mut Frame<Target>, rect: Rect, label: &str, lit: bool, target: Target) {
        let hot = self.hover == Some(target);
        self.palette.push_surface(
            f,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            4.0,
            if lit || hot {
                Surface::Selected
            } else {
                Surface::Card
            },
        );
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - SMALL_TEXT) / 2.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: if lit {
                self.palette.text
            } else {
                self.palette.subtext0
            },
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, rect);
    }

    /// Width of a [`Self::button`] labelled `label`.
    fn button_width(label: &str) -> f32 {
        text::measure(label, SMALL_TEXT, FontWeightHint::Bold) + 16.0
    }

    fn draw_toolbar(&self, f: &mut Frame<Target>) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            self.window_width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        f.push(RenderCommand::Text {
            x: PADDING,
            y: 13.0,
            text: "Regex Tester".into(),
            font_size: TITLE_TEXT,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });

        let mut tab_x = 170.0;
        for tab in ActiveTab::ALL {
            let active = tab == self.active_tab;
            // Measured bold whatever the state, so the strip does not reflow
            // when the user switches tabs.
            let w = text::measure(tab.label(), NORMAL_TEXT, FontWeightHint::Bold) + 20.0;
            let rect = Rect::new(tab_x, 8.0, w, 28.0);
            if active || self.hover == Some(Target::Tab(tab)) {
                self.palette.push_surface(
                    f,
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    4.0,
                    Surface::Selected,
                );
            }
            f.push(RenderCommand::Text {
                x: tab_x + 10.0,
                y: 15.0,
                text: tab.label().into(),
                font_size: NORMAL_TEXT,
                color: if active {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Tab(tab), rect);
            tab_x += w + 6.0;
        }

        // The right-hand group, from the right edge in.
        let mut right = self.window_width - PADDING;
        let next = Rect::new(right - 24.0, 8.0, 24.0, 28.0);
        right = next.x - 2.0;
        let count = if self.matches.is_empty() {
            "0/0".to_string()
        } else {
            format!(
                "{}/{}",
                self.current_match_index.saturating_add(1),
                self.matches.len()
            )
        };
        let count_w = text::measure(&count, SMALL_TEXT, FontWeightHint::Regular) + 12.0;
        right -= count_w;
        f.push(RenderCommand::Text {
            x: right + 6.0,
            y: 16.0,
            text: count,
            font_size: SMALL_TEXT,
            color: self.palette.subtext1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(count_w),
            overflow: TextOverflow::Ellipsis,
        });
        let prev = Rect::new(right - 26.0, 8.0, 24.0, 28.0);
        self.button(f, prev, "<", false, Target::MatchPrev);
        self.button(f, next, ">", false, Target::MatchNext);
        right = prev.x - 12.0;

        for flag in Flag::ALL.iter().rev() {
            let rect = Rect::new(right - 30.0, 8.0, 30.0, 28.0);
            let on = self.flags.get(*flag);
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: if on {
                    self.palette.blue
                } else if self.hover == Some(Target::Flag(*flag)) {
                    self.palette.surface1
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(4.0),
            });
            f.push(RenderCommand::Text {
                x: rect.x + 11.0,
                y: 15.0,
                text: flag.letter().into(),
                font_size: NORMAL_TEXT,
                color: if on {
                    self.palette.crust
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(rect.w),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Flag(*flag), rect);
            right = rect.x - 6.0;
        }
        right -= 6.0;

        for (label, lit, target) in [
            ("Save…", false, Target::SavePattern),
            ("Replace", self.show_replace, Target::ReplaceToggle),
        ] {
            let w = Self::button_width(label);
            let rect = Rect::new(right - w, 8.0, w, 28.0);
            // Past the tabs there is no room; a button drawn over a tab would
            // take its presses.
            if rect.x < tab_x {
                break;
            }
            self.button(f, rect, label, lit, target);
            right = rect.x - 6.0;
        }
    }

    fn draw_tester(&self, f: &mut Frame<Target>) {
        let l = self.tester_layout();
        self.draw_field(
            f,
            "Pattern:",
            l.pattern,
            &self.pattern,
            ActiveField::Pattern,
            Target::PatternField,
        );

        if let Some(err) = &self.compile_error {
            f.push(RenderCommand::Text {
                x: l.pattern.x,
                y: l.status_y,
                text: format!("Error: {err}"),
                font_size: SMALL_TEXT,
                color: self.palette.ink(self.palette.red),
                font_weight: FontWeightHint::Regular,
                max_width: Some(l.pattern.w),
                overflow: TextOverflow::Ellipsis,
            });
        } else if !self.pattern.text().is_empty() {
            f.push(RenderCommand::Text {
                x: l.pattern.x,
                y: l.status_y,
                text: self.match_stats(),
                font_size: SMALL_TEXT,
                color: self.palette.ink(self.palette.green),
                font_weight: FontWeightHint::Regular,
                max_width: Some(l.pattern.w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if let (Some(replace), Some(result)) = (l.replace, l.result) {
            self.draw_field(
                f,
                "Replace:",
                replace,
                &self.replace,
                ActiveField::Replace,
                Target::ReplaceField,
            );
            // The replacement itself. `apply_replacement` has produced this on
            // every keystroke since the program was written, into a field
            // nothing drew -- so a user typed a replacement into a regex
            // tester, was shown which parts matched, and never once saw the
            // text that would come out. The matches are the working; this is
            // the answer.
            //
            // Drawn even when there is nothing to draw, because a box that
            // appears and disappears as the pattern compiles and fails is
            // harder to read than an empty one that stays put.
            self.draw_result(f, result);
        }

        self.draw_input(f, &l);
        self.draw_results(f, &l);
    }

    /// A labelled one-line field, drawn with the toolkit's single-line editor
    /// so its caret, selection and sideways scroll are every other field's.
    fn draw_field(
        &self,
        f: &mut Frame<Target>,
        label: &str,
        rect: Rect,
        input: &TextInput,
        field: ActiveField,
        target: Target,
    ) {
        let focused = self.active_tab == ActiveTab::Tester
            && self.active_field == field
            && self.save_name.is_none();
        f.push(RenderCommand::Text {
            x: PADDING,
            y: rect.y + 10.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });
        let mut tree = RenderTree::new();
        textedit::draw(
            &mut tree,
            &textedit::SingleLine {
                text: input.text(),
                cursor: if focused {
                    input.cursor()
                } else {
                    TextCursor::default()
                },
                selection_anchor: if focused {
                    input.selection_anchor()
                } else {
                    None
                },
                focused,
                x: rect.x + 8.0,
                y: rect.y + 9.0,
                width: (rect.w - 16.0).max(0.0),
                line_height: 18.0,
                font_size: NORMAL_TEXT,
                weight: FontWeightHint::Regular,
                color: self.palette.text,
                selection_bg: self.palette.blue,
                selection_fg: self.palette.crust,
                caret_width: textedit::CARET_WIDTH,
            },
        );
        f.extend(tree.commands);
        f.hit(target, rect);
    }

    /// The header every text box here has: a strip with its label, and a
    /// count on the right.
    fn draw_box_header(&self, f: &mut Frame<Target>, rect: Rect, label: &str, count: &str) {
        self.palette.push_surface_radii(
            f,
            rect.x,
            rect.y,
            rect.w,
            24.0,
            CornerRadii {
                top_left: 4.0,
                top_right: 4.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
            Surface::Card,
        );
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + 5.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: self.palette.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - 100.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: rect.right() - 90.0,
            y: rect.y + 5.0,
            text: count.into(),
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(84.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The test input: the text, its line numbers, every match highlighted
    /// (the current one more strongly), the selection and the caret.
    fn draw_input(&self, f: &mut Frame<Target>, l: &TesterLayout) {
        let text = self.input.text();
        let focused = self.active_field == ActiveField::Input && self.save_name.is_none();
        self.draw_box_header(
            f,
            l.input,
            "Test Input:",
            &format!("{} lines", self.input.line_count()),
        );
        let body = l.input_body();
        self.palette.push_surface_radii(
            f,
            body.x,
            body.y,
            body.w,
            body.h,
            CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: 4.0,
                bottom_right: 4.0,
            },
            Surface::Card,
        );
        f.push(RenderCommand::StrokeRect {
            x: l.input.x,
            y: l.input.y,
            width: l.input.w,
            height: l.input.h,
            color: if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });
        f.hit(Target::InputArea, body);

        let area = l.input_text();
        let hscroll = self.input_hscroll(area.w);
        let rows = self.input_rows();
        // Every character's byte offset, once: the matches are counted in
        // characters and the text is sliced in bytes. The highlight used a
        // match's character index as a byte offset, so on any line with a
        // character wider than a byte it painted the wrong letters, and
        // `get` of a split character drew nothing at all.
        let bytes = char_bytes(text);
        let selection = if focused {
            self.input.selection()
        } else {
            None
        };
        let mut line_start = 0usize;
        let mut first_char = 0usize;
        for (li, line) in text.split('\n').enumerate() {
            let line_end = line_start.saturating_add(line.len());
            let line_chars = line.chars().count();
            if li >= self.input_scroll && li < self.input_scroll.saturating_add(rows) {
                let ly = area.y + (li.saturating_sub(self.input_scroll)) as f32 * LINE_HEIGHT;
                f.push(RenderCommand::Text {
                    x: body.x + 4.0,
                    y: ly,
                    text: format!("{:>3}", li.saturating_add(1)),
                    font_size: SMALL_TEXT,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(GUTTER - 6.0),
                    overflow: TextOverflow::Clip,
                });
                f.clip(area);
                let origin = area.x - hscroll;
                let last_char = first_char.saturating_add(line_chars);
                for (mi, m) in self.matches.iter().enumerate() {
                    // This line's characters are `first_char..last_char`, and
                    // `last_char` is its line break. An empty match is a place
                    // and is on the line if that place is; a match with width
                    // is if it covers any of the line, its break included.
                    let on_line = if m.start == m.end {
                        (first_char..=last_char).contains(&m.start)
                    } else {
                        m.start <= last_char && m.end > first_char
                    };
                    if !on_line {
                        continue;
                    }
                    let from = bytes
                        .get(m.start.max(first_char))
                        .copied()
                        .unwrap_or(text.len())
                        .saturating_sub(line_start)
                        .min(line.len());
                    let to = bytes
                        .get(m.end.min(last_char))
                        .copied()
                        .unwrap_or(text.len())
                        .saturating_sub(line_start)
                        .min(line.len());
                    // From the palette: the wash was Catppuccin's blue as a
                    // literal, the same on every theme.
                    let colour = if mi == self.current_match_index {
                        with_alpha(self.palette.peach, 110)
                    } else {
                        with_alpha(self.palette.blue, 60)
                    };
                    if from >= to {
                        // An empty match -- `^`, `\b`, `x*` between two
                        // letters -- is a place, drawn as a bar; a match of
                        // the line break alone is drawn as a block past the
                        // line's end, where the break is.
                        let x = text::measure(
                            line.get(..from).unwrap_or(""),
                            NORMAL_TEXT,
                            FontWeightHint::Regular,
                        );
                        f.push(RenderCommand::FillRect {
                            x: origin + x - 1.0,
                            y: ly - 2.0,
                            width: if m.start == m.end { 2.0 } else { 7.0 },
                            height: LINE_HEIGHT,
                            color: colour,
                            corner_radii: CornerRadii::ZERO,
                        });
                        continue;
                    }
                    for (left, width) in
                        text::selection_boxes(line, from, to, NORMAL_TEXT, FontWeightHint::Regular)
                    {
                        f.push(RenderCommand::FillRect {
                            x: origin + left,
                            y: ly - 2.0,
                            width,
                            height: LINE_HEIGHT,
                            color: colour,
                            corner_radii: CornerRadii::all(2.0),
                        });
                    }
                }
                if let Some((sel_from, sel_to)) = selection {
                    let from = sel_from
                        .clamp(line_start, line_end)
                        .saturating_sub(line_start);
                    let to = sel_to
                        .clamp(line_start, line_end)
                        .saturating_sub(line_start);
                    if from < to {
                        for (left, width) in text::selection_boxes(
                            line,
                            from,
                            to,
                            NORMAL_TEXT,
                            FontWeightHint::Regular,
                        ) {
                            f.push(RenderCommand::FillRect {
                                x: origin + left,
                                y: ly - 2.0,
                                width,
                                height: LINE_HEIGHT,
                                color: with_alpha(self.palette.blue, 90),
                                corner_radii: CornerRadii::ZERO,
                            });
                        }
                    }
                }
                f.push(RenderCommand::Text {
                    x: origin,
                    y: ly,
                    text: line.to_string(),
                    font_size: NORMAL_TEXT,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                if focused && (line_start..=line_end).contains(&self.input.caret) {
                    let caret = text::measure(
                        line.get(..self.input.caret.saturating_sub(line_start))
                            .unwrap_or(""),
                        NORMAL_TEXT,
                        FontWeightHint::Regular,
                    );
                    f.push(RenderCommand::FillRect {
                        x: origin + caret,
                        y: ly - 1.0,
                        width: textedit::CARET_WIDTH,
                        height: LINE_HEIGHT - 2.0,
                        color: self.palette.text,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
                f.unclip();
            }
            line_start = line_end.saturating_add(1);
            first_char = first_char.saturating_add(line_chars).saturating_add(1);
        }
    }

    /// The replacement's result: read-only text, which scrolls.
    fn draw_result(&self, f: &mut Frame<Target>, rect: Rect) {
        let result = self.replace_result.as_deref().unwrap_or("");
        let lines: Vec<&str> = result.split('\n').collect();
        self.draw_box_header(
            f,
            rect,
            "Result:",
            &if result.is_empty() {
                String::new()
            } else {
                format!("{} lines", lines.len())
            },
        );
        let body = Rect::new(rect.x, rect.y + 24.0, rect.w, (rect.h - 24.0).max(0.0));
        self.palette
            .push_surface(f, body.x, body.y, body.w, body.h, 4.0, Surface::Card);
        f.hit(Target::ResultArea, body);
        let rows = rows_in(body.h - 6.0, LINE_HEIGHT);
        f.clip(body);
        for (i, line) in lines.iter().enumerate().skip(self.result_scroll).take(rows) {
            let ly = body.y + 4.0 + (i.saturating_sub(self.result_scroll)) as f32 * LINE_HEIGHT;
            f.push(RenderCommand::Text {
                x: body.x + 8.0,
                y: ly,
                text: (*line).to_string(),
                font_size: NORMAL_TEXT,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some((body.w - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        f.unclip();
    }

    /// The results panel: its three views' tabs, and the one showing.
    fn draw_results(&self, f: &mut Frame<Target>, l: &TesterLayout) {
        let panel = l.results;
        self.palette
            .push_surface(f, panel.x, panel.y, panel.w, panel.h, 4.0, Surface::Card);
        let mut tx = panel.x + 4.0;
        for view in ResultsView::ALL {
            let w = text::measure(view.label(), SMALL_TEXT, FontWeightHint::Bold) + 16.0;
            let rect = Rect::new(tx, panel.y + 4.0, w, 22.0);
            let on = view == self.results_view;
            if on || self.hover == Some(Target::ResultTab(view)) {
                self.palette.push_surface(
                    f,
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    3.0,
                    Surface::Selected,
                );
            }
            f.push(RenderCommand::Text {
                x: rect.x + 8.0,
                y: rect.y + 4.0,
                text: view.label().into(),
                font_size: SMALL_TEXT,
                color: if on {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::ResultTab(view), rect);
            tx += w + 4.0;
        }
        if self.results_view == ResultsView::Matches {
            let label = if self.show_groups {
                "Groups: shown"
            } else {
                "Groups: hidden"
            };
            let w = Self::button_width(label);
            let rect = Rect::new(panel.right() - w - 6.0, panel.y + 4.0, w, 22.0);
            if rect.x > tx {
                self.button(f, rect, label, self.show_groups, Target::GroupsInline);
            }
        }

        let body = l.results_body();
        f.hit(Target::ResultsBody, body);
        f.clip(body);
        match self.results_view {
            ResultsView::Matches => self.draw_matches(f, body),
            ResultsView::Groups => self.draw_groups(f, body),
            ResultsView::Explain => self.draw_explanation(f, body),
        }
        f.unclip();
    }

    /// A line of text in the results panel's body.
    fn body_text(f: &mut Frame<Target>, body: Rect, y: f32, line: String, colour: Color) {
        f.push(RenderCommand::Text {
            x: body.x + 12.0,
            y,
            text: line,
            font_size: SMALL_TEXT,
            color: colour,
            font_weight: FontWeightHint::Regular,
            max_width: Some((body.w - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// What the results panel says when there is nothing to list.
    fn empty_results(&self) -> Option<(&'static str, Color)> {
        if self.pattern.text().is_empty() {
            Some(("Enter a pattern to begin", self.palette.subtext0))
        } else if self.compile_error.is_some() {
            Some((
                "The pattern does not compile -- see above",
                self.palette.ink(self.palette.red),
            ))
        } else if self.matches.is_empty() {
            Some(("No matches found", self.palette.ink(self.palette.yellow)))
        } else {
            None
        }
    }

    fn draw_matches(&self, f: &mut Frame<Target>, body: Rect) {
        if let Some((message, colour)) = self.empty_results() {
            Self::body_text(f, body, body.y + 20.0, message.to_string(), colour);
            return;
        }
        let input_chars: Vec<char> = self.input.text().chars().collect();
        let scroll = self.scroll_of(Pane::Results(ResultsView::Matches));
        let row_h = ResultsView::Matches.row_height();
        for (mi, m) in self
            .matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows_in(body.h, row_h).saturating_add(1))
        {
            let row_y = body.y + 4.0 + (mi.saturating_sub(scroll)) as f32 * row_h;
            let row = Rect::new(body.x + 4.0, row_y, (body.w - 8.0).max(0.0), row_h - 4.0);
            let current = mi == self.current_match_index;
            if current || self.hover == Some(Target::MatchRow(mi)) {
                self.palette
                    .push_surface(f, row.x, row.y, row.w, row.h, 4.0, Surface::Selected);
            }
            f.push(RenderCommand::Text {
                x: row.x + 4.0,
                y: row_y + 2.0,
                text: format!("#{} [{}-{}]", mi.saturating_add(1), m.start, m.end),
                font_size: SMALL_TEXT,
                color: if current {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(110.0),
                overflow: TextOverflow::Ellipsis,
            });
            let matched: String = input_chars
                .get(m.start..m.end.min(input_chars.len()))
                .unwrap_or_default()
                .iter()
                .collect();
            f.push(RenderCommand::Text {
                x: row.x + 4.0,
                y: row_y + LINE_HEIGHT,
                text: format!("\"{}\"", printable(&matched)),
                font_size: SMALL_TEXT,
                color: self.palette.ink(self.palette.green),
                font_weight: FontWeightHint::Regular,
                max_width: Some((row.w - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            if self.show_groups {
                let groups: Vec<String> = m
                    .groups
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter_map(|(gi, g)| {
                        g.map(|(gs, ge)| {
                            let text: String = input_chars
                                .get(gs..ge.min(input_chars.len()))
                                .unwrap_or_default()
                                .iter()
                                .collect();
                            format!("${gi}=\"{}\"", printable(&text))
                        })
                    })
                    .collect();
                if !groups.is_empty() {
                    f.push(RenderCommand::Text {
                        x: row.x + 120.0,
                        y: row_y + 2.0,
                        text: groups.join("  "),
                        font_size: SMALL_TEXT,
                        color: self.palette.ink(self.palette.mauve),
                        font_weight: FontWeightHint::Regular,
                        max_width: Some((row.w - 124.0).max(0.0)),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            f.hit(Target::MatchRow(mi), row);
        }
    }

    /// The current match's groups, one to a row. "Groups" was a sub-tab
    /// drawn with nothing behind it.
    fn draw_groups(&self, f: &mut Frame<Target>, body: Rect) {
        if let Some((message, colour)) = self.empty_results() {
            Self::body_text(f, body, body.y + 20.0, message.to_string(), colour);
            return;
        }
        let Some(m) = self.matches.get(self.current_match_index) else {
            return;
        };
        let input_chars: Vec<char> = self.input.text().chars().collect();
        let scroll = self.scroll_of(Pane::Results(ResultsView::Groups));
        for (gi, g) in m
            .groups
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows_in(body.h, LINE_HEIGHT).saturating_add(1))
        {
            let y = body.y + 6.0 + (gi.saturating_sub(scroll)) as f32 * LINE_HEIGHT;
            // Group 0 is the whole match, which the engine keeps in the
            // match's own span rather than in its group list.
            let (what, g) = if gi == 0 {
                (
                    format!("Match #{}", self.current_match_index.saturating_add(1)),
                    &Some((m.start, m.end)),
                )
            } else {
                (format!("${gi}"), g)
            };
            let line = match g {
                Some((gs, ge)) => {
                    let text: String = input_chars
                        .get(*gs..(*ge).min(input_chars.len()))
                        .unwrap_or_default()
                        .iter()
                        .collect();
                    format!("{what}  \"{}\"  [{gs}-{ge}]", printable(&text))
                }
                None => format!("{what}  took no part in this match"),
            };
            Self::body_text(
                f,
                body,
                y,
                line,
                if g.is_some() {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
            );
        }
    }

    /// The pattern, piece by piece -- all of it; it was cut at six lines.
    fn draw_explanation(&self, f: &mut Frame<Target>, body: Rect) {
        if self.explanations.is_empty() {
            Self::body_text(
                f,
                body,
                body.y + 20.0,
                "Enter a pattern to begin".to_string(),
                self.palette.subtext0,
            );
            return;
        }
        let scroll = self.scroll_of(Pane::Results(ResultsView::Explain));
        for (i, line) in self
            .explanations
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows_in(body.h, LINE_HEIGHT).saturating_add(1))
        {
            let y = body.y + 6.0 + (i.saturating_sub(scroll)) as f32 * LINE_HEIGHT;
            Self::body_text(f, body, y, line.clone(), self.palette.subtext1);
        }
    }

    fn draw_library(&self, f: &mut Frame<Target>) {
        let content = self.content_rect();
        let mut chip_x = PADDING;
        for (ci, cat) in LIBRARY_FILTERS.iter().enumerate() {
            let label = cat.map_or("All", PatternCategory::label);
            let w = text::measure(label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
            let rect = Rect::new(chip_x, content.y, w, 24.0);
            let selected = self.library_category_filter == *cat;
            self.palette.push_surface(
                f,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                12.0,
                if selected || self.hover == Some(Target::Chip(ci)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            f.push(RenderCommand::Text {
                x: rect.x + 8.0,
                y: rect.y + 5.0,
                text: label.into(),
                font_size: SMALL_TEXT,
                color: if selected {
                    cat.map_or(self.palette.ink(self.palette.blue), |c| {
                        c.color(&self.palette)
                    })
                } else {
                    self.palette.subtext0
                },
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Chip(ci), rect);
            chip_x += w + 6.0;
        }

        let list = self.library_list_rect();
        f.hit(Target::LibraryBody, list);
        let shown = self.visible_library();
        if shown.is_empty() {
            let message = if self.library_category_filter == Some(PatternCategory::Custom) {
                "No saved patterns yet: save one from the tester with Save… or Ctrl+S"
            } else {
                "No patterns in this category"
            };
            f.push(RenderCommand::Text {
                x: text::center_x(
                    message,
                    self.window_width / 2.0,
                    NORMAL_TEXT,
                    FontWeightHint::Regular,
                ),
                y: list.y + 40.0,
                text: message.into(),
                font_size: NORMAL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(list.w),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }
        f.clip(list);
        for (vi, (index, entry)) in shown
            .iter()
            .enumerate()
            .skip(self.library_scroll)
            .take(self.library_rows().saturating_add(1))
        {
            let row_y =
                list.y + (vi.saturating_sub(self.library_scroll)) as f32 * LIBRARY_ROW_HEIGHT;
            let row = Rect::new(list.x, row_y, list.w, LIBRARY_ROW_HEIGHT - 6.0);
            let selected = self.selected_library_entry == Some(*index);
            self.palette.push_surface(
                f,
                row.x,
                row.y,
                row.w,
                row.h,
                6.0,
                if selected || self.hover == Some(Target::LibraryRow(*index)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            f.hit(Target::LibraryRow(*index), row);

            let cat_label = entry.category.label();
            let badge_w = text::measure(cat_label, BADGE_TEXT, FontWeightHint::Bold) + 12.0;
            f.push(RenderCommand::FillRect {
                x: row.x + 8.0,
                y: row_y + 6.0,
                width: badge_w,
                height: 18.0,
                color: entry.category.color(&self.palette),
                corner_radii: CornerRadii::all(9.0),
            });
            f.push(RenderCommand::Text {
                x: row.x + 14.0,
                y: row_y + 9.0,
                text: cat_label.into(),
                font_size: BADGE_TEXT,
                color: self.palette.crust,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: row.x + badge_w + 16.0,
                y: row_y + 8.0,
                text: entry.name.clone(),
                font_size: NORMAL_TEXT,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });

            // The buttons, from the right; the description stops short of
            // them.
            let mut right = row.right() - 8.0;
            let mut buttons = vec![("Use", Target::LibraryUse(*index))];
            if entry.category == PatternCategory::Custom {
                buttons.push(("Delete", Target::LibraryDelete(*index)));
            }
            for (label, target) in buttons {
                let w = Self::button_width(label);
                let rect = Rect::new(right - w, row_y + 14.0, w, 26.0);
                self.button(f, rect, label, false, target);
                right = rect.x - 6.0;
            }
            let description = entry.flags.map_or_else(
                || entry.description.clone(),
                |flags| format!("{}  /{}", entry.description, flags.letters()),
            );
            f.push(RenderCommand::Text {
                x: (right - 240.0).max(row.x + badge_w + 330.0),
                y: row_y + 8.0,
                text: description,
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(230.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: row.x + 8.0,
                y: row_y + 30.0,
                text: entry.pattern.clone(),
                font_size: SMALL_TEXT,
                color: self.palette.ink(self.palette.sky),
                font_weight: FontWeightHint::Regular,
                max_width: Some((right - row.x - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        f.unclip();
    }

    /// The reference: two columns, which scroll together when the window is
    /// too short for them.
    fn draw_reference(&self, f: &mut Frame<Target>) {
        let content = self.content_rect();
        let col_width = ((self.window_width - 3.0 * PADDING) / 2.0).max(0.0);
        let right_x = PADDING + col_width + PADDING;
        f.hit(Target::ReferenceBody, content);
        self.palette.push_surface(
            f,
            PADDING,
            content.y,
            col_width,
            content.h,
            6.0,
            Surface::Card,
        );
        self.palette.push_surface(
            f,
            right_x,
            content.y,
            col_width,
            content.h,
            6.0,
            Surface::Card,
        );
        f.clip(content);
        let top = content.y - self.reference_scroll as f32 * LINE_HEIGHT;
        let heading = |f: &mut Frame<Target>, x: f32, y: f32, label: &str, colour: Color| {
            f.push(RenderCommand::Text {
                x,
                y,
                text: label.into(),
                font_size: HEADER_TEXT,
                color: colour,
                font_weight: FontWeightHint::Bold,
                max_width: Some((col_width - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        };
        let pair =
            |f: &mut Frame<Target>, x: f32, y: f32, syntax: &str, desc: &str, colour: Color| {
                f.push(RenderCommand::Text {
                    x,
                    y,
                    text: syntax.into(),
                    font_size: SMALL_TEXT,
                    color: colour,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });
                f.push(RenderCommand::Text {
                    x: x + 88.0,
                    y,
                    text: desc.into(),
                    font_size: SMALL_TEXT,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((col_width - 112.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            };

        heading(
            f,
            PADDING + 12.0,
            top + 10.0,
            "Syntax Reference",
            self.palette.ink(self.palette.blue),
        );
        for (si, (syntax, desc)) in SYNTAX_REFERENCE.iter().enumerate() {
            let y = top + 36.0 + si as f32 * LINE_HEIGHT;
            pair(
                f,
                PADDING + 12.0,
                y,
                syntax,
                desc,
                self.palette.ink(self.palette.green),
            );
        }

        heading(
            f,
            right_x + 12.0,
            top + 10.0,
            "Replacement Reference",
            self.palette.ink(self.palette.peach),
        );
        for (ri, (syntax, desc)) in REPLACEMENT_REFERENCE.iter().enumerate() {
            let y = top + 36.0 + ri as f32 * LINE_HEIGHT;
            pair(
                f,
                right_x + 12.0,
                y,
                syntax,
                desc,
                self.palette.ink(self.palette.peach),
            );
        }
        let tips_y = top + 140.0;
        heading(
            f,
            right_x + 12.0,
            tips_y,
            "Tips & Tricks",
            self.palette.ink(self.palette.teal),
        );
        for (ti, tip) in TIPS.iter().enumerate() {
            let y = tips_y + 26.0 + ti as f32 * LINE_HEIGHT;
            f.push(RenderCommand::Text {
                x: right_x + 16.0,
                y,
                text: format!("- {tip}"),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((col_width - 28.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        f.unclip();
    }

    /// How many rows the reference is, at its tallest column.
    fn reference_rows() -> usize {
        // Header (36 px) and the syntax list; the right column is its list,
        // the tips' heading at 140 px and the tips.
        let left = SYNTAX_REFERENCE.len().saturating_add(2);
        let right = TIPS.len().saturating_add(9);
        left.max(right)
    }

    fn draw_status_bar(&self, f: &mut Frame<Target>) {
        let y = self.window_height - STATUS_BAR_HEIGHT;
        self.palette.push_surface(
            f,
            0.0,
            y,
            self.window_width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );
        let message = self
            .hover
            .and_then(Target::tip)
            .map(str::to_string)
            .or_else(|| (!self.status.is_empty()).then(|| self.status.clone()));
        if let Some(message) = message {
            f.push(RenderCommand::Text {
                x: PADDING,
                y: y + 5.0,
                text: message,
                font_size: SMALL_TEXT,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some((self.window_width - 2.0 * PADDING).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Where the save dialog's card goes.
    fn save_dialog_rect(&self) -> Rect {
        let (w, h) = (380.0_f32.min(self.window_width - 20.0).max(0.0), 150.0);
        Rect::new(
            (self.window_width - w) / 2.0,
            (self.window_height - h) / 2.0,
            w,
            h,
        )
    }

    fn draw_save_dialog(&self, f: &mut Frame<Target>) {
        let Some(name) = &self.save_name else {
            return;
        };
        f.hit(
            Target::ModalBackdrop,
            Rect::new(0.0, 0.0, self.window_width, self.window_height),
        );
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: with_alpha(self.palette.crust, 140),
            corner_radii: CornerRadii::ZERO,
        });
        let card = self.save_dialog_rect();
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 8.0, Surface::Card);
        f.hit(Target::ModalBackdrop, card);
        f.push(RenderCommand::Text {
            x: card.x + 16.0,
            y: card.y + 14.0,
            text: "Save the pattern to the library".into(),
            font_size: HEADER_TEXT,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some((card.w - 32.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        let field = Rect::new(card.x + 16.0, card.y + 44.0, (card.w - 32.0).max(0.0), 32.0);
        self.palette
            .push_surface(f, field.x, field.y, field.w, field.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: field.x,
            y: field.y,
            width: field.w,
            height: field.h,
            color: self.palette.blue,
            line_width: 2.0,
            corner_radii: CornerRadii::all(4.0),
        });
        let mut tree = RenderTree::new();
        textedit::draw(
            &mut tree,
            &textedit::SingleLine {
                text: name.text(),
                cursor: name.cursor(),
                selection_anchor: name.selection_anchor(),
                focused: true,
                x: field.x + 8.0,
                y: field.y + 7.0,
                width: (field.w - 16.0).max(0.0),
                line_height: 18.0,
                font_size: NORMAL_TEXT,
                weight: FontWeightHint::Regular,
                color: self.palette.text,
                selection_bg: self.palette.blue,
                selection_fg: self.palette.crust,
                caret_width: textedit::CARET_WIDTH,
            },
        );
        f.extend(tree.commands);
        f.hit(Target::SaveName, field);
        if let Some(error) = &self.save_error {
            f.push(RenderCommand::Text {
                x: card.x + 16.0,
                y: field.bottom() + 8.0,
                text: error.clone(),
                font_size: SMALL_TEXT,
                color: self.palette.ink(self.palette.red),
                font_weight: FontWeightHint::Regular,
                max_width: Some((card.w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let mut right = card.right() - 16.0;
        for (label, lit, target) in [
            ("Save", true, Target::SaveConfirm),
            ("Cancel", false, Target::SaveCancel),
        ] {
            let w = Self::button_width(label);
            let rect = Rect::new(right - w, card.bottom() - 40.0, w, 28.0);
            self.button(f, rect, label, lit, target);
            right = rect.x - 8.0;
        }
    }

    // -- events --------------------------------------------------------------------

    /// Route one event, answering whether anything changed.
    ///
    /// **This app had no event handling of any kind until 2026-09-03**: `main`
    /// built an `App`, rendered one frame and returned; and until 2026-09-25 no
    /// pointer event reached it.
    fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Resize { width, height } => {
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                self.keep_caret_visible();
                true
            }
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    /// Keyboard handling.
    ///
    /// Typing goes to whichever field has focus, which is what makes this a
    /// tester rather than a viewer: the pattern is recompiled on every edit, so
    /// the match list under it follows the keystroke.
    fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return true;
        }
        if key.key == Key::Escape && self.show_help {
            self.show_help = false;
            return true;
        }
        if self.save_name.is_some() {
            return self.handle_save_key(key);
        }
        if key.key == Key::F3 {
            if key.modifiers.shift {
                self.prev_match();
            } else {
                self.next_match();
            }
            return !self.matches.is_empty();
        }
        if key.modifiers.ctrl
            && let Some(done) = self.handle_chord(key)
        {
            return done;
        }
        match self.active_tab {
            ActiveTab::Tester => self.handle_tester_key(key),
            ActiveTab::Library => self.handle_library_key(key),
            ActiveTab::Reference => self.handle_reference_key(key),
        }
    }

    /// A key with Ctrl held, or `None` for one that is an editing chord for
    /// the field with the keyboard.
    ///
    /// Chords and not bare letters, because every printable character is
    /// typed into whichever field has focus -- an `i` belongs in somebody's
    /// pattern before it belongs to a setting.
    fn handle_chord(&mut self, key: &KeyEvent) -> Option<bool> {
        Some(match key.key {
            Key::Num1 => self.set_tab(ActiveTab::Tester),
            Key::Num2 => self.set_tab(ActiveTab::Library),
            Key::Num3 => self.set_tab(ActiveTab::Reference),
            Key::G if key.modifiers.shift => {
                self.show_groups = !self.show_groups;
                true
            }
            Key::I => self.toggle_flag(Flag::CaseInsensitive),
            Key::G => self.toggle_flag(Flag::Global),
            Key::M => self.toggle_flag(Flag::Multiline),
            Key::R => self.toggle_replace(),
            Key::S => self.ask_to_save(),
            Key::L => {
                let at = LIBRARY_FILTERS
                    .iter()
                    .position(|c| *c == self.library_category_filter)
                    .unwrap_or(0);
                // A comparison rather than a remainder: `%` can divide by
                // zero and this crate denies arithmetic that can.
                let next = at.saturating_add(1);
                let wrapped = if next >= LIBRARY_FILTERS.len() {
                    0
                } else {
                    next
                };
                self.set_library_filter(LIBRARY_FILTERS.get(wrapped).copied().flatten())
            }
            // Editing chords belong to the field with the keyboard.
            Key::A | Key::C | Key::X | Key::V | Key::Home | Key::End => return None,
            _ => false,
        })
    }

    fn set_tab(&mut self, tab: ActiveTab) -> bool {
        if self.active_tab == tab {
            return false;
        }
        self.active_tab = tab;
        self.dragging = false;
        true
    }

    fn toggle_flag(&mut self, flag: Flag) -> bool {
        self.flags.toggle(flag);
        self.update_regex();
        true
    }

    fn toggle_replace(&mut self) -> bool {
        self.show_replace = !self.show_replace;
        // Hiding the pane takes the caret with it, rather than leaving it
        // typing into something off screen.
        if !self.show_replace && self.active_field == ActiveField::Replace {
            self.active_field = ActiveField::Pattern;
        }
        self.result_scroll = 0;
        self.update_regex();
        self.keep_caret_visible();
        true
    }

    /// How many characters `field` holds.
    ///
    /// `MAX_PATTERN_LEN`, `MAX_INPUT_LEN` and `MAX_REPLACE_LEN` were declared
    /// with the layout constants and consulted by nothing, so all three fields
    /// were unbounded. That matters more here than in most text boxes: the
    /// pattern is compiled and run across the whole input on *every
    /// keystroke*, so the cost of one character is a function of everything
    /// typed before it.
    fn capacity(field: ActiveField) -> usize {
        match field {
            ActiveField::Pattern => MAX_PATTERN_LEN,
            ActiveField::Input => MAX_INPUT_LEN,
            ActiveField::Replace => MAX_REPLACE_LEN,
        }
    }

    /// Move the keyboard to the next or the previous field on screen.
    fn cycle_field(&mut self, back: bool) {
        let fields: &[ActiveField] = if self.show_replace {
            &[
                ActiveField::Pattern,
                ActiveField::Input,
                ActiveField::Replace,
            ]
        } else {
            &[ActiveField::Pattern, ActiveField::Input]
        };
        let at = fields
            .iter()
            .position(|f| *f == self.active_field)
            .unwrap_or(0);
        let next = if back {
            at.checked_sub(1).unwrap_or(fields.len().saturating_sub(1))
        } else if at.saturating_add(1) >= fields.len() {
            0
        } else {
            at.saturating_add(1)
        };
        self.active_field = fields.get(next).copied().unwrap_or(ActiveField::Pattern);
    }

    fn handle_tester_key(&mut self, key: &KeyEvent) -> bool {
        if key.key == Key::Tab {
            // Cycles focus rather than inserting a tab: a regex tester's
            // fields are the whole interface, and Tab is how every form on
            // every desktop moves between them. Skips the replacement while
            // its pane is hidden.
            self.cycle_field(key.modifiers.shift);
            return true;
        }
        match self.active_field {
            ActiveField::Input => self.handle_input_key(key),
            field @ (ActiveField::Pattern | ActiveField::Replace) => {
                match key.key {
                    // Up and Down have no meaning in a one-line box; they step
                    // through the matches, as they always did here.
                    Key::Down | Key::Enter => {
                        self.next_match();
                        return !self.matches.is_empty();
                    }
                    Key::Up => {
                        self.prev_match();
                        return !self.matches.is_empty();
                    }
                    _ => {}
                }
                let capacity = Self::capacity(field);
                let clipboard = self.clipboard.clone();
                let input = if field == ActiveField::Pattern {
                    &mut self.pattern
                } else {
                    &mut self.replace
                };
                let before = (
                    input.text().to_string(),
                    input.cursor(),
                    input.selection_anchor(),
                );
                let edited = edit_line(input, key, capacity, &clipboard);
                let typed = input.text() != before.0;
                let moved = (input.cursor(), input.selection_anchor()) != (before.1, before.2);
                let copied = edited.copied.is_some();
                if let Some(copied) = edited.copied {
                    self.clipboard = copied;
                }
                if typed {
                    self.update_regex();
                }
                // A key that changed nothing -- a letter into a full field, End
                // at the end -- costs no frame and no recompile.
                edited.handled && (typed || moved || copied)
            }
        }
    }

    /// A key while the test input has the keyboard.
    fn handle_input_key(&mut self, key: &KeyEvent) -> bool {
        let shift = key.modifiers.shift;
        let ctrl = key.modifiers.ctrl;
        let page = self.input_rows().saturating_sub(1).max(1);
        let before = (self.input.caret, self.input.anchor, self.clipboard.len());
        let changed = match key.key {
            Key::Left => {
                self.input.left(shift);
                false
            }
            Key::Right => {
                self.input.right(shift);
                false
            }
            Key::Up => {
                self.input.vertical(false, 1, shift);
                false
            }
            Key::Down => {
                self.input.vertical(true, 1, shift);
                false
            }
            Key::PageUp => {
                self.input.vertical(false, page, shift);
                false
            }
            Key::PageDown => {
                self.input.vertical(true, page, shift);
                false
            }
            Key::Home if ctrl => {
                self.input.move_to(0, shift);
                false
            }
            Key::End if ctrl => {
                self.input.move_to(self.input.text().len(), shift);
                false
            }
            Key::Home => {
                self.input.home(shift);
                false
            }
            Key::End => {
                self.input.end(shift);
                false
            }
            Key::A if ctrl => {
                self.input.select_all();
                false
            }
            Key::C if ctrl => {
                if self.input.selected_text().is_empty() {
                    return false;
                }
                self.clipboard = self.input.selected_text().to_string();
                return true;
            }
            Key::X if ctrl => {
                if self.input.selected_text().is_empty() {
                    return false;
                }
                self.clipboard = self.input.selected_text().to_string();
                self.input.delete_selection()
            }
            Key::V if ctrl => {
                let clipboard = self.clipboard.clone();
                self.input
                    .insert(&clipboard, Self::capacity(ActiveField::Input))
            }
            Key::Enter => self.input.insert("\n", Self::capacity(ActiveField::Input)),
            Key::Backspace => self.input.backspace(),
            Key::Delete => self.input.delete(),
            _ => {
                if key.text.is_empty() || ctrl {
                    return false;
                }
                // Every character the keystroke produced, not just the first:
                // a dead key followed by a letter composes into one, and an
                // input method can deliver a whole word.
                self.input
                    .insert(&key.text, Self::capacity(ActiveField::Input))
            }
        };
        if changed {
            self.update_regex();
        }
        self.keep_caret_visible();
        changed || (self.input.caret, self.input.anchor, self.clipboard.len()) != before
    }

    fn handle_library_key(&mut self, key: &KeyEvent) -> bool {
        match key.key {
            Key::Up => self.step_library(-1),
            Key::Down => self.step_library(1),
            Key::PageUp => self.step_library(
                isize::try_from(self.library_rows())
                    .unwrap_or(1)
                    .saturating_neg(),
            ),
            Key::PageDown => self.step_library(isize::try_from(self.library_rows()).unwrap_or(1)),
            Key::Enter => self
                .selected_library_entry
                .is_some_and(|i| self.use_library_entry(i)),
            Key::Delete => self
                .selected_library_entry
                .is_some_and(|i| self.delete_library_entry(i)),
            _ => false,
        }
    }

    fn handle_reference_key(&mut self, key: &KeyEvent) -> bool {
        let rows = Self::reference_rows();
        let before = self.reference_scroll;
        match key.key {
            Key::Up => self.reference_scroll = self.reference_scroll.saturating_sub(1),
            Key::Down => {
                self.reference_scroll = self.reference_scroll.saturating_add(1).min(rows);
            }
            _ => return false,
        }
        self.reference_scroll != before
    }

    /// Keys while the save dialog is up: it has the keyboard.
    fn handle_save_key(&mut self, key: &KeyEvent) -> bool {
        match key.key {
            Key::Escape => {
                self.save_name = None;
                self.save_error = None;
                true
            }
            Key::Enter => self.confirm_save(),
            _ => {
                let clipboard = self.clipboard.clone();
                let Some(name) = self.save_name.as_mut() else {
                    return false;
                };
                let before = (
                    name.text().to_string(),
                    name.cursor(),
                    name.selection_anchor(),
                );
                let edited = edit_line(name, key, MAX_NAME_LEN, &clipboard);
                let changed = (
                    name.text().to_string(),
                    name.cursor(),
                    name.selection_anchor(),
                ) != before;
                let copied = edited.copied.is_some();
                if let Some(copied) = edited.copied {
                    self.clipboard = copied;
                }
                if changed {
                    self.save_error = None;
                }
                edited.handled && (changed || copied)
            }
        }
    }

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        // The card is modal: a press anywhere puts it away, and nothing
        // under it hears one.
        if self.show_help {
            if matches!(event.kind, MouseEventKind::Press(_)) {
                self.show_help = false;
                return true;
            }
            return false;
        }
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self.frame().hit_test(event.x, event.y) else {
                    return false;
                };
                self.activate(target, event.x, event.y)
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                match self.frame().hit_test(event.x, event.y) {
                    Some(Target::LibraryRow(i)) => self.use_library_entry(i),
                    _ => false,
                }
            }
            MouseEventKind::Release(MouseButton::Left) => std::mem::take(&mut self.dragging),
            MouseEventKind::Move => {
                let mut changed = false;
                if self.dragging {
                    let before = (self.input.caret, self.input.anchor);
                    self.place_input_caret(event.x, event.y, true);
                    changed = before != (self.input.caret, self.input.anchor);
                }
                let over = self.target_at(event.x, event.y);
                if over != self.hover {
                    self.hover = over;
                    changed = true;
                }
                changed
            }
            MouseEventKind::Leave => {
                self.dragging = false;
                self.hover.take().is_some()
            }
            MouseEventKind::Scroll { dy, .. } => {
                let Some(over) = self.target_at(event.x, event.y) else {
                    return false;
                };
                self.scroll(over, dy)
            }
            _ => false,
        }
    }

    /// Turn the wheel `dy` over `over`.
    ///
    /// Nothing here scrolled: `scroll_offset` and `match_scroll_offset` were
    /// read by the drawing and written by nothing, so a text longer than its
    /// box, the matches past the first screenful, the library's second half
    /// and the end of the reference could not be seen at all.
    fn scroll(&mut self, over: Target, dy: f32) -> bool {
        let pane = match Pane::of(over) {
            // The results panel's body is whichever view is showing.
            Some(Pane::Results(_)) => Pane::Results(self.results_view),
            Some(pane) => pane,
            None => return false,
        };
        let step = self.wheel.rows(dy);
        if step == 0 {
            return false;
        }
        let (len, room) = self.pane_extent(pane);
        let limit = len.saturating_sub(room.max(1));
        let slot = self.pane_scroll(pane);
        let before = *slot;
        *slot = slot.saturating_add_signed(step).min(limit);
        *slot != before
    }

    /// How many rows pane `pane` has, and how many fit.
    fn pane_extent(&self, pane: Pane) -> (usize, usize) {
        match pane {
            Pane::Input => (self.input.line_count(), self.input_rows()),
            Pane::Result => (
                self.replace_result
                    .as_deref()
                    .map_or(0, |r| r.split('\n').count()),
                self.tester_layout()
                    .result
                    .map_or(1, |r| rows_in(r.h - 30.0, LINE_HEIGHT)),
            ),
            Pane::Results(view) => (self.results_len(view), self.results_rows(view)),
            Pane::Library => (self.visible_library().len(), self.library_rows()),
            Pane::Reference => (
                Self::reference_rows(),
                rows_in(self.content_rect().h, LINE_HEIGHT),
            ),
        }
    }

    /// The first row pane `pane` shows, to change.
    fn pane_scroll(&mut self, pane: Pane) -> &mut usize {
        let [matches, groups, explain] = &mut self.results_scroll;
        match pane {
            Pane::Input => &mut self.input_scroll,
            Pane::Result => &mut self.result_scroll,
            Pane::Results(ResultsView::Matches) => matches,
            Pane::Results(ResultsView::Groups) => groups,
            Pane::Results(ResultsView::Explain) => explain,
            Pane::Library => &mut self.library_scroll,
            Pane::Reference => &mut self.reference_scroll,
        }
    }

    /// The first row pane `pane` shows.
    fn scroll_of(&self, pane: Pane) -> usize {
        let [matches, groups, explain] = self.results_scroll;
        match pane {
            Pane::Input => self.input_scroll,
            Pane::Result => self.result_scroll,
            Pane::Results(ResultsView::Matches) => matches,
            Pane::Results(ResultsView::Groups) => groups,
            Pane::Results(ResultsView::Explain) => explain,
            Pane::Library => self.library_scroll,
            Pane::Reference => self.reference_scroll,
        }
    }

    /// Put the test input's caret under the pointer at `(x, y)`.
    fn place_input_caret(&mut self, x: f32, y: f32, extend: bool) {
        let area = self.tester_layout().input_text();
        let hscroll = self.input_hscroll(area.w);
        let row = ((y - area.y) / LINE_HEIGHT).floor().max(0.0) as usize;
        let line = self.input_scroll.saturating_add(row);
        self.input.click(line, x - area.x + hscroll, extend);
    }

    /// Put a one-line field's caret under the pointer.
    fn place_line_caret(input: &mut TextInput, rect: Rect, x: f32) {
        let cursor = textedit::cursor_at_click(
            input.text(),
            input.cursor(),
            (rect.w - 16.0).max(0.0),
            NORMAL_TEXT,
            FontWeightHint::Regular,
            x - rect.x - 8.0,
        );
        input.set_selection_anchor(None);
        input.set_cursor(cursor);
    }

    /// Do what pressing `target` at `(x, y)` means.
    fn activate(&mut self, target: Target, x: f32, y: f32) -> bool {
        if self.save_name.is_some() {
            return match target {
                Target::SaveConfirm => self.confirm_save(),
                Target::SaveCancel => {
                    self.save_name = None;
                    self.save_error = None;
                    true
                }
                Target::SaveName => {
                    let field = self
                        .frame()
                        .rect_of(|t| *t == Target::SaveName)
                        .unwrap_or_default();
                    if let Some(name) = self.save_name.as_mut() {
                        Self::place_line_caret(name, field, x);
                    }
                    true
                }
                // The dialog is modal: nothing behind it hears a press.
                _ => false,
            };
        }
        match target {
            Target::Tab(tab) => self.set_tab(tab),
            Target::Flag(flag) => self.toggle_flag(flag),
            Target::MatchPrev => {
                self.prev_match();
                !self.matches.is_empty()
            }
            Target::MatchNext => {
                self.next_match();
                !self.matches.is_empty()
            }
            Target::ReplaceToggle => self.toggle_replace(),
            Target::SavePattern => self.ask_to_save(),
            Target::PatternField | Target::ReplaceField => {
                let l = self.tester_layout();
                let (field, rect) = if target == Target::PatternField {
                    (ActiveField::Pattern, l.pattern)
                } else {
                    (ActiveField::Replace, l.replace.unwrap_or(l.pattern))
                };
                self.active_field = field;
                let input = if field == ActiveField::Pattern {
                    &mut self.pattern
                } else {
                    &mut self.replace
                };
                Self::place_line_caret(input, rect, x);
                true
            }
            Target::InputArea => {
                self.active_field = ActiveField::Input;
                self.place_input_caret(x, y, false);
                self.dragging = true;
                true
            }
            Target::ResultTab(view) => {
                if self.results_view == view {
                    return false;
                }
                self.results_view = view;
                true
            }
            Target::GroupsInline => {
                self.show_groups = !self.show_groups;
                true
            }
            Target::MatchRow(i) => {
                if i == self.current_match_index || i >= self.matches.len() {
                    return false;
                }
                self.current_match_index = i;
                self.reveal_current_match();
                true
            }
            Target::Chip(i) => self.set_library_filter(LIBRARY_FILTERS.get(i).copied().flatten()),
            Target::LibraryRow(i) => {
                if self.selected_library_entry == Some(i) {
                    return false;
                }
                self.selected_library_entry = Some(i);
                true
            }
            Target::LibraryUse(i) => self.use_library_entry(i),
            Target::LibraryDelete(i) => self.delete_library_entry(i),
            Target::HelpCard => {
                self.show_help = false;
                true
            }
            Target::SaveName
            | Target::SaveConfirm
            | Target::SaveCancel
            | Target::ModalBackdrop
            | Target::ResultArea
            | Target::ResultsBody
            | Target::LibraryBody
            | Target::ReferenceBody => false,
        }
    }
}

/// What one keystroke did to a one-line field.
struct LineEdit {
    /// Whether the key was an editing key.
    handled: bool,
    /// What was copied or cut, for the window's clipboard.
    copied: Option<String>,
}

/// Apply a keystroke to a one-line field, as every text box in the tree
/// does, taking no more than leaves it at `capacity` characters. Paste reads
/// the window's `clipboard`; copy and cut hand theirs back in the result.
fn edit_line(input: &mut TextInput, key: &KeyEvent, capacity: usize, clipboard: &str) -> LineEdit {
    let shift = key.modifiers.shift;
    let ctrl = key.modifiers.ctrl;
    let mut copied = None;
    match key.key {
        Key::Left => input.move_cursor_left(shift, NORMAL_TEXT, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, NORMAL_TEXT, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if ctrl => input.select_all(),
        Key::C if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
            }
        }
        Key::X if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
                input.delete_selection();
            }
        }
        Key::V if ctrl => insert_limited(input, clipboard, capacity),
        _ => {
            if key.text.is_empty() || ctrl {
                return LineEdit {
                    handled: false,
                    copied: None,
                };
            }
            insert_limited(input, &key.text, capacity);
        }
    }
    LineEdit {
        handled: true,
        copied,
    }
}

/// Type `typed` into `input` over its selection, stopping at `capacity`
/// characters. A limit counted in characters, not bytes: a limit in bytes
/// would cut a multi-byte character in half.
///
/// `MAX_PATTERN_LEN` and its siblings were declared and consulted by nothing,
/// and it matters here more than in most text boxes: the pattern is compiled
/// and run across the whole input on *every keystroke*.
fn insert_limited(input: &mut TextInput, typed: &str, capacity: usize) {
    if input.has_selection() {
        input.delete_selection();
    }
    for ch in typed.chars() {
        // A field is one line: a control character -- a newline in a paste
        // included -- has no place in it.
        if ch.is_control() {
            continue;
        }
        if input.text().chars().count() >= capacity {
            break;
        }
        input.insert_char(ch);
    }
}

/// `text` with its line breaks and tabs shown as escapes, for a one-line
/// cell: a match that spans lines would otherwise draw as one run with the
/// break silently dropped.
fn printable(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
}

/// Every character's byte offset in `text`, and its length at the end: how
/// a match counted in characters is found in the text.
fn char_bytes(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect()
}

/// The byte offset of character `index` in `text`.
fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(b, _)| b)
}

/// How many whole rows of `row_h` fit in `height`: at least one.
fn rows_in(height: f32, row_h: f32) -> usize {
    if row_h <= 0.0 || !height.is_finite() {
        return 1;
    }
    let rows = (height / row_h).floor().max(0.0) as usize;
    rows.max(1)
}

/// The first row to show so that row `index` is in view, given the first
/// shown now and how many fit.
fn keep_in_view(first: usize, index: usize, rows: usize) -> usize {
    if index < first {
        index
    } else if index >= first.saturating_add(rows.max(1)) {
        index.saturating_sub(rows.max(1).saturating_sub(1))
    } else {
        first
    }
}

/// A library entry the user saved.
fn custom_entry(name: &str, pattern: &str, flags: RegexFlags) -> PatternEntry {
    PatternEntry {
        name: name.to_string(),
        pattern: pattern.to_string(),
        description: "Saved".to_string(),
        category: PatternCategory::Custom,
        flags: Some(flags),
    }
}

/// The syntax the engine understands, as the reference lists it.
const SYNTAX_REFERENCE: &[(&str, &str)] = &[
    (".", "Any character (except newline)"),
    ("^", "Start of string (of a line, with m)"),
    ("$", "End of string (of a line, with m)"),
    ("*", "Zero or more"),
    ("+", "One or more"),
    ("?", "Zero or one"),
    ("{n}", "Exactly n times"),
    ("{n,}", "n or more times"),
    ("{n,m}", "Between n and m times"),
    ("*? +? ??", "Lazy quantifiers"),
    ("(...)", "Capturing group"),
    ("(?:...)", "Non-capturing group"),
    ("a|b", "Alternation (a or b)"),
    ("[abc]", "Character class"),
    ("[^abc]", "Negated class"),
    ("[a-z]", "Character range"),
    ("\\d", "Digit [0-9]"),
    ("\\D", "Non-digit"),
    ("\\w", "Word char [a-zA-Z0-9_]"),
    ("\\W", "Non-word char"),
    ("\\s", "Whitespace"),
    ("\\S", "Non-whitespace"),
    ("\\b", "Word boundary"),
    ("\\n \\r \\t", "Newline, CR, Tab"),
    ("\\\\", "Escaped backslash"),
];

/// What a replacement can say, as the reference lists it.
const REPLACEMENT_REFERENCE: &[(&str, &str)] = &[
    ("$0", "Entire match"),
    ("$1-$9", "Capture group N"),
    ("\\n", "Newline"),
    ("\\t", "Tab"),
    ("\\\\", "Literal backslash"),
];

const TIPS: &[&str] = &[
    "Use \\b for word boundaries to avoid partial matches",
    "Character classes [] are faster than alternation |",
    "Non-capturing groups (?:) when you don't need the capture",
    "Use lazy quantifiers *? +? to match as little as possible",
    "Anchors ^ $ don't consume characters",
    "Escape special chars with \\ when matching literally",
    "Test patterns incrementally - start simple, add complexity",
];

impl oswindow::app::App for App {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Regex Tester".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> oswindow::app::Response {
        use oswindow::app::Response;
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        if self.handle_event(event) {
            Response::Redraw
        } else {
            Response::Idle
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window_width = width;
        self.window_height = height;
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }

    // No `tick_interval`: nothing here ages. A regex tester recompiles on a
    // keystroke and has no animation, no playback and no timer.
}

fn main() -> std::process::ExitCode {
    let mut app = App::new();
    // The user's own patterns, saved from an earlier session.
    app.load_library(&settingsfile::load(CONFIG_NAME));
    oswindow::app::launch("regextester", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that overflows or indexes out of range should fail loudly and
    // point at the line that did it — that is the diagnosis. The defensive
    // lints exist to keep panics out of code that runs on a user's data.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    /// Every string this program draws at a plausible window size.
    fn drawn(app: &mut App) -> Vec<String> {
        app.render(1200.0, 800.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// An app with a pattern, some input and a replacement, all applied.
    fn replacing() -> App {
        let mut app = App::new();
        app.pattern.set_text("world");
        app.input.set_text("hello world");
        app.replace.set_text("earth");
        app.show_replace = true;
        app.update_regex();
        app
    }

    /// **The replaced text is on the screen.**
    ///
    /// `apply_replacement` has filled `replace_result` on every keystroke
    /// since this program was written and nothing drew it. The user was shown
    /// which parts matched -- the working -- and never the answer.
    #[test]
    fn the_replacement_result_is_drawn() {
        let mut app = replacing();
        assert_eq!(app.replace_result.as_deref(), Some("hello earth"));

        let texts = drawn(&mut app);
        assert!(
            texts.iter().any(|t| t == "hello earth"),
            "the result is computed and not drawn; drawn: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Result:"),
            "the panel has no label; drawn: {texts:?}"
        );
    }

    /// With Replace turned off there is no result panel to read.
    #[test]
    fn no_result_panel_until_replace_is_shown() {
        let mut app = replacing();
        app.show_replace = false;
        let texts = drawn(&mut app);
        assert!(!texts.iter().any(|t| t == "Result:"), "{texts:?}");
    }

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
        assert!(app.pattern.text().is_empty());
        assert!(app.input.text().is_empty());
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
        app.pattern.set_text("\\d+");
        app.input.set_text("abc123def456");
        app.update_regex();
        assert!(app.compiled.is_some());
        assert!(app.compile_error.is_none());
        assert!(!app.matches.is_empty());
    }

    #[test]
    fn test_app_update_invalid_pattern() {
        let mut app = App::new();
        app.pattern.set_text("(unclosed");
        app.update_regex();
        assert!(app.compile_error.is_some());
        assert!(app.matches.is_empty());
    }

    #[test]
    fn test_app_match_navigation() {
        let mut app = App::new();
        app.pattern.set_text("\\d");
        app.input.set_text("a1b2c3");
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
    fn test_app_load_library() {
        let mut app = App::new();
        assert!(app.use_library_entry(0));
        assert!(app.selected_library_entry.is_some());
        assert!(!app.pattern.text().is_empty());
    }

    #[test]
    fn test_app_save_to_library() {
        settingsfile::testing::with_scratch_config("rt_save_basic", |_| {
            let mut app = App::new();
            let initial_len = app.library.len();
            app.pattern.set_text("custom_pattern");
            assert!(app.ask_to_save());
            app.save_name.as_mut().unwrap().set_text("My Pattern");
            assert!(app.confirm_save());
            assert_eq!(app.library.len(), initial_len + 1);
        });
    }

    #[test]
    fn test_app_save_empty_rejected() {
        settingsfile::testing::with_scratch_config("rt_save_empty", |_| {
            let mut app = App::new();
            let initial_len = app.library.len();
            // An empty pattern is not offered a name at all.
            app.ask_to_save();
            assert!(app.save_name.is_none());
            assert!(app.status.starts_with("Nothing to save"), "{}", app.status);
            // An empty name is refused, and the dialog stays up to say so.
            app.pattern.set_text("x+");
            app.ask_to_save();
            app.confirm_save();
            assert_eq!(app.library.len(), initial_len);
            assert!(app.save_name.is_some());
            assert!(app.save_error.is_some());
        });
    }

    #[test]
    fn test_app_match_stats_empty() {
        let app = App::new();
        assert_eq!(app.match_stats(), "No matches");
    }

    #[test]
    fn test_app_match_stats_with_matches() {
        let mut app = App::new();
        app.pattern.set_text("\\d+");
        app.input.set_text("abc123");
        app.update_regex();
        let stats = app.match_stats();
        assert!(stats.contains("match"));
    }

    #[test]
    fn test_app_replace() {
        let mut app = App::new();
        app.pattern.set_text("world");
        app.input.set_text("hello world");
        app.replace.set_text("earth");
        app.show_replace = true;
        app.update_regex();
        assert!(app.replace_result.is_some());
    }

    #[test]
    fn test_app_render() {
        let app = App::new();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_all_tabs() {
        let mut app = App::new();

        app.active_tab = ActiveTab::Tester;
        let cmds1 = app.render_commands();
        assert!(!cmds1.is_empty());

        app.active_tab = ActiveTab::Library;
        let cmds2 = app.render_commands();
        assert!(!cmds2.is_empty());

        app.active_tab = ActiveTab::Reference;
        let cmds3 = app.render_commands();
        assert!(!cmds3.is_empty());
    }

    // --- Utility tests ---

    /// Truncation is by measured width now, not by a character budget derived
    /// from a nominal cell. The old helper compared `s.len()` — bytes —
    /// against that budget, so an accented pattern was cut short even when it
    /// fitted, and a wide one still overflowed.
    #[test]
    fn elided_text_fits_the_box_it_is_drawn_in() {
        let box_w = 90.0;
        for s in [
            "hello",
            "hello world this is a long line of text",
            "éééééééééééé",
        ] {
            let out = text::elide(s, box_w, "...", NORMAL_TEXT, FontWeightHint::Regular);
            let w = text::measure(&out, NORMAL_TEXT, FontWeightHint::Regular);
            assert!(w <= box_w + 0.01, "{out:?} is {w} px in a {box_w} px box");
        }
    }

    /// Text that already fits is passed through untouched.
    #[test]
    fn text_that_fits_is_not_elided() {
        let out = text::elide("hi", 500.0, "...", NORMAL_TEXT, FontWeightHint::Regular);
        assert_eq!(out, "hi");
    }

    /// A match highlight has to sit exactly over the substring it marks. The
    /// offsets are bytes, so an accent before the match used to slide the band
    /// one cell right per extra byte.
    #[test]
    fn match_highlight_lines_up_with_the_match() {
        let line = "éé needle tail";
        let start = line.find("needle").expect("literal is present");
        let end = start + "needle".len();
        let upto = |b: usize| {
            text::measure(
                line.get(..b).expect("byte offset is on a boundary"),
                NORMAL_TEXT,
                FontWeightHint::Regular,
            )
        };
        let hl_x = upto(start);
        let hl_w = upto(end) - hl_x;
        assert!(hl_w > 0.0, "the band has no width");
        assert!(
            hl_x > 0.0,
            "two accents before the match must push the band right of zero"
        );
        assert!(
            (hl_x + hl_w - upto(end)).abs() < 0.01,
            "the band does not end where the match does"
        );
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
        app.pattern.set_text("hello");
        app.input.set_text("Hello HELLO hello");
        app.flags.case_insensitive = true;
        app.update_regex();
        assert!(app.matches.len() >= 2);
    }

    // --- Compositor wiring ---

    use oswindow::app::App as _;

    fn press(app: &mut App, k: guitk::event::Key, text: &str) -> bool {
        app.handle_event(&guitk::event::Event::Key(guitk::event::KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: text.to_string(),
        }))
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// The label is read by `guitk::shortcut` rather than matched against a
    /// table beside it here, which would be a third copy of the same fact.
    ///
    /// A key answers `true` only when it changed something -- End at the end
    /// does not -- so the check asks whether *some* state answers it, over
    /// states chosen so that between them every key has work.
    #[test]
    fn every_advertised_key_does_something() {
        settingsfile::testing::with_scratch_config("rt_advertised", |_| {
            for (label, what) in SHORTCUTS {
                for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                    let answered = help_states()
                        .iter_mut()
                        .any(|app| app.handle_event(&guitk::event::Event::Key(stroke.clone())));
                    assert!(
                        answered,
                        "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                        stroke.key
                    );
                }
            }
        });
    }

    /// Testers chosen so that between them every advertised key has work.
    fn help_states() -> Vec<App> {
        let base = || {
            let mut app = App::new();
            app.set_input("banana");
            app.set_pattern("a");
            app.clipboard = "x".to_string();
            app
        };
        // The caret at the end of the pattern: Left, Home, Backspace, and a
        // clipboard to paste.
        let at_end = base();
        // At its start: Right, End, Delete.
        let mut at_start = base();
        at_start.pattern.move_home(false);
        // All of it selected: copy and cut.
        let mut selected = base();
        selected.pattern.select_all();
        // In the library, with a saved pattern selected: Ctrl+1 has somewhere
        // to go back to, and Delete something to delete.
        let mut library = base();
        library.active_tab = ActiveTab::Library;
        library
            .library
            .push(custom_entry("mine", "m+", RegexFlags::default()));
        library.selected_library_entry = Some(library.library.len() - 1);
        vec![at_end, at_start, selected, library]
    }

    /// **All three tabs can be reached, and each draws something different.**
    ///
    /// `active_tab` was `ActiveTab::Tester` at construction, matched to choose
    /// the view, drawn to highlight the strip -- and written only by tests. So
    /// the Library and Reference tabs were rendered code no user could reach,
    /// in a window that showed three tabs. A tab strip with one tab
    /// highlighted looks exactly like a tab strip, which is why this survived
    /// a careful reading of the same file two hours earlier.
    ///
    /// Asserts what each tab *draws*, not which variant the field holds: a
    /// chord that sets the enum and a renderer that ignores it would pass the
    /// weaker test, which is the defect `multiline` had in this very app.
    #[test]
    fn every_tab_can_be_reached_and_shows_its_own_content() {
        let mut app = App::new();
        let tester = drawn_help_text(&mut app);
        assert!(
            tester.contains("Tester"),
            "the tab strip is not drawn at all"
        );

        assert!(ctrl(&mut app, guitk::event::Key::Num3), "Ctrl+3 unanswered");
        let reference = drawn_help_text(&mut app);
        assert!(
            reference.contains("Syntax Reference"),
            "Ctrl+3 did not bring up the reference tab"
        );

        assert!(ctrl(&mut app, guitk::event::Key::Num2), "Ctrl+2 unanswered");
        let library = drawn_help_text(&mut app);
        assert_ne!(
            library, reference,
            "the library and the reference draw the same thing"
        );

        assert!(ctrl(&mut app, guitk::event::Key::Num1), "Ctrl+1 unanswered");
        assert_eq!(
            drawn_help_text(&mut app),
            tester,
            "Ctrl+1 did not come back to the tester"
        );
    }

    /// **The library's category chips can be chosen, and the list follows.**
    ///
    /// `library_category_filter` was `None` at construction, read to highlight
    /// the selected chip and again to filter the entries, and written
    /// nowhere -- eight chips drawn and none selectable. Fourth defect found
    /// in this one crate.
    ///
    /// Asserts the *entries on screen*, not the field: a key that sets the
    /// filter while the list ignores it would pass the weaker version, which
    /// is this crate's own `multiline` defect from earlier today.
    #[test]
    fn the_library_categories_can_be_chosen_and_filter_the_list() {
        let mut app = App::new();
        assert!(ctrl(&mut app, guitk::event::Key::Num2), "Ctrl+2 unanswered");

        let shown = |app: &mut App| -> usize {
            drawn_help_text(app)
                .split(" | ")
                .filter(|t| !t.is_empty())
                .count()
        };
        let all = shown(&mut app);
        assert!(app.library_category_filter.is_none(), "starts unfiltered");

        // Step to the first real category; fewer entries have to be drawn.
        assert!(ctrl(&mut app, guitk::event::Key::L), "Ctrl+L unanswered");
        assert_eq!(
            app.library_category_filter,
            Some(PatternCategory::Validation),
            "Ctrl+L did not step to the first category"
        );
        assert!(
            shown(&mut app) < all,
            "filtering to one category drew as much as no filter did"
        );

        // ...and round the ring, back to no filter.
        for _ in 0..(LIBRARY_FILTERS.len() - 1) {
            ctrl(&mut app, guitk::event::Key::L);
        }
        assert!(
            app.library_category_filter.is_none(),
            "the cycle did not come back to All"
        );
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/rssreader`'s overlay drew twenty of its
    /// twenty-one rows for weeks.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = App::new();
        assert!(
            !drawn_help_text(&mut app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        assert!(press(&mut app, guitk::event::Key::F1, ""));
        let shown = drawn_help_text(&mut app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(*keys), "{keys:?} never reached the window");
            assert!(shown.contains(*what), "{what:?} never reached the window");
        }

        assert!(press(&mut app, guitk::event::Key::Escape, ""));
        assert!(
            !drawn_help_text(&mut app).contains("F1 closes this"),
            "Escape did not close it"
        );
    }

    /// Every string the window is drawing, joined.
    fn drawn_help_text(app: &mut App) -> String {
        let (w, h) = (app.window_width, app.window_height);
        oswindow::app::App::render(app, w, h)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A key with Ctrl and Shift held.
    fn ctrl_shift(app: &mut App, k: guitk::event::Key) -> bool {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = true;
        modifiers.shift = true;
        app.handle_event(&guitk::event::Event::Key(guitk::event::KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        }))
    }

    /// **The replacement box can be shown, and Tab stops skipping it.**
    ///
    /// `show_replace` was `false` at construction with no writer, so the pane
    /// was never drawn and `replace_in_active` never ran -- while `Tab` cycled
    /// the caret *into* the field it holds, and the shortcut list said Tab
    /// moved between three fields. One of the three was invisible.
    #[test]
    fn the_replacement_box_can_be_shown_and_tab_follows_it() {
        let mut app = App::new();

        // Hidden: Tab goes pattern -> input -> pattern, never resting in a
        // field the window is not drawing.
        assert!(!app.show_replace, "the pane starts hidden");
        assert_eq!(app.active_field, ActiveField::Pattern);
        press(&mut app, guitk::event::Key::Tab, "");
        assert_eq!(app.active_field, ActiveField::Input);
        press(&mut app, guitk::event::Key::Tab, "");
        assert_eq!(
            app.active_field,
            ActiveField::Pattern,
            "Tab rested in the replacement field while its pane was hidden"
        );

        // Shown: the third field joins the cycle.
        assert!(ctrl(&mut app, guitk::event::Key::R), "Ctrl+R unanswered");
        assert!(app.show_replace, "Ctrl+R did not show the pane");
        press(&mut app, guitk::event::Key::Tab, "");
        press(&mut app, guitk::event::Key::Tab, "");
        assert_eq!(
            app.active_field,
            ActiveField::Replace,
            "the replacement field is drawn and Tab still skips it"
        );

        // Hiding it again takes the caret out rather than leaving it typing
        // into something off screen.
        assert!(ctrl(&mut app, guitk::event::Key::R));
        assert_eq!(app.active_field, ActiveField::Pattern);
    }

    /// **The capture groups can be put away, and the chord is not the flag.**
    ///
    /// `Ctrl+Shift+G` sits above `Ctrl+G`, which toggles the global flag. A
    /// guard narrows only the arm it is on, so the wrong order would send this
    /// chord to the flag -- and both answer `true`, so only the effect tells
    /// them apart.
    #[test]
    fn the_capture_groups_can_be_hidden_without_touching_the_global_flag() {
        let mut app = App::new();
        let global = app.flags.global;
        assert!(app.show_groups, "the groups start shown");

        assert!(ctrl_shift(&mut app, guitk::event::Key::G), "unanswered");

        assert!(!app.show_groups, "Ctrl+Shift+G did not hide the groups");
        assert_eq!(
            app.flags.global, global,
            "Ctrl+Shift+G fell through to the global flag"
        );
    }

    /// A key with Ctrl held.
    fn ctrl(app: &mut App, k: guitk::event::Key) -> bool {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = true;
        app.handle_event(&guitk::event::Event::Key(guitk::event::KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        }))
    }

    /// **The three flags the toolbar draws can be changed.**
    ///
    /// They were drawn as buttons, coloured by their state, and read by the
    /// matcher -- and the only assignment to any of them in the crate was
    /// inside a test. The window offered three settings and answered none of
    /// them. Found by `scripts/frozen-flag-survey.py`, which looks for exactly
    /// this: a boolean the program reads and can never write.
    #[test]
    fn the_flag_buttons_can_be_toggled() {
        let mut app = App::new();
        for (key, read) in [
            (guitk::event::Key::I, 0usize),
            (guitk::event::Key::G, 1),
            (guitk::event::Key::M, 2),
        ] {
            let before = [
                app.flags.case_insensitive,
                app.flags.global,
                app.flags.multiline,
            ][read];
            assert!(ctrl(&mut app, key), "the chord was not answered");
            let after = [
                app.flags.case_insensitive,
                app.flags.global,
                app.flags.multiline,
            ][read];
            assert_ne!(before, after, "{key:?} did not change its flag");
        }
    }

    /// **`m` changes what the pattern matches.**
    ///
    /// The flag existed, was drawn, and was read by nothing: the matcher's
    /// anchor arm was `pos == 0` and `pos == len` whatever the flag said. A
    /// toggle for a setting with no effect is worse than no toggle, because
    /// the button is a claim -- so this asserts the *result*, not the field.
    #[test]
    fn multiline_makes_the_anchors_match_at_line_boundaries() {
        // Built from a char code rather than written as an escape, so no
        // heredoc or editor between here and the file can turn it into a real
        // newline -- which has happened three times today.
        let haystack = ["alpha", "beta", "gamma"].join(&String::from(char::from(10)));

        let one_line = RegexCompiler::new("^beta$", false)
            .compile()
            .expect("a valid pattern");
        assert!(
            execute_regex(&one_line, &haystack, 0).is_none(),
            "without the flag, ^ and $ are the ends of the whole text"
        );

        let many = RegexCompiler::new("^beta$", false)
            .multiline(true)
            .compile()
            .expect("a valid pattern");
        let found = execute_regex(&many, &haystack, 0).expect("the middle line");
        assert_eq!(
            haystack.get(found.start..found.end),
            Some("beta"),
            "the flag matched something other than the middle line"
        );
    }

    /// Typing goes to the focused field and recompiles as it goes.
    ///
    /// This app had no event handling until 2026-09-03: `main` built an `App`,
    /// rendered one frame and returned, so `update_regex` was exercised only by
    /// tests calling it directly. Being able to *type* a pattern is the whole
    /// application.
    #[test]
    fn typing_a_pattern_recompiles_it() {
        let mut app = App::new();
        app.active_field = ActiveField::Pattern;
        for c in ['a', '+', 'b'] {
            assert!(press(&mut app, guitk::event::Key::A, &c.to_string()));
        }
        assert_eq!(app.pattern.text(), "a+b");
        assert!(app.compiled.is_some(), "the pattern was not compiled");
        assert!(app.compile_error.is_none());
    }

    /// Tab cycles the fields on screen rather than inserting a tab
    /// character.
    ///
    /// **This test used to assert the cycle reached `Replace` unconditionally,
    /// and that was the defect written down as a requirement.** The
    /// replacement pane is drawn only when `show_replace` is set, and
    /// `show_replace` was `false` with no writer anywhere -- so the cycle this
    /// test protected put the caret in a field the window never drew. The
    /// pane can be opened now, and the cycle follows what is on screen.
    #[test]
    fn tab_cycles_the_fields_that_are_on_screen() {
        let mut app = App::new();
        app.active_field = ActiveField::Pattern;
        assert!(!app.show_replace, "the replacement pane starts hidden");

        press(&mut app, guitk::event::Key::Tab, "");
        assert_eq!(app.active_field, ActiveField::Input);
        press(&mut app, guitk::event::Key::Tab, "");
        assert_eq!(
            app.active_field,
            ActiveField::Pattern,
            "Tab rested in the replacement field while its pane was hidden"
        );

        // With the pane open the third field joins the cycle.
        assert!(ctrl(&mut app, guitk::event::Key::R));
        press(&mut app, guitk::event::Key::Tab, "");
        press(&mut app, guitk::event::Key::Tab, "");
        assert_eq!(app.active_field, ActiveField::Replace);

        assert!(
            app.pattern.text().is_empty(),
            "Tab typed a character into the field"
        );
    }

    /// Backspace removes a whole character, not a byte.
    ///
    /// The field holds a regex, which may contain any character. Truncating by
    /// one byte would split a multi-byte one and panic on the next slice — so
    /// this is a crash test, not a cosmetic one.
    #[test]
    fn backspace_removes_a_whole_character() {
        let mut app = App::new();
        app.active_field = ActiveField::Input;
        app.input.set_text("aé");
        assert!(press(&mut app, guitk::event::Key::Backspace, ""));
        assert_eq!(app.input.text(), "a");
        assert!(press(&mut app, guitk::event::Key::Backspace, ""));
        assert_eq!(app.input.text(), "");
        assert!(
            !press(&mut app, guitk::event::Key::Backspace, ""),
            "backspace on an empty field reported a change"
        );
    }

    /// The size the compositor grants is the size the layout uses.
    ///
    /// The renderer read the `WINDOW_WIDTH`/`WINDOW_HEIGHT` constants directly
    /// until 2026-09-03, so the first frame was laid out for 1100x750 whatever
    /// the window actually was, and a resize changed nothing.
    #[test]
    fn the_layout_follows_the_granted_size() {
        let mut app = App::new();
        let _ = app.render(1600.0, 900.0);
        assert_eq!(app.window_width, 1600.0);
        assert_eq!(app.window_height, 900.0);

        app.handle_event(&guitk::event::Event::Resize {
            width: 800,
            height: 600,
        });
        assert_eq!(app.window_width, 800.0);
    }

    /// Replacement still works after the indexing rewrite.
    ///
    /// `apply_replacement` walks the replacement string by index and had seven
    /// bare `rep_chars[ri]`. The rewrite has to preserve `$1` group references
    /// and backslash escapes, which is what this checks — the arithmetic being
    /// right is not the same as the behaviour being unchanged.
    #[test]
    fn replacement_survives_the_bounds_rewrite() {
        let mut app = App::new();
        app.active_field = ActiveField::Pattern;
        app.pattern.set_text("(a)(b)");
        app.input.set_text("ab");
        app.replace.set_text("$2$1");
        // The replace pane has to be open, or `update_regex` deliberately leaves
        // `replace_result` empty — which is what this test first caught.
        app.show_replace = true;
        app.update_regex();
        assert_eq!(
            app.replace_result.as_deref(),
            Some("ba"),
            "group references did not survive"
        );
    }

    // --- Field limits ---
    //
    // `MAX_PATTERN_LEN`, `MAX_INPUT_LEN` and `MAX_REPLACE_LEN` were declared
    // with the layout constants and consulted by nothing, so all three fields
    // grew without bound -- and `update_regex` runs a backtracking engine over
    // the whole input on every keystroke.

    #[test]
    fn a_field_stops_accepting_at_its_limit() {
        let mut app = App::new();
        app.active_field = ActiveField::Pattern;
        app.pattern.set_text(&"a".repeat(MAX_PATTERN_LEN));
        assert!(
            !press(&mut app, guitk::event::Key::A, "b"),
            "a keystroke into a full field costs no redraw and no recompile"
        );
        assert_eq!(app.pattern.text().chars().count(), MAX_PATTERN_LEN);
    }

    #[test]
    fn each_field_has_its_own_limit() {
        assert_eq!(App::capacity(ActiveField::Pattern), MAX_PATTERN_LEN);
        assert_eq!(App::capacity(ActiveField::Input), MAX_INPUT_LEN);
        assert_eq!(App::capacity(ActiveField::Replace), MAX_REPLACE_LEN);
    }

    #[test]
    fn a_field_one_short_of_its_limit_still_accepts_one() {
        let mut app = App::new();
        app.active_field = ActiveField::Replace;
        app.replace
            .set_text(&"x".repeat(MAX_REPLACE_LEN.saturating_sub(1)));
        assert!(press(&mut app, guitk::event::Key::A, "y"));
        assert_eq!(app.replace.text().chars().count(), MAX_REPLACE_LEN);
    }

    #[test]
    fn the_limit_is_counted_in_characters_not_bytes() {
        // A limit in bytes would cut a multi-byte character in half, and these
        // fields hold any character at all.
        let mut app = App::new();
        app.active_field = ActiveField::Replace;
        app.replace
            .set_text(&"é".repeat(MAX_REPLACE_LEN.saturating_sub(1)));
        assert!(press(&mut app, guitk::event::Key::A, "é"));
        assert_eq!(app.replace.text().chars().count(), MAX_REPLACE_LEN);
        assert!(!press(&mut app, guitk::event::Key::A, "é"));
    }

    #[test]
    fn a_long_keystroke_is_truncated_rather_than_refused() {
        let mut app = App::new();
        app.active_field = ActiveField::Replace;
        app.replace
            .set_text(&"x".repeat(MAX_REPLACE_LEN.saturating_sub(2)));
        // Three characters offered, two seats left.
        assert!(press(&mut app, guitk::event::Key::A, "abc"));
        assert_eq!(app.replace.text().chars().count(), MAX_REPLACE_LEN);
        assert!(app.replace.text().ends_with("ab"));
    }

    // --- Multi-character keystrokes ---

    #[test]
    fn every_character_a_keystroke_typed_is_taken() {
        // One keypress can produce none, one, or several characters: a dead
        // key composes with the next, and an input method can deliver a whole
        // word. This used to take `text.chars().next()` and drop the rest.
        let mut app = App::new();
        app.active_field = ActiveField::Input;
        assert!(press(&mut app, guitk::event::Key::A, "the"));
        assert_eq!(app.input.text(), "the");
    }

    #[test]
    fn a_keystroke_that_typed_nothing_costs_no_frame() {
        let mut app = App::new();
        app.active_field = ActiveField::Input;
        assert!(!press(&mut app, guitk::event::Key::A, ""));
        assert!(
            !press(&mut app, guitk::event::Key::A, "\u{7}"),
            "a control character is not text"
        );
        assert_eq!(app.input.text(), "");
    }

    #[test]
    fn control_characters_are_dropped_from_a_mixed_keystroke() {
        let mut app = App::new();
        app.active_field = ActiveField::Input;
        assert!(press(&mut app, guitk::event::Key::A, "a\u{7}b"));
        assert_eq!(app.input.text(), "ab");
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        use oswindow::app::App as _;
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut App) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = App::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // The pointer, the fields, the library
    //
    // `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`: three
    // tabs, three flag buttons, three fields, a match list, three sub-tabs,
    // eight chips and a library, and no pointer event handled at all.
    // ------------------------------------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for App {
        type Target = Target;
        type Outcome = bool;
        const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

        /// Drawn at the window's own size, which these tests leave at `SIZE`
        /// unless they resize it, and then read the frame directly.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(&mut self, x: f32, y: f32, button: MouseButton, _size: (f32, f32)) -> bool {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> bool {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<bool> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    /// A tester with `pattern` run over `input`.
    fn testing(pattern: &str, input: &str) -> App {
        let mut app = App::new();
        app.set_input(input);
        app.set_pattern(pattern);
        app
    }

    /// Every piece of text the window draws.
    fn texts(app: &App) -> Vec<String> {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// States that between them draw every control there is.
    fn pointer_states() -> Vec<(&'static str, App)> {
        let mut tester = testing("(a)n", "banana");
        tester.toggle_replace();
        tester.set_replacement("<$1>");
        let mut library = App::new();
        library.active_tab = ActiveTab::Library;
        library
            .library
            .push(custom_entry("mine", "m+", RegexFlags::default()));
        // The saved one is last, past the rows that fit; the chip brings it up.
        library.library_category_filter = Some(PatternCategory::Custom);
        let mut reference = App::new();
        reference.active_tab = ActiveTab::Reference;
        let mut saving = testing("a", "banana");
        saving.ask_to_save();
        vec![
            ("the tester", tester),
            ("the library", library),
            ("the reference", reference),
            ("the save dialog", saving),
        ]
    }

    /// **Every control drawn is the one a press on it reaches** -- none is
    /// covered by another -- and between them the states draw every kind of
    /// control there is. What each one does is the business of the tests
    /// after this.
    #[test]
    fn every_control_drawn_is_the_one_a_press_on_it_reaches() {
        let mut kinds = std::collections::BTreeSet::new();
        for (what, app) in pointer_states() {
            let frame = app.frame();
            let modal = app.save_name.is_some();
            for (target, rect) in frame.hits() {
                kinds.insert(probe::variant_name(*target));
                // Under the dialog everything is covered, which is the point
                // of a modal; only its own controls must be reachable.
                if modal
                    && !matches!(
                        target,
                        Target::SaveName | Target::SaveConfirm | Target::SaveCancel
                    )
                {
                    continue;
                }
                // Those that hold other controls, and are there for the wheel
                // or to swallow a press rather than to answer one.
                if matches!(
                    target,
                    Target::ResultsBody
                        | Target::LibraryBody
                        | Target::ReferenceBody
                        | Target::ModalBackdrop
                        | Target::LibraryRow(_)
                ) {
                    continue;
                }
                let (x, y) = rect.centre();
                assert_eq!(
                    frame.hit_test(x, y),
                    Some(*target),
                    "{target:?} in {what} is covered by something else"
                );
            }
        }
        let mut help = App::new();
        help.handle_event(&Event::Key(probe::press(Key::F1)));
        for (target, _) in help.frame().hits() {
            kinds.insert(probe::variant_name(*target));
        }
        for kind in [
            "Tab",
            "Flag",
            "MatchPrev",
            "MatchNext",
            "ReplaceToggle",
            "SavePattern",
            "PatternField",
            "ReplaceField",
            "InputArea",
            "ResultArea",
            "ResultTab",
            "GroupsInline",
            "MatchRow",
            "ResultsBody",
            "Chip",
            "LibraryRow",
            "LibraryUse",
            "LibraryDelete",
            "LibraryBody",
            "ReferenceBody",
            "SaveName",
            "SaveConfirm",
            "SaveCancel",
            "ModalBackdrop",
            "HelpCard",
        ] {
            assert!(kinds.contains(kind), "no state draws a {kind}: {kinds:?}");
        }
    }

    #[test]
    fn the_tabs_are_buttons() {
        let mut app = App::new();
        for tab in [ActiveTab::Library, ActiveTab::Reference, ActiveTab::Tester] {
            assert!(probe::click(&mut app, Target::Tab(tab)));
            assert_eq!(app.active_tab, tab);
        }
        assert!(
            !probe::click(&mut app, Target::Tab(ActiveTab::Tester)),
            "the tab already showing"
        );
    }

    #[test]
    fn the_flag_buttons_toggle_their_flags() {
        let mut app = testing("A", "aA");
        assert_eq!(app.matches.len(), 1);
        probe::click(&mut app, Target::Flag(Flag::CaseInsensitive));
        assert!(app.flags.case_insensitive);
        assert_eq!(app.matches.len(), 2, "the match list was not run again");
        probe::click(&mut app, Target::Flag(Flag::Global));
        assert!(!app.flags.global);
        assert_eq!(app.matches.len(), 1);
        probe::click(&mut app, Target::Flag(Flag::Multiline));
        assert!(app.flags.multiline);
    }

    #[test]
    fn the_match_buttons_and_rows_step_through_the_matches() {
        let mut app = testing("a", "banana");
        assert_eq!(app.matches.len(), 3);
        assert!(probe::click(&mut app, Target::MatchNext));
        assert_eq!(app.current_match_index, 1);
        assert!(probe::click(&mut app, Target::MatchPrev));
        assert_eq!(app.current_match_index, 0);
        assert!(probe::click(&mut app, Target::MatchRow(2)));
        assert_eq!(app.current_match_index, 2);
        assert!(
            !probe::click(&mut app, Target::MatchRow(2)),
            "the row already current"
        );
    }

    #[test]
    fn stepping_through_matches_brings_the_current_one_into_both_views() {
        let lines: Vec<String> = (0..100).map(|i| format!("line {i} x")).collect();
        let mut app = testing("x", &lines.join("\n"));
        assert_eq!(app.matches.len(), 100);
        for _ in 0..60 {
            app.handle_event(&Event::Key(probe::press(Key::F3)));
        }
        assert_eq!(app.current_match_index, 60);
        assert!(
            probe::is_visible(&app, Target::MatchRow(60)),
            "the current match is off the bottom of the list"
        );
        let rows = app.input_rows();
        assert!(
            (app.input_scroll..app.input_scroll + rows).contains(&60),
            "line 60 is not in view: the input shows from {} for {rows}",
            app.input_scroll
        );
        app.handle_event(&Event::Key(probe::shift(Key::F3)));
        assert_eq!(app.current_match_index, 59);
    }

    #[test]
    fn a_press_in_a_field_gives_it_the_keyboard_and_puts_the_caret_there() {
        let mut app = testing("abcdef", "");
        app.active_field = ActiveField::Input;
        let rect = probe::rect_of(&app, Target::PatternField).unwrap();
        let x = rect.x + 8.0 + text::measure("abc", NORMAL_TEXT, FontWeightHint::Regular);
        app.handle_event(&mouse(
            x,
            rect.centre().1,
            MouseEventKind::Press(MouseButton::Left),
        ));
        assert_eq!(app.active_field, ActiveField::Pattern);
        assert_eq!(app.pattern.cursor().byte, 3);
        // And what is typed goes there, in the middle.
        probe::type_str(&mut app, "X");
        assert_eq!(app.pattern.text(), "abcXdef");
    }

    #[test]
    fn the_test_input_takes_new_lines_and_can_be_edited_anywhere() {
        let mut app = App::new();
        probe::click(&mut app, Target::InputArea);
        assert_eq!(app.active_field, ActiveField::Input);
        probe::type_str(&mut app, "ab");
        assert!(app.handle_event(&Event::Key(probe::press(Key::Enter))));
        probe::type_str(&mut app, "cd");
        assert_eq!(app.input.text(), "ab\ncd");
        app.handle_event(&Event::Key(probe::press(Key::Up)));
        probe::type_str(&mut app, "X");
        assert_eq!(app.input.text(), "abX\ncd", "Up did not keep the column");
        app.handle_event(&Event::Key(probe::press(Key::Home)));
        probe::type_str(&mut app, "Y");
        assert_eq!(app.input.text(), "YabX\ncd");
        app.handle_event(&Event::Key(probe::press(Key::Down)));
        app.handle_event(&Event::Key(probe::press(Key::End)));
        app.handle_event(&Event::Key(probe::press(Key::Backspace)));
        assert_eq!(app.input.text(), "YabX\nc");
        app.handle_event(&Event::Key(probe::ctrl(Key::Home)));
        app.handle_event(&Event::Key(probe::press(Key::Delete)));
        assert_eq!(app.input.text(), "abX\nc");
    }

    /// The multiline flag had nothing to act on: no newline could be typed.
    #[test]
    fn multiline_can_be_tried_on_text_typed_in_the_window() {
        let mut app = App::new();
        app.set_pattern("^c");
        probe::click(&mut app, Target::InputArea);
        probe::type_str(&mut app, "ab");
        app.handle_event(&Event::Key(probe::press(Key::Enter)));
        probe::type_str(&mut app, "cd");
        assert!(app.matches.is_empty(), "^ is the start of the text");
        app.handle_event(&Event::Key(probe::ctrl(Key::M)));
        assert_eq!(app.matches.len(), 1, "^ is the start of each line with m");
    }

    #[test]
    fn a_press_in_the_test_input_puts_the_caret_under_it_and_a_drag_selects() {
        let mut app = App::new();
        app.set_input("first line\nsecond line\nthird");
        let area = app.tester_layout().input_text();
        let at = |line: f32, prefix: &str| {
            (
                area.x + text::measure(prefix, NORMAL_TEXT, FontWeightHint::Regular),
                area.y + line * LINE_HEIGHT + LINE_HEIGHT / 2.0,
            )
        };
        let (x, y) = at(1.0, "sec");
        app.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)));
        assert_eq!(app.active_field, ActiveField::Input);
        assert_eq!(app.input.caret, "first line\nsec".len());
        let (x2, y2) = at(2.0, "th");
        assert!(app.handle_event(&mouse(x2, y2, MouseEventKind::Move)));
        assert_eq!(app.input.selected_text(), "ond line\nth");
        app.handle_event(&mouse(x2, y2, MouseEventKind::Release(MouseButton::Left)));
        assert!(!app.dragging);
        // A move after the release selects nothing more.
        let (x3, y3) = at(0.0, "f");
        app.handle_event(&mouse(x3, y3, MouseEventKind::Move));
        assert_eq!(app.input.selected_text(), "ond line\nth");
    }

    #[test]
    fn copy_and_paste_carry_text_between_the_fields() {
        let mut app = testing("[0-9]+", "");
        app.active_field = ActiveField::Pattern;
        app.handle_event(&Event::Key(probe::ctrl(Key::A)));
        assert!(app.handle_event(&Event::Key(probe::ctrl(Key::C))));
        app.handle_event(&Event::Key(probe::press(Key::Tab)));
        assert_eq!(app.active_field, ActiveField::Input);
        assert!(app.handle_event(&Event::Key(probe::ctrl(Key::V))));
        assert_eq!(app.input.text(), "[0-9]+");
        // Cut from the input takes the text out, and it pastes back.
        app.handle_event(&Event::Key(probe::ctrl(Key::A)));
        app.handle_event(&Event::Key(probe::ctrl(Key::X)));
        assert_eq!(app.input.text(), "");
        app.handle_event(&Event::Key(probe::ctrl(Key::V)));
        assert_eq!(app.input.text(), "[0-9]+");
    }

    #[test]
    fn the_test_input_scrolls_and_follows_its_caret() {
        let lines: Vec<String> = (0..120).map(|i| format!("row {i}")).collect();
        let mut app = App::new();
        app.set_input(&lines.join("\n"));
        probe::click(&mut app, Target::InputArea);
        app.handle_event(&Event::Key(probe::ctrl(Key::End)));
        let rows = app.input_rows();
        assert!(
            app.input_scroll + rows > 119 && app.input_scroll <= 119,
            "the caret's line is not in view"
        );
        let scrolled = app.input_scroll;
        assert!(probe::scroll_at_point(&mut app, Target::InputArea, 2.0));
        assert!(
            app.input_scroll < scrolled,
            "the wheel did not move the text"
        );
    }

    /// A match's position is counted in characters; the highlight was placed
    /// as if it were in bytes, so after an accented letter it marked the
    /// wrong ones.
    #[test]
    fn the_highlight_covers_what_matched_after_a_wide_character() {
        let app = testing("1", "é1");
        let area = app.tester_layout().input_text();
        let wash = with_alpha(app.palette.peach, 110);
        let highlight = app
            .render_commands()
            .into_iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x, width, color, ..
                } if color == wash => Some((x, width)),
                _ => None,
            })
            .expect("the current match is highlighted");
        let before = text::measure("é", NORMAL_TEXT, FontWeightHint::Regular);
        let one = text::measure("1", NORMAL_TEXT, FontWeightHint::Regular);
        assert!(
            (highlight.0 - (area.x + before)).abs() < 0.5,
            "the band starts at {} and the 1 at {}",
            highlight.0,
            area.x + before
        );
        assert!((highlight.1 - one).abs() < 0.5);
    }

    /// The replacement's result was drawn with the input's highlights on it,
    /// at the input's positions -- over whatever the result had there.
    #[test]
    fn the_result_is_not_painted_with_the_inputs_highlights() {
        let mut app = testing("a", "aaa");
        app.toggle_replace();
        app.set_replacement("bb");
        assert_eq!(app.replace_result.as_deref(), Some("bbbbbb"));
        let washes = [
            with_alpha(app.palette.peach, 110),
            with_alpha(app.palette.blue, 60),
        ];
        let bands = app
            .render_commands()
            .iter()
            .filter(
                |c| matches!(c, RenderCommand::FillRect { color, .. } if washes.contains(color)),
            )
            .count();
        assert_eq!(bands, 3, "one band per match, and only in the input");
    }

    #[test]
    fn the_sub_tabs_show_what_they_name() {
        let mut app = testing("(b)(x)?an", "banana");
        assert!(probe::click(
            &mut app,
            Target::ResultTab(ResultsView::Groups)
        ));
        let drawn = texts(&app);
        assert!(
            drawn.iter().any(|t| t.starts_with("$1  \"b\"")),
            "{drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t.starts_with("$2  took no part")),
            "{drawn:?}"
        );
        assert!(probe::click(
            &mut app,
            Target::ResultTab(ResultsView::Explain)
        ));
        let drawn = texts(&app);
        for line in &app.explanations {
            assert!(drawn.contains(line), "{line:?} is not drawn");
        }
        assert!(probe::click(
            &mut app,
            Target::ResultTab(ResultsView::Matches)
        ));
        assert!(probe::is_visible(&app, Target::MatchRow(0)));
    }

    /// The breakdown was cut at six lines; all of it can be read now.
    #[test]
    fn a_long_explanation_can_be_read_to_its_end() {
        // Forty different letters, so no two lines of the breakdown are the
        // same and the last one is told apart from the rest.
        let pattern = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN";
        let mut app = testing(pattern, "");
        assert!(app.explanations.len() > app.results_rows(ResultsView::Explain));
        probe::click(&mut app, Target::ResultTab(ResultsView::Explain));
        let last = app.explanations.last().unwrap().clone();
        assert!(!texts(&app).contains(&last));
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::ResultsBody, -1.0);
        }
        assert!(
            texts(&app).contains(&last),
            "the end of the breakdown is out of reach"
        );
    }

    #[test]
    fn the_groups_button_shows_and_hides_the_groups_in_the_list() {
        let mut app = testing("(n)a", "banana");
        assert!(texts(&app).iter().any(|t| t.contains("$1=\"n\"")));
        assert!(probe::click(&mut app, Target::GroupsInline));
        assert!(!app.show_groups);
        assert!(!texts(&app).iter().any(|t| t.contains("$1=\"n\"")));
    }

    #[test]
    fn the_replace_button_shows_the_replacement_and_its_result() {
        let mut app = testing("a", "banana");
        assert!(!probe::is_visible(&app, Target::ReplaceField));
        assert!(probe::click(&mut app, Target::ReplaceToggle));
        assert!(app.show_replace);
        probe::click(&mut app, Target::ReplaceField);
        assert_eq!(app.active_field, ActiveField::Replace);
        probe::type_str(&mut app, "o");
        assert_eq!(app.replace_result.as_deref(), Some("bonono"));
        assert!(texts(&app).contains(&"bonono".to_string()));
    }

    #[test]
    fn a_long_result_scrolls() {
        let lines: Vec<String> = (0..20).map(|i| format!("r{i}")).collect();
        let mut app = testing("r", &lines.join("\n"));
        app.toggle_replace();
        app.set_replacement("s");
        assert!(probe::scroll_at_point(&mut app, Target::ResultArea, -1.0));
        assert!(app.result_scroll > 0);
    }

    #[test]
    fn a_library_pattern_can_be_used_by_its_button_by_a_double_press_or_by_enter() {
        let mut app = App::new();
        probe::click(&mut app, Target::Tab(ActiveTab::Library));
        assert!(probe::click(&mut app, Target::LibraryUse(0)));
        assert_eq!(app.active_tab, ActiveTab::Tester);
        assert_eq!(app.pattern.text(), app.library[0].pattern);

        probe::click(&mut app, Target::Tab(ActiveTab::Library));
        let (x, y) = probe::rect_of(&app, Target::LibraryRow(1))
            .unwrap()
            .centre();
        app.handle_event(&mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left)));
        assert_eq!(app.pattern.text(), app.library[1].pattern);

        probe::click(&mut app, Target::Tab(ActiveTab::Library));
        probe::click(&mut app, Target::LibraryRow(2));
        assert_eq!(app.selected_library_entry, Some(2));
        app.handle_event(&Event::Key(probe::press(Key::Enter)));
        assert_eq!(app.pattern.text(), app.library[2].pattern);
        assert_eq!(app.active_tab, ActiveTab::Tester);
    }

    #[test]
    fn the_chips_filter_the_library_and_the_list_scrolls_to_its_end() {
        let mut app = App::new();
        app.active_tab = ActiveTab::Library;
        let network = LIBRARY_FILTERS
            .iter()
            .position(|c| *c == Some(PatternCategory::Network))
            .unwrap();
        assert!(probe::click(&mut app, Target::Chip(network)));
        assert!(
            app.visible_library()
                .iter()
                .all(|(_, e)| e.category == PatternCategory::Network)
        );
        probe::click(&mut app, Target::Chip(0));
        let last = app.library.len() - 1;
        assert!(
            !probe::is_visible(&app, Target::LibraryRow(last)),
            "the whole library fits; nothing to scroll"
        );
        for _ in 0..20 {
            probe::scroll_at_point(&mut app, Target::LibraryBody, -1.0);
        }
        assert!(
            probe::is_visible(&app, Target::LibraryRow(last)),
            "the end of the library is out of reach"
        );
    }

    #[test]
    fn a_pattern_saved_to_the_library_is_kept_with_its_flags() {
        settingsfile::testing::with_scratch_config("rt_saved", |_| {
            let mut app = testing("\\d+", "a1 b22");
            app.toggle_flag(Flag::CaseInsensitive);
            app.toggle_flag(Flag::Multiline);
            assert!(probe::click(&mut app, Target::SavePattern));
            assert!(probe::is_visible(&app, Target::SaveName));
            probe::type_str(&mut app, "digits");
            assert!(probe::click(&mut app, Target::SaveConfirm));
            assert!(app.save_name.is_none());
            assert!(app.status.starts_with("Saved digits"), "{}", app.status);

            // A new window finds it, and using it brings its flags back.
            let mut next = App::new();
            next.load_library(&settingsfile::load(CONFIG_NAME));
            let index = next
                .library
                .iter()
                .position(|e| e.name == "digits")
                .expect("the saved pattern is gone");
            assert_eq!(next.library[index].category, PatternCategory::Custom);
            assert!(!next.flags.case_insensitive);
            next.use_library_entry(index);
            assert_eq!(next.pattern.text(), "\\d+");
            assert!(next.flags.case_insensitive && next.flags.multiline);
        });
    }

    #[test]
    fn saving_under_a_saved_name_replaces_it() {
        settingsfile::testing::with_scratch_config("rt_resave", |_| {
            let mut app = testing("a+", "");
            let before = app.library.len();
            app.ask_to_save();
            probe::type_str(&mut app, "mine");
            app.handle_event(&Event::Key(probe::press(Key::Enter)));
            app.set_pattern("b+");
            app.ask_to_save();
            probe::type_str(&mut app, "mine");
            app.handle_event(&Event::Key(probe::press(Key::Enter)));
            assert_eq!(app.library.len(), before + 1, "a second entry of one name");
            let mut next = App::new();
            next.load_library(&settingsfile::load(CONFIG_NAME));
            let mine: Vec<&PatternEntry> =
                next.library.iter().filter(|e| e.name == "mine").collect();
            assert_eq!(mine.len(), 1);
            assert_eq!(mine[0].pattern, "b+");
        });
    }

    #[test]
    fn a_saved_pattern_can_be_deleted_and_stays_deleted() {
        settingsfile::testing::with_scratch_config("rt_delete", |_| {
            let mut app = testing("z+", "");
            app.ask_to_save();
            probe::type_str(&mut app, "gone");
            app.confirm_save();
            let index = app.library.iter().position(|e| e.name == "gone").unwrap();
            app.active_tab = ActiveTab::Library;
            app.library_category_filter = Some(PatternCategory::Custom);
            assert!(probe::click(&mut app, Target::LibraryDelete(index)));
            assert!(!app.library.iter().any(|e| e.name == "gone"));
            let mut next = App::new();
            next.load_library(&settingsfile::load(CONFIG_NAME));
            assert!(!next.library.iter().any(|e| e.name == "gone"));
            // The Delete key does the same to the one selected.
            app.set_pattern("y+");
            app.ask_to_save();
            probe::type_str(&mut app, "keyed");
            app.confirm_save();
            app.active_tab = ActiveTab::Library;
            let keyed = app.library.iter().position(|e| e.name == "keyed").unwrap();
            app.selected_library_entry = Some(keyed);
            assert!(app.handle_event(&Event::Key(probe::press(Key::Delete))));
            assert!(!app.library.iter().any(|e| e.name == "keyed"));
            // The built-in patterns have no Delete to press.
            app.library_category_filter = None;
            assert!(!probe::is_visible(&app, Target::LibraryDelete(0)));
            assert!(!app.delete_library_entry(0));
        });
    }

    #[test]
    fn the_save_dialog_is_modal_and_cancel_saves_nothing() {
        settingsfile::testing::with_scratch_config("rt_modal", |_| {
            let mut app = testing("a", "");
            let library_tab = probe::rect_of(&app, Target::Tab(ActiveTab::Library))
                .unwrap()
                .centre();
            probe::click(&mut app, Target::SavePattern);
            app.handle_event(&mouse(
                library_tab.0,
                library_tab.1,
                MouseEventKind::Press(MouseButton::Left),
            ));
            assert_eq!(
                app.active_tab,
                ActiveTab::Tester,
                "a press went through the dialog"
            );
            probe::type_str(&mut app, "never");
            assert!(probe::click(&mut app, Target::SaveCancel));
            assert!(app.save_name.is_none());
            assert!(!app.library.iter().any(|e| e.name == "never"));
            // Escape too.
            app.ask_to_save();
            app.handle_event(&Event::Key(probe::press(Key::Escape)));
            assert!(app.save_name.is_none());
        });
    }

    #[test]
    fn the_library_says_when_it_is_full() {
        settingsfile::testing::with_scratch_config("rt_full", |_| {
            let mut app = testing("q", "");
            while app.library.len() < MAX_LIBRARY_ENTRIES {
                let n = app.library.len();
                app.library
                    .push(custom_entry(&format!("p{n}"), "x", RegexFlags::default()));
            }
            app.ask_to_save();
            probe::type_str(&mut app, "one more");
            app.confirm_save();
            assert_eq!(app.library.len(), MAX_LIBRARY_ENTRIES);
            assert!(
                app.save_error
                    .as_deref()
                    .is_some_and(|e| e.contains("full")),
                "{:?}",
                app.save_error
            );
        });
    }

    /// A saved entry this version cannot read is the user's, and a save of
    /// another entry leaves it in the file.
    #[test]
    fn a_saved_entry_that_cannot_be_read_is_left_alone() {
        settingsfile::testing::with_scratch_config("rt_foreign", |_| {
            let mut doc = yamldoc::Document::new();
            doc.set_str(&[LIBRARY_KEY, "odd", "note"], "no pattern here");
            settingsfile::store(CONFIG_NAME, &doc).unwrap();
            let mut app = testing("k", "");
            app.load_library(&settingsfile::load(CONFIG_NAME));
            assert!(!app.library.iter().any(|e| e.name == "odd"));
            app.ask_to_save();
            probe::type_str(&mut app, "kept");
            app.confirm_save();
            let doc = settingsfile::load(CONFIG_NAME);
            assert_eq!(
                doc.get_str(&[LIBRARY_KEY, "odd", "note"]).as_deref(),
                Some("no pattern here")
            );
            assert_eq!(
                doc.get_str(&[LIBRARY_KEY, "kept", "pattern"]).as_deref(),
                Some("k")
            );
        });
    }

    /// Typing reached the tester's fields from any tab, so letters typed in
    /// the library went into a pattern nobody could see.
    #[test]
    fn typing_in_the_library_or_the_reference_goes_nowhere() {
        let mut app = testing("a", "banana");
        for tab in [ActiveTab::Library, ActiveTab::Reference] {
            app.active_tab = tab;
            assert!(!app.handle_event(&Event::Key(probe::typing("x"))));
            assert_eq!(app.pattern.text(), "a");
        }
    }

    #[test]
    fn the_reference_scrolls_in_a_short_window() {
        let mut app = App::new();
        app.active_tab = ActiveTab::Reference;
        app.handle_event(&Event::Resize {
            width: 900,
            height: 300,
        });
        let (x, y) = app
            .frame()
            .rect_of(|t| *t == Target::ReferenceBody)
            .unwrap()
            .centre();
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 })));
        assert!(app.reference_scroll > 0);
        assert!(app.handle_event(&Event::Key(probe::press(Key::Up))));
    }

    #[test]
    fn the_pointer_lights_what_it_is_over_and_the_status_bar_says_what_it_does() {
        let mut app = App::new();
        let (x, y) = probe::rect_of(&app, Target::Flag(Flag::Global))
            .unwrap()
            .centre();
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Move)));
        assert_eq!(app.hover, Some(Target::Flag(Flag::Global)));
        assert!(texts(&app).contains(&Flag::Global.tip().to_string()));
        assert!(!app.handle_event(&mouse(x, y, MouseEventKind::Move)));
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Leave)));
        assert_eq!(app.hover, None);
    }

    #[test]
    fn a_press_puts_the_shortcut_card_away_and_reaches_nothing_under_it() {
        let mut app = App::new();
        let (x, y) = probe::rect_of(&app, Target::Tab(ActiveTab::Library))
            .unwrap()
            .centre();
        app.handle_event(&Event::Key(probe::press(Key::F1)));
        assert_eq!(app.frame().hit_test(x, y), Some(Target::HelpCard));
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left))));
        assert!(!app.show_help);
        assert_eq!(
            app.active_tab,
            ActiveTab::Tester,
            "the press reached the tab"
        );
    }

    #[test]
    fn the_toolbar_is_laid_out_at_the_size_it_is_given() {
        let mut app = App::new();
        app.handle_event(&Event::Resize {
            width: 1500,
            height: 900,
        });
        let next = app.frame().rect_of(|t| *t == Target::MatchNext).unwrap();
        assert!(
            (next.right() - (1500.0 - PADDING)).abs() < 0.5,
            "{next:?} is not at the right edge"
        );
    }

    #[test]
    fn a_chord_nobody_bound_types_nothing() {
        let mut app = testing("a", "");
        let mut chord = probe::ctrl(Key::D);
        chord.text = "d".to_string();
        assert!(!app.handle_event(&Event::Key(chord)));
        assert_eq!(app.pattern.text(), "a");
    }

    #[test]
    fn enter_and_the_arrows_step_through_matches_from_the_pattern() {
        let mut app = testing("a", "banana");
        app.active_field = ActiveField::Pattern;
        assert!(app.handle_event(&Event::Key(probe::press(Key::Enter))));
        assert_eq!(app.current_match_index, 1);
        app.handle_event(&Event::Key(probe::press(Key::Down)));
        assert_eq!(app.current_match_index, 2);
        app.handle_event(&Event::Key(probe::press(Key::Up)));
        assert_eq!(app.current_match_index, 1);
    }

    #[test]
    fn a_match_that_spans_lines_is_shown_with_its_break() {
        let app = testing("a\\nb", "a\nb");
        assert_eq!(app.matches.len(), 1);
        assert!(texts(&app).contains(&"\"a\\nb\"".to_string()));
    }
}
