//! SlateOS `lex` -- lexical analyzer generator (lex/flex compatible)
//!
//! Multi-personality binary: detected via argv\[0\] as either `lex` or `flex`.
//! Reads a lex specification file (definitions, rules, user code) and generates
//! C source code for a scanner.
//!
//! Internals:
//!   1. Parse the `.l` specification into definitions, rules, and user code
//!   2. Compile each rule regex into an NFA (Thompson's construction)
//!   3. Combine all rule NFAs with alternation
//!   4. Convert the combined NFA to a DFA (subset construction)
//!   5. Minimize the DFA (Hopcroft's algorithm)
//!   6. Emit C source implementing the scanner table-driven automaton

#![cfg_attr(not(test), no_main)]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
#[cfg(not(test))]
use std::env;
use std::fmt::Write as FmtWrite;
#[cfg(not(test))]
use std::fs;
#[cfg(not(test))]
use std::io::{self, Write as IoWrite};

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Lex,
    Flex,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit(&['/', '\\']).next().unwrap_or(argv0);
    let lower = base.to_ascii_lowercase();
    if lower.starts_with("flex") {
        Personality::Flex
    } else {
        Personality::Lex
    }
}

// ---------------------------------------------------------------------------
// Options parsed from %option directives and CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Options {
    noyywrap: bool,
    yylineno: bool,
    case_insensitive: bool,
    prefix: String,
    output_file: Option<String>,
    header_file: Option<String>,
    debug: bool,
}

impl Options {
    fn new() -> Self {
        Self {
            noyywrap: false,
            yylineno: false,
            case_insensitive: false,
            prefix: "yy".into(),
            output_file: None,
            header_file: None,
            debug: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Spec parser: definitions, rules, user code
// ---------------------------------------------------------------------------

/// A start condition declaration.
#[derive(Debug, Clone)]
struct StartCondition {
    name: String,
    exclusive: bool,
}

/// A single rule: optional start conditions, regex pattern, action code.
#[derive(Debug, Clone)]
struct Rule {
    start_conditions: Vec<String>,
    pattern: String,
    action: String,
}

/// Named definition from the definitions section.
#[derive(Debug, Clone)]
struct Definition {
    name: String,
    expansion: String,
}

/// Parsed lex specification.
#[derive(Debug, Clone)]
struct LexSpec {
    definitions: Vec<Definition>,
    start_conditions: Vec<StartCondition>,
    option_directives: Vec<(String, String)>,
    top_code: String,
    rules: Vec<Rule>,
    user_code: String,
}

/// Parse a lex specification from its text content.
fn parse_spec(input: &str) -> Result<LexSpec, String> {
    let mut definitions = Vec::new();
    let mut start_conditions = Vec::new();
    let mut option_directives = Vec::new();
    let mut top_code = String::new();
    let mut rules = Vec::new();
    let mut user_code = String::new();

    let lines: Vec<&str> = input.lines().collect();
    let mut idx = 0;
    let line_count = lines.len();

    // -- Section 1: Definitions --
    while idx < line_count {
        let line = lines[idx];

        // The %% delimiter starts the rules section.
        if line.trim() == "%%" {
            idx += 1;
            break;
        }

        // %{ ... %} blocks: verbatim C code for the top of the file.
        if line.trim_start().starts_with("%{") {
            idx += 1;
            while idx < line_count {
                if lines[idx].trim_start().starts_with("%}") {
                    idx += 1;
                    break;
                }
                top_code.push_str(lines[idx]);
                top_code.push('\n');
                idx += 1;
            }
            continue;
        }

        // %option directives.
        if line.trim_start().starts_with("%option") {
            let rest = line.trim_start().strip_prefix("%option").unwrap_or("").trim();
            for opt in rest.split_whitespace() {
                if let Some((k, v)) = opt.split_once('=') {
                    option_directives.push((k.to_string(), v.trim_matches('"').to_string()));
                } else {
                    option_directives.push((opt.to_string(), String::new()));
                }
            }
            idx += 1;
            continue;
        }

        // %s / %x start conditions.
        if line.trim_start().starts_with("%s ") || line.trim_start().starts_with("%s\t") {
            let rest = line.trim_start().strip_prefix("%s").unwrap_or("").trim();
            for name in rest.split_whitespace() {
                start_conditions.push(StartCondition {
                    name: name.to_string(),
                    exclusive: false,
                });
            }
            idx += 1;
            continue;
        }
        if line.trim_start().starts_with("%x ") || line.trim_start().starts_with("%x\t") {
            let rest = line.trim_start().strip_prefix("%x").unwrap_or("").trim();
            for name in rest.split_whitespace() {
                start_conditions.push(StartCondition {
                    name: name.to_string(),
                    exclusive: true,
                });
            }
            idx += 1;
            continue;
        }

        // Named definition: NAME<whitespace>EXPANSION
        // Must start at column 0 with an alpha/underscore character.
        let bytes = line.as_bytes();
        if !bytes.is_empty()
            && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
            && !line.starts_with("%%")
        {
            // Find the separator: one or more whitespace chars.
            if let Some(pos) = line.find([' ', '\t']) {
                let name = line[..pos].to_string();
                let expansion = line[pos..].trim().to_string();
                definitions.push(Definition { name, expansion });
            }
            idx += 1;
            continue;
        }

        // Indented lines in the definitions section are verbatim code.
        if !bytes.is_empty() && (bytes[0] == b' ' || bytes[0] == b'\t') {
            top_code.push_str(line.trim_start());
            top_code.push('\n');
            idx += 1;
            continue;
        }

        // Blank or unrecognised -- skip.
        idx += 1;
    }

    // -- Section 2: Rules --
    while idx < line_count {
        let line = lines[idx];

        // Second %% starts user code section.
        if line.trim() == "%%" {
            idx += 1;
            break;
        }

        // Skip blank lines.
        if line.trim().is_empty() {
            idx += 1;
            continue;
        }

        // Indented lines are verbatim C in the rules section (local declarations).
        let bytes = line.as_bytes();
        if !bytes.is_empty() && (bytes[0] == b' ' || bytes[0] == b'\t') {
            // This is treated as action code for the preceding rule, or a
            // local declaration block. We skip it for simplicity here; real
            // lex would attach it to the previous rule or emit it in the
            // action switch.
            idx += 1;
            continue;
        }

        // Parse a rule: [<SC1,SC2>]pattern  action
        let (scs, pat, action, new_idx) = parse_rule(&lines, idx)?;
        rules.push(Rule {
            start_conditions: scs,
            pattern: pat,
            action,
        });
        idx = new_idx;
    }

    // -- Section 3: User code --
    while idx < line_count {
        user_code.push_str(lines[idx]);
        user_code.push('\n');
        idx += 1;
    }

    Ok(LexSpec {
        definitions,
        start_conditions,
        option_directives,
        top_code,
        rules,
        user_code,
    })
}

/// Parse a single rule from the rules section.
/// Returns (start_conditions, pattern, action, next_line_index).
fn parse_rule(lines: &[&str], start: usize) -> Result<(Vec<String>, String, String, usize), String> {
    let line = lines[start];
    let mut pos = 0;
    let bytes = line.as_bytes();
    let mut scs = Vec::new();

    // Optional start conditions: <SC1,SC2>
    if pos < bytes.len() && bytes[pos] == b'<' {
        pos += 1;
        let close = line[pos..]
            .find('>')
            .ok_or_else(|| format!("unterminated start condition on line {}", start + 1))?;
        let sc_str = &line[pos..pos + close];
        for sc in sc_str.split(',') {
            let trimmed = sc.trim();
            if !trimmed.is_empty() {
                scs.push(trimmed.to_string());
            }
        }
        pos += close + 1;
    }

    // Pattern: read until unquoted whitespace (handling char classes, quotes, escapes).
    let pattern_start = pos;
    let mut in_class = false;
    let mut in_quotes = false;
    let mut escaped = false;

    while pos < bytes.len() {
        if escaped {
            escaped = false;
            pos += 1;
            continue;
        }
        match bytes[pos] {
            b'\\' => {
                escaped = true;
                pos += 1;
            }
            b'"' => {
                in_quotes = !in_quotes;
                pos += 1;
            }
            b'[' if !in_quotes => {
                in_class = true;
                pos += 1;
            }
            b']' if !in_quotes && in_class => {
                in_class = false;
                pos += 1;
            }
            b' ' | b'\t' if !in_class && !in_quotes => break,
            _ => {
                pos += 1;
            }
        }
    }
    let pattern = line[pattern_start..pos].to_string();

    // Skip whitespace between pattern and action.
    while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
        pos += 1;
    }

    // Action: rest of the line, potentially multi-line with braces.
    let action_start = pos;
    let action_first = &line[action_start..];

    // If the action starts with '{', we need to find the matching '}'.
    if action_first.starts_with('{') {
        let mut brace_depth = 0i32;
        let mut action_text = String::new();
        let mut lidx = start;

        // Count braces in the remainder of the first line, starting from `action_start`.
        let first_part = &line[action_start..];
        for ch in first_part.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }
        action_text.push_str(first_part);
        lidx += 1;

        while brace_depth > 0 && lidx < lines.len() {
            action_text.push('\n');
            action_text.push_str(lines[lidx]);
            for ch in lines[lidx].chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            lidx += 1;
        }

        Ok((scs, pattern, action_text, lidx))
    } else if action_first.starts_with('|') {
        // '|' means fall through to next rule's action.
        Ok((scs, pattern, "|".into(), start + 1))
    } else {
        Ok((scs, pattern, action_first.to_string(), start + 1))
    }
}

// ---------------------------------------------------------------------------
// Regex AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegexAst {
    Literal(u8),
    AnyChar,
    CharClass {
        ranges: Vec<(u8, u8)>,
        singles: Vec<u8>,
        negated: bool,
    },
    Concat(Vec<RegexAst>),
    Alternation(Vec<RegexAst>),
    Star(Box<RegexAst>),
    Plus(Box<RegexAst>),
    Question(Box<RegexAst>),
    /// Start-of-line anchor.
    StartAnchor,
    /// End-of-line anchor.
    EndAnchor,
}

// ---------------------------------------------------------------------------
// Regex parser: pattern string -> RegexAst
// ---------------------------------------------------------------------------

struct RegexParser<'a> {
    input: &'a [u8],
    pos: usize,
    definitions: &'a HashMap<String, String>,
    case_insensitive: bool,
}

