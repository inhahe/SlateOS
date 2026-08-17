//! Slate OS `bc` -- arbitrary-precision calculator
//!
//! A POSIX-compatible `bc` implementation with extensions.  Supports
//! arbitrary-precision integers and fixed-point decimals, variables,
//! user-defined functions, control flow, and the `-l` math library.
//!
//! Architecture: hand-written lexer -> recursive-descent parser -> AST ->
//! tree-walk interpreter.  The numbers are `bignum::Decimal`, shared with `dc`.

use std::collections::HashMap;
use std::env;
#[cfg(not(test))]
use std::io::Write;
use std::io::{self, BufRead};
use std::process;

// -------------------------------------------------------------------------
// The numbers live in the `bignum` crate
// -------------------------------------------------------------------------
//
// `BigInt` moved there first, so that `bc`, `dc`, `genius-cli` and `expr` could
// not disagree about what an exact integer is. `Decimal` -- this file's former
// private `BcNum`, a `BigInt` mantissa and a decimal scale -- followed for the
// same reason and a sharper one: `dc` had no equivalent at all and computed in
// `f64`, so the two halves of one calculator disagreed above 2^53.
//
// The lift changed three things, and every one of them is visible from here:
// `div`, `modulo` and `sqrt` now return a `Result` instead of printing to
// stderr and handing back zero; the parse and format paths no longer index or
// slice; and `Ord` is implemented, so `1.5 == 1.50` and the relational
// operators go through it. See `bignum::decimal` for the reasoning.

