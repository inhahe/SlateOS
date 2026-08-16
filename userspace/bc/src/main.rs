//! Slate OS `bc` -- arbitrary-precision calculator
//!
//! A POSIX-compatible `bc` implementation with extensions.  Supports
//! arbitrary-precision integers and fixed-point decimals, variables,
//! user-defined functions, control flow, and the `-l` math library.
//!
//! Architecture: hand-written lexer -> recursive-descent parser -> AST ->
//! tree-walk interpreter.  The BigInt type uses base-10^9 limbs for easy
//! decimal I/O while keeping arithmetic efficient.

use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead};
#[cfg(not(test))]
use std::io::Write;
use std::process;

// -------------------------------------------------------------------------
// BigInt now lives in the `bignum` crate, so that `bc`, `dc`, `genius-cli` and
// `expr` cannot disagree about what an exact integer is. Nothing else changed:
// the crate is this file's former contents, lifted verbatim.
// -------------------------------------------------------------------------

use bignum::{digit_to_char, BigInt};


// -------------------------------------------------------------------------
// BcNum -- fixed-point decimal with arbitrary precision
// -------------------------------------------------------------------------

/// A fixed-point decimal number: `int_part * 10^(-scale)`.
/// The `digits` BigInt stores the unscaled integer, and `scale` is the
/// number of fractional decimal digits.
#[derive(Clone, Debug)]
struct BcNum {
    digits: BigInt,
    scale: usize,
}

impl BcNum {
    fn zero() -> Self {
        Self {
            digits: BigInt::zero(),
            scale: 0,
        }
    }

    fn one() -> Self {
        Self {
            digits: BigInt::one(),
            scale: 0,
        }
    }

    fn from_i64(v: i64) -> Self {
        Self {
            digits: BigInt::from_i64(v),
            scale: 0,
        }
    }

    fn is_zero(&self) -> bool {
        self.digits.is_zero()
    }

    fn is_negative(&self) -> bool {
        self.digits.negative
    }

    fn negate(&self) -> Self {
        let mut r = self.clone();
        r.digits.negative = !r.digits.negative;
        if r.digits.is_zero() {
            r.digits.negative = false;
        }
        r
    }

    fn abs(&self) -> Self {
        let mut r = self.clone();
        r.digits.negative = false;
        r
    }

    /// Set scale to `new_scale` decimal digits (truncating or zero-extending).
    fn rescale(&self, new_scale: usize) -> Self {
        if new_scale == self.scale {
            return self.clone();
        }
        if new_scale > self.scale {
            let diff = new_scale - self.scale;
            Self {
                digits: self.digits.shift_left_decimal(diff),
                scale: new_scale,
            }
        } else {
            let diff = self.scale - new_scale;
            let divisor = BigInt::from_i64(10).pow(&BigInt::from_i64(diff as i64));
            let (q, _) = self.digits.divmod(&divisor);
            Self {
                digits: q,
                scale: new_scale,
            }
        }
    }

    fn add(&self, other: &Self) -> Self {
        let s = self.scale.max(other.scale);
        let a = self.rescale(s);
        let b = other.rescale(s);
        Self {
            digits: a.digits.add(&b.digits),
            scale: s,
        }
    }

    fn sub(&self, other: &Self) -> Self {
        let s = self.scale.max(other.scale);
        let a = self.rescale(s);
        let b = other.rescale(s);
        Self {
            digits: a.digits.sub(&b.digits),
            scale: s,
        }
    }

    fn mul(&self, other: &Self, result_scale: usize) -> Self {
        let product = self.digits.mul(&other.digits);
        let total_scale = self.scale + other.scale;
        let full = Self {
            digits: product,
            scale: total_scale,
        };
        full.rescale(result_scale)
    }

    fn div(&self, other: &Self, result_scale: usize) -> Self {
        if other.is_zero() {
            eprintln!("Runtime error: division by zero");
            return Self::zero();
        }
        // To get `result_scale` fractional digits, we scale the dividend
        // so that integer division gives us enough precision.
        let needed = result_scale + other.scale;
        let a = if needed > self.scale {
            self.rescale(needed)
        } else {
            self.clone()
        };
        // Now a has `a.scale` fractional digits, other has `other.scale`.
        // After integer division, the result has `a.scale - other.scale` fractional digits.
        let (q, _) = a.digits.divmod(&other.digits);
        let q_scale = a.scale.saturating_sub(other.scale);
        let result = Self {
            digits: q,
            scale: q_scale,
        };
        result.rescale(result_scale)
    }

    fn modulo(&self, other: &Self, result_scale: usize) -> Self {
        if other.is_zero() {
            eprintln!("Runtime error: modulo by zero");
            return Self::zero();
        }
        // bc modulo: a - (a/b)*b, where a/b is truncated to `scale` digits.
        let q = self.div(other, result_scale);
        let qb = q.mul(other, result_scale);
        self.sub(&qb)
    }

    fn pow(&self, exp: &Self, result_scale: usize) -> Self {
        // bc only supports integer exponents for ^.
        let e = exp.rescale(0);
        if e.digits.negative {
            // Negative exponent: 1 / (base ^ |exp|)
            let abs_exp = e.negate();
            let base_pow = self.pow(&abs_exp, result_scale);
            return Self::one().div(&base_pow, result_scale);
        }
        if e.is_zero() {
            return Self::one();
        }
        let mut result = Self::one();
        let mut base = self.clone();
        let mut exponent = e.digits.clone();
        let two = BigInt::from_i64(2);
        loop {
            if exponent.is_zero() {
                break;
            }
            let (half, rem) = exponent.divmod(&two);
            if !rem.is_zero() {
                result = result.mul(&base, result_scale);
            }
            base = base.mul(&base, result_scale);
            exponent = half;
        }
        result.rescale(result_scale)
    }

    fn sqrt(&self, result_scale: usize) -> Self {
        if self.is_negative() {
            eprintln!("Runtime error: square root of negative number");
            return Self::zero();
        }
        if self.is_zero() {
            return Self::zero();
        }
        // Scale up to get enough precision, take integer sqrt, scale back.
        let extra = result_scale * 2 + 2;
        let scaled = self.rescale(self.scale + extra);
        let isqrt = scaled.digits.isqrt();
        let r = Self {
            digits: isqrt,
            scale: scaled.scale.div_ceil(2),
        };
        r.rescale(result_scale)
    }

    /// Compare: returns -1, 0, or 1.
    fn cmp(&self, other: &Self) -> i32 {
        let diff = self.sub(other);
        if diff.is_zero() {
            0
        } else if diff.is_negative() {
            -1
        } else {
            1
        }
    }

    /// Parse a bc number like "123.456" with optional input base.
    fn parse(s: &str, ibase: u32) -> Self {
        let s = s.trim();
        if s.is_empty() {
            return Self::zero();
        }
        let (negative, body) = if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else {
            (false, s)
        };

        if let Some(dot_pos) = body.find('.') {
            let int_part = &body[..dot_pos];
            let frac_part = &body[dot_pos + 1..];
            let scale = frac_part.len();

            // Combine integer and fractional parts as one big number.
            let combined = format!("{}{}", int_part, frac_part);
            let mut digits = BigInt::from_str_radix(&combined, ibase);
            digits.negative = negative;
            digits.normalize();
            Self {
                digits,
                scale,
            }
        } else {
            let mut digits = BigInt::from_str_radix(body, ibase);
            digits.negative = negative;
            digits.normalize();
            Self { digits, scale: 0 }
        }
    }

    /// Format for output with the given obase.
    fn format(&self, obase: u32) -> String {
        if obase == 10 {
            return self.format_base10();
        }
        // For non-decimal output: convert integer part, then fractional.
        let int_val = self.rescale(0);
        let mut result = int_val.digits.to_str_radix(obase);
        if self.scale > 0 {
            result.push('.');
            // Get fractional part.
            let ten_pow = BigInt::from_i64(10).pow(&BigInt::from_i64(self.scale as i64));
            let frac_digits = {
                let int_scaled = int_val.rescale(self.scale);
                let diff = self.sub(&int_scaled);
                diff.abs()
            };
            // Convert fractional part by repeated multiplication.
            let mut frac = frac_digits.digits.clone();
            let base_big = BigInt::from_i64(obase as i64);
            for _ in 0..self.scale {
                frac = frac.mul(&base_big);
                let (q, r) = frac.divmod(&ten_pow);
                let d = if q.is_zero() { 0u32 } else { q.limbs[0] };
                result.push(digit_to_char(d));
                frac = r;
            }
        }
        result
    }

