//! TOML: tree-sitter-toml 0.7.0 (MIT, Ika and the tree-sitter-grammars
//! contributors; published on crates.io as `tree-sitter-toml-ng`).
//! `grammars/toml/` holds its `parser.c`, its highlight query, its test
//! corpus and its `scanner.c` as published; the scanner -- the end of a line
//! (or of the file) where a key-value pair must end, and the ends of the
//! triple-quoted strings -- is ported below as the C has it. It keeps no
//! state.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("toml", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/toml/highlights.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    LineEndingOrEof,
    MultilineBasicStringContent,
    MultilineBasicStringEnd,
    MultilineLiteralStringContent,
    MultilineLiteralStringEnd,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The scanner, which keeps nothing between tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner;

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// At a `delimiter` inside a triple-quoted string: the content it continues,
/// or the string's end. Up to three delimiters close it; a fourth or fifth
/// before the three are content, as TOML allows (`"""a""""` ends in `a"`).
fn scan_multiline_string_end(
    lexer: &mut Lexer<'_>,
    valid: &dyn Fn(Token) -> bool,
    delimiter: char,
    content: Token,
    end: Token,
) -> bool {
    if !valid(end) || !is(lexer.lookahead(), delimiter) {
        return false;
    }
    lexer.advance();
    lexer.mark_end();
    if !is(lexer.lookahead(), delimiter) {
        lexer.set_result(content.symbol());
        return true;
    }
    lexer.advance();
    if !is(lexer.lookahead(), delimiter) {
        lexer.mark_end();
        lexer.set_result(content.symbol());
        return true;
    }
    lexer.advance();
    if !is(lexer.lookahead(), delimiter) {
        lexer.mark_end();
        lexer.set_result(end.symbol());
        return true;
    }
    lexer.set_result(content.symbol());
    true
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_line_ending_or_eof",
        "_multiline_basic_string_content",
        "_multiline_basic_string_end",
        "_multiline_literal_string_content",
        "_multiline_literal_string_end",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if scan_multiline_string_end(
            lexer,
            &valid,
            '"',
            Token::MultilineBasicStringContent,
            Token::MultilineBasicStringEnd,
        ) || scan_multiline_string_end(
            lexer,
            &valid,
            '\'',
            Token::MultilineLiteralStringContent,
            Token::MultilineLiteralStringEnd,
        ) {
            return true;
        }
        if valid(Token::LineEndingOrEof) {
            lexer.set_result(Token::LineEndingOrEof.symbol());
            while is(lexer.lookahead(), ' ') || is(lexer.lookahead(), '\t') {
                lexer.skip();
            }
            if lexer.lookahead() == 0 || is(lexer.lookahead(), '\n') {
                return true;
            }
            if is(lexer.lookahead(), '\r') {
                lexer.skip();
                if is(lexer.lookahead(), '\n') {
                    return true;
                }
            }
        }
        false
    }

    fn serialize(&self, _buffer: &mut [u8]) -> usize {
        0
    }

    fn deserialize(&mut self, _bytes: &[u8]) {}
}
