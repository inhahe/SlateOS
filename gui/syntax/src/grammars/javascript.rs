//! JavaScript: tree-sitter-javascript 0.25.0 (MIT, Max Brunsfeld, Amaan
//! Qureshi and the tree-sitter contributors). `grammars/javascript/` holds
//! its `parser.c`, its queries, its test corpus and highlight tests, and its
//! `scanner.c` as published; the scanner is ported below. It settles what a
//! state machine cannot: whether a line break ends a statement written
//! without its `;` (automatic semicolon insertion), where a template
//! string's text stops, a `?` that begins a conditional rather than `?.`,
//! the HTML comments old scripts hid themselves behind (`<!--`, `-->`), and
//! the text between JSX tags. It keeps no state.
//!
//! # Where the port differs
//!
//! The C asks `iswspace` and `iswalpha`, whose answers depend on the C
//! library's locale -- in the C locale a grammar is tested in, ASCII's. Here
//! each asks what the grammar itself says, which is also what JavaScript
//! says:
//!
//! - **A blank is the grammar's**: what it skips between tokens, in its own
//!   words `[\s\p{Zs}\uFEFF\u2028\u2029\u2060\u200B]` ([`BLANKS`], its
//!   generated lexer's set). In the C a no-break space opening the line
//!   after `x` stopped the look past the line break, and a semicolon went in
//!   before a `(y)` the grammar reads as `x`'s arguments.
//! - **A carriage return ends a line**, as it does in JavaScript and in the
//!   grammar's own comments. The C counted `\n`, U+2028 and U+2029: text
//!   whose lines end in a bare `\r` had no semicolon inserted anywhere, and
//!   a `\r` inside a block comment did not make the comment a line break.
//!   Between JSX tags the C read `\r\n` as text, so a CRLF file's blank
//!   lines between tags were text nodes that its LF twin does not have.
//! - **A block comment with a line break in it is a line break**, as
//!   JavaScript reads one, wherever it is. After an expression -- where an
//!   operator may follow, and the C leaves a line break to be found on the
//!   scanner's next call, past the comment -- the C lost the break the
//!   comment held: `x /* ⏎ */ y` was one statement, and an error. And it
//!   took the character just after the comment for the next token, blank or
//!   not: `let a /* ⏎ */ = 1` had a semicolon put in before its `=`. Here
//!   the next token past the comment decides, as it does after any line
//!   break.
//! - **A name goes on with what the grammar's names go on with** ([`NAME`]:
//!   a letter or digit in any script, `_`, `$`, the `\` of a `\u` escape),
//!   where the C, deciding whether a line starting `in` or `instanceof`
//!   continues the last as that operator, asked `iswalpha`: `x` and then
//!   `in_range` on the next line were one statement, and an error.
//!
//! Between JSX tags a blank is still ASCII's, as it is in the C: whether
//! text between tags is all blank decides whether it is text at all, and
//! JSX keeps a no-break space there, as HTML does.

use crate::ffi::{ExternalScanner, Lexer, set_contains};

super::generated!("javascript", super::Scanner);

/// The highlight queries, as published, in the order `tree-sitter.json`
/// lists them: the grammar's, then JSX's tags and attributes, then
/// parameters. A later pattern wins over an earlier one for the same node,
/// so the order is part of what they say.
pub(crate) const HIGHLIGHTS: &str = concat!(
    include_str!("../../grammars/javascript/highlights.scm"),
    "\n",
    include_str!("../../grammars/javascript/highlights-jsx.scm"),
    "\n",
    include_str!("../../grammars/javascript/highlights-params.scm"),
);

/// The injection query, as published: a tagged template's text in the
/// language its tag names (`css`, `html`) -- and regular expressions, JSDoc
/// comments and Glimmer templates, which name languages this crate has no
/// grammar for, and so stay in JavaScript's colours.
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/javascript/injections.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    AutomaticSemicolon,
    TemplateChars,
    TernaryQmark,
    HtmlComment,
    /// `||`, never scanned: its being allowed says an operator may follow,
    /// which is when a comment and a line break do not end a statement.
    LogicalOr,
    /// Never scanned: allowed only inside a string or a template, where
    /// `<!--` is text.
    EscapeSequence,
    /// Never scanned: allowed only inside a regular expression, where
    /// `<!--` is part of the pattern.
    RegexPattern,
    JsxText,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The scanner, which keeps nothing between tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner;

