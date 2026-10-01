//! CSS: tree-sitter-css 0.25.0 (MIT, Max Brunsfeld and the tree-sitter
//! contributors). `grammars/css/` holds its `parser.c`, its highlight query,
//! its test corpus and its `scanner.c` as published; the scanner -- telling
//! the blank between two selectors (`nav a`, a descendant) from the blank
//! before a property, and a pseudo-class's colon (`a:hover {`) from a
//! property's (`color: red;`) -- is ported below. It keeps no state.
//!
//! # Where the port differs
//!
//! The C asks `iswspace` and `iswalnum`, whose answers depend on the C
//! library's locale. Here a blank is the C locale's six and a letter or digit
//! is Unicode's -- CSS names may be written in any script, and a selector
//! `nav é` is a descendant as much as `nav a` is.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("css", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/css/highlights.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    DescendantOp,
    PseudoClassSelectorColon,
    ErrorRecovery,
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

/// `iswspace` in the C locale: space, tab, newline, vertical tab, form
/// feed, carriage return.
fn is_space(c: i32) -> bool {
    c == 0x20 || (0x09..=0x0d).contains(&c)
}

/// A letter or digit, in any script.
fn is_alnum(c: i32) -> bool {
    u32::try_from(c)
        .ok()
        .and_then(char::from_u32)
        .is_some_and(char::is_alphanumeric)
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_descendant_operator",
        "_pseudo_class_selector_colon",
        "__error_recovery",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::ErrorRecovery) {
            return false;
        }

        if is_space(lexer.lookahead()) && valid(Token::DescendantOp) {
            lexer.set_result(Token::DescendantOp.symbol());
            lexer.skip();
            while is_space(lexer.lookahead()) {
                lexer.skip();
            }
            lexer.mark_end();
            let c = lexer.lookahead();
            if is(c, '#') || is(c, '.') || is(c, '[') || is(c, '-') || is(c, '*') || is_alnum(c) {
                return true;
            }
            if is(c, ':') {
                lexer.advance();
                if is_space(lexer.lookahead()) {
                    return false;
                }
                loop {
                    let c = lexer.lookahead();
                    if is(c, ';') || is(c, '}') || lexer.eof() {
                        return false;
                    }
                    if is(c, '{') {
                        return true;
                    }
                    lexer.advance();
                }
            }
        }

        if valid(Token::PseudoClassSelectorColon) {
            while is_space(lexer.lookahead()) {
                lexer.skip();
            }
            if is(lexer.lookahead(), ':') {
                lexer.advance();
                if is(lexer.lookahead(), ':') {
                    return false;
                }
                lexer.mark_end();
                lexer.set_result(Token::PseudoClassSelectorColon.symbol());
                // A `{` before a `;` or `}` makes a selector; a `;` a
                // property -- unless it is inside a comment.
                let mut in_comment = false;
                while !is(lexer.lookahead(), ';') && !is(lexer.lookahead(), '}') && !lexer.eof() {
                    lexer.advance();
                    let c = lexer.lookahead();
                    if is(c, '{') && !in_comment {
                        return true;
                    }
                    if is(c, '/') && !in_comment {
                        lexer.advance();
                        if is(lexer.lookahead(), '*') {
                            in_comment = true;
                        }
                    } else if is(c, '*') && in_comment {
                        lexer.advance();
                        if is(lexer.lookahead(), '/') {
                            in_comment = false;
                        }
                    }
                }
                // At the end with no `{` found it is still a selector's
                // colon: malformed code reads better as a broken selector
                // than as a broken property.
                return lexer.eof();
            }
        }

        false
    }

    fn serialize(&self, _buffer: &mut [u8]) -> usize {
        0
    }

    fn deserialize(&mut self, _bytes: &[u8]) {}
}
