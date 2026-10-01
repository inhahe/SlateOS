//! TypeScript: tree-sitter-typescript 0.23.2 (MIT, Max Brunsfeld and the
//! tree-sitter contributors) -- its TypeScript grammar; its TSX grammar is
//! `tsx.rs`, which shares this module's scanner. `grammars/typescript/`
//! holds the TypeScript grammar's `parser.c`, the package's queries and test
//! corpus (which is TSX's too), and its scanner -- `scanner.h`, which each
//! grammar's two-line `scanner.c` includes -- as published; the scanner is
//! ported below.
//!
//! TypeScript's scanner is a fork of JavaScript's, taken before JavaScript's
//! learned to read comments ahead of a line break, the line and paragraph
//! separators, and where included ranges resume. What it adds: no semicolon
//! before a `}` that a `:` follows (`({a}: {a: number}) => ...` is a typed
//! pattern), before a `{` where a function's signature may go on into its
//! body, or before a `(` or `[` in an expression; and no conditional `?`
//! before a `:`, `)` or `,` (`x?: number`, an optional parameter). The
//! parts it shares with JavaScript's are JavaScript's port's
//! (`javascript.rs`): template text, JSX text, HTML comments, the blanks and
//! comments after a line break, and the grammar's character sets -- the same
//! sets in both grammars, which the tests hold them to.
//!
//! # The queries
//!
//! TypeScript's highlight query adds types, parameters and TypeScript's
//! keywords to JavaScript's -- the one vendored beside JavaScript's grammar,
//! the same text as the 0.23.1 query this package names. `tree-sitter.json`
//! lists TypeScript's *first*; but of one node's captures the last pattern's
//! wins (§1438), so JavaScript's `(identifier) @variable` would paint every
//! parameter a plain variable, and its `"<"` an operator in a type's `<...>`.
//! Here JavaScript's query comes first and TypeScript's after it, as
//! JavaScript lists its own parameters query after its main one (§1441).
//!
//! # Where the port differs
//!
//! As JavaScript's port does, for the same reasons: a blank is the grammar's
//! (the C asked `iswspace`, ASCII's in the C locale); a line ends at a
//! carriage return, a line or a paragraph separator as at a line feed (the
//! fork's C counted `\n` only); after `in` or `instanceof`, a name goes on
//! with the grammar's name characters (the C asked `iswalpha`); a block
//! comment with a line break in it is a line break (the fork's C returned at
//! the comment's `/`, and the break inside was lost); a statement ends
//! where the text resumes after a stretch left out of the parse -- which
//! JavaScript's C learned after TypeScript's forked; and `x?.5:1` is a
//! conditional, as in JavaScript's C and the language -- there is no `?.`
//! before a digit -- where the fork's C took every `?.` for optional
//! chaining, and the rest for an error.

use crate::ffi::{ExternalScanner, Lexer};

use super::javascript::{
    Ahead, blanks_and_comments, ends_line, goes_on_with_name, is, is_blank, is_digit,
    scan_html_comment, scan_jsx_text, scan_template_chars,
};

super::generated!("typescript", super::Scanner);

/// The highlight queries: JavaScript's, then TypeScript's (see the module
/// docs for the order).
pub(crate) const HIGHLIGHTS: &str = concat!(
    include_str!("../../grammars/javascript/highlights.scm"),
    "\n",
    include_str!("../../grammars/typescript/highlights.scm"),
);

/// The locals query, as `tree-sitter.json` lists it: TypeScript's -- a
/// parameter declares its name -- then JavaScript's.
pub(crate) const LOCALS: &str = concat!(
    include_str!("../../grammars/typescript/locals.scm"),
    "\n",
    include_str!("../../grammars/javascript/locals.scm"),
);

