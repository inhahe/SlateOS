//! Markdown's inline grammar: tree-sitter-markdown 0.5.3's
//! `tree-sitter-markdown-inline` (MIT, Matthias Deiml and the
//! tree-sitter-grammars contributors). The block grammar (`markdown.rs`)
//! injects it into every paragraph and heading; it is not offered on its
//! own. `grammars/markdown_inline/` holds its `parser.c`, its queries, its
//! test corpus and its `scanner.c` as published; the scanner -- code spans,
//! LaTeX spans, and the delimiter runs of emphasis and strikethrough -- is
//! ported below.
//!
//! # Where the port differs
//!
//! The C has three copies of one function for `*`, `_` and `~` runs; here it
//! is one, [`parse_delimiter_run`], given the run's character and tokens.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("markdown_inline", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/markdown_inline/highlights.scm");

/// The injection query, as published: HTML tags and LaTeX, which have no
/// grammar here yet and so are left as they are.
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/markdown_inline/injections.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    Error,
    TriggerError,
    CodeSpanStart,
    CodeSpanClose,
    EmphasisOpenStar,
    EmphasisOpenUnderscore,
    EmphasisCloseStar,
    EmphasisCloseUnderscore,
    LastTokenWhitespace,
    LastTokenPunctuation,
    StrikethroughOpen,
    StrikethroughClose,
    LatexSpanStart,
    LatexSpanClose,
    UnclosedSpan,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The current delimiter run opens (`STATE_EMPHASIS_DELIMITER_IS_OPEN`).
const STATE_EMPHASIS_DELIMITER_IS_OPEN: u8 = 0x1 << 2;

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// Punctuation, as the Markdown spec defines it.
fn is_punctuation(c: i32) -> bool {
    [('!', '/'), (':', '@'), ('[', '`'), ('{', '~')]
        .iter()
        .any(|&(low, high)| (low as i32..=high as i32).contains(&c))
}

/// The scanner's state: flags, the open code and LaTeX spans' delimiter
/// lengths, and how much of the current delimiter run is left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    state: u8,
    code_span_delimiter_length: u8,
    latex_span_delimiter_length: u8,
    num_emphasis_delimiters_left: u8,
}

/// A span delimited by a run of one character -- a code span's backticks,
/// a LaTeX span's dollars -- which closes only on a run as long.
fn parse_leaf_delimiter(
    lexer: &mut Lexer<'_>,
    delimiter_length: &mut u8,
    valid: &dyn Fn(Token) -> bool,
    delimiter: char,
    open_token: Token,
    close_token: Token,
) -> bool {
    let mut level: u8 = 0;
    while is(lexer.lookahead(), delimiter) {
        lexer.advance();
        level = level.wrapping_add(1);
    }
    lexer.mark_end();
    if level == *delimiter_length && valid(close_token) {
        *delimiter_length = 0;
        lexer.set_result(close_token.symbol());
        return true;
    }
    if valid(open_token) {
        // Is there a run as long further on, to close it?
        let mut close_level: usize = 0;
        while !lexer.eof() {
            if is(lexer.lookahead(), delimiter) {
                close_level = close_level.saturating_add(1);
            } else {
                if close_level == usize::from(level) {
                    break;
                }
                close_level = 0;
            }
            lexer.advance();
        }
        if close_level == usize::from(level) {
            *delimiter_length = level;
            lexer.set_result(open_token.symbol());
            return true;
        }
        if valid(Token::UnclosedSpan) {
            lexer.set_result(Token::UnclosedSpan.symbol());
            return true;
        }
    }
    false
}

