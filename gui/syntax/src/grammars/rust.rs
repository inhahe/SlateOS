//! Rust: tree-sitter-rust 0.24.2 (MIT, Maxim Sokolov and the tree-sitter
//! contributors). `grammars/rust/` holds its `parser.c`, its queries, its
//! test corpus and its `scanner.c` as published; the scanner is ported below,
//! function for function, with the upstream C's names.
//!
//! # Where the port differs
//!
//! - **A comment's characters are compared whole.** The C reads each
//!   character of a block comment into a `char`, which keeps only its low
//!   byte, so `Ī` (U+012A, low byte `*`) followed by `/` ended a comment.
//!   Here a character is a character.
//! - **Letters and blanks are Unicode's and ASCII's, stated.** The C asks
//!   `iswalpha` (is a letter after `1.` -- then it is a method call, not a
//!   float) and `iswspace` (blanks to skip), whose answers depend on the C
//!   library's locale. Here a letter is `char::is_alphabetic`, as Rust's own
//!   identifiers are, and a blank is the C locale's six.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("rust", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/rust/highlights.scm");

/// The external tokens, in the grammar's order (`TokenType` in scanner.c).
#[derive(Clone, Copy)]
enum Token {
    StringContent,
    StringClose,
    RawStringLiteralStart,
    RawStringLiteralContent,
    RawStringLiteralEnd,
    FloatLiteral,
    BlockOuterDocMarker,
    BlockInnerDocMarker,
    BlockCommentContent,
    LineDocContent,
    ErrorSentinel,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The scanner's state: how many `#`s opened the raw string it is in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    opening_hash_count: u8,
}

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// `iswdigit`: an ASCII digit.
fn is_digit(c: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&c)
}

fn is_num_char(c: i32) -> bool {
    is(c, '_') || is_digit(c)
}

/// `iswspace` in the C locale: space, tab, newline, vertical tab, form
/// feed, carriage return.
fn is_space(c: i32) -> bool {
    c == 0x20 || (0x09..=0x0d).contains(&c)
}

/// `iswalpha`, as a letter Rust's identifiers may start with.
fn is_alpha(c: i32) -> bool {
    u32::try_from(c)
        .ok()
        .and_then(char::from_u32)
        .is_some_and(char::is_alphabetic)
}

fn process_string(lexer: &mut Lexer<'_>) -> bool {
    let mut has_content = false;
    loop {
        let c = lexer.lookahead();
        if is(c, '"') || is(c, '\\') {
            break;
        }
        if lexer.eof() {
            return false;
        }
        has_content = true;
        lexer.advance();
    }
    lexer.set_result(Token::StringContent.symbol());
    lexer.mark_end();
    has_content
}

fn scan_raw_string_start(scanner: &mut Scanner, lexer: &mut Lexer<'_>) -> bool {
    if is(lexer.lookahead(), 'b') || is(lexer.lookahead(), 'c') {
        lexer.advance();
    }
    if !is(lexer.lookahead(), 'r') {
        return false;
    }
    lexer.advance();
    // A `u8`, as in the C: Rust allows at most 255 `#`s.
    let mut opening_hash_count: u8 = 0;
    while is(lexer.lookahead(), '#') {
        lexer.advance();
        opening_hash_count = opening_hash_count.wrapping_add(1);
    }
    if !is(lexer.lookahead(), '"') {
        return false;
    }
    lexer.advance();
    scanner.opening_hash_count = opening_hash_count;
    lexer.set_result(Token::RawStringLiteralStart.symbol());
    true
}

fn scan_raw_string_content(scanner: &Scanner, lexer: &mut Lexer<'_>) -> bool {
    loop {
        if lexer.eof() {
            return false;
        }
        if is(lexer.lookahead(), '"') {
            lexer.mark_end();
            lexer.advance();
            let mut hash_count: u8 = 0;
            while is(lexer.lookahead(), '#') && hash_count < scanner.opening_hash_count {
                lexer.advance();
                hash_count = hash_count.saturating_add(1);
            }
            if hash_count == scanner.opening_hash_count {
                lexer.set_result(Token::RawStringLiteralContent.symbol());
                return true;
            }
        } else {
            lexer.advance();
        }
    }
}