    fn format_base10(&self) -> String {
        if self.scale == 0 {
            return self.digits.to_string_base10();
        }
        let s = self.digits.to_string_base10();
        let negative = s.starts_with('-');
        let abs_s = if negative { &s[1..] } else { &s[..] };

        let (int_part, frac_part) = if abs_s.len() <= self.scale {
            let padding = self.scale - abs_s.len();
            let frac = format!("{}{}", "0".repeat(padding), abs_s);
            ("0".to_string(), frac)
        } else {
            let split = abs_s.len() - self.scale;
            (abs_s[..split].to_string(), abs_s[split..].to_string())
        };

        // Trim trailing zeros from fractional part (bc behavior).
        let frac_trimmed = frac_part.trim_end_matches('0');
        let prefix = if negative { "-" } else { "" };
        if frac_trimmed.is_empty() {
            format!("{}{}", prefix, int_part)
        } else {
            format!("{}{}.{}", prefix, int_part, frac_trimmed)
        }
    }

    /// Number of significant digits.
    fn length(&self) -> usize {
        let s = self.digits.to_string_base10();
        let s = s.trim_start_matches('-');
        s.len()
    }

    /// Check if this number is negligible at the given working scale.
    /// Returns true if |self| < 10^(-scale), meaning the number has no
    /// significant digits within the precision we care about.
    fn is_negligible(&self, working_scale: usize) -> bool {
        if self.is_zero() {
            return true;
        }
        // After rescaling to working_scale, the integer representation is
        // the value * 10^working_scale.  If that is zero, the value is
        // smaller than our precision can represent.
        let scaled = self.abs().rescale(working_scale);
        scaled.digits.is_zero()
    }
}

// -------------------------------------------------------------------------
// Lexer
// -------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(String),
    StringLit(String),
    Ident(String),
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    // Assignment operators
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    CaretAssign,
    // Increment/decrement
    PlusPlus,
    MinusMinus,
    // Comparison
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    // Logical
    Not,
    And,
    Or,
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
    // Keywords
    If,
    Else,
    While,
    For,
    Define,
    Return,
    Auto,
    Break,
    Continue,
    Quit,
    Print,
    // End of input
    Eof,
}

struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip spaces and tabs (but not newlines -- they are significant).
            while let Some(b) = self.peek_byte() {
                if b == b' ' || b == b'\t' || b == b'\r' || b == b'\\' {
                    if b == b'\\' {
                        // Line continuation.
                        if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'\n' {
                            self.pos += 2;
                            continue;
                        }
                    }
                    self.pos += 1;
                } else {
                    break;
                }
            }
            // Skip /* ... */ comments.
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'/'
                && self.input[self.pos + 1] == b'*'
            {
                self.pos += 2;
                while self.pos + 1 < self.input.len() {
                    if self.input[self.pos] == b'*' && self.input[self.pos + 1] == b'/' {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                continue;
            }
            // Skip # comments.
            if let Some(b'#') = self.peek_byte() {
                while let Some(b) = self.peek_byte() {
                    if b == b'\n' {
                        break;
                    }
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();

        let b = match self.peek_byte() {
            Some(b) => b,
            None => return Token::Eof,
        };

        // Newlines.
        if b == b'\n' {
            self.advance();
            return Token::Newline;
        }

        // Numbers: digits, leading dot-digit, or uppercase A-F (hex digit
        // values 10-15 in bc's number syntax).
        if b.is_ascii_digit()
            || (b'A'..=b'F').contains(&b)
            || (b == b'.'
                && self.pos + 1 < self.input.len()
                && self.input[self.pos + 1].is_ascii_hexdigit())
        {
            return self.read_number();
        }

        // String literals.
        if b == b'"' {
            return self.read_string();
        }

        // Identifiers and keywords (bc identifiers use lowercase + underscore).
        if b.is_ascii_lowercase() || b == b'_' {
            return self.read_ident();
        }

        // Operators and punctuation.
        self.advance();
        match b {
            b'+' => {
                if self.peek_byte() == Some(b'+') {
                    self.advance();
                    Token::PlusPlus
                } else if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::PlusAssign
                } else {
                    Token::Plus
                }
            }
            b'-' => {
                if self.peek_byte() == Some(b'-') {
                    self.advance();
                    Token::MinusMinus
                } else if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::MinusAssign
                } else {
                    Token::Minus
                }
            }
            b'*' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::StarAssign
                } else {
                    Token::Star
                }
            }
            b'/' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::SlashAssign
                } else {
                    Token::Slash
                }
            }
            b'%' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::PercentAssign
                } else {
                    Token::Percent
                }
            }
            b'^' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::CaretAssign
                } else {
                    Token::Caret
                }
            }
            b'=' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::EqEq
                } else {
                    Token::Assign
                }
            }
            b'!' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::NotEq
                } else {
                    Token::Not
                }
            }
            b'<' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::LtEq
                } else {
                    Token::Lt
                }
            }
            b'>' => {
                if self.peek_byte() == Some(b'=') {
                    self.advance();
                    Token::GtEq
                } else {
                    Token::Gt
                }
            }
            b'&' => {
                if self.peek_byte() == Some(b'&') {
                    self.advance();
                }
                Token::And
            }
            b'|' => {
                if self.peek_byte() == Some(b'|') {
                    self.advance();
                }
                Token::Or
            }
            b'(' => Token::LParen,
            b')' => Token::RParen,
            b'{' => Token::LBrace,
            b'}' => Token::RBrace,
            b'[' => Token::LBracket,
            b']' => Token::RBracket,
            b';' => Token::Semicolon,
            b',' => Token::Comma,
            _ => {
                // Unknown character, skip.
                self.next_token()
            }
        }
    }

    fn read_number(&mut self) -> Token {
        let start = self.pos;
        // bc numbers: digits, hex digits (for bases > 10 using uppercase A-F),
        // and at most one decimal point.
        let mut has_dot = false;
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_digit() || (b'A'..=b'F').contains(&b) {
                self.advance();
            } else if b == b'.' && !has_dot {
                has_dot = true;
                self.advance();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("0");
        Token::Number(s.to_string())
    }

    fn read_string(&mut self) -> Token {
        self.advance(); // skip opening "
        let mut s = String::new();
        while let Some(b) = self.peek_byte() {
            if b == b'"' {
                self.advance();
                break;
            }
            if b == b'\\' {
                self.advance();
                match self.peek_byte() {
                    Some(b'n') => {
                        s.push('\n');
                        self.advance();
                    }
                    Some(b't') => {
                        s.push('\t');
                        self.advance();
                    }
                    Some(b'\\') => {
                        s.push('\\');
                        self.advance();
                    }
                    Some(b'"') => {
                        s.push('"');
                        self.advance();
                    }
                    Some(other) => {
                        s.push('\\');
                        s.push(other as char);
                        self.advance();
                    }
                    None => break,
                }
            } else {
                s.push(b as char);
                self.advance();
            }
        }
        Token::StringLit(s)
    }

    fn read_ident(&mut self) -> Token {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
        match s {
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "define" => Token::Define,
            "return" => Token::Return,
            "auto" => Token::Auto,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "quit" => Token::Quit,
            "print" => Token::Print,
            _ => Token::Ident(s.to_string()),
        }
    }
}

// -------------------------------------------------------------------------
// AST
// -------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Expr {
    Number(String),
    StringLit(String),
    Var(String),
    ArrayAccess(String, Box<Expr>),
    /// `last` or `.`
    Last,
    UnaryMinus(Box<Expr>),
    UnaryNot(Box<Expr>),
    BinOp(Box<Expr>, BinOp, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(Box<Expr>, BinOp, Box<Expr>),
    PreInc(Box<Expr>),
    PreDec(Box<Expr>),
    PostInc(Box<Expr>),
    PostDec(Box<Expr>),
    Call(String, Vec<Expr>),
    /// Comparison operators return 0 or 1.
    Compare(Box<Expr>, CmpOp, Box<Expr>),
    Logical(Box<Expr>, LogOp, Box<Expr>),
}

#[derive(Clone, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Clone, Debug)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Debug)]
enum LogOp {
    And,
    Or,
}

