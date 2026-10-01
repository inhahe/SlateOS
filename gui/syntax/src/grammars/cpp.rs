//! C++: tree-sitter-cpp 0.23.4 (MIT, Max Brunsfeld and the tree-sitter
//! contributors), which builds on C's grammar. `grammars/cpp/` holds its
//! `parser.c`, its queries, its test corpus -- C's own among it, which C++
//! must parse too -- and highlight tests, and its `scanner.c`, as published;
//! the scanner is ported below. It reads the one thing a state machine
//! cannot, a raw string, `R"delim(...)delim"`, whose contents run to a `)`,
//! the same delimiter and a `"`; so it keeps the delimiter while inside one.
//!
//! # Where the port differs
//!
//! - **A delimiter may be sixteen characters long**, as C++ allows: the C
//!   checked the length before looking for the `(`, and so refused every
//!   sixteen-character delimiter.
//! - **A saved delimiter is its characters, four bytes each**: the C copied
//!   its `wchar_t`s, two bytes each on Windows and four elsewhere, so a
//!   state saved on one could not be read on the other -- nor, on Windows, a
//!   character past U+FFFF, cut to its low half. (C++ delimiters are ASCII;
//!   a file may still hold anything.)

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("cpp", super::Scanner);

/// The highlight queries: C's -- the one vendored beside C's grammar, the
/// same text as the 0.23.1 query this package names -- then C++'s, as
/// `tree-sitter.json` lists them.
pub(crate) const HIGHLIGHTS: &str = concat!(
    include_str!("../../grammars/c/highlights.scm"),
    "\n",
    include_str!("../../grammars/cpp/highlights.scm"),
);

/// The injection query, as published: a raw string in the language its
/// delimiter names (`R"sql(...)sql"`).
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/cpp/injections.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    RawStringDelimiter,
    RawStringContent,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The longest raw string delimiter C++ allows.
const MAX_DELIMITER: usize = 16;

/// The scanner: the delimiter of the raw string it is inside, if any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    delimiter: Vec<i32>,
}

/// Whether `c` is one of ASCII's six blanks, as the C's `iswspace` in the C
/// locale -- none of which a delimiter may hold.
fn is_blank(c: i32) -> bool {
    c == 0x20 || (0x09..=0x0d).contains(&c)
}

impl Scanner {
    /// A raw string's delimiter: the opening one, recorded -- up to its
    /// `(`, and not empty, which the grammar's own rule reads -- or the
    /// closing one, which must be the same.
    fn scan_delimiter(&mut self, lexer: &mut Lexer<'_>) -> bool {
        if !self.delimiter.is_empty() {
            for &c in &self.delimiter {
                if lexer.lookahead() != c {
                    return false;
                }
                lexer.advance();
            }
            self.delimiter.clear();
            return true;
        }
        loop {
            let c = lexer.lookahead();
            if c == i32::from(b'(') {
                return !self.delimiter.is_empty();
            }
            if self.delimiter.len() >= MAX_DELIMITER
                || lexer.eof()
                || c == i32::from(b'\\')
                || is_blank(c)
            {
                return false;
            }
            self.delimiter.push(c);
            lexer.advance();
        }
    }

    /// A raw string's contents: everything up to `)`, the delimiter and `"`
    /// -- or to the end of the text, which leaves the string unfinished.
    fn scan_content(&self, lexer: &mut Lexer<'_>) -> bool {
        // How much of `)delimiter` has been matched since the last `)`: the
        // delimiter holds no `)`, so one count is enough.
        let mut matched: Option<usize> = None;
        loop {
            if lexer.eof() {
                lexer.mark_end();
                return true;
            }
            let c = lexer.lookahead();
            if let Some(at) = matched {
                matched = if at == self.delimiter.len() {
                    if c == i32::from(b'"') {
                        return true;
                    }
                    None
                } else if self.delimiter.get(at) == Some(&c) {
                    Some(at.saturating_add(1))
                } else {
                    None
                };
            }
            if matched.is_none() && c == i32::from(b')') {
                // The contents end before `)delimiter"`, which is read
                // through to be sure of it.
                lexer.mark_end();
                matched = Some(0);
            }
            lexer.advance();
        }
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &["raw_string_delimiter", "raw_string_content"];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // Both at once is error recovery, which this leaves to the parser.
        if valid(Token::RawStringDelimiter) && valid(Token::RawStringContent) {
            return false;
        }
        // No blanks are skipped: a raw string's delimiter and contents are
        // exactly what the text holds.
        if valid(Token::RawStringDelimiter) {
            lexer.set_result(Token::RawStringDelimiter.symbol());
            return self.scan_delimiter(lexer);
        }
        if valid(Token::RawStringContent) {
            lexer.set_result(Token::RawStringContent.symbol());
            return self.scan_content(lexer);
        }
        false
    }