/// The external tokens, in the grammars' order: JavaScript's eight, then
/// TypeScript's own -- and a tenth, `__error_recovery`, which the scanner
/// never asks about.
#[derive(Clone, Copy)]
enum Token {
    AutomaticSemicolon,
    TemplateChars,
    TernaryQmark,
    HtmlComment,
    /// `||`, never scanned: its being allowed says an operator may follow --
    /// an expression, not a type.
    LogicalOr,
    /// Never scanned: allowed only inside a string or a template.
    EscapeSequence,
    /// Never scanned: allowed only inside a regular expression.
    RegexPattern,
    JsxText,
    /// Never scanned: its being allowed says a function's signature may go
    /// on into its body.
    FunctionSignatureAutomaticSemicolon,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The scanner -- both grammars' -- which keeps nothing between tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner;

/// Whether the token at the start of a line -- after a line break, past its
/// blanks and comments -- cannot go on with the statement before it, which
/// then ends at the break. `valid` says what the grammar allows here.
fn begins_statement(lexer: &mut Lexer<'_>, valid: impl Fn(Token) -> bool) -> bool {
    let next = u32::try_from(lexer.lookahead())
        .ok()
        .and_then(char::from_u32);
    match next {
        Some(
            '`' | ',' | '.' | ';' | '*' | '%' | '>' | '<' | '=' | '?' | '^' | '|' | '&' | '/' | ':',
        ) => false,
        // A function's signature may go on into its body.
        Some('{') => !valid(Token::FunctionSignatureAutomaticSemicolon),
        // In an expression -- where an operator may follow -- a call or an
        // index goes on with it; in a type, a new member begins.
        Some('(' | '[') => !valid(Token::LogicalOr),
        // Before `++` and `--`, not before a binary `+` or `-`.
        Some(sign @ ('+' | '-')) => {
            lexer.skip();
            is(lexer.lookahead(), sign)
        }
        // Before a unary `!`, not before `!=`.
        Some('!') => {
            lexer.skip();
            !is(lexer.lookahead(), '=')
        }
        // Before a name, not before `in` or `instanceof`.
        Some('i') => {
            lexer.skip();
            if !is(lexer.lookahead(), 'n') {
                return true;
            }
            lexer.skip();
            if !goes_on_with_name(lexer.lookahead()) {
                return false;
            }
            for expected in "stanceof".chars() {
                if !is(lexer.lookahead(), expected) {
                    return true;
                }
                lexer.skip();
            }
            goes_on_with_name(lexer.lookahead())
        }
        _ => true,
    }
}

/// A semicolon the text leaves out: at the end, before a `}` (unless a `:`
/// follows it, in a type), where the text resumes after a stretch left out
/// of the parse, or at a line break -- a block comment's included -- the
/// next line does not continue past.
fn scan_automatic_semicolon(
    lexer: &mut Lexer<'_>,
    valid: impl Fn(Token) -> bool + Copy,
    scanned_comment: &mut bool,
) -> bool {
    lexer.set_result(Token::AutomaticSemicolon.symbol());
    lexer.mark_end();
    loop {
        let c = lexer.lookahead();
        if c == 0 {
            return true;
        }
        if is(c, '}') {
            // `type F = ({a}: {a: number}) => number;`: a `}` and then a
            // `:` is a typed pattern, not a statement's end -- unless an
            // operator may follow, in a conditional's branches.
            loop {
                lexer.skip();
                if !is_blank(lexer.lookahead()) {
                    break;
                }
            }
            if is(lexer.lookahead(), ':') {
                return valid(Token::LogicalOr);
            }
            return true;
        }
        if lexer.is_at_included_range_start() {
            return true;
        }
        if is(c, '/') {
            // A block comment with a line break in it is a line break;
            // anything else here -- a comment on the line, a division --
            // leaves the semicolon to be decided past it.
            if blanks_and_comments(lexer, scanned_comment, false) == Ahead::BrokenInComment {
                if blanks_and_comments(lexer, scanned_comment, true) == Ahead::Reject {
                    return false;
                }
                return begins_statement(lexer, valid);
            }
            return false;
        }
        if !is_blank(c) {
            return false;
        }
        if ends_line(c) {
            break;
        }
        lexer.skip();
    }
    lexer.skip();
    if blanks_and_comments(lexer, scanned_comment, true) == Ahead::Reject {
        return false;
    }
    begins_statement(lexer, valid)
}

/// The `?` of a conditional -- not `??` or optional chaining's `?.`, and
/// not the `?` of an optional parameter or member, before a `:`, `)` or
/// `,` -- though `?.5`, and `? .5`, are conditionals: JavaScript has no
/// `?.` before a digit.
fn scan_ternary_qmark(lexer: &mut Lexer<'_>) -> bool {
    while is_blank(lexer.lookahead()) {
        lexer.skip();
    }
    if !is(lexer.lookahead(), '?') {
        return false;
    }
    lexer.advance();
    if is(lexer.lookahead(), '?') {
        return false;
    }
    lexer.mark_end();
    lexer.set_result(Token::TernaryQmark.symbol());
    while is_blank(lexer.lookahead()) {
        lexer.advance();
    }
    let c = lexer.lookahead();
    if is(c, ':') || is(c, ')') || is(c, ',') {
        return false;
    }
    if is(c, '.') {
        lexer.advance();
        return is_digit(lexer.lookahead());
    }
    true
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_automatic_semicolon",
        "_template_chars",
        "_ternary_qmark",
        "html_comment",
        "PIPE_PIPE",
        "escape_sequence",
        "regex_pattern",
        "jsx_text",
        "_function_signature_automatic_semicolon",
        "__error_recovery",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::TemplateChars) {
            // Both at once is error recovery, which this leaves to the
            // parser.
            if valid(Token::AutomaticSemicolon) {
                return false;
            }
            return scan_template_chars(lexer);
        }
        if valid(Token::JsxText) && scan_jsx_text(lexer) {
            return true;
        }
        if valid(Token::AutomaticSemicolon) || valid(Token::FunctionSignatureAutomaticSemicolon) {
            let mut scanned_comment = false;
            let found = scan_automatic_semicolon(lexer, valid, &mut scanned_comment);
            if !found
                && !scanned_comment
                && valid(Token::TernaryQmark)
                && is(lexer.lookahead(), '?')
            {
                return scan_ternary_qmark(lexer);
            }
            return found;
        }
        if valid(Token::TernaryQmark) {
            return scan_ternary_qmark(lexer);
        }
        if valid(Token::HtmlComment)
            && !valid(Token::LogicalOr)
            && !valid(Token::EscapeSequence)
            && !valid(Token::RegexPattern)
        {
            return scan_html_comment(lexer);
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
    use crate::grammars::javascript::tests::character_set_in;
    use crate::grammars::{javascript, tsx};

    /// `text`'s tree in TypeScript, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// **The scanner JavaScript's port shares with this one sees the same
    /// grammar**: JavaScript's tokens are TypeScript's first eight, at the
    /// same places -- the shared scanning sets them by place -- and both of
    /// TypeScript's grammars have JavaScript's blanks and name characters.
    #[test]
    fn what_is_shared_with_javascript_is_the_same() {
        assert_eq!(
            Scanner::TOKENS.get(..8),
            Some(<javascript::Scanner as ExternalScanner>::TOKENS)
        );
        for dir in ["typescript", "tsx"] {
            assert_eq!(
                javascript::BLANKS,
                character_set_in(dir, "extras_character_set_1"),
                "{dir}"
            );
            assert_eq!(
                javascript::NAME,
                character_set_in(dir, "sym_identifier_character_set_2"),
                "{dir}"
            );
        }
        // TSX's scanner is this one: its grammar's list of tokens is
        // checked against this one's when it builds (`generated!`).
        let _ = tsx::HIGHLIGHTS;
    }

    /// **A block comment with a line break in it is a line break**, as in
    /// JavaScript's port: `x /* ⏎ */ y` is two statements -- the fork's C
    /// lost the break inside the comment -- while `x /* ⏎ */ + y` goes on.
    #[test]
    fn a_line_break_in_a_block_comment_is_a_line_break() {
        assert_eq!(
            sexp("x /* a\n b */ y\n"),
            "(program (expression_statement (identifier)) (comment) (expression_statement (identifier)))"
        );
        assert_eq!(
            sexp("x /* a\n b */ + y\n"),
            "(program (expression_statement (binary_expression left: (identifier) (comment) right: (identifier))))"
        );
    }

    /// **A carriage return ends a line**, and so do the line and paragraph
    /// separators, where the fork's C counted `\n` alone.
    #[test]
    fn every_line_terminator_ends_a_line() {
        let two =
            "(program (expression_statement (identifier)) (expression_statement (identifier)))";
        for text in [
            "x\ny\n",
            "x\ry\r",
            "x\r\ny\r\n",
            "x\u{2028}y\n",
            "x\u{2029}y\n",
        ] {
            assert_eq!(sexp(text), two, "{text:?}");
        }
    }

    /// **A name beginning `in` starts a statement**: `in_range` and `in2`
    /// are names, where the C's `iswalpha` took them for `in`.
    #[test]
    fn a_name_beginning_in_starts_a_statement() {
        let two =
            "(program (expression_statement (identifier)) (expression_statement (identifier)))";
        for name in ["in_range", "in2", "instanceof$"] {
            assert_eq!(sexp(&format!("x\n{name}\n")), two, "{name}");
        }
    }

    /// **TypeScript's own rules hold**: a `?` before `:` or `)` is an
    /// optional parameter's; a `}` then a `:` is a typed pattern; a call on
    /// the next line goes on with an expression.
    #[test]
    fn typescripts_own_rules_hold() {
        let optional = sexp("function f(a?: number, b?) {}\n");
        assert!(!optional.contains("ERROR"), "{optional}");
        assert_eq!(
            optional.matches("optional_parameter").count(),
            2,
            "{optional}"
        );
        let typed = sexp("type F = ({a}: {a: number}) => number;\n");
        assert!(!typed.contains("ERROR"), "{typed}");
        let call = sexp("let x = f\n(1)\n");
        assert!(call.contains("call_expression"), "{call}");
        // A method chain goes on across lines.
        let chain = sexp("x\n  .y()\n");
        assert_eq!(
            chain,
            "(program (expression_statement (call_expression function: (member_expression object: (identifier) property: (property_identifier)) arguments: (arguments))))"
        );
        // An overload's signature, and the body after it -- a function's,
        // and a method's, whose signature a brace on the next line would
        // make a definition.
        let overload = sexp("function f(a: string): void\nfunction f(a) {}\n");
        assert!(!overload.contains("ERROR"), "{overload}");
        assert!(overload.contains("function_signature"), "{overload}");
        let method = sexp("class A {\n  foo(a: string): void\n  foo(a) {}\n  bar()\n  {}\n}\n");
        assert!(!method.contains("ERROR"), "{method}");
        assert!(method.contains("method_signature"), "{method}");
        assert_eq!(method.matches("method_definition").count(), 2, "{method}");
    }

    /// **`?.` before a digit is a conditional**, as in JavaScript: `x?.5:1`
    /// chooses between `.5` and `1`, while `x?.y` is optional chaining.
    #[test]
    fn a_question_mark_before_a_decimal_is_a_conditional() {
        let conditional = sexp("x?.5:1;\n");
        assert!(conditional.contains("ternary_expression"), "{conditional}");
        assert!(!conditional.contains("ERROR"), "{conditional}");
        let chained = sexp("x?.y;\n");
        assert!(chained.contains("optional_chain"), "{chained}");
    }

    /// **Where the text resumes after a stretch left out of the parse, a
    /// statement ends**, as in JavaScript's port.
    #[test]
    fn a_statement_ends_where_the_text_resumes() {
        let text = "x <%= %>y\n";
        let point = |column| tree_sitter::Point { row: 0, column };
        let range = |start: usize, end: usize| tree_sitter::Range {
            start_byte: start,
            end_byte: end,
            start_point: point(start),
            end_point: point(end),
        };
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser
            .set_included_ranges(&[range(0, 2), range(8, 10)])
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            tree.root_node().to_sexp(),
            "(program (expression_statement (identifier)) (expression_statement (identifier)))"
        );
    }
}