impl<'a> RegexParser<'a> {
    fn new(
        input: &'a [u8],
        definitions: &'a HashMap<String, String>,
        case_insensitive: bool,
    ) -> Self {
        Self {
            input,
            pos: 0,
            definitions,
            case_insensitive,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn parse(&mut self) -> Result<RegexAst, String> {
        let ast = self.parse_alternation()?;
        Ok(ast)
    }

    fn parse_alternation(&mut self) -> Result<RegexAst, String> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek() == Some(b'|') {
            self.advance();
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.remove(0))
        } else {
            Ok(RegexAst::Alternation(branches))
        }
    }

    fn parse_concat(&mut self) -> Result<RegexAst, String> {
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == b'|' || ch == b')' {
                break;
            }
            items.push(self.parse_quantified()?);
        }
        if items.len() == 1 {
            Ok(items.remove(0))
        } else if items.is_empty() {
            // Empty pattern matches empty string -- represent as empty concat.
            Ok(RegexAst::Concat(Vec::new()))
        } else {
            Ok(RegexAst::Concat(items))
        }
    }

    fn parse_quantified(&mut self) -> Result<RegexAst, String> {
        let base = self.parse_atom()?;
        match self.peek() {
            Some(b'*') => {
                self.advance();
                Ok(RegexAst::Star(Box::new(base)))
            }
            Some(b'+') => {
                self.advance();
                Ok(RegexAst::Plus(Box::new(base)))
            }
            Some(b'?') => {
                self.advance();
                Ok(RegexAst::Question(Box::new(base)))
            }
            _ => Ok(base),
        }
    }

    fn parse_atom(&mut self) -> Result<RegexAst, String> {
        match self.peek() {
            Some(b'(') => {
                self.advance();
                let inner = self.parse_alternation()?;
                if self.peek() == Some(b')') {
                    self.advance();
                } else {
                    return Err("unmatched '(' in regex".into());
                }
                Ok(inner)
            }
            Some(b'[') => self.parse_char_class(),
            Some(b'.') => {
                self.advance();
                Ok(RegexAst::AnyChar)
            }
            Some(b'^') => {
                self.advance();
                Ok(RegexAst::StartAnchor)
            }
            Some(b'$') => {
                self.advance();
                Ok(RegexAst::EndAnchor)
            }
            Some(b'"') => self.parse_quoted_literal(),
            Some(b'{') => self.parse_definition_ref(),
            Some(b'\\') => self.parse_escape(),
            Some(_) => {
                let ch = self.advance().unwrap_or(0);
                Ok(self.make_literal(ch))
            }
            None => Err("unexpected end of regex".into()),
        }
    }

    fn make_literal(&self, ch: u8) -> RegexAst {
        if self.case_insensitive && ch.is_ascii_alphabetic() {
            let lo = ch.to_ascii_lowercase();
            let hi = ch.to_ascii_uppercase();
            RegexAst::CharClass {
                ranges: Vec::new(),
                singles: vec![lo, hi],
                negated: false,
            }
        } else {
            RegexAst::Literal(ch)
        }
    }

    fn parse_quoted_literal(&mut self) -> Result<RegexAst, String> {
        self.advance(); // skip opening '"'
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == b'"' {
                self.advance();
                break;
            }
            let c = self.advance().unwrap_or(0);
            items.push(self.make_literal(c));
        }
        if items.len() == 1 {
            Ok(items.remove(0))
        } else {
            Ok(RegexAst::Concat(items))
        }
    }

    fn parse_definition_ref(&mut self) -> Result<RegexAst, String> {
        self.advance(); // skip '{'
        let start_pos = self.pos;
        while let Some(ch) = self.peek() {
            if ch == b'}' {
                break;
            }
            self.advance();
        }
        let name_bytes = &self.input[start_pos..self.pos];
        let name = String::from_utf8_lossy(name_bytes).to_string();
        if self.peek() == Some(b'}') {
            self.advance();
        }
        if let Some(expansion) = self.definitions.get(&name) {
            let mut sub = RegexParser::new(
                expansion.as_bytes(),
                self.definitions,
                self.case_insensitive,
            );
            sub.parse()
        } else {
            Err(format!("undefined name '{}' in regex", name))
        }
    }

    fn parse_escape(&mut self) -> Result<RegexAst, String> {
        self.advance(); // skip '\'
        match self.advance() {
            Some(b'n') => Ok(RegexAst::Literal(b'\n')),
            Some(b't') => Ok(RegexAst::Literal(b'\t')),
            Some(b'r') => Ok(RegexAst::Literal(b'\r')),
            Some(b'a') => Ok(RegexAst::Literal(0x07)),
            Some(b'b') => Ok(RegexAst::Literal(0x08)),
            Some(b'f') => Ok(RegexAst::Literal(0x0C)),
            Some(b'0') => Ok(RegexAst::Literal(0)),
            Some(ch) => Ok(RegexAst::Literal(ch)),
            None => Err("trailing backslash in regex".into()),
        }
    }

    fn parse_char_class(&mut self) -> Result<RegexAst, String> {
        self.advance(); // skip '['
        let negated = if self.peek() == Some(b'^') {
            self.advance();
            true
        } else {
            false
        };

        let mut ranges = Vec::new();
        let mut singles = Vec::new();

        // Handle ']' or '-' as first character (literal).
        if self.peek() == Some(b']') {
            singles.push(b']');
            self.advance();
        }

        while let Some(ch) = self.peek() {
            if ch == b']' {
                self.advance();
                return Ok(RegexAst::CharClass {
                    ranges,
                    singles,
                    negated,
                });
            }
            if ch == b'\\' {
                self.advance();
                let esc = self.advance().ok_or("unterminated escape in char class")?;
                let literal = match esc {
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'r' => b'\r',
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0C,
                    other => other,
                };
                // Check for range.
                if self.peek() == Some(b'-') && self.input.get(self.pos + 1) != Some(&b']') {
                    self.advance(); // skip '-'
                    let hi = self.parse_class_char()?;
                    ranges.push((literal, hi));
                } else {
                    singles.push(literal);
                }
            } else {
                let lo = self.advance().unwrap_or(0);
                if self.peek() == Some(b'-') && self.input.get(self.pos + 1) != Some(&b']') {
                    self.advance(); // skip '-'
                    let hi = self.parse_class_char()?;
                    ranges.push((lo, hi));
                } else {
                    singles.push(lo);
                }
            }
        }

        Err("unterminated character class".into())
    }

    fn parse_class_char(&mut self) -> Result<u8, String> {
        if self.peek() == Some(b'\\') {
            self.advance();
            match self.advance() {
                Some(b'n') => Ok(b'\n'),
                Some(b't') => Ok(b'\t'),
                Some(b'r') => Ok(b'\r'),
                Some(ch) => Ok(ch),
                None => Err("unterminated escape".into()),
            }
        } else {
            self.advance().ok_or_else(|| "unexpected end of char class".into())
        }
    }
}

/// Parse a pattern string into a RegexAst.
fn parse_regex(
    pattern: &str,
    definitions: &HashMap<String, String>,
    case_insensitive: bool,
) -> Result<RegexAst, String> {
    let mut parser = RegexParser::new(pattern.as_bytes(), definitions, case_insensitive);
    parser.parse()
}

// ---------------------------------------------------------------------------
// NFA (Thompson's construction)
// ---------------------------------------------------------------------------

/// Transition label for NFA edges.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NfaLabel {
    Epsilon,
    Byte(u8),
    ByteRange(u8, u8),
    AnyByte,
    /// Matches any byte NOT in the given set.
    NegatedClass(Vec<u8>),
}

#[derive(Debug, Clone)]
struct NfaTransition {
    label: NfaLabel,
    target: usize,
}

#[derive(Debug, Clone)]
struct NfaState {
    transitions: Vec<NfaTransition>,
    /// If this is an accepting state, which rule index it accepts.
    accepting: Option<usize>,
}

#[derive(Debug, Clone)]
struct Nfa {
    states: Vec<NfaState>,
    start: usize,
    accept: usize,
}

impl Nfa {
    fn new_state(&mut self) -> usize {
        let id = self.states.len();
        self.states.push(NfaState {
            transitions: Vec::new(),
            accepting: None,
        });
        id
    }

    fn add_transition(&mut self, from: usize, label: NfaLabel, to: usize) {
        self.states[from].transitions.push(NfaTransition {
            label,
            target: to,
        });
    }
}

fn new_empty_nfa() -> Nfa {
    let mut nfa = Nfa {
        states: Vec::new(),
        start: 0,
        accept: 0,
    };
    let s = nfa.new_state();
    let a = nfa.new_state();
    nfa.start = s;
    nfa.accept = a;
    nfa.add_transition(s, NfaLabel::Epsilon, a);
    nfa
}

/// Flatten a CharClass into all individual bytes it matches.
fn char_class_bytes(ranges: &[(u8, u8)], singles: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for &(lo, hi) in ranges {
        let actual_lo = lo.min(hi);
        let actual_hi = lo.max(hi);
        for b in actual_lo..=actual_hi {
            result.push(b);
        }
    }
    result.extend_from_slice(singles);
    result.sort_unstable();
    result.dedup();
    result
}