#[derive(Clone, Debug)]
enum Stmt {
    Expr(Expr),
    Print(Vec<PrintItem>),
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>),
    While(Expr, Vec<Stmt>),
    For(Option<Expr>, Option<Expr>, Option<Expr>, Vec<Stmt>),
    Return(Option<Expr>),
    Break,
    Continue,
    Quit,
    FuncDef(String, Vec<String>, Vec<String>, Vec<Stmt>),
    Block(Vec<Stmt>),
}

#[derive(Clone, Debug)]
enum PrintItem {
    Expr(Expr),
    StringLit(String),
}

// -------------------------------------------------------------------------
// Parser
// -------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            let is_eof = tok == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
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

    fn expect(&mut self, expected: &Token) -> bool {
        if self.peek() == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Token::Newline || *self.peek() == Token::Semicolon {
            self.advance();
        }
    }

    fn parse_program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while *self.peek() != Token::Eof {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
            self.skip_newlines();
        }
        stmts
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Eof => None,
            Token::Quit => {
                self.advance();
                self.skip_terminator();
                Some(Stmt::Quit)
            }
            Token::Print => {
                self.advance();
                let items = self.parse_print_list();
                self.skip_terminator();
                Some(Stmt::Print(items))
            }
            Token::If => Some(self.parse_if()),
            Token::While => Some(self.parse_while()),
            Token::For => Some(self.parse_for()),
            Token::Define => Some(self.parse_define()),
            Token::Return => {
                self.advance();
                let expr = if self.is_expr_start() {
                    Some(self.parse_expr())
                } else {
                    None
                };
                self.skip_terminator();
                Some(Stmt::Return(expr))
            }
            Token::Break => {
                self.advance();
                self.skip_terminator();
                Some(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.skip_terminator();
                Some(Stmt::Continue)
            }
            Token::LBrace => {
                self.advance();
                let body = self.parse_stmt_list();
                self.expect(&Token::RBrace);
                Some(Stmt::Block(body))
            }
            _ => {
                if self.is_expr_start() {
                    let expr = self.parse_expr();
                    self.skip_terminator();
                    Some(Stmt::Expr(expr))
                } else {
                    // Skip unexpected token.
                    self.advance();
                    None
                }
            }
        }
    }

    fn skip_terminator(&mut self) {
        if *self.peek() == Token::Newline || *self.peek() == Token::Semicolon {
            self.advance();
        }
    }

    fn is_expr_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Number(_)
                | Token::StringLit(_)
                | Token::Ident(_)
                | Token::LParen
                | Token::Minus
                | Token::Not
                | Token::PlusPlus
                | Token::MinusMinus
        )
    }

    fn parse_print_list(&mut self) -> Vec<PrintItem> {
        let mut items = Vec::new();
        loop {
            match self.peek().clone() {
                Token::StringLit(s) => {
                    self.advance();
                    items.push(PrintItem::StringLit(s));
                }
                _ if self.is_expr_start() => {
                    let expr = self.parse_expr();
                    items.push(PrintItem::Expr(expr));
                }
                _ => break,
            }
            if *self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        items
    }

    fn parse_if(&mut self) -> Stmt {
        self.advance(); // consume 'if'
        self.expect(&Token::LParen);
        let cond = self.parse_expr();
        self.expect(&Token::RParen);
        self.skip_newlines();
        let then_body = self.parse_block_or_stmt();
        self.skip_newlines();
        let else_body = if *self.peek() == Token::Else {
            self.advance();
            self.skip_newlines();
            Some(self.parse_block_or_stmt())
        } else {
            None
        };
        Stmt::If(cond, then_body, else_body)
    }

    fn parse_while(&mut self) -> Stmt {
        self.advance(); // consume 'while'
        self.expect(&Token::LParen);
        let cond = self.parse_expr();
        self.expect(&Token::RParen);
        self.skip_newlines();
        let body = self.parse_block_or_stmt();
        Stmt::While(cond, body)
    }

    fn parse_for(&mut self) -> Stmt {
        self.advance(); // consume 'for'
        self.expect(&Token::LParen);
        let init = if self.is_expr_start() {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(&Token::Semicolon);
        let cond = if self.is_expr_start() {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(&Token::Semicolon);
        let step = if self.is_expr_start() {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(&Token::RParen);
        self.skip_newlines();
        let body = self.parse_block_or_stmt();
        Stmt::For(init, cond, step, body)
    }

    fn parse_define(&mut self) -> Stmt {
        self.advance(); // consume 'define'
        let name = match self.advance() {
            Token::Ident(s) => s,
            _ => "unknown".to_string(),
        };
        self.expect(&Token::LParen);
        let mut params = Vec::new();
        while let Token::Ident(p) = self.peek().clone() {
            self.advance();
            params.push(p);
            if *self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&Token::RParen);
        self.skip_newlines();
        self.expect(&Token::LBrace);
        self.skip_newlines();

        // Parse optional 'auto' declarations.
        let mut auto_vars = Vec::new();
        if *self.peek() == Token::Auto {
            self.advance();
            while let Token::Ident(v) = self.peek().clone() {
                self.advance();
                auto_vars.push(v);
                if *self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.skip_terminator();
        }

        let body = self.parse_stmt_list();
        self.expect(&Token::RBrace);
        Stmt::FuncDef(name, params, auto_vars, body)
    }

    fn parse_block_or_stmt(&mut self) -> Vec<Stmt> {
        if *self.peek() == Token::LBrace {
            self.advance();
            let stmts = self.parse_stmt_list();
            self.expect(&Token::RBrace);
            stmts
        } else if let Some(stmt) = self.parse_stmt() {
            vec![stmt]
        } else {
            Vec::new()
        }
    }

    fn parse_stmt_list(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while *self.peek() != Token::RBrace && *self.peek() != Token::Eof {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
            self.skip_newlines();
        }
        stmts
    }

    // Expression parsing with precedence climbing.

    fn parse_expr(&mut self) -> Expr {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Expr {
        let lhs = self.parse_or();
        match self.peek().clone() {
            Token::Assign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::Assign(Box::new(lhs), Box::new(rhs))
            }
            Token::PlusAssign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::OpAssign(Box::new(lhs), BinOp::Add, Box::new(rhs))
            }
            Token::MinusAssign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::OpAssign(Box::new(lhs), BinOp::Sub, Box::new(rhs))
            }
            Token::StarAssign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::OpAssign(Box::new(lhs), BinOp::Mul, Box::new(rhs))
            }
            Token::SlashAssign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::OpAssign(Box::new(lhs), BinOp::Div, Box::new(rhs))
            }
            Token::PercentAssign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::OpAssign(Box::new(lhs), BinOp::Mod, Box::new(rhs))
            }
            Token::CaretAssign => {
                self.advance();
                let rhs = self.parse_assignment();
                Expr::OpAssign(Box::new(lhs), BinOp::Pow, Box::new(rhs))
            }
            _ => lhs,
        }
    }

    fn parse_or(&mut self) -> Expr {
        let mut lhs = self.parse_and();
        while *self.peek() == Token::Or {
            self.advance();
            let rhs = self.parse_and();
            lhs = Expr::Logical(Box::new(lhs), LogOp::Or, Box::new(rhs));
        }
        lhs
    }

    fn parse_and(&mut self) -> Expr {
        let mut lhs = self.parse_comparison();
        while *self.peek() == Token::And {
            self.advance();
            let rhs = self.parse_comparison();
            lhs = Expr::Logical(Box::new(lhs), LogOp::And, Box::new(rhs));
        }
        lhs
    }

    fn parse_comparison(&mut self) -> Expr {
        let lhs = self.parse_add();
        let op = match self.peek() {
            Token::EqEq => CmpOp::Eq,
            Token::NotEq => CmpOp::Ne,
            Token::Lt => CmpOp::Lt,
            Token::Gt => CmpOp::Gt,
            Token::LtEq => CmpOp::Le,
            Token::GtEq => CmpOp::Ge,
            _ => return lhs,
        };
        self.advance();
        let rhs = self.parse_add();
        Expr::Compare(Box::new(lhs), op, Box::new(rhs))
    }

    fn parse_add(&mut self) -> Expr {
        let mut lhs = self.parse_mul();
        loop {
            match self.peek() {
                Token::Plus => {
                    self.advance();
                    let rhs = self.parse_mul();
                    lhs = Expr::BinOp(Box::new(lhs), BinOp::Add, Box::new(rhs));
                }
                Token::Minus => {
                    self.advance();
                    let rhs = self.parse_mul();
                    lhs = Expr::BinOp(Box::new(lhs), BinOp::Sub, Box::new(rhs));
                }
                _ => break,
            }
        }
        lhs
    }

    fn parse_mul(&mut self) -> Expr {
        let mut lhs = self.parse_power();
        loop {
            match self.peek() {
                Token::Star => {
                    self.advance();
                    let rhs = self.parse_power();
                    lhs = Expr::BinOp(Box::new(lhs), BinOp::Mul, Box::new(rhs));
                }
                Token::Slash => {
                    self.advance();
                    let rhs = self.parse_power();
                    lhs = Expr::BinOp(Box::new(lhs), BinOp::Div, Box::new(rhs));
                }
                Token::Percent => {
                    self.advance();
                    let rhs = self.parse_power();
                    lhs = Expr::BinOp(Box::new(lhs), BinOp::Mod, Box::new(rhs));
                }
                _ => break,
            }
        }
        lhs
    }

    fn parse_power(&mut self) -> Expr {
        let base = self.parse_unary();
        if *self.peek() == Token::Caret {
            self.advance();
            let exp = self.parse_unary(); // Right-associative.
            Expr::BinOp(Box::new(base), BinOp::Pow, Box::new(exp))
        } else {
            base
        }
    }

    fn parse_unary(&mut self) -> Expr {
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary();
                Expr::UnaryMinus(Box::new(expr))
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_unary();
                Expr::UnaryNot(Box::new(expr))
            }
            Token::PlusPlus => {
                self.advance();
                let expr = self.parse_postfix();
                Expr::PreInc(Box::new(expr))
            }
            Token::MinusMinus => {
                self.advance();
                let expr = self.parse_postfix();
                Expr::PreDec(Box::new(expr))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            match self.peek() {
                Token::PlusPlus => {
                    self.advance();
                    expr = Expr::PostInc(Box::new(expr));
                }
                Token::MinusMinus => {
                    self.advance();
                    expr = Expr::PostDec(Box::new(expr));
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek().clone() {
            Token::Number(s) => {
                self.advance();
                Expr::Number(s)
            }
            Token::StringLit(s) => {
                self.advance();
                Expr::StringLit(s)
            }
            Token::Ident(name) => {
                self.advance();
                if name == "last" {
                    return Expr::Last;
                }
                // Check for function call.
                if *self.peek() == Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    if *self.peek() != Token::RParen {
                        args.push(self.parse_expr());
                        while *self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr());
                        }
                    }
                    self.expect(&Token::RParen);
                    return Expr::Call(name, args);
                }
                // Check for array access.
                if *self.peek() == Token::LBracket {
                    self.advance();
                    let idx = self.parse_expr();
                    self.expect(&Token::RBracket);
                    return Expr::ArrayAccess(name, Box::new(idx));
                }
                Expr::Var(name)
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr();
                self.expect(&Token::RParen);
                expr
            }
            _ => {
                // Return zero for unexpected tokens.
                Expr::Number("0".to_string())
            }
        }
    }
}

