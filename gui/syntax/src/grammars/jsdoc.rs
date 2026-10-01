//! JSDoc: tree-sitter-jsdoc 0.25.0 (MIT, Max Brunsfeld and the tree-sitter
//! contributors) -- the documentation comments of JavaScript and
//! TypeScript, which their injection queries hand to it, every comment
//! but a `//` one being parsed as one. `grammars/jsdoc/` holds its
//! `parser.c`, its highlight query, its test corpus and its `scanner.c` as
//! published; the scanner is ported below. It reads a tag's `{type}` --
//! everything up to the `}` that balances the `{`, braces inside included --
//! and keeps no state. Its second token, a code block's line, it leaves to
//! the grammar's own lexer, which the grammar defines it for too.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("jsdoc", super::Scanner);

/// The highlight query, as published: a tag's name a keyword, its type a
/// type.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/jsdoc/highlights.scm");

/// The external tokens, in the grammar's order: the scanner reads the
/// first; the grammar's lexer, the second (`code_block_line`).
#[derive(Clone, Copy)]
enum Token {
    Type,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The scanner, which keeps nothing between tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner;

/// A `{type}`'s text, its `{` taken: up to the `}` that balances it, on
/// the same line.
fn scan_for_type(lexer: &mut Lexer<'_>) -> bool {
    // How many `{`s inside the type are open.
    let mut open: u32 = 0;
    loop {
        if lexer.eof() {
            return false;
        }
        let c = lexer.lookahead();
        if c == i32::from(b'{') {
            open = open.saturating_add(1);
        } else if c == i32::from(b'}') {
            let Some(left) = open.checked_sub(1) else {
                return true;
            };
            open = left;
        } else if c == i32::from(b'\n') || c == 0 {
            // A type does not run on past its line.
            return false;
        }
        lexer.advance();
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &["type", "code_block_line"];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::Type) && scan_for_type(lexer) {
            lexer.set_result(Token::Type.symbol());
            lexer.mark_end();
            return true;
        }
        false
    }

    fn serialize(&self, _buffer: &mut [u8]) -> usize {
        0
    }

    fn deserialize(&mut self, _bytes: &[u8]) {}
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a parse that fails, or a slice out of range, is the failure"
)]
mod tests {
    use super::*;

    /// Every `type` node's text in `text`'s tree.
    fn types(text: &str) -> Vec<String> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        let mut out = Vec::new();
        let mut cursor = tree.walk();
        loop {
            if cursor.node().kind() == "type" {
                out.push(text[cursor.node().byte_range()].to_owned());
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return out;
                }
            }
        }
    }

    /// **A type does not run on past its line**: `{string` left open is no
    /// type, however far the next `}` is.
    #[test]
    fn a_type_does_not_run_past_its_line() {
        let found = types("/**\n * @param {string\n * more} x\n */");
        assert!(found.iter().all(|t| !t.contains('\n')), "{found:?}");
        assert_eq!(
            types("/**\n * @param {Map<string, {a: number}>} x\n */"),
            ["Map<string, {a: number}>"]
        );
    }

    /// **A type is read only where the grammar allows one**: a code block's
    /// line that ends a function's body, `}`, is code, not a type.
    #[test]
    fn a_type_is_read_only_where_one_is_allowed() {
        let text =
            "/**\n * Example:\n * ```js\n * function f() {\n *   return 1;\n * }\n * ```\n */";
        assert_eq!(types(text), Vec::<String>::new());
    }
}