/// Build an NFA from a RegexAst using Thompson's construction.
fn build_nfa(ast: &RegexAst) -> Nfa {
    match ast {
        RegexAst::Literal(b) => {
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let s = nfa.new_state();
            let a = nfa.new_state();
            nfa.start = s;
            nfa.accept = a;
            nfa.add_transition(s, NfaLabel::Byte(*b), a);
            nfa
        }
        RegexAst::AnyChar => {
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let s = nfa.new_state();
            let a = nfa.new_state();
            nfa.start = s;
            nfa.accept = a;
            nfa.add_transition(s, NfaLabel::AnyByte, a);
            nfa
        }
        RegexAst::CharClass {
            ranges,
            singles,
            negated,
        } => {
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let s = nfa.new_state();
            let a = nfa.new_state();
            nfa.start = s;
            nfa.accept = a;
            if *negated {
                let included = char_class_bytes(ranges, singles);
                nfa.add_transition(s, NfaLabel::NegatedClass(included), a);
            } else {
                // Add transitions for each range and single byte.
                for &(lo, hi) in ranges {
                    nfa.add_transition(s, NfaLabel::ByteRange(lo.min(hi), lo.max(hi)), a);
                }
                for &b in singles {
                    nfa.add_transition(s, NfaLabel::Byte(b), a);
                }
            }
            nfa
        }
        RegexAst::Concat(items) => {
            if items.is_empty() {
                return new_empty_nfa();
            }
            let mut combined = build_nfa(&items[0]);
            for item in &items[1..] {
                let next = build_nfa(item);
                combined = concat_nfa(combined, next);
            }
            combined
        }
        RegexAst::Alternation(branches) => {
            if branches.is_empty() {
                return new_empty_nfa();
            }
            if branches.len() == 1 {
                return build_nfa(&branches[0]);
            }
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let new_start = nfa.new_state();
            let new_accept = nfa.new_state();
            nfa.start = new_start;
            nfa.accept = new_accept;

            for branch in branches {
                let sub = build_nfa(branch);
                let offset = nfa.states.len();
                for mut state in sub.states {
                    for tr in &mut state.transitions {
                        tr.target += offset;
                    }
                    nfa.states.push(state);
                }
                nfa.add_transition(new_start, NfaLabel::Epsilon, sub.start + offset);
                nfa.add_transition(sub.accept + offset, NfaLabel::Epsilon, new_accept);
            }
            nfa
        }
        RegexAst::Star(inner) => {
            let sub = build_nfa(inner);
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let new_start = nfa.new_state();
            let new_accept = nfa.new_state();
            nfa.start = new_start;
            nfa.accept = new_accept;

            let offset = nfa.states.len();
            for mut state in sub.states {
                for tr in &mut state.transitions {
                    tr.target += offset;
                }
                nfa.states.push(state);
            }
            nfa.add_transition(new_start, NfaLabel::Epsilon, sub.start + offset);
            nfa.add_transition(new_start, NfaLabel::Epsilon, new_accept);
            nfa.add_transition(sub.accept + offset, NfaLabel::Epsilon, sub.start + offset);
            nfa.add_transition(sub.accept + offset, NfaLabel::Epsilon, new_accept);
            nfa
        }
        RegexAst::Plus(inner) => {
            let sub = build_nfa(inner);
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let new_start = nfa.new_state();
            let new_accept = nfa.new_state();
            nfa.start = new_start;
            nfa.accept = new_accept;

            let offset = nfa.states.len();
            for mut state in sub.states {
                for tr in &mut state.transitions {
                    tr.target += offset;
                }
                nfa.states.push(state);
            }
            nfa.add_transition(new_start, NfaLabel::Epsilon, sub.start + offset);
            nfa.add_transition(sub.accept + offset, NfaLabel::Epsilon, sub.start + offset);
            nfa.add_transition(sub.accept + offset, NfaLabel::Epsilon, new_accept);
            nfa
        }
        RegexAst::Question(inner) => {
            let sub = build_nfa(inner);
            let mut nfa = Nfa {
                states: Vec::new(),
                start: 0,
                accept: 0,
            };
            let new_start = nfa.new_state();
            let new_accept = nfa.new_state();
            nfa.start = new_start;
            nfa.accept = new_accept;

            let offset = nfa.states.len();
            for mut state in sub.states {
                for tr in &mut state.transitions {
                    tr.target += offset;
                }
                nfa.states.push(state);
            }
            nfa.add_transition(new_start, NfaLabel::Epsilon, sub.start + offset);
            nfa.add_transition(new_start, NfaLabel::Epsilon, new_accept);
            nfa.add_transition(sub.accept + offset, NfaLabel::Epsilon, new_accept);
            nfa
        }
        RegexAst::StartAnchor | RegexAst::EndAnchor => {
            // Anchors are handled at a higher level; treat as epsilon here.
            new_empty_nfa()
        }
    }
}

/// Concatenate two NFAs by merging accept of first with start of second.
fn concat_nfa(mut first: Nfa, second: Nfa) -> Nfa {
    let offset = first.states.len();
    for mut state in second.states {
        for tr in &mut state.transitions {
            tr.target += offset;
        }
        first.states.push(state);
    }
    first.add_transition(first.accept, NfaLabel::Epsilon, second.start + offset);
    first.accept = second.accept + offset;
    first
}

/// Combine multiple rule NFAs into one with a new start state.
fn combine_rule_nfas(rule_nfas: Vec<(Nfa, usize)>) -> Nfa {
    let mut combined = Nfa {
        states: Vec::new(),
        start: 0,
        accept: 0, // Not meaningful for the combined NFA.
    };
    let new_start = combined.new_state();
    combined.start = new_start;
    // Accept states are per-NFA, marked with rule indices.
    // The `accept` field of the combined NFA is unused.
    combined.accept = new_start;

    for (sub, rule_idx) in rule_nfas {
        let offset = combined.states.len();
        for mut state in sub.states {
            for tr in &mut state.transitions {
                tr.target += offset;
            }
            combined.states.push(state);
        }
        combined.add_transition(new_start, NfaLabel::Epsilon, sub.start + offset);
        combined.states[sub.accept + offset].accepting = Some(rule_idx);
    }

    combined
}

// ---------------------------------------------------------------------------
// NFA -> DFA (subset construction)
// ---------------------------------------------------------------------------

type StateSet = BTreeSet<usize>;

/// Compute the epsilon closure of a set of NFA states.
fn epsilon_closure(nfa: &Nfa, states: &StateSet) -> StateSet {
    let mut closure = states.clone();
    let mut worklist: Vec<usize> = states.iter().copied().collect();
    while let Some(s) = worklist.pop() {
        for tr in &nfa.states[s].transitions {
            if tr.label == NfaLabel::Epsilon && closure.insert(tr.target) {
                worklist.push(tr.target);
            }
        }
    }
    closure
}

/// Compute the set of NFA states reachable from `states` on input byte `b`.
fn move_on_byte(nfa: &Nfa, states: &StateSet, byte: u8) -> StateSet {
    let mut result = BTreeSet::new();
    for &s in states {
        for tr in &nfa.states[s].transitions {
            let matches = match &tr.label {
                NfaLabel::Byte(b2) => *b2 == byte,
                NfaLabel::ByteRange(lo, hi) => byte >= *lo && byte <= *hi,
                NfaLabel::AnyByte => byte != b'\n',
                NfaLabel::NegatedClass(excluded) => {
                    byte != b'\n' && !excluded.contains(&byte)
                }
                NfaLabel::Epsilon => false,
            };
            if matches {
                result.insert(tr.target);
            }
        }
    }
    result
}

#[derive(Debug, Clone)]
struct DfaState {
    transitions: [Option<usize>; 256],
    accepting: Option<usize>,
}

impl DfaState {
    fn new() -> Self {
        Self {
            transitions: [None; 256],
            accepting: None,
        }
    }
}

#[derive(Debug, Clone)]
struct Dfa {
    states: Vec<DfaState>,
    start: usize,
}

/// Convert an NFA to a DFA using subset construction.
fn nfa_to_dfa(nfa: &Nfa) -> Dfa {
    let mut dfa = Dfa {
        states: Vec::new(),
        start: 0,
    };

    let start_set = {
        let mut s = BTreeSet::new();
        s.insert(nfa.start);
        epsilon_closure(nfa, &s)
    };

    let mut state_map: BTreeMap<StateSet, usize> = BTreeMap::new();
    let mut worklist: VecDeque<StateSet> = VecDeque::new();

    let start_id = dfa.states.len();
    dfa.states.push(DfaState::new());
    dfa.start = start_id;
    dfa.states[start_id].accepting = find_accepting(nfa, &start_set);
    state_map.insert(start_set.clone(), start_id);
    worklist.push_back(start_set);

    while let Some(current_set) = worklist.pop_front() {
        let current_id = state_map[&current_set];

        for byte in 0u16..=255u16 {
            let b = byte as u8;
            let moved = move_on_byte(nfa, &current_set, b);
            if moved.is_empty() {
                continue;
            }
            let target_set = epsilon_closure(nfa, &moved);
            if target_set.is_empty() {
                continue;
            }

            let target_id = if let Some(&id) = state_map.get(&target_set) {
                id
            } else {
                let id = dfa.states.len();
                dfa.states.push(DfaState::new());
                dfa.states[id].accepting = find_accepting(nfa, &target_set);
                state_map.insert(target_set.clone(), id);
                worklist.push_back(target_set);
                id
            };

            dfa.states[current_id].transitions[b as usize] = Some(target_id);
        }
    }

    dfa
}

/// Find the lowest-numbered accepting rule in a set of NFA states.
fn find_accepting(nfa: &Nfa, states: &StateSet) -> Option<usize> {
    let mut best: Option<usize> = None;
    for &s in states {
        if let Some(rule) = nfa.states[s].accepting {
            best = Some(match best {
                Some(prev) => prev.min(rule),
                None => rule,
            });
        }
    }
    best
}

// ---------------------------------------------------------------------------
// DFA minimization (Hopcroft's algorithm)
// ---------------------------------------------------------------------------

fn minimize_dfa(dfa: &Dfa) -> Dfa {
    let n = dfa.states.len();
    if n == 0 {
        return dfa.clone();
    }

    // Partition states into groups: one group per accepting rule + one for
    // non-accepting states.
    let mut accepting_groups: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
    for (i, state) in dfa.states.iter().enumerate() {
        accepting_groups
            .entry(state.accepting)
            .or_default()
            .push(i);
    }

    let mut partitions: Vec<BTreeSet<usize>> = accepting_groups
        .into_values()
        .map(|v| v.into_iter().collect())
        .collect();

    // Map each state to its partition index.
    let mut state_to_part = vec![0usize; n];
    for (pi, part) in partitions.iter().enumerate() {
        for &s in part {
            state_to_part[s] = pi;
        }
    }

    // Iteratively refine partitions.
    let mut changed = true;
    while changed {
        changed = false;
        let mut new_partitions: Vec<BTreeSet<usize>> = Vec::new();

        for part in &partitions {
            if part.len() <= 1 {
                new_partitions.push(part.clone());
                continue;
            }

            // Split this partition based on transitions.
            let mut sub_groups: BTreeMap<Vec<Option<usize>>, BTreeSet<usize>> = BTreeMap::new();
            for &s in part {
                let signature: Vec<Option<usize>> = (0..256)
                    .map(|b| {
                        dfa.states[s].transitions[b].map(|t| state_to_part[t])
                    })
                    .collect();
                sub_groups.entry(signature).or_default().insert(s);
            }

            if sub_groups.len() > 1 {
                changed = true;
            }
            for group in sub_groups.into_values() {
                new_partitions.push(group);
            }
        }

        partitions = new_partitions;
        for (pi, part) in partitions.iter().enumerate() {
            for &s in part {
                state_to_part[s] = pi;
            }
        }
    }

    // Build the minimized DFA.
    let mut min_dfa = Dfa {
        states: Vec::new(),
        start: state_to_part[dfa.start],
    };

    for _ in 0..partitions.len() {
        min_dfa.states.push(DfaState::new());
    }

    for (pi, part) in partitions.iter().enumerate() {
        // Use the first state in the partition as the representative.
        let representative = *part.iter().next().unwrap_or(&0);
        min_dfa.states[pi].accepting = dfa.states[representative].accepting;
        for byte in 0..256 {
            if let Some(target) = dfa.states[representative].transitions[byte] {
                min_dfa.states[pi].transitions[byte] = Some(state_to_part[target]);
            }
        }
    }

    min_dfa
}