fn scan_raw_string_end(scanner: &Scanner, lexer: &mut Lexer<'_>) -> bool {
    lexer.advance();
    for _ in 0..scanner.opening_hash_count {
        lexer.advance();
    }
    lexer.set_result(Token::RawStringLiteralEnd.symbol());
    true
}

fn process_float_literal(lexer: &mut Lexer<'_>) -> bool {
    lexer.set_result(Token::FloatLiteral.symbol());
    lexer.advance();
    while is_num_char(lexer.lookahead()) {
        lexer.advance();
    }
    let mut has_fraction = false;
    let mut has_exponent = false;
    if is(lexer.lookahead(), '.') {
        has_fraction = true;
        lexer.advance();
        // A letter after the dot: `1.max(2)` is a method on an integer.
        if is_alpha(lexer.lookahead()) {
            return false;
        }
        if is(lexer.lookahead(), '.') {
            return false;
        }
        while is_num_char(lexer.lookahead()) {
            lexer.advance();
        }
    }
    lexer.mark_end();
    if is(lexer.lookahead(), 'e') || is(lexer.lookahead(), 'E') {
        has_exponent = true;
        lexer.advance();
        if is(lexer.lookahead(), '+') || is(lexer.lookahead(), '-') {
            lexer.advance();
        }
        if !is_num_char(lexer.lookahead()) {
            return true;
        }
        lexer.advance();
        while is_num_char(lexer.lookahead()) {
            lexer.advance();
        }
        lexer.mark_end();
    }
    if !has_exponent && !has_fraction {
        return false;
    }
    let c = lexer.lookahead();
    if !is(c, 'u') && !is(c, 'i') && !is(c, 'f') {
        return true;
    }
    lexer.advance();
    if !is_digit(lexer.lookahead()) {
        return true;
    }
    while is_digit(lexer.lookahead()) {
        lexer.advance();
    }
    lexer.mark_end();
    true
}

fn process_line_doc_content(lexer: &mut Lexer<'_>) -> bool {
    lexer.set_result(Token::LineDocContent.symbol());
    loop {
        if lexer.eof() {
            return true;
        }
        if is(lexer.lookahead(), '\n') {
            // The newline is part of the doc content, for the markdown
            // injection.
            lexer.advance();
            return true;
        }
        lexer.advance();
    }
}

/// Where a block comment's reading is, one character back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockCommentState {
    LeftForwardSlash,
    LeftAsterisk,
    Continuing,
}

struct BlockCommentProcessing {
    state: BlockCommentState,
    nesting_depth: u32,
}

fn process_left_forward_slash(processing: &mut BlockCommentProcessing, current: i32) {
    if is(current, '*') {
        processing.nesting_depth = processing.nesting_depth.saturating_add(1);
    }
    processing.state = BlockCommentState::Continuing;
}

fn process_left_asterisk(
    processing: &mut BlockCommentProcessing,
    current: i32,
    lexer: &mut Lexer<'_>,
) {
    if is(current, '*') {
        lexer.mark_end();
        processing.state = BlockCommentState::LeftAsterisk;
        return;
    }
    if is(current, '/') {
        processing.nesting_depth = processing.nesting_depth.saturating_sub(1);
    }
    processing.state = BlockCommentState::Continuing;
}

fn process_continuing(processing: &mut BlockCommentProcessing, current: i32) {
    if is(current, '/') {
        processing.state = BlockCommentState::LeftForwardSlash;
    } else if is(current, '*') {
        processing.state = BlockCommentState::LeftAsterisk;
    }
}

