//! OurOS `awk` — pattern-action text processor
//!
//! A core-subset AWK implementation: lexer, recursive-descent parser, and
//! tree-walk interpreter.  Handles `BEGIN`/`END` rules, regex and expression
//! patterns, field splitting, associative arrays, and the standard built-in
//! functions required for everyday text processing.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

// ---------------------------------------------------------------------------
// Regex engine (basic backtracking, reused pattern from the sed utility)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ReNode {
    Literal(u8),
    AnyChar,
    StartAnchor,
    EndAnchor,
    CharClass { bytes: Vec<u8>, negated: bool },
    Star(Box<ReNode>),
    Plus(Box<ReNode>),
    Question(Box<ReNode>),
}

#[derive(Debug, Clone)]
struct Regex {
    nodes: Vec<ReNode>,
}

fn parse_char_class(pattern: &[u8], pos: &mut usize) -> Result<(Vec<u8>, bool), String> {
    let mut bytes = Vec::new();
    let negated = if *pos < pattern.len() && pattern[*pos] == b'^' {
        *pos += 1;
        true
    } else {
        false
    };
    if *pos < pattern.len() && pattern[*pos] == b']' {
        bytes.push(b']');
        *pos += 1;
    }
    while *pos < pattern.len() && pattern[*pos] != b']' {
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
            match pattern[*pos] {
                b'n' => bytes.push(b'\n'),
                b't' => bytes.push(b'\t'),
                other => bytes.push(other),
            }
            *pos += 1;
        } else {
            bytes.push(pattern[*pos]);
            *pos += 1;
        }
    }
    if *pos < pattern.len() && pattern[*pos] == b']' {
        *pos += 1;
    } else {
        return Err("unterminated character class".into());
    }
    Ok((bytes, negated))
}

fn compile_regex(pattern: &[u8]) -> Result<Regex, String> {
    let mut nodes: Vec<ReNode> = Vec::new();
    let mut pos = 0;
    while pos < pattern.len() {
        match pattern[pos] {
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
                pos += 1;
            }
            b'+' => {
                if let Some(prev) = nodes.pop() {
                    nodes.push(ReNode::Plus(Box::new(prev)));
                }
                pos += 1;
            }
            b'?' => {
                if let Some(prev) = nodes.pop() {
                    nodes.push(ReNode::Question(Box::new(prev)));
                }
                pos += 1;
            }
            b'\\' => {
                pos += 1;
                if pos >= pattern.len() {
                    return Err("trailing backslash in regex".into());
                }
                match pattern[pos] {
                    b'n' => nodes.push(ReNode::Literal(b'\n')),
                    b't' => nodes.push(ReNode::Literal(b'\t')),
                    other => nodes.push(ReNode::Literal(other)),
                }
                pos += 1;
            }
            ch => {
                nodes.push(ReNode::Literal(ch));
                pos += 1;
            }
        }
    }
    Ok(Regex { nodes })
}

impl Regex {
    fn is_match(&self, text: &[u8]) -> bool {
        self.find(text).is_some()
    }

    fn find(&self, text: &[u8]) -> Option<(usize, usize)> {
        if matches!(self.nodes.first(), Some(ReNode::StartAnchor)) {
            return self
                .match_at(text, 0, 1)
                .map(|end| (0, end));
        }
        for start in 0..=text.len() {
            if let Some(end) = self.match_at(text, start, 0) {
                return Some((start, end));
            }
        }
        None
    }

