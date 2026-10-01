//! SQL: tree-sitter-sql 0.3.11 (MIT, Derek Stride and contributors), the
//! grammar of `tree-sitter-sequel` -- a broad SQL, PostgreSQL's, MySQL's and
//! SQLite's dialects in one. `grammars/sql/` holds its `parser.c`, its
//! highlight query, its test corpus and highlight tests, and its `scanner.c`
//! as published; the scanner is ported below. It reads PostgreSQL's
//! dollar-quoted strings -- `$$ ... $$`, `$body$ ... $body$` -- and keeps the
//! tag of a function body's, open until its end tag, with the parse's state.
//!
//! # Where the port differs
//!
//! - **A tag's name does not begin with a digit**: the C took anything but
//!   a blank or a `$` between a tag's two `$`s, so `$1,$` (in
//!   `SELECT $1,$2`) began a string wherever another `$1,$` came later --
//!   `SELECT $1,$2; SELECT $1,$3;` was one statement with a string in it --
//!   and each such beginning read on to the end of the text looking for its
//!   close. PostgreSQL's own rule is narrower still (a name is an
//!   identifier's letters, digits and `_`), but the grammar's tests take
//!   `$.$` for a tag, so the rest of the C's rule stands.
//! - **A blank is ASCII's**: `\t` to `\r` and the space, the grammar's own
//!   lexer's and PostgreSQL's. The C asked the C library's `iswspace`,
//!   which on some systems takes other spaces for blanks too -- where
//!   PostgreSQL takes a no-break space as part of a name.
//! - **A string ends at the first place its tag is**: the C, reading
//!   `$a$...` for its end, took a whole other tag at a time, so in `$x$a$`
//!   it read `$x$` and missed the `$a$` sharing its second `$`.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("sql", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/sql/highlights.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    StartTag,
    EndTag,
    String,
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

/// `c` as a character, if it is one.
fn char_of(c: i32) -> Option<char> {
    u32::try_from(c).ok().and_then(char::from_u32)
}

/// Whether `c` is a blank: `\t` to `\r`, or the space.
fn is_blank(c: i32) -> bool {
    char_of(c).is_some_and(|c| matches!(c, '\t'..='\r' | ' '))
}

/// Pass over blanks, not part of the token.
fn skip_blanks(lexer: &mut Lexer<'_>) {
    while is_blank(lexer.lookahead()) {
        lexer.skip();
    }
}

/// A tag, `$name$` or `$$`, taken -- none if what is here is not one. A
/// name is anything but a blank or a `$`, not a digit first.
fn scan_tag(lexer: &mut Lexer<'_>) -> Option<String> {
    if !is(lexer.lookahead(), '$') {
        return None;
    }
    lexer.advance();
    let mut tag = String::from("$");
    while !lexer.eof() && !is(lexer.lookahead(), '$') && !is_blank(lexer.lookahead()) {
        // Text that is not UTF-8 is no tag's.
        let c = char_of(lexer.lookahead())?;
        // `$1` is a parameter, not the start of a tag.
        if tag.len() == 1 && c.is_ascii_digit() {
            return None;
        }
        tag.push(c);
        lexer.advance();
    }
    if !is(lexer.lookahead(), '$') {
        return None;
    }
    lexer.advance();
    tag.push('$');
    Some(tag)
}

/// Take `tag` if it comes next -- as much of it as does if it does not,
/// leaving the lexer at the first character that differs.
fn take_tag(lexer: &mut Lexer<'_>, tag: &str) -> bool {
    for c in tag.chars() {
        if lexer.eof() || !is(lexer.lookahead(), c) {
            return false;
        }
        lexer.advance();
    }
    true
}