// -------------------------------------------------------------------------
// Interpreter
// -------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct FuncDef {
    params: Vec<String>,
    auto_vars: Vec<String>,
    body: Vec<Stmt>,
}

/// Control flow signals from statement execution.
enum StmtResult {
    Normal,
    Return(BcNum),
    Break,
    Continue,
}

struct Interpreter {
    /// Named variables.
    vars: HashMap<String, BcNum>,
    /// Array variables: name -> (index -> value).
    arrays: HashMap<String, HashMap<String, BcNum>>,
    /// User-defined functions.
    funcs: HashMap<String, FuncDef>,
    /// scale, ibase, obase.
    scale: usize,
    ibase: u32,
    obase: u32,
    /// Last printed value.
    last: BcNum,
    /// Whether the math library is loaded (-l flag).
    math_lib: bool,
    /// When set, output is captured here instead of going to stdout.
    /// Used by tests to verify output without I/O.
    #[cfg(test)]
    output_buf: Vec<String>,
}

impl Interpreter {
    fn new(math_lib: bool) -> Self {
        let scale = if math_lib { 20 } else { 0 };
        Self {
            vars: HashMap::new(),
            arrays: HashMap::new(),
            funcs: HashMap::new(),
            scale,
            ibase: 10,
            obase: 10,
            last: BcNum::zero(),
            math_lib,
            #[cfg(test)]
            output_buf: Vec::new(),
        }
    }

    /// Output a line (with trailing newline).  In test mode, captured to
    /// `output_buf`; otherwise printed to stdout.
    fn output_line(&mut self, s: &str) {
        #[cfg(test)]
        {
            self.output_buf.push(s.to_string());
        }
        #[cfg(not(test))]
        {
            println!("{}", s);
        }
    }

    /// Output a string fragment (no trailing newline).  In test mode, captured
    /// to `output_buf`; otherwise printed to stdout.
    fn output_str(&mut self, s: &str) {
        #[cfg(test)]
        {
            self.output_buf.push(s.to_string());
        }
        #[cfg(not(test))]
        {
            print!("{}", s);
            let _ = io::stdout().flush();
        }
    }

    fn get_var(&self, name: &str) -> BcNum {
        match name {
            "scale" => BcNum::from_i64(self.scale as i64),
            "ibase" => BcNum::from_i64(self.ibase as i64),
            "obase" => BcNum::from_i64(self.obase as i64),
            _ => self.vars.get(name).cloned().unwrap_or_else(BcNum::zero),
        }
    }

    fn set_var(&mut self, name: &str, val: BcNum) {
        match name {
            "scale" => {
                let v = val.rescale(0);
                let s = v.digits.to_string_base10();
                self.scale = s.trim_start_matches('-').parse::<usize>().unwrap_or(0);
            }
            "ibase" => {
                let v = val.rescale(0);
                let s = v.digits.to_string_base10();
                let b = s.trim_start_matches('-').parse::<u32>().unwrap_or(10);
                if (2..=16).contains(&b) {
                    self.ibase = b;
                }
            }
            "obase" => {
                let v = val.rescale(0);
                let s = v.digits.to_string_base10();
                let b = s.trim_start_matches('-').parse::<u32>().unwrap_or(10);
                if (2..=16).contains(&b) {
                    self.obase = b;
                }
            }
            _ => {
                self.vars.insert(name.to_string(), val);
            }
        }
    }

    fn get_array(&self, name: &str, idx: &str) -> BcNum {
        self.arrays
            .get(name)
            .and_then(|m| m.get(idx))
            .cloned()
            .unwrap_or_else(BcNum::zero)
    }

    fn set_array(&mut self, name: &str, idx: &str, val: BcNum) {
        self.arrays
            .entry(name.to_string())
            .or_default()
            .insert(idx.to_string(), val);
    }