    fn match_at(&self, text: &[u8], pos: usize, ni: usize) -> Option<usize> {
        if ni >= self.nodes.len() {
            return Some(pos);
        }
        match &self.nodes[ni] {
            ReNode::StartAnchor => {
                if pos == 0 {
                    self.match_at(text, pos, ni + 1)
                } else {
                    None
                }
            }
            ReNode::EndAnchor => {
                if pos == text.len() {
                    self.match_at(text, pos, ni + 1)
                } else {
                    None
                }
            }
            ReNode::Literal(ch) => {
                if pos < text.len() && text[pos] == *ch {
                    self.match_at(text, pos + 1, ni + 1)
                } else {
                    None
                }
            }
            ReNode::AnyChar => {
                if pos < text.len() && text[pos] != b'\n' {
                    self.match_at(text, pos + 1, ni + 1)
                } else {
                    None
                }
            }
            ReNode::CharClass { bytes, negated } => {
                if pos < text.len() {
                    let in_class = bytes.contains(&text[pos]);
                    let ok = if *negated { !in_class } else { in_class };
                    if ok {
                        self.match_at(text, pos + 1, ni + 1)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ReNode::Star(inner) => {
                let mut positions = vec![pos];
                let mut p = pos;
                while p < text.len() {
                    if let Some(end) = self.match_single(inner, text, p) {
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
                    if let Some(end) = self.match_at(text, try_pos, ni + 1) {
                        return Some(end);
                    }
                }
                None
            }
            ReNode::Plus(inner) => {
                let mut positions = Vec::new();
                let mut p = pos;
                while p < text.len() {
                    if let Some(end) = self.match_single(inner, text, p) {
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
                    if let Some(end) = self.match_at(text, try_pos, ni + 1) {
                        return Some(end);
                    }
                }
                None
            }
            ReNode::Question(inner) => {
                if let Some(end1) = self.match_single(inner, text, pos) {
                    if let Some(end) = self.match_at(text, end1, ni + 1) {
                        return Some(end);
                    }
                }
                self.match_at(text, pos, ni + 1)
            }
        }
    }

    fn match_single(&self, node: &ReNode, text: &[u8], pos: usize) -> Option<usize> {
        match node {
            ReNode::Literal(ch) => {
                if pos < text.len() && text[pos] == *ch {
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
                    let in_class = bytes.contains(&text[pos]);
                    let ok = if *negated { !in_class } else { in_class };
                    if ok {
                        Some(pos + 1)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Find all non-overlapping matches in `text`.
fn regex_find_all(re: &Regex, text: &[u8]) -> Vec<(usize, usize)> {
    let mut results = Vec::new();
    let mut start = 0;
    while start <= text.len() {
        if matches!(re.nodes.first(), Some(ReNode::StartAnchor)) {
            if start != 0 {
                break;
            }
            if let Some(end) = re.match_at(text, 0, 1) {
                results.push((0, end));
                start = if end > 0 { end } else { 1 };
            } else {
                break;
            }
            continue;
        }
        let mut found = false;
        for try_start in start..=text.len() {
            if let Some(end) = re.match_at(text, try_start, 0) {
                results.push((try_start, end));
                start = if end > try_start { end } else { try_start + 1 };
                found = true;
                break;
            }
        }
        if !found {
            break;
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    // Literals
    Number(f64),
    StringLit(String),
    Regex(String),
    // Identifiers & keywords
    Ident(String),
    Begin,
    End,
    If,
    Else,
    While,
    For,
    In,
    Do,
    Break,
    Continue,
    Next,
    Exit,
    Delete,
    Print,
    Printf,
    Getline,
    Function,
    Return,
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    PlusPlus,
    MinusMinus,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    Match,
    NotMatch,
    Question,
    Colon,
    Dollar,
    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Comma,
    Newline,
    // Append operator
    Append,
    Pipe,
    // End of input
    Eof,
}

struct Lexer {
    src: Vec<u8>,
    pos: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    fn new(src: &str) -> Self {
        Self {
            src: src.as_bytes().to_vec(),
            pos: 0,
            tokens: Vec::new(),
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip spaces and tabs (but not newlines -- they're significant).
            while let Some(b) = self.peek_byte() {
                if b == b' ' || b == b'\t' || b == b'\r' {
                    self.pos += 1;
                } else if b == b'\\' && self.src.get(self.pos + 1) == Some(&b'\n') {
                    // Line continuation
                    self.pos += 2;
                } else {
                    break;
                }
            }
            // Skip comments.
            if self.peek_byte() == Some(b'#') {
                while let Some(b) = self.peek_byte() {
                    if b == b'\n' {
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Determine whether a `/` at the current position should start a regex
    /// literal rather than a division operator.  Regex is expected after
    /// operators, keywords, and at the start of input -- basically any context
    /// where a primary expression is expected.
    fn expect_regex(&self) -> bool {
        match self.tokens.last() {
            None => true,
            Some(t) => matches!(
                t,
                Token::Newline
                    | Token::Semicolon
                    | Token::LBrace
                    | Token::LParen
                    | Token::Comma
                    | Token::Not
                    | Token::And
                    | Token::Or
                    | Token::Match
                    | Token::NotMatch
                    | Token::Assign
                    | Token::PlusAssign
                    | Token::MinusAssign
                    | Token::StarAssign
                    | Token::SlashAssign
                    | Token::PercentAssign
                    | Token::Plus
                    | Token::Minus
                    | Token::Star
                    | Token::Percent
                    | Token::Caret
                    | Token::Eq
                    | Token::Ne
                    | Token::Lt
                    | Token::Le
                    | Token::Gt
                    | Token::Ge
                    | Token::Question
                    | Token::Colon
                    | Token::Dollar
                    | Token::Print
                    | Token::Printf
                    | Token::Return
                    | Token::LBracket
                    | Token::Begin
                    | Token::End
            ),
        }
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        loop {
            self.skip_whitespace_and_comments();
            let Some(b) = self.peek_byte() else {
                self.tokens.push(Token::Eof);
                break;
            };

            match b {
                b'\n' => {
                    self.advance();
                    // Collapse multiple newlines.
                    if !matches!(
                        self.tokens.last(),
                        Some(Token::Newline) | Some(Token::Semicolon) | Some(Token::LBrace) | None
                    ) {
                        self.tokens.push(Token::Newline);
                    }
                }
                b'0'..=b'9' => self.read_number()?,
                b'"' => self.read_string()?,
                b'/' if self.expect_regex() => self.read_regex()?,
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.read_ident(),
                b'+' => {
                    self.advance();
                    if self.peek_byte() == Some(b'+') {
                        self.advance();
                        self.tokens.push(Token::PlusPlus);
                    } else if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::PlusAssign);
                    } else {
                        self.tokens.push(Token::Plus);
                    }
                }
                b'-' => {
                    self.advance();
                    if self.peek_byte() == Some(b'-') {
                        self.advance();
                        self.tokens.push(Token::MinusMinus);
                    } else if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::MinusAssign);
                    } else {
                        self.tokens.push(Token::Minus);
                    }
                }
                b'*' => {
                    self.advance();
                    if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::StarAssign);
                    } else {
                        self.tokens.push(Token::Star);
                    }
                }
                b'/' => {
                    self.advance();
                    if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::SlashAssign);
                    } else {
                        self.tokens.push(Token::Slash);
                    }
                }
                b'%' => {
                    self.advance();
                    if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::PercentAssign);
                    } else {
                        self.tokens.push(Token::Percent);
                    }
                }
                b'^' => {
                    self.advance();
                    self.tokens.push(Token::Caret);
                }
                b'=' => {
                    self.advance();
                    if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::Eq);
                    } else {
                        self.tokens.push(Token::Assign);
                    }
                }
                b'!' => {
                    self.advance();
                    if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::Ne);
                    } else if self.peek_byte() == Some(b'~') {
                        self.advance();
                        self.tokens.push(Token::NotMatch);
                    } else {
                        self.tokens.push(Token::Not);
                    }
                }
                b'<' => {
                    self.advance();
                    if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::Le);
                    } else {
                        self.tokens.push(Token::Lt);
                    }
                }
                b'>' => {
                    self.advance();
                    if self.peek_byte() == Some(b'>') {
                        self.advance();
                        self.tokens.push(Token::Append);
                    } else if self.peek_byte() == Some(b'=') {
                        self.advance();
                        self.tokens.push(Token::Ge);
                    } else {
                        self.tokens.push(Token::Gt);
                    }
                }
                b'&' => {
                    self.advance();
                    if self.peek_byte() == Some(b'&') {
                        self.advance();
                        self.tokens.push(Token::And);
                    } else {
                        return Err("unexpected '&' (use '&&' for logical AND)".into());
                    }
                }
                b'|' => {
                    self.advance();
                    if self.peek_byte() == Some(b'|') {
                        self.advance();
                        self.tokens.push(Token::Or);
                    } else {
                        self.tokens.push(Token::Pipe);
                    }
                }
                b'~' => {
                    self.advance();
                    self.tokens.push(Token::Match);
                }
                b'?' => {
                    self.advance();
                    self.tokens.push(Token::Question);
                }
                b':' => {
                    self.advance();
                    self.tokens.push(Token::Colon);
                }
                b'$' => {
                    self.advance();
                    self.tokens.push(Token::Dollar);
                }
                b'(' => {
                    self.advance();
                    self.tokens.push(Token::LParen);
                }
                b')' => {
                    self.advance();
                    self.tokens.push(Token::RParen);
                }
                b'{' => {
                    self.advance();
                    self.tokens.push(Token::LBrace);
                }
                b'}' => {
                    self.advance();
                    self.tokens.push(Token::RBrace);
                }
                b'[' => {
                    self.advance();
                    self.tokens.push(Token::LBracket);
                }
                b']' => {
                    self.advance();
                    self.tokens.push(Token::RBracket);
                }
                b';' => {
                    self.advance();
                    self.tokens.push(Token::Semicolon);
                }
                b',' => {
                    self.advance();
                    self.tokens.push(Token::Comma);
                }
                _ => {
                    return Err(format!(
                        "unexpected character: '{}'",
                        char::from(b)
                    ));
                }
            }
        }
        Ok(self.tokens.clone())
    }

    fn read_number(&mut self) -> Result<(), String> {
        let start = self.pos;
        // Integer part.
        if self.peek_byte() == Some(b'0')
            && matches!(self.src.get(self.pos + 1), Some(b'x') | Some(b'X'))
        {
            // Hexadecimal.
            self.pos += 2;
            while let Some(b) = self.peek_byte() {
                if b.is_ascii_hexdigit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let s = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
            let val = i64::from_str_radix(&s[2..], 16).map_err(|e| format!("bad hex: {e}"))?;
            self.tokens.push(Token::Number(val as f64));
            return Ok(());
        }
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        // Fractional part.
        if self.peek_byte() == Some(b'.') {
            self.pos += 1;
            while let Some(b) = self.peek_byte() {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        // Exponent.
        if matches!(self.peek_byte(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while let Some(b) = self.peek_byte() {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        let s = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
        let val: f64 = s.parse().map_err(|e: std::num::ParseFloatError| {
            format!("bad number '{s}': {e}")
        })?;
        self.tokens.push(Token::Number(val));
        Ok(())
    }

    fn read_string(&mut self) -> Result<(), String> {
        self.advance(); // skip opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err("unterminated string".into()),
                Some(b'"') => break,
                Some(b'\\') => match self.advance() {
                    None => return Err("unterminated string escape".into()),
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(b'r') => s.push('\r'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'"') => s.push('"'),
                    Some(b'/') => s.push('/'),
                    Some(b'a') => s.push('\x07'),
                    Some(b'b') => s.push('\x08'),
                    Some(b'f') => s.push('\x0c'),
                    Some(other) => {
                        s.push('\\');
                        s.push(char::from(other));
                    }
                },
                Some(ch) => s.push(char::from(ch)),
            }
        }
        self.tokens.push(Token::StringLit(s));
        Ok(())
    }

    fn read_regex(&mut self) -> Result<(), String> {
        self.advance(); // skip opening /
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err("unterminated regex".into()),
                Some(b'/') => break,
                Some(b'\\') => {
                    s.push('\\');
                    match self.advance() {
                        None => return Err("unterminated regex escape".into()),
                        Some(ch) => s.push(char::from(ch)),
                    }
                }
                Some(ch) => s.push(char::from(ch)),
            }
        }
        self.tokens.push(Token::Regex(s));
        Ok(())
    }

    fn read_ident(&mut self) {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let word = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
        let tok = match word.as_str() {
            "BEGIN" => Token::Begin,
            "END" => Token::End,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "do" => Token::Do,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "next" => Token::Next,
            "exit" => Token::Exit,
            "delete" => Token::Delete,
            "print" => Token::Print,
            "printf" => Token::Printf,
            "getline" => Token::Getline,
            "function" => Token::Function,
            "return" => Token::Return,
            _ => Token::Ident(word),
        };
        self.tokens.push(tok);
    }
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Number(f64),
    StringLit(String),
    Regex(String),
    Var(String),
    FieldAccess(Box<Expr>),
    ArrayRef(String, Vec<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(String, Box<Expr>, Box<Expr>),
    BinOp(String, Box<Expr>, Box<Expr>),
    UnaryOp(String, Box<Expr>),
    PreIncr(Box<Expr>),
    PreDecr(Box<Expr>),
    PostIncr(Box<Expr>),
    PostDecr(Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    MatchOp(Box<Expr>, Box<Expr>),
    NotMatchOp(Box<Expr>, Box<Expr>),
    Concat(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    InArray(String, Vec<Expr>),
    Getline,
}

#[derive(Debug, Clone)]
enum Stmt {
    ExprStmt(Expr),
    PrintStmt(Vec<Expr>, Option<Box<Expr>>),
    PrintfStmt(Vec<Expr>, Option<Box<Expr>>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Box<Stmt>>, Box<Stmt>),
    ForIn(String, String, Box<Stmt>),
    Next,
    Exit(Option<Expr>),
    Break,
    Continue,
    Delete(String, Vec<Expr>),
    Return(Option<Expr>),
}

#[derive(Debug, Clone)]
enum Pattern {
    Begin,
    End,
    Expression(Expr),
    Regex(String),
    All,
}

#[derive(Debug, Clone)]
struct Rule {
    pattern: Pattern,
    action: Vec<Stmt>,
}

#[derive(Debug, Clone)]
struct FuncDef {
    name: String,
    params: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
struct Program {
    rules: Vec<Rule>,
    functions: Vec<FuncDef>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let tok = self.advance();
        if &tok == expected {
            Ok(())
        } else {
            Err(format!("expected {expected:?}, got {tok:?}"))
        }
    }

    fn skip_terminators(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Semicolon) {
            self.advance();
        }
    }

    fn parse_program(&mut self) -> Result<Program, String> {
        let mut rules = Vec::new();
        let mut functions = Vec::new();
        self.skip_terminators();

        while *self.peek() != Token::Eof {
            if *self.peek() == Token::Function {
                functions.push(self.parse_func_def()?);
            } else {
                rules.push(self.parse_rule()?);
            }
            self.skip_terminators();
        }
        Ok(Program { rules, functions })
    }

    fn parse_func_def(&mut self) -> Result<FuncDef, String> {
        self.expect(&Token::Function)?;
        let name = match self.advance() {
            Token::Ident(n) => n,
            other => return Err(format!("expected function name, got {other:?}")),
        };
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != Token::RParen {
            loop {
                match self.advance() {
                    Token::Ident(p) => params.push(p),
                    other => return Err(format!("expected parameter name, got {other:?}")),
                }
                if *self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        self.skip_terminators();
        let body = self.parse_action()?;
        Ok(FuncDef { name, params, body })
    }

    fn parse_rule(&mut self) -> Result<Rule, String> {
        let pattern = match self.peek().clone() {
            Token::Begin => {
                self.advance();
                Pattern::Begin
            }
            Token::End => {
                self.advance();
                Pattern::End
            }
            Token::LBrace => Pattern::All,
            Token::Regex(r) => {
                self.advance();
                Pattern::Regex(r)
            }
            _ => {
                // Could be an expression pattern or just an action.
                // Try to parse expression; if next is `{`, it's a pattern.
                let expr = self.parse_expr()?;
                Pattern::Expression(expr)
            }
        };

        self.skip_terminators();

        let action = if *self.peek() == Token::LBrace {
            self.parse_action()?
        } else {
            // No explicit action: for expression/regex patterns, default is
            // "print $0"; for BEGIN/END with no braces, that is an error.
            match &pattern {
                Pattern::Begin | Pattern::End => {
                    return Err("BEGIN/END rule requires an action block".into());
                }
                _ => vec![Stmt::PrintStmt(vec![], None)],
            }
        };

        Ok(Rule { pattern, action })
    }

    fn parse_action(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Token::LBrace)?;
        self.skip_terminators();
        let mut stmts = Vec::new();
        while *self.peek() != Token::RBrace && *self.peek() != Token::Eof {
            stmts.push(self.parse_stmt()?);
            self.skip_terminators();
        }
        self.expect(&Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::Do => self.parse_do_while(),
            Token::For => self.parse_for(),
            Token::LBrace => {
                let stmts = self.parse_action()?;
                Ok(Stmt::Block(stmts))
            }
            Token::Print => self.parse_print(),
            Token::Printf => self.parse_printf(),
            Token::Next => {
                self.advance();
                Ok(Stmt::Next)
            }
            Token::Exit => {
                self.advance();
                if self.is_expr_start() {
                    let e = self.parse_expr()?;
                    Ok(Stmt::Exit(Some(e)))
                } else {
                    Ok(Stmt::Exit(None))
                }
            }
            Token::Break => {
                self.advance();
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                Ok(Stmt::Continue)
            }
            Token::Delete => self.parse_delete(),
            Token::Return => {
                self.advance();
                if self.is_expr_start() {
                    let e = self.parse_expr()?;
                    Ok(Stmt::Return(Some(e)))
                } else {
                    Ok(Stmt::Return(None))
                }
            }
            _ => {
                let e = self.parse_expr()?;
                Ok(Stmt::ExprStmt(e))
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::If)?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.skip_terminators();
        let body = self.parse_stmt()?;
        self.skip_terminators();
        let else_body = if *self.peek() == Token::Else {
            self.advance();
            self.skip_terminators();
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If(cond, Box::new(body), else_body))
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::While)?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.skip_terminators();
        let body = self.parse_stmt()?;
        Ok(Stmt::While(cond, Box::new(body)))
    }

    fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Do)?;
        self.skip_terminators();
        let body = self.parse_stmt()?;
        self.skip_terminators();
        self.expect(&Token::While)?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        Ok(Stmt::DoWhile(Box::new(body), cond))
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::For)?;
        self.expect(&Token::LParen)?;

        // Disambiguate `for (var in array)` vs `for (init; cond; incr)`.
        // Peek ahead: if second token is `in`, it's for-in.
        if let Token::Ident(var_name) = self.peek().clone() {
            let saved_pos = self.pos;
            self.advance(); // consume ident
            if *self.peek() == Token::In {
                self.advance(); // consume `in`
                let array_name = match self.advance() {
                    Token::Ident(n) => n,
                    other => return Err(format!("expected array name, got {other:?}")),
                };
                self.expect(&Token::RParen)?;
                self.skip_terminators();
                let body = self.parse_stmt()?;
                return Ok(Stmt::ForIn(var_name, array_name, Box::new(body)));
            }
            // Not for-in; backtrack.
            self.pos = saved_pos;
        }

        let init = if *self.peek() == Token::Semicolon {
            None
        } else {
            Some(Box::new(self.parse_stmt()?))
        };
        self.expect(&Token::Semicolon)?;
        let cond = if *self.peek() == Token::Semicolon {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&Token::Semicolon)?;
        let incr = if *self.peek() == Token::RParen {
            None
        } else {
            Some(Box::new(self.parse_stmt()?))
        };
        self.expect(&Token::RParen)?;
        self.skip_terminators();
        let body = self.parse_stmt()?;
        Ok(Stmt::For(init, cond, incr, Box::new(body)))
    }

    fn parse_print(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Print)?;
        let mut args = Vec::new();
        let mut dest = None;

        if self.is_expr_start() && *self.peek() != Token::Gt && *self.peek() != Token::Append
            && *self.peek() != Token::Pipe
        {
            args.push(self.parse_non_assign_expr()?);
            while *self.peek() == Token::Comma {
                self.advance();
                args.push(self.parse_non_assign_expr()?);
            }
        }

        // Output redirection: > file, >> file, | cmd
        if *self.peek() == Token::Gt || *self.peek() == Token::Append || *self.peek() == Token::Pipe
        {
            let _redir = self.advance();
            dest = Some(Box::new(self.parse_primary()?));
        }

        Ok(Stmt::PrintStmt(args, dest))
    }

    fn parse_printf(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Printf)?;
        let mut args = Vec::new();
        args.push(self.parse_non_assign_expr()?);
        while *self.peek() == Token::Comma {
            self.advance();
            args.push(self.parse_non_assign_expr()?);
        }

        let mut dest = None;
        if *self.peek() == Token::Gt || *self.peek() == Token::Append || *self.peek() == Token::Pipe
        {
            let _redir = self.advance();
            dest = Some(Box::new(self.parse_primary()?));
        }

        Ok(Stmt::PrintfStmt(args, dest))
    }

    fn parse_delete(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Delete)?;
        let name = match self.advance() {
            Token::Ident(n) => n,
            other => return Err(format!("expected array name after delete, got {other:?}")),
        };
        if *self.peek() == Token::LBracket {
            self.advance();
            let mut subscripts = vec![self.parse_expr()?];
            while *self.peek() == Token::Comma {
                self.advance();
                subscripts.push(self.parse_expr()?);
            }
            self.expect(&Token::RBracket)?;
            Ok(Stmt::Delete(name, subscripts))
        } else {
            // delete entire array
            Ok(Stmt::Delete(name, vec![]))
        }
    }

    fn is_expr_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Number(_)
                | Token::StringLit(_)
                | Token::Regex(_)
                | Token::Ident(_)
                | Token::Dollar
                | Token::LParen
                | Token::Minus
                | Token::Plus
                | Token::Not
                | Token::PlusPlus
                | Token::MinusMinus
                | Token::Getline
        )
    }

