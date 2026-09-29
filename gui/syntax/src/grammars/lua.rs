//! Lua: tree-sitter-lua 0.5.0 (MIT, Munif Tanjim and the tree-sitter-grammars
//! contributors). `grammars/lua/` holds its `parser.c`, its highlight,
//! locals and injection queries, its test corpus and highlight test, and its
//! `scanner.c` as published; the scanner is ported below. It reads Lua's long
//! brackets -- a string `[==[ ... ]==]` and a comment `--[[ ... ]]` -- whose
//! closing bracket must have as many `=` as the opening one, a count the
//! scanner keeps with the parse's state.
//!
//! # Where the port differs
//!
//! - **A long bracket's level is kept whole**: the C counted `=` in a byte,
//!   so a bracket with 256 of them was level 0 and closed at `]]`.
//! - **A string's text runs to its end, a NUL byte included**: the C took a
//!   NUL character for the end of the text and stopped.
//! - **Blanks before a long bracket are Lua's six**: space, tab, line feed,
//!   carriage return, form feed and vertical tab; the C asked the C library,
//!   which on some systems counts more.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("lua", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/lua/highlights.scm");

/// The locals query, as published: where names are declared and used.
pub(crate) const LOCALS: &str = include_str!("../../grammars/lua/locals.scm");

/// The injection query, as published: the C declarations handed to LuaJIT's
/// `ffi.cdef`, in C.
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/lua/injections.scm");

/// The external tokens, in the grammar's order: a long bracket's three
/// parts, a comment's and a string's.
#[derive(Clone, Copy)]
enum Token {
    CommentStart,
    CommentContent,
    CommentEnd,
    StringStart,
    StringContent,
    StringEnd,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// Whether `c` is one of Lua's blanks.
fn is_blank(c: i32) -> bool {
    [' ', '\t', '\n', '\r', '\u{c}', '\u{b}']
        .into_iter()
        .any(|b| is(c, b))
}

/// The scanner's state: the level -- how many `=` -- of the long bracket
/// open where it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    level: u32,
}

/// How many `ch` come next, taken.
fn count(lexer: &mut Lexer<'_>, ch: char) -> u32 {
    let mut n: u32 = 0;
    while !lexer.eof() && is(lexer.lookahead(), ch) {
        n = n.saturating_add(1);
        lexer.advance();
    }
    n
}

/// Take `ch` if it comes next.
fn take(lexer: &mut Lexer<'_>, ch: char) -> bool {
    if lexer.eof() || !is(lexer.lookahead(), ch) {
        return false;
    }
    lexer.advance();
    true
}

impl Scanner {
    /// An opening long bracket, `[`, `=`s, `[`: its level kept.
    fn scan_start(&mut self, lexer: &mut Lexer<'_>) -> bool {
        if !take(lexer, '[') {
            return false;
        }
        let level = count(lexer, '=');
        if !take(lexer, '[') {
            return false;
        }
        self.level = level;
        true
    }

    /// A closing long bracket of the open one's level.
    fn scan_end(&self, lexer: &mut Lexer<'_>) -> bool {
        take(lexer, ']') && count(lexer, '=') == self.level && take(lexer, ']')
    }

    /// A long bracket's text: everything up to its closing bracket, which
    /// is not taken -- none if the text ends first.
    fn scan_content(&self, lexer: &mut Lexer<'_>) -> bool {
        while !lexer.eof() {
            if is(lexer.lookahead(), ']') {
                lexer.mark_end();
                if self.scan_end(lexer) {
                    return true;
                }
            } else {
                lexer.advance();
            }
        }
        false
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_block_comment_start",
        "_block_comment_content",
        "_block_comment_end",
        "_block_string_start",
        "_block_string_content",
        "_block_string_end",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::StringEnd) && self.scan_end(lexer) {
            self.level = 0;
            lexer.set_result(Token::StringEnd.symbol());
            return true;
        }
        if valid(Token::StringContent) && self.scan_content(lexer) {
            lexer.set_result(Token::StringContent.symbol());
            return true;
        }
        if valid(Token::CommentEnd) && self.scan_end(lexer) {
            self.level = 0;
            lexer.set_result(Token::CommentEnd.symbol());
            return true;
        }
        if valid(Token::CommentContent) && self.scan_content(lexer) {
            lexer.set_result(Token::CommentContent.symbol());
            return true;
        }
        while is_blank(lexer.lookahead()) {
            lexer.skip();
        }
        if valid(Token::StringStart) && self.scan_start(lexer) {
            lexer.set_result(Token::StringStart.symbol());
            return true;
        }
        if valid(Token::CommentStart) && take(lexer, '-') && take(lexer, '-') {
            // `--` alone is a line comment, the grammar's: the token ends
            // here unless a long bracket follows.
            lexer.mark_end();
            if self.scan_start(lexer) {
                lexer.mark_end();
                lexer.set_result(Token::CommentStart.symbol());
                return true;
            }
        }
        false
    }

    /// The level, a little-endian `u32`.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let bytes = self.level.to_le_bytes();
        match buffer.get_mut(..bytes.len()) {
            Some(out) => {
                out.copy_from_slice(&bytes);
                bytes.len()
            }
            None => 0,
        }
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.level = bytes
            .get(..4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, u32::from_le_bytes);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test: a parse that fails is the failure"
)]
mod tests {
    use super::*;
    use crate::ffi::SERIALIZATION_BUFFER_SIZE;

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// **A saved level comes back**, past 255 as well, and nothing is
    /// level 0.
    #[test]
    fn a_saved_level_comes_back() {
        for level in [0, 3, 300] {
            let scanner = Scanner { level };
            let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
            let size = scanner.serialize(&mut buffer);
            let mut back = Scanner { level: 7 };
            back.deserialize(buffer.get(..size).unwrap());
            assert_eq!(back, scanner);
        }
        let mut back = Scanner { level: 7 };
        back.deserialize(&[]);
        assert_eq!(back, Scanner::default());
    }

    /// **A long bracket closes only at its own level**: `]]` inside a
    /// level-2 string is its text; so is `]=]`; and a level past 255 is
    /// still its own.
    #[test]
    fn a_long_bracket_closes_only_at_its_own_level() {
        let tree = sexp("s = [==[ a ]] b ]=] c ]==]\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert_eq!(tree.matches("(string_content)").count(), 1, "{tree}");
        let many = "=".repeat(256);
        let text = format!("s = [{many}[ ]] ]{many}]\n");
        let tree = sexp(&text);
        assert!(!tree.contains("ERROR"), "{tree}");
        let tree = sexp("s = [[]]\n");
        assert!(!tree.contains("ERROR"), "{tree}");
    }

    /// **A block comment is a long bracket after `--`; `--` alone is a line
    /// comment**, and a string's text may hold a NUL.
    #[test]
    fn a_block_comment_follows_two_dashes() {
        let tree = sexp("--[=[ a\n]] still ]=] x = 1\n-- line [[\ny = 2\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert_eq!(tree.matches("(comment ").count(), 2, "{tree}");
        assert_eq!(tree.matches("(assignment_statement").count(), 2, "{tree}");
        let tree = sexp("s = [[a\0b]]\n");
        assert!(!tree.contains("ERROR"), "{tree}");
    }
}