/// A run of `*`, `_` or `~`: whether it opens or closes emphasis (or
/// strikethrough), decided by what is on either side of it, as the spec's
/// left- and right-flanking rules say. The C's `parse_star`,
/// `parse_underscore` and `parse_tilde`, which are this with their own
/// character and tokens.
fn parse_delimiter_run(
    s: &mut Scanner,
    lexer: &mut Lexer<'_>,
    valid: &dyn Fn(Token) -> bool,
    delimiter: char,
    open: Token,
    close: Token,
) -> bool {
    lexer.advance();
    // Part of a run already decided: the rest of it goes the same way.
    if s.num_emphasis_delimiters_left > 0 {
        if (s.state & STATE_EMPHASIS_DELIMITER_IS_OPEN) != 0 && valid(open) {
            s.state &= !STATE_EMPHASIS_DELIMITER_IS_OPEN;
            lexer.set_result(open.symbol());
            s.num_emphasis_delimiters_left = s.num_emphasis_delimiters_left.wrapping_sub(1);
            return true;
        }
        if valid(close) {
            lexer.set_result(close.symbol());
            s.num_emphasis_delimiters_left = s.num_emphasis_delimiters_left.wrapping_sub(1);
            return true;
        }
    }
    lexer.mark_end();
    let mut count: u8 = 1;
    while is(lexer.lookahead(), delimiter) {
        count = count.wrapping_add(1);
        lexer.advance();
    }
    let line_end = is(lexer.lookahead(), '\n') || is(lexer.lookahead(), '\r') || lexer.eof();
    if valid(open) || valid(close) {
        // What the first of the run is, the rest are too.
        s.num_emphasis_delimiters_left = count.wrapping_sub(1);
        let next_symbol_whitespace =
            line_end || is(lexer.lookahead(), ' ') || is(lexer.lookahead(), '\t');
        let next_symbol_punctuation = is_punctuation(lexer.lookahead());
        // What came before the run is in `valid` (see grammar.js).
        if valid(close)
            && !valid(Token::LastTokenWhitespace)
            && (!valid(Token::LastTokenPunctuation)
                || next_symbol_punctuation
                || next_symbol_whitespace)
        {
            // Closing takes precedence.
            s.state &= !STATE_EMPHASIS_DELIMITER_IS_OPEN;
            lexer.set_result(close.symbol());
            return true;
        }
        if !next_symbol_whitespace
            && (!next_symbol_punctuation
                || valid(Token::LastTokenPunctuation)
                || valid(Token::LastTokenWhitespace))
        {
            s.state |= STATE_EMPHASIS_DELIMITER_IS_OPEN;
            lexer.set_result(open.symbol());
            return true;
        }
    }
    false
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_error",
        "_trigger_error",
        "_code_span_start",
        "_code_span_close",
        "_emphasis_open_star",
        "_emphasis_open_underscore",
        "_emphasis_close_star",
        "_emphasis_close_underscore",
        "_last_token_whitespace",
        "_last_token_punctuation",
        "_strikethrough_open",
        "_strikethrough_close",
        "_latex_span_start",
        "_latex_span_close",
        "_unclosed_span",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // A grammar rule asked for an error, to end a branch.
        if valid(Token::TriggerError) {
            lexer.set_result(Token::Error.symbol());
            return true;
        }
        let c = lexer.lookahead();
        if is(c, '`') {
            let mut length = self.code_span_delimiter_length;
            let found = parse_leaf_delimiter(
                lexer,
                &mut length,
                &valid,
                '`',
                Token::CodeSpanStart,
                Token::CodeSpanClose,
            );
            self.code_span_delimiter_length = length;
            found
        } else if is(c, '$') {
            let mut length = self.latex_span_delimiter_length;
            let found = parse_leaf_delimiter(
                lexer,
                &mut length,
                &valid,
                '$',
                Token::LatexSpanStart,
                Token::LatexSpanClose,
            );
            self.latex_span_delimiter_length = length;
            found
        } else if is(c, '*') {
            parse_delimiter_run(
                self,
                lexer,
                &valid,
                '*',
                Token::EmphasisOpenStar,
                Token::EmphasisCloseStar,
            )
        } else if is(c, '_') {
            parse_delimiter_run(
                self,
                lexer,
                &valid,
                '_',
                Token::EmphasisOpenUnderscore,
                Token::EmphasisCloseUnderscore,
            )
        } else if is(c, '~') {
            parse_delimiter_run(
                self,
                lexer,
                &valid,
                '~',
                Token::StrikethroughOpen,
                Token::StrikethroughClose,
            )
        } else {
            false
        }
    }

    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let state = [
            self.state,
            self.code_span_delimiter_length,
            self.latex_span_delimiter_length,
            self.num_emphasis_delimiters_left,
        ];
        match buffer.get_mut(..state.len()) {
            Some(slot) => {
                slot.copy_from_slice(&state);
                state.len()
            }
            None => 0,
        }
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        *self = match bytes {
            [state, code, latex, left, ..] => Self {
                state: *state,
                code_span_delimiter_length: *code,
                latex_span_delimiter_length: *latex,
                num_emphasis_delimiters_left: *left,
            },
            _ => Self::default(),
        };
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test: a parse that fails is the failure"
)]
mod tests {
    use core::ops::Range;

    /// Each code span `text` parses to, as the bytes of its delimiters.
    fn delimiters(text: &str) -> Vec<Vec<Range<usize>>> {
        let language = crate::Language::for_injection("markdown_inline").unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language.ts_language()).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        let mut cursor = root.walk();
        let spans: Vec<_> = root
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "code_span")
            .collect();
        spans
            .into_iter()
            .map(|span| {
                let mut inner = span.walk();
                span.named_children(&mut inner)
                    .filter(|n| n.kind() == "code_span_delimiter")
                    .map(|n| n.byte_range())
                    .collect()
            })
            .collect()
    }

    /// **A code span closes on a run of backticks exactly as long as the one
    /// that opened it, and no other** (the C's `level == *delimiter_length`).
    /// The corpus happens to have no longer run inside a span, so it passes a
    /// scanner that closes on any run at least as long; this pins it. In
    /// `` `a``b` `` the grammar lexes the pair's first backtick as text and
    /// the scanner sees the second as a run of one, which closes the span:
    /// the closing delimiter is that one byte, where a scanner closing on
    /// "at least as long" takes both. (CommonMark would run the span to the
    /// end; upstream's grammar, like this port, does not.)
    #[test]
    fn a_code_span_closes_only_on_a_run_as_long_as_its_opening() {
        assert_eq!(delimiters("`a``b`"), [[0..1, 3..4]]);
        // A run as long closes it, whole.
        assert_eq!(delimiters("``a`b`` c"), [[0..2, 5..7]]);
    }
}