    fn run(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match self.exec_stmt(stmt) {
                StmtResult::Normal => {}
                StmtResult::Return(_) => return,
                StmtResult::Break | StmtResult::Continue => return,
            }
        }
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> StmtResult {
        match stmt {
            Stmt::Expr(expr) => {
                let val = self.eval(expr);
                // In bc, a bare expression prints its value.
                // But assignments don't print (they are silent).
                if !is_assignment_expr(expr) {
                    let formatted = val.format(self.obase);
                    self.output_line(&formatted);
                    self.last = val;
                } else {
                    self.last = val;
                }
                StmtResult::Normal
            }
            Stmt::Print(items) => {
                for item in items {
                    match item {
                        PrintItem::StringLit(s) => self.output_str(s),
                        PrintItem::Expr(expr) => {
                            let val = self.eval(expr);
                            let formatted = val.format(self.obase);
                            self.output_str(&formatted);
                            self.last = val;
                        }
                    }
                }
                #[cfg(not(test))]
                {
                    let _ = io::stdout().flush();
                }
                StmtResult::Normal
            }
            Stmt::If(cond, then_body, else_body) => {
                let val = self.eval(cond);
                if !val.is_zero() {
                    for s in then_body {
                        match self.exec_stmt(s) {
                            StmtResult::Normal => {}
                            other => return other,
                        }
                    }
                } else if let Some(else_stmts) = else_body {
                    for s in else_stmts {
                        match self.exec_stmt(s) {
                            StmtResult::Normal => {}
                            other => return other,
                        }
                    }
                }
                StmtResult::Normal
            }
            Stmt::While(cond, body) => {
                loop {
                    let val = self.eval(cond);
                    if val.is_zero() {
                        break;
                    }
                    let mut should_break = false;
                    for s in body {
                        match self.exec_stmt(s) {
                            StmtResult::Normal => {}
                            StmtResult::Break => {
                                should_break = true;
                                break;
                            }
                            StmtResult::Continue => break,
                            StmtResult::Return(v) => return StmtResult::Return(v),
                        }
                    }
                    if should_break {
                        break;
                    }
                }
                StmtResult::Normal
            }
            Stmt::For(init, cond, step, body) => {
                if let Some(init_expr) = init {
                    self.eval(init_expr);
                }
                loop {
                    if let Some(cond_expr) = cond {
                        let val = self.eval(cond_expr);
                        if val.is_zero() {
                            break;
                        }
                    }
                    let mut should_break = false;
                    for s in body {
                        match self.exec_stmt(s) {
                            StmtResult::Normal => {}
                            StmtResult::Break => {
                                should_break = true;
                                break;
                            }
                            StmtResult::Continue => break,
                            StmtResult::Return(v) => return StmtResult::Return(v),
                        }
                    }
                    if should_break {
                        break;
                    }
                    if let Some(step_expr) = step {
                        self.eval(step_expr);
                    }
                }
                StmtResult::Normal
            }
            Stmt::Return(expr) => {
                let val = expr
                    .as_ref()
                    .map(|e| self.eval(e))
                    .unwrap_or_else(BcNum::zero);
                StmtResult::Return(val)
            }
            Stmt::Break => StmtResult::Break,
            Stmt::Continue => StmtResult::Continue,
            Stmt::Quit => {
                process::exit(0);
            }
            Stmt::FuncDef(name, params, auto_vars, body) => {
                self.funcs.insert(
                    name.clone(),
                    FuncDef {
                        params: params.clone(),
                        auto_vars: auto_vars.clone(),
                        body: body.clone(),
                    },
                );
                StmtResult::Normal
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    match self.exec_stmt(s) {
                        StmtResult::Normal => {}
                        other => return other,
                    }
                }
                StmtResult::Normal
            }
        }
    }

    fn eval(&mut self, expr: &Expr) -> BcNum {
        match expr {
            Expr::Number(s) => BcNum::parse(s, self.ibase),
            Expr::StringLit(s) => {
                // In bc, strings in expression context are printed.
                self.output_str(s);
                BcNum::zero()
            }
            Expr::Var(name) => self.get_var(name),
            Expr::ArrayAccess(name, idx) => {
                let idx_val = self.eval(idx);
                let idx_str = idx_val.rescale(0).format(10);
                self.get_array(name, &idx_str)
            }
            Expr::Last => self.last.clone(),
            Expr::UnaryMinus(e) => self.eval(e).negate(),
            Expr::UnaryNot(e) => {
                let val = self.eval(e);
                if val.is_zero() {
                    BcNum::from_i64(1)
                } else {
                    BcNum::zero()
                }
            }
            Expr::BinOp(lhs, op, rhs) => {
                let a = self.eval(lhs);
                let b = self.eval(rhs);
                let scale = self.scale;
                match op {
                    BinOp::Add => a.add(&b),
                    BinOp::Sub => a.sub(&b),
                    BinOp::Mul => a.mul(&b, scale),
                    BinOp::Div => a.div(&b, scale),
                    BinOp::Mod => a.modulo(&b, scale),
                    BinOp::Pow => a.pow(&b, scale),
                }
            }
            Expr::Assign(target, val_expr) => {
                let val = self.eval(val_expr);
                self.assign_to(target, val.clone());
                val
            }
            Expr::OpAssign(target, op, val_expr) => {
                let current = self.eval_lvalue(target);
                let rhs = self.eval(val_expr);
                let scale = self.scale;
                let result = match op {
                    BinOp::Add => current.add(&rhs),
                    BinOp::Sub => current.sub(&rhs),
                    BinOp::Mul => current.mul(&rhs, scale),
                    BinOp::Div => current.div(&rhs, scale),
                    BinOp::Mod => current.modulo(&rhs, scale),
                    BinOp::Pow => current.pow(&rhs, scale),
                };
                self.assign_to(target, result.clone());
                result
            }
            Expr::PreInc(e) => {
                let val = self.eval_lvalue(e).add(&BcNum::from_i64(1));
                self.assign_to(e, val.clone());
                val
            }
            Expr::PreDec(e) => {
                let val = self.eval_lvalue(e).sub(&BcNum::from_i64(1));
                self.assign_to(e, val.clone());
                val
            }
            Expr::PostInc(e) => {
                let val = self.eval_lvalue(e);
                let new_val = val.add(&BcNum::from_i64(1));
                self.assign_to(e, new_val);
                val
            }
            Expr::PostDec(e) => {
                let val = self.eval_lvalue(e);
                let new_val = val.sub(&BcNum::from_i64(1));
                self.assign_to(e, new_val);
                val
            }
            Expr::Call(name, args) => self.call_func(name, args),
            Expr::Compare(lhs, op, rhs) => {
                let a = self.eval(lhs);
                let b = self.eval(rhs);
                let cmp = a.cmp(&b);
                let result = match op {
                    CmpOp::Eq => cmp == 0,
                    CmpOp::Ne => cmp != 0,
                    CmpOp::Lt => cmp < 0,
                    CmpOp::Gt => cmp > 0,
                    CmpOp::Le => cmp <= 0,
                    CmpOp::Ge => cmp >= 0,
                };
                if result {
                    BcNum::from_i64(1)
                } else {
                    BcNum::zero()
                }
            }
            Expr::Logical(lhs, op, rhs) => match op {
                LogOp::And => {
                    let a = self.eval(lhs);
                    if a.is_zero() {
                        BcNum::zero()
                    } else {
                        let b = self.eval(rhs);
                        if b.is_zero() {
                            BcNum::zero()
                        } else {
                            BcNum::from_i64(1)
                        }
                    }
                }
                LogOp::Or => {
                    let a = self.eval(lhs);
                    if !a.is_zero() {
                        BcNum::from_i64(1)
                    } else {
                        let b = self.eval(rhs);
                        if !b.is_zero() {
                            BcNum::from_i64(1)
                        } else {
                            BcNum::zero()
                        }
                    }
                }
            },
        }
    }

    fn eval_lvalue(&mut self, expr: &Expr) -> BcNum {
        match expr {
            Expr::Var(name) => self.get_var(name),
            Expr::ArrayAccess(name, idx) => {
                let idx_val = self.eval(idx);
                let idx_str = idx_val.rescale(0).format(10);
                self.get_array(name, &idx_str)
            }
            _ => self.eval(expr),
        }
    }

    fn assign_to(&mut self, target: &Expr, val: BcNum) {
        match target {
            Expr::Var(name) => self.set_var(name, val),
            Expr::ArrayAccess(name, idx) => {
                let idx_val = self.eval(idx);
                let idx_str = idx_val.rescale(0).format(10);
                self.set_array(name, &idx_str, val);
            }
            _ => {} // Cannot assign to non-lvalue.
        }
    }

    fn call_func(&mut self, name: &str, args: &[Expr]) -> BcNum {
        // Built-in functions.
        match name {
            "sqrt" => {
                if args.is_empty() {
                    return BcNum::zero();
                }
                let x = self.eval(&args[0]);
                return x.sqrt(self.scale);
            }
            "length" => {
                if args.is_empty() {
                    return BcNum::zero();
                }
                let x = self.eval(&args[0]);
                return BcNum::from_i64(x.length() as i64);
            }
            "scale" if !args.is_empty() => {
                let x = self.eval(&args[0]);
                return BcNum::from_i64(x.scale as i64);
            }
            "read" => {
                let mut line = String::new();
                let _ = io::stdin().read_line(&mut line);
                return BcNum::parse(line.trim(), self.ibase);
            }
            _ => {}
        }

        // Math library functions (available with -l).
        if self.math_lib {
            match name {
                "s" => {
                    if args.is_empty() {
                        return BcNum::zero();
                    }
                    let x = self.eval(&args[0]);
                    return self.builtin_sin(x);
                }
                "c" => {
                    if args.is_empty() {
                        return BcNum::zero();
                    }
                    let x = self.eval(&args[0]);
                    return self.builtin_cos(x);
                }
                "a" => {
                    if args.is_empty() {
                        return BcNum::zero();
                    }
                    let x = self.eval(&args[0]);
                    return self.builtin_atan(x);
                }
                "l" => {
                    if args.is_empty() {
                        return BcNum::zero();
                    }
                    let x = self.eval(&args[0]);
                    return self.builtin_ln(x);
                }
                "e" => {
                    if args.is_empty() {
                        return BcNum::zero();
                    }
                    let x = self.eval(&args[0]);
                    return self.builtin_exp(x);
                }
                "j" => {
                    if args.len() < 2 {
                        return BcNum::zero();
                    }
                    let n = self.eval(&args[0]);
                    let x = self.eval(&args[1]);
                    return self.builtin_bessel(n, x);
                }
                _ => {}
            }
        }

        // User-defined function.
        let func = match self.funcs.get(name) {
            Some(f) => f.clone(),
            None => {
                eprintln!("Runtime error: undefined function {}", name);
                return BcNum::zero();
            }
        };

        // Evaluate arguments.
        let arg_vals: Vec<BcNum> = args.iter().map(|a| self.eval(a)).collect();

        // Save variables that will be shadowed.
        let mut saved = Vec::new();
        for (i, param) in func.params.iter().enumerate() {
            saved.push((param.clone(), self.vars.get(param).cloned()));
            let val = arg_vals.get(i).cloned().unwrap_or_else(BcNum::zero);
            self.vars.insert(param.clone(), val);
        }
        for auto_var in &func.auto_vars {
            saved.push((auto_var.clone(), self.vars.get(auto_var).cloned()));
            self.vars.insert(auto_var.clone(), BcNum::zero());
        }

        // Execute body.
        let mut result = BcNum::zero();
        for s in &func.body {
            match self.exec_stmt(s) {
                StmtResult::Normal => {}
                StmtResult::Return(v) => {
                    result = v;
                    break;
                }
                StmtResult::Break | StmtResult::Continue => break,
            }
        }

        // Restore saved variables.
        for (name_key, old_val) in saved {
            match old_val {
                Some(v) => {
                    self.vars.insert(name_key, v);
                }
                None => {
                    self.vars.remove(&name_key);
                }
            }
        }

        result
    }

    // -----------------------------------------------------------------
    // Math library built-in functions (Taylor series implementations)
    // -----------------------------------------------------------------

    /// sin(x) using Taylor series.
    fn builtin_sin(&self, x: BcNum) -> BcNum {
        let scale = self.scale + 5; // Extra precision for intermediate calculations.
        // Reduce x modulo 2*pi for better convergence.
        let x = self.reduce_angle(x, scale);

        let mut result = BcNum::zero();
        let mut term = x.clone();
        let mut n = 1i64;
        let neg_one = BcNum::from_i64(-1);

        for _ in 0..50 {
            result = result.add(&term);
            n += 2;
            let denom = BcNum::from_i64((n - 1) * n);
            term = term.mul(&x, scale).mul(&x, scale);
            term = term.div(&denom, scale);
            term = term.mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        result.rescale(self.scale)
    }

    /// cos(x) using Taylor series.
    fn builtin_cos(&self, x: BcNum) -> BcNum {
        let scale = self.scale + 5;
        let x = self.reduce_angle(x, scale);

        let mut result = BcNum::zero();
        let mut term = BcNum::one();
        let mut n = 0i64;
        let neg_one = BcNum::from_i64(-1);

        for _ in 0..50 {
            result = result.add(&term);
            n += 2;
            let denom = BcNum::from_i64((n - 1) * n);
            term = term.mul(&x, scale).mul(&x, scale);
            term = term.div(&denom, scale);
            term = term.mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        result.rescale(self.scale)
    }

    /// atan(x) using Taylor series (converges for |x| <= 1).
    /// For |x| > 1, use identity: atan(x) = pi/2 - atan(1/x).
    fn builtin_atan(&self, x: BcNum) -> BcNum {
        let scale = self.scale + 5;
        let one = BcNum::from_i64(1);

        // Check |x| > 1.
        if x.abs().cmp(&one) > 0 {
            let pi_half = self.compute_pi(scale).div(&BcNum::from_i64(2), scale);
            let inv = one.div(&x, scale);
            let atan_inv = self.atan_series(inv, scale);
            let result = if x.is_negative() {
                pi_half.negate().sub(&atan_inv)
            } else {
                pi_half.sub(&atan_inv)
            };
            return result.rescale(self.scale);
        }
        self.atan_series(x, scale).rescale(self.scale)
    }

    fn atan_series(&self, x: BcNum, scale: usize) -> BcNum {
        let mut result = BcNum::zero();
        let mut term = x.clone();
        let x_sq = x.mul(&x, scale);
        let neg_one = BcNum::from_i64(-1);

        for i in 0..100 {
            let denom = BcNum::from_i64(2 * i + 1);
            let contrib = term.div(&denom, scale);
            result = result.add(&contrib);
            term = term.mul(&x_sq, scale).mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        result
    }

    /// Natural logarithm using series: ln(x) = 2 * sum( ((x-1)/(x+1))^(2k+1) / (2k+1) ).
    fn builtin_ln(&self, x: BcNum) -> BcNum {
        if x.is_zero() || x.is_negative() {
            eprintln!("Runtime error: log of non-positive number");
            return BcNum::zero();
        }
        let scale = self.scale + 5;
        let one = BcNum::from_i64(1);

        // For better convergence, reduce x: ln(x) = ln(m * 2^e) = ln(m) + e*ln(2).
        // Simple approach: just use the series for values near 1.
        // Factor out powers of e (or 2) to bring x close to 1.
        let two = BcNum::from_i64(2);
        let mut val = x.clone();
        let mut exp_count: i64 = 0;

        // Bring val into [0.5, 2) by dividing/multiplying by 2.
        while val.cmp(&two) > 0 {
            val = val.div(&two, scale);
            exp_count += 1;
        }
        let half = one.div(&two, scale);
        while val.cmp(&half) < 0 {
            val = val.mul(&two, scale);
            exp_count -= 1;
        }

        // Now compute ln(val) using the series.
        let num = val.sub(&one);
        let den = val.add(&one);
        let ratio = num.div(&den, scale);
        let ratio_sq = ratio.mul(&ratio, scale);

        let mut result = BcNum::zero();
        let mut term = ratio.clone();

        for i in 0..100 {
            let denom = BcNum::from_i64(2 * i + 1);
            let contrib = term.div(&denom, scale);
            result = result.add(&contrib);
            term = term.mul(&ratio_sq, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        result = result.mul(&two, scale);

        // Add back the exp_count * ln(2).
        if exp_count != 0 {
            let ln2 = self.compute_ln2(scale);
            result = result.add(&ln2.mul(&BcNum::from_i64(exp_count), scale));
        }
        result.rescale(self.scale)
    }

    /// e^x using Taylor series.
    fn builtin_exp(&self, x: BcNum) -> BcNum {
        let scale = self.scale + 5;
        let mut result = BcNum::one();
        let mut term = BcNum::one();

        for n in 1..100 {
            term = term.mul(&x, scale);
            term = term.div(&BcNum::from_i64(n), scale);
            result = result.add(&term);
            if term.is_negligible(scale) {
                break;
            }
        }
        result.rescale(self.scale)
    }

    /// Bessel function J(n, x) using series expansion.
    fn builtin_bessel(&self, n: BcNum, x: BcNum) -> BcNum {
        let scale = self.scale + 5;
        let n_int = {
            let s = n.rescale(0).format(10);
            s.parse::<i64>().unwrap_or(0).unsigned_abs()
        };

        let x_half = x.div(&BcNum::from_i64(2), scale);
        let neg_x_sq_4 = x.mul(&x, scale).negate().div(&BcNum::from_i64(4), scale);

        // (x/2)^n / n!
        let mut pow = BcNum::one();
        for _ in 0..n_int {
            pow = pow.mul(&x_half, scale);
        }
        let mut factorial = BcNum::one();
        for i in 1..=n_int {
            factorial = factorial.mul(&BcNum::from_i64(i as i64), scale);
        }
        let mut term = pow.div(&factorial, scale);
        let mut result = term.clone();

        for k in 1i64..100 {
            // term *= -x^2/4 / (k * (n + k))
            let denom = BcNum::from_i64(k * (n_int as i64 + k));
            term = term.mul(&neg_x_sq_4, scale).div(&denom, scale);
            result = result.add(&term);
            if term.is_negligible(scale) {
                break;
            }
        }
        result.rescale(self.scale)
    }

    /// Compute pi to the given scale using Machin's formula:
    /// pi/4 = 4*atan(1/5) - atan(1/239).
    fn compute_pi(&self, scale: usize) -> BcNum {
        let one = BcNum::from_i64(1);
        let four = BcNum::from_i64(4);
        let a1 = one.div(&BcNum::from_i64(5), scale);
        let a2 = one.div(&BcNum::from_i64(239), scale);
        let t1 = self.atan_series(a1, scale);
        let t2 = self.atan_series(a2, scale);
        four.mul(&t1, scale)
            .sub(&t2)
            .mul(&four, scale)
    }

    /// Compute ln(2) to the given scale.
    fn compute_ln2(&self, scale: usize) -> BcNum {
        let one = BcNum::from_i64(1);
        let two = BcNum::from_i64(2);
        // ln(2) via the series for ln((1+y)/(1-y)) where y = 1/3.
        let num = two.sub(&one); // 1
        let den = two.add(&one); // 3
        let ratio = num.div(&den, scale);
        let ratio_sq = ratio.mul(&ratio, scale);
        let mut result = BcNum::zero();
        let mut term = ratio.clone();
        for i in 0..100 {
            let denom = BcNum::from_i64(2 * i + 1);
            let contrib = term.div(&denom, scale);
            result = result.add(&contrib);
            term = term.mul(&ratio_sq, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        result.mul(&two, scale)
    }

    /// Reduce angle modulo 2*pi for trig functions.
    fn reduce_angle(&self, x: BcNum, scale: usize) -> BcNum {
        let two_pi = self.compute_pi(scale).mul(&BcNum::from_i64(2), scale);
        if two_pi.is_zero() {
            return x;
        }
        let abs_x = x.abs();
        if abs_x.cmp(&two_pi) <= 0 {
            return x;
        }
        // x mod 2pi.
        let q = x.div(&two_pi, 0).rescale(0);
        
        x.sub(&q.mul(&two_pi, scale))
    }
}

/// Returns true if the expression is a pure assignment (should not auto-print).
fn is_assignment_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Assign(_, _)
            | Expr::OpAssign(_, _, _)
            | Expr::PreInc(_)
            | Expr::PreDec(_)
            | Expr::PostInc(_)
            | Expr::PostDec(_)
    )
}

// -------------------------------------------------------------------------
// Main entry point
// -------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut math_lib = false;
    let mut quiet = false;
    let mut expr_to_eval: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-l" => math_lib = true,
            "-q" => quiet = true,
            "-e" => {
                i += 1;
                if i < args.len() {
                    expr_to_eval = Some(args[i].clone());
                }
            }
            arg if arg.starts_with('-') => {
                // Flags combined: -lq etc.
                for ch in arg[1..].chars() {
                    match ch {
                        'l' => math_lib = true,
                        'q' => quiet = true,
                        _ => eprintln!("bc: unknown option: -{}", ch),
                    }
                }
            }
            _ => files.push(args[i].clone()),
        }
        i += 1;
    }

    let mut interp = Interpreter::new(math_lib);

    // -e: evaluate expression and exit.
    if let Some(expr_str) = expr_to_eval {
        let mut parser = Parser::new(&expr_str);
        let stmts = parser.parse_program();
        interp.run(&stmts);
        return;
    }

    // Process input files.
    if !files.is_empty() {
        for file in &files {
            match std::fs::read_to_string(file) {
                Ok(content) => {
                    let mut parser = Parser::new(&content);
                    let stmts = parser.parse_program();
                    interp.run(&stmts);
                }
                Err(e) => {
                    eprintln!("bc: cannot open '{}': {}", file, e);
                }
            }
        }
        return;
    }

    // Interactive/pipe mode.
    let stdin = io::stdin();
    let is_tty = std::env::var("TERM").is_ok();

    if !quiet && is_tty {
        println!("bc 1.0 (Slate OS)");
        println!("Type 'quit' to exit.");
    }

    // Read all input, accumulating multi-line constructs.
    let mut buffer = String::new();
    let mut brace_depth: i32 = 0;

    for line_result in stdin.lock().lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        // Track brace depth for multi-line input.
        for ch in line.chars() {
            if ch == '{' {
                brace_depth += 1;
            } else if ch == '}' {
                brace_depth -= 1;
            }
        }
        buffer.push_str(&line);
        buffer.push('\n');

        // If braces are balanced, parse and execute.
        if brace_depth <= 0 {
            brace_depth = 0;
            let input = std::mem::take(&mut buffer);
            let mut parser = Parser::new(&input);
            let stmts = parser.parse_program();
            interp.run(&stmts);
        }
    }

    // Process any remaining buffer.
    if !buffer.is_empty() {
        let mut parser = Parser::new(&buffer);
        let stmts = parser.parse_program();
        interp.run(&stmts);
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: evaluate an expression string and return the formatted result.
    fn eval_expr(input: &str) -> String {
        let mut interp = Interpreter::new(false);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        // For tests: the last value is stored in `last`.
        for stmt in &stmts {
            interp.exec_stmt(stmt);
        }
        interp.last.format(interp.obase)
    }

    #[allow(dead_code)]
    fn eval_expr_ml(input: &str) -> String {
        let mut interp = Interpreter::new(true);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        for stmt in &stmts {
            interp.exec_stmt(stmt);
        }
        interp.last.format(interp.obase)
    }

    // Capture output from the interpreter.  Uses the output_buf field that
    // is active in test builds.
    fn capture_output(input: &str) -> Vec<String> {
        let mut interp = Interpreter::new(false);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        interp.run(&stmts);
        interp.output_buf
    }

    fn capture_output_ml(input: &str) -> Vec<String> {
        let mut interp = Interpreter::new(true);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        interp.run(&stmts);
        interp.output_buf
    }

    // --- BcNum tests ---

    #[test]
    fn test_bcnum_parse_integer() {
        let n = BcNum::parse("42", 10);
        assert_eq!(n.format(10), "42");
    }

    #[test]
    fn test_bcnum_parse_decimal() {
        let n = BcNum::parse("3.14", 10);
        assert_eq!(n.format(10), "3.14");
    }

    #[test]
    fn test_bcnum_add() {
        let a = BcNum::parse("1.5", 10);
        let b = BcNum::parse("2.3", 10);
        assert_eq!(a.add(&b).format(10), "3.8");
    }

    #[test]
    fn test_bcnum_sub() {
        let a = BcNum::parse("5.5", 10);
        let b = BcNum::parse("2.3", 10);
        assert_eq!(a.sub(&b).format(10), "3.2");
    }

    #[test]
    fn test_bcnum_mul() {
        let a = BcNum::parse("2.5", 10);
        let b = BcNum::parse("4.0", 10);
        assert_eq!(a.mul(&b, 2).format(10), "10");
    }

    #[test]
    fn test_bcnum_div() {
        let a = BcNum::parse("10", 10);
        let b = BcNum::parse("3", 10);
        assert_eq!(a.div(&b, 5).format(10), "3.33333");
    }

    #[test]
    fn test_bcnum_mod() {
        let a = BcNum::parse("10", 10);
        let b = BcNum::parse("3", 10);
        // 10 / 3 = 3 (at scale 0), 3*3 = 9, 10-9 = 1
        assert_eq!(a.modulo(&b, 0).format(10), "1");
    }

    #[test]
    fn test_bcnum_pow() {
        let a = BcNum::parse("2", 10);
        let b = BcNum::parse("10", 10);
        assert_eq!(a.pow(&b, 0).format(10), "1024");
    }

    #[test]
    fn test_bcnum_sqrt() {
        let a = BcNum::parse("2", 10);
        let result = a.sqrt(10);
        // Should be approximately 1.4142135623.
        let s = result.format(10);
        assert!(s.starts_with("1.414213562"), "got: {}", s);
    }

    #[test]
    fn test_bcnum_length() {
        let n = BcNum::parse("12345.678", 10);
        assert_eq!(n.length(), 8); // 8 significant digits (12345678)
    }

    #[test]
    fn test_bcnum_negative() {
        let n = BcNum::parse("-42", 10);
        assert!(n.is_negative());
        assert_eq!(n.format(10), "-42");
    }

    #[test]
    fn test_bcnum_negate() {
        let n = BcNum::parse("42", 10);
        let neg = n.negate();
        assert_eq!(neg.format(10), "-42");
        let pos = neg.negate();
        assert_eq!(pos.format(10), "42");
    }

    // --- Expression evaluation tests ---

    #[test]
    fn test_simple_add() {
        assert_eq!(eval_expr("2+3"), "5");
    }

    #[test]
    fn test_simple_mul() {
        assert_eq!(eval_expr("6*7"), "42");
    }

    #[test]
    fn test_precedence() {
        assert_eq!(eval_expr("2+3*4"), "14");
    }

    #[test]
    fn test_parens() {
        assert_eq!(eval_expr("(2+3)*4"), "20");
    }

    #[test]
    fn test_power() {
        assert_eq!(eval_expr("2^10"), "1024");
    }

    #[test]
    fn test_unary_minus() {
        assert_eq!(eval_expr("-5+10"), "5");
    }

    // --- Variable tests ---

    #[test]
    fn test_variable_assign_and_use() {
        let output = capture_output("x=5\nx+3");
        assert_eq!(output, vec!["8"]);
    }

    #[test]
    fn test_scale_variable() {
        let output = capture_output("scale=5\n10/3");
        assert_eq!(output, vec!["3.33333"]);
    }

    #[test]
    fn test_increment() {
        let output = capture_output("x=5\nx++\nx");
        // x++ returns 5 (post-increment), then x is 6
        assert_eq!(output, vec!["6"]);
    }

    #[test]
    fn test_pre_increment() {
        let _output = capture_output("x=5\n++x");
        // ++x returns 6 (pre-increment)
        // But ++x is an assignment expr so it doesn't auto-print.
        // x is now 6, check via bare expression.
        let output2 = capture_output("x=5\n++x\nx");
        assert_eq!(output2, vec!["6"]);
    }

    // --- Function definition tests ---

    #[test]
    fn test_user_function() {
        let output = capture_output("define double(x) { return 2*x }\ndouble(21)");
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn test_recursive_function() {
        let output = capture_output(
            "define fact(n) { if (n <= 1) return 1\nreturn n * fact(n-1) }\nfact(10)",
        );
        assert_eq!(output, vec!["3628800"]);
    }

    // --- Control flow tests ---

    #[test]
    fn test_if_true() {
        let output = capture_output("if (1) 42");
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn test_if_false() {
        let output = capture_output("if (0) 42");
        assert!(output.is_empty());
    }

    #[test]
    fn test_while_loop() {
        let output = capture_output("x=0\nwhile (x < 5) { x = x + 1 }\nx");
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn test_for_loop() {
        let output = capture_output("s=0\nfor (i=1; i<=10; i=i+1) { s = s + i }\ns");
        assert_eq!(output, vec!["55"]);
    }

    // --- Comparison tests ---

    #[test]
    fn test_comparison_eq() {
        assert_eq!(eval_expr("5 == 5"), "1");
        assert_eq!(eval_expr("5 == 6"), "0");
    }

    #[test]
    fn test_comparison_ne() {
        assert_eq!(eval_expr("5 != 6"), "1");
        assert_eq!(eval_expr("5 != 5"), "0");
    }

    #[test]
    fn test_comparison_lt() {
        assert_eq!(eval_expr("3 < 5"), "1");
        assert_eq!(eval_expr("5 < 3"), "0");
    }

    #[test]
    fn test_comparison_gt() {
        assert_eq!(eval_expr("5 > 3"), "1");
        assert_eq!(eval_expr("3 > 5"), "0");
    }

    #[test]
    fn test_comparison_le() {
        assert_eq!(eval_expr("5 <= 5"), "1");
        assert_eq!(eval_expr("6 <= 5"), "0");
    }

    #[test]
    fn test_comparison_ge() {
        assert_eq!(eval_expr("5 >= 5"), "1");
        assert_eq!(eval_expr("4 >= 5"), "0");
    }

    // --- Base conversion tests ---

    #[test]
    fn test_obase_hex() {
        let output = capture_output("obase=16\n255");
        assert_eq!(output, vec!["FF"]);
    }

    #[test]
    fn test_ibase_hex() {
        let output = capture_output("ibase=16\nFF");
        assert_eq!(output, vec!["255"]);
    }

    #[test]
    fn test_obase_binary() {
        let output = capture_output("obase=2\n10");
        assert_eq!(output, vec!["1010"]);
    }

    // --- Math library tests (need -l) ---

    #[test]
    fn test_sqrt_builtin() {
        let output = capture_output_ml("scale=10\nsqrt(2)");
        assert!(!output.is_empty());
        let s = &output[0];
        assert!(s.starts_with("1.414213562"), "got: {}", s);
    }

    #[test]
    fn test_exp_of_zero() {
        let output = capture_output_ml("scale=10\ne(0)");
        assert!(!output.is_empty());
        assert_eq!(output[0], "1");
    }

    #[test]
    fn test_exp_of_one() {
        let output = capture_output_ml("scale=10\ne(1)");
        assert!(!output.is_empty());
        let s = &output[0];
        assert!(s.starts_with("2.71828182"), "got: {}", s);
    }

    // --- Arbitrary precision test ---

    #[test]
    fn test_large_factorial() {
        let output =
            capture_output("define fact(n) { if (n <= 1) return 1\nreturn n*fact(n-1) }\nfact(20)");
        assert_eq!(output, vec!["2432902008176640000"]);
    }

    #[test]
    fn test_large_power() {
        let output = capture_output("2^100");
        assert_eq!(output, vec!["1267650600228229401496703205376"]);
    }

    // --- Edge cases ---

    #[test]
    fn test_division_by_zero() {
        // Should not panic, returns 0.
        let output = capture_output("10/0");
        assert_eq!(output, vec!["0"]);
    }

    #[test]
    fn test_empty_input() {
        let output = capture_output("");
        assert!(output.is_empty());
    }

    #[test]
    fn test_comments() {
        let output = capture_output("/* this is a comment */\n5+3 # inline comment");
        assert_eq!(output, vec!["8"]);
    }

    #[test]
    fn test_multiline_function() {
        let input = r#"
define sum_to(n) {
    auto s, i
    s = 0
    for (i = 1; i <= n; i = i + 1) {
        s = s + i
    }
    return s
}
sum_to(100)
"#;
        let output = capture_output(input);
        assert_eq!(output, vec!["5050"]);
    }

    #[test]
    fn test_nested_functions() {
        let input = r#"
define square(x) { return x*x }
define sum_of_squares(a, b) { return square(a) + square(b) }
sum_of_squares(3, 4)
"#;
        let output = capture_output(input);
        assert_eq!(output, vec!["25"]);
    }

    #[test]
    fn test_break_in_loop() {
        let input = r#"
x = 0
while (1) {
    x = x + 1
    if (x == 5) break
}
x
"#;
        let output = capture_output(input);
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn test_continue_in_loop() {
        let input = r#"
s = 0
for (i = 1; i <= 10; i = i + 1) {
    if (i % 2 == 0) continue
    s = s + i
}
s
"#;
        // Sum of odd numbers 1+3+5+7+9 = 25
        let output = capture_output(input);
        assert_eq!(output, vec!["25"]);
    }

    #[test]
    fn test_logical_and() {
        assert_eq!(eval_expr("1 && 1"), "1");
        assert_eq!(eval_expr("1 && 0"), "0");
        assert_eq!(eval_expr("0 && 1"), "0");
    }

    #[test]
    fn test_logical_or() {
        assert_eq!(eval_expr("0 || 1"), "1");
        assert_eq!(eval_expr("0 || 0"), "0");
        assert_eq!(eval_expr("1 || 0"), "1");
    }

    #[test]
    fn test_not_operator() {
        assert_eq!(eval_expr("!0"), "1");
        assert_eq!(eval_expr("!1"), "0");
        assert_eq!(eval_expr("!42"), "0");
    }

    #[test]
    fn test_compound_assignment() {
        let output = capture_output("x=10\nx+=5\nx");
        assert_eq!(output, vec!["15"]);
    }

    #[test]
    fn test_string_in_print() {
        // Just verifying print with string doesn't crash.
        let mut interp = Interpreter::new(false);
        let mut parser = Parser::new("print \"hello\\n\"");
        let stmts = parser.parse_program();
        interp.run(&stmts);
    }

    #[test]
    fn test_if_else() {
        let output = capture_output("if (0) 1 else 2");
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn test_negative_exponent() {
        let output = capture_output("scale=5\n2^-3");
        assert_eq!(output, vec!["0.125"]);
    }
}