use bignum::{Decimal, DecimalError};

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

    /// The byte `offset` positions past the cursor, or `None` past the end.
    ///
    /// The lexer's lookahead is all one or two bytes deep, and every site that
    /// wants it used to spell it `self.pos + n < self.input.len() &&
    /// self.input[self.pos + n] == …` — an addition that can overflow and an
    /// index that can panic, repeated eight times, each repetition another
    /// chance to get the bound wrong. One accessor that cannot do either is
    /// both shorter at the call site and impossible to misuse.
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.input.get(self.pos.checked_add(offset)?).copied()
    }

    /// Move the cursor forward `n` bytes, stopping at the end of the input.
    fn bump(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.input.len());
    }

    /// The bytes from `start` to the cursor, as text.
    ///
    /// `start` is always a cursor value this lexer produced, so the range is
    /// in bounds and lies on a token boundary; `get` rather than a slice
    /// expression states that without asking the reader to trust it.
    fn slice_from(&self, start: usize) -> &str {
        self.input
            .get(start..self.pos)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.peek_byte();
        if b.is_some() {
            self.bump(1);
        }
        b
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip spaces and tabs (but not newlines -- they are significant).
            while let Some(b) = self.peek_byte() {
                if b == b' ' || b == b'\t' || b == b'\r' || b == b'\\' {
                    // A backslash-newline is a line continuation: both bytes go.
                    if b == b'\\' && self.peek_at(1) == Some(b'\n') {
                        self.bump(2);
                    } else {
                        self.bump(1);
                    }
                } else {
                    break;
                }
            }
            // Skip /* ... */ comments.
            if self.peek_byte() == Some(b'/') && self.peek_at(1) == Some(b'*') {
                self.bump(2);
                loop {
                    match (self.peek_byte(), self.peek_at(1)) {
                        // An unterminated comment runs to end of input, which
                        // is what `None` here means; stop rather than spin.
                        (None, _) | (_, None) => {
                            self.bump(1);
                            break;
                        }
                        (Some(b'*'), Some(b'/')) => {
                            self.bump(2);
                            break;
                        }
                        _ => self.bump(1),
                    }
                }
                continue;
            }
            // Skip # comments.
            if let Some(b'#') = self.peek_byte() {
                while let Some(b) = self.peek_byte() {
                    if b == b'\n' {
                        break;
                    }
                    self.bump(1);
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
            || (b == b'.' && self.peek_at(1).is_some_and(|n| n.is_ascii_hexdigit()))
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
        Token::Number(self.slice_from(start).to_string())
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
        match self.slice_from(start) {
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
            other => Token::Ident(other.to_string()),
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

#[derive(Clone, Copy, Debug)]
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
        // Saturating rather than wrapping: at the end of input `peek` already
        // answers `Eof` for any position past the last token, so a cursor that
        // stops advancing is exactly the right behaviour, whereas one that
        // wraps to zero would send the parser back to the start of the program.
        self.pos = self.pos.saturating_add(1);
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
    Return(Decimal),
    Break,
    Continue,
}

/// What a loop should do after running its body once.
enum LoopFlow {
    /// Go round again — the body ended normally or hit `continue`.
    Continue,
    Break,
    Return(Decimal),
}

/// Something that makes the rest of the current statement meaningless.
///
/// Before `Decimal` moved to `bignum`, there was no such type: a division by
/// zero printed to stderr from inside the arithmetic and returned zero, so
/// `x = 1/0 + 5` assigned 5 and the program carried on as though the user had
/// written `0`. That is the one outcome a calculator must not have. These
/// propagate to [`Interpreter::run`], which prints them and abandons the rest of
/// the input line — which is what GNU `bc` does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeError {
    /// The arithmetic itself could not produce a value.
    Math(DecimalError),
    /// A call to a name that is neither a builtin nor a defined function.
    UndefinedFunction(String),
    /// `l(x)` for `x <= 0`, where the logarithm is not defined over the reals.
    LogOfNonPositive,
}

impl From<DecimalError> for RuntimeError {
    fn from(e: DecimalError) -> Self {
        Self::Math(e)
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Math(e) => write!(f, "{e}"),
            Self::UndefinedFunction(name) => write!(f, "undefined function {name}"),
            Self::LogOfNonPositive => f.write_str("log of non-positive number"),
        }
    }
}

/// The result of evaluating an expression: a number, or the reason there is not
/// one.
type Eval = Result<Decimal, RuntimeError>;

struct Interpreter {
    /// Named variables.
    vars: HashMap<String, Decimal>,
    /// Array variables: name -> (index -> value).
    arrays: HashMap<String, HashMap<String, Decimal>>,
    /// User-defined functions.
    funcs: HashMap<String, FuncDef>,
    /// scale, ibase, obase.
    scale: usize,
    ibase: u32,
    obase: u32,
    /// Last printed value.
    last: Decimal,
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
            last: Decimal::zero(),
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

    fn get_var(&self, name: &str) -> Decimal {
        match name {
            "scale" => Decimal::from_i64(self.scale as i64),
            "ibase" => Decimal::from_i64(self.ibase as i64),
            "obase" => Decimal::from_i64(self.obase as i64),
            _ => self.vars.get(name).cloned().unwrap_or_else(Decimal::zero),
        }
    }

    fn set_var(&mut self, name: &str, val: Decimal) {
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

    fn get_array(&self, name: &str, idx: &str) -> Decimal {
        self.arrays
            .get(name)
            .and_then(|m| m.get(idx))
            .cloned()
            .unwrap_or_else(Decimal::zero)
    }

    fn set_array(&mut self, name: &str, idx: &str, val: Decimal) {
        self.arrays
            .entry(name.to_string())
            .or_default()
            .insert(idx.to_string(), val);
    }

    /// Execute a parsed program.
    ///
    /// This is the only place a `RuntimeError` is printed, and the granularity
    /// of recovery is the **top-level statement**: a failure abandons the
    /// statement it happened in — including the whole of a loop or an `if` it
    /// was nested inside, and the frame of any function it was inside — and
    /// then execution resumes at the next statement. Nothing partial is
    /// printed, and nothing computed from a value that was never produced is
    /// either.
    ///
    /// The alternative, abandoning the entire program, would make one mistyped
    /// expression discard the rest of a script; the alternative in the other
    /// direction, resuming inside the failed statement, is not available — the
    /// value it needed does not exist. See `design-decisions.md` §323.
    fn run(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match self.exec_stmt(stmt) {
                Ok(StmtResult::Normal) => {}
                // `break`, `continue` or `return` outside any enclosing
                // construct ends the program, as there is nothing to return to.
                Ok(_) => return,
                Err(e) => eprintln!("Runtime error: {e}"),
            }
        }
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<StmtResult, RuntimeError> {
        match stmt {
            Stmt::Expr(expr) => {
                let val = self.eval(expr)?;
                // In bc, a bare expression prints its value.
                // But assignments don't print (they are silent).
                if !suppresses_auto_print(expr) {
                    let formatted = val.format(self.obase);
                    self.output_line(&formatted);
                }
                self.last = val;
                Ok(StmtResult::Normal)
            }
            Stmt::Print(items) => {
                for item in items {
                    match item {
                        PrintItem::StringLit(s) => self.output_str(s),
                        PrintItem::Expr(expr) => {
                            let val = self.eval(expr)?;
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
                Ok(StmtResult::Normal)
            }
            Stmt::If(cond, then_body, else_body) => {
                let val = self.eval(cond)?;
                let branch = if val.is_zero() {
                    else_body.as_ref()
                } else {
                    Some(then_body)
                };
                if let Some(body) = branch {
                    for s in body {
                        match self.exec_stmt(s)? {
                            StmtResult::Normal => {}
                            other => return Ok(other),
                        }
                    }
                }
                Ok(StmtResult::Normal)
            }
            Stmt::While(cond, body) => {
                while !self.eval(cond)?.is_zero() {
                    match self.exec_body(body)? {
                        LoopFlow::Continue => {}
                        LoopFlow::Break => break,
                        LoopFlow::Return(v) => return Ok(StmtResult::Return(v)),
                    }
                }
                Ok(StmtResult::Normal)
            }
            Stmt::For(init, cond, step, body) => {
                if let Some(init_expr) = init {
                    self.eval(init_expr)?;
                }
                loop {
                    if let Some(cond_expr) = cond
                        && self.eval(cond_expr)?.is_zero()
                    {
                        break;
                    }
                    match self.exec_body(body)? {
                        LoopFlow::Continue => {}
                        LoopFlow::Break => break,
                        LoopFlow::Return(v) => return Ok(StmtResult::Return(v)),
                    }
                    if let Some(step_expr) = step {
                        self.eval(step_expr)?;
                    }
                }
                Ok(StmtResult::Normal)
            }
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval(e)?,
                    None => Decimal::zero(),
                };
                Ok(StmtResult::Return(val))
            }
            Stmt::Break => Ok(StmtResult::Break),
            Stmt::Continue => Ok(StmtResult::Continue),
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
                Ok(StmtResult::Normal)
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    match self.exec_stmt(s)? {
                        StmtResult::Normal => {}
                        other => return Ok(other),
                    }
                }
                Ok(StmtResult::Normal)
            }
        }
    }

    /// Run one pass of a loop body and say what the loop should do next.
    ///
    /// `while` and `for` differ only in their headers; sharing the body keeps
    /// `continue` meaning "next iteration" in both, which is easy to get wrong
    /// when the two are written out separately — `for`'s step expression must
    /// still run.
    fn exec_body(&mut self, body: &[Stmt]) -> Result<LoopFlow, RuntimeError> {
        for s in body {
            match self.exec_stmt(s)? {
                StmtResult::Normal => {}
                StmtResult::Break => return Ok(LoopFlow::Break),
                StmtResult::Continue => return Ok(LoopFlow::Continue),
                StmtResult::Return(v) => return Ok(LoopFlow::Return(v)),
            }
        }
        Ok(LoopFlow::Continue)
    }

    fn eval(&mut self, expr: &Expr) -> Eval {
        match expr {
            Expr::Number(s) => Ok(Decimal::parse(s, self.ibase)),
            Expr::StringLit(s) => {
                // In bc, strings in expression context are printed.
                self.output_str(s);
                Ok(Decimal::zero())
            }
            Expr::Var(name) => Ok(self.get_var(name)),
            Expr::ArrayAccess(name, idx) => {
                let idx_str = self.index_of(idx)?;
                Ok(self.get_array(name, &idx_str))
            }
            Expr::Last => Ok(self.last.clone()),
            Expr::UnaryMinus(e) => Ok(self.eval(e)?.negate()),
            Expr::UnaryNot(e) => Ok(Self::boolean(self.eval(e)?.is_zero())),
            Expr::BinOp(lhs, op, rhs) => {
                let a = self.eval(lhs)?;
                let b = self.eval(rhs)?;
                self.apply(&a, *op, &b)
            }
            Expr::Assign(target, val_expr) => {
                let val = self.eval(val_expr)?;
                self.assign_to(target, val.clone())?;
                Ok(val)
            }
            Expr::OpAssign(target, op, val_expr) => {
                let current = self.eval_lvalue(target)?;
                let rhs = self.eval(val_expr)?;
                let result = self.apply(&current, *op, &rhs)?;
                self.assign_to(target, result.clone())?;
                Ok(result)
            }
            Expr::PreInc(e) => {
                let val = self.eval_lvalue(e)?.add(&Decimal::from_i64(1));
                self.assign_to(e, val.clone())?;
                Ok(val)
            }
            Expr::PreDec(e) => {
                let val = self.eval_lvalue(e)?.sub(&Decimal::from_i64(1));
                self.assign_to(e, val.clone())?;
                Ok(val)
            }
            Expr::PostInc(e) => {
                let val = self.eval_lvalue(e)?;
                let new_val = val.add(&Decimal::from_i64(1));
                self.assign_to(e, new_val)?;
                Ok(val)
            }
            Expr::PostDec(e) => {
                let val = self.eval_lvalue(e)?;
                let new_val = val.sub(&Decimal::from_i64(1));
                self.assign_to(e, new_val)?;
                Ok(val)
            }
            Expr::Call(name, args) => self.call_func(name, args),
            Expr::Compare(lhs, op, rhs) => {
                let a = self.eval(lhs)?;
                let b = self.eval(rhs)?;
                // `Decimal`'s ordering is by value, so `1.5` and `1.50` compare
                // equal here even though they are stored differently.
                let ord = a.cmp(&b);
                Ok(Self::boolean(match op {
                    CmpOp::Eq => ord.is_eq(),
                    CmpOp::Ne => ord.is_ne(),
                    CmpOp::Lt => ord.is_lt(),
                    CmpOp::Gt => ord.is_gt(),
                    CmpOp::Le => ord.is_le(),
                    CmpOp::Ge => ord.is_ge(),
                }))
            }
            // Both operators short-circuit, which is not merely an
            // optimisation: `x != 0 && 1/x > 2` must not evaluate the division
            // when `x` is zero, or it reports a runtime error the user's guard
            // was written to prevent.
            Expr::Logical(lhs, op, rhs) => match op {
                LogOp::And => {
                    if self.eval(lhs)?.is_zero() {
                        return Ok(Decimal::zero());
                    }
                    Ok(Self::boolean(!self.eval(rhs)?.is_zero()))
                }
                LogOp::Or => {
                    if !self.eval(lhs)?.is_zero() {
                        return Ok(Decimal::from_i64(1));
                    }
                    Ok(Self::boolean(!self.eval(rhs)?.is_zero()))
                }
            },
        }
    }

    /// bc's spelling of a truth value: 1 or 0, as a number like any other.
    fn boolean(b: bool) -> Decimal {
        if b {
            Decimal::from_i64(1)
        } else {
            Decimal::zero()
        }
    }

    /// One binary operator, at the interpreter's current scale.
    ///
    /// `a op b` and `a op= b` are the same arithmetic, so they are the same
    /// code — and there is exactly one place where a division by zero becomes a
    /// `RuntimeError` rather than two that could drift apart.
    fn apply(&self, a: &Decimal, op: BinOp, b: &Decimal) -> Eval {
        let scale = self.scale;
        Ok(match op {
            BinOp::Add => a.add(b),
            BinOp::Sub => a.sub(b),
            BinOp::Mul => a.mul(b, scale),
            BinOp::Div => a.div(b, scale)?,
            BinOp::Mod => a.modulo(b, scale)?,
            BinOp::Pow => a.pow(b, scale)?,
        })
    }

    /// An array subscript, rendered as the string the map is keyed by.
    ///
    /// Always base ten, never `obase`: the key is an internal identity, and
    /// keying it by the *output* base would make `a[10]` and `a[16]` the same
    /// element after `obase=16`.
    fn index_of(&mut self, idx: &Expr) -> Result<String, RuntimeError> {
        Ok(self.eval(idx)?.rescale(0).format(10))
    }

    fn eval_lvalue(&mut self, expr: &Expr) -> Eval {
        match expr {
            Expr::Var(name) => Ok(self.get_var(name)),
            Expr::ArrayAccess(name, idx) => {
                let idx_str = self.index_of(idx)?;
                Ok(self.get_array(name, &idx_str))
            }
            _ => self.eval(expr),
        }
    }

    fn assign_to(&mut self, target: &Expr, val: Decimal) -> Result<(), RuntimeError> {
        match target {
            Expr::Var(name) => self.set_var(name, val),
            Expr::ArrayAccess(name, idx) => {
                let idx_str = self.index_of(idx)?;
                self.set_array(name, &idx_str, val);
            }
            _ => {} // Cannot assign to non-lvalue.
        }
        Ok(())
    }

    fn call_func(&mut self, name: &str, args: &[Expr]) -> Eval {
        /// The first argument, or zero — bc's own reading of a call with none.
        macro_rules! arg0 {
            () => {
                match args.first() {
                    Some(a) => self.eval(a)?,
                    None => return Ok(Decimal::zero()),
                }
            };
        }

        // Built-in functions.
        match name {
            "sqrt" => return Ok(arg0!().sqrt(self.scale)?),
            "length" => return Ok(Decimal::from_i64(arg0!().length() as i64)),
            "scale" if !args.is_empty() => return Ok(Decimal::from_i64(arg0!().scale as i64)),
            "read" => {
                let mut line = String::new();
                let _ = io::stdin().read_line(&mut line);
                return Ok(Decimal::parse(line.trim(), self.ibase));
            }
            _ => {}
        }

        // Math library functions (available with -l). Each argument is bound to
        // a local before the call, because evaluating it borrows the
        // interpreter mutably and the builtin borrows it again.
        if self.math_lib {
            match name {
                "s" => {
                    let x = arg0!();
                    return self.builtin_sin(x);
                }
                "c" => {
                    let x = arg0!();
                    return self.builtin_cos(x);
                }
                "a" => {
                    let x = arg0!();
                    return self.builtin_atan(x);
                }
                "l" => {
                    let x = arg0!();
                    return self.builtin_ln(&x);
                }
                "e" => {
                    let x = arg0!();
                    return self.builtin_exp(&x);
                }
                "j" => {
                    let (Some(n_expr), Some(x_expr)) = (args.first(), args.get(1)) else {
                        return Ok(Decimal::zero());
                    };
                    let n = self.eval(n_expr)?;
                    let x = self.eval(x_expr)?;
                    return self.builtin_bessel(&n, &x);
                }
                _ => {}
            }
        }

        // User-defined function.
        let Some(func) = self.funcs.get(name).cloned() else {
            return Err(RuntimeError::UndefinedFunction(name.to_string()));
        };

        // Evaluate arguments *before* the parameters are bound, so that an
        // argument mentioning a variable the function also takes as a parameter
        // sees the caller's value rather than a half-built frame.
        let mut arg_vals = Vec::with_capacity(args.len());
        for a in args {
            arg_vals.push(self.eval(a)?);
        }

        // Save variables that will be shadowed.
        let mut saved = Vec::new();
        for (i, param) in func.params.iter().enumerate() {
            saved.push((param.clone(), self.vars.get(param).cloned()));
            let val = arg_vals.get(i).cloned().unwrap_or_else(Decimal::zero);
            self.vars.insert(param.clone(), val);
        }
        for auto_var in &func.auto_vars {
            saved.push((auto_var.clone(), self.vars.get(auto_var).cloned()));
            self.vars.insert(auto_var.clone(), Decimal::zero());
        }

        // Execute body. The result is held rather than returned, because the
        // frame has to be torn down on the failing path too: a `?` here would
        // leave the caller's variables shadowed by the callee's for the rest of
        // the session.
        let mut outcome = Ok(Decimal::zero());
        for s in &func.body {
            match self.exec_stmt(s) {
                Ok(StmtResult::Normal) => {}
                Ok(StmtResult::Return(v)) => {
                    outcome = Ok(v);
                    break;
                }
                Ok(StmtResult::Break | StmtResult::Continue) => break,
                Err(e) => {
                    outcome = Err(e);
                    break;
                }
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

        outcome
    }

    // -----------------------------------------------------------------
    // Math library built-in functions (Taylor series implementations)
    // -----------------------------------------------------------------
    //
    // Every division below is by a term the series itself produced: a loop
    // counter, a factorial, a literal, or a quantity the enclosing branch has
    // just shown to be non-zero. None of them can be driven to zero by the
    // user's expression, and each site says which case it is. They still go
    // through the fallible `div`, and the `?` still propagates -- an argument
    // that cannot be zero is a claim about this code, and if the claim is ever
    // wrong the user gets "Runtime error: divide by zero" rather than a series
    // that quietly converges to the wrong number.
    //
    // The working scale is the user's plus five guard digits, so the truncation
    // in each term does not accumulate into the digits that get printed.

    /// The extra digits carried through an iterative series.
    ///
    /// Each term truncates, and a hundred truncations at the output scale would
    /// show in the last digit or two. Five guard digits is what `bc`'s own
    /// library uses.
    const GUARD_DIGITS: usize = 5;

    fn working_scale(&self) -> usize {
        self.scale.saturating_add(Self::GUARD_DIGITS)
    }

    /// sin(x) using Taylor series.
    fn builtin_sin(&self, x: Decimal) -> Eval {
        let scale = self.working_scale();
        // Reduce x modulo 2*pi for better convergence.
        let x = self.reduce_angle(&x, scale)?;

        let mut result = Decimal::zero();
        let mut term = x.clone();
        let mut n = 1i64;
        let neg_one = Decimal::from_i64(-1);

        for _ in 0..50 {
            result = result.add(&term);
            n = n.saturating_add(2);
            // (n-1)*n for odd n >= 3, so at least 6 -- never zero.
            let denom = Decimal::from_i64(n.saturating_sub(1).saturating_mul(n));
            term = term.mul(&x, scale).mul(&x, scale);
            term = term.div(&denom, scale)?;
            term = term.mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result.rescale(self.scale))
    }

    /// cos(x) using Taylor series.
    fn builtin_cos(&self, x: Decimal) -> Eval {
        let scale = self.working_scale();
        let x = self.reduce_angle(&x, scale)?;

        let mut result = Decimal::zero();
        let mut term = Decimal::one();
        let mut n = 0i64;
        let neg_one = Decimal::from_i64(-1);

        for _ in 0..50 {
            result = result.add(&term);
            n = n.saturating_add(2);
            // (n-1)*n for even n >= 2, so at least 2 -- never zero.
            let denom = Decimal::from_i64(n.saturating_sub(1).saturating_mul(n));
            term = term.mul(&x, scale).mul(&x, scale);
            term = term.div(&denom, scale)?;
            term = term.mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result.rescale(self.scale))
    }

    /// atan(x) using Taylor series (converges for |x| <= 1).
    /// For |x| > 1, use identity: atan(x) = pi/2 - atan(1/x).
    fn builtin_atan(&self, x: Decimal) -> Eval {
        let scale = self.working_scale();
        let one = Decimal::from_i64(1);

        if x.abs() > one {
            let pi_half = self.compute_pi(scale)?.div(&Decimal::from_i64(2), scale)?;
            // |x| > 1 is exactly the branch condition, so x is not zero.
            let inv = one.div(&x, scale)?;
            let atan_inv = self.atan_series(&inv, scale)?;
            let result = if x.is_negative() {
                pi_half.negate().sub(&atan_inv)
            } else {
                pi_half.sub(&atan_inv)
            };
            return Ok(result.rescale(self.scale));
        }
        Ok(self.atan_series(&x, scale)?.rescale(self.scale))
    }

    fn atan_series(&self, x: &Decimal, scale: usize) -> Eval {
        let mut result = Decimal::zero();
        let mut term = x.clone();
        let x_sq = x.mul(x, scale);
        let neg_one = Decimal::from_i64(-1);

        for i in 0..100i64 {
            // 2i+1 is odd, so never zero.
            let denom = Decimal::from_i64(i.saturating_mul(2).saturating_add(1));
            let contrib = term.div(&denom, scale)?;
            result = result.add(&contrib);
            term = term.mul(&x_sq, scale).mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result)
    }

    /// Natural logarithm using series: ln(x) = 2 * sum( ((x-1)/(x+1))^(2k+1) / (2k+1) ).
    fn builtin_ln(&self, x: &Decimal) -> Eval {
        if x.is_zero() || x.is_negative() {
            return Err(RuntimeError::LogOfNonPositive);
        }
        let scale = self.working_scale();
        let one = Decimal::from_i64(1);

        // ln(x) = ln(m * 2^e) = ln(m) + e*ln(2): halve or double until the
        // argument is in [0.5, 2), where the series converges quickly.
        let two = Decimal::from_i64(2);
        let mut val = x.clone();
        let mut exp_count: i64 = 0;

        while val > two {
            val = val.div(&two, scale)?;
            exp_count = exp_count.saturating_add(1);
        }
        let half = one.div(&two, scale)?;
        while val < half {
            val = val.mul(&two, scale);
            exp_count = exp_count.saturating_sub(1);
        }

        // Now compute ln(val) using the series.
        let num = val.sub(&one);
        // val is in [0.5, 2] and positive, so val+1 is at least 1.5.
        let den = val.add(&one);
        let ratio = num.div(&den, scale)?;
        let ratio_sq = ratio.mul(&ratio, scale);

        let mut result = Decimal::zero();
        let mut term = ratio.clone();

        for i in 0..100i64 {
            // 2i+1 is odd, so never zero.
            let denom = Decimal::from_i64(i.saturating_mul(2).saturating_add(1));
            let contrib = term.div(&denom, scale)?;
            result = result.add(&contrib);
            term = term.mul(&ratio_sq, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        result = result.mul(&two, scale);

        // Add back the exp_count * ln(2).
        if exp_count != 0 {
            let ln2 = self.compute_ln2(scale)?;
            result = result.add(&ln2.mul(&Decimal::from_i64(exp_count), scale));
        }
        Ok(result.rescale(self.scale))
    }

    /// e^x using Taylor series.
    fn builtin_exp(&self, x: &Decimal) -> Eval {
        let scale = self.working_scale();
        let mut result = Decimal::one();
        let mut term = Decimal::one();

        for n in 1..100 {
            term = term.mul(x, scale);
            // n starts at 1, so never zero.
            term = term.div(&Decimal::from_i64(n), scale)?;
            result = result.add(&term);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result.rescale(self.scale))
    }

    /// Bessel function J(n, x) using series expansion.
    fn builtin_bessel(&self, n: &Decimal, x: &Decimal) -> Eval {
        let scale = self.working_scale();
        let n_int = {
            let s = n.rescale(0).format(10);
            s.parse::<i64>().unwrap_or(0).unsigned_abs()
        };

        let x_half = x.div(&Decimal::from_i64(2), scale)?;
        let neg_x_sq_4 = x.mul(x, scale).negate().div(&Decimal::from_i64(4), scale)?;

        // (x/2)^n / n!
        let mut pow = Decimal::one();
        for _ in 0..n_int {
            pow = pow.mul(&x_half, scale);
        }
        // A factorial of non-negative integers, so at least 1 -- never zero.
        let mut factorial = Decimal::one();
        for i in 1..=n_int {
            factorial = factorial.mul(&Decimal::from_i64(i as i64), scale);
        }
        let mut term = pow.div(&factorial, scale)?;
        let mut result = term.clone();

        for k in 1i64..100 {
            // term *= -x^2/4 / (k * (n + k)); k >= 1 and n >= 0, so never zero.
            let denom = Decimal::from_i64(
                k.saturating_mul(i64::try_from(n_int).unwrap_or(i64::MAX).saturating_add(k)),
            );
            term = term.mul(&neg_x_sq_4, scale).div(&denom, scale)?;
            result = result.add(&term);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result.rescale(self.scale))
    }

    /// Compute pi to the given scale using Machin's formula:
    /// pi/4 = 4*atan(1/5) - atan(1/239).
    fn compute_pi(&self, scale: usize) -> Eval {
        let one = Decimal::from_i64(1);
        let four = Decimal::from_i64(4);
        let a1 = one.div(&Decimal::from_i64(5), scale)?;
        let a2 = one.div(&Decimal::from_i64(239), scale)?;
        let t1 = self.atan_series(&a1, scale)?;
        let t2 = self.atan_series(&a2, scale)?;
        Ok(four.mul(&t1, scale).sub(&t2).mul(&four, scale))
    }

    /// Compute ln(2) to the given scale.
    fn compute_ln2(&self, scale: usize) -> Eval {
        let one = Decimal::from_i64(1);
        let two = Decimal::from_i64(2);
        // ln(2) via the series for ln((1+y)/(1-y)) where y = 1/3.
        let num = two.sub(&one); // 1
        let den = two.add(&one); // 3
        let ratio = num.div(&den, scale)?;
        let ratio_sq = ratio.mul(&ratio, scale);
        let mut result = Decimal::zero();
        let mut term = ratio.clone();
        for i in 0..100i64 {
            // 2i+1 is odd, so never zero.
            let denom = Decimal::from_i64(i.saturating_mul(2).saturating_add(1));
            let contrib = term.div(&denom, scale)?;
            result = result.add(&contrib);
            term = term.mul(&ratio_sq, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result.mul(&two, scale))
    }

    /// Reduce angle modulo 2*pi for trig functions.
    fn reduce_angle(&self, x: &Decimal, scale: usize) -> Eval {
        let two_pi = self.compute_pi(scale)?.mul(&Decimal::from_i64(2), scale);
        // pi is a computed value rather than a constant, so at scale 0 it can
        // legitimately truncate to zero. Reducing by nothing is the right
        // answer there, and it is also what keeps the division below safe.
        if two_pi.is_zero() || x.abs() <= two_pi {
            return Ok(x.clone());
        }
        let q = x.div(&two_pi, 0)?.rescale(0);
        Ok(x.sub(&q.mul(&two_pi, scale)))
    }
}

/// Whether a bare expression statement prints nothing of its own.
///
/// bc echoes the value of any expression written as a statement, *except* an
/// assignment (which is silent, so that `x = 1` does not print) and a string
/// literal (which writes its own text and has no value to echo).
fn suppresses_auto_print(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Assign(_, _)
            | Expr::OpAssign(_, _, _)
            | Expr::PreInc(_)
            | Expr::PreDec(_)
            | Expr::PostInc(_)
            | Expr::PostDec(_)
            // A bare string statement -- `"hello"` -- writes the string and
            // nothing else. Evaluating it returns zero for want of anything
            // better, and printing that zero as well made every string
            // statement emit a stray `0` after its text.
            | Expr::StringLit(_)
    )
}

/// How many `{` in `text` are still unclosed.
///
/// Interactive bc reads a `define` or a multi-line `while` across several
/// lines, so it has to know when the construct is finished before it can parse
/// anything. Counting the brace *characters* is the obvious way and is wrong:
/// `print "{"` would leave the count at one and swallow every following line
/// until the user typed a `}` that was never part of any block. Running the
/// lexer instead costs a re-scan of a buffer that is at most a few lines long,
/// and gets strings, `#` comments and `/* */` comments right by construction,
/// because that is the one piece of code in this program that already knows
/// what a brace inside a string is.
fn open_brace_depth(text: &str) -> i32 {
    let mut lexer = Lexer::new(text);
    let mut depth: i32 = 0;
    loop {
        match lexer.next_token() {
            Token::LBrace => depth = depth.saturating_add(1),
            Token::RBrace => depth = depth.saturating_sub(1),
            Token::Eof => return depth,
            _ => {}
        }
    }
}

// -------------------------------------------------------------------------
// Main entry point
// -------------------------------------------------------------------------

fn main() {
    let mut math_lib = false;
    let mut quiet = false;
    let mut expr_to_eval: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    // Walked with an iterator rather than an index, because `-e` consumes the
    // argument after it: an index-and-counter version has to bump the counter
    // in two places and bound-check between them, and getting either wrong is
    // a panic on a command line the user typed.
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-l" => math_lib = true,
            "-q" => quiet = true,
            "-e" => expr_to_eval = args.next(),
            // Combined short flags: -lq and so on.
            flag if flag.starts_with('-') && flag.len() > 1 => {
                for ch in flag.chars().skip(1) {
                    match ch {
                        'l' => math_lib = true,
                        'q' => quiet = true,
                        _ => eprintln!("bc: unknown option: -{ch}"),
                    }
                }
            }
            // A bare "-" is the conventional name for standard input, and
            // falls through to the file list where the reader handles it.
            _ => files.push(arg),
        }
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

    for line_result in stdin.lock().lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        buffer.push_str(&line);
        buffer.push('\n');

        // Once the braces balance, the construct is complete: parse and run it.
        if open_brace_depth(&buffer) <= 0 {
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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // Helper: evaluate an expression string and return the formatted result.
    fn eval_expr(input: &str) -> String {
        let mut interp = Interpreter::new(false);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        // For tests: the last value is stored in `last`. A failing statement
        // is unwrapped rather than ignored -- a test that expects a value and
        // silently gets the previous one is worse than a test that fails.
        for stmt in &stmts {
            interp.exec_stmt(stmt).expect("statement failed");
        }
        interp.last.format(interp.obase)
    }

    #[allow(dead_code)]
    fn eval_expr_ml(input: &str) -> String {
        let mut interp = Interpreter::new(true);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        for stmt in &stmts {
            interp.exec_stmt(stmt).expect("statement failed");
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

    // The number type's own tests live with the type, in `bignum::decimal` --
    // parsing, truncation, the error cases and exactness past 2^53 are
    // properties of `Decimal`, not of `bc`, and duplicating them here would
    // mean two suites to update and the chance of them disagreeing. What
    // follows is `bc`: the lexer, the parser, and the interpreter's use of the
    // number type.

    // --- Reading input: where one line ends and the next construct begins ---

    #[test]
    fn a_brace_inside_a_string_does_not_open_a_block() {
        // Interactive bc decides a construct is complete when its braces
        // balance. Counting brace *characters* meant `print "{"` opened a
        // block that nothing would ever close, and every line the user typed
        // afterwards was swallowed into a buffer that never ran.
        assert_eq!(open_brace_depth("print \"{\"\n"), 0);
        assert_eq!(open_brace_depth("print \"}\"\n"), 0);
        assert_eq!(open_brace_depth("s = \"{{{\"\n"), 0);
    }

    #[test]
    fn a_brace_inside_a_comment_does_not_open_a_block() {
        assert_eq!(open_brace_depth("1 + 1 # }\n"), 0);
        assert_eq!(open_brace_depth("1 + 1 /* { */\n"), 0);
    }

    #[test]
    fn an_unfinished_block_reports_its_open_braces() {
        assert_eq!(open_brace_depth("define f(x) {\n"), 1);
        assert_eq!(open_brace_depth("define f(x) {\n  if (x) {\n"), 2);
        assert_eq!(open_brace_depth("define f(x) {\n  return(x)\n}\n"), 0);
    }

    #[test]
    fn an_unterminated_comment_or_string_still_terminates_the_scan() {
        // Both of these run to end of input. The lexer must reach `Eof`
        // rather than spin, or the interactive loop hangs on a typo.
        assert_eq!(open_brace_depth("1 /* never closed"), 0);
        assert_eq!(open_brace_depth("\"never closed"), 0);
        assert_eq!(open_brace_depth("{ /* never closed"), 1);
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
    fn a_division_by_zero_prints_nothing_and_abandons_the_line() {
        // This test previously asserted `["0"]` -- that `10/0` printed zero --
        // which is what the arithmetic used to return after complaining to
        // stderr. It is the one answer a calculator must not give: `x = 1/0`
        // assigned 0 and every later line computed with it.
        assert!(capture_output("10/0").is_empty());
        assert!(capture_output("10%0").is_empty());
        // The statement is abandoned whole -- `1/0 + 5` does not print 5 --
        // but the next statement still runs.
        assert!(capture_output("1/0 + 5").is_empty());
        assert_eq!(capture_output("1/0\n7"), vec!["7"]);
    }

    #[test]
    fn a_failure_inside_a_loop_abandons_the_whole_loop() {
        // Not just the iteration: resuming the loop would run every remaining
        // iteration through the same failing division, printing the diagnostic
        // once per pass.
        let output = capture_output("for (i = 0; i < 3; i++) { i / 0 }\n\"done\"");
        assert_eq!(output, vec!["done"]);
    }

    #[test]
    fn a_failed_line_leaves_the_session_usable() {
        // A runtime error abandons its line, not the interpreter: the variables
        // set before it keep their values and the next line still runs.
        let output = capture_output("x = 5\nx / 0\nx + 1");
        assert_eq!(output, vec!["6"]);
    }

    #[test]
    fn an_error_inside_a_function_does_not_leave_its_frame_behind() {
        // The callee's parameter shadows the caller's `x`. If the failing path
        // skipped the frame teardown, `x` would still read 99 afterwards.
        let output = capture_output("define f(x) { return (x / 0) }\nx = 5\nf(99)\nx");
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn a_guard_short_circuits_before_the_division_it_guards() {
        // `x != 0 && 1/x` must not evaluate the division when x is zero, or the
        // guard the user wrote would report the error it exists to prevent.
        let output = capture_output("x = 0\nif (x != 0 && 1/x > 2) { print \"big\\n\" }\n42");
        assert_eq!(output, vec!["42"]);
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
        let input = r"
define sum_to(n) {
    auto s, i
    s = 0
    for (i = 1; i <= n; i = i + 1) {
        s = s + i
    }
    return s
}
sum_to(100)
";
        let output = capture_output(input);
        assert_eq!(output, vec!["5050"]);
    }

    #[test]
    fn test_nested_functions() {
        let input = r"
define square(x) { return x*x }
define sum_of_squares(a, b) { return square(a) + square(b) }
sum_of_squares(3, 4)
";
        let output = capture_output(input);
        assert_eq!(output, vec!["25"]);
    }

    #[test]
    fn test_break_in_loop() {
        let input = r"
x = 0
while (1) {
    x = x + 1
    if (x == 5) break
}
x
";
        let output = capture_output(input);
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn test_continue_in_loop() {
        let input = r"
s = 0
for (i = 1; i <= 10; i = i + 1) {
    if (i % 2 == 0) continue
    s = s + i
}
s
";
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
