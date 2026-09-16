//! Slate OS `bc` -- arbitrary-precision calculator
//!
//! A POSIX-compatible `bc` implementation with extensions.  Supports
//! arbitrary-precision integers and fixed-point decimals, variables,
//! user-defined functions, control flow, and the `-l` math library.
//!
//! Architecture: hand-written lexer -> recursive-descent parser -> AST ->
//! tree-walk interpreter.  The numbers are `bignum::Decimal`, shared with `dc`.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::quotef_os;
use coreutils::stdfd;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
#[cfg(not(test))]
use std::io::Write;
use std::io::{self, BufRead};
use std::process::ExitCode;

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
    /// `quit`, which ends the run when it is *read* — see [`Parser::saw_quit`].
    /// It is deliberately not a statement: by the time there is a statement
    /// list to execute, a chunk holding this token has already been discarded.
    Quit,
    /// `halt`, which ends the run when it is *executed*.
    Halt,
    Print,
    /// A byte that starts no token at all — `$`, a backtick, an opening quote
    /// with no closing one.
    ///
    /// It is a *token* rather than something the scanner swallows, because the
    /// scanner and the parser report in reading order and only the parser knows
    /// where the reading got to. GNU emits both diagnostics for `1 $ 2` —
    /// `illegal character: $` and then `syntax error`, in that order, on the
    /// same line — which a scanner that dropped the byte could not reproduce:
    /// the parser would see `1 2` and have nothing to complain about.
    Illegal(u8),
    // End of input
    Eof,
}

struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    /// Whether the scan ended in the middle of something that spans lines: an
    /// unclosed `/* */` comment, or a `\` line continuation whose next line has
    /// not arrived.
    ///
    /// These are the two constructs the *parser* cannot notice, because by the
    /// time it sees tokens they have already been swallowed. `/* one` produces
    /// no tokens at all and looks exactly like a blank line;
    /// `1 + \` produces `1` and `+` and looks exactly like the missing-operand
    /// syntax error the parser is right to refuse to wait for. Only the scanner
    /// knows the difference, so only the scanner can say so — and [`Chunker`]
    /// needs to be told, or `2 + /* one` and `two */ 2` are run as two separate
    /// programs and answer `2` where GNU answers `4`.
    unfinished: bool,
    /// The line the scanner is currently on, counting from 1.
    ///
    /// Incremented wherever a newline byte is *consumed*, which is three
    /// places, not one: the `Newline` token, the blanks skipped between
    /// tokens, and the interior of a `/* */` comment. Missing any of them
    /// makes every diagnostic after the first multi-line comment point at the
    /// wrong line, which is worse than no line number at all — a reader sent
    /// to a line that looks fine concludes the report is noise.
    line: u32,
    /// The line the token just returned by [`Self::next_token`] *started* on.
    ///
    /// Distinct from `line`, which by then has already moved past a token that
    /// contained newlines. A diagnostic names where the offending thing begins.
    token_line: u32,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            unfinished: false,
            line: 1,
            token_line: 1,
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
        let end = self.pos.saturating_add(n).min(self.input.len());
        // Counted HERE, in the one place the cursor ever moves, rather than at
        // the newline token. The scanner consumes newlines in three unrelated
        // branches -- the `Newline` token, the blanks between tokens, and the
        // interior of a `/* */` comment -- and a counter maintained at only
        // some of them sends every later diagnostic to the wrong line. A reader
        // pointed at a line that looks fine concludes the report is noise,
        // which is worse than reporting no line at all.
        // `naive_bytecount` wants the `bytecount` crate here. Declined: it is a
        // SIMD dependency earning its keep on megabytes, and this counts the
        // newlines in one `bc` statement — a few dozen bytes, once per token.
        // A new dependency on the userland's critical path is a real cost; the
        // loop is not.
        #[allow(clippy::naive_bytecount)]
        let crossed = self
            .input
            .get(self.pos..end)
            .map_or(0, |seg| seg.iter().filter(|&&b| b == b'\n').count());
        self.line = self
            .line
            .saturating_add(u32::try_from(crossed).unwrap_or(u32::MAX));
        self.pos = end;
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
                        // Landing exactly at the end means the line being
                        // continued *onto* has not been read yet. Every line the
                        // chunker holds arrives with its newline attached, so a
                        // continuation that is not at the end is one whose
                        // successor is already in the buffer.
                        if self.pos >= self.input.len() {
                            self.unfinished = true;
                        }
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
                        // It also means the `*/` is on a line not yet read.
                        (None, _) | (_, None) => {
                            self.bump(1);
                            self.unfinished = true;
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
        // After the skip, not before: a diagnostic should name the line the
        // token is on, not the line the previous one ended on.
        self.token_line = self.line;

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
                // Consume the byte and hand it on as a token rather than
                // recursing past it.
                //
                // Skipping silently is one of the four sites that made a typo
                // produce a wrong *number* with no message: `1 $ 2` scanned as
                // `1 2` and printed both, where GNU prints neither and says
                // `illegal character: $` and then `syntax error`. The byte has
                // to survive scanning for the parser to be able to refuse it.
                self.bump(1);
                Token::Illegal(b)
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

    /// Read a string literal.
    ///
    /// A `bc` string runs to the very next `"` and holds exactly the bytes
    /// between the quotes — there is no escape here, not even for the quote
    /// itself, so `"a\"b"` is the string `a\` followed by the syntax error
    /// GNU `bc` reports for the stray `b"`. Escapes are a property of
    /// `print`, not of the literal (see [`print_escaped`]), which is
    /// why `"a\nb"` on its own line writes four characters while
    /// `print "a\nb"` writes three.
    fn read_string(&mut self) -> Token {
        // A string with no closing quote is NOT a string. Measured: GNU bc
        // 1.07.1 answers `printf 'print "abc\n' | bc -q` with
        // `(standard_in) 1: illegal character: "` — its scanner's rule needs
        // the closing quote, so the opening one falls through to the
        // illegal-character rule rather than matching a string that runs to
        // end of input. Ours used to consume the rest of the file and print
        // it, which is the shape of this whole entry: recover silently, emit
        // a wrong answer, exit 0.
        //
        // But `unfinished` as well as the token, because bc strings genuinely
        // DO span lines — measured, `print "ab\ncd"\n` prints `ab\ncd`. So an
        // unterminated string mid-session means *the rest has not arrived*,
        // and only at end of input is it an error. Setting both lets
        // [`Chunker`] wait for the next line, and leaves the `Illegal` token
        // in place to be reported if the input stops first. One flag, both
        // behaviours, no second code path to keep in step.
        if !self.rest_has_closing_quote() {
            self.bump(1); // the quote itself, and nothing after it
            self.unfinished = true;
            return Token::Illegal(b'"');
        }
        self.advance(); // skip opening "
        let mut s = String::new();
        while let Some(b) = self.peek_byte() {
            self.advance();
            if b == b'"' {
                break;
            }
            s.push(b as char);
        }
        Token::StringLit(s)
    }

    /// Whether a closing `"` exists anywhere after the cursor.
    ///
    /// bc has no escape for a quote inside a string — the scanner's rule is
    /// `"[^"]*"` — so the first `"` found is the closing one, and a plain
    /// search is exactly right rather than an approximation of one.
    fn rest_has_closing_quote(&self) -> bool {
        self.input
            .get(self.pos.saturating_add(1)..)
            .is_some_and(|rest| rest.contains(&b'"'))
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
            "halt" => Token::Halt,
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
    /// `halt`: stop the session, from wherever this is reached.
    ///
    /// There is no `Stmt::Quit` beside it, and that absence *is* the difference
    /// between the two keywords. `quit` never becomes a statement, because it
    /// never gets as far as execution — see [`Parser::saw_quit`].
    Halt,
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

/// One diagnostic, in GNU's two flavours, carrying the line it belongs to.
///
/// The line is relative to the chunk the parser was handed; [`Chunker`] adds
/// the number of lines already consumed before anything is printed, because a
/// parser that only ever sees one construct at a time cannot know it is on
/// line 40 of the file.
#[derive(Clone, Debug, PartialEq)]
struct SyntaxError {
    /// Index of the offending token in the *unfiltered* token stream.
    ///
    /// Carried only so the diagnostics can be put back into reading order
    /// before printing. Measured, GNU orders them by position and not by which
    /// stage produced them: `) $` prints `syntax error` then
    /// `illegal character: $`, while `$ )` prints them the other way round. A
    /// scanner-first rule gets the first of those backwards.
    at: usize,
    line: u32,
    kind: ErrorKind,
}

#[derive(Clone, Debug, PartialEq)]
enum ErrorKind {
    /// A byte that starts no token: `illegal character: $`.
    Illegal(u8),
    /// Anything the grammar refuses: `syntax error`.
    Syntax,
}

impl SyntaxError {
    /// The message without the `NAME LINE: ` prefix, which only the caller
    /// knows (it differs between a file operand and the stdin session).
    fn message(&self) -> String {
        match self.kind {
            // `as char` is safe for the printable ASCII this fires on, and for
            // a high byte it prints the Latin-1 character rather than refusing
            // to report at all — GNU prints the raw byte, so neither of us is
            // doing anything principled with a non-ASCII one.
            ErrorKind::Illegal(b) => format!("illegal character: {}", b as char),
            ErrorKind::Syntax => "syntax error".to_string(),
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    /// The line each token in `tokens` started on, same length and same order.
    ///
    /// Parallel to `tokens` rather than a field inside `Token` because `Token`
    /// is compared with `==` throughout the parser — `self.peek() == expected`
    /// — and a line number inside it would make two otherwise identical tokens
    /// from different lines compare unequal, silently breaking every one of
    /// those comparisons.
    lines: Vec<u32>,
    /// Each retained token's index in the *unfiltered* stream, so a parser
    /// diagnostic can be ordered against a scanner one that was filtered out.
    order: Vec<usize>,
    /// Diagnostics, sorted into reading order by [`Self::take_errors`].
    errors: Vec<SyntaxError>,
    pos: usize,
    /// Whether the parse ran off the end of the token stream while a construct
    /// was still open — `if (x)` with no body yet, a `define` whose `}` has not
    /// arrived.
    ///
    /// This is what makes a *chunk* boundary something other than a line
    /// boundary. GNU's parser pulls tokens from the scanner and the scanner
    /// reads another line whenever it needs one, so `if (x)` and its body may
    /// sit on separate lines and still be one statement. Ours parses a finished
    /// string, so the equivalent is to notice that the string ended too early
    /// and ask [`Chunker`] for the next line before running anything.
    ///
    /// The parser sets it at exactly two places, both positions where a token
    /// was *required* and `Eof` arrived instead: [`Self::expect`] and
    /// [`Self::parse_block_or_stmt`]'s empty body. Every other `Eof` is a legal
    /// place to stop — including a missing operand, which GNU treats as an
    /// error on the spot rather than as an invitation to read on. See the
    /// measurement in [`Self::parse_primary`].
    ///
    /// It also starts out true when [`Lexer::unfinished`] says the scan ended
    /// inside a construct the parser never gets to see. The two answers are
    /// combined here rather than kept apart because [`Chunker`] asks a single
    /// question — is there more of this to come? — and does not care which
    /// layer noticed.
    truncated: bool,
}

impl Parser {
    fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        let mut lines = Vec::new();
        let mut order = Vec::new();
        let mut errors = Vec::new();
        let mut index = 0usize;
        loop {
            let tok = lexer.next_token();
            let is_eof = tok == Token::Eof;
            // An illegal byte is reported and then DROPPED, which is GNU's own
            // arrangement and is load-bearing rather than incidental. Measured:
            // `1 $ 2` gives `illegal character: $` *and* `syntax error`,
            // because with the `$` gone the parser sees `1 2` — two
            // expressions with nothing between them — and refuses it. But
            // `1; $ 2` gives only `illegal character: $`, because there the
            // cleaned stream is `1 ; 2`, which is perfectly good bc. A parser
            // that saw the illegal token itself would report a syntax error in
            // both, and be wrong in the second.
            if let Token::Illegal(b) = tok {
                // Only the first is reported, and the rest are dropped in
                // silence. Measured: `$ % &` answers `illegal character: $`
                // and then `syntax error` — the `&`, equally illegal, is never
                // mentioned. The byte is still removed from the stream either
                // way, so what the parser goes on to see is unaffected.
                if !errors
                    .iter()
                    .any(|e: &SyntaxError| matches!(e.kind, ErrorKind::Illegal(_)))
                {
                    errors.push(SyntaxError {
                        at: index,
                        line: lexer.token_line,
                        kind: ErrorKind::Illegal(b),
                    });
                }
            } else {
                // A newline is blamed on the line it ENDS, not the one it
                // begins — `lexer.line` has already moved past it, which is
                // exactly the number GNU would report.
                //
                // Measured: `x = 1 +\n2\nx\n` is `h2.bc 2: syntax error` on
                // GNU, not line 1, even though the incomplete expression is on
                // line 1. Its `line_no` is incremented as the newline is
                // scanned and the missing operand is only noticed afterwards,
                // so the count has moved on by the time it reports. Ours said
                // 1 until this line existed. Copied deliberately: the test
                // `a_half_finished_expression_is_an_error_not_a_continuation`
                // already carried GNU's `2:` as the measured truth, and a bc
                // that numbers its errors differently from the bc every script
                // was written against is a worse tool for being more logical.
                let blamed = if tok == Token::Newline {
                    lexer.line
                } else {
                    lexer.token_line
                };
                tokens.push(tok);
                lines.push(blamed);
                order.push(index);
            }
            index = index.saturating_add(1);
            if is_eof {
                break;
            }
        }
        Self {
            tokens,
            lines,
            order,
            errors,
            pos: 0,
            truncated: lexer.unfinished,
        }
    }

    /// The diagnostics, in reading order, leaving the parser empty.
    ///
    /// Sorted by token position: scanner and parser findings interleave by
    /// where they are in the text, not by which stage found them. The sort is
    /// stable, so two diagnostics blamed on the same token keep the order they
    /// were recorded in.
    fn take_errors(&mut self) -> Vec<SyntaxError> {
        let mut out = std::mem::take(&mut self.errors);
        out.sort_by_key(|e| e.at);
        out
    }

    /// The line the token under the cursor starts on.
    fn line_here(&self) -> u32 {
        // The last entry is `Eof`'s line, which is the right answer for a
        // cursor that has run off the end — that is where the input stopped.
        self.lines
            .get(self.pos)
            .or_else(|| self.lines.last())
            .copied()
            .unwrap_or(1)
    }

    /// Record that the grammar refused the token under the cursor.
    ///
    /// At most one per unit, which is measured rather than assumed: GNU
    /// answers `print ) ) )` and `) ) )` and `1 +++ 2` with a single
    /// `syntax error` each. Its parser enters yacc's error-recovery state and
    /// stays there until it has resynchronised, so one mistake yields one
    /// message however much wreckage follows it.
    ///
    /// Worth keeping even aside from matching GNU: the alternative buries the
    /// real mistake under a screenful of consequences of it, which is the
    /// failure mode every compiler eventually grows a suppression rule for.
    fn record_error(&mut self) {
        if self.errors.iter().any(|e| e.kind == ErrorKind::Syntax) {
            return;
        }
        let at = self.order.get(self.pos).copied().unwrap_or(usize::MAX);
        self.errors.push(SyntaxError {
            at,
            line: self.line_here(),
            kind: ErrorKind::Syntax,
        });
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
            // A required token that is missing *because the input stopped* is a
            // different thing from one that is missing because something else
            // was written instead. The first wants another line; the second is
            // an error. Only the first is recorded.
            if *self.peek() == Token::Eof {
                self.truncated = true;
            } else {
                // Something else was written where this token was required —
                // an error now, not a request for more input. `Chunker` turns
                // a `truncated` that survives to end of input into one of
                // these, so the Eof case is not being let off, only deferred
                // until it is known that no further line is coming.
                self.record_error();
            }
            false
        }
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Token::Newline || *self.peek() == Token::Semicolon {
            self.advance();
        }
    }

    /// Whether this chunk of input contains a `quit` anywhere at all.
    ///
    /// `quit` is defined by *when it is read*, not by where it sits: the GNU
    /// manual says "when this statement is read, the bc processor is
    /// terminated, **regardless of where the quit statement is found**". So it
    /// is answered here, from the token stream, before a single statement has
    /// been built — and a chunk that contains one is discarded whole, including
    /// whatever was written before it on the same line.
    ///
    /// That is why this asks about the *tokens* and not about the text. `print
    /// "quit"` holds the four letters and is not a `quit`; the lexer has
    /// already made that distinction, and re-deriving it here from the source
    /// would be re-deriving it wrongly.
    fn saw_quit(&self) -> bool {
        self.tokens.contains(&Token::Quit)
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
            // `quit` should never reach the parser: [`run_text`] asks
            // [`Self::saw_quit`] first and throws the chunk away. Treating it as
            // `halt` is the backstop for a caller that forgets — it stops the
            // session, which is at least the same direction, where a syntax
            // error would be a wrong answer with a diagnostic attached.
            Token::Quit | Token::Halt => {
                self.advance();
                self.require_terminator();
                Some(Stmt::Halt)
            }
            Token::Print => {
                self.advance();
                let items = self.parse_print_list();
                self.require_terminator();
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
                self.require_terminator();
                Some(Stmt::Return(expr))
            }
            Token::Break => {
                self.advance();
                self.require_terminator();
                Some(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.require_terminator();
                Some(Stmt::Continue)
            }
            Token::LBrace => {
                self.advance();
                let body = self.parse_stmt_list();
                self.expect(&Token::RBrace);
                self.require_terminator();
                Some(Stmt::Block(body))
            }
            _ => {
                if self.is_expr_start() {
                    let expr = self.parse_expr();
                    self.require_terminator();
                    Some(Stmt::Expr(expr))
                } else {
                    // Skip unexpected token — but say so first. Skipping in
                    // silence is what let `print )` run to completion and
                    // report nothing.
                    self.record_error();
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

    /// End a statement, refusing whatever cannot legally come next.
    ///
    /// bc separates statements with `;` or a newline, and until this existed
    /// ours did not insist: `1 2` printed `1` and `2` where GNU calls it a
    /// syntax error. That is mostly harmless on its own — nobody writes
    /// `1 2` — but it is what stopped a *typo* from being reported, because a
    /// stray character removed by the scanner turns `1 $ 2` into exactly that.
    ///
    /// # What may follow a statement, measured rather than reasoned
    ///
    /// The failure mode of getting this wrong is rejecting valid programs,
    /// which is worse than the over-acceptance being fixed, so every row below
    /// was run against GNU bc 1.07.1 before a line of this was written:
    ///
    /// | after a statement | GNU |
    /// |---|---|
    /// | `;` or newline | the separators themselves |
    /// | `}` | accepted — `{ print "a" }` |
    /// | `else` | accepted — `if (1) print "a" else print "b"` |
    /// | end of input | accepted |
    /// | anything else | `syntax error` |
    ///
    /// Two results are worth keeping because they are not what one would
    /// guess. **A closing brace does not license a following statement**:
    /// `{ 1 } 2` is refused, as are `if (1) { … } 2` and `while (0) { } 2`, so
    /// a block ends a statement and still needs a separator after it. And a
    /// **function definition is not a statement** in this sense — `define f()
    /// { return (1) } f()` is accepted — because GNU's grammar makes a
    /// definition its own input item. That is why [`Self::parse_define`] does
    /// not call this.
    fn require_terminator(&mut self) {
        match *self.peek() {
            Token::Newline | Token::Semicolon => {
                self.advance();
            }
            // Legal followers that are NOT terminators, so they stay put for
            // whoever is parsing the construct around this one.
            Token::RBrace | Token::Else | Token::Eof => {}
            _ => self.record_error(),
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
            // The braced body ends the enclosing `if`/`while`/`for`, so the
            // statement-separator rule applies here as much as after a bare
            // one: measured, `if (1) { print "a" } 2` is a syntax error on
            // GNU. `else` is among the legal followers, so the `else` half of
            // an `if` still parses.
            //
            // The braceless branch below needs nothing: `parse_stmt` has
            // already required a terminator for whatever statement it read,
            // and that terminator is the enclosing construct's too. Requiring
            // a second one there would reject `if (1) print "a"` followed by
            // any next line at all.
            self.require_terminator();
            stmts
        } else if let Some(stmt) = self.parse_stmt() {
            vec![stmt]
        } else {
            // `parse_stmt` answers `None` only at `Eof`, and a body position is
            // the one place `Eof` is not a legal stopping point: `while (i < 3)`
            // alone is half a loop, not a loop with an empty body.
            self.truncated = true;
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
                //
                // Deliberately *not* a [`Self::truncated`] site, though it looks
                // like one. A missing operand is a missing operand, not a line
                // that has yet to arrive: measured against GNU bc 1.07.1,
                // `printf 'x = 1 +\n2\nx\n' | bc -q` answers
                // `(standard_in) 2: syntax error`, then `2`, then `0` -- the
                // newline ended the statement, `2` was a fresh one, and `x` was
                // never assigned. Waiting for more input here would have joined
                // those two lines into `1 + 2` and printed `3`.
                //
                // The zero stays — the parser still has to return *an*
                // expression — but it is no longer the whole of the response.
                // The recorded error means the statement holding this zero is
                // never executed, so the placeholder cannot reach a result.
                self.record_error();
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
    /// `halt` was executed: the session is over, at whatever depth this is.
    Halt,
}

/// What a loop should do after running its body once.
enum LoopFlow {
    /// Go round again — the body ended normally or hit `continue`.
    Continue,
    Break,
    Return(Decimal),
    /// `halt` in the body: unwind out of the loop and out of everything.
    Halt,
}

/// Whether the caller of [`run_text`] has any more work to do.
///
/// The stop is carried back as a value rather than taken with `process::exit`,
/// because the exit status is decided in `main`: exiting from inside the
/// interpreter skips [`stdfd::close_stderr`], so a stop that followed output
/// which could not be written would report success. Measured — GNU `bc` gives
/// status 1 for `printf 'print "hi"\nquit\n' | bc > /dev/full`, reaching the
/// same place by having `exit(3)` run the `atexit` handler that we do not have.
///
/// One variant covers both keywords, because from the caller's side they ask
/// for the same thing — read no more input. What differs is *when* each is
/// decided, and that is settled before this type is produced: `quit` in
/// [`run_text`], from the tokens; `halt` in [`Interpreter::run`], from
/// execution.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Session {
    /// Keep reading.
    Continue,
    /// `quit` was read or `halt` was executed: stop, evaluating nothing more.
    Stop,
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
    /// Not an error: `halt` reached inside a called function.
    ///
    /// A `halt` in a statement position comes back as [`StmtResult::Halt`], but
    /// `define f() { halt }` puts one under an *expression* — `f() + 1` — and
    /// the only channel that unwinds an expression is this one. It is never
    /// printed: [`Interpreter::run`] stops on it instead, exactly as it stops
    /// on `StmtResult::Halt`, and the `Display` arm below exists only because
    /// the trait requires it to be total.
    ///
    /// `quit` has no counterpart here, and cannot acquire one: it is decided
    /// from the token stream before any function is defined, let alone called.
    Halt,
}

impl From<DecimalError> for RuntimeError {
    fn from(e: DecimalError) -> Self {
        Self::Math(e)
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Capitalised rather than reworded, and that is the point.
            // `DecimalError`'s text is shared with `dc` on purpose — its own
            // comment says so, "the wording is the calculators' own, so a
            // caller can print this straight through without restating it and
            // drifting from the other". GNU words them differently in the two
            // programs: `bc` says `Divide by zero` and `Square root of a
            // negative number`, `dc` says `divide by zero` and `square root of
            // negative number` (no `a`). Our shared strings already match
            // *bc*'s wording exactly apart from the leading capital, so
            // capitalising here reaches GNU's bc text without a second copy of
            // the words existing to fall out of step with the first.
            Self::Math(e) => {
                let text = e.to_string();
                let mut chars = text.chars();
                match chars.next() {
                    Some(first) => {
                        for c in first.to_uppercase() {
                            write!(f, "{c}")?;
                        }
                        f.write_str(chars.as_str())
                    }
                    None => Ok(()),
                }
            }
            // Restated, because this one is a different sentence and not a
            // different case: GNU's is `Function f not defined.`, ending in a
            // full stop, where ours was `undefined function f`.
            Self::UndefinedFunction(name) => write!(f, "Function {name} not defined."),
            Self::Halt => f.write_str("halt"),
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
    /// Digits per line before a printed number is continued with a `\`.
    ///
    /// Already converted from `BC_LINE_LENGTH` by [`wrap_chunk`], because the
    /// two are not the same number: `bc` keeps the backslash *inside* the
    /// stated width, so `BC_LINE_LENGTH=10` puts 8 digits on a line. Zero
    /// disables the break; see [`bignum::wrap_number`].
    wrap_chunk: usize,
    /// The innermost function a runtime error escaped from, for `func=` in the
    /// diagnostic. `None` means the fault was at the top level, which GNU
    /// spells `(main)`.
    ///
    /// Recorded when the error leaves a function *body* rather than when it is
    /// raised, because by the time [`Interpreter::run`] catches it the call
    /// stack has already unwound and there is nothing left to ask. The first
    /// body it escapes wins and later frames do not overwrite it — measured,
    /// an error inside `g` called from `f` is `func=g`, the innermost, not the
    /// outermost.
    ///
    /// Cleared before every statement. Without that a *second* error would
    /// inherit the first one's function: measured, `g(1)` then `1/0` is
    /// `func=g` and then `func=(main)`, so the reset is load-bearing rather
    /// than tidiness.
    fault_fn: Option<String>,
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
            wrap_chunk: line_length_from_env("BC_LINE_LENGTH"),
            fault_fn: None,
            #[cfg(test)]
            output_buf: Vec::new(),
        }
    }

    /// Render a value for output: in `obase`, then broken across lines.
    ///
    /// Every path that prints a number goes through here, which is what keeps
    /// `1/3` in a `print` statement and `1/3` on a line of its own from being
    /// written two different ways.
    fn render(&self, val: &Decimal) -> String {
        bignum::wrap_number(&val.format(self.obase), self.wrap_chunk)
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

    /// Report something the program can carry on past, on stderr.
    ///
    /// A warning is not a value, so it never goes through `output_str` and is
    /// not captured in tests: a caller redirecting stdout must not find
    /// diagnostics mixed into the numbers.
    fn warn(&self, message: &str) {
        diag!("Runtime warning (func=(main)): {message}");
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
                // There is no upper limit: past sixteen a digit is written as
                // a decimal group rather than a character (`obase=36; 1295`
                // is ` 35 35`), so every base has a notation. GNU accepts
                // 2^30 and clamps anything below two up to two with a warning
                // on stderr rather than refusing it.
                if v.is_negative() || b < 2 {
                    self.warn("obase too small, set to 2");
                    self.obase = 2;
                } else {
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
    ///
    /// The return value distinguishes the two ways this can stop early. A stray
    /// `break` ends the *program text* it was given but leaves the session
    /// alive; `quit` ends the session, and the caller must not read the next
    /// line, let alone evaluate it.
    /// What `func=` should say for the fault just caught: the innermost
    /// function the error escaped, or GNU's `(main)` for the top level.
    fn fault_label(&self) -> &str {
        self.fault_fn.as_deref().unwrap_or("(main)")
    }

    fn run(&mut self, stmts: &[Stmt]) -> Session {
        for stmt in stmts {
            // Cleared per statement, not per error: a statement that faults
            // inside `g` must not leave `g` behind for the next statement's
            // fault at the top level. Measured on GNU -- `g(1)` then `1/0`
            // reports `func=g` and then `func=(main)`.
            self.fault_fn = None;
            match self.exec_stmt(stmt) {
                Ok(StmtResult::Normal) => {}
                // Not a diagnostic, and not printed as one: `quit` under an
                // expression has no other way out of `eval`. See
                // [`RuntimeError::Halt`].
                Ok(StmtResult::Halt) | Err(RuntimeError::Halt) => return Session::Stop,
                // `break`, `continue` or `return` outside any enclosing
                // construct ends the program, as there is nothing to return to.
                Ok(_) => return Session::Continue,
                // Bound as `why`, not `e`: this is bc's own [`RuntimeError`]
                // and its own wording, not an `io::Error` whose text the host
                // chose. The name is what `scripts/host-errmsg.py` reads to
                // tell those apart, and the file stays under that gate — a
                // whole-file exemption would hide the next real site here.
                //
                // `(func=…)` but no `adr=`. GNU prints
                // `Runtime error (func=(main), adr=3): Divide by zero`, where
                // `adr` is the byte offset into the dc program it compiled the
                // statement to. We walk a tree and compile nothing, so there
                // is no honest value to put there — and it is not a line
                // number standing in disguise: measured, `1/0` is `adr=3`
                // whether it is the first line of the file or the fourth. A
                // number that looks meaningful and is not would be worse than
                // an absent field, because the absence is visible and the
                // fiction is not. See `design-decisions.md` §1025.
                Err(why) => diag!("Runtime error (func={}): {why}", self.fault_label()),
            }
        }
        Session::Continue
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<StmtResult, RuntimeError> {
        match stmt {
            Stmt::Expr(expr) => {
                let val = self.eval(expr)?;
                // In bc, a bare expression prints its value.
                // But assignments don't print (they are silent).
                if !suppresses_auto_print(expr) {
                    let formatted = self.render(&val);
                    self.output_line(&formatted);
                }
                self.last = val;
                Ok(StmtResult::Normal)
            }
            Stmt::Print(items) => {
                for item in items {
                    match item {
                        PrintItem::StringLit(s) => {
                            let text = print_escaped(s);
                            self.output_str(&text);
                        }
                        PrintItem::Expr(expr) => {
                            let val = self.eval(expr)?;
                            let formatted = self.render(&val);
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
                        LoopFlow::Halt => return Ok(StmtResult::Halt),
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
                        LoopFlow::Halt => return Ok(StmtResult::Halt),
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
            // Returned rather than taken with `process::exit`, so that the
            // status still passes through the `close_stderr` funnel in `main`.
            Stmt::Halt => Ok(StmtResult::Halt),
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
                StmtResult::Halt => return Ok(LoopFlow::Halt),
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
            // `multiply`, not `mul`: POSIX gives a product the scale
            // min(a + b, max(scale, a, b)), so `scale = 0; 1.5 * 1.5` is 2.2
            // rather than 2. `scale` governs division, where digits have to be
            // invented, not multiplication, where they are already there.
            BinOp::Mul => a.multiply(b, scale),
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
                // The frame below is still torn down first — a `quit` must not
                // leave the caller's variables shadowed by the callee's, since
                // the session may yet print something from them on the way out.
                Ok(StmtResult::Halt) => {
                    outcome = Err(RuntimeError::Halt);
                    break;
                }
                Err(e) => {
                    // The error is leaving THIS function's body, so this is
                    // the frame GNU names — but only if nothing inner claimed
                    // it first. `get_or_insert_with` is the whole of the
                    // innermost-wins rule: for `f` calling `g`, `g`'s body is
                    // unwound before `f`'s, so `g` writes here and `f` finds
                    // it already set. Measured: GNU says `func=g`.
                    //
                    // `Halt` is deliberately not recorded — it is not an
                    // error and is never printed, so naming a function for it
                    // would leave a stale value behind for the next real one.
                    if !matches!(e, RuntimeError::Halt) {
                        self.fault_fn.get_or_insert_with(|| name.to_string());
                    }
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

    /// atan(x), to the working scale.
    ///
    /// For |x| > 1, `atan(x) = ±pi/2 - atan(1/x)` brings the argument inside
    /// the unit interval; [`Self::atan_reduced`] then does the real work. That
    /// inversion is NOT enough on its own, which is the whole of
    /// `TD-B-BC-MATHLIB-ARCTANGENT-IS-INACCURATE`: it maps `x = 1.0001` to
    /// `0.9999`, which is just as slow to sum as the argument it came from.
    fn builtin_atan(&self, x: Decimal) -> Eval {
        let scale = self.working_scale();
        let one = Decimal::from_i64(1);

        if x.abs() > one {
            let pi_half = self.compute_pi(scale)?.div(&Decimal::from_i64(2), scale)?;
            // |x| > 1 is exactly the branch condition, so x is not zero.
            let inv = one.div(&x, scale)?;
            let atan_inv = self.atan_reduced(&inv, scale)?;
            let result = if x.is_negative() {
                pi_half.negate().sub(&atan_inv)
            } else {
                pi_half.sub(&atan_inv)
            };
            return Ok(result.rescale(self.scale));
        }
        Ok(self.atan_reduced(&x, scale)?.rescale(self.scale))
    }

    /// atan(x) for |x| <= 1, by halving the argument until the series is
    /// quick, then doubling the answer back.
    ///
    /// `atan(x) = 2 * atan( x / (1 + sqrt(1 + x^2)) )`, applied until |x| is
    /// under [`Self::ATAN_REDUCE_TO`].
    ///
    /// # Why the plain series was wrong, and wrong in the third digit
    ///
    /// The Maclaurin series for arctangent is `x - x^3/3 + x^5/5 - …`, whose
    /// terms fall off like `1/(2k+1)` when `x = 1` — it is the alternating
    /// harmonic series there, and it converges so slowly that no practical
    /// term count reaches even four correct digits. The old code summed a
    /// **fixed 100 terms** and stopped, and its `is_negligible` guard could
    /// never fire at `x = 1` because `x^2 = 1` leaves the numerator at ±1 for
    /// ever.
    ///
    /// The arithmetic is exact enough to be worth stating, because it is what
    /// identifies the cause rather than merely being consistent with it: an
    /// alternating series truncated after N terms sits within half the first
    /// omitted term, here `1/(2*100+1)/2 = 0.00248…`, and the measured error
    /// was `.7853981633 - .7828982258 = .0024999`. That is the truncation, not
    /// rounding drift and not a wrong formula.
    ///
    /// # Why reduction rather than more terms
    ///
    /// Raising the cap buys digits at a ruinous rate: the alternating harmonic
    /// series needs about `10^d` terms for `d` digits, so even ten correct
    /// digits is out of reach. Each halving instead costs one square root and
    /// roughly halves the argument, so five of them take `x = 1` to about
    /// `0.03`, where the series gains ~3 digits per term. Bounded work for
    /// unbounded precision, which a term cap can never be.
    fn atan_reduced(&self, x: &Decimal, outer_scale: usize) -> Eval {
        // Guard digits of our own, on top of the caller's. Each halving is
        // undone by a doubling at the end, so whatever error the series carries
        // is multiplied by `2^halvings` -- about 16 -- and every reduction step
        // truncates a division and a square root at the working scale. Ten
        // spare digits cover both with room over; without them `a(0.6)` at
        // `scale=30` agreed with GNU to only 17 places.
        let scale = outer_scale.saturating_add(10);
        let one = Decimal::from_i64(1);
        let two = Decimal::from_i64(2);
        let threshold = one.div(&Decimal::from_i64(Self::ATAN_REDUCE_TO), scale)?;

        let mut v = x.clone();
        let mut halvings = 0u32;
        // The bound is a non-termination guard, not an accuracy parameter:
        // each pass strictly shrinks |v|, and from |x| <= 1 the threshold is
        // reached in five. A `while` with no bound would be a hang if some
        // future `Decimal` rounding made the sequence stall.
        while halvings < 64 && v.abs() > threshold {
            // sqrt cannot fail here: 1 + v^2 >= 1 > 0.
            // sqrt cannot fail here: 1 + v^2 >= 1 > 0.
            let root = one.add(&v.mul(&v, scale)).sqrt(scale)?;
            v = v.div(&one.add(&root), scale)?;
            halvings = halvings.saturating_add(1);
        }

        let mut result = self.atan_series(&v, scale)?;
        for _ in 0..halvings {
            result = result.mul(&two, scale);
        }
        // Back to the caller's working scale; the extra digits were scaffolding.
        Ok(result.rescale(outer_scale))
    }

    /// Reduce |x| below `1/ATAN_REDUCE_TO` before summing the series.
    ///
    /// 16 rather than something larger because the two costs pull opposite
    /// ways: a smaller target means more square roots, a larger one means more
    /// series terms. At 1/16 the series gains about 2.4 digits per term, which
    /// puts even a 100-digit `scale` inside fifty terms, and `x = 1` needs
    /// only five reductions to get there.
    const ATAN_REDUCE_TO: i64 = 16;

    /// The arctangent series itself, with no reduction: `x - x^3/3 + x^5/5 …`.
    ///
    /// Only correct to the working scale when |x| is comfortably below 1, so
    /// it is private to [`Self::atan_reduced`], which is what guarantees that.
    fn atan_series(&self, x: &Decimal, scale: usize) -> Eval {
        let mut result = Decimal::zero();
        let mut term = x.clone();
        let x_sq = x.mul(x, scale);
        let neg_one = Decimal::from_i64(-1);

        // Derived from the requested precision rather than fixed at 100. With
        // |x| < 1/16 each term adds about 2.4 digits, so `scale` terms is
        // ample; the `+ 64` covers small scales where the constant dominates.
        // Accuracy comes from the `is_negligible` exit below -- this is only
        // the guarantee that the loop ends. A FIXED cap was the bug: it made
        // the answer depend on the argument rather than on the precision asked
        // for, so `a(1)` was wrong in the third digit while `a(0.5)` was fine.
        let max_terms = scale.saturating_mul(2).saturating_add(64);
        for i in 0..max_terms {
            // 2i+1 is odd, so never zero.
            let idx = i64::try_from(i).unwrap_or(i64::MAX);
            let denom = Decimal::from_i64(idx.saturating_mul(2).saturating_add(1));
            let contrib = term.div(&denom, scale)?;
            result = result.add(&contrib);
            term = term.mul(&x_sq, scale).mul(&neg_one, scale);
            if term.is_negligible(scale) {
                break;
            }
        }
        Ok(result)
    }

    /// What `l(x)` answers for `x <= 0`: GNU's saturating stand-in for minus
    /// infinity, which is `-(10^scale - 1)` at the current scale.
    ///
    /// Measured at six scales rather than one, because the tracker entry
    /// warned in as many words that a constant matching at `scale=10` "would
    /// be a new bug wearing the old one's clothes" — and it would have been:
    ///
    /// | scale | GNU |
    /// |---|---|
    /// | 0 | `0` |
    /// | 1 | `-9.0` |
    /// | 5 | `-99999.00000` |
    /// | 10 | `-9999999999.0000000000` |
    /// | 20 | twenty nines |
    /// | 50 | fifty nines |
    ///
    /// `scale=0` giving `0` rather than a minus sign is the formula agreeing
    /// with itself: `10^0 - 1` is zero.
    ///
    /// The same sweep answered the entry's other open question. **Every**
    /// non-positive argument saturates identically — `l(-1)`, `l(-100)` and
    /// `l(-0.5)` all give the value `l(0)` does — so GNU has no error path
    /// here at all, and neither do we now.
    fn log_saturation(&self) -> Decimal {
        // 10^scale - 1, negated. Built by arithmetic rather than by writing
        // out nines, so it cannot drift from the formula it is documenting.
        let ten = Decimal::from_i64(10);
        let exp = Decimal::from_i64(i64::try_from(self.scale).unwrap_or(i64::MAX));
        let magnitude = ten
            .pow(&exp, self.scale)
            .unwrap_or_else(|_| Decimal::zero())
            .sub(&Decimal::from_i64(1));
        magnitude.negate().rescale(self.scale)
    }

    /// Natural logarithm using series: ln(x) = 2 * sum( ((x-1)/(x+1))^(2k+1) / (2k+1) ).
    fn builtin_ln(&self, x: &Decimal) -> Eval {
        if x.is_zero() || x.is_negative() {
            return Ok(self.log_saturation());
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

/// How many digits `bc` puts on a line, given a `BC_LINE_LENGTH` of `n`.
///
/// `bc` counts the continuation backslash against the stated width and then
/// leaves one column beyond it unused, so `BC_LINE_LENGTH=10` emits nine
/// columns: eight digits and a `\`. That is one digit narrower than `dc` makes
/// of the same number, which is why the arithmetic lives in each front-end
/// rather than in `bignum` (see [`bignum::wrap_number`]).
///
/// Below 3 there is no room to make progress, and `bc` stops wrapping entirely
/// rather than emitting a backslash per digit — as does `BC_LINE_LENGTH=0`,
/// the documented way for a script to ask for one long number.
fn wrap_chunk(line_length: usize) -> usize {
    if line_length < 3 {
        return 0;
    }
    line_length.saturating_sub(2)
}

/// The output line length, from the environment or the traditional default.
///
/// A setting that is not a number is ignored rather than rejected: a malformed
/// environment should not stop a calculator from calculating.
fn line_length_from_env(var: &str) -> usize {
    let stated = env::var(var)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(bignum::DEFAULT_LINE_LENGTH);
    wrap_chunk(stated)
}

/// Apply `print`'s escape table to a string literal.
///
/// Only `print` interprets escapes; a bare string statement writes its bytes
/// as they were typed. This is not a subtlety we invented — it is what GNU
/// `bc` 1.07.1 does, and `scripts/calc-diff.sh` compares against it:
///
/// | source | `print "…"` | `"…"` alone |
/// |---|---|---|
/// | `a\nb` | `a`, newline, `b` | `a`, `\`, `n`, `b` |
/// | `a\\b` | `a\b` | `a\\b` |
///
/// An escape that is not in the table takes *both* characters with it —
/// `print "a\vb"` writes `ab`, not `a\vb` — as does a backslash with nothing
/// after it. That is deliberate on GNU's part (it is how `\` at end of line
/// continues a string) and a program that relies on an unknown escape
/// surviving would break differently on the two implementations, so we match
/// it rather than improve on it.
fn print_escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('a') => out.push('\x07'),
            Some('b') => out.push('\x08'),
            Some('f') => out.push('\x0c'),
            Some('n') => out.push('\n'),
            // `\q` is bc's way of writing a quote, since the lexer ends a
            // string at the first unescaped `"` and there is no escaped one.
            Some('q') => out.push('"'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            // Both characters are dropped: an unknown escape, and a trailing
            // backslash with nothing to escape.
            Some(_) | None => {}
        }
    }
    out
}

/// Whether a bare expression statement prints nothing of its own.
///
/// bc echoes the value of any expression written as a statement, *except* an
/// assignment (which is silent, so that `x = 1` does not print) and a string
/// literal (which writes its own text and has no value to echo).
///
/// `++`/`--` are *not* assignments for this purpose: GNU `bc` prints `10` for
/// `x = 10; x++` and `11` for `x = 10; ++x`, which falls straight out of each
/// operator's value once the statement is allowed to echo at all. Only a real
/// `=` is silent, so `y = x++` prints nothing while `x++` prints the old `x`.
fn suppresses_auto_print(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Assign(_, _)
            | Expr::OpAssign(_, _, _)
            // A bare string statement -- `"hello"` -- writes the string and
            // nothing else. Evaluating it returns zero for want of anything
            // better, and printing that zero as well made every string
            // statement emit a stray `0` after its text.
            | Expr::StringLit(_)
    )
}

/// What a line of input turned out to be, once appended to whatever was pending.
enum Feed {
    /// The construct is not finished; nothing runs until another line arrives.
    Incomplete,
    /// A complete unit of program, ready to execute.
    Ready(Vec<Stmt>),
    /// A complete unit that will NOT be executed, and why.
    ///
    /// Measured against GNU bc 1.07.1: a unit containing any diagnostic
    /// produces no output at all. `1; $ 2` prints nothing — not even the `1`,
    /// which is a finished statement sitting before the mistake — while
    /// `1\n$ 2\n` prints `1`, because there the good statement is on its own
    /// line. So the thing discarded is the whole unit, and the unit is exactly
    /// what this chunker already accumulates.
    ///
    /// Keeping it separate from `Ready` is what makes "a statement with a
    /// mistake in it does not run" a property of the type rather than a rule
    /// every caller has to remember: there is no way to receive these
    /// statements, because they are not in here.
    Failed(Vec<SyntaxError>),
    /// A `quit` was read. The run stops here and the pending text is discarded
    /// — including any statement written *before* the `quit` in the same unit.
    Quit,
}

/// Assembles input lines into the units GNU bc compiles and runs as one.
///
/// # Why this exists at all
///
/// bc's two keywords for ending a session differ only in *when* they act, and
/// the difference is invisible until input is divided the way GNU divides it.
/// `halt` fires when it is executed; `quit` fires when it is **read** — the
/// manual's words are "when this statement is read, the bc processor is
/// terminated, regardless of where the quit statement is found". Measured
/// against GNU bc 1.07.1:
///
/// ```text
/// printf 'print "A"; quit\n'   | bc -q   ->  (nothing)
/// printf 'print "A"\nquit\n'   | bc -q   ->  A
/// printf 'if (0) { quit }\n'   | bc -q   ->  (nothing)
/// printf 'if (0) { halt }\n'   | bc -q   ->  (nothing runs, session continues)
/// ```
///
/// The first two lines carry the same characters in the same order and differ
/// only in where the newline falls, so no rule phrased over statements can tell
/// them apart. The rule that does fit is phrased over units of *reading*: GNU's
/// scanner exits the moment it scans `quit`, and the statements it has compiled
/// but not yet executed die with it. Hence [`Feed::Quit`] discards the buffer
/// rather than running the part before the keyword.
///
/// # Why a unit is not simply a line
///
/// `if (x)` and its body may sit on separate lines. GNU gets this for free —
/// its parser pulls tokens and its scanner reads another line whenever the
/// parser wants one — whereas ours parses a finished string, so it must ask
/// first whether the string ended too early. That question is
/// [`Parser::truncated`], and it is a strictly better test than the brace
/// counting this used to do: `if (0)` followed by its body on the next line has
/// no braces to count, and under the old rule the body became a second unit and
/// ran unconditionally.
struct Chunker {
    /// Lines read since the last unit was dispatched, each with its newline.
    buffer: String,
    /// The input line number that `buffer`'s first line is, counting from 1.
    ///
    /// The parser sees one unit at a time and numbers its diagnostics from the
    /// start of that unit, so without this every error in the file would be
    /// reported as line 1 — which is indistinguishable from a correct report
    /// about a genuine first-line error, and so worse than useless.
    line_base: u32,
}

impl Chunker {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            line_base: 1,
        }
    }

    /// Move `line_base` past the buffer and empty it.
    fn retire_buffer(&mut self) {
        let lines = u32::try_from(self.buffer.lines().count()).unwrap_or(0);
        self.line_base = self.line_base.saturating_add(lines);
        self.buffer.clear();
    }

    /// Shift a unit's diagnostics onto absolute input lines.
    fn absolute(&self, mut errors: Vec<SyntaxError>) -> Vec<SyntaxError> {
        for e in &mut errors {
            // `line` is 1-based within the unit and `line_base` is the unit's
            // own line, so the two 1s are the same 1 and one of them comes off.
            e.line = self.line_base.saturating_add(e.line.saturating_sub(1));
        }
        errors
    }

    /// Add one line, without its terminator, and say what to do next.
    fn feed(&mut self, line: &str) -> Feed {
        self.buffer.push_str(line);
        self.buffer.push('\n');
        // The whole buffer is re-lexed, not just the new line, because a line is
        // not independently lexable: it may open inside a `/* */` comment or a
        // string started two lines up, where the same characters mean something
        // else entirely. Re-scanning a few lines costs nothing next to getting
        // that wrong.
        let mut parser = Parser::new(&self.buffer);
        if parser.saw_quit() {
            self.retire_buffer();
            return Feed::Quit;
        }
        let stmts = parser.parse_program();
        if parser.truncated {
            // Still open: an `if` with no body yet, a string whose closing
            // quote has not arrived. Any diagnostics found so far are
            // discarded along with the parse, because the next line will be
            // parsed from the top of the same buffer and find them again.
            return Feed::Incomplete;
        }
        let errors = self.absolute(parser.take_errors());
        self.retire_buffer();
        if errors.is_empty() {
            Feed::Ready(stmts)
        } else {
            Feed::Failed(errors)
        }
    }

    /// Whatever is left when the input ends.
    ///
    /// It is run rather than discarded: a program is far more often missing its
    /// final newline than genuinely half-written, and dropping the last line
    /// would answer `printf '2+2' | bc` with silence.
    fn finish(&mut self) -> Option<Feed> {
        if self.buffer.is_empty() {
            return None;
        }
        let text = self.buffer.clone();
        // No `saw_quit` check: every line in the buffer already went through
        // `feed`, which stops at the first `quit` token.
        let mut parser = Parser::new(&text);
        let stmts = parser.parse_program();
        let mut errors = parser.take_errors();
        // `errors.is_empty()` guards it: a truncation that ALREADY has a
        // diagnostic does not get a second one. `print "abc` with no closing
        // quote is truncated *because* of the illegal character just reported,
        // so adding a syntax error on top says one mistake twice — and GNU,
        // measured, prints only `illegal character: "` for it. The truncation
        // that does need reporting is the one where nothing else went wrong:
        // `if (1) {` with a body that never arrives.
        if parser.truncated && errors.is_empty() {
            // `truncated` means "a required token was missing because the
            // input stopped" — which up to now has meant *wait for the next
            // line*. Here there is no next line, so the same fact is a syntax
            // error instead. This is the only place that knows the difference,
            // and it is why `expect` does not report the Eof case itself.
            //
            // GNU agrees: `if (1) {\nprint "A"\n` with nothing after it is
            // `syntax error` and prints nothing, where ours used to print `A`.
            errors.push(SyntaxError {
                at: usize::MAX,
                line: parser.line_here(),
                kind: ErrorKind::Syntax,
            });
        }
        let errors = self.absolute(errors);
        self.retire_buffer();
        Some(if errors.is_empty() {
            Feed::Ready(stmts)
        } else {
            Feed::Failed(errors)
        })
    }
}

/// Run a program already held in memory, in the same units the session on
/// standard input would have run it in.
///
/// File operands and `-e` expressions go through here rather than being parsed
/// whole, because `quit` is defined by reading order and reading order is
/// exactly what parsing-whole throws away. Before this, `printf 'print "A"\nquit\n' > f; bc f`
/// printed nothing, since the single parse saw the `quit` before anything ran.
/// GNU's name for the stdin session, used verbatim in its diagnostics.
const STDIN_SOURCE: &str = "(standard_in)";

/// Print a unit's diagnostics in GNU's format: `NAME LINE: message`.
///
/// No `bc: ` prefix — measured, GNU does not put one on these, the same as its
/// `File %s is unavailable.`. A script that greps for `bc:` will not see these
/// lines, and that is upstream's behaviour rather than an oversight here.
fn report_syntax(source: &str, errors: &[SyntaxError]) {
    for e in errors {
        diag!("{} {}: {}", source, e.line, e.message());
    }
}

fn run_text(interp: &mut Interpreter, text: &str, source: &str) -> Session {
    let mut chunker = Chunker::new();
    // `lines()` rather than `split('\n')`: it strips a trailing `\r` as well, so
    // a script saved with CRLF endings is read the same as one without, and it
    // does not invent a final empty line for text that ends in a newline.
    for line in text.lines() {
        match chunker.feed(line) {
            Feed::Incomplete => {}
            Feed::Quit => return Session::Stop,
            // Reported and not run. The next unit is still read: measured,
            // `print )\nprint "after\n"\n` prints `after` on GNU, so an error
            // is "say so, then carry on", not "stop at the first problem".
            Feed::Failed(errors) => report_syntax(source, &errors),
            Feed::Ready(stmts) => {
                if interp.run(&stmts) == Session::Stop {
                    return Session::Stop;
                }
            }
        }
    }
    match chunker.finish() {
        Some(Feed::Ready(stmts)) => interp.run(&stmts),
        Some(Feed::Failed(errors)) => {
            report_syntax(source, &errors);
            Session::Continue
        }
        Some(Feed::Incomplete | Feed::Quit) | None => Session::Continue,
    }
}

// -------------------------------------------------------------------------
// The command line
// -------------------------------------------------------------------------
//
// Every sentence and every exit status below was *measured* against GNU bc
// 1.07.1 through WSL, not recalled -- and recall was wrong three times, in
// ways that matter:
//
//   * `-e` and `-f` **do not exist** in GNU bc. `bc -e 2+2` answers
//     `invalid option -- 'e'` and exits 1. Our `-e` is a SlateOS extension
//     (it is Gavin Howard's bc that has one), and is marked as such in the
//     usage text so nobody ports a script to a GNU host expecting it.
//   * The long-option table is **alphabetical** and has eight entries, one of
//     which (`--compile`) the usage text does not mention. Measured with
//     `bc --=x`, whose empty prefix matches every entry and so prints the
//     table in declaration order:
//
//         bc: option '--=x' is ambiguous; possibilities: '--compile'
//         '--help' '--interactive' '--mathlib' '--quiet' '--standard'
//         '--version' '--warn'
//
//   * A file that will not open is `File NAME is unavailable.` -- with **no**
//     `bc: ` prefix, and it **stops the run**: `bc good.bc missing.bc
//     good.bc` runs the first file, reports the second and never reaches the
//     third, exiting 1. The previous code here printed a different sentence,
//     kept going, and exited **0**.
//
// Two further behaviours were measured because no amount of reading the
// manual settles them:
//
//   * A file operand does **not** end the run. `printf '9+9\n' | bc a.bc`
//     prints a.bc's output and then evaluates standard input, in one
//     interpreter, so a file may define functions a later session uses.
//   * A bare `-` is **not** standard input; it is a file name that fails to
//     open, and is reported exactly like any other. The comment that used to
//     sit on the operand arm claiming otherwise was wrong.
//
// The one deliberate deviation is quoting, and it is narrower than it used to
// be. GNU prints the name bare, so a file called
// `x⏎bc: /etc/shadow: Permission denied` forges a line bc never wrote. Names
// go through `quotef_os` -- the ELIDING form -- which keeps that protection
// exactly where it is needed and drops it where it is not: a name containing a
// newline, a space or a control character is still quoted and cannot forge
// anything, while an ordinary `nosuch.bc` prints bare and matches GNU byte for
// byte.
//
// It was `quoteaf_os`, the always-quote form, until 2026-09-16. The forgery
// argument was never an argument for quoting CLEAN names, only for quoting
// dangerous ones, and the always-quote form was additionally inconsistent with
// the syntax-error prefix, which names the same file without quotes. See
// `design-decisions.md` §1027.

/// Exits 1 on a bad command line, measured with `bc --zzz-bogus; echo $?`.
const BC: Program = Program::new("bc", 1);

/// GNU's usage block, reduced to what this bc actually does, and printed on
/// **stdout** even when it follows a diagnostic on stderr -- which is what
/// GNU does, because `getopt_long` writes the sentence and the program's own
/// `usage()` writes this.
const USAGE: &str = "\
usage: bc [options] [file ...]
  -h  --help         print this usage and exit
  -i  --interactive  force interactive mode
  -l  --mathlib      use the predefined math routines
  -q  --quiet        don't print initial banner
  -w  --warn         warn about non-standard bc constructs (accepted, no-op)
  -v  --version      print version information and exit
  -e  --expression EXPR   evaluate EXPR (a SlateOS extension; GNU bc has none)";

/// The long options **in GNU's declaration order**, which is observable
/// because `getopt_long` lists an ambiguous prefix's candidates in it.
/// Measured with `bc --=x`; `expression` is ours and is inserted where
/// alphabetical order puts it, so the list still reads as GNU's does.
///
/// The two we refuse are listed rather than omitted, because the table is
/// what decides whether an abbreviation is ambiguous: drop `--standard` and
/// `--s` would silently resolve to `--standard`'s neighbour instead of being
/// refused.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("compile", Long::Compile),
    ("expression", Long::Expression),
    ("help", Long::Help),
    ("interactive", Long::Interactive),
    ("mathlib", Long::Mathlib),
    ("quiet", Long::Quiet),
    ("standard", Long::Standard),
    ("version", Long::Version),
    ("warn", Long::Warn),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Compile,
    Expression,
    Help,
    Interactive,
    Mathlib,
    Quiet,
    Standard,
    Version,
    Warn,
}

/// One thing to evaluate, kept in command-line order.
///
/// Ordered rather than "expressions first, then files" because the
/// interpreter is one piece of state: `bc -e 'define f(x){return x*2}'
/// use.bc` and `bc use.bc -e '…'` are different programs, and the order the
/// user typed is the only defensible reading of which one they meant.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Input {
    /// `-e EXPR`. Bytes, because an argument need not be UTF-8 and the
    /// diagnostic for one that is not should name it rather than panic.
    Expression(Vec<u8>),
    File(OsString),
}

/// What the command line asked for.
#[derive(PartialEq, Eq, Debug)]
enum Request {
    Run(Settings),
    Help,
    Version,
}

#[derive(Default, PartialEq, Eq, Debug)]
struct Settings {
    math_lib: bool,
    quiet: bool,
    /// `-i`: behave as if standard input were a terminal.
    force_interactive: bool,
    inputs: Vec<Input>,
}

impl Settings {
    /// Whether standard input is read after the operands.
    ///
    /// GNU always reads it, because GNU has no `-e`. Ours stops after an
    /// explicit expression, since `bc -e '2+2'` dropping into an interactive
    /// session is nobody's behaviour -- but a plain `bc file.bc` continues to
    /// standard input exactly as GNU's does.
    fn reads_stdin(&self) -> bool {
        !self
            .inputs
            .iter()
            .any(|input| matches!(input, Input::Expression(_)))
    }
}

/// A command line that cannot be run.
#[derive(Debug)]
enum Refusal {
    Getopt(getopt::Error),
    /// A flag GNU implements and we do not. Refused rather than ignored,
    /// because its absence changes the answer -- see [`Refusal::report`].
    Unimplemented(&'static str),
}

impl Refusal {
    fn report(&self) -> ExitCode {
        let status = match self {
            Self::Getopt(e) => {
                diag!("bc: {}", e.sentence);
                // GNU prints the sentence on stderr and the usage block on
                // stdout, from two different pieces of code. Reproduced
                // rather than tidied: a script doing `bc -x 2>/dev/null`
                // still sees the usage, as it does upstream.
                println!("{USAGE}");
                e.status
            }
            Self::Unimplemented(message) => {
                diag!("bc: {message}");
                println!("{USAGE}");
                1
            }
        };
        ExitCode::from(u8::try_from(status).unwrap_or(1))
    }
}

/// A failure while evaluating, which ends the run the way GNU's does.
///
/// The variants carry the *name*, not a rendered sentence, so that the choice
/// between `quoteaf_os` and `quotef_os` is made in one place by the shape of
/// the sentence the name lands in — which is the rule `coreutils::quote`
/// states and the rule a caller assembling its own string always gets wrong.
#[derive(Debug)]
enum Trouble {
    /// GNU's `File %s is unavailable.`, quoted per this tree's policy.
    Unavailable(OsString),
    /// Ours alone: GNU's lexer is byte-oriented and ours needs `&str`, so a
    /// source file that is not UTF-8 is refused instead of being silently
    /// truncated at the first bad byte -- which is what the old `lines()`
    /// loop did, exiting 0 with a partial answer. Tracked in
    /// `known-issues.md` as a limitation to remove by making the lexer take
    /// bytes.
    FileNotUtf8(OsString),
    /// The same, for an argument rather than a file: `bc -e $'\xe9'`.
    ExpressionNotUtf8,
    /// The same again, for the session on standard input.
    StdinNotUtf8,
    /// A read on standard input that failed for a reason other than EOF.
    StdinRead(String),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            // `quotef_os`, the eliding form: a name that needs no quotes gets
            // none, and `File nosuch.bc is unavailable.` then matches GNU bc
            // byte for byte, while a name containing a space or a newline is
            // still quoted rather than dissolving into the sentence.
            //
            // This used to be `quoteaf_os`, the always-quote form, on the
            // grounds that a mid-sentence name needs the quotes to stand out.
            // What settled it was not that argument but an inconsistency:
            // since the syntax-error prefix started naming the source file,
            // `bc` printed the SAME name two different ways -- `File 'prog.bc'
            // is unavailable.` beside `prog.bc 1: syntax error`. One program
            // spelling one file two ways is worse than either convention, and
            // the eliding form is the one that also matches upstream.
            Self::Unavailable(name) => diag!("File {} is unavailable.", quotef_os(name)),
            // Ends the clause, so it takes the bare form when it can, exactly
            // as `wc: missing.txt: No such file or directory` does.
            Self::FileNotUtf8(name) => diag!("bc: {}: not valid UTF-8", quotef_os(name)),
            Self::ExpressionNotUtf8 => diag!("bc: -e expression: not valid UTF-8"),
            Self::StdinNotUtf8 => diag!("bc: standard input: not valid UTF-8"),
            Self::StdinRead(message) => diag!("bc: standard input: {message}"),
        }
        ExitCode::FAILURE
    }
}

// -------------------------------------------------------------------------
// Main entry point
// -------------------------------------------------------------------------

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    // `args_os`, not `args`: `env::args()` panics on an argument that is not
    // UTF-8, so `bc $'caf\xe9.bc'` aborted before the file name could even be
    // reported. A path may hold every byte but `/` and NUL.
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let settings = match parse_args(&args) {
        Err(refusal) => return refusal.report(),
        Ok(Request::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Request::Version) => {
            println!("bc (SlateOS coreutils) 0.1.0");
            return ExitCode::SUCCESS;
        }
        Ok(Request::Run(settings)) => settings,
    };

    let mut interp = Interpreter::new(settings.math_lib);

    // Whether input is a terminal, not whether the *environment* looks like
    // one. `TERM` is inherited by every child of a terminal session, pipes
    // included, so the previous probe said "interactive" for
    // `echo 1+1 | bc` -- and the banner went into the caller's captured
    // output, ahead of the answer. `$(echo 1+1 | bc)` is the single most
    // common way this program is used.
    let stdin = io::stdin();
    let interactive = settings.force_interactive || {
        use std::io::IsTerminal;
        stdin.is_terminal()
    };

    // Before the operands, as GNU's is: the banner introduces the session,
    // and there is no session to introduce when `-e` ends the run.
    if !settings.quiet && interactive && settings.reads_stdin() {
        println!("bc (SlateOS coreutils) 0.1.0");
        println!("Type 'quit' to exit.");
    }

    for input in &settings.inputs {
        // Stop at the first one that fails, which is GNU's behaviour and the
        // only safe one: a later file that uses a function an unreadable
        // earlier file was to have defined would otherwise compute a wrong
        // answer rather than report the missing file.
        match eval_input(&mut interp, input) {
            Err(trouble) => return trouble.report(),
            // `bc -e quit -e 'print "x"'` prints nothing, and neither does a
            // file operand after one that quit: `quit` ends the run, not just
            // the text it appeared in.
            Ok(Session::Stop) => return ExitCode::SUCCESS,
            Ok(Session::Continue) => {}
        }
    }

    if settings.reads_stdin()
        && let Err(trouble) = eval_stdin(&mut interp, &stdin)
    {
        return trouble.report();
    }

    ExitCode::SUCCESS
}

/// Run one `-e` expression or one file operand.
fn eval_input(interp: &mut Interpreter, input: &Input) -> Result<Session, Trouble> {
    // The second element is what to blame if the bytes turn out not to be
    // UTF-8, which for `-e` is the command line rather than any file.
    let (text, blame) = match input {
        Input::Expression(bytes) => (bytes.clone(), Trouble::ExpressionNotUtf8),
        Input::File(path) => {
            // `read`, not `read_to_string`: the latter reports an invalid byte
            // as an *open* failure, so `bc data.bin` claimed the file could
            // not be opened when it had been opened and read in full.
            let bytes = std::fs::read(path).map_err(|_| Trouble::Unavailable(path.clone()))?;
            (bytes, Trouble::FileNotUtf8(path.clone()))
        }
    };
    let text = String::from_utf8(text).map_err(|_| blame)?;
    // The name a diagnostic is blamed on. GNU uses the file operand exactly as
    // it was written on the command line — measured, `bc -q prog.bc` says
    // `prog.bc 1: syntax error`, not an absolutised or quoted form.
    //
    // `-e` has no GNU behaviour to match, because GNU bc has no `-e`: it
    // answers `invalid option -- 'e'` and exits 1 (the flag is Gavin Howard's,
    // and ours is a SlateOS extension). So it gets its own honest label rather
    // than borrowing `(standard_in)`, which would blame the wrong input.
    // `quotef_os`, not `to_string_lossy`: a path is bytes and may not be UTF-8,
    // and lossy decoding would put U+FFFD in a diagnostic that is supposed to
    // name a file the reader can go and open. It takes the bare form when the
    // name has nothing needing quotes, so an ordinary `prog.bc` prints exactly
    // as GNU prints it.
    let source = match input {
        Input::Expression(_) => "(command line)".to_string(),
        Input::File(path) => quotef_os(path),
    };
    Ok(run_text(interp, &text, &source))
}

/// The interactive/pipe session: read until EOF, evaluating each construct as
/// soon as its braces balance.
fn eval_stdin(interp: &mut Interpreter, stdin: &io::Stdin) -> Result<Session, Trouble> {
    let mut handle = stdin.lock();
    let mut chunker = Chunker::new();

    loop {
        // Bytes, then one explicit UTF-8 check. `BufRead::lines()` yields
        // `io::Result<String>` and the old loop answered a decoding failure
        // with `break` -- so one stray byte in a piped script silently
        // truncated the program and still exited 0.
        let mut raw: Vec<u8> = Vec::new();
        match handle.read_until(b'\n', &mut raw) {
            Ok(0) => break,
            Ok(_) => {}
            // Reported, not swallowed. `break` here -- which is what the old
            // loop did -- turns a failed read into a normal end of input, so
            // a truncated program is evaluated and the run exits 0.
            Err(e) => return Err(Trouble::StdinRead(strerror(&e))),
        }
        let Ok(line) = String::from_utf8(raw) else {
            return Err(Trouble::StdinNotUtf8);
        };
        // One `\n` and then one `\r`, which is exactly what `BufRead::lines()`
        // strips. `trim_end_matches` would eat a run of them, so a line whose
        // data genuinely ends in `\r\r` would come back shorter than it was.
        let line = line.strip_suffix('\n').unwrap_or(&line);
        let line = line.strip_suffix('\r').unwrap_or(line);

        match chunker.feed(line) {
            Feed::Incomplete => {}
            // `quit` stops the read loop as well as the evaluation: the rest of
            // the script is not the next thing to run, it is nothing at all.
            // Reading no further is the point of the keyword — on a terminal it
            // is how the session ends, and in a pipe it is what stops a script
            // from being consumed after it asked to stop.
            Feed::Quit => return Ok(Session::Stop),
            Feed::Failed(errors) => report_syntax(STDIN_SOURCE, &errors),
            Feed::Ready(stmts) => {
                if interp.run(&stmts) == Session::Stop {
                    return Ok(Session::Stop);
                }
            }
        }
    }

    // Process any remaining buffer.
    match chunker.finish() {
        Some(Feed::Ready(stmts)) => Ok(interp.run(&stmts)),
        Some(Feed::Failed(errors)) => {
            report_syntax(STDIN_SOURCE, &errors);
            Ok(Session::Continue)
        }
        Some(Feed::Incomplete | Feed::Quit) | None => Ok(Session::Continue),
    }
}

// -------------------------------------------------------------------- parsing

fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut settings = Settings::default();
    let mut only_operands = false;
    let mut at = 0usize;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        if only_operands {
            settings.inputs.push(Input::File(arg.clone()));
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A bare `-` is a file name, not standard input: GNU answers
            // `bc -` with `File - is unavailable.` and exits 1.
            settings.inputs.push(Input::File(arg.clone()));
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) = long_option(body, &bytes, args, &mut at, &mut settings)? {
                return Ok(request);
            }
        } else if let Some(request) = short_options(&bytes, args, &mut at, &mut settings)? {
            return Ok(request);
        }
    }

    Ok(Request::Run(settings))
}

/// The two flags GNU implements and this bc does not.
///
/// They are refused rather than accepted-and-ignored because their absence
/// **changes the answer**: `-s` makes non-standard constructs errors, so
/// silently ignoring it runs a program POSIX bc would have rejected (measured:
/// `echo 'print 1,2' | bc -s` prints `(standard_in) 1: Error: print statement`
/// and computes nothing, while plain `bc` prints `12`), and `-c` emits dc code
/// instead of results. `-w` is the counter-example and is accepted as a no-op
/// -- ignoring it omits an advisory on stderr and leaves every computed value
/// identical, so refusing `bc -w` would break working scripts to no purpose.
///
/// The rule, stated as a property of the flag rather than of this utility:
/// **refuse when ignoring it would change a computed value or an exit status,
/// accept when it would only omit an advisory.** See `design-decisions.md`
/// §361; the work to implement them properly is in `todo.txt`.
const NO_STANDARD: Refusal =
    Refusal::Unimplemented("-s/--standard (reject non-standard constructs) is not implemented");
const NO_COMPILE: Refusal =
    Refusal::Unimplemented("-c/--compile (emit dc code) is not implemented");

/// One `--name`, `--name=value` or `--name value` argument.
fn long_option(
    body: &[u8],
    whole: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<Option<Request>, Refusal> {
    // Split before resolving, so the *name* is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 matches none of
    // them and takes the unrecognised path, reported as the bytes typed.
    let typed =
        std::str::from_utf8(typed).map_err(|_| Refusal::Getopt(BC.unrecognized_option(whole)))?;
    let (name, which) = BC
        .resolve_long(typed, whole, LONG_OPTIONS)
        .map_err(Refusal::Getopt)?;

    match which {
        Long::Standard => return Err(NO_STANDARD),
        Long::Compile => return Err(NO_COMPILE),
        _ => {}
    }

    if which == Long::Expression {
        let value = match inline {
            Some(value) => value.to_vec(),
            None => {
                let Some(separate) = args.get(*next) else {
                    return Err(Refusal::Getopt(BC.long_missing_argument(name)));
                };
                *next = next.saturating_add(1);
                arg_bytes(separate)
            }
        };
        settings.inputs.push(Input::Expression(value));
        return Ok(None);
    }

    if inline.is_some() {
        return Err(Refusal::Getopt(BC.long_unwanted_argument(name)));
    }
    match which {
        Long::Mathlib => settings.math_lib = true,
        Long::Quiet => settings.quiet = true,
        Long::Interactive => settings.force_interactive = true,
        // Accepted and deliberately does nothing; see `refuse`.
        Long::Warn => {}
        Long::Help => return Ok(Some(Request::Help)),
        Long::Version => return Ok(Some(Request::Version)),
        Long::Compile | Long::Expression | Long::Standard => {}
    }
    Ok(None)
}

/// One `-abc` cluster.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed. The old loop did exactly
/// that, and also *continued* past an unknown flag and exited 0.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<Option<Request>, Refusal> {
    let cluster = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = cluster.get(at) {
        match c {
            b'l' => settings.math_lib = true,
            b'q' => settings.quiet = true,
            b'i' => settings.force_interactive = true,
            b'w' => {}
            b's' => return Err(NO_STANDARD),
            b'c' => return Err(NO_COMPILE),
            b'h' => return Ok(Some(Request::Help)),
            b'v' => return Ok(Some(Request::Version)),
            b'e' => {
                // A *required* argument: the rest of the cluster if there is
                // one, otherwise the whole of the next argument. `bc -e` with
                // nothing after it used to become an interactive session,
                // because the missing argument was an `Option` nobody checked.
                let rest = cluster.get(at.saturating_add(1)..).unwrap_or_default();
                let value = if rest.is_empty() {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(BC.short_missing_argument(b'e')));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                } else {
                    rest.to_vec()
                };
                settings.inputs.push(Input::Expression(value));
                return Ok(None);
            }
            _ => return Err(Refusal::Getopt(BC.invalid_option(c))),
        }
        at = at.saturating_add(1);
    }
    Ok(None)
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)]
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

    /// Run a script and report what `func=` would have said for the LAST
    /// fault in it.
    fn fault_context(input: &str) -> String {
        let mut interp = Interpreter::new(false);
        let mut parser = Parser::new(input);
        let stmts = parser.parse_program();
        interp.run(&stmts);
        interp.fault_label().to_string()
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

    /// Feed a whole script to a fresh [`Chunker`] and report, one word per
    /// line, what each line turned out to be. Lines that finished a construct
    /// read `ready`; lines still waiting on more input read `incomplete`; a
    /// line holding a `quit` reads `quit` and ends the list, because nothing
    /// after it is read at all.
    fn feed_lines(script: &str) -> Vec<&'static str> {
        let mut chunker = Chunker::new();
        let mut seen = Vec::new();
        for line in script.lines() {
            match chunker.feed(line) {
                Feed::Incomplete => seen.push("incomplete"),
                Feed::Ready(_) => seen.push("ready"),
                // A unit that parsed but will not run. Distinct from `ready`
                // here rather than folded into it, so a test that expects a
                // line to *execute* cannot be satisfied by one that merely
                // finished parsing and was then thrown away.
                Feed::Failed(_) => seen.push("failed"),
                Feed::Quit => {
                    seen.push("quit");
                    break;
                }
            }
        }
        seen
    }

    /// Every diagnostic a script produces, as `LINE: message`, in the order
    /// they would be printed.
    ///
    /// The line number is included because it is the half most likely to be
    /// quietly wrong: a checker that only asserted "it complained" would pass
    /// on a `bc` that blamed every error in a 400-line script on line 1, which
    /// is the state this whole change is fixing and is barely better than
    /// silence. The source name is left off -- that is the caller's to supply
    /// and is covered where the callers are.
    fn diagnostics(script: &str) -> Vec<String> {
        let mut chunker = Chunker::new();
        let mut seen = Vec::new();
        let mut collect = |feed: Feed| {
            if let Feed::Failed(errors) = feed {
                for e in errors {
                    seen.push(format!("{}: {}", e.line, e.message()));
                }
            }
        };
        for line in script.lines() {
            collect(chunker.feed(line));
        }
        if let Some(feed) = chunker.finish() {
            collect(feed);
        }
        seen
    }

    #[test]
    fn a_brace_inside_a_string_does_not_open_a_block() {
        // The chunker decides a construct is complete by parsing it. Deciding
        // by counting brace *characters* -- which is what this used to do --
        // meant `print "{"` opened a block that nothing would ever close, and
        // every line typed afterwards was swallowed into a buffer that never
        // ran.
        assert_eq!(feed_lines("print \"{\"\n"), ["ready"]);
        assert_eq!(feed_lines("print \"}\"\n"), ["ready"]);
        assert_eq!(feed_lines("s = \"{{{\"\n"), ["ready"]);
    }

    #[test]
    fn a_brace_inside_a_comment_does_not_open_a_block() {
        assert_eq!(feed_lines("1 + 1 # }\n"), ["ready"]);
        assert_eq!(feed_lines("1 + 1 /* { */\n"), ["ready"]);
    }

    #[test]
    fn an_unfinished_block_waits_for_the_rest() {
        assert_eq!(feed_lines("define f(x) {\n"), ["incomplete"]);
        assert_eq!(
            feed_lines("define f(x) {\n  if (x) {\n"),
            ["incomplete", "incomplete"]
        );
        assert_eq!(
            feed_lines("define f(x) {\n  return(x)\n}\n"),
            ["incomplete", "incomplete", "ready"]
        );
    }

    #[test]
    fn a_construct_split_across_lines_without_braces_is_one_unit() {
        // The reason the brace count had to go. GNU's scanner reads another
        // line whenever its parser wants one, so a body on the line after its
        // `if` is still that `if`'s body. Under brace counting these were two
        // units, and the second ran unconditionally.
        assert_eq!(feed_lines("if (0)\nprint \"x\"\n"), ["incomplete", "ready"]);
        assert_eq!(
            feed_lines("while (0)\nprint \"z\"\n"),
            ["incomplete", "ready"]
        );
        assert_eq!(
            feed_lines("for (i=0;i<2;i++)\nprint i\n"),
            ["incomplete", "ready"]
        );
    }

    #[test]
    fn a_half_finished_expression_is_an_error_not_a_continuation() {
        // The boundary of the rule above, and it was measured rather than
        // reasoned: `printf 'x = 1 +\n2\nx\n' | bc -q` answers
        // `(standard_in) 2: syntax error`, then `2`, then `0`. So the second
        // line is a unit of its own and `x` is never assigned -- a missing
        // *operand* ends the statement, where a missing *body* does not.
        //
        // `failed`, not `ready`: the first line is the syntax error GNU says
        // it is, and a unit with an error in it does not run. That it used to
        // read `ready` is why `x` was assigned nothing yet nothing was said --
        // the parser had reached the right verdict and had nowhere to put it.
        assert_eq!(feed_lines("x = 1 +\n2\n"), ["failed", "ready"]);
        // ...and the diagnostic is GNU's, line number included.
        assert_eq!(
            diagnostics("x = 1 +\n2\n"),
            ["2: syntax error"],
            "a missing operand is blamed on the line the newline ENDS"
        );
    }

    #[test]
    fn an_unterminated_comment_or_string_still_terminates_the_scan() {
        // These run to end of input. The lexer must reach `Eof` rather than
        // spin, or the interactive loop hangs on a typo -- and `feed_lines`
        // returning at all is the assertion that it does.
        assert_eq!(feed_lines("1 /* never closed"), ["incomplete"]);
        assert_eq!(feed_lines("{ /* never closed"), ["incomplete"]);
        // A string is `incomplete` rather than `ready` because bc strings
        // genuinely span lines -- measured, `print "ab\ncd"` prints both --
        // so an unclosed quote means the rest has not arrived yet. It used to
        // read `ready`, which is why the closing line of a multi-line string
        // was run as a program of its own.
        assert_eq!(feed_lines("\"never closed"), ["incomplete"]);
        // The waiting ends at end of input, and then it is GNU's wording: the
        // quote that opened a string nothing closed is the illegal character,
        // and there is exactly ONE diagnostic for it -- the truncation is
        // explained by the illegal character and does not earn a second.
        assert_eq!(diagnostics("print \"abc\n"), ["1: illegal character: \""]);
    }

    // --- Saying so: the diagnostics themselves -------------------------------

    #[test]
    fn a_program_with_no_mistakes_in_it_says_nothing() {
        // The control, and the reason it is first: every other test below
        // asserts that bc COMPLAINS, and all of them would pass on a bc that
        // complained about everything -- including one that reported a syntax
        // error on every line of a correct script, which is a worse tool than
        // the silent one this change replaced. Nothing else here can fail in
        // that direction, so this has to.
        for good in [
            "2+2\n",
            "x = 5\nx * 3\n",
            "if (1) {\nprint \"A\"\n}\n",
            "define f(x) {\nreturn (x * 2)\n}\nf(4)\n",
            "while (0) {\n}\n",
            "/* a comment\nspanning lines */ 1\n",
            "1 + \\\n2\n",
            "print \"ab\ncd\"\n",
            "",
        ] {
            assert_eq!(diagnostics(good), Vec::<String>::new(), "on {good:?}");
        }
    }

    #[test]
    fn a_syntax_error_is_named_and_its_unit_does_not_run() {
        // The headline case. `print )` used to print nothing, say nothing and
        // exit 0, which is indistinguishable from an empty program.
        assert_eq!(diagnostics("print )\n"), ["1: syntax error"]);
        // ONE diagnostic, not one per token the confused parser then walks
        // over. A parser that resynchronises noisily buries the real mistake.
        assert_eq!(diagnostics("print ) ) )\n"), ["1: syntax error"]);
        // The unit is discarded whole -- `Feed::Failed` carries no statements,
        // so there is no way for the interpreter to run one. That is the
        // "wrong number with no message" half of the bug, and it is now a
        // property of the type rather than a rule someone has to follow.
        assert_eq!(feed_lines("print )\n"), ["failed"]);
    }

    #[test]
    fn an_error_does_not_stop_the_lines_after_it() {
        // Measured: GNU prints `after`. So this is "say so, then carry on",
        // not "stop at the first problem" -- a distinction that matters for a
        // script whose first line has a typo and whose remaining forty do the
        // work.
        assert_eq!(
            diagnostics("print )\nprint \"after\"\n"),
            ["1: syntax error"]
        );
        assert_eq!(
            feed_lines("print )\nprint \"after\"\n"),
            ["failed", "ready"]
        );
    }

    #[test]
    fn an_illegal_character_is_named_and_then_dropped() {
        // Dropped, not carried: with the `$` gone the parser sees `1 2`, which
        // is itself a syntax error, so BOTH diagnostics appear -- exactly as
        // GNU prints them. This is the pair that made the scanner/parser split
        // worth building: the illegal character is the scanner's finding and
        // the syntax error is the parser's, and neither can produce the other.
        assert_eq!(
            diagnostics("1 $ 2\n"),
            ["1: illegal character: $", "1: syntax error"]
        );
        // With a separator there is nothing for the parser to object to, and
        // GNU agrees: `1; $ 2` is `illegal character: $` and nothing else.
        assert_eq!(diagnostics("1; $ 2\n"), ["1: illegal character: $"]);
        // Still fatal to the unit, though -- measured, GNU prints neither the
        // `1` nor the `2`.
        assert_eq!(feed_lines("1; $ 2\n"), ["failed"]);
    }

    #[test]
    fn two_statements_need_something_between_them() {
        // GNU refuses all of these. Ours used to run them, printing answers to
        // a program GNU calls malformed.
        for bad in [
            "1 2\n",
            "print \"a\" print \"b\"\n",
            "1 x=2\n",
            // A closing brace ends a statement but does NOT license a
            // following one -- measured, and the opposite of what one would
            // guess from most languages.
            "{ 1 } 2\n",
            "if (1) { print \"a\" } 2\n",
            "while (0) { } 2\n",
            "for (i=0;i<1;i++) { } 2\n",
            // A braceless body is a statement like any other.
            "if (1) print \"a\" 2\n",
            // And inside a block the rule is the same.
            "{ 1 2 }\n",
            "1 halt\n",
        ] {
            assert_eq!(
                diagnostics(bad),
                ["1: syntax error"],
                "expected a syntax error for {bad:?}"
            );
        }
    }

    #[test]
    fn requiring_a_separator_does_not_reject_valid_programs() {
        // The control for the test above, and the reason the followers were
        // MEASURED rather than reasoned about: the failure mode of getting
        // this rule too strict is refusing programs people actually write,
        // which is worse than the over-acceptance it fixes. Every line here is
        // accepted by GNU.
        for good in [
            "1; 2\n",
            "1\n2\n",
            // `}` may follow a statement.
            "{ print \"a\" }\n",
            "if (1) { print \"a\" }\n",
            "define f() {\n  return (1)\n}\n",
            // `else` may follow one, braced or not.
            "if (1) print \"a\" else print \"b\"\n",
            "if (0) { print \"a\" } else { print \"b\" }\n",
            // A function DEFINITION is its own input item in GNU's grammar,
            // so something may follow it with no separator at all. This is the
            // row that stops `require_terminator` being bolted onto
            // `parse_define` as well.
            "define f() { return (1) } f()\n",
            // End of input is a legal follower, which is what lets a file
            // without a trailing newline run.
            "1",
            "print \"a\"",
            // Trailing separators, empty statements and blank lines.
            "1;\n",
            "1;;\n",
            "\n\n1\n\n",
            "for (i=0;i<2;i++) { print i }\n",
            "while (0) { }\n",
        ] {
            assert_eq!(
                diagnostics(good),
                Vec::<String>::new(),
                "valid program refused: {good:?}"
            );
        }
    }

    #[test]
    fn diagnostics_come_out_in_reading_order_not_stage_order() {
        // Measured both ways round, because this is exactly the kind of thing
        // that looks obviously scanner-first until it is checked:
        //     `) $` -> syntax error, then illegal character
        //     `$ )` -> illegal character, then syntax error
        // A rule of "scanner findings first" gets the first of these backwards,
        // and nothing else in the suite would notice.
        assert_eq!(
            diagnostics(") $\n"),
            ["1: syntax error", "1: illegal character: $"]
        );
        assert_eq!(
            diagnostics("$ )\n"),
            ["1: illegal character: $", "1: syntax error"]
        );
    }

    #[test]
    fn line_numbers_count_the_whole_input_not_the_unit() {
        // The parser sees one unit at a time and numbers from the top of it,
        // so without `Chunker::line_base` every one of these would read `1:`.
        // That failure mode is worth a test of its own because it is invisible
        // in any one-line example -- and a report that blames line 1 for a
        // mistake on line 40 is worse than no line number, since the reader
        // goes and stares at a line that is fine.
        assert_eq!(diagnostics("1\n2\nprint )\n"), ["3: syntax error"]);
        assert_eq!(
            diagnostics("1\n2\n3 $ 4\n"),
            ["3: illegal character: $", "3: syntax error"]
        );
        assert_eq!(
            diagnostics("print )\n1\nprint )\n"),
            ["1: syntax error", "3: syntax error"]
        );
        // Newlines inside a block comment are counted too. They are consumed
        // by a different branch of the scanner from the one that makes the
        // `Newline` token, which is why the count lives in `bump` and not
        // there: miss this and every diagnostic after the first long comment
        // points somewhere plausible and wrong.
        assert_eq!(
            diagnostics("/* one\ntwo\nthree */ print )\n"),
            ["3: syntax error"]
        );
        // ...and so are the ones inside a multi-line string.
        assert_eq!(
            diagnostics("s = \"one\ntwo\"\nprint )\n"),
            ["3: syntax error"]
        );
    }

    #[test]
    fn arctangent_is_right_where_its_series_converges_slowest() {
        // `a(1)` is pi/4, and it is the hardest argument there is: the
        // Maclaurin series becomes the alternating harmonic series at x = 1,
        // and the old fixed 100-term sum stopped at `.7828982258` -- wrong in
        // the THIRD digit, by 0.3%. Every value below was measured against GNU
        // bc 1.07.1 and agrees with it exactly.
        let atan = |expr: &str, scale: usize| -> String {
            let src = format!("scale={scale}\n{expr}\n");
            let mut interp = Interpreter::new(true);
            let mut parser = Parser::new(&src);
            let stmts = parser.parse_program();
            interp.run(&stmts);
            interp.output_buf.join("")
        };

        assert_eq!(atan("a(1)", 10), ".7853981633");
        // Four times it is pi, which is the check a reader can do by eye.
        assert_eq!(atan("4*a(1)", 10), "3.1415926532");
        // Either side of 1, including the neighbourhood the |x|>1 inversion
        // maps INTO the slow region: `a(1.0001)` becomes `a(.9999)`, which the
        // inversion alone does not help at all.
        assert_eq!(atan("a(0)", 10), "0");
        assert_eq!(atan("a(0.5)", 10), ".4636476090");
        assert_eq!(atan("a(2)", 10), "1.1071487177");
        assert_eq!(atan("a(-1)", 10), "-.7853981633");
        assert_eq!(atan("a(1.0001)", 10), ".7854481608");
        assert_eq!(atan("a(100)", 10), "1.5607966601");
        // Just over the reduction threshold of 1/16, where the argument is
        // halved exactly once -- the boundary a fixed term count never had.
        assert_eq!(atan("a(0.07)", 10), ".0698860016");
        // At scale 30, where a term-capped sum could not get near. `a(0.6)`
        // is exact to all thirty places.
        assert_eq!(atan("a(0.6)", 30), ".540419500270584155443578364608");
        // `a(1)` was exact to only 24 places when this was written, capped not
        // by this function but by the long-division borrow in `BigInt::divmod`
        // that every square root in the reduction leans on. With that fixed it
        // is exact to all thirty, and to fifty.
        assert_eq!(atan("a(1)", 30), ".785398163397448309615660845819");
        assert_eq!(
            atan("a(1)", 50),
            ".78539816339744830961566084581987572104929234984377"
        );
        // The neighbours are untouched: `j` sits in the same harness row and
        // was always right, so a change that broke it would be caught here
        // rather than in a differential run hours later.
        assert_eq!(atan("j(0,1)", 10), ".7651976865");
        assert_eq!(atan("s(1)", 10), ".8414709848");
        assert_eq!(atan("c(1)", 10), ".5403023058");
        assert_eq!(atan("e(1)", 10), "2.7182818284");
        assert_eq!(atan("l(2)", 10), ".6931471805");
    }

    #[test]
    fn the_log_of_a_non_positive_number_saturates_as_gnu_does() {
        let ml = |expr: &str, scale: usize| -> String {
            let src = format!("scale={scale}\n{expr}\n");
            let mut interp = Interpreter::new(true);
            let mut parser = Parser::new(&src);
            let stmts = parser.parse_program();
            interp.run(&stmts);
            interp.output_buf.join("")
        };

        // `-(10^scale - 1)`, rendered at the current scale. Measured against
        // GNU bc 1.07.1 at every scale below -- and at six of them rather than
        // one, because a constant that happened to match at `scale=10` would
        // have looked exactly like a fix.
        assert_eq!(ml("l(0)", 0), "0");
        assert_eq!(ml("l(0)", 1), "-9.0");
        assert_eq!(ml("l(0)", 5), "-99999.00000");
        assert_eq!(ml("l(0)", 10), "-9999999999.0000000000");
        assert_eq!(ml("l(0)", 20), "-99999999999999999999.00000000000000000000");
        // `scale=0` answering `0` and not `-0` is the formula agreeing with
        // itself: `10^0 - 1` is zero, and zero has no sign.
        assert_eq!(ml("l(0)", 0), "0");

        // EVERY non-positive argument, not just zero. GNU has no error path
        // here, which the entry listed as unmeasured and this settles.
        for arg in ["l(-1)", "l(-100)", "l(-0.5)", "l(0)"] {
            assert_eq!(ml(arg, 10), "-9999999999.0000000000", "on {arg}");
        }

        // The control: a positive argument still computes a logarithm rather
        // than saturating. Without this, an `l` that returned the sentinel for
        // everything would pass every assertion above.
        assert_eq!(ml("l(1)", 10), "0");
        assert_eq!(ml("l(2)", 10), ".6931471805");
        assert_eq!(ml("l(7)", 10), "1.9459101490");
        assert_eq!(ml("l(0.5)", 10), "-.6931471805");
        // `e(l(7))` is `6.9999999996`, NOT `7.0000000000` -- measured on GNU,
        // which answers the same. Written down as the round trip really comes
        // out rather than as the number it ought to be: the first draft of
        // this line asserted the tidy value, which is a claim about arithmetic
        // nobody performed.
        assert_eq!(ml("e(l(7))", 10), "6.9999999996");
    }

    #[test]
    fn a_runtime_error_is_worded_the_way_gnu_words_it() {
        // GNU capitalises in `bc` and does not in `dc`, and the two programs
        // do not even use the same words -- `dc` says "square root of negative
        // number", with no "a". The shared `DecimalError` text is `dc`-shaped
        // apart from the capital, so `bc` capitalises it rather than keeping a
        // second copy of the sentence that could drift from the first.
        assert_eq!(
            RuntimeError::Math(DecimalError::DivideByZero).to_string(),
            "Divide by zero"
        );
        assert_eq!(
            RuntimeError::Math(DecimalError::NegativeSqrt).to_string(),
            "Square root of a negative number"
        );
        // A different sentence, not a different case: GNU's ends in a full
        // stop, and ours used to read `undefined function f`.
        assert_eq!(
            RuntimeError::UndefinedFunction("f".to_string()).to_string(),
            "Function f not defined."
        );
        // Only the `Math` arm is capitalised; `Halt` is bc's own text and is
        // never printed at all. Asserted so that a future variant cannot be
        // added to the capitalising arm by accident.
        assert_eq!(RuntimeError::Halt.to_string(), "halt");
    }

    #[test]
    fn a_runtime_error_names_the_function_it_happened_in() {
        // All measured against GNU bc 1.07.1 before being written here.
        // Top level is GNU's `(main)`, spelled with the parentheses.
        assert_eq!(fault_context("1/0\n"), "(main)");
        assert_eq!(fault_context("sqrt(-1)\n"), "(main)");
        // A function that was never defined is a fault in the CALLER, because
        // the callee has no body to be inside of.
        assert_eq!(fault_context("f(1)\n"), "(main)");
        // The innermost frame wins. `f` calls `g`, `g` divides by zero, and
        // GNU says `func=g` -- the frame the fault happened in, not the one
        // the user typed.
        assert_eq!(
            fault_context("define g(x) { return (x/0) }\ndefine f(x) { return (g(x)) }\nf(1)\n"),
            "g"
        );
        // ...and when the callee does not exist, the innermost frame that DOES
        // exist is the caller, so this is `f` and not `(main)`.
        assert_eq!(
            fault_context("define f(x) {\n  return (g(x))\n}\nf(1)\n"),
            "f"
        );
        // The fault in `f`'s own body, with `g` succeeding, is `f`.
        assert_eq!(
            fault_context("define g(x) { return (x) }\ndefine f(x) { return (g(x)/0) }\nf(1)\n"),
            "f"
        );
    }

    #[test]
    fn the_function_name_does_not_survive_into_the_next_statement() {
        // The reason `run` clears `fault_fn` every statement rather than after
        // printing. Measured: GNU reports `func=g` and then `func=(main)` for
        // exactly this script. Without the reset the second fault inherits the
        // first one's frame and blames a function it never entered -- a wrong
        // answer in a diagnostic, which is the failure this whole pair of
        // entries is about.
        assert_eq!(
            fault_context("define g(x) { return (x/0) }\ng(1)\n1/0\n"),
            "(main)"
        );
        // The control for it: a run whose only fault IS in `g` still says `g`,
        // so the assertion above is about the reset and not about the label
        // being stuck on `(main)` for every script that has two statements.
        assert_eq!(
            fault_context("define g(x) { return (x/0) }\n1\ng(1)\n"),
            "g"
        );
        // And a script with no fault at all never acquires a frame.
        assert_eq!(fault_context("2+2\n"), "(main)");
    }

    #[test]
    fn a_construct_left_open_at_end_of_input_is_an_error() {
        // While more input might still arrive this is `incomplete`, which is
        // what lets an `if` body sit on the next line. When the input stops it
        // becomes the syntax error GNU calls it -- ours used to run the body
        // anyway and print `A`.
        assert_eq!(
            feed_lines("if (1) {\nprint \"A\"\n"),
            ["incomplete", "incomplete"]
        );
        assert_eq!(diagnostics("if (1) {\nprint \"A\"\n"), ["3: syntax error"]);
        // The same fact one line earlier: still open, still nothing said yet.
        assert_eq!(diagnostics("if (1) {\n"), ["2: syntax error"]);
    }

    #[test]
    fn a_comment_or_a_continuation_may_span_lines() {
        // Both are invisible to the parser: `/* one` yields no tokens at all
        // and reads exactly like a blank line, and `1 + \` yields `1` and `+`
        // and reads exactly like the missing-operand error the parser is right
        // not to wait for. The scanner is the only layer that can tell.
        //
        // Measured: `printf '/* one\ntwo */ 2+2\n' | bc -q` answers 4, and
        // `printf '1 + \\\n2\n' | bc -q` answers 3. Splitting either into two
        // units answers something else -- 2 and "1 then 2" respectively.
        assert_eq!(feed_lines("/* one\ntwo */ 2+2\n"), ["incomplete", "ready"]);
        assert_eq!(feed_lines("1 + \\\n2\n"), ["incomplete", "ready"]);
    }

    #[test]
    fn a_quit_inside_a_comment_is_not_a_quit() {
        // The chunker asks the token stream, and a comment produces no tokens
        // -- so this holds by construction. It is asserted anyway because the
        // construction is exactly what a later refactor would be tempted to
        // replace with a substring search.
        assert_eq!(feed_lines("/* quit */ 1\n"), ["ready"]);
        assert_eq!(feed_lines("# quit\n1\n"), ["ready", "ready"]);
    }

    // --- `quit` is read-time, `halt` is execution-time ---

    #[test]
    fn quit_is_noticed_when_the_line_is_read_not_when_it_would_run() {
        // Measured against GNU bc 1.07.1. The two scripts hold the same
        // characters in the same order and differ only in where the newline
        // falls, which is why no rule phrased over statements can separate
        // them.
        assert_eq!(feed_lines("print \"A\"; quit\n"), ["quit"]);
        assert_eq!(feed_lines("print \"A\"\nquit\n"), ["ready", "quit"]);
    }

    #[test]
    fn quit_fires_from_inside_a_branch_that_is_never_taken() {
        // `if (0) { quit }` ends the session on GNU: the keyword is read, and
        // reading is the whole trigger. `halt` in the same place does nothing,
        // because it is reached only if the branch runs.
        assert_eq!(feed_lines("if (0) { quit }\n"), ["quit"]);
        assert_eq!(feed_lines("if (0) { halt }\n"), ["ready"]);
    }

    #[test]
    fn the_letters_of_quit_are_not_a_quit() {
        // The question is asked of the tokens, never of the text: a string and
        // an identifier that merely start with those four letters are not the
        // keyword, and the lexer has already drawn that line.
        assert_eq!(feed_lines("print \"quit\"\n"), ["ready"]);
        assert_eq!(feed_lines("quitx = 1\n"), ["ready"]);
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

    // `++`/`--` written as a statement *do* echo, unlike `=`. The two tests
    // below used to assert that they were silent, which is what GNU bc
    // 1.07.1 disagrees with: `x=5; x++; x` prints `5` then `6`.
    #[test]
    fn test_increment() {
        // `x++` echoes the value before the increment, then `x` is 6.
        assert_eq!(capture_output("x=5\nx++\nx"), vec!["5", "6"]);
        assert_eq!(capture_output("x=5\nx--\nx"), vec!["5", "4"]);
    }

    #[test]
    fn test_pre_increment() {
        // `++x` echoes the value after the increment.
        assert_eq!(capture_output("x=5\n++x\nx"), vec!["6", "6"]);
        assert_eq!(capture_output("x=5\n--x\nx"), vec!["4", "4"]);
    }

    #[test]
    fn an_increment_inside_an_assignment_stays_silent() {
        // Only the outermost operator decides: `=` is silent even though the
        // `++` it wraps would have echoed on its own.
        assert_eq!(capture_output("x=5\ny=x++\ny\nx"), vec!["5", "6"]);
    }

    #[test]
    fn a_bare_string_statement_does_not_interpret_escapes() {
        // Escapes belong to `print`, not to the literal. GNU bc writes four
        // characters for `"a\nb"` on its own line and three for the same
        // string given to `print`.
        assert_eq!(capture_output("\"a\\nb\""), vec!["a\\nb"]);
        assert_eq!(capture_output("print \"a\\nb\""), vec!["a\nb"]);
        // `\q` is the only way to get a quote out, since the lexer ends a
        // string at the very next `"`.
        assert_eq!(capture_output("print \"a\\qb\""), vec!["a\"b"]);
        // An escape that is not in the table takes both characters with it.
        assert_eq!(capture_output("print \"a\\vb\""), vec!["ab"]);
        assert_eq!(capture_output("print \"a\\\\b\""), vec!["a\\b"]);
    }

    #[test]
    fn a_base_above_sixteen_prints_digits_as_decimal_groups() {
        // Measured against GNU bc 1.07.1; see `Decimal::format_grouped`.
        assert_eq!(capture_output("obase=36\n1295"), vec![" 35 35"]);
        assert_eq!(capture_output("obase=36\n1"), vec![" 01"]);
        assert_eq!(capture_output("obase=36\n0"), vec!["0"]);
        assert_eq!(capture_output("obase=36\n-1295"), vec!["- 35 35"]);
        assert_eq!(capture_output("obase=100\n12345"), vec![" 01 23 45"]);
        assert_eq!(capture_output("obase=17\n255"), vec![" 15 00"]);
        assert_eq!(capture_output("obase=1000\n999999"), vec![" 999 999"]);
        // The `.` stands in for the first fractional digit's space.
        assert_eq!(
            capture_output("scale=4\nobase=20\n1/2"),
            vec![".10 00 00 00"]
        );
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
        // Exact, but still reported to the ten places `scale` asked for: the
        // library's final step is a division, and a division has exactly
        // `scale` places whether or not the last of them are zero.
        let output = capture_output_ml("scale=10\ne(0)");
        assert!(!output.is_empty());
        assert_eq!(output[0], "1.0000000000");
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
        // A negative exponent is `1/(a^|b|)`, a division, so the result has
        // exactly `scale` places -- `.125` padded to five, not trimmed to three.
        let output = capture_output("scale=5\n2^-3");
        assert_eq!(output, vec![".12500"]);
    }

    #[test]
    fn the_stated_line_length_leaves_a_column_beyond_the_backslash() {
        // Measured against GNU bc 1.07.1: `BC_LINE_LENGTH=10` emits nine
        // columns, eight digits and a `\`, and the default 70 gives 68 digits.
        // GNU *dc* puts one more on each line from the same source tarball,
        // which is why this conversion lives here and not in `bignum`.
        assert_eq!(wrap_chunk(10), 8);
        assert_eq!(wrap_chunk(70), 68);
        assert_eq!(wrap_chunk(4), 2);
        assert_eq!(wrap_chunk(3), 1);
        // Below 3, GNU bc stops wrapping rather than emitting a backslash per
        // digit -- `BC_LINE_LENGTH=2` prints 2^40 on one line.
        assert_eq!(wrap_chunk(2), 0);
        assert_eq!(wrap_chunk(1), 0);
        assert_eq!(wrap_chunk(0), 0);
    }

    #[test]
    fn a_long_number_is_continued_at_the_width_bc_uses() {
        // End to end through `render`, at the default width: 2^1000 is 302
        // digits, so four lines of 68 and a last of 30, each continued line 69
        // columns wide including the backslash. Verified against GNU bc.
        let interp = Interpreter::new(false);
        let value = Decimal::parse("2", 10)
            .pow(&Decimal::parse("1000", 10), 0)
            .expect("2^1000");
        let rendered = interp.render(&value);
        let lines: Vec<&str> = rendered.split('\n').collect();
        assert_eq!(lines.len(), 5);
        for line in lines.iter().take(4) {
            assert_eq!(line.len(), 69);
            assert!(line.ends_with('\\'));
        }
        assert_eq!(lines[4].len(), 30);
        let rejoined: String = lines
            .iter()
            .map(|l| l.strip_suffix('\\').unwrap_or(l))
            .collect();
        assert_eq!(rejoined.len(), 302);
        assert!(rejoined.ends_with("069376"));
    }

    // ---------------------------------------------------------------------
    // The command line
    // ---------------------------------------------------------------------
    //
    // Every expectation below was measured against GNU bc 1.07.1 through WSL
    // and is cited in the comment on the test that locks it in. The tests
    // that assert a *sentence* are asserting glibc's, reached through
    // `coreutils::getopt`, not a sentence invented here.

    fn parse(argv: &[&str]) -> Result<Request, Refusal> {
        let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
        parse_args(&args)
    }

    fn settings(argv: &[&str]) -> Settings {
        match parse(argv) {
            Ok(Request::Run(s)) => s,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn refusal(argv: &[&str]) -> Refusal {
        match parse(argv) {
            Err(refusal) => refusal,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn getopt_sentence(argv: &[&str]) -> String {
        match refusal(argv) {
            Refusal::Getopt(e) => e.sentence,
            other => panic!("expected a getopt error, got {other:?}"),
        }
    }

    fn file(name: &str) -> Input {
        Input::File(OsString::from(name))
    }

    fn expr(text: &str) -> Input {
        Input::Expression(text.as_bytes().to_vec())
    }

    #[test]
    fn no_arguments_reads_standard_input() {
        let s = settings(&[]);
        assert_eq!(s, Settings::default());
        assert!(s.reads_stdin());
    }

    #[test]
    fn short_flags_cluster() {
        let s = settings(&["-lq"]);
        assert!(s.math_lib);
        assert!(s.quiet);
        assert_eq!(settings(&["-l", "-q"]), s);
        assert_eq!(settings(&["--mathlib", "--quiet"]), s);
    }

    #[test]
    fn interactive_is_forced_by_i_and_by_the_long_name() {
        assert!(settings(&["-i"]).force_interactive);
        assert!(settings(&["--interactive"]).force_interactive);
        assert!(!settings(&[]).force_interactive);
    }

    #[test]
    fn warn_is_accepted_and_does_nothing() {
        // Measured: `-w` only adds an advisory on stderr -- `echo 'print 1,2'
        // | bc -w` still prints `12` -- so ignoring it cannot change an
        // answer, which is why it is the one unimplemented flag not refused.
        assert_eq!(settings(&["-w"]), Settings::default());
        assert_eq!(settings(&["--warn"]), Settings::default());
    }

    #[test]
    fn standard_and_compile_are_refused_rather_than_ignored() {
        // Both change the answer if ignored, so a bc that quietly accepted
        // them would run a program POSIX bc rejects, or print results where
        // dc code was asked for.
        for argv in [
            &["-s"][..],
            &["--standard"][..],
            &["-c"][..],
            &["--compile"][..],
        ] {
            match refusal(argv) {
                Refusal::Unimplemented(message) => assert!(
                    message.contains("is not implemented"),
                    "{argv:?} -> {message}"
                ),
                other => panic!("{argv:?} should be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn operands_are_files_in_the_order_typed() {
        let s = settings(&["a.bc", "b.bc"]);
        assert_eq!(s.inputs, vec![file("a.bc"), file("b.bc")]);
        assert!(s.reads_stdin(), "GNU reads stdin after the operands");
    }

    #[test]
    fn a_bare_dash_is_a_file_name_and_not_standard_input() {
        // Measured: `printf '3+3\n' | bc -` answers `File - is unavailable.`
        // and exits 1. It never reads the pipe.
        assert_eq!(settings(&["-"]).inputs, vec![file("-")]);
    }

    #[test]
    fn double_dash_ends_the_options() {
        assert_eq!(settings(&["--", "-l"]).inputs, vec![file("-l")]);
        assert!(!settings(&["--", "-l"]).math_lib);
    }

    #[test]
    fn expressions_and_files_keep_command_line_order() {
        assert_eq!(
            settings(&["-e", "1+1", "a.bc", "-e", "2+2"]).inputs,
            vec![expr("1+1"), file("a.bc"), expr("2+2")]
        );
    }

    #[test]
    fn every_spelling_of_an_expression_argument_is_accepted() {
        let want = vec![expr("1+1")];
        assert_eq!(settings(&["-e", "1+1"]).inputs, want);
        assert_eq!(settings(&["-e1+1"]).inputs, want);
        assert_eq!(settings(&["--expression", "1+1"]).inputs, want);
        assert_eq!(settings(&["--expression=1+1"]).inputs, want);
    }

    #[test]
    fn an_expression_suppresses_the_standard_input_session() {
        // Ours, not GNU's -- GNU has no `-e` at all. `bc -e '2+2'` dropping
        // into an interactive session is nobody's behaviour.
        assert!(!settings(&["-e", "2+2"]).reads_stdin());
        assert!(!settings(&["a.bc", "-e", "2+2"]).reads_stdin());
        assert!(settings(&["a.bc"]).reads_stdin());
    }

    #[test]
    fn a_missing_expression_argument_is_an_error_not_a_session() {
        // The old parser wrote `args.next()` into an `Option` nobody checked,
        // so `bc -e` silently became an interactive bc.
        assert_eq!(
            getopt_sentence(&["-e"]),
            "option requires an argument -- 'e'"
        );
        assert_eq!(
            getopt_sentence(&["--expression"]),
            "option '--expression' requires an argument"
        );
    }

    #[test]
    fn an_unknown_short_option_stops_the_run() {
        // Measured: `bc -Z` prints `bc: invalid option -- 'Z'` on stderr, the
        // usage block on stdout, and exits 1. The old parser printed
        // `bc: unknown option: -Z`, carried on, and exited 0.
        assert_eq!(getopt_sentence(&["-Z"]), "invalid option -- 'Z'");
        match refusal(&["-Z"]) {
            Refusal::Getopt(e) => assert_eq!(e.status, 1),
            other => panic!("expected a getopt error, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_long_option_echoes_what_was_typed() {
        assert_eq!(getopt_sentence(&["--zzz"]), "unrecognized option '--zzz'");
        // `=VALUE` and all, because nothing resolved to name instead.
        assert_eq!(
            getopt_sentence(&["--zzz=1"]),
            "unrecognized option '--zzz=1'"
        );
    }

    #[test]
    fn an_ambiguous_abbreviation_lists_the_table_in_gnus_order() {
        // Measured with `bc --=x`, whose empty prefix matches every entry:
        // GNU's table is alphabetical. `--expression` is ours and sits where
        // alphabetical order puts it, between `--compile` and `--help`.
        assert_eq!(
            getopt_sentence(&["--=x"]),
            "option '--=x' is ambiguous; possibilities: '--compile' \
             '--expression' '--help' '--interactive' '--mathlib' '--quiet' \
             '--standard' '--version' '--warn'"
        );
    }

    #[test]
    fn an_unambiguous_abbreviation_resolves() {
        assert!(settings(&["--math"]).math_lib);
        // `--q` is unique, `--warn` and `--version` share no prefix with it.
        assert!(settings(&["--q"]).quiet);
    }

    #[test]
    fn a_flag_that_takes_nothing_refuses_a_value() {
        assert_eq!(
            getopt_sentence(&["--mathlib=1"]),
            "option '--mathlib' doesn't allow an argument"
        );
    }

    #[test]
    fn help_and_version_are_requests_rather_than_settings() {
        assert_eq!(parse(&["-h"]).ok(), Some(Request::Help));
        assert_eq!(parse(&["--help"]).ok(), Some(Request::Help));
        assert_eq!(parse(&["-v"]).ok(), Some(Request::Version));
        assert_eq!(parse(&["--version"]).ok(), Some(Request::Version));
        // They win from inside a cluster too, and before a later bad option.
        assert_eq!(parse(&["-lh"]).ok(), Some(Request::Help));
    }

    #[test]
    fn the_usage_block_names_every_flag_the_parser_accepts() {
        // A usage text that drifts from the parser is how a user learns an
        // option exists only by reading the source.
        for flag in ["-h", "-i", "-l", "-q", "-w", "-v", "-e"] {
            assert!(USAGE.contains(flag), "usage does not mention {flag}");
        }
        assert!(
            USAGE.contains("SlateOS extension"),
            "-e is not GNU's and the usage must say so"
        );
    }

    /// `env::args()` panics on one of these, which is what made
    /// `bc $'caf\xe9.bc'` abort before it could name the file.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_argument_is_a_file_name_rather_than_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let arg = OsString::from_vec(b"caf\xe9.bc".to_vec());
        assert!(
            arg.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        let parsed = match parse_args(std::slice::from_ref(&arg)) {
            Ok(Request::Run(s)) => s,
            other => panic!("expected a run, got {other:?}"),
        };
        assert_eq!(parsed.inputs, vec![Input::File(arg)]);
    }

    /// The twin of the test above, for the development host.
    ///
    /// The `#[cfg(unix)]` one is the regression test for the defect this
    /// parser was rewritten to fix, and on Windows it **does not run** — which
    /// is the same blind spot that let the defect exist. Windows has its own
    /// argument that no `String` can hold: an unpaired surrogate (a UTF-16 code
    /// unit in `0xD800..=0xDFFF` with no partner), which reaches the same
    /// `unwrap` inside `env::args()` by a different route. Without this the
    /// only build that checks anything here is the `x86_64-slateos` one.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_argument_is_a_file_name_rather_than_a_panic() {
        use std::os::windows::ffi::OsStringExt;
        // "caf\u{D800}.bc" — a lone high surrogate in the middle of a name.
        let arg = OsString::from_wide(&[0x0063, 0x0061, 0x0066, 0xD800, 0x002E, 0x0062, 0x0063]);
        assert!(
            arg.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        let parsed = match parse_args(std::slice::from_ref(&arg)) {
            Ok(Request::Run(s)) => s,
            other => panic!("expected a run, got {other:?}"),
        };
        assert_eq!(parsed.inputs, vec![Input::File(arg)]);
    }
}