    /// The delimiter's characters, each as a little-endian `u32`.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let mut size: usize = 0;
        for &c in &self.delimiter {
            let end = size.saturating_add(4);
            let Some(out) = buffer.get_mut(size..end) else {
                break;
            };
            out.copy_from_slice(&c.to_le_bytes());
            size = end;
        }
        size
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.delimiter = bytes
            .chunks_exact(4)
            .take(MAX_DELIMITER)
            .filter_map(|b| <[u8; 4]>::try_from(b).ok())
            .map(i32::from_le_bytes)
            .collect();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a parse that fails, or a slice out of range, is the failure"
)]
mod tests {
    use super::*;
    use crate::ffi::SERIALIZATION_BUFFER_SIZE;

    /// `text`'s tree.
    fn tree(text: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap()
    }

    /// The text of the first node of `kind` in `tree`.
    fn first<'t>(tree: &tree_sitter::Tree, text: &'t str, kind: &str) -> Option<&'t str> {
        let mut cursor = tree.walk();
        loop {
            if cursor.node().kind() == kind {
                return Some(&text[cursor.node().byte_range()]);
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return None;
                }
            }
        }
    }

    /// **A delimiter may be sixteen characters long**, as C++ allows -- the
    /// C refused sixteen -- but not seventeen.
    #[test]
    fn a_delimiter_may_be_sixteen_characters_long() {
        let sixteen = "0123456789abcdef";
        let text = format!("auto s = R\"{sixteen}(a)b){sixteen}\";\n");
        let parsed = tree(&text);
        assert_eq!(
            first(&parsed, &text, "raw_string_content"),
            Some("a)b"),
            "{}",
            parsed.root_node().to_sexp()
        );
        assert!(
            !parsed.root_node().has_error(),
            "{}",
            parsed.root_node().to_sexp()
        );
        let seventeen = format!("{sixteen}g");
        let text = format!("auto s = R\"{seventeen}(a){seventeen}\";\n");
        assert!(tree(&text).root_node().has_error());
    }

    /// **A raw string's contents run to its own delimiter**: a `)` and a
    /// `"` inside are contents, and so is `)x` where the delimiter is `xy`.
    #[test]
    fn a_raw_strings_contents_run_to_its_own_delimiter() {
        let text = "auto s = R\"xy(a)\" )x\" b)xy\";\n";
        let tree = tree(text);
        assert_eq!(
            first(&tree, text, "raw_string_content"),
            Some("a)\" )x\" b")
        );
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
    }

    /// **A saved state comes back whole**, delimiter and all -- a character
    /// past U+FFFF included, which the C's two-byte `wchar_t` on Windows cut
    /// -- and an empty one is no raw string.
    #[test]
    fn a_saved_state_comes_back_whole() {
        let scanner = Scanner {
            delimiter: vec![i32::from(b'x'), 0x1_F600, i32::from(b'y')],
        };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        assert_eq!(size, 12);
        let mut back = Scanner::default();
        back.deserialize(&buffer[..size]);
        assert_eq!(back, scanner);
        back.deserialize(&[]);
        assert_eq!(back, Scanner::default());
    }

    /// **A delimiter holds no blank or backslash, and two raw strings each
    /// close at their own**: `R"a b(x)a b"` is no raw string, and after one
    /// raw string closes the next opens afresh.
    #[test]
    fn a_delimiter_holds_no_blank_and_each_string_closes_its_own() {
        assert!(tree("auto s = R\"a b(x)a b\";\n").root_node().has_error());
        assert!(tree("auto s = R\"a\\b(x)a\\b\";\n").root_node().has_error());
        let text = "auto s = R\"x(1)x\";\nauto t = R\"y(2)y\";\n";
        let parsed = tree(text);
        assert!(
            !parsed.root_node().has_error(),
            "{}",
            parsed.root_node().to_sexp()
        );
        assert_eq!(
            parsed
                .root_node()
                .to_sexp()
                .matches("raw_string_literal")
                .count(),
            2
        );
    }

    /// **Error recovery reads no delimiter out of ordinary code**: past a
    /// character the grammar has no token for, `@bar(2)` is still a call --
    /// not a raw string's delimiter `@bar` and a stray `2` -- because the
    /// scanner declines when every token is allowed, which is how the
    /// runtime asks while it recovers.
    #[test]
    fn error_recovery_reads_no_delimiter() {
        for text in ["int x = @bar(2);\n", "int x = `baz(3);\n"] {
            let sexp = tree(text).root_node().to_sexp();
            assert!(!sexp.contains("raw_string_delimiter"), "{text:?}: {sexp}");
            assert!(sexp.contains("call_expression"), "{text:?}: {sexp}");
        }
    }

    /// **An unfinished raw string ends at the end of the text** -- its
    /// contents run there, and no closing delimiter is read out of nothing.
    #[test]
    fn an_unfinished_raw_string_ends_at_the_end() {
        let text = "auto s = R\"xy(abc";
        let parsed = tree(text);
        assert_eq!(first(&parsed, text, "raw_string_content"), Some("abc"));
        let sexp = parsed.root_node().to_sexp();
        assert!(parsed.root_node().has_error(), "{sexp}");
        assert_eq!(sexp.matches("(raw_string_delimiter)").count(), 1, "{sexp}");
    }
}
