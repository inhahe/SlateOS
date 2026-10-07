//! Nushell: tree-sitter-nu (MIT, the Nushell project) at commit `4f577aa`,
//! the grammar of SlateOS's default shell (`design.txt`: "Nushell as default
//! interactive shell"). `grammars/nu/` holds its `parser.c`, its highlight and
//! injection queries, its test corpus and its `scanner.c` as published; the
//! scanner is ported below. It reads a raw string -- `r#'...'#`, with as many
//! `#`s either side as the text needs -- as three tokens: the opening, the
//! text, and the closing.
//!
//! # Where the port differs
//!
//! Each follows Nushell's own lexer (`nu-parser`'s `lex_raw_string`), which
//! the C did not:
//!
//! - **A raw string opens with at least one `#`**: `r'text'` is the word `r`
//!   and a string, as Nushell reads it. The C took it for a raw string with
//!   no `#`s, which it then had no way to read to its end.
//! - **It closes at a `'` followed by its `#`s**: the C closed it at *any*
//!   character followed by the right number of `#`s, so `r#'a#b'#` ended at
//!   `a#` and the rest of it was an error.
//! - **Its `#`s close it, however many follow**: `'##` closes `r#'...` after
//!   the first `#`, as Nushell's lexer does, and the second begins a comment.
//!   The C counted every `#` there, found two where it wanted one, and read
//!   on to the end of the text.
//! - **Nothing wraps round**: the C counted the `#`s in a byte, so an opening
//!   of 256 `#`s was one of none. Past 255 it is not a raw string here.
//! - **No fourth flag**: the C first asked whether a fourth token, an error
//!   sentinel, was valid -- a token its grammar does not declare, so the flag
//!   it read was past the end of the array it was handed.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("nu", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/nu/highlights.scm");

/// The injection query, as published: regular expressions in `find -r` and
/// `=~`, and Nushell in `nu -c`. (Its comments name a `comment` language no
/// grammar here is, and are left as they are.)
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/nu/injections.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    Begin,
    Content,
    End,
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

/// Whether `c` is a blank: `\t` to `\r`, or the space -- what the C library's
/// `iswspace` answers in the C locale, which is the one the C ran in.
fn is_blank(c: i32) -> bool {
    u32::try_from(c)
        .ok()
        .and_then(char::from_u32)
        .is_some_and(|c| matches!(c, '\t'..='\r' | ' '))
}

/// The scanner's state: how many `#`s the raw string open here was opened
/// with; zero when none is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    level: u8,
}

impl Scanner {
    /// `r`, one or more `#`s, and `'`: a raw string opening, its `#`s kept.
    fn scan_begin(&mut self, lexer: &mut Lexer<'_>) -> bool {
        while is_blank(lexer.lookahead()) {
            lexer.skip();
        }
        if !is(lexer.lookahead(), 'r') {
            return false;
        }
        lexer.advance();
        let mut hashes: u16 = 0;
        while is(lexer.lookahead(), '#') && !lexer.eof() {
            lexer.advance();
            hashes = hashes.saturating_add(1);
        }
        let Ok(level) = u8::try_from(hashes) else {
            return false;
        };
        if level == 0 || !is(lexer.lookahead(), '\'') {
            return false;
        }
        lexer.advance();
        self.level = level;
        lexer.set_result(Token::Begin.symbol());
        true
    }

    /// Everything up to the first `'` followed by the opening's `#`s --
    /// empty, when that is what comes first.
    fn scan_content(&self, lexer: &mut Lexer<'_>) -> bool {
        loop {
            if lexer.eof() {
                return false;
            }
            if !is(lexer.lookahead(), '\'') {
                lexer.advance();
                continue;
            }
            // The text ends here if this closes it.
            lexer.mark_end();
            lexer.advance();
            let mut hashes: u8 = 0;
            while hashes < self.level && is(lexer.lookahead(), '#') && !lexer.eof() {
                lexer.advance();
                hashes = hashes.saturating_add(1);
            }
            if hashes == self.level {
                lexer.set_result(Token::Content.symbol());
                return true;
            }
            // A `'` with too few `#`s is text; the character that stopped
            // the count may begin the closing, so it is looked at again.
        }
    }