fn process_block_comment(lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
    // The first character, kept so the branches below may each advance
    // once and still know what they started from.
    let first = lexer.lookahead();
    if valid(Token::BlockInnerDocMarker) && is(first, '!') {
        lexer.set_result(Token::BlockInnerDocMarker.symbol());
        lexer.advance();
        return true;
    }
    if valid(Token::BlockOuterDocMarker) && is(first, '*') {
        lexer.advance();
        lexer.mark_end();
        // `/**/` is an empty comment, not a doc comment.
        if is(lexer.lookahead(), '/') {
            return false;
        }
        // Three stars or more are not a doc comment's two.
        if !is(lexer.lookahead(), '*') {
            lexer.set_result(Token::BlockOuterDocMarker.symbol());
            return true;
        }
    } else {
        lexer.advance();
    }

    if valid(Token::BlockCommentContent) {
        let mut processing = BlockCommentProcessing {
            state: BlockCommentState::Continuing,
            nesting_depth: 1,
        };
        if is(first, '*') {
            processing.state = BlockCommentState::LeftAsterisk;
            // `/*!*/`: an empty doc comment has no content.
            if is(lexer.lookahead(), '/') {
                return false;
            }
        } else if is(first, '/') {
            processing.state = BlockCommentState::LeftForwardSlash;
        }
        // An unterminated comment runs to the end rather than failing, so
        // code being typed before its `*/` still highlights as a comment.
        while !lexer.eof() && processing.nesting_depth != 0 {
            let current = lexer.lookahead();
            match processing.state {
                BlockCommentState::LeftForwardSlash => {
                    process_left_forward_slash(&mut processing, current)
                }
                BlockCommentState::LeftAsterisk => {
                    process_left_asterisk(&mut processing, current, lexer)
                }
                BlockCommentState::Continuing => {
                    lexer.mark_end();
                    process_continuing(&mut processing, current);
                }
            }
            lexer.advance();
            if is(current, '/') && processing.nesting_depth != 0 {
                lexer.mark_end();
            }
        }
        lexer.set_result(Token::BlockCommentContent.symbol());
        return true;
    }
    false
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "string_content",
        "string_close",
        "_raw_string_literal_start",
        "raw_string_literal_content",
        "_raw_string_literal_end",
        "float_literal",
        "_outer_block_doc_comment_marker",
        "_inner_block_doc_comment_marker",
        "_block_comment_content",
        "_line_doc_content",
        "_error_sentinel",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // During error recovery the runtime offers every token; nothing here
        // can help it, so decline (the grammar's error sentinel says when).
        if valid(Token::ErrorSentinel) {
            return false;
        }
        if valid(Token::BlockCommentContent)
            || valid(Token::BlockInnerDocMarker)
            || valid(Token::BlockOuterDocMarker)
        {
            return process_block_comment(lexer, &valid);
        }
        if valid(Token::StringContent) && !valid(Token::FloatLiteral) && process_string(lexer) {
            return true;
        }
        // No content: the next character is `"` or `\`, and a `"` closes.
        if valid(Token::StringClose) && is(lexer.lookahead(), '"') {
            lexer.advance();
            lexer.set_result(Token::StringClose.symbol());
            lexer.mark_end();
            return true;
        }
        if valid(Token::LineDocContent) {
            return process_line_doc_content(lexer);
        }
        while is_space(lexer.lookahead()) {
            lexer.skip();
        }
        let c = lexer.lookahead();
        if valid(Token::RawStringLiteralStart) && (is(c, 'r') || is(c, 'b') || is(c, 'c')) {
            return scan_raw_string_start(self, lexer);
        }
        if valid(Token::RawStringLiteralContent) {
            return scan_raw_string_content(self, lexer);
        }
        if valid(Token::RawStringLiteralEnd) && is(c, '"') {
            return scan_raw_string_end(self, lexer);
        }
        if valid(Token::FloatLiteral) && is_digit(c) {
            return process_float_literal(lexer);
        }
        false
    }

    fn serialize(&self, buffer: &mut [u8]) -> usize {
        match buffer.first_mut() {
            Some(first) => {
                *first = self.opening_hash_count;
                1
            }
            None => 0,
        }
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.opening_hash_count = match bytes {
            [count] => *count,
            _ => 0,
        };
    }
}
