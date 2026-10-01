//! Document type definitions: tree-sitter-xml 0.7.0's second grammar (MIT,
//! ObserverOfTime and Amaan Qureshi) -- a `.dtd` file on its own, as XML's
//! grammar reads one inside a `<!DOCTYPE ...[ ]>`. `grammars/dtd/` holds its
//! `parser.c`, its highlight query and highlight test, and its `scanner.c`
//! as published; its examples are in XML's corpus (`grammars/xml/corpus/`,
//! marked `:language(dtd)`). The scanner, ported below, is the part of
//! XML's the two share -- a processing instruction's target and text, and a
//! comment -- read by [`super::xml`]'s functions, and differs from the C as
//! that module's docs say.

use super::xml::{is, scan_comment, scan_pi_content, scan_pi_target};
use crate::ffi::{ExternalScanner, Lexer};

super::generated!("dtd", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/dtd/highlights.scm");

/// The external tokens, in the grammar's order: the first three of XML's.
#[derive(Clone, Copy)]
enum Token {
    PiTarget,
    PiContent,
    Comment,
}

/// The scanner: it keeps nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner;

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &["PITarget", "_pi_content", "Comment"];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // Error recovery asks for every token at once.
        if valid(Token::PiTarget) && valid(Token::PiContent) && valid(Token::Comment) {
            return false;
        }
        if valid(Token::PiTarget) {
            return scan_pi_target(lexer, |_| false);
        }
        if valid(Token::PiContent) {
            return scan_pi_content(lexer);
        }
        if valid(Token::Comment) {
            for c in ['<', '!'] {
                if lexer.eof() || !is(lexer.lookahead(), c) {
                    return false;
                }
                lexer.advance();
            }
            return scan_comment(lexer);
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

    /// **A definition's comments and instructions are its own tokens**:
    /// a comment between declarations, an instruction over two lines.
    #[test]
    fn a_definitions_comments_and_instructions_are_read() {
        let tree = sexp(
            "<!-- the note -->\n<!ELEMENT note (#PCDATA)>\n<?check\n all ?>\n<!ATTLIST note id ID #REQUIRED>\n",
        );
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(tree.contains("(Comment)"), "{tree}");
        assert!(tree.contains("(PI (PITarget)"), "{tree}");
        assert!(sexp("<!-- open\n<!ELEMENT a EMPTY>").contains("ERROR"));
        assert_eq!(Scanner.serialize(&mut [0; 4]), 0);
    }
}