/// The grammar's blanks: what it skips between tokens -- in its
/// `grammar.js`, `[\s\p{Zs}\uFEFF\u2028\u2029\u2060\u200B]` -- as its
/// generated lexer has them (`extras_character_set_1`, which a test holds
/// this to).
const BLANKS: &[(i32, i32)] = &[
    (0x09, 0x0d),
    (0x20, 0x20),
    (0xa0, 0xa0),
    (0x1680, 0x1680),
    (0x2000, 0x200b),
    (0x2028, 0x2029),
    (0x202f, 0x202f),
    (0x205f, 0x2060),
    (0x3000, 0x3000),
    (0xfeff, 0xfeff),
];

/// What a name goes on with after its first character, as the grammar's
/// generated lexer has it (`sym_identifier_character_set_2`, which a test
/// holds this to): anything but ASCII's punctuation, controls and the
/// grammar's blanks -- so a letter or digit in any script, `_`, `$`, and the
/// `\` that begins a `\u` escape.
const NAME: &[(i32, i32)] = &[
    (0x24, 0x24),
    (0x30, 0x39),
    (0x41, 0x5a),
    (0x5c, 0x5c),
    (0x5f, 0x5f),
    (0x61, 0x7a),
    (0x7f, 0x9f),
    (0xa1, 0x167f),
    (0x1681, 0x1fff),
    (0x200c, 0x2027),
    (0x202a, 0x202e),
    (0x2030, 0x205e),
    (0x2061, 0x2fff),
    (0x3001, 0xfefe),
    (0xff00, 0x0010_ffff),
];

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// Whether `c` is one of the grammar's blanks ([`BLANKS`]).
fn is_blank(c: i32) -> bool {
    set_contains(BLANKS, c)
}

/// Whether `c` ends a line: JavaScript's line terminators -- line feed,
/// carriage return, and the line and paragraph separators.
fn ends_line(c: i32) -> bool {
    is(c, '\n') || is(c, '\r') || c == 0x2028 || c == 0x2029
}

/// Whether `c` is a decimal digit: `iswdigit`, which is ASCII's in every
/// locale.
fn is_digit(c: i32) -> bool {
    (0x30..=0x39).contains(&c)
}

/// Whether a name goes on with `c` ([`NAME`]).
fn goes_on_with_name(c: i32) -> bool {
    set_contains(NAME, c)
}

/// A blank between JSX tags: ASCII's six -- space, tab, line feed,
/// vertical tab, form feed, carriage return -- as the C's `iswspace` has
/// them in the C locale.
fn is_jsx_blank(c: i32) -> bool {
    c == 0x20 || (0x09..=0x0d).contains(&c)
}

/// What the blanks and comments ahead say about a semicolon before them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ahead {
    /// A `/` that begins no comment -- a division, or a regular
    /// expression: no semicolon. (`REJECT`)
    Reject,
    /// A block comment with no line break in it, and something after it on
    /// its line: undecided. (`NO_NEWLINE`)
    NoNewline,
    /// A block comment with a line break in it -- a line break, to
    /// JavaScript -- and something after it on its line. (`ACCEPT`, in the
    /// C, which then read the character after the comment as the next
    /// token.)
    BrokenInComment,
    /// Nothing against one: at the next token past the blanks and comments,
    /// a `//` comment's line break among them. (`ACCEPT`)
    Accept,
}

/// A template string's text, up to its end, a `${` or an escape.
fn scan_template_chars(lexer: &mut Lexer<'_>) -> bool {
    lexer.set_result(Token::TemplateChars.symbol());
    let mut has_content = false;
    loop {
        lexer.mark_end();
        let c = lexer.lookahead();
        if is(c, '`') || is(c, '\\') {
            return has_content;
        }
        if c == 0 {
            return false;
        }
        lexer.advance();
        if is(c, '$') && is(lexer.lookahead(), '{') {
            return has_content;
        }
        has_content = true;
    }
}