// ---------------------------------------------------------------------------
// C code generation
// ---------------------------------------------------------------------------

fn generate_scanner(
    spec: &LexSpec,
    dfa: &Dfa,
    rules: &[Rule],
    options: &Options,
) -> String {
    let prefix = &options.prefix;
    let mut out = String::with_capacity(16384);

    // Header comment.
    let _ = writeln!(out, "/* Generated by SlateOS lex */");
    let _ = writeln!(out, "#include <stdio.h>");
    let _ = writeln!(out, "#include <stdlib.h>");
    let _ = writeln!(out, "#include <string.h>");
    let _ = writeln!(out);

    // Top code from spec.
    if !spec.top_code.is_empty() {
        let _ = writeln!(out, "/* User top code */");
        out.push_str(&spec.top_code);
        let _ = writeln!(out);
    }

    // Start condition defines.
    let _ = writeln!(out, "/* Start conditions */");
    let _ = writeln!(out, "#define INITIAL 0");
    for (i, sc) in spec.start_conditions.iter().enumerate() {
        let kind = if sc.exclusive { "exclusive" } else { "inclusive" };
        let _ = writeln!(out, "#define {} {} /* {} */", sc.name, i + 1, kind);
    }
    let _ = writeln!(out);

    // Scanner state.
    let _ = writeln!(out, "/* Scanner state */");
    let _ = writeln!(out, "static int {prefix}_start = INITIAL;");
    let _ = writeln!(out, "#define BEGIN(s) ({prefix}_start = (s))");
    let _ = writeln!(out);
    let _ = writeln!(out, "char *{prefix}text = NULL;");
    let _ = writeln!(out, "int {prefix}leng = 0;");
    if options.yylineno {
        let _ = writeln!(out, "int {prefix}lineno = 1;");
    }
    let _ = writeln!(out, "FILE *{prefix}in = NULL;");
    let _ = writeln!(out, "FILE *{prefix}out = NULL;");
    let _ = writeln!(out);

    // Input buffer.
    let _ = writeln!(out, "/* Input buffering */");
    let _ = writeln!(out, "#define YY_BUF_SIZE 16384");
    let _ = writeln!(out, "struct {prefix}_buffer_state {{");
    let _ = writeln!(out, "    FILE *{prefix}_input_file;");
    let _ = writeln!(out, "    char *{prefix}_ch_buf;");
    let _ = writeln!(out, "    int {prefix}_buf_size;");
    let _ = writeln!(out, "    int {prefix}_buf_pos;");
    let _ = writeln!(out, "    int {prefix}_n_chars;");
    let _ = writeln!(out, "    int {prefix}_eof;");
    let _ = writeln!(out, "}};");
    let _ = writeln!(out, "typedef struct {prefix}_buffer_state *YY_BUFFER_STATE;");
    let _ = writeln!(out, "static YY_BUFFER_STATE {prefix}_current_buffer = NULL;");
    let _ = writeln!(out);

    // Buffer management functions.
    let _ = writeln!(out, "YY_BUFFER_STATE {prefix}_create_buffer(FILE *file, int size) {{");
    let _ = writeln!(out, "    YY_BUFFER_STATE b = (YY_BUFFER_STATE)malloc(sizeof(struct {prefix}_buffer_state));");
    let _ = writeln!(out, "    if (!b) return NULL;");
    let _ = writeln!(out, "    b->{prefix}_ch_buf = (char *)malloc(size + 2);");
    let _ = writeln!(out, "    if (!b->{prefix}_ch_buf) {{ free(b); return NULL; }}");
    let _ = writeln!(out, "    b->{prefix}_buf_size = size;");
    let _ = writeln!(out, "    b->{prefix}_input_file = file;");
    let _ = writeln!(out, "    b->{prefix}_buf_pos = 0;");
    let _ = writeln!(out, "    b->{prefix}_n_chars = 0;");
    let _ = writeln!(out, "    b->{prefix}_eof = 0;");
    let _ = writeln!(out, "    return b;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(out, "void {prefix}_delete_buffer(YY_BUFFER_STATE b) {{");
    let _ = writeln!(out, "    if (b) {{");
    let _ = writeln!(out, "        free(b->{prefix}_ch_buf);");
    let _ = writeln!(out, "        free(b);");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(out, "void {prefix}_switch_to_buffer(YY_BUFFER_STATE b) {{");
    let _ = writeln!(out, "    {prefix}_current_buffer = b;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    // Yymore and REJECT support.
    let _ = writeln!(out, "static int {prefix}_more_flag = 0;");
    let _ = writeln!(out, "static int {prefix}_more_len = 0;");
    let _ = writeln!(out, "#define {prefix}more() ({prefix}_more_flag = 1)");
    let _ = writeln!(out);
    let _ = writeln!(out, "static int {prefix}_reject_flag = 0;");
    let _ = writeln!(out, "#define REJECT ({prefix}_reject_flag = 1)");
    let _ = writeln!(out);

    // DFA tables.
    generate_dfa_tables(&mut out, dfa, prefix);

    // Accept table.
    let _ = writeln!(out, "static const int {prefix}_accept[{}] = {{", dfa.states.len());
    for (i, state) in dfa.states.iter().enumerate() {
        let val = state.accepting.map_or(-1i32, |r| r as i32);
        if i + 1 < dfa.states.len() {
            let _ = write!(out, "    {val},");
        } else {
            let _ = write!(out, "    {val}");
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "}};");
    let _ = writeln!(out);

    // yylex function.
    let _ = writeln!(out, "int {prefix}lex(void) {{");
    let _ = writeln!(out, "    if (!{prefix}in) {prefix}in = stdin;");
    let _ = writeln!(out, "    if (!{prefix}out) {prefix}out = stdout;");
    let _ = writeln!(out);
    let _ = writeln!(out, "    if (!{prefix}_current_buffer) {{");
    let _ = writeln!(out, "        {prefix}_current_buffer = {prefix}_create_buffer({prefix}in, YY_BUF_SIZE);");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "    static char {prefix}_textbuf[YY_BUF_SIZE];");
    let _ = writeln!(out, "    int {prefix}_textpos = {prefix}_more_flag ? {prefix}_more_len : 0;");
    let _ = writeln!(out, "    {prefix}_more_flag = 0;");
    let _ = writeln!(out);
    let _ = writeln!(out, "scan_again:");
    let _ = writeln!(out, "    {{");
    let _ = writeln!(out, "        int state = {prefix}_dfa_start;");
    let _ = writeln!(out, "        int last_accept = -1;");
    let _ = writeln!(out, "        int last_accept_pos = {prefix}_textpos;");
    let _ = writeln!(out, "        int scan_start = {prefix}_textpos;");
    let _ = writeln!(out);
    let _ = writeln!(out, "        while (1) {{");
    let _ = writeln!(out, "            int c;");
    let _ = writeln!(out, "            YY_BUFFER_STATE buf = {prefix}_current_buffer;");
    let _ = writeln!(out, "            if (buf->{prefix}_buf_pos < buf->{prefix}_n_chars) {{");
    let _ = writeln!(out, "                c = (unsigned char)buf->{prefix}_ch_buf[buf->{prefix}_buf_pos++];");
    let _ = writeln!(out, "            }} else if (buf->{prefix}_eof) {{");
    let _ = writeln!(out, "                c = -1;");
    let _ = writeln!(out, "            }} else {{");
    let _ = writeln!(out, "                int n = fread(buf->{prefix}_ch_buf, 1, buf->{prefix}_buf_size, buf->{prefix}_input_file);");
    let _ = writeln!(out, "                if (n <= 0) {{");
    let _ = writeln!(out, "                    buf->{prefix}_eof = 1;");
    let _ = writeln!(out, "                    c = -1;");
    let _ = writeln!(out, "                }} else {{");
    let _ = writeln!(out, "                    buf->{prefix}_n_chars = n;");
    let _ = writeln!(out, "                    buf->{prefix}_buf_pos = 0;");
    let _ = writeln!(out, "                    c = (unsigned char)buf->{prefix}_ch_buf[buf->{prefix}_buf_pos++];");
    let _ = writeln!(out, "                }}");
    let _ = writeln!(out, "            }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "            if (c < 0) {{");
    let _ = writeln!(out, "                if (last_accept >= 0) break;");
    let _ = writeln!(out, "                if ({prefix}_textpos > scan_start) break;");
    let _ = writeln!(out, "                return 0; /* EOF */");
    let _ = writeln!(out, "            }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "            int next = {prefix}_dfa_trans[state][c];");
    let _ = writeln!(out, "            if (next < 0) break;");
    let _ = writeln!(out);
    let _ = writeln!(out, "            {prefix}_textbuf[{prefix}_textpos++] = (char)c;");
    let _ = writeln!(out, "            state = next;");
    let _ = writeln!(out);
    let _ = writeln!(out, "            if ({prefix}_accept[state] >= 0) {{");
    let _ = writeln!(out, "                last_accept = {prefix}_accept[state];");
    let _ = writeln!(out, "                last_accept_pos = {prefix}_textpos;");
    let _ = writeln!(out, "            }}");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "        /* Push back characters beyond the match */");
    let _ = writeln!(out, "        if ({prefix}_textpos > last_accept_pos && last_accept >= 0) {{");
    let _ = writeln!(out, "            int pushback = {prefix}_textpos - last_accept_pos;");
    let _ = writeln!(out, "            {prefix}_current_buffer->{prefix}_buf_pos -= pushback;");
    let _ = writeln!(out, "            {prefix}_textpos = last_accept_pos;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "        if (last_accept < 0) {{");
    let _ = writeln!(out, "            /* No match -- output one character and try again (default rule) */");
    let _ = writeln!(out, "            if ({prefix}_textpos > scan_start) {{");
    let _ = writeln!(out, "                fputc({prefix}_textbuf[scan_start], {prefix}out);");
    if options.yylineno {
        let _ = writeln!(out, "                if ({prefix}_textbuf[scan_start] == '\\n') {prefix}lineno++;");
    }
    let _ = writeln!(out, "                /* push back remaining */");
    let _ = writeln!(out, "                int remain = {prefix}_textpos - scan_start - 1;");
    let _ = writeln!(out, "                if (remain > 0) {prefix}_current_buffer->{prefix}_buf_pos -= remain;");
    let _ = writeln!(out, "                {prefix}_textpos = 0;");
    let _ = writeln!(out, "                goto scan_again;");
    let _ = writeln!(out, "            }}");
    let _ = writeln!(out, "            return 0; /* EOF */");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "        {prefix}_textbuf[last_accept_pos] = '\\0';");
    let _ = writeln!(out, "        {prefix}text = {prefix}_textbuf + scan_start;");
    let _ = writeln!(out, "        {prefix}leng = last_accept_pos - scan_start;");
    if options.yylineno {
        let _ = writeln!(out, "        {{ int _i; for (_i = scan_start; _i < last_accept_pos; _i++) if ({prefix}_textbuf[_i] == '\\n') {prefix}lineno++; }}");
    }
    let _ = writeln!(out);

    // Action switch.
    let _ = writeln!(out, "        {prefix}_reject_flag = 0;");
    let _ = writeln!(out, "        switch (last_accept) {{");
    for (i, rule) in rules.iter().enumerate() {
        if rule.start_conditions.is_empty() {
            let _ = writeln!(out, "        case {i}: /* <*> */");
        } else {
            let sc_list = rule.start_conditions.join(",");
            let _ = writeln!(out, "        case {i}: /* <{}> */", sc_list);
        }
        if rule.action.trim() == "|" {
            // Fall through to next rule.
            let _ = writeln!(out, "            /* fall through */");
        } else {
            let _ = writeln!(out, "            {}", rule.action.trim());
            if !rule.action.trim().ends_with('}') && !rule.action.trim().ends_with(';') {
                let _ = writeln!(out, "            ;");
            }
            let _ = writeln!(out, "            break;");
        }
    }
    let _ = writeln!(out, "        default: break;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out);

    // REJECT support: if the REJECT macro was invoked, we would need to try
    // the next-best match. For simplicity, we re-scan with a shortened match.
    let _ = writeln!(out, "        if ({prefix}_reject_flag) {{");
    let _ = writeln!(out, "            /* REJECT: push back matched text and skip one char */");
    let _ = writeln!(out, "            {prefix}_current_buffer->{prefix}_buf_pos -= ({prefix}leng - 1);");
    let _ = writeln!(out, "            fputc({prefix}_textbuf[scan_start], {prefix}out);");
    let _ = writeln!(out, "            {prefix}_textpos = 0;");
    let _ = writeln!(out, "            goto scan_again;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out);

    // yymore support.
    let _ = writeln!(out, "        if ({prefix}_more_flag) {{");
    let _ = writeln!(out, "            {prefix}_more_len = last_accept_pos;");
    let _ = writeln!(out, "        }} else {{");
    let _ = writeln!(out, "            {prefix}_textpos = 0;");
    let _ = writeln!(out, "            {prefix}_more_len = 0;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "        goto scan_again;");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    // yywrap.
    if !options.noyywrap {
        let _ = writeln!(out, "int {prefix}wrap(void) {{ return 1; }}");
        let _ = writeln!(out);
    }

    // Default main.
    let _ = writeln!(out, "#ifndef YY_NO_MAIN");
    let _ = writeln!(out, "int main(int argc, char **argv) {{");
    let _ = writeln!(out, "    if (argc > 1) {{");
    let _ = writeln!(out, "        {prefix}in = fopen(argv[1], \"r\");");
    let _ = writeln!(out, "        if (!{prefix}in) {{");
    let _ = writeln!(out, "            fprintf(stderr, \"Cannot open %s\\n\", argv[1]);");
    let _ = writeln!(out, "            return 1;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    while ({prefix}lex() != 0) ;");
    let _ = writeln!(out, "    return 0;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "#endif /* YY_NO_MAIN */");
    let _ = writeln!(out);

    // User code section.
    if !spec.user_code.is_empty() {
        let _ = writeln!(out, "/* User code section */");
        out.push_str(&spec.user_code);
    }

    out
}

/// Generate the DFA transition table as C arrays.
fn generate_dfa_tables(out: &mut String, dfa: &Dfa, prefix: &str) {
    let n = dfa.states.len();
    let _ = writeln!(out, "/* DFA tables: {n} states */");
    let _ = writeln!(out, "static const int {prefix}_dfa_start = {};", dfa.start);
    let _ = writeln!(out);

    // Transition table: dfa_trans[state][byte] = next_state (-1 for no transition).
    let _ = writeln!(out, "static const int {prefix}_dfa_trans[{n}][256] = {{");
    for (si, state) in dfa.states.iter().enumerate() {
        let _ = write!(out, "    /* state {si} */ {{");
        for (bi, tr) in state.transitions.iter().enumerate() {
            let val = tr.map_or(-1i32, |t| t as i32);
            if bi + 1 < 256 {
                let _ = write!(out, "{val},");
            } else {
                let _ = write!(out, "{val}");
            }
        }
        if si + 1 < n {
            let _ = writeln!(out, "}},");
        } else {
            let _ = writeln!(out, "}}");
        }
    }
    let _ = writeln!(out, "}};");
    let _ = writeln!(out);
}

// ---------------------------------------------------------------------------
// Apply options from %option directives
// ---------------------------------------------------------------------------

fn apply_options(opts: &mut Options, directives: &[(String, String)]) {
    for (key, val) in directives {
        match key.as_str() {
            "noyywrap" => opts.noyywrap = true,
            "yylineno" => opts.yylineno = true,
            "case-insensitive" | "caseless" => opts.case_insensitive = true,
            "prefix" => opts.prefix = val.clone(),
            "outfile" => opts.output_file = Some(val.clone()),
            "header-file" => opts.header_file = Some(val.clone()),
            "debug" => opts.debug = true,
            _ => {} // Unknown options are silently ignored (flex compat).
        }
    }
}

// ---------------------------------------------------------------------------
// Expand named definitions in patterns
// ---------------------------------------------------------------------------

fn expand_definitions(pattern: &str, definitions: &[Definition]) -> String {
    let mut result = pattern.to_string();
    // Iterate several times to handle nested definitions (up to 10 levels).
    for _ in 0..10 {
        let mut changed = false;
        for def in definitions {
            let placeholder = format!("{{{}}}", def.name);
            if result.contains(&placeholder) {
                result = result.replace(&placeholder, &format!("({})", def.expansion));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Full pipeline: spec -> C source
// ---------------------------------------------------------------------------

fn compile_spec(input: &str, options: &mut Options) -> Result<String, String> {
    let spec = parse_spec(input)?;

    // Apply %option directives.
    apply_options(options, &spec.option_directives);

    // Build a lookup map for definitions.
    let def_map: HashMap<String, String> = spec
        .definitions
        .iter()
        .map(|d| (d.name.clone(), d.expansion.clone()))
        .collect();

    // For each rule, expand definitions in the pattern, parse regex, build NFA.
    let mut rule_nfas = Vec::new();
    for (i, rule) in spec.rules.iter().enumerate() {
        let expanded = expand_definitions(&rule.pattern, &spec.definitions);
        let ast = parse_regex(&expanded, &def_map, options.case_insensitive)?;
        let nfa = build_nfa(&ast);
        rule_nfas.push((nfa, i));
    }

    if rule_nfas.is_empty() {
        return Err("no rules in specification".into());
    }

    // Combine all rule NFAs.
    let combined_nfa = combine_rule_nfas(rule_nfas);

    // Convert to DFA.
    let dfa = nfa_to_dfa(&combined_nfa);

    // Minimize the DFA.
    let min_dfa = minimize_dfa(&dfa);

    // Generate C code.
    let c_source = generate_scanner(&spec, &min_dfa, &spec.rules, options);

    Ok(c_source)
}

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

struct CliArgs {
    input_file: Option<String>,
    output_file: Option<String>,
    show_help: bool,
    show_version: bool,
}

fn parse_cli(args: &[String]) -> CliArgs {
    let mut result = CliArgs {
        input_file: None,
        output_file: None,
        show_help: false,
        show_version: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => result.show_help = true,
            "-V" | "--version" => result.show_version = true,
            "-o" => {
                i += 1;
                if i < args.len() {
                    result.output_file = Some(args[i].clone());
                }
            }
            "-t" => {
                // Output to stdout (default).
            }
            arg if !arg.starts_with('-') => {
                result.input_file = Some(arg.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    result
}

#[cfg(not(test))]
fn print_help(personality: Personality) {
    let name = match personality {
        Personality::Lex => "lex",
        Personality::Flex => "flex",
    };
    println!("Usage: {name} [OPTIONS] [FILE]");
    println!();
    println!("Generate a lexical analyzer from a specification file.");
    println!();
    println!("Options:");
    println!("  -o FILE   Write output to FILE (default: lex.yy.c)");
    println!("  -t        Write output to stdout");
    println!("  -h        Show this help");
    println!("  -V        Show version");
}

#[cfg(not(test))]
fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Lex => "lex",
        Personality::Flex => "flex",
    };
    println!("{name} (SlateOS) 0.1.0");
}

// ---------------------------------------------------------------------------
// run() wrapper
// ---------------------------------------------------------------------------

#[cfg(not(test))]
fn run() -> Result<i32, String> {
    let args: Vec<String> = env::args().collect();
    let personality = detect_personality(args.first().map_or("lex", |s| s.as_str()));
    let cli = parse_cli(&args);

    if cli.show_help {
        print_help(personality);
        return Ok(0);
    }
    if cli.show_version {
        print_version(personality);
        return Ok(0);
    }

    let input = match &cli.input_file {
        Some(path) => fs::read_to_string(path).map_err(|e| format!("cannot read '{}': {}", path, e))?,
        None => {
            let mut buf = String::new();
            io::stdin()
                .read_line(&mut buf)
                .map_err(|e| format!("cannot read stdin: {e}"))?;
            // Read remaining lines.
            let mut rest = String::new();
            io::Read::read_to_string(&mut io::stdin(), &mut rest)
                .map_err(|e| format!("cannot read stdin: {e}"))?;
            buf.push_str(&rest);
            buf
        }
    };

    let mut options = Options::new();
    if let Some(ref f) = cli.output_file {
        options.output_file = Some(f.clone());
    }

    let c_source = compile_spec(&input, &mut options)?;

    let output_path = options
        .output_file
        .as_deref()
        .unwrap_or("lex.yy.c");

    if cli.output_file.is_none() && cli.input_file.is_some() {
        // Write to file.
        fs::write(output_path, &c_source)
            .map_err(|e| format!("cannot write '{}': {}", output_path, e))?;
        eprintln!("lex: wrote {}", output_path);
    } else if cli.output_file.is_some() {
        fs::write(output_path, &c_source)
            .map_err(|e| format!("cannot write '{}': {}", output_path, e))?;
        eprintln!("lex: wrote {}", output_path);
    } else {
        // stdout
        print!("{c_source}");
    }

    Ok(0)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    match run() {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(io::stderr(), "lex: {e}");
            1
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn defs_map() -> HashMap<String, String> {
        HashMap::new()
    }

    fn parse_ok(pattern: &str) -> RegexAst {
        parse_regex(pattern, &defs_map(), false).expect("regex parse failed")
    }

    fn compile_to_dfa(patterns: &[&str]) -> Dfa {
        let defs = defs_map();
        let mut rule_nfas = Vec::new();
        for (i, pat) in patterns.iter().enumerate() {
            let ast = parse_regex(pat, &defs, false).unwrap();
            let nfa = build_nfa(&ast);
            rule_nfas.push((nfa, i));
        }
        let combined = combine_rule_nfas(rule_nfas);
        nfa_to_dfa(&combined)
    }

    /// Simulate running the DFA on a byte string; returns the accepting rule
    /// index and length of the longest match, or None.
    fn dfa_match(dfa: &Dfa, input: &[u8]) -> Option<(usize, usize)> {
        let mut state = dfa.start;
        let mut last_accept: Option<(usize, usize)> = None;

        if let Some(rule) = dfa.states[state].accepting {
            last_accept = Some((rule, 0));
        }

        for (i, &b) in input.iter().enumerate() {
            match dfa.states[state].transitions[b as usize] {
                Some(next) => {
                    state = next;
                    if let Some(rule) = dfa.states[state].accepting {
                        last_accept = Some((rule, i + 1));
                    }
                }
                None => break,
            }
        }

        last_accept
    }

    // -----------------------------------------------------------------------
    // Personality detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_lex() {
        assert_eq!(detect_personality("lex"), Personality::Lex);
        assert_eq!(detect_personality("/usr/bin/lex"), Personality::Lex);
    }

    #[test]
    fn test_personality_flex() {
        assert_eq!(detect_personality("flex"), Personality::Flex);
        assert_eq!(detect_personality("/usr/local/bin/flex"), Personality::Flex);
    }

    #[test]
    fn test_personality_unknown_defaults_lex() {
        assert_eq!(detect_personality("scanner"), Personality::Lex);
    }

    // -----------------------------------------------------------------------
    // Regex parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_literal() {
        let ast = parse_ok("a");
        assert_eq!(ast, RegexAst::Literal(b'a'));
    }

    #[test]
    fn test_parse_concat() {
        let ast = parse_ok("ab");
        match ast {
            RegexAst::Concat(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], RegexAst::Literal(b'a'));
                assert_eq!(items[1], RegexAst::Literal(b'b'));
            }
            _ => panic!("expected Concat"),
        }
    }

    #[test]
    fn test_parse_alternation() {
        let ast = parse_ok("a|b");
        match ast {
            RegexAst::Alternation(branches) => {
                assert_eq!(branches.len(), 2);
            }
            _ => panic!("expected Alternation"),
        }
    }

    #[test]
    fn test_parse_star() {
        let ast = parse_ok("a*");
        match ast {
            RegexAst::Star(inner) => assert_eq!(*inner, RegexAst::Literal(b'a')),
            _ => panic!("expected Star"),
        }
    }

    #[test]
    fn test_parse_plus() {
        let ast = parse_ok("a+");
        match ast {
            RegexAst::Plus(inner) => assert_eq!(*inner, RegexAst::Literal(b'a')),
            _ => panic!("expected Plus"),
        }
    }

    #[test]
    fn test_parse_question() {
        let ast = parse_ok("a?");
        match ast {
            RegexAst::Question(inner) => assert_eq!(*inner, RegexAst::Literal(b'a')),
            _ => panic!("expected Question"),
        }
    }

    #[test]
    fn test_parse_dot() {
        let ast = parse_ok(".");
        assert_eq!(ast, RegexAst::AnyChar);
    }

    #[test]
    fn test_parse_grouping() {
        let ast = parse_ok("(ab)");
        match ast {
            RegexAst::Concat(items) => assert_eq!(items.len(), 2),
            _ => panic!("expected Concat from group"),
        }
    }

    #[test]
    fn test_parse_char_class_simple() {
        let ast = parse_ok("[abc]");
        match ast {
            RegexAst::CharClass { singles, negated, .. } => {
                assert!(!negated);
                assert!(singles.contains(&b'a'));
                assert!(singles.contains(&b'b'));
                assert!(singles.contains(&b'c'));
            }
            _ => panic!("expected CharClass"),
        }
    }

    #[test]
    fn test_parse_char_class_range() {
        let ast = parse_ok("[a-z]");
        match ast {
            RegexAst::CharClass { ranges, negated, .. } => {
                assert!(!negated);
                assert_eq!(ranges.len(), 1);
                assert_eq!(ranges[0], (b'a', b'z'));
            }
            _ => panic!("expected CharClass"),
        }
    }

    #[test]
    fn test_parse_char_class_negated() {
        let ast = parse_ok("[^0-9]");
        match ast {
            RegexAst::CharClass { negated, .. } => {
                assert!(negated);
            }
            _ => panic!("expected CharClass"),
        }
    }

    #[test]
    fn test_parse_escape_n() {
        let ast = parse_ok("\\n");
        assert_eq!(ast, RegexAst::Literal(b'\n'));
    }

    #[test]
    fn test_parse_escape_t() {
        let ast = parse_ok("\\t");
        assert_eq!(ast, RegexAst::Literal(b'\t'));
    }

    #[test]
    fn test_parse_escape_special() {
        // Escape a metacharacter.
        let ast = parse_ok("\\*");
        assert_eq!(ast, RegexAst::Literal(b'*'));
    }

    #[test]
    fn test_parse_quoted_literal() {
        let ast = parse_ok("\"ab\"");
        match ast {
            RegexAst::Concat(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], RegexAst::Literal(b'a'));
                assert_eq!(items[1], RegexAst::Literal(b'b'));
            }
            _ => panic!("expected Concat"),
        }
    }

    #[test]
    fn test_parse_start_anchor() {
        let ast = parse_ok("^a");
        match ast {
            RegexAst::Concat(items) => {
                assert_eq!(items[0], RegexAst::StartAnchor);
                assert_eq!(items[1], RegexAst::Literal(b'a'));
            }
            _ => panic!("expected Concat with anchor"),
        }
    }

    #[test]
    fn test_parse_end_anchor() {
        let ast = parse_ok("a$");
        match ast {
            RegexAst::Concat(items) => {
                assert_eq!(items[0], RegexAst::Literal(b'a'));
                assert_eq!(items[1], RegexAst::EndAnchor);
            }
            _ => panic!("expected Concat with anchor"),
        }
    }

    #[test]
    fn test_parse_definition_ref() {
        let mut defs = HashMap::new();
        defs.insert("DIGIT".into(), "[0-9]".into());
        let ast = parse_regex("{DIGIT}", &defs, false).unwrap();
        match ast {
            RegexAst::CharClass { ranges, negated, .. } => {
                assert!(!negated);
                assert_eq!(ranges[0], (b'0', b'9'));
            }
            _ => panic!("expected CharClass from definition"),
        }
    }

    #[test]
    fn test_parse_undefined_ref_error() {
        let defs = HashMap::new();
        let result = parse_regex("{NOPE}", &defs, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_complex_regex() {
        // Test a reasonably complex pattern.
        let ast = parse_ok("[a-zA-Z_][a-zA-Z0-9_]*");
        match ast {
            RegexAst::Concat(items) => {
                assert_eq!(items.len(), 2);
                // Second item should be Star.
                match &items[1] {
                    RegexAst::Star(_) => {}
                    _ => panic!("expected Star"),
                }
            }
            _ => panic!("expected Concat"),
        }
    }

    #[test]
    fn test_parse_nested_groups() {
        let ast = parse_ok("((a|b)c)");
        match ast {
            RegexAst::Concat(items) => {
                assert_eq!(items.len(), 2);
                match &items[0] {
                    RegexAst::Alternation(_) => {}
                    _ => panic!("expected Alternation in first group element"),
                }
            }
            _ => panic!("expected Concat"),
        }
    }

    #[test]
    fn test_case_insensitive_literal() {
        let defs = HashMap::new();
        let ast = parse_regex("a", &defs, true).unwrap();
        match ast {
            RegexAst::CharClass { singles, .. } => {
                assert!(singles.contains(&b'a'));
                assert!(singles.contains(&b'A'));
            }
            _ => panic!("expected CharClass for case-insensitive"),
        }
    }

    // -----------------------------------------------------------------------
    // NFA construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_nfa_literal() {
        let ast = RegexAst::Literal(b'x');
        let nfa = build_nfa(&ast);
        assert_eq!(nfa.states.len(), 2);
        assert_eq!(nfa.states[nfa.start].transitions.len(), 1);
    }

    #[test]
    fn test_nfa_concat() {
        let ast = RegexAst::Concat(vec![RegexAst::Literal(b'a'), RegexAst::Literal(b'b')]);
        let nfa = build_nfa(&ast);
        // Should have states for both sub-NFAs plus epsilon transitions.
        assert!(nfa.states.len() >= 4);
    }

    #[test]
    fn test_nfa_alternation() {
        let ast = RegexAst::Alternation(vec![RegexAst::Literal(b'a'), RegexAst::Literal(b'b')]);
        let nfa = build_nfa(&ast);
        // New start + new accept + sub-NFA states.
        assert!(nfa.states.len() >= 6);
    }

    #[test]
    fn test_nfa_star() {
        let ast = RegexAst::Star(Box::new(RegexAst::Literal(b'a')));
        let nfa = build_nfa(&ast);
        assert!(nfa.states.len() >= 4);
    }

    #[test]
    fn test_nfa_plus() {
        let ast = RegexAst::Plus(Box::new(RegexAst::Literal(b'a')));
        let nfa = build_nfa(&ast);
        assert!(nfa.states.len() >= 4);
    }

    #[test]
    fn test_nfa_question() {
        let ast = RegexAst::Question(Box::new(RegexAst::Literal(b'a')));
        let nfa = build_nfa(&ast);
        assert!(nfa.states.len() >= 4);
    }

    #[test]
    fn test_nfa_any_char() {
        let ast = RegexAst::AnyChar;
        let nfa = build_nfa(&ast);
        assert_eq!(nfa.states.len(), 2);
    }

    #[test]
    fn test_nfa_char_class() {
        let ast = RegexAst::CharClass {
            ranges: vec![(b'a', b'z')],
            singles: vec![],
            negated: false,
        };
        let nfa = build_nfa(&ast);
        assert_eq!(nfa.states.len(), 2);
        // Should have a range transition.
        assert!(!nfa.states[nfa.start].transitions.is_empty());
    }

    #[test]
    fn test_nfa_negated_class() {
        let ast = RegexAst::CharClass {
            ranges: vec![],
            singles: vec![b'a'],
            negated: true,
        };
        let nfa = build_nfa(&ast);
        assert_eq!(nfa.states.len(), 2);
    }

    // -----------------------------------------------------------------------
    // DFA construction and matching tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dfa_single_literal() {
        let dfa = compile_to_dfa(&["a"]);
        assert_eq!(dfa_match(&dfa, b"a"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"b"), None);
    }

    #[test]
    fn test_dfa_concat() {
        let dfa = compile_to_dfa(&["ab"]);
        assert_eq!(dfa_match(&dfa, b"ab"), Some((0, 2)));
        assert_eq!(dfa_match(&dfa, b"a"), None);
    }

    #[test]
    fn test_dfa_alternation() {
        let dfa = compile_to_dfa(&["a|b"]);
        assert_eq!(dfa_match(&dfa, b"a"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"b"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"c"), None);
    }

    #[test]
    fn test_dfa_star() {
        let dfa = compile_to_dfa(&["a*"]);
        // Star matches empty string.
        assert_eq!(dfa_match(&dfa, b""), Some((0, 0)));
        assert_eq!(dfa_match(&dfa, b"aaa"), Some((0, 3)));
    }

    #[test]
    fn test_dfa_plus() {
        let dfa = compile_to_dfa(&["a+"]);
        assert_eq!(dfa_match(&dfa, b""), None);
        assert_eq!(dfa_match(&dfa, b"a"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"aaa"), Some((0, 3)));
    }

    #[test]
    fn test_dfa_question() {
        let dfa = compile_to_dfa(&["ab?"]);
        assert_eq!(dfa_match(&dfa, b"a"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"ab"), Some((0, 2)));
    }

    #[test]
    fn test_dfa_char_class() {
        let dfa = compile_to_dfa(&["[a-c]"]);
        assert_eq!(dfa_match(&dfa, b"a"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"b"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"c"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"d"), None);
    }

    #[test]
    fn test_dfa_negated_class() {
        let dfa = compile_to_dfa(&["[^a]"]);
        assert_eq!(dfa_match(&dfa, b"a"), None);
        assert_eq!(dfa_match(&dfa, b"b"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"z"), Some((0, 1)));
    }

    #[test]
    fn test_dfa_dot() {
        let dfa = compile_to_dfa(&["."]);
        assert_eq!(dfa_match(&dfa, b"x"), Some((0, 1)));
        // Dot should not match newline.
        assert_eq!(dfa_match(&dfa, b"\n"), None);
    }

    #[test]
    fn test_dfa_multiple_rules_priority() {
        // First rule has higher priority.
        let dfa = compile_to_dfa(&["ab", "a"]);
        assert_eq!(dfa_match(&dfa, b"ab"), Some((0, 2)));
        assert_eq!(dfa_match(&dfa, b"a"), Some((1, 1)));
    }

    #[test]
    fn test_dfa_longest_match() {
        let dfa = compile_to_dfa(&["a+", "a"]);
        // Longest match should win: "aaa" matches rule 0 with length 3.
        assert_eq!(dfa_match(&dfa, b"aaa"), Some((0, 3)));
    }

    #[test]
    fn test_dfa_identifier_pattern() {
        let dfa = compile_to_dfa(&["[a-zA-Z_][a-zA-Z0-9_]*"]);
        assert_eq!(dfa_match(&dfa, b"foo"), Some((0, 3)));
        assert_eq!(dfa_match(&dfa, b"_bar"), Some((0, 4)));
        assert_eq!(dfa_match(&dfa, b"x123"), Some((0, 4)));
        assert_eq!(dfa_match(&dfa, b"123"), None);
    }

    #[test]
    fn test_dfa_integer_pattern() {
        let dfa = compile_to_dfa(&["[0-9]+"]);
        assert_eq!(dfa_match(&dfa, b"42"), Some((0, 2)));
        assert_eq!(dfa_match(&dfa, b"0"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"abc"), None);
    }

    // -----------------------------------------------------------------------
    // DFA minimization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_minimize_reduces_states() {
        // Build a DFA for a pattern that produces redundant states.
        let dfa = compile_to_dfa(&["a|a"]);
        let min = minimize_dfa(&dfa);
        assert!(min.states.len() <= dfa.states.len());
    }

    #[test]
    fn test_minimize_preserves_matching() {
        let dfa = compile_to_dfa(&["(a|b)*c"]);
        let min = minimize_dfa(&dfa);
        // Both DFAs should accept the same strings.
        assert_eq!(dfa_match(&dfa, b"c"), dfa_match(&min, b"c"));
        assert_eq!(dfa_match(&dfa, b"abc"), dfa_match(&min, b"abc"));
        assert_eq!(dfa_match(&dfa, b"bac"), dfa_match(&min, b"bac"));
        assert_eq!(dfa_match(&dfa, b"aaabbbccc"), dfa_match(&min, b"aaabbbccc"));
    }

    #[test]
    fn test_minimize_single_rule() {
        let dfa = compile_to_dfa(&["hello"]);
        let min = minimize_dfa(&dfa);
        assert_eq!(dfa_match(&min, b"hello"), Some((0, 5)));
        assert_eq!(dfa_match(&min, b"hell"), None);
    }

    #[test]
    fn test_minimize_multi_rule() {
        let dfa = compile_to_dfa(&["if", "int", "[a-z]+"]);
        let min = minimize_dfa(&dfa);
        // "if" matches rule 0 (higher priority, same longest length).
        assert_eq!(dfa_match(&min, b"if"), Some((0, 2)));
        // "int" matches rule 1.
        assert_eq!(dfa_match(&min, b"int"), Some((1, 3)));
        // "foo" matches rule 2.
        assert_eq!(dfa_match(&min, b"foo"), Some((2, 3)));
    }

    // -----------------------------------------------------------------------
    // Spec parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_spec_basic() {
        let input = "\
%%
[a-z]+   printf(\"WORD\");
[0-9]+   printf(\"NUM\");
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.rules.len(), 2);
        assert_eq!(spec.rules[0].pattern, "[a-z]+");
        assert!(spec.rules[0].action.contains("printf"));
    }

    #[test]
    fn test_parse_spec_definitions() {
        let input = "\
DIGIT [0-9]
ALPHA [a-zA-Z]
%%
{DIGIT}+  return NUM;
{ALPHA}+  return WORD;
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.definitions.len(), 2);
        assert_eq!(spec.definitions[0].name, "DIGIT");
        assert_eq!(spec.definitions[0].expansion, "[0-9]");
    }

    #[test]
    fn test_parse_spec_option_directives() {
        let input = "\
%option noyywrap yylineno
%%
.  ;
%%
";
        let spec = parse_spec(input).unwrap();
        assert!(spec.option_directives.iter().any(|(k, _)| k == "noyywrap"));
        assert!(spec.option_directives.iter().any(|(k, _)| k == "yylineno"));
    }

    #[test]
    fn test_parse_spec_start_conditions() {
        let input = "\
%s COMMENT
%x STRING
%%
.  ;
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.start_conditions.len(), 2);
        assert!(!spec.start_conditions[0].exclusive);
        assert_eq!(spec.start_conditions[0].name, "COMMENT");
        assert!(spec.start_conditions[1].exclusive);
        assert_eq!(spec.start_conditions[1].name, "STRING");
    }

    #[test]
    fn test_parse_spec_top_code() {
        let input = "\
%{
#include <stdio.h>
int count = 0;
%}
%%
.  count++;
%%
";
        let spec = parse_spec(input).unwrap();
        assert!(spec.top_code.contains("#include <stdio.h>"));
        assert!(spec.top_code.contains("int count = 0;"));
    }

    #[test]
    fn test_parse_spec_user_code() {
        let input = "\
%%
.  ;
%%
int main() { yylex(); return 0; }
";
        let spec = parse_spec(input).unwrap();
        assert!(spec.user_code.contains("int main()"));
    }

    #[test]
    fn test_parse_spec_multi_line_action() {
        let input = "\
%%
[a-z]+  {
    printf(\"word: %s\\n\", yytext);
    count++;
}
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.rules.len(), 1);
        assert!(spec.rules[0].action.contains("printf"));
        assert!(spec.rules[0].action.contains("count++"));
    }

    #[test]
    fn test_parse_spec_fall_through() {
        let input = "\
%%
a  |
b  printf(\"a or b\");
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.rules.len(), 2);
        assert_eq!(spec.rules[0].action, "|");
    }

    #[test]
    fn test_parse_spec_start_condition_rule() {
        let input = "\
%x STR
%%
<STR>[^\"]*  ;
.            ;
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.rules[0].start_conditions, vec!["STR"]);
    }

    // -----------------------------------------------------------------------
    // Options / apply_options tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_noyywrap() {
        let mut opts = Options::new();
        apply_options(&mut opts, &[("noyywrap".into(), String::new())]);
        assert!(opts.noyywrap);
    }

    #[test]
    fn test_apply_yylineno() {
        let mut opts = Options::new();
        apply_options(&mut opts, &[("yylineno".into(), String::new())]);
        assert!(opts.yylineno);
    }

    #[test]
    fn test_apply_case_insensitive() {
        let mut opts = Options::new();
        apply_options(
            &mut opts,
            &[("case-insensitive".into(), String::new())],
        );
        assert!(opts.case_insensitive);
    }

    #[test]
    fn test_apply_prefix() {
        let mut opts = Options::new();
        apply_options(&mut opts, &[("prefix".into(), "zz".into())]);
        assert_eq!(opts.prefix, "zz");
    }

    // -----------------------------------------------------------------------
    // Definition expansion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_expand_single_def() {
        let defs = vec![Definition {
            name: "D".into(),
            expansion: "[0-9]".into(),
        }];
        let expanded = expand_definitions("{D}+", &defs);
        assert_eq!(expanded, "([0-9])+");
    }

    #[test]
    fn test_expand_nested_defs() {
        let defs = vec![
            Definition {
                name: "D".into(),
                expansion: "[0-9]".into(),
            },
            Definition {
                name: "NUM".into(),
                expansion: "{D}+".into(),
            },
        ];
        let expanded = expand_definitions("{NUM}", &defs);
        assert_eq!(expanded, "(([0-9])+)");
    }

    #[test]
    fn test_expand_no_match() {
        let defs = vec![];
        let expanded = expand_definitions("hello", &defs);
        assert_eq!(expanded, "hello");
    }

    // -----------------------------------------------------------------------
    // Full pipeline tests (spec -> C code)
    // -----------------------------------------------------------------------

    #[test]
    fn test_compile_basic_spec() {
        let input = "\
%%
[a-z]+  printf(\"WORD\");
[0-9]+  printf(\"NUM\");
%%
";
        let mut opts = Options::new();
        let result = compile_spec(input, &mut opts);
        assert!(result.is_ok());
        let code = result.unwrap();
        assert!(code.contains("yylex"));
        assert!(code.contains("yytext"));
        assert!(code.contains("WORD"));
        assert!(code.contains("NUM"));
    }

    #[test]
    fn test_compile_with_definitions() {
        let input = "\
DIGIT [0-9]
%%
{DIGIT}+  return 1;
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("yy_dfa_trans"));
        assert!(code.contains("return 1;"));
    }

    #[test]
    fn test_compile_with_options() {
        let input = "\
%option noyywrap yylineno
%%
.  ;
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(opts.noyywrap);
        assert!(opts.yylineno);
        assert!(code.contains("yylineno"));
        // Should NOT contain yywrap definition since noyywrap.
        assert!(!code.contains("int yywrap(void)"));
    }

    #[test]
    fn test_compile_with_start_conditions() {
        let input = "\
%x STR
%%
\"      BEGIN(STR);
<STR>\"  BEGIN(INITIAL);
<STR>.   ;
.        ;
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("#define STR 1"));
        assert!(code.contains("#define INITIAL 0"));
        assert!(code.contains("BEGIN("));
    }

    #[test]
    fn test_compile_with_prefix() {
        let input = "\
%option prefix=\"zz\"
%%
.  ;
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("zzlex"));
        assert!(code.contains("zztext"));
    }

    #[test]
    fn test_compile_with_user_code() {
        let input = "\
%%
.  ;
%%
void helper(void) { /* custom */ }
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("void helper(void)"));
    }

    #[test]
    fn test_compile_with_top_code() {
        let input = "\
%{
#include \"myheader.h\"
%}
%%
.  ;
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("#include \"myheader.h\""));
    }

    #[test]
    fn test_compile_empty_rules_error() {
        let input = "\
%%
%%
";
        let mut opts = Options::new();
        let result = compile_spec(input, &mut opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_reject_support() {
        let input = "\
%%
ab  REJECT;
.   printf(\"x\");
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("yy_reject_flag"));
        assert!(code.contains("REJECT"));
    }

    #[test]
    fn test_compile_yymore_support() {
        let input = "\
%%
ab  yymore();
.   ;
%%
";
        let mut opts = Options::new();
        let code = compile_spec(input, &mut opts).unwrap();
        assert!(code.contains("yy_more_flag"));
        assert!(code.contains("yymore()"));
    }

    // -----------------------------------------------------------------------
    // DFA table generation format tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dfa_tables_contain_start() {
        let dfa = compile_to_dfa(&["a"]);
        let mut out = String::new();
        generate_dfa_tables(&mut out, &dfa, "yy");
        assert!(out.contains("yy_dfa_start"));
    }

    #[test]
    fn test_dfa_tables_contain_trans() {
        let dfa = compile_to_dfa(&["a"]);
        let mut out = String::new();
        generate_dfa_tables(&mut out, &dfa, "yy");
        assert!(out.contains("yy_dfa_trans"));
    }

    // -----------------------------------------------------------------------
    // Char class bytes helper tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_char_class_bytes_range() {
        let bytes = char_class_bytes(&[(b'a', b'c')], &[]);
        assert_eq!(bytes, vec![b'a', b'b', b'c']);
    }

    #[test]
    fn test_char_class_bytes_singles() {
        let bytes = char_class_bytes(&[], b"xy");
        assert_eq!(bytes, vec![b'x', b'y']);
    }

    #[test]
    fn test_char_class_bytes_dedup() {
        let bytes = char_class_bytes(&[(b'a', b'c')], b"b");
        // 'b' is in both range and singles; dedup removes it.
        assert_eq!(bytes, vec![b'a', b'b', b'c']);
    }

    // -----------------------------------------------------------------------
    // Epsilon closure tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_epsilon_closure_single() {
        let nfa = build_nfa(&RegexAst::Literal(b'a'));
        let mut start_set = BTreeSet::new();
        start_set.insert(nfa.start);
        let closure = epsilon_closure(&nfa, &start_set);
        // Literal NFA: start --byte-> accept. No epsilon transitions from start.
        assert_eq!(closure.len(), 1);
    }

    #[test]
    fn test_epsilon_closure_star() {
        let nfa = build_nfa(&RegexAst::Star(Box::new(RegexAst::Literal(b'a'))));
        let mut start_set = BTreeSet::new();
        start_set.insert(nfa.start);
        let closure = epsilon_closure(&nfa, &start_set);
        // Star NFA has epsilon from new_start to sub_start and to new_accept.
        assert!(closure.len() >= 3);
    }

    // -----------------------------------------------------------------------
    // CLI parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cli_help() {
        let args = vec!["lex".into(), "-h".into()];
        let cli = parse_cli(&args);
        assert!(cli.show_help);
    }

    #[test]
    fn test_cli_version() {
        let args = vec!["lex".into(), "--version".into()];
        let cli = parse_cli(&args);
        assert!(cli.show_version);
    }

    #[test]
    fn test_cli_output_file() {
        let args = vec!["lex".into(), "-o".into(), "out.c".into(), "input.l".into()];
        let cli = parse_cli(&args);
        assert_eq!(cli.output_file.as_deref(), Some("out.c"));
        assert_eq!(cli.input_file.as_deref(), Some("input.l"));
    }

    #[test]
    fn test_cli_input_only() {
        let args = vec!["lex".into(), "scanner.l".into()];
        let cli = parse_cli(&args);
        assert_eq!(cli.input_file.as_deref(), Some("scanner.l"));
        assert!(cli.output_file.is_none());
    }

    // -----------------------------------------------------------------------
    // Edge case / robustness tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dfa_empty_input() {
        let dfa = compile_to_dfa(&["a+"]);
        assert_eq!(dfa_match(&dfa, b""), None);
    }

    #[test]
    fn test_dfa_long_input() {
        let dfa = compile_to_dfa(&["a+"]);
        let input = vec![b'a'; 1000];
        assert_eq!(dfa_match(&dfa, &input), Some((0, 1000)));
    }

    #[test]
    fn test_regex_empty_alternation_branch() {
        // "(|a)" means "empty string or a".
        let dfa = compile_to_dfa(&["(|a)"]);
        assert!(dfa_match(&dfa, b"").is_some());
        assert!(dfa_match(&dfa, b"a").is_some());
    }

    #[test]
    fn test_multiple_char_class_ranges() {
        let dfa = compile_to_dfa(&["[a-zA-Z]"]);
        assert_eq!(dfa_match(&dfa, b"m"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"M"), Some((0, 1)));
        assert_eq!(dfa_match(&dfa, b"5"), None);
    }

    #[test]
    fn test_combined_nfa_preserves_rule_indices() {
        let defs = defs_map();
        let ast0 = parse_regex("hello", &defs, false).unwrap();
        let ast1 = parse_regex("world", &defs, false).unwrap();
        let nfa0 = build_nfa(&ast0);
        let nfa1 = build_nfa(&ast1);
        let combined = combine_rule_nfas(vec![(nfa0, 0), (nfa1, 1)]);
        // Verify both accepting markers exist.
        let has_0 = combined.states.iter().any(|s| s.accepting == Some(0));
        let has_1 = combined.states.iter().any(|s| s.accepting == Some(1));
        assert!(has_0);
        assert!(has_1);
    }

    #[test]
    fn test_minimize_empty_dfa() {
        let dfa = Dfa {
            states: Vec::new(),
            start: 0,
        };
        let min = minimize_dfa(&dfa);
        assert!(min.states.is_empty());
    }

    #[test]
    fn test_parse_spec_no_second_delimiter() {
        // Spec with only one %% (no user code section).
        let input = "\
%%
[a-z]+  printf(\"w\");
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.rules.len(), 1);
        assert!(spec.user_code.is_empty());
    }

    #[test]
    fn test_spec_multiple_option_lines() {
        let input = "\
%option noyywrap
%option yylineno
%option case-insensitive
%%
.  ;
%%
";
        let spec = parse_spec(input).unwrap();
        assert_eq!(spec.option_directives.len(), 3);
    }

    #[test]
    fn test_option_caseless_alias() {
        let mut opts = Options::new();
        apply_options(&mut opts, &[("caseless".into(), String::new())]);
        assert!(opts.case_insensitive);
    }

    #[test]
    fn test_dfa_three_rules_all_match() {
        // Three rules that all match "abc" with different prefixes.
        let dfa = compile_to_dfa(&["abc", "ab", "a"]);
        // Longest match: "abc" rule 0.
        assert_eq!(dfa_match(&dfa, b"abc"), Some((0, 3)));
        // Only "ab" or "a" possible.
        assert_eq!(dfa_match(&dfa, b"ab"), Some((1, 2)));
        assert_eq!(dfa_match(&dfa, b"a"), Some((2, 1)));
    }
}