/// The scanner's state: the tag of the function body open where it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    open: Option<String>,
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_dollar_quoted_string_start_tag",
        "_dollar_quoted_string_end_tag",
        "_dollar_quoted_string",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::StartTag) && self.open.is_none() {
            skip_blanks(lexer);
            let Some(tag) = scan_tag(lexer) else {
                return false;
            };
            self.open = Some(tag);
            lexer.set_result(Token::StartTag.symbol());
            return true;
        }
        if valid(Token::EndTag)
            && let Some(open) = self.open.as_deref()
        {
            skip_blanks(lexer);
            if take_tag(lexer, open) {
                self.open = None;
                lexer.set_result(Token::EndTag.symbol());
                return true;
            }
            return false;
        }
        if valid(Token::String) {
            lexer.mark_end();
            skip_blanks(lexer);
            let Some(tag) = scan_tag(lexer) else {
                return false;
            };
            // The open body's own tag here is its end, not a string.
            if self.open.as_deref() == Some(tag.as_str()) {
                return false;
            }
            while !lexer.eof() {
                if is(lexer.lookahead(), '$') {
                    if take_tag(lexer, &tag) {
                        lexer.mark_end();
                        lexer.set_result(Token::String.symbol());
                        return true;
                    }
                    // What differed may begin the tag itself: look again
                    // from there, without taking it.
                } else {
                    lexer.advance();
                }
            }
            return false;
        }
        false
    }

    /// The open tag's bytes; nothing when none is open.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let Some(tag) = self.open.as_deref() else {
            return 0;
        };
        match buffer.get_mut(..tag.len()) {
            Some(out) => {
                out.copy_from_slice(tag.as_bytes());
                tag.len()
            }
            // Past the buffer: saved as none open, as the C saves it.
            None => 0,
        }
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.open = core::str::from_utf8(bytes)
            .ok()
            .filter(|tag| tag.len() >= 2)
            .map(str::to_owned);
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

    /// **An open tag comes back**; none open is nothing saved.
    #[test]
    fn an_open_tag_comes_back() {
        let scanner = Scanner {
            open: Some("$body$".into()),
        };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        let mut back = Scanner::default();
        back.deserialize(buffer.get(..size).unwrap());
        assert_eq!(back, scanner);
        assert_eq!(Scanner::default().serialize(&mut buffer), 0);
        back.deserialize(&[]);
        assert_eq!(back, Scanner::default());
    }

    /// **A tag's name does not begin with a digit**: `$1` is a parameter,
    /// not a tag, so two statements using the same parameters are two
    /// statements. Past its first character, a name is anything but a blank
    /// or a `$`.
    #[test]
    fn a_tag_does_not_begin_with_a_digit() {
        let tree = sexp("SELECT $1,$2; SELECT $1,$3;\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert_eq!(tree.matches("(statement").count(), 2, "{tree}");
        for text in [
            "SELECT $tag$ it's $1 $tag$;\n",
            "SELECT $t1$ x $t1$;\n",
            "SELECT $.$ x $.$;\n",
        ] {
            let tree = sexp(text);
            assert!(!tree.contains("ERROR"), "{text:?}: {tree}");
            assert!(tree.contains("(literal)"), "{text:?}: {tree}");
        }
    }

    /// **What is left open ends at the end of the text**: a tag or a string
    /// not closed there is an error, not a scan that reads past the end.
    #[test]
    fn what_is_left_open_ends_at_the_end_of_the_text() {
        for text in ["SELECT $abc", "SELECT $$ abc", "SELECT $a$ x $a"] {
            let tree = sexp(text);
            assert!(tree.contains("ERROR"), "{text:?}: {tree}");
        }
    }

    /// **A blank is ASCII's**: a vertical tab is passed over before a tag,
    /// and a name ends at one, where a no-break space is part of a name.
    #[test]
    fn a_blank_is_asciis() {
        for text in ["SELECT\u{b}$$ x $$;\n", "SELECT $a\u{a0}b$ x $a\u{a0}b$;\n"] {
            let tree = sexp(text);
            assert!(!tree.contains("ERROR"), "{text:?}: {tree}");
            assert!(tree.contains("(literal)"), "{text:?}: {tree}");
        }
        for text in ["SELECT $ a$ x $ a$;\n", "SELECT $a\u{b}b$ x $a\u{b}b$;\n"] {
            let tree = sexp(text);
            assert!(tree.contains("ERROR"), "{text:?}: {tree}");
        }
    }

    /// **A string ends at the first place its tag is**, even where that
    /// shares a `$` with something tag-like before it.
    #[test]
    fn a_string_ends_at_the_first_place_its_tag_is() {
        let tree = sexp("SELECT $a$ x $x$a$;\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        let tree = sexp("SELECT $a$ x $$a$;\n");
        assert!(!tree.contains("ERROR"), "{tree}");
    }
}