/// Step over blanks and comments, answering what they say about a
/// semicolon before them; `scanned_comment` is set when there was a
/// comment. Unless `consume`, a block comment is looked past only as far as
/// needed to tell whether it held a line break.
fn blanks_and_comments(lexer: &mut Lexer<'_>, scanned_comment: &mut bool, consume: bool) -> Ahead {
    let mut saw_block_newline = false;
    loop {
        while is_blank(lexer.lookahead()) {
            lexer.skip();
        }
        if !is(lexer.lookahead(), '/') {
            return Ahead::Accept;
        }
        lexer.skip();
        if is(lexer.lookahead(), '/') {
            lexer.skip();
            while lexer.lookahead() != 0 && !ends_line(lexer.lookahead()) {
                lexer.skip();
            }
            *scanned_comment = true;
        } else if is(lexer.lookahead(), '*') {
            lexer.skip();
            while lexer.lookahead() != 0 {
                if is(lexer.lookahead(), '*') {
                    lexer.skip();
                    if is(lexer.lookahead(), '/') {
                        lexer.skip();
                        *scanned_comment = true;
                        if !is(lexer.lookahead(), '/') && !consume {
                            return if saw_block_newline {
                                Ahead::BrokenInComment
                            } else {
                                Ahead::NoNewline
                            };
                        }
                        break;
                    }
                } else {
                    saw_block_newline |= ends_line(lexer.lookahead());
                    lexer.skip();
                }
            }
        } else {
            return Ahead::Reject;
        }
    }
}

/// A semicolon the text leaves out: at the end, before a `}`, where the
/// text resumes after a stretch left out of this parse, or at a line break
/// the next line does not continue past. `comment_condition` is whether a
/// comment ending in a line break may end the statement: not where an
/// operator may follow.
fn scan_automatic_semicolon(
    lexer: &mut Lexer<'_>,
    comment_condition: bool,
    scanned_comment: &mut bool,
) -> bool {
    lexer.set_result(Token::AutomaticSemicolon.symbol());
    lexer.mark_end();
    loop {
        if lexer.lookahead() == 0 {
            return true;
        }
        if is(lexer.lookahead(), '/') {
            match blanks_and_comments(lexer, scanned_comment, false) {
                Ahead::Reject => return false,
                Ahead::BrokenInComment => {
                    return after_break_in_comment(lexer, comment_condition, scanned_comment);
                }
                Ahead::Accept => {
                    let c = lexer.lookahead();
                    if comment_condition && !is(c, ',') && !is(c, '=') {
                        return true;
                    }
                }
                Ahead::NoNewline => {}
            }
        }
        if is(lexer.lookahead(), '}') || lexer.is_at_included_range_start() {
            return true;
        }
        if ends_line(lexer.lookahead()) {
            break;
        }
        if !is_blank(lexer.lookahead()) {
            return false;
        }
        lexer.skip();
    }
    lexer.skip();
    if blanks_and_comments(lexer, scanned_comment, true) == Ahead::Reject {
        return false;
    }
    begins_statement(lexer)
}

/// After a block comment with a line break in it: past the blanks and
/// comments that follow, as [`scan_automatic_semicolon`] decides after a
/// `//` comment's line break -- and when an operator may follow, as it
/// decides after any line break.
fn after_break_in_comment(
    lexer: &mut Lexer<'_>,
    comment_condition: bool,
    scanned_comment: &mut bool,
) -> bool {
    if blanks_and_comments(lexer, scanned_comment, true) == Ahead::Reject {
        return false;
    }
    let c = lexer.lookahead();
    if comment_condition && !is(c, ',') && !is(c, '=') {
        return true;
    }
    begins_statement(lexer)
}