    /// The `'` and the opening's `#`s.
    fn scan_end(&mut self, lexer: &mut Lexer<'_>) -> bool {
        if !is(lexer.lookahead(), '\'') {
            return false;
        }
        lexer.advance();
        for _ in 0..self.level {
            if !is(lexer.lookahead(), '#') {
                return false;
            }
            lexer.advance();
        }
        self.level = 0;
        lexer.set_result(Token::End.symbol());
        true
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] =
        &["raw_string_begin", "raw_string_content", "raw_string_end"];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::Begin) && self.level == 0 {
            return self.scan_begin(lexer);
        }
        if valid(Token::Content) && self.level != 0 {
            return self.scan_content(lexer);
        }
        if valid(Token::End) && self.level != 0 {
            return self.scan_end(lexer);
        }
        false
    }

    /// The open string's `#`s, in one byte, as the C saves them.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        match buffer.first_mut() {
            Some(byte) => {
                *byte = self.level;
                1
            }
            None => 0,
        }
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.level = match bytes {
            [level] => *level,
            _ => 0,
        };
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

    /// **An open string's `#`s come back**; none open is saved as none.
    #[test]
    fn an_open_strings_hashes_come_back() {
        let scanner = Scanner { level: 3 };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        assert_eq!(size, 1);
        let mut back = Scanner::default();
        back.deserialize(buffer.get(..size).unwrap());
        assert_eq!(back, scanner);
        back.deserialize(&[]);
        assert_eq!(back, Scanner::default());
        assert_eq!(scanner.serialize(&mut []), 0);
    }

    /// **A raw string opens with at least one `#`**: `r'text'` is a word and
    /// a string.
    #[test]
    fn a_raw_string_opens_with_a_hash() {
        let tree = sexp("r#'text'#\n");
        assert!(tree.contains("(raw_string_begin)"), "{tree}");
        assert!(!tree.contains("ERROR"), "{tree}");
        let tree = sexp("r'text'\n");
        assert!(!tree.contains("raw_string"), "{tree}");
    }

    /// **It closes at a `'` followed by its `#`s**, not at any character that
    /// is: `a#` inside is text.
    #[test]
    fn it_closes_at_a_quote_and_its_hashes() {
        for text in ["r#'a#b'#\n", "r##'a'#b'##\n", "r#''#\n"] {
            let tree = sexp(text);
            assert!(!tree.contains("ERROR"), "{text:?}: {tree}");
            assert_eq!(
                tree.matches("(raw_string_end)").count(),
                1,
                "{text:?}: {tree}"
            );
        }
    }

    /// **Its `#`s close it, however many follow**: the rest is a comment, as
    /// Nushell reads it.
    #[test]
    fn its_hashes_close_it_however_many_follow() {
        let tree = sexp("r#'x'##\n");
        assert!(tree.contains("(raw_string_end)"), "{tree}");
        assert!(tree.contains("(comment)"), "{tree}");
        assert!(!tree.contains("ERROR"), "{tree}");
    }

    /// **Nothing wraps round**: 256 `#`s -- none, in a byte -- is not a raw
    /// string, nor is 257 -- one.
    #[test]
    fn nothing_wraps_round() {
        for hashes in [256, 257] {
            let open = format!("r{}'x'{}\n", "#".repeat(hashes), "#".repeat(hashes));
            assert!(!sexp(&open).contains("raw_string_begin"), "{hashes}");
        }
        let open = format!("r{}'x'{}\n", "#".repeat(255), "#".repeat(255));
        let tree = sexp(&open);
        assert!(tree.contains("(raw_string_end)"), "{tree}");
    }
}
