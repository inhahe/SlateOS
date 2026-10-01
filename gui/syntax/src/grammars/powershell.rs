//! PowerShell: tree-sitter-powershell 0.26.4 (MIT, Airbus CERT, from
//! Microsoft's grammar). `grammars/powershell/` holds its `parser.c`, its
//! highlight query, its test corpus and its `scanner.c` as published; the
//! scanner is ported below. It says where a statement ends: its token is
//! empty, found by looking ahead past blanks to a line's end, a `;`, a `}`,
//! a `)` or the end of the text.
//!
//! # Where the port differs
//!
//! - **A blank is Unicode's**: the C asked the C library's `iswspace`,
//!   which on some systems knows only ASCII's -- and PowerShell's own
//!   tokenizer takes a no-break space, and every other Unicode space, as a
//!   blank.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("powershell", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/powershell/highlights.scm");

/// The one external token: a statement's end.
const STATEMENT_TERMINATOR: u16 = 0;

/// The scanner: it keeps nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner;

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &["_statement_terminator"];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        if !valid_symbols.first().copied().unwrap_or(false) {
            return false;
        }
        lexer.set_result(STATEMENT_TERMINATOR);
        // Empty: the token is where the scan starts, and the rest is a look
        // ahead at what ends a statement.
        lexer.mark_end();
        loop {
            let c = lexer.lookahead();
            if c == 0 || [')', ';', '}', '\n'].iter().any(|&e| c == e as i32) {
                return true;
            }
            if !u32::try_from(c)
                .ok()
                .and_then(char::from_u32)
                .is_some_and(char::is_whitespace)
            {
                return false;
            }
            lexer.skip();
        }
    }

    fn serialize(&self, _buffer: &mut [u8]) -> usize {
        0
    }

    fn deserialize(&mut self, _bytes: &[u8]) {}
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test: a parse that fails is the failure"
)]
mod tests {
    use super::*;

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// **A statement ends at a line's end, a `;`, a `}` or the text's end,
    /// blanks before them passed over -- Unicode's too.**
    #[test]
    fn a_statement_ends_where_powershell_ends_one() {
        for text in [
            "$a = 1\n$b = 2\n",
            "$a = 1; $b = 2",
            "if ($a) { $b = 2 }",
            "$a = 1 \t\u{a0}\n$b = 2",
        ] {
            let tree = sexp(text);
            assert!(!tree.contains("ERROR"), "{text:?}: {tree}");
        }
        assert_eq!(Scanner.serialize(&mut [0; 4]), 0);
    }
}