/// Whether the token at the start of a line -- after a line break, past
/// its blanks and comments -- cannot go on with the statement before it,
/// which then ends at the break.
fn begins_statement(lexer: &mut Lexer<'_>) -> bool {
    let next = u32::try_from(lexer.lookahead())
        .ok()
        .and_then(char::from_u32);
    match next {
        Some(
            '`' | ',' | ':' | ';' | '*' | '%' | '>' | '<' | '=' | '[' | '(' | '?' | '^' | '|' | '&'
            | '/',
        ) => false,
        // A semicolon before `.5`, not before `.name`.
        Some('.') => {
            lexer.skip();
            is_digit(lexer.lookahead())
        }
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

/// The `?` of a conditional -- not `??`, and not the `?.` of optional
/// chaining, though `? .5` is a conditional.
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
    if is(lexer.lookahead(), '.') {
        lexer.advance();
        return is_digit(lexer.lookahead());
    }
    true
}

/// An HTML comment, as a script may still hide itself from a browser that
/// does not run scripts: `<!--` or `-->`, to the end of the line.
fn scan_html_comment(lexer: &mut Lexer<'_>) -> bool {
    while is_blank(lexer.lookahead()) {
        lexer.skip();
    }
    let delimiter = if is(lexer.lookahead(), '<') {
        "<!--"
    } else if is(lexer.lookahead(), '-') {
        "-->"
    } else {
        return false;
    };
    for expected in delimiter.chars() {
        if !is(lexer.lookahead(), expected) {
            return false;
        }
        lexer.advance();
    }
    while lexer.lookahead() != 0 && !ends_line(lexer.lookahead()) {
        lexer.advance();
    }
    lexer.set_result(Token::HtmlComment.symbol());
    lexer.mark_end();
    true
}

/// Text between JSX tags, up to a tag, an expression or an entity: a token
/// only if there is text in it -- not if it is only blanks with a line
/// break among them, which JSX drops.
fn scan_jsx_text(lexer: &mut Lexer<'_>) -> bool {
    // Anything but blanks, or a blank on a line before its break.
    let mut saw_text = false;
    // At a line break, or at a blank after one.
    let mut at_newline = false;
    loop {
        let c = lexer.lookahead();
        if c == 0 || is(c, '<') || is(c, '>') || is(c, '{') || is(c, '}') || is(c, '&') {
            break;
        }
        if is(c, '\n') || is(c, '\r') {
            at_newline = true;
        } else {
            at_newline &= is_jsx_blank(c);
            saw_text |= !at_newline;
        }
        lexer.advance();
    }
    lexer.set_result(Token::JsxText.symbol());
    saw_text
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
        if valid(Token::AutomaticSemicolon) {
            let mut scanned_comment = false;
            let found =
                scan_automatic_semicolon(lexer, !valid(Token::LogicalOr), &mut scanned_comment);
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
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a parse that fails, or a table that cannot be read, is the failure"
)]
mod tests {
    use super::*;

    /// The grammar.
    fn language() -> tree_sitter::Language {
        tree_sitter::Language::new(generated::language_fn())
    }

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// The character ranges the generated lexer calls `name`, read out of
    /// the vendored `parser.c`: `{'\t', '\r'}, {' ', ' '}, {0xa0, 0xa0}`.
    fn character_set(name: &str) -> Vec<(i32, i32)> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/grammars/javascript/parser.c");
        let source = std::fs::read_to_string(path).expect("the vendored parser.c");
        let head = format!("static const TSCharacterRange {name}[] = {{");
        let start = source.find(&head).expect("the set") + head.len();
        let body = &source[start..start + source[start..].find("};").expect("its end")];
        let value = |item: &str| -> i32 {
            let item = item.trim();
            if let Some(hex) = item.strip_prefix("0x") {
                return i32::from_str_radix(hex, 16).expect(item);
            }
            let quoted = item
                .strip_prefix('\'')
                .and_then(|i| i.strip_suffix('\''))
                .expect(item);
            let c = match quoted {
                "\\t" => '\t',
                "\\n" => '\n',
                "\\r" => '\r',
                "\\\\" => '\\',
                "\\'" => '\'',
                _ => {
                    let mut chars = quoted.chars();
                    let c = chars.next().expect(item);
                    assert!(chars.next().is_none(), "{item}");
                    c
                }
            };
            c as i32
        };
        body.split('}')
            .filter_map(|pair| pair.split_once('{').map(|(_, p)| p))
            .map(|pair| {
                let (from, to) = pair.split_once(", ").expect(pair);
                (value(from), value(to))
            })
            .collect()
    }

    /// **A blank, and what a name goes on with, are the grammar's**: the
    /// sets the port asks are the ones its generated lexer has.
    #[test]
    fn blanks_and_names_are_the_grammars() {
        assert_eq!(BLANKS, character_set("extras_character_set_1"));
        assert_eq!(NAME, character_set("sym_identifier_character_set_2"));
    }

    /// **A blank the C locale does not know still lets a line go on**: a
    /// no-break space before `(y)` on the line after `x` is a blank, as it
    /// is to the grammar and to JavaScript, so `(y)` is `x`'s arguments --
    /// as with a space -- and not a statement of its own.
    #[test]
    fn a_blank_past_ascii_lets_a_line_go_on() {
        let call = "(program (expression_statement (call_expression function: (identifier) arguments: (arguments (identifier)))))";
        assert_eq!(sexp("x\n (y)\n"), call);
        assert_eq!(sexp("x\n\u{a0}(y)\n"), call);
        assert_eq!(sexp("x\n\u{3000}(y)\n"), call);
    }

    /// **A carriage return ends a line**: alone, as old Macintosh text has
    /// it, a line break ends a statement as `\n` does -- and inside a block
    /// comment makes the comment one.
    #[test]
    fn a_carriage_return_ends_a_line() {
        let two =
            "(program (expression_statement (identifier)) (expression_statement (identifier)))";
        assert_eq!(sexp("x\ny\n"), two);
        assert_eq!(sexp("x\ry\r"), two);
        assert_eq!(sexp("x\r\ny\r\n"), two);
        let commented = "(program (expression_statement (identifier)) (comment) (expression_statement (identifier)))";
        assert_eq!(
            sexp("return /* a\r b */ y\r"),
            "(program (return_statement) (comment) (expression_statement (identifier)))"
        );
        assert_eq!(sexp("x /* a\r b */ y\r"), commented);
    }

    /// **A block comment with a line break in it is a line break**, after an
    /// expression too: `x /* ⏎ */ y` is two statements, as `x ⏎ y` is --
    /// while what goes on with an expression still does, `+ y` or `.y` --
    /// and it is the next token past the comment that decides, so
    /// `let a /* ⏎ */ = 1` is a declaration with a value.
    #[test]
    fn a_line_break_in_a_block_comment_is_a_line_break() {
        assert_eq!(
            sexp("x /* a\n b */ y\n"),
            "(program (expression_statement (identifier)) (comment) (expression_statement (identifier)))"
        );
        assert_eq!(
            sexp("x /* a\n b */ ++y\n"),
            "(program (expression_statement (identifier)) (comment) (expression_statement (update_expression argument: (identifier))))"
        );
        assert_eq!(
            sexp("x /* a\n b */ + y\n"),
            "(program (expression_statement (binary_expression left: (identifier) (comment) right: (identifier))))"
        );
        assert_eq!(
            sexp("x /* a\n b */ .y\n"),
            "(program (expression_statement (member_expression object: (identifier) (comment) property: (property_identifier))))"
        );
        assert_eq!(
            sexp("let a /* a\n b */ = 1\n"),
            "(program (lexical_declaration (variable_declarator name: (identifier) (comment) value: (number))))"
        );
        assert_eq!(
            sexp("return /* a\n b */ y\n"),
            "(program (return_statement) (comment) (expression_statement (identifier)))"
        );
    }

    /// **A line of blanks between JSX tags is not text, however it ends**:
    /// a CRLF file's tree is its LF twin's -- while a no-break space there
    /// is text, as JSX keeps it.
    #[test]
    fn blank_lines_between_jsx_tags_are_not_text() {
        let lf = sexp("<div>\n  <b/>\n</div>\n");
        assert!(!lf.contains("jsx_text"), "{lf}");
        assert_eq!(sexp("<div>\r\n  <b/>\r\n</div>\r\n"), lf);
        let nbsp = sexp("<div>\n\u{a0}</div>\n");
        assert!(nbsp.contains("jsx_text"), "{nbsp}");
        assert!(!nbsp.contains("ERROR"), "{nbsp}");
    }

    /// **A line that starts with a name beginning `in` is a statement of its
    /// own** -- `in_range`, `in2`, `instanceof$` -- where a line that starts
    /// with the operator `in` or `instanceof` goes on with the last.
    #[test]
    fn a_name_beginning_in_starts_a_statement() {
        let two =
            "(program (expression_statement (identifier)) (expression_statement (identifier)))";
        for name in [
            "inside",
            "in_range",
            "in$",
            "in2",
            "inä",
            "instanceof_",
            "instanceofé",
        ] {
            assert_eq!(sexp(&format!("x\n{name}\n")), two, "{name}");
        }
        for operator in ["in", "instanceof"] {
            let tree = sexp(&format!("x\n{operator} y\n"));
            assert_eq!(
                tree,
                "(program (expression_statement (binary_expression left: (identifier) right: (identifier))))",
                "{operator}"
            );
        }
    }

    /// **Where the text resumes after a stretch left out of the parse, a
    /// statement ends** (`is_at_included_range_start`): `x ` and `y` in two
    /// ranges of a template, with no line break the parser sees between
    /// them, are two statements.
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
        parser.set_language(&language()).unwrap();
        parser
            .set_included_ranges(&[range(0, 2), range(8, 10)])
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            tree.root_node().to_sexp(),
            "(program (expression_statement (identifier)) (expression_statement (identifier)))"
        );
    }

    /// **A template string's text stops at its end, at `${` and at an
    /// escape** -- and a `$` not before `{` is text.
    #[test]
    fn a_template_strings_text_stops_where_it_should() {
        let tree = sexp("`a$b${c}\\n$`\n");
        assert_eq!(
            tree,
            "(program (expression_statement (template_string (string_fragment) (template_substitution (identifier)) (escape_sequence) (string_fragment))))"
        );
    }
}