    // -- Expression parsing (precedence climbing) --

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_ternary()?;
        match self.peek().clone() {
            Token::Assign => {
                self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::Assign(Box::new(lhs), Box::new(rhs)))
            }
            Token::PlusAssign => {
                self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::OpAssign("+".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::MinusAssign => {
                self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::OpAssign("-".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::StarAssign => {
                self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::OpAssign("*".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::SlashAssign => {
                self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::OpAssign("/".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::PercentAssign => {
                self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::OpAssign("%".into(), Box::new(lhs), Box::new(rhs)))
            }
            _ => Ok(lhs),
        }
    }

    fn parse_ternary(&mut self) -> Result<Expr, String> {
        let cond = self.parse_or()?;
        if *self.peek() == Token::Question {
            self.advance();
            let then_expr = self.parse_assign()?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_assign()?;
            Ok(Expr::Ternary(
                Box::new(cond),
                Box::new(then_expr),
                Box::new(else_expr),
            ))
        } else {
            Ok(cond)
        }
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while *self.peek() == Token::Or {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr::BinOp("||".into(), Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_in_expr()?;
        while *self.peek() == Token::And {
            self.advance();
            let rhs = self.parse_in_expr()?;
            lhs = Expr::BinOp("&&".into(), Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_in_expr(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_match()?;
        if *self.peek() == Token::In {
            self.advance();
            let arr = match self.advance() {
                Token::Ident(n) => n,
                other => return Err(format!("expected array name after 'in', got {other:?}")),
            };
            // The key expression(s) for multi-dimensional arrays are inside `()`.
            // For single key: `(expr) in arr` or `expr in arr`.
            // We already parsed lhs which might be a parenthesized expression.
            let keys = match lhs {
                Expr::BinOp(ref op, _, _) if op == "," => {
                    // Won't actually happen with current parsing, but handle.
                    vec![lhs]
                }
                _ => vec![lhs],
            };
            return Ok(Expr::InArray(arr, keys));
        }
        Ok(lhs)
    }

    fn parse_match(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_comparison()?;
        match self.peek().clone() {
            Token::Match => {
                self.advance();
                let rhs = self.parse_primary()?;
                Ok(Expr::MatchOp(Box::new(lhs), Box::new(rhs)))
            }
            Token::NotMatch => {
                self.advance();
                let rhs = self.parse_primary()?;
                Ok(Expr::NotMatchOp(Box::new(lhs), Box::new(rhs)))
            }
            _ => Ok(lhs),
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_concat()?;
        match self.peek().clone() {
            Token::Lt => {
                self.advance();
                let rhs = self.parse_concat()?;
                Ok(Expr::BinOp("<".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::Le => {
                self.advance();
                let rhs = self.parse_concat()?;
                Ok(Expr::BinOp("<=".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::Gt => {
                self.advance();
                let rhs = self.parse_concat()?;
                Ok(Expr::BinOp(">".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::Ge => {
                self.advance();
                let rhs = self.parse_concat()?;
                Ok(Expr::BinOp(">=".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::Eq => {
                self.advance();
                let rhs = self.parse_concat()?;
                Ok(Expr::BinOp("==".into(), Box::new(lhs), Box::new(rhs)))
            }
            Token::Ne => {
                self.advance();
                let rhs = self.parse_concat()?;
                Ok(Expr::BinOp("!=".into(), Box::new(lhs), Box::new(rhs)))
            }
            _ => Ok(lhs),
        }
    }

    /// String concatenation: two adjacent expressions with no operator between
    /// them are concatenated.  This sits between comparison and addition in
    /// precedence.
    fn parse_concat(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_addition()?;
        // If the next token can start a primary and is not an operator, it's
        // concatenation.
        while self.is_concat_start() {
            let rhs = self.parse_addition()?;
            lhs = Expr::Concat(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn is_concat_start(&self) -> bool {
        // Concatenation happens when the next token can start a value but is
        // not an infix/postfix operator or terminator.
        matches!(
            self.peek(),
            Token::Number(_)
                | Token::StringLit(_)
                | Token::Dollar
                | Token::LParen
                | Token::Not
                | Token::PlusPlus
                | Token::MinusMinus
        ) || matches!(self.peek(), Token::Ident(_))
    }

    fn parse_addition(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_multiplication()?;
        loop {
            match self.peek() {
                Token::Plus => {
                    self.advance();
                    let rhs = self.parse_multiplication()?;
                    lhs = Expr::BinOp("+".into(), Box::new(lhs), Box::new(rhs));
                }
                Token::Minus => {
                    self.advance();
                    let rhs = self.parse_multiplication()?;
                    lhs = Expr::BinOp("-".into(), Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_power()?;
        loop {
            match self.peek() {
                Token::Star => {
                    self.advance();
                    let rhs = self.parse_power()?;
                    lhs = Expr::BinOp("*".into(), Box::new(lhs), Box::new(rhs));
                }
                Token::Slash => {
                    self.advance();
                    let rhs = self.parse_power()?;
                    lhs = Expr::BinOp("/".into(), Box::new(lhs), Box::new(rhs));
                }
                Token::Percent => {
                    self.advance();
                    let rhs = self.parse_power()?;
                    lhs = Expr::BinOp("%".into(), Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_power(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if *self.peek() == Token::Caret {
            self.advance();
            let exp = self.parse_power()?; // right-associative
            Ok(Expr::BinOp("^".into(), Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Not => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::UnaryOp("!".into(), Box::new(e)))
            }
            Token::Minus => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::UnaryOp("-".into(), Box::new(e)))
            }
            Token::Plus => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::UnaryOp("+".into(), Box::new(e)))
            }
            Token::PlusPlus => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::PreIncr(Box::new(e)))
            }
            Token::MinusMinus => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::PreDecr(Box::new(e)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::PlusPlus => {
                    self.advance();
                    e = Expr::PostIncr(Box::new(e));
                }
                Token::MinusMinus => {
                    self.advance();
                    e = Expr::PostDecr(Box::new(e));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::StringLit(s) => {
                self.advance();
                Ok(Expr::StringLit(s))
            }
            Token::Regex(r) => {
                self.advance();
                Ok(Expr::Regex(r))
            }
            Token::Dollar => {
                self.advance();
                let e = self.parse_primary()?;
                Ok(Expr::FieldAccess(Box::new(e)))
            }
            Token::LParen => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            Token::Getline => {
                self.advance();
                Ok(Expr::Getline)
            }
            Token::Ident(name) => {
                self.advance();
                if *self.peek() == Token::LParen {
                    // Function call.
                    self.advance();
                    let mut args = Vec::new();
                    if *self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while *self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Call(name, args))
                } else if *self.peek() == Token::LBracket {
                    // Array subscript.
                    self.advance();
                    let mut subscripts = vec![self.parse_expr()?];
                    while *self.peek() == Token::Comma {
                        self.advance();
                        subscripts.push(self.parse_expr()?);
                    }
                    self.expect(&Token::RBracket)?;
                    Ok(Expr::ArrayRef(name, subscripts))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(format!("unexpected token in expression: {other:?}")),
        }
    }

    /// Parse an expression that does NOT include top-level assignment.
    /// Used in print argument lists so that `print a = 5` is not parsed
    /// as `print (a = 5)`.
    fn parse_non_assign_expr(&mut self) -> Result<Expr, String> {
        self.parse_ternary()
    }
}

// ---------------------------------------------------------------------------
// Values (dynamically typed: number or string, auto-converting)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Value {
    Num(f64),
    Str(String),
    /// Uninitialized -- behaves as 0 or "" depending on context.
    Uninit,
}

impl Value {
    fn to_num(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Str(s) => parse_leading_number(s),
            Value::Uninit => 0.0,
        }
    }

    fn to_str(&self) -> String {
        match self {
            Value::Num(n) => format_number(*n),
            Value::Str(s) => s.clone(),
            Value::Uninit => String::new(),
        }
    }

    fn is_true(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Uninit => false,
        }
    }
}

fn format_number(n: f64) -> String {
    if n == n.floor() && n.abs() < 1e16 && !n.is_infinite() {
        // Print as integer when it is one.
        format!("{}", n as i64)
    } else {
        // OFMT default: "%.6g"
        format!("{:.6}", n)
    }
}

fn parse_leading_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    // Try to parse the longest prefix that looks numeric.
    let mut end = 0;
    let bytes = s.as_bytes();
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
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
    if end == 0 {
        return 0.0;
    }
    s[..end].parse::<f64>().unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

/// Signal returned from statement evaluation to alter control flow.
enum ControlFlow {
    None,
    Next,
    Break,
    Continue,
    Exit(i32),
    Return(Value),
}

struct Interpreter {
    /// Global variables.
    globals: HashMap<String, Value>,
    /// Associative arrays (global scope).  Key is SUBSEP-joined string.
    arrays: HashMap<String, HashMap<String, Value>>,
    /// Field values: fields[0] = $0, fields[1] = $1, etc.
    fields: Vec<String>,
    /// User-defined functions.
    functions: HashMap<String, FuncDef>,
    /// Random seed state (simple LCG).
    rng_state: u64,
    /// Current filename.
    filename: String,
    /// NR (total records read).
    nr: usize,
    /// FNR (records in current file).
    fnr: usize,
}

impl Interpreter {
    fn new(argv: Vec<String>) -> Self {
        let mut globals = HashMap::new();
        globals.insert("FS".into(), Value::Str(" ".into()));
        globals.insert("RS".into(), Value::Str("\n".into()));
        globals.insert("OFS".into(), Value::Str(" ".into()));
        globals.insert("ORS".into(), Value::Str("\n".into()));
        globals.insert("NR".into(), Value::Num(0.0));
        globals.insert("NF".into(), Value::Num(0.0));
        globals.insert("FNR".into(), Value::Num(0.0));
        globals.insert("SUBSEP".into(), Value::Str("\x1c".into()));
        globals.insert("FILENAME".into(), Value::Uninit);
        globals.insert("ARGC".into(), Value::Num(argv.len() as f64));

        let mut arrays = HashMap::new();
        let mut argv_map = HashMap::new();
        for (i, a) in argv.iter().enumerate() {
            argv_map.insert(i.to_string(), Value::Str(a.clone()));
        }
        arrays.insert("ARGV".into(), argv_map);

        Self {
            globals,
            arrays,
            fields: vec![String::new()],
            functions: HashMap::new(),
            rng_state: 12345,
            filename: String::new(),
            nr: 0,
            fnr: 0,
        }
    }

    fn set_var(&mut self, name: &str, val: Value) {
        self.globals.insert(name.into(), val);
    }

    fn get_var(&self, name: &str) -> Value {
        self.globals
            .get(name)
            .cloned()
            .unwrap_or(Value::Uninit)
    }

    fn set_record(&mut self, line: &str) {
        self.fields.clear();
        self.fields.push(line.into()); // $0

        let fs = self.get_var("FS").to_str();
        let parts: Vec<&str> = if fs == " " {
            // Default FS: split on runs of whitespace, strip leading/trailing.
            line.split_whitespace().collect()
        } else if fs.len() == 1 {
            line.split(fs.as_bytes()[0] as char).collect()
        } else {
            // Multi-char FS: treat as regex.
            match compile_regex(fs.as_bytes()) {
                Ok(re) => regex_split(&re, line),
                Err(_) => line.split_whitespace().collect(),
            }
        };

        for p in &parts {
            self.fields.push((*p).to_string());
        }
        self.globals
            .insert("NF".into(), Value::Num((self.fields.len() - 1) as f64));
    }

    fn get_field(&self, idx: usize) -> String {
        if idx < self.fields.len() {
            self.fields[idx].clone()
        } else {
            String::new()
        }
    }

    fn set_field(&mut self, idx: usize, val: &str) {
        while self.fields.len() <= idx {
            self.fields.push(String::new());
        }
        self.fields[idx] = val.to_string();
        if idx == 0 {
            // Re-split.
            let line = val.to_string();
            self.set_record(&line);
        } else {
            // Rebuild $0.
            let ofs = self.get_var("OFS").to_str();
            let rebuilt: String = self.fields[1..].join(&ofs);
            self.fields[0] = rebuilt;
            self.globals
                .insert("NF".into(), Value::Num((self.fields.len() - 1) as f64));
        }
    }

    fn run(
        &mut self,
        program: &Program,
        input_files: &[String],
        out: &mut dyn Write,
    ) -> Result<i32, String> {
        for f in &program.functions {
            self.functions.insert(f.name.clone(), f.clone());
        }

        // Run BEGIN rules.
        for rule in &program.rules {
            if matches!(rule.pattern, Pattern::Begin) {
                match self.exec_stmts(&rule.action, out)? {
                    ControlFlow::Exit(code) => return Ok(code),
                    _ => {}
                }
            }
        }

        // Process input.
        let result = if input_files.is_empty() {
            self.process_reader(io::stdin().lock(), "<stdin>", program, out)
        } else {
            let mut code = None;
            for path in input_files {
                self.fnr = 0;
                self.filename = path.clone();
                self.globals
                    .insert("FILENAME".into(), Value::Str(path.clone()));
                if path == "-" {
                    match self.process_reader(io::stdin().lock(), "-", program, out)? {
                        Some(c) => {
                            code = Some(c);
                            break;
                        }
                        None => {}
                    }
                } else {
                    let file = fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
                    let reader = io::BufReader::new(file);
                    match self.process_reader(reader, path, program, out)? {
                        Some(c) => {
                            code = Some(c);
                            break;
                        }
                        None => {}
                    }
                }
            }
            Ok(code)
        };

        let exit_code = match result {
            Ok(Some(c)) => c,
            Ok(None) => 0,
            Err(e) => return Err(e),
        };

        // Run END rules.
        for rule in &program.rules {
            if matches!(rule.pattern, Pattern::End) {
                match self.exec_stmts(&rule.action, out)? {
                    ControlFlow::Exit(code) => return Ok(code),
                    _ => {}
                }
            }
        }

        Ok(exit_code)
    }

    fn process_reader<R: BufRead>(
        &mut self,
        reader: R,
        filename: &str,
        program: &Program,
        out: &mut dyn Write,
    ) -> Result<Option<i32>, String> {
        self.filename = filename.into();
        let rs = self.get_var("RS").to_str();

        if rs == "\n" || rs.is_empty() {
            // Line-by-line reading (common case).
            for line_result in reader.lines() {
                let line = line_result.map_err(|e| e.to_string())?;
                self.nr += 1;
                self.fnr += 1;
                self.globals.insert("NR".into(), Value::Num(self.nr as f64));
                self.globals
                    .insert("FNR".into(), Value::Num(self.fnr as f64));
                self.set_record(&line);

                match self.run_main_rules(program, out)? {
                    ControlFlow::Exit(code) => return Ok(Some(code)),
                    ControlFlow::Next => continue,
                    _ => {}
                }
            }
        } else {
            // Custom RS: read all content and split on RS.
            let mut content = String::new();
            let mut buf_reader = reader;
            buf_reader
                .read_to_string(&mut content)
                .map_err(|e| e.to_string())?;
            let sep = if rs.len() == 1 {
                rs.clone()
            } else {
                rs
            };
            let records: Vec<&str> = content.split(&sep).collect();
            for rec in records {
                if rec.is_empty() && sep.len() <= 1 {
                    continue;
                }
                self.nr += 1;
                self.fnr += 1;
                self.globals.insert("NR".into(), Value::Num(self.nr as f64));
                self.globals
                    .insert("FNR".into(), Value::Num(self.fnr as f64));
                self.set_record(rec);

                match self.run_main_rules(program, out)? {
                    ControlFlow::Exit(code) => return Ok(Some(code)),
                    ControlFlow::Next => continue,
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn run_main_rules(
        &mut self,
        program: &Program,
        out: &mut dyn Write,
    ) -> Result<ControlFlow, String> {
        for rule in &program.rules {
            match &rule.pattern {
                Pattern::Begin | Pattern::End => continue,
                Pattern::All => {}
                Pattern::Regex(r) => {
                    let re = compile_regex(r.as_bytes())?;
                    if !re.is_match(self.fields[0].as_bytes()) {
                        continue;
                    }
                }
                Pattern::Expression(expr) => {
                    let val = self.eval_expr(expr, out)?;
                    if !val.is_true() {
                        continue;
                    }
                }
            }
            match self.exec_stmts(&rule.action, out)? {
                ControlFlow::Next => return Ok(ControlFlow::Next),
                ControlFlow::Exit(code) => return Ok(ControlFlow::Exit(code)),
                _ => {}
            }
        }
        Ok(ControlFlow::None)
    }

    fn exec_stmts(
        &mut self,
        stmts: &[Stmt],
        out: &mut dyn Write,
    ) -> Result<ControlFlow, String> {
        for stmt in stmts {
            match self.exec_stmt(stmt, out)? {
                ControlFlow::None => {}
                other => return Ok(other),
            }
        }
        Ok(ControlFlow::None)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, out: &mut dyn Write) -> Result<ControlFlow, String> {
        match stmt {
            Stmt::ExprStmt(e) => {
                self.eval_expr(e, out)?;
                Ok(ControlFlow::None)
            }
            Stmt::PrintStmt(args, _dest) => {
                // Output redirection is not fully implemented (would need
                // file/pipe management).  All output goes to `out`.
                let ofs = self.get_var("OFS").to_str();
                let ors = self.get_var("ORS").to_str();
                if args.is_empty() {
                    let line = self.get_field(0);
                    write!(out, "{line}").map_err(|e| e.to_string())?;
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(out, "{ofs}").map_err(|e| e.to_string())?;
                        }
                        let val = self.eval_expr(arg, out)?;
                        write!(out, "{}", val.to_str()).map_err(|e| e.to_string())?;
                    }
                }
                write!(out, "{ors}").map_err(|e| e.to_string())?;
                Ok(ControlFlow::None)
            }
            Stmt::PrintfStmt(args, _dest) => {
                if args.is_empty() {
                    return Ok(ControlFlow::None);
                }
                let fmt_val = self.eval_expr(&args[0], out)?;
                let fmt = fmt_val.to_str();
                let arg_vals: Vec<Value> = args[1..]
                    .iter()
                    .map(|a| self.eval_expr(a, out))
                    .collect::<Result<_, _>>()?;
                let result = self.sprintf_impl(&fmt, &arg_vals);
                write!(out, "{result}").map_err(|e| e.to_string())?;
                Ok(ControlFlow::None)
            }
            Stmt::Block(stmts) => self.exec_stmts(stmts, out),
            Stmt::If(cond, body, else_body) => {
                let val = self.eval_expr(cond, out)?;
                if val.is_true() {
                    self.exec_stmt(body, out)
                } else if let Some(eb) = else_body {
                    self.exec_stmt(eb, out)
                } else {
                    Ok(ControlFlow::None)
                }
            }
            Stmt::While(cond, body) => loop {
                let val = self.eval_expr(cond, out)?;
                if !val.is_true() {
                    break Ok(ControlFlow::None);
                }
                match self.exec_stmt(body, out)? {
                    ControlFlow::Break => break Ok(ControlFlow::None),
                    ControlFlow::Continue => continue,
                    ControlFlow::None => {}
                    other => break Ok(other),
                }
            },
            Stmt::DoWhile(body, cond) => loop {
                match self.exec_stmt(body, out)? {
                    ControlFlow::Break => break Ok(ControlFlow::None),
                    ControlFlow::Continue => {}
                    ControlFlow::None => {}
                    other => break Ok(other),
                }
                let val = self.eval_expr(cond, out)?;
                if !val.is_true() {
                    break Ok(ControlFlow::None);
                }
            },
            Stmt::For(init, cond, incr, body) => {
                if let Some(init_stmt) = init {
                    self.exec_stmt(init_stmt, out)?;
                }
                loop {
                    if let Some(cond_expr) = cond {
                        let val = self.eval_expr(cond_expr, out)?;
                        if !val.is_true() {
                            break;
                        }
                    }
                    match self.exec_stmt(body, out)? {
                        ControlFlow::Break => break,
                        ControlFlow::Continue => {}
                        ControlFlow::None => {}
                        other => return Ok(other),
                    }
                    if let Some(incr_stmt) = incr {
                        self.exec_stmt(incr_stmt, out)?;
                    }
                }
                Ok(ControlFlow::None)
            }
            Stmt::ForIn(var, arr, body) => {
                let keys: Vec<String> = self
                    .arrays
                    .get(arr)
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                for key in keys {
                    self.set_var(var, Value::Str(key));
                    match self.exec_stmt(body, out)? {
                        ControlFlow::Break => break,
                        ControlFlow::Continue => continue,
                        ControlFlow::None => {}
                        other => return Ok(other),
                    }
                }
                Ok(ControlFlow::None)
            }
            Stmt::Next => Ok(ControlFlow::Next),
            Stmt::Exit(code_expr) => {
                let code = if let Some(e) = code_expr {
                    self.eval_expr(e, out)?.to_num() as i32
                } else {
                    0
                };
                Ok(ControlFlow::Exit(code))
            }
            Stmt::Break => Ok(ControlFlow::Break),
            Stmt::Continue => Ok(ControlFlow::Continue),
            Stmt::Delete(name, subscripts) => {
                if subscripts.is_empty() {
                    self.arrays.remove(name);
                } else {
                    let key = self.build_array_key(subscripts, out)?;
                    if let Some(map) = self.arrays.get_mut(name) {
                        map.remove(&key);
                    }
                }
                Ok(ControlFlow::None)
            }
            Stmt::Return(expr) => {
                let val = if let Some(e) = expr {
                    self.eval_expr(e, out)?
                } else {
                    Value::Uninit
                };
                Ok(ControlFlow::Return(val))
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr, out: &mut dyn Write) -> Result<Value, String> {
        match expr {
            Expr::Number(n) => Ok(Value::Num(*n)),
            Expr::StringLit(s) => Ok(Value::Str(s.clone())),
            Expr::Regex(r) => {
                // Bare /regex/ in expression context matches against $0.
                let re = compile_regex(r.as_bytes())?;
                let matched = re.is_match(self.fields[0].as_bytes());
                Ok(Value::Num(if matched { 1.0 } else { 0.0 }))
            }
            Expr::Var(name) => Ok(self.get_var(name)),
            Expr::FieldAccess(idx_expr) => {
                let idx = self.eval_expr(idx_expr, out)?.to_num() as usize;
                Ok(Value::Str(self.get_field(idx)))
            }
            Expr::ArrayRef(name, subscripts) => {
                let key = self.build_array_key(subscripts, out)?;
                let val = self
                    .arrays
                    .get(name)
                    .and_then(|m| m.get(&key))
                    .cloned()
                    .unwrap_or(Value::Uninit);
                Ok(val)
            }
            Expr::Assign(lhs, rhs) => {
                let val = self.eval_expr(rhs, out)?;
                self.assign_to(lhs, &val, out)?;
                Ok(val)
            }
            Expr::OpAssign(op, lhs, rhs) => {
                let lval = self.eval_expr(lhs, out)?;
                let rval = self.eval_expr(rhs, out)?;
                let result = self.apply_arith(op, &lval, &rval);
                self.assign_to(lhs, &result, out)?;
                Ok(result)
            }
            Expr::BinOp(op, lhs, rhs) => {
                let lval = self.eval_expr(lhs, out)?;
                match op.as_str() {
                    "&&" => {
                        if !lval.is_true() {
                            return Ok(Value::Num(0.0));
                        }
                        let rval = self.eval_expr(rhs, out)?;
                        Ok(Value::Num(if rval.is_true() { 1.0 } else { 0.0 }))
                    }
                    "||" => {
                        if lval.is_true() {
                            return Ok(Value::Num(1.0));
                        }
                        let rval = self.eval_expr(rhs, out)?;
                        Ok(Value::Num(if rval.is_true() { 1.0 } else { 0.0 }))
                    }
                    _ => {
                        let rval = self.eval_expr(rhs, out)?;
                        match op.as_str() {
                            "+" | "-" | "*" | "/" | "%" | "^" => {
                                Ok(self.apply_arith(op, &lval, &rval))
                            }
                            "<" | "<=" | ">" | ">=" | "==" | "!=" => {
                                Ok(self.apply_comparison(op, &lval, &rval))
                            }
                            _ => Err(format!("unknown binary operator: {op}")),
                        }
                    }
                }
            }
            Expr::UnaryOp(op, inner) => {
                let val = self.eval_expr(inner, out)?;
                match op.as_str() {
                    "-" => Ok(Value::Num(-val.to_num())),
                    "+" => Ok(Value::Num(val.to_num())),
                    "!" => Ok(Value::Num(if val.is_true() { 0.0 } else { 1.0 })),
                    _ => Err(format!("unknown unary operator: {op}")),
                }
            }
            Expr::PreIncr(inner) => {
                let val = self.eval_expr(inner, out)?.to_num() + 1.0;
                let result = Value::Num(val);
                self.assign_to(inner, &result, out)?;
                Ok(result)
            }
            Expr::PreDecr(inner) => {
                let val = self.eval_expr(inner, out)?.to_num() - 1.0;
                let result = Value::Num(val);
                self.assign_to(inner, &result, out)?;
                Ok(result)
            }
            Expr::PostIncr(inner) => {
                let old = self.eval_expr(inner, out)?.to_num();
                let new_val = Value::Num(old + 1.0);
                self.assign_to(inner, &new_val, out)?;
                Ok(Value::Num(old))
            }
            Expr::PostDecr(inner) => {
                let old = self.eval_expr(inner, out)?.to_num();
                let new_val = Value::Num(old - 1.0);
                self.assign_to(inner, &new_val, out)?;
                Ok(Value::Num(old))
            }
            Expr::Ternary(cond, then_e, else_e) => {
                let c = self.eval_expr(cond, out)?;
                if c.is_true() {
                    self.eval_expr(then_e, out)
                } else {
                    self.eval_expr(else_e, out)
                }
            }
            Expr::MatchOp(lhs, rhs) => {
                let s = self.eval_expr(lhs, out)?.to_str();
                let pat = self.expr_to_regex_pattern(rhs, out)?;
                let re = compile_regex(pat.as_bytes())?;
                Ok(Value::Num(if re.is_match(s.as_bytes()) {
                    1.0
                } else {
                    0.0
                }))
            }
            Expr::NotMatchOp(lhs, rhs) => {
                let s = self.eval_expr(lhs, out)?.to_str();
                let pat = self.expr_to_regex_pattern(rhs, out)?;
                let re = compile_regex(pat.as_bytes())?;
                Ok(Value::Num(if re.is_match(s.as_bytes()) {
                    0.0
                } else {
                    1.0
                }))
            }
            Expr::Concat(lhs, rhs) => {
                let ls = self.eval_expr(lhs, out)?.to_str();
                let rs = self.eval_expr(rhs, out)?.to_str();
                Ok(Value::Str(format!("{ls}{rs}")))
            }
            Expr::InArray(arr, keys) => {
                let key = self.build_array_key(keys, out)?;
                let exists = self
                    .arrays
                    .get(arr)
                    .map_or(false, |m| m.contains_key(&key));
                Ok(Value::Num(if exists { 1.0 } else { 0.0 }))
            }
            Expr::Call(name, args) => self.call_function(name, args, out),
            Expr::Getline => {
                // Simple getline: read next line from stdin.
                let mut line = String::new();
                match io::stdin().lock().read_line(&mut line) {
                    Ok(0) => Ok(Value::Num(0.0)),
                    Ok(_) => {
                        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                        self.set_record(trimmed);
                        self.nr += 1;
                        self.fnr += 1;
                        self.globals.insert("NR".into(), Value::Num(self.nr as f64));
                        self.globals
                            .insert("FNR".into(), Value::Num(self.fnr as f64));
                        Ok(Value::Num(1.0))
                    }
                    Err(_) => Ok(Value::Num(-1.0)),
                }
            }
        }
    }

    fn expr_to_regex_pattern(&mut self, expr: &Expr, out: &mut dyn Write) -> Result<String, String> {
        match expr {
            Expr::Regex(r) => Ok(r.clone()),
            _ => Ok(self.eval_expr(expr, out)?.to_str()),
        }
    }

    fn assign_to(&mut self, lhs: &Expr, val: &Value, out: &mut dyn Write) -> Result<(), String> {
        match lhs {
            Expr::Var(name) => {
                self.set_var(name, val.clone());
            }
            Expr::FieldAccess(idx_expr) => {
                let idx = self.eval_expr(idx_expr, out)?.to_num() as usize;
                self.set_field(idx, &val.to_str());
            }
            Expr::ArrayRef(name, subscripts) => {
                let key = self.build_array_key(subscripts, out)?;
                self.arrays
                    .entry(name.clone())
                    .or_default()
                    .insert(key, val.clone());
            }
            _ => {
                // Assignments to non-lvalues silently succeed (matches some
                // awk implementations for expressions like `++expr`).
            }
        }
        Ok(())
    }

    fn build_array_key(
        &mut self,
        subscripts: &[Expr],
        out: &mut dyn Write,
    ) -> Result<String, String> {
        let subsep = self.get_var("SUBSEP").to_str();
        let mut parts = Vec::new();
        for s in subscripts {
            parts.push(self.eval_expr(s, out)?.to_str());
        }
        Ok(parts.join(&subsep))
    }

    fn apply_arith(&self, op: &str, lhs: &Value, rhs: &Value) -> Value {
        let a = lhs.to_num();
        let b = rhs.to_num();
        let result = match op {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => {
                if b == 0.0 {
                    0.0 // awk typically returns 0 for division by zero
                } else {
                    a / b
                }
            }
            "%" => {
                if b == 0.0 {
                    0.0
                } else {
                    a % b
                }
            }
            "^" => a.powf(b),
            _ => 0.0,
        };
        Value::Num(result)
    }

    fn apply_comparison(&self, op: &str, lhs: &Value, rhs: &Value) -> Value {
        // If both sides look numeric, compare as numbers; otherwise as strings.
        let numeric = matches!(
            (lhs, rhs),
            (Value::Num(_), _) | (_, Value::Num(_))
        );
        let result = if numeric {
            let a = lhs.to_num();
            let b = rhs.to_num();
            match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                ">=" => a >= b,
                "==" => (a - b).abs() < f64::EPSILON,
                "!=" => (a - b).abs() >= f64::EPSILON,
                _ => false,
            }
        } else {
            let a = lhs.to_str();
            let b = rhs.to_str();
            match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                ">=" => a >= b,
                "==" => a == b,
                "!=" => a != b,
                _ => false,
            }
        };
        Value::Num(if result { 1.0 } else { 0.0 })
    }

    // -- Built-in functions --

    fn call_function(
        &mut self,
        name: &str,
        args: &[Expr],
        out: &mut dyn Write,
    ) -> Result<Value, String> {
        match name {
            "length" => {
                let s = if args.is_empty() {
                    self.get_field(0)
                } else {
                    self.eval_expr(&args[0], out)?.to_str()
                };
                Ok(Value::Num(s.len() as f64))
            }
            "substr" => {
                if args.is_empty() {
                    return Err("substr requires at least 2 arguments".into());
                }
                let s = self.eval_expr(&args[0], out)?.to_str();
                let start = if args.len() > 1 {
                    self.eval_expr(&args[1], out)?.to_num() as i64
                } else {
                    1
                };
                // AWK substr is 1-based.
                let start_idx = if start < 1 { 0 } else { (start - 1) as usize };
                let chars: Vec<char> = s.chars().collect();
                let len = if args.len() > 2 {
                    let l = self.eval_expr(&args[2], out)?.to_num() as usize;
                    l
                } else {
                    chars.len().saturating_sub(start_idx)
                };
                let end_idx = (start_idx + len).min(chars.len());
                let result: String = chars
                    .get(start_idx..end_idx)
                    .unwrap_or(&[])
                    .iter()
                    .collect();
                Ok(Value::Str(result))
            }
            "index" => {
                if args.len() < 2 {
                    return Err("index requires 2 arguments".into());
                }
                let s = self.eval_expr(&args[0], out)?.to_str();
                let target = self.eval_expr(&args[1], out)?.to_str();
                let pos = s.find(&target).map_or(0, |i| i + 1);
                Ok(Value::Num(pos as f64))
            }
            "split" => {
                if args.len() < 2 {
                    return Err("split requires at least 2 arguments".into());
                }
                let s = self.eval_expr(&args[0], out)?.to_str();
                let arr_name = match &args[1] {
                    Expr::Var(n) | Expr::ArrayRef(n, _) => n.clone(),
                    _ => return Err("split: second argument must be an array name".into()),
                };
                let sep = if args.len() > 2 {
                    self.eval_expr(&args[2], out)?.to_str()
                } else {
                    self.get_var("FS").to_str()
                };

                // Clear the array.
                self.arrays.remove(&arr_name);
                let parts: Vec<&str> = if sep == " " {
                    s.split_whitespace().collect()
                } else if sep.len() == 1 {
                    s.split(sep.as_bytes()[0] as char).collect()
                } else {
                    match compile_regex(sep.as_bytes()) {
                        Ok(re) => regex_split(&re, &s),
                        Err(_) => s.split_whitespace().collect(),
                    }
                };
                let mut map = HashMap::new();
                for (i, p) in parts.iter().enumerate() {
                    map.insert((i + 1).to_string(), Value::Str((*p).to_string()));
                }
                let count = parts.len();
                self.arrays.insert(arr_name, map);
                Ok(Value::Num(count as f64))
            }
            "sub" => {
                if args.len() < 2 {
                    return Err("sub requires at least 2 arguments".into());
                }
                let pat = self.expr_to_regex_pattern(&args[0], out)?;
                let repl = self.eval_expr(&args[1], out)?.to_str();
                let re = compile_regex(pat.as_bytes())?;
                // Target defaults to $0.
                let target = if args.len() > 2 {
                    self.eval_expr(&args[2], out)?.to_str()
                } else {
                    self.get_field(0)
                };
                let (result, count) = regex_sub(&re, &repl, &target, false);
                if args.len() > 2 {
                    self.assign_to(&args[2], &Value::Str(result), out)?;
                } else {
                    self.set_field(0, &result);
                }
                Ok(Value::Num(count as f64))
            }
            "gsub" => {
                if args.len() < 2 {
                    return Err("gsub requires at least 2 arguments".into());
                }
                let pat = self.expr_to_regex_pattern(&args[0], out)?;
                let repl = self.eval_expr(&args[1], out)?.to_str();
                let re = compile_regex(pat.as_bytes())?;
                let target = if args.len() > 2 {
                    self.eval_expr(&args[2], out)?.to_str()
                } else {
                    self.get_field(0)
                };
                let (result, count) = regex_sub(&re, &repl, &target, true);
                if args.len() > 2 {
                    self.assign_to(&args[2], &Value::Str(result), out)?;
                } else {
                    self.set_field(0, &result);
                }
                Ok(Value::Num(count as f64))
            }
            "match" => {
                if args.len() < 2 {
                    return Err("match requires 2 arguments".into());
                }
                let s = self.eval_expr(&args[0], out)?.to_str();
                let pat = self.expr_to_regex_pattern(&args[1], out)?;
                let re = compile_regex(pat.as_bytes())?;
                match re.find(s.as_bytes()) {
                    Some((start, end)) => {
                        let pos = start + 1; // 1-based
                        self.set_var("RSTART", Value::Num(pos as f64));
                        self.set_var("RLENGTH", Value::Num((end - start) as f64));
                        Ok(Value::Num(pos as f64))
                    }
                    None => {
                        self.set_var("RSTART", Value::Num(0.0));
                        self.set_var("RLENGTH", Value::Num(-1.0));
                        Ok(Value::Num(0.0))
                    }
                }
            }
            "tolower" => {
                let s = if args.is_empty() {
                    self.get_field(0)
                } else {
                    self.eval_expr(&args[0], out)?.to_str()
                };
                Ok(Value::Str(s.to_lowercase()))
            }
            "toupper" => {
                let s = if args.is_empty() {
                    self.get_field(0)
                } else {
                    self.eval_expr(&args[0], out)?.to_str()
                };
                Ok(Value::Str(s.to_uppercase()))
            }
            "sprintf" => {
                if args.is_empty() {
                    return Ok(Value::Str(String::new()));
                }
                let fmt = self.eval_expr(&args[0], out)?.to_str();
                let arg_vals: Vec<Value> = args[1..]
                    .iter()
                    .map(|a| self.eval_expr(a, out))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Str(self.sprintf_impl(&fmt, &arg_vals)))
            }
            "int" => {
                let val = if args.is_empty() {
                    self.get_field(0).parse::<f64>().unwrap_or(0.0)
                } else {
                    self.eval_expr(&args[0], out)?.to_num()
                };
                Ok(Value::Num(val.trunc()))
            }
            "sqrt" => {
                let val = self.eval_one_num_arg(args, out)?;
                Ok(Value::Num(val.sqrt()))
            }
            "sin" => {
                let val = self.eval_one_num_arg(args, out)?;
                Ok(Value::Num(val.sin()))
            }
            "cos" => {
                let val = self.eval_one_num_arg(args, out)?;
                Ok(Value::Num(val.cos()))
            }
            "log" => {
                let val = self.eval_one_num_arg(args, out)?;
                Ok(Value::Num(val.ln()))
            }
            "exp" => {
                let val = self.eval_one_num_arg(args, out)?;
                Ok(Value::Num(val.exp()))
            }
            "rand" => {
                // Simple LCG pseudo-random number generator.
                self.rng_state = self
                    .rng_state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let val = (self.rng_state >> 33) as f64 / (1u64 << 31) as f64;
                Ok(Value::Num(val))
            }
            "srand" => {
                let old_seed = self.rng_state;
                if args.is_empty() {
                    // Seed from a simple time-based source; just use NR + a
                    // constant since we lack a good entropy source.
                    self.rng_state = (self.nr as u64).wrapping_add(0xdeadbeef);
                } else {
                    self.rng_state = self.eval_expr(&args[0], out)?.to_num() as u64;
                }
                Ok(Value::Num(old_seed as f64))
            }
            _ => {
                // User-defined function.
                if let Some(func) = self.functions.get(name).cloned() {
                    let mut arg_vals = Vec::new();
                    for a in args {
                        arg_vals.push(self.eval_expr(a, out)?);
                    }
                    // Set up local scope: parameters shadow globals.
                    let mut saved = HashMap::new();
                    for (i, param) in func.params.iter().enumerate() {
                        if let Some(old) = self.globals.remove(param) {
                            saved.insert(param.clone(), old);
                        }
                        let val = arg_vals.get(i).cloned().unwrap_or(Value::Uninit);
                        self.globals.insert(param.clone(), val);
                    }
                    let result = match self.exec_stmts(&func.body, out)? {
                        ControlFlow::Return(v) => v,
                        _ => Value::Uninit,
                    };
                    // Restore globals.
                    for param in &func.params {
                        self.globals.remove(param);
                        if let Some(old) = saved.remove(param) {
                            self.globals.insert(param.clone(), old);
                        }
                    }
                    Ok(result)
                } else {
                    Err(format!("undefined function: {name}"))
                }
            }
        }
    }

    fn eval_one_num_arg(
        &mut self,
        args: &[Expr],
        out: &mut dyn Write,
    ) -> Result<f64, String> {
        if args.is_empty() {
            Ok(0.0)
        } else {
            Ok(self.eval_expr(&args[0], out)?.to_num())
        }
    }

    // -- sprintf implementation --

    fn sprintf_impl(&self, fmt: &str, args: &[Value]) -> String {
        let mut result = String::new();
        let bytes = fmt.as_bytes();
        let mut pos = 0;
        let mut arg_idx = 0;

        while pos < bytes.len() {
            if bytes[pos] == b'%' {
                pos += 1;
                if pos >= bytes.len() {
                    result.push('%');
                    break;
                }
                if bytes[pos] == b'%' {
                    result.push('%');
                    pos += 1;
                    continue;
                }

                // Parse flags.
                let mut flags = String::new();
                while pos < bytes.len()
                    && (bytes[pos] == b'-'
                        || bytes[pos] == b'+'
                        || bytes[pos] == b' '
                        || bytes[pos] == b'0'
                        || bytes[pos] == b'#')
                {
                    flags.push(char::from(bytes[pos]));
                    pos += 1;
                }

                // Parse width.
                let mut width = String::new();
                if pos < bytes.len() && bytes[pos] == b'*' {
                    pos += 1;
                    let w = args.get(arg_idx).map_or(0.0, |v| v.to_num()) as i64;
                    arg_idx += 1;
                    width = w.to_string();
                } else {
                    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                        width.push(char::from(bytes[pos]));
                        pos += 1;
                    }
                }

                // Parse precision.
                let mut precision = String::new();
                if pos < bytes.len() && bytes[pos] == b'.' {
                    pos += 1;
                    if pos < bytes.len() && bytes[pos] == b'*' {
                        pos += 1;
                        let p = args.get(arg_idx).map_or(0.0, |v| v.to_num()) as i64;
                        arg_idx += 1;
                        precision = p.to_string();
                    } else {
                        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                            precision.push(char::from(bytes[pos]));
                            pos += 1;
                        }
                    }
                }

                if pos >= bytes.len() {
                    break;
                }

                let conv = bytes[pos];
                pos += 1;

                let arg = args.get(arg_idx).cloned().unwrap_or(Value::Uninit);
                arg_idx += 1;

                let w: usize = width.parse().unwrap_or(0);
                let left_align = flags.contains('-');
                let zero_pad = flags.contains('0') && !left_align;
                let prec: Option<usize> = if precision.is_empty() {
                    None
                } else {
                    precision.parse().ok()
                };

                let formatted = match conv {
                    b'd' | b'i' => {
                        let n = arg.to_num() as i64;
                        format!("{n}")
                    }
                    b'o' => {
                        let n = arg.to_num() as u64;
                        format!("{n:o}")
                    }
                    b'x' => {
                        let n = arg.to_num() as u64;
                        format!("{n:x}")
                    }
                    b'X' => {
                        let n = arg.to_num() as u64;
                        format!("{n:X}")
                    }
                    b'f' => {
                        let n = arg.to_num();
                        let p = prec.unwrap_or(6);
                        format!("{n:.p$}")
                    }
                    b'e' => {
                        let n = arg.to_num();
                        let p = prec.unwrap_or(6);
                        format_scientific(n, p, false)
                    }
                    b'E' => {
                        let n = arg.to_num();
                        let p = prec.unwrap_or(6);
                        format_scientific(n, p, true)
                    }
                    b'g' => {
                        let n = arg.to_num();
                        let p = prec.unwrap_or(6);
                        format_g(n, p, false)
                    }
                    b'G' => {
                        let n = arg.to_num();
                        let p = prec.unwrap_or(6);
                        format_g(n, p, true)
                    }
                    b's' => {
                        let mut s = arg.to_str();
                        if let Some(p) = prec {
                            s.truncate(p);
                        }
                        s
                    }
                    b'c' => {
                        let c = match &arg {
                            Value::Str(s) if !s.is_empty() => {
                                s.chars().next().unwrap_or('\0')
                            }
                            _ => {
                                let n = arg.to_num() as u32;
                                char::from_u32(n).unwrap_or('\0')
                            }
                        };
                        c.to_string()
                    }
                    _ => {
                        // Unknown conversion, output literally.
                        format!("%{conv}", conv = char::from(conv))
                    }
                };

                // Apply width and padding.
                if w > formatted.len() {
                    let pad_char = if zero_pad && !matches!(conv, b's' | b'c') {
                        '0'
                    } else {
                        ' '
                    };
                    let padding: String =
                        std::iter::repeat(pad_char).take(w - formatted.len()).collect();
                    if left_align {
                        result.push_str(&formatted);
                        result.push_str(&padding);
                    } else {
                        result.push_str(&padding);
                        result.push_str(&formatted);
                    }
                } else {
                    result.push_str(&formatted);
                }
            } else {
                result.push(char::from(bytes[pos]));
                pos += 1;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_scientific(n: f64, prec: usize, upper: bool) -> String {
    if n == 0.0 {
        let e_char = if upper { 'E' } else { 'e' };
        return format!("{:.prec$}{e_char}+00", 0.0, prec = prec, e_char = e_char);
    }
    let exp = n.abs().log10().floor() as i32;
    let mantissa = n / 10f64.powi(exp);
    let e_char = if upper { 'E' } else { 'e' };
    let sign = if exp >= 0 { '+' } else { '-' };
    format!(
        "{mantissa:.prec$}{e_char}{sign}{exp:02}",
        mantissa = mantissa,
        prec = prec,
        e_char = e_char,
        sign = sign,
        exp = exp.unsigned_abs()
    )
}

fn format_g(n: f64, prec: usize, upper: bool) -> String {
    let p = if prec == 0 { 1 } else { prec };
    if n == 0.0 {
        return "0".into();
    }
    let exp = n.abs().log10().floor() as i32;
    if exp >= -4 && exp < p as i32 {
        // Use %f style, strip trailing zeros.
        let decimal_places = if (p as i32) > exp {
            (p as i32 - exp - 1) as usize
        } else {
            0
        };
        let s = format!("{n:.decimal_places$}");
        strip_trailing_zeros(&s)
    } else {
        let sci_prec = if p > 1 { p - 1 } else { 0 };
        let s = format_scientific(n, sci_prec, upper);
        // Strip trailing zeros in mantissa.
        if let Some(e_pos) = s.find(if upper { 'E' } else { 'e' }) {
            let mantissa = strip_trailing_zeros(&s[..e_pos]);
            format!("{mantissa}{}", &s[e_pos..])
        } else {
            s
        }
    }
}

fn strip_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.into();
    }
    let trimmed = s.trim_end_matches('0');
    let trimmed = trimmed.trim_end_matches('.');
    trimmed.into()
}

/// Split a string using a compiled regex as separator.
fn regex_split<'a>(re: &Regex, text: &'a str) -> Vec<&'a str> {
    let bytes = text.as_bytes();
    let matches = regex_find_all(re, bytes);
    if matches.is_empty() {
        return vec![text];
    }
    let mut parts = Vec::new();
    let mut prev_end = 0;
    for (mstart, mend) in &matches {
        // SAFETY: mstart and mend are byte indices from the regex engine,
        // bounded by `text.len()`.  We only feed ASCII-compatible patterns
        // and the source text to the engine, so these boundaries sit on valid
        // UTF-8 boundaries for any all-ASCII input.  For non-ASCII text the
        // split is best-effort since our regex operates on raw bytes.
        let part = &text[prev_end..*mstart];
        parts.push(part);
        prev_end = *mend;
    }
    parts.push(&text[prev_end..]);
    parts
}

/// Perform regex substitution.  Returns the result string and the number of
/// substitutions made.
fn regex_sub(re: &Regex, replacement: &str, target: &str, global: bool) -> (String, usize) {
    let bytes = target.as_bytes();
    let mut result = String::new();
    let mut count = 0;
    let mut search_start = 0;

    loop {
        // Find next match starting from search_start.
        let sub_bytes = &bytes[search_start..];
        let m = re.find(sub_bytes);
        match m {
            Some((ms, me)) => {
                // Append text before match.
                result.push_str(&target[search_start..search_start + ms]);
                // Append replacement, expanding `&` as the matched text.
                let matched_text = &target[search_start + ms..search_start + me];
                for ch in replacement.chars() {
                    if ch == '&' {
                        result.push_str(matched_text);
                    } else if ch == '\\' {
                        // In awk sub/gsub, `\\` in replacement is literal backslash
                        // and `\&` is literal ampersand. We simplify here.
                        result.push('\\');
                    } else {
                        result.push(ch);
                    }
                }
                count += 1;
                let advance = if me > ms { me } else { ms + 1 };
                if search_start + advance > bytes.len() {
                    // Append any remaining text after last match.
                    if search_start + me < bytes.len() {
                        result.push_str(&target[search_start + me..]);
                    }
                    break;
                }
                search_start += advance;
                if !global {
                    // Append rest of string.
                    result.push_str(&target[search_start..]);
                    break;
                }
            }
            None => {
                result.push_str(&target[search_start..]);
                break;
            }
        }
    }
    (result, count)
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

struct Args {
    /// Program text from `-e` or bare argument.
    program_texts: Vec<String>,
    /// Program files from `-f`.
    program_files: Vec<String>,
    /// Field separator from `-F`.
    field_sep: Option<String>,
    /// Pre-set variables from `-v`.
    vars: Vec<(String, String)>,
    /// Input files.
    input_files: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = env::args().collect();
    let mut args = Args {
        program_texts: Vec::new(),
        program_files: Vec::new(),
        field_sep: None,
        vars: Vec::new(),
        input_files: Vec::new(),
    };

    let mut i = 1;
    let mut found_double_dash = false;
    let mut found_program = false;

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

        if arg == "-F" {
            i += 1;
            if i >= argv.len() {
                return Err("-F requires an argument".into());
            }
            args.field_sep = Some(argv[i].clone());
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("-F") {
            args.field_sep = Some(rest.to_string());
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("--field-separator=") {
            args.field_sep = Some(rest.to_string());
            i += 1;
        } else if arg == "-v" {
            i += 1;
            if i >= argv.len() {
                return Err("-v requires var=val argument".into());
            }
            let assignment = &argv[i];
            if let Some(eq_pos) = assignment.find('=') {
                let var_name = assignment[..eq_pos].to_string();
                let var_val = assignment[eq_pos + 1..].to_string();
                args.vars.push((var_name, var_val));
            } else {
                return Err(format!("-v: invalid assignment: {assignment}"));
            }
            i += 1;
        } else if arg == "-f" {
            i += 1;
            if i >= argv.len() {
                return Err("-f requires a filename".into());
            }
            args.program_files.push(argv[i].clone());
            found_program = true;
            i += 1;
        } else if arg == "-e" {
            i += 1;
            if i >= argv.len() {
                return Err("-e requires program text".into());
            }
            args.program_texts.push(argv[i].clone());
            found_program = true;
            i += 1;
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown option: {arg}"));
        } else if !found_program && args.program_texts.is_empty() && args.program_files.is_empty() {
            // First non-option arg is the program.
            args.program_texts.push(arg.clone());
            found_program = true;
            i += 1;
        } else {
            args.input_files.push(arg.clone());
            i += 1;
        }
    }

    if args.program_texts.is_empty() && args.program_files.is_empty() {
        return Err("no program specified. Usage: awk [OPTIONS] 'program' [file...]".into());
    }

    Ok(args)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn run() -> Result<i32, String> {
    let args = parse_args()?;

    // Assemble program source.
    let mut source = String::new();
    for text in &args.program_texts {
        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(text);
    }
    for file in &args.program_files {
        let contents = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(&contents);
    }

    // Tokenize.
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize()?;

    // Parse.
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    // Build ARGV for interpreter.
    let mut interp_argv = vec!["awk".to_string()];
    for f in &args.input_files {
        interp_argv.push(f.clone());
    }

    // Set up interpreter.
    let mut interp = Interpreter::new(interp_argv);

    // Apply -F.
    if let Some(ref fs) = args.field_sep {
        interp.set_var("FS", Value::Str(fs.clone()));
    }

    // Apply -v assignments.
    for (var, val) in &args.vars {
        // Try to parse as number; if it looks numeric, store as number.
        let value = if let Ok(n) = val.parse::<f64>() {
            Value::Num(n)
        } else {
            Value::Str(val.clone())
        };
        interp.set_var(var, value);
    }

    // Run.
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let exit_code = interp.run(&program, &args.input_files, &mut out)?;
    out.flush().map_err(|e| e.to_string())?;

    Ok(exit_code)
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(e) => {
            let _ = writeln!(io::stderr(), "awk: {e}");
            process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: run an awk program on input, return stdout as a String.
    fn run_awk(program: &str, input: &str) -> String {
        run_awk_with_fs(program, input, None)
    }

    fn run_awk_with_fs(program: &str, input: &str, fs: Option<&str>) -> String {
        let mut lexer = Lexer::new(program);
        let tokens = lexer.tokenize().expect("tokenize failed");
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().expect("parse failed");

        let mut interp = Interpreter::new(vec!["awk".into()]);
        if let Some(sep) = fs {
            interp.set_var("FS", Value::Str(sep.into()));
        }

        // Register user-defined functions.
        for f in &prog.functions {
            interp.functions.insert(f.name.clone(), f.clone());
        }

        let mut output = Vec::new();
        let reader = io::Cursor::new(input.as_bytes().to_vec());
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        // Execute BEGIN.
        let mut exited = false;
        for rule in &prog.rules {
            if matches!(rule.pattern, Pattern::Begin) {
                if let Ok(ControlFlow::Exit(_)) =
                    interp.exec_stmts(&rule.action, &mut output)
                {
                    exited = true;
                    break;
                }
            }
        }

        // Process lines.
        if !exited {
            'lines: for line in &lines {
                interp.nr += 1;
                interp.fnr += 1;
                interp
                    .globals
                    .insert("NR".into(), Value::Num(interp.nr as f64));
                interp
                    .globals
                    .insert("FNR".into(), Value::Num(interp.fnr as f64));
                interp.set_record(line);

                match interp.run_main_rules(&prog, &mut output).unwrap() {
                    ControlFlow::Exit(_) => break 'lines,
                    ControlFlow::Next => continue 'lines,
                    _ => {}
                }
            }
        }

        // Execute END.
        for rule in &prog.rules {
            if matches!(rule.pattern, Pattern::End) {
                let _ = interp.exec_stmts(&rule.action, &mut output);
            }
        }

        String::from_utf8(output).unwrap()
    }

    // -- Regex tests --

    #[test]
    fn regex_literal_match() {
        let re = compile_regex(b"hello").unwrap();
        assert!(re.is_match(b"say hello world"));
        assert!(!re.is_match(b"say helo world"));
    }

    #[test]
    fn regex_dot() {
        let re = compile_regex(b"h.llo").unwrap();
        assert!(re.is_match(b"hello"));
        assert!(re.is_match(b"hallo"));
        assert!(!re.is_match(b"hllo"));
    }

    #[test]
    fn regex_star() {
        let re = compile_regex(b"ab*c").unwrap();
        assert!(re.is_match(b"ac"));
        assert!(re.is_match(b"abc"));
        assert!(re.is_match(b"abbc"));
    }

    #[test]
    fn regex_plus() {
        let re = compile_regex(b"ab+c").unwrap();
        assert!(!re.is_match(b"ac"));
        assert!(re.is_match(b"abc"));
        assert!(re.is_match(b"abbc"));
    }

    #[test]
    fn regex_question() {
        let re = compile_regex(b"ab?c").unwrap();
        assert!(re.is_match(b"ac"));
        assert!(re.is_match(b"abc"));
        assert!(!re.is_match(b"abbc"));
    }

    #[test]
    fn regex_anchors() {
        let re_start = compile_regex(b"^hello").unwrap();
        assert!(re_start.is_match(b"hello world"));
        assert!(!re_start.is_match(b"say hello"));

        let re_end = compile_regex(b"world$").unwrap();
        assert!(re_end.is_match(b"hello world"));
        assert!(!re_end.is_match(b"world peace"));
    }

    #[test]
    fn regex_char_class() {
        let re = compile_regex(b"[abc]").unwrap();
        assert!(re.is_match(b"a"));
        assert!(re.is_match(b"b"));
        assert!(!re.is_match(b"d"));

        let re_neg = compile_regex(b"[^abc]").unwrap();
        assert!(!re_neg.is_match(b"a"));
        assert!(re_neg.is_match(b"d"));
    }

    // -- Lexer tests --

    #[test]
    fn lex_simple_program() {
        let mut lexer = Lexer::new("{ print $0 }");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0], Token::LBrace));
        assert!(matches!(tokens[1], Token::Print));
        assert!(matches!(tokens[2], Token::Dollar));
        assert!(matches!(tokens[3], Token::Number(n) if n == 0.0));
        assert!(matches!(tokens[4], Token::RBrace));
    }

    #[test]
    fn lex_string_and_regex() {
        let mut lexer = Lexer::new(r#"/foo/ { print "bar" }"#);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0], Token::Regex(r) if r == "foo"));
        assert!(matches!(&tokens[2], Token::Print));
        assert!(matches!(&tokens[3], Token::StringLit(s) if s == "bar"));
    }

    #[test]
    fn lex_operators() {
        let mut lexer = Lexer::new("a += 1; b -= 2; c *= 3");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[1], Token::PlusAssign));
        assert!(matches!(tokens[5], Token::MinusAssign));
        assert!(matches!(tokens[9], Token::StarAssign));
    }

    // -- Parser tests --

    #[test]
    fn parse_begin_end() {
        let mut lexer = Lexer::new("BEGIN { x = 1 } END { print x }");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().unwrap();
        assert_eq!(prog.rules.len(), 2);
        assert!(matches!(prog.rules[0].pattern, Pattern::Begin));
        assert!(matches!(prog.rules[1].pattern, Pattern::End));
    }

    #[test]
    fn parse_regex_pattern() {
        let mut lexer = Lexer::new("/hello/ { print }");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().unwrap();
        assert!(matches!(&prog.rules[0].pattern, Pattern::Regex(r) if r == "hello"));
    }

    #[test]
    fn parse_expression_pattern() {
        let mut lexer = Lexer::new("NR > 2 { print $1 }");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().unwrap();
        assert!(matches!(prog.rules[0].pattern, Pattern::Expression(_)));
    }

    #[test]
    fn parse_for_loop() {
        let mut lexer = Lexer::new("{ for (i = 0; i < NF; i++) print $i }");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().unwrap();
        assert!(!prog.rules.is_empty());
    }

    #[test]
    fn parse_for_in() {
        let mut lexer = Lexer::new("END { for (k in arr) print k }");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().unwrap();
        assert!(matches!(prog.rules[0].pattern, Pattern::End));
    }

    #[test]
    fn parse_function_def() {
        let mut lexer = Lexer::new("function add(a, b) { return a + b }");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program().unwrap();
        assert_eq!(prog.functions.len(), 1);
        assert_eq!(prog.functions[0].name, "add");
        assert_eq!(prog.functions[0].params, vec!["a", "b"]);
    }

    // -- Integration tests --

    #[test]
    fn print_all_lines() {
        let out = run_awk("{ print }", "hello\nworld\n");
        assert_eq!(out, "hello\nworld\n");
    }

    #[test]
    fn print_field() {
        let out = run_awk("{ print $2 }", "one two three\nfour five six\n");
        assert_eq!(out, "two\nfive\n");
    }

    #[test]
    fn print_nr_nf() {
        let out = run_awk("{ print NR, NF }", "a b c\nd e\n");
        assert_eq!(out, "1 3\n2 2\n");
    }

    #[test]
    fn begin_end() {
        let out = run_awk("BEGIN { print \"start\" } END { print \"end\" }", "mid\n");
        assert_eq!(out, "start\nend\n");
    }

    #[test]
    fn regex_pattern_filter() {
        let out = run_awk("/hello/ { print }", "hello world\ngoodbye\nhello again\n");
        assert_eq!(out, "hello world\nhello again\n");
    }

    #[test]
    fn expression_pattern() {
        let out = run_awk("NR > 1 { print }", "first\nsecond\nthird\n");
        assert_eq!(out, "second\nthird\n");
    }

    #[test]
    fn field_separator() {
        let out = run_awk_with_fs("{ print $2 }", "a:b:c\nd:e:f\n", Some(":"));
        assert_eq!(out, "b\ne\n");
    }

    #[test]
    fn arithmetic() {
        let out = run_awk("BEGIN { print 2 + 3 * 4 }",  "");
        assert_eq!(out, "14\n");
    }

    #[test]
    fn string_concatenation() {
        let out = run_awk("BEGIN { a = \"hello\"; b = \" world\"; print a b }", "");
        assert_eq!(out, "hello world\n");
    }

    #[test]
    fn associative_array() {
        let out = run_awk(
            "{ count[$1]++ } END { for (w in count) print w, count[w] }",
            "a\nb\na\na\nb\n",
        );
        // Order is not guaranteed for for-in, so check both lines exist.
        assert!(out.contains("a 3"));
        assert!(out.contains("b 2"));
    }

    #[test]
    fn printf_format() {
        let out = run_awk("BEGIN { printf \"%05d\\n\", 42 }", "");
        assert_eq!(out, "00042\n");
    }

    #[test]
    fn printf_string() {
        let out = run_awk("BEGIN { printf \"%-10s|\\n\", \"hi\" }", "");
        assert_eq!(out, "hi        |\n");
    }

    #[test]
    fn builtin_length() {
        let out = run_awk("{ print length($0) }", "hello\nhi\n");
        assert_eq!(out, "5\n2\n");
    }

    #[test]
    fn builtin_substr() {
        let out = run_awk("BEGIN { print substr(\"hello world\", 7) }", "");
        assert_eq!(out, "world\n");
    }

    #[test]
    fn builtin_substr_with_len() {
        let out = run_awk("BEGIN { print substr(\"hello world\", 1, 5) }", "");
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn builtin_index() {
        let out = run_awk("BEGIN { print index(\"hello world\", \"world\") }", "");
        assert_eq!(out, "7\n");
    }

    #[test]
    fn builtin_split() {
        let out = run_awk(
            "BEGIN { n = split(\"a:b:c\", arr, \":\"); for (i = 1; i <= n; i++) print arr[i] }",
            "",
        );
        assert_eq!(out, "a\nb\nc\n");
    }

    #[test]
    fn builtin_tolower_toupper() {
        let out = run_awk("BEGIN { print tolower(\"HELLO\"), toupper(\"world\") }", "");
        assert_eq!(out, "hello WORLD\n");
    }

    #[test]
    fn builtin_gsub() {
        let out = run_awk("{ gsub(/o/, \"0\"); print }", "foo bar\n");
        assert_eq!(out, "f00 bar\n");
    }

    #[test]
    fn builtin_sub() {
        let out = run_awk("{ sub(/o/, \"0\"); print }", "foo bar\n");
        assert_eq!(out, "f0o bar\n");
    }

    #[test]
    fn builtin_match() {
        let out = run_awk(
            "BEGIN { print match(\"hello world\", /wor/) }",
            "",
        );
        assert_eq!(out, "7\n");
    }

    #[test]
    fn builtin_sprintf() {
        let out = run_awk("BEGIN { s = sprintf(\"%d + %d = %d\", 1, 2, 3); print s }", "");
        assert_eq!(out, "1 + 2 = 3\n");
    }

    #[test]
    fn builtin_int() {
        let out = run_awk("BEGIN { print int(3.9), int(-3.9) }", "");
        assert_eq!(out, "3 -3\n");
    }

    #[test]
    fn builtin_sqrt() {
        let out = run_awk("BEGIN { print int(sqrt(144)) }", "");
        assert_eq!(out, "12\n");
    }

    #[test]
    fn if_else() {
        let out = run_awk("{ if ($1 > 2) print \"big\"; else print \"small\" }", "1\n3\n2\n");
        assert_eq!(out, "small\nbig\nsmall\n");
    }

    #[test]
    fn while_loop() {
        let out = run_awk("BEGIN { i = 1; while (i <= 3) { print i; i++ } }", "");
        assert_eq!(out, "1\n2\n3\n");
    }

    #[test]
    fn for_loop() {
        let out = run_awk("BEGIN { for (i = 1; i <= 3; i++) print i }", "");
        assert_eq!(out, "1\n2\n3\n");
    }

    #[test]
    fn ternary_operator() {
        let out = run_awk("{ print ($1 > 0 ? \"pos\" : \"non-pos\") }", "5\n-3\n0\n");
        assert_eq!(out, "pos\nnon-pos\nnon-pos\n");
    }

    #[test]
    fn match_operator() {
        let out = run_awk("$0 ~ /^[0-9]/ { print }", "123\nabc\n456\n");
        assert_eq!(out, "123\n456\n");
    }

    #[test]
    fn not_match_operator() {
        let out = run_awk("$0 !~ /^#/ { print }", "#comment\ncode\n#another\n");
        assert_eq!(out, "code\n");
    }

    #[test]
    fn delete_array() {
        let out = run_awk(
            "{ a[$1] = 1 } END { delete a[\"b\"]; for (k in a) print k }",
            "a\nb\nc\n",
        );
        assert!(out.contains("a"));
        assert!(out.contains("c"));
        assert!(!out.contains("b"));
    }

    #[test]
    fn ofs_setting() {
        let out = run_awk("BEGIN { OFS = \",\" } { print $1, $2 }", "a b\nc d\n");
        assert_eq!(out, "a,b\nc,d\n");
    }

    #[test]
    fn user_function() {
        let out = run_awk(
            "function double(x) { return x * 2 } BEGIN { print double(21) }",
            "",
        );
        assert_eq!(out, "42\n");
    }

    #[test]
    fn power_operator() {
        let out = run_awk("BEGIN { print 2 ^ 10 }", "");
        assert_eq!(out, "1024\n");
    }

    #[test]
    fn pre_post_increment() {
        let out = run_awk("BEGIN { x = 5; print ++x; print x++; print x }", "");
        assert_eq!(out, "6\n6\n7\n");
    }

    #[test]
    fn no_pattern_means_all() {
        let out = run_awk("{ print \"line\" }", "a\nb\n");
        assert_eq!(out, "line\nline\n");
    }

    #[test]
    fn empty_input() {
        let out = run_awk("{ print }", "");
        assert_eq!(out, "");
    }

    #[test]
    fn begin_only() {
        let out = run_awk("BEGIN { print 42 }", "");
        assert_eq!(out, "42\n");
    }

    #[test]
    fn sum_column() {
        let out = run_awk("{ s += $1 } END { print s }", "10\n20\n30\n");
        assert_eq!(out, "60\n");
    }

    #[test]
    fn count_lines() {
        let out = run_awk("END { print NR }", "a\nb\nc\n");
        assert_eq!(out, "3\n");
    }

    #[test]
    fn printf_percent() {
        let out = run_awk("BEGIN { printf \"100%%\\n\" }", "");
        assert_eq!(out, "100%\n");
    }

    #[test]
    fn in_array_test() {
        let out = run_awk(
            "BEGIN { a[\"x\"] = 1; if (\"x\" in a) print \"yes\"; if (\"y\" in a) print \"no\" }",
            "",
        );
        assert_eq!(out, "yes\n");
    }

    #[test]
    fn multiline_print_with_comma() {
        let out = run_awk("{ print $1, $2, $3 }", "one two three\n");
        assert_eq!(out, "one two three\n");
    }

    #[test]
    fn next_statement() {
        let out = run_awk("NR == 2 { next } { print }", "a\nb\nc\n");
        assert_eq!(out, "a\nc\n");
    }

    #[test]
    fn value_auto_conversion() {
        // String "3" should auto-convert to number 3 in arithmetic.
        let out = run_awk("BEGIN { x = \"3\"; print x + 2 }", "");
        assert_eq!(out, "5\n");
    }

    #[test]
    fn exit_statement() {
        let out = run_awk("NR == 2 { exit } { print }", "a\nb\nc\n");
        assert_eq!(out, "a\n");
    }
}
