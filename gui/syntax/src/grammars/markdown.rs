//! Markdown: tree-sitter-markdown 0.5.3's block grammar (MIT, Matthias
//! Deiml and the tree-sitter-grammars contributors). It finds the blocks --
//! headings, lists, quotes, code blocks, tables, front matter -- and injects
//! the inline grammar (`markdown_inline.rs`) into each paragraph and heading,
//! and a fenced block's language into its code. `grammars/markdown/` holds
//! its `parser.c`, its queries, its test corpus and its `scanner.c` as
//! published; the scanner, which is most of the block grammar, is ported
//! below function for function under the C's names.
//!
//! # Where the port differs
//!
//! - **Digits and letters are ASCII, as HTML's and Markdown's are.** The C
//!   hands a code point to `isdigit`, whose argument must be a byte (a larger
//!   one is undefined behaviour), and to `iswalpha`/`iswalnum`/`towlower`,
//!   whose answers depend on the C library's locale.
//! - **Saving stays inside its buffer, a block a byte.** The C writes each
//!   open block as a four-byte enum and never checks the room.
//! - **Punctuation is the whole character's.** The C casts a code point to a
//!   `char` first, so `ġ` (U+0121, low byte `!`) was punctuation to it after
//!   a table cell's backslash.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("markdown", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/markdown/highlights.scm");

/// The injection query, as published: inline content, fenced code in its
/// language, front matter as YAML or TOML, HTML blocks.
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/markdown/injections.scm");

/// The external tokens, in the grammar's order (`TokenType`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
#[allow(
    dead_code,
    reason = "every token in the grammar's order, so each discriminant is its index; the \
              heading markers after the first are reached from it by level, as the C does"
)]
enum Token {
    LineEnding,
    SoftLineEnding,
    BlockClose,
    BlockContinuation,
    BlockQuoteStart,
    IndentedChunkStart,
    AtxH1Marker,
    AtxH2Marker,
    AtxH3Marker,
    AtxH4Marker,
    AtxH5Marker,
    AtxH6Marker,
    SetextH1Underline,
    SetextH2Underline,
    ThematicBreak,
    ListMarkerMinus,
    ListMarkerPlus,
    ListMarkerStar,
    ListMarkerParenthesis,
    ListMarkerDot,
    ListMarkerMinusDontInterrupt,
    ListMarkerPlusDontInterrupt,
    ListMarkerStarDontInterrupt,
    ListMarkerParenthesisDontInterrupt,
    ListMarkerDotDontInterrupt,
    FencedCodeBlockStartBacktick,
    FencedCodeBlockStartTilde,
    BlankLineStart,
    FencedCodeBlockEndBacktick,
    FencedCodeBlockEndTilde,
    HtmlBlock1Start,
    HtmlBlock1End,
    HtmlBlock2Start,
    HtmlBlock3Start,
    HtmlBlock4Start,
    HtmlBlock5Start,
    HtmlBlock6Start,
    HtmlBlock7Start,
    CloseBlock,
    NoIndentedChunk,
    Error,
    TriggerError,
    // The C's `TOKEN_EOF`.
    Eof,
    MinusMetadata,
    PlusMetadata,
    PipeTableStart,
    PipeTableLineEnding,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// How many tokens there are.
const TOKEN_COUNT: usize = 47;

/// The tokens that may interrupt a paragraph: what the scanner asks itself
/// for, simulating, to decide whether a line break ended one
/// (`paragraph_interrupt_symbols`).
const PARAGRAPH_INTERRUPT_SYMBOLS: [bool; TOKEN_COUNT] = [
    false, // LINE_ENDING
    false, // SOFT_LINE_ENDING
    false, // BLOCK_CLOSE
    false, // BLOCK_CONTINUATION
    true,  // BLOCK_QUOTE_START
    false, // INDENTED_CHUNK_START
    true,  // ATX_H1_MARKER
    true,  // ATX_H2_MARKER
    true,  // ATX_H3_MARKER
    true,  // ATX_H4_MARKER
    true,  // ATX_H5_MARKER
    true,  // ATX_H6_MARKER
    true,  // SETEXT_H1_UNDERLINE
    true,  // SETEXT_H2_UNDERLINE
    true,  // THEMATIC_BREAK
    true,  // LIST_MARKER_MINUS
    true,  // LIST_MARKER_PLUS
    true,  // LIST_MARKER_STAR
    true,  // LIST_MARKER_PARENTHESIS
    true,  // LIST_MARKER_DOT
    false, // LIST_MARKER_MINUS_DONT_INTERRUPT
    false, // LIST_MARKER_PLUS_DONT_INTERRUPT
    false, // LIST_MARKER_STAR_DONT_INTERRUPT
    false, // LIST_MARKER_PARENTHESIS_DONT_INTERRUPT
    false, // LIST_MARKER_DOT_DONT_INTERRUPT
    true,  // FENCED_CODE_BLOCK_START_BACKTICK
    true,  // FENCED_CODE_BLOCK_START_TILDE
    true,  // BLANK_LINE_START
    false, // FENCED_CODE_BLOCK_END_BACKTICK
    false, // FENCED_CODE_BLOCK_END_TILDE
    true,  // HTML_BLOCK_1_START
    false, // HTML_BLOCK_1_END
    true,  // HTML_BLOCK_2_START
    true,  // HTML_BLOCK_3_START
    true,  // HTML_BLOCK_4_START
    true,  // HTML_BLOCK_5_START
    true,  // HTML_BLOCK_6_START
    false, // HTML_BLOCK_7_START
    false, // CLOSE_BLOCK
    false, // NO_INDENTED_CHUNK
    false, // ERROR
    false, // TRIGGER_ERROR
    false, // EOF
    false, // MINUS_METADATA
    false, // PLUS_METADATA
    true,  // PIPE_TABLE_START
    false, // PIPE_TABLE_LINE_ENDING
];

/// A block on the open-blocks stack, as the C's `Block` numbers it: a list
/// item's value is `LIST_ITEM` plus how far its content is indented past
/// two, so any value may be held.
type Block = u8;
const BLOCK_QUOTE: Block = 0;
const INDENTED_CODE_BLOCK: Block = 1;
const LIST_ITEM: Block = 2;
const LIST_ITEM_MAX_INDENTATION: Block = 17;
const FENCED_CODE_BLOCK: Block = 18;
const ANONYMOUS: Block = 19;

/// Matching the open blocks at the start of a line.
const STATE_MATCHING: u8 = 0x1 << 0;
/// The last line break was inside a paragraph.
const STATE_WAS_SOFT_LINE_BREAK: u8 = 0x1 << 1;
/// Close the innermost block after the next line break.
const STATE_CLOSE_BLOCK: u8 = 0x1 << 4;

/// HTML block start condition 1's tag names.
const HTML_TAG_NAMES_RULE_1: [&str; 3] = ["pre", "script", "style"];

/// HTML block start condition 6's tag names.
const HTML_TAG_NAMES_RULE_7: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "section",
    "source",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

fn is_blank(c: i32) -> bool {
    is(c, ' ') || is(c, '\t')
}

fn is_newline(c: i32) -> bool {
    is(c, '\n') || is(c, '\r')
}

fn is_digit(c: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&c)
}

fn is_ascii_alpha(c: i32) -> bool {
    u8::try_from(c).is_ok_and(|b| b.is_ascii_alphabetic())
}

fn is_ascii_alnum(c: i32) -> bool {
    u8::try_from(c).is_ok_and(|b| b.is_ascii_alphanumeric())
}

/// ASCII punctuation, as the Markdown spec defines it.
fn is_punctuation(c: i32) -> bool {
    [('!', '/'), (':', '@'), ('[', '`'), ('{', '~')]
        .iter()
        .any(|&(low, high)| (low as i32..=high as i32).contains(&c))
}

/// The indentation a list item's lines need at least.
fn list_item_indentation(block: Block) -> u8 {
    block.wrapping_sub(LIST_ITEM).wrapping_add(2)
}

/// A number the C holds in a byte, as the byte.
fn low_byte(n: usize) -> u8 {
    n.to_le_bytes().first().copied().unwrap_or(0)
}

/// The scanner's state: the stack of open blocks, flags, how many blocks this
/// line has matched, indentation consumed but not yet used, the column (for
/// tab stops), and the open fenced block's delimiter length. `simulate` is
/// set while the scanner asks itself what a line would start, which must not
/// change the stack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    open_blocks: Vec<Block>,
    state: u8,
    matched: u8,
    indentation: u8,
    column: u8,
    fenced_code_block_delimiter_length: u8,
    simulate: bool,
}

impl Scanner {
    fn push_block(&mut self, block: Block) {
        self.open_blocks.push(block);
    }

    fn pop_block(&mut self) {
        self.open_blocks.pop();
    }

    /// How many blocks are open, as the byte the C compares with.
    fn open_count(&self) -> u8 {
        low_byte(self.open_blocks.len())
    }

    fn mark_end(&self, lexer: &mut Lexer<'_>) {
        if !self.simulate {
            lexer.mark_end();
        }
    }

    /// Advance one character, keeping the column for tab stops of four;
    /// answers how many columns it took.
    fn advance(&mut self, lexer: &mut Lexer<'_>) -> u8 {
        let mut size = 1;
        if is(lexer.lookahead(), '\t') {
            size = 4u8.wrapping_sub(self.column);
            self.column = 0;
        } else {
            // `(column + 1) % 4`.
            self.column = self.column.wrapping_add(1) & 3;
        }
        lexer.advance();
        size
    }

    /// Consume what belongs to `block` at the start of a line -- a list
    /// item's or code block's indentation, a quote's `>` -- answering whether
    /// the line continues it.
    fn match_block(&mut self, lexer: &mut Lexer<'_>, block: Block) -> bool {
        match block {
            INDENTED_CODE_BLOCK => {
                while self.indentation < 4 && is_blank(lexer.lookahead()) {
                    let n = self.advance(lexer);
                    self.indentation = self.indentation.wrapping_add(n);
                }
                if self.indentation >= 4 && !is_newline(lexer.lookahead()) {
                    self.indentation = self.indentation.saturating_sub(4);
                    return true;
                }
                false
            }
            LIST_ITEM..=LIST_ITEM_MAX_INDENTATION => {
                let want = list_item_indentation(block);
                while self.indentation < want && is_blank(lexer.lookahead()) {
                    let n = self.advance(lexer);
                    self.indentation = self.indentation.wrapping_add(n);
                }
                if self.indentation >= want {
                    self.indentation = self.indentation.saturating_sub(want);
                    return true;
                }
                if is_newline(lexer.lookahead()) {
                    self.indentation = 0;
                    return true;
                }
                false
            }
            BLOCK_QUOTE => {
                while is_blank(lexer.lookahead()) {
                    let n = self.advance(lexer);
                    self.indentation = self.indentation.wrapping_add(n);
                }
                if is(lexer.lookahead(), '>') {
                    self.advance(lexer);
                    self.indentation = 0;
                    if is_blank(lexer.lookahead()) {
                        let n = self.advance(lexer);
                        self.indentation = self.indentation.wrapping_add(n.wrapping_sub(1));
                    }
                    return true;
                }
                false
            }
            FENCED_CODE_BLOCK | ANONYMOUS => true,
            _ => false,
        }
    }

    fn parse_fenced_code_block(
        &mut self,
        delimiter: char,
        lexer: &mut Lexer<'_>,
        valid: &dyn Fn(Token) -> bool,
    ) -> bool {
        let backtick = delimiter == '`';
        let mut level: u8 = 0;
        while is(lexer.lookahead(), delimiter) {
            self.advance(lexer);
            level = level.wrapping_add(1);
        }
        self.mark_end(lexer);
        // Closing an open block is the only reading when it can: at least as
        // long as the opening, and indented less than four.
        let end = if backtick {
            Token::FencedCodeBlockEndBacktick
        } else {
            Token::FencedCodeBlockEndTilde
        };
        if valid(end) && self.indentation < 4 && level >= self.fenced_code_block_delimiter_length {
            while is_blank(lexer.lookahead()) {
                self.advance(lexer);
            }
            if is_newline(lexer.lookahead()) {
                self.fenced_code_block_delimiter_length = 0;
                lexer.set_result(end.symbol());
                return true;
            }
        }
        let start = if backtick {
            Token::FencedCodeBlockStartBacktick
        } else {
            Token::FencedCodeBlockStartTilde
        };
        if valid(start) && level >= 3 {
            // A backtick fence's info string may not hold a backtick.
            let mut info_string_has_backtick = false;
            if backtick {
                while !is_newline(lexer.lookahead()) && !lexer.eof() {
                    if is(lexer.lookahead(), '`') {
                        info_string_has_backtick = true;
                        break;
                    }
                    self.advance(lexer);
                }
            }
            if !info_string_has_backtick {
                lexer.set_result(start.symbol());
                if !self.simulate {
                    self.push_block(FENCED_CODE_BLOCK);
                }
                self.fenced_code_block_delimiter_length = level;
                self.indentation = 0;
                return true;
            }
        }
        false
    }

    /// The list item a marker opens, its content indented `extra` past the
    /// marker (one space of which the marker takes): the C's shared tail of
    /// every list marker's parse.
    fn open_list_item(&mut self, mut extra_indentation: u8, digits: usize) {
        extra_indentation = extra_indentation.wrapping_sub(1);
        if extra_indentation <= 3 {
            extra_indentation = extra_indentation.wrapping_add(self.indentation);
            self.indentation = 0;
        } else {
            // The content starts an indented code block: the marker's own
            // indentation is the item's, and the rest is kept for the block.
            core::mem::swap(&mut self.indentation, &mut extra_indentation);
        }
        if !self.simulate {
            self.push_block(
                LIST_ITEM
                    .wrapping_add(extra_indentation)
                    .wrapping_add(low_byte(digits)),
            );
        }
    }

    fn parse_star(&mut self, lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
        self.advance(lexer);
        self.mark_end(lexer);
        // Stars, with blanks allowed between them; and how far past the
        // first star the next character is.
        let mut star_count: usize = 1;
        let mut extra_indentation: u8 = 0;
        loop {
            let c = lexer.lookahead();
            if is(c, '*') {
                if star_count == 1 && extra_indentation >= 1 && valid(Token::ListMarkerStar) {
                    // The token is at least this long if it is a list marker.
                    self.mark_end(lexer);
                }
                star_count = star_count.saturating_add(1);
                self.advance(lexer);
            } else if is_blank(c) {
                let n = self.advance(lexer);
                if star_count == 1 {
                    extra_indentation = extra_indentation.wrapping_add(n);
                }
            } else {
                break;
            }
        }
        let line_end = is_newline(lexer.lookahead());
        let mut dont_interrupt = false;
        if star_count == 1 && line_end {
            extra_indentation = 1;
            // An empty item does not interrupt a paragraph.
            dont_interrupt = self.matched == self.open_count();
        }
        let thematic_break = star_count >= 3 && line_end;
        let list_marker_star = star_count >= 1 && extra_indentation >= 1;
        if valid(Token::ThematicBreak) && thematic_break && self.indentation < 4 {
            lexer.set_result(Token::ThematicBreak.symbol());
            self.mark_end(lexer);
            self.indentation = 0;
            return true;
        }
        let marker = if dont_interrupt {
            Token::ListMarkerStarDontInterrupt
        } else {
            Token::ListMarkerStar
        };
        if valid(marker) && list_marker_star {
            if star_count == 1 {
                self.mark_end(lexer);
            }
            self.open_list_item(extra_indentation, 0);
            lexer.set_result(marker.symbol());
            return true;
        }
        false
    }

    fn parse_thematic_break_underscore(
        &mut self,
        lexer: &mut Lexer<'_>,
        valid: &dyn Fn(Token) -> bool,
    ) -> bool {
        self.advance(lexer);
        self.mark_end(lexer);
        let mut underscore_count: usize = 1;
        loop {
            let c = lexer.lookahead();
            if is(c, '_') {
                underscore_count = underscore_count.saturating_add(1);
                self.advance(lexer);
            } else if is_blank(c) {
                self.advance(lexer);
            } else {
                break;
            }
        }
        if underscore_count >= 3 && is_newline(lexer.lookahead()) && valid(Token::ThematicBreak) {
            lexer.set_result(Token::ThematicBreak.symbol());
            self.mark_end(lexer);
            self.indentation = 0;
            return true;
        }
        false
    }

    fn parse_block_quote(&mut self, lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
        if !valid(Token::BlockQuoteStart) {
            return false;
        }
        self.advance(lexer);
        self.indentation = 0;
        if is_blank(lexer.lookahead()) {
            let n = self.advance(lexer);
            self.indentation = self.indentation.wrapping_add(n.wrapping_sub(1));
        }
        lexer.set_result(Token::BlockQuoteStart.symbol());
        if !self.simulate {
            self.push_block(BLOCK_QUOTE);
        }
        true
    }

    fn parse_atx_heading(&mut self, lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
        if !(valid(Token::AtxH1Marker) && self.indentation <= 3) {
            return false;
        }
        self.mark_end(lexer);
        let mut level: u16 = 0;
        while is(lexer.lookahead(), '#') && level <= 6 {
            self.advance(lexer);
            level = level.saturating_add(1);
        }
        let c = lexer.lookahead();
        if (1..=6).contains(&level) && (is_blank(c) || is_newline(c)) {
            lexer.set_result(
                Token::AtxH1Marker
                    .symbol()
                    .saturating_add(level.saturating_sub(1)),
            );
            self.indentation = 0;
            self.mark_end(lexer);
            return true;
        }
        false
    }

    fn parse_setext_underline(
        &mut self,
        lexer: &mut Lexer<'_>,
        valid: &dyn Fn(Token) -> bool,
    ) -> bool {
        if !(valid(Token::SetextH1Underline) && self.matched == self.open_count()) {
            return false;
        }
        self.mark_end(lexer);
        while is(lexer.lookahead(), '=') {
            self.advance(lexer);
        }
        while is_blank(lexer.lookahead()) {
            self.advance(lexer);
        }
        if is_newline(lexer.lookahead()) {
            lexer.set_result(Token::SetextH1Underline.symbol());
            self.mark_end(lexer);
            return true;
        }
        false
    }

    /// Past a line ending: `\r\n`, `\r` or `\n`.
    fn advance_newline(&mut self, lexer: &mut Lexer<'_>) {
        if is(lexer.lookahead(), '\r') {
            self.advance(lexer);
            if is(lexer.lookahead(), '\n') {
                self.advance(lexer);
            }
        } else {
            self.advance(lexer);
        }
    }

    /// Front matter fenced by lines of exactly three `fence`s, from just
    /// after the first line's: the lines up to the closing fence, answering
    /// whether there was one (`minus_metadata`, `plus_metadata`).
    fn parse_metadata(&mut self, lexer: &mut Lexer<'_>, fence: char, token: Token) -> bool {
        loop {
            self.advance_newline(lexer);
            let mut count: usize = 0;
            while is(lexer.lookahead(), fence) {
                count = count.saturating_add(1);
                self.advance(lexer);
            }
            if count == 3 {
                while is_blank(lexer.lookahead()) {
                    self.advance(lexer);
                }
                if is_newline(lexer.lookahead()) {
                    self.advance_newline(lexer);
                    self.mark_end(lexer);
                    lexer.set_result(token.symbol());
                    return true;
                }
            }
            while !is_newline(lexer.lookahead()) && !lexer.eof() {
                self.advance(lexer);
            }
            // No closing fence before the end: not front matter.
            if lexer.eof() {
                return false;
            }
        }
    }

    fn parse_plus(&mut self, lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
        if !(self.indentation <= 3
            && (valid(Token::ListMarkerPlus)
                || valid(Token::ListMarkerPlusDontInterrupt)
                || valid(Token::PlusMetadata)))
        {
            return false;
        }
        self.advance(lexer);
        if valid(Token::PlusMetadata) && is(lexer.lookahead(), '+') {
            self.advance(lexer);
            if !is(lexer.lookahead(), '+') {
                return false;
            }
            self.advance(lexer);
            while is_blank(lexer.lookahead()) {
                self.advance(lexer);
            }
            if !is_newline(lexer.lookahead()) {
                return false;
            }
            return self.parse_metadata(lexer, '+', Token::PlusMetadata);
        }
        let mut extra_indentation: u8 = 0;
        while is_blank(lexer.lookahead()) {
            let n = self.advance(lexer);
            extra_indentation = extra_indentation.wrapping_add(n);
        }
        let mut dont_interrupt = false;
        if is_newline(lexer.lookahead()) {
            extra_indentation = 1;
            dont_interrupt = true;
        }
        dont_interrupt = dont_interrupt && self.matched == self.open_count();
        let marker = if dont_interrupt {
            Token::ListMarkerPlusDontInterrupt
        } else {
            Token::ListMarkerPlus
        };
        if extra_indentation >= 1 && valid(marker) {
            lexer.set_result(marker.symbol());
            self.open_list_item(extra_indentation, 0);
            return true;
        }
        false
    }

    fn parse_ordered_list_marker(
        &mut self,
        lexer: &mut Lexer<'_>,
        valid: &dyn Fn(Token) -> bool,
    ) -> bool {
        if !(self.indentation <= 3
            && (valid(Token::ListMarkerParenthesis)
                || valid(Token::ListMarkerDot)
                || valid(Token::ListMarkerParenthesisDontInterrupt)
                || valid(Token::ListMarkerDotDontInterrupt)))
        {
            return false;
        }
        let mut digits: usize = 1;
        let mut dont_interrupt = !is_digit(lexer.lookahead());
        self.advance(lexer);
        while is_digit(lexer.lookahead()) {
            dont_interrupt = true;
            digits = digits.saturating_add(1);
            self.advance(lexer);
        }
        if !(1..=9).contains(&digits) {
            return false;
        }
        let dot = is(lexer.lookahead(), '.');
        let parenthesis = !dot && is(lexer.lookahead(), ')');
        if !(dot || parenthesis) {
            return false;
        }
        self.advance(lexer);
        let mut extra_indentation: u8 = 0;
        while is_blank(lexer.lookahead()) {
            let n = self.advance(lexer);
            extra_indentation = extra_indentation.wrapping_add(n);
        }
        if is_newline(lexer.lookahead()) {
            extra_indentation = 1;
            dont_interrupt = true;
        }
        dont_interrupt = dont_interrupt && self.matched == self.open_count();
        let asked = match (dot, dont_interrupt) {
            (true, true) => Token::ListMarkerDotDontInterrupt,
            (true, false) => Token::ListMarkerDot,
            (false, true) => Token::ListMarkerParenthesisDontInterrupt,
            (false, false) => Token::ListMarkerParenthesis,
        };
        if extra_indentation >= 1 && valid(asked) {
            // Reported without the don't-interrupt variant, as the C has it.
            let reported = if dot {
                Token::ListMarkerDot
            } else {
                Token::ListMarkerParenthesis
            };
            lexer.set_result(reported.symbol());
            self.open_list_item(extra_indentation, digits);
            return true;
        }
        false
    }

    fn parse_minus(&mut self, lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
        if !(self.indentation <= 3
            && (valid(Token::ListMarkerMinus)
                || valid(Token::ListMarkerMinusDontInterrupt)
                || valid(Token::SetextH2Underline)
                || valid(Token::ThematicBreak)
                || valid(Token::MinusMetadata)))
        {
            return false;
        }
        self.mark_end(lexer);
        let mut whitespace_after_minus = false;
        let mut minus_after_whitespace = false;
        let mut minus_count: usize = 0;
        let mut extra_indentation: u8 = 0;
        loop {
            let c = lexer.lookahead();
            if is(c, '-') {
                if minus_count == 1 && extra_indentation >= 1 {
                    self.mark_end(lexer);
                }
                minus_count = minus_count.saturating_add(1);
                self.advance(lexer);
                minus_after_whitespace = whitespace_after_minus;
            } else if is_blank(c) {
                let n = self.advance(lexer);
                if minus_count == 1 {
                    extra_indentation = extra_indentation.wrapping_add(n);
                }
                whitespace_after_minus = true;
            } else {
                break;
            }
        }
        let line_end = is_newline(lexer.lookahead());
        let mut dont_interrupt = false;
        if minus_count == 1 && line_end {
            extra_indentation = 1;
            dont_interrupt = true;
        }
        dont_interrupt = dont_interrupt && self.matched == self.open_count();
        let thematic_break = minus_count >= 3 && line_end;
        // A setext heading cannot break a lazy continuation.
        let underline = minus_count >= 1
            && !minus_after_whitespace
            && line_end
            && self.matched == self.open_count();
        let list_marker_minus = minus_count >= 1 && extra_indentation >= 1;
        let mut success = false;
        let marker = if dont_interrupt {
            Token::ListMarkerMinusDontInterrupt
        } else {
            Token::ListMarkerMinus
        };
        if valid(Token::SetextH2Underline) && underline {
            lexer.set_result(Token::SetextH2Underline.symbol());
            self.mark_end(lexer);
            self.indentation = 0;
            success = true;
        } else if valid(Token::ThematicBreak) && thematic_break {
            lexer.set_result(Token::ThematicBreak.symbol());
            self.mark_end(lexer);
            self.indentation = 0;
            success = true;
        } else if valid(marker) && list_marker_minus {
            if minus_count == 1 {
                self.mark_end(lexer);
            }
            self.open_list_item(extra_indentation, 0);
            lexer.set_result(marker.symbol());
            return true;
        }
        if minus_count == 3
            && !minus_after_whitespace
            && line_end
            && valid(Token::MinusMetadata)
            && self.parse_metadata(lexer, '-', Token::MinusMetadata)
        {
            return true;
        }
        success
    }

    /// An HTML block's start, the CommonMark spec's seven conditions.
    #[allow(
        clippy::too_many_lines,
        reason = "scanner.c's `parse_html_block`, ported branch for branch so it can be checked against the C"
    )]
    fn parse_html_block(&mut self, lexer: &mut Lexer<'_>, valid: &dyn Fn(Token) -> bool) -> bool {
        let any = [
            Token::HtmlBlock1Start,
            Token::HtmlBlock1End,
            Token::HtmlBlock2Start,
            Token::HtmlBlock3Start,
            Token::HtmlBlock4Start,
            Token::HtmlBlock5Start,
            Token::HtmlBlock6Start,
            Token::HtmlBlock7Start,
        ];
        if !any.iter().any(|t| valid(*t)) {
            return false;
        }
        self.advance(lexer);
        // What most starts do: an anonymous block the grammar closes.
        let start = |s: &mut Self, lexer: &mut Lexer<'_>, token: Token| {
            lexer.set_result(token.symbol());
            if !s.simulate {
                s.push_block(ANONYMOUS);
            }
            true
        };
        if is(lexer.lookahead(), '?') && valid(Token::HtmlBlock3Start) {
            self.advance(lexer);
            return start(self, lexer, Token::HtmlBlock3Start);
        }
        if is(lexer.lookahead(), '!') {
            self.advance(lexer);
            let c = lexer.lookahead();
            if is(c, '-') {
                self.advance(lexer);
                if is(lexer.lookahead(), '-') && valid(Token::HtmlBlock2Start) {
                    self.advance(lexer);
                    return start(self, lexer, Token::HtmlBlock2Start);
                }
            } else if (i32::from(b'A')..=i32::from(b'Z')).contains(&c)
                && valid(Token::HtmlBlock4Start)
            {
                self.advance(lexer);
                return start(self, lexer, Token::HtmlBlock4Start);
            } else if is(c, '[') {
                self.advance(lexer);
                let mut cdata = true;
                for ch in "CDATA".chars() {
                    if !is(lexer.lookahead(), ch) {
                        cdata = false;
                        break;
                    }
                    self.advance(lexer);
                }
                if cdata && is(lexer.lookahead(), '[') && valid(Token::HtmlBlock5Start) {
                    self.advance(lexer);
                    return start(self, lexer, Token::HtmlBlock5Start);
                }
            }
        }
        let starting_slash = is(lexer.lookahead(), '/');
        if starting_slash {
            self.advance(lexer);
        }
        // The tag's name, lowered, up to ten letters; longer is no name the
        // lists hold.
        let mut name = String::new();
        let mut too_long = false;
        while is_ascii_alpha(lexer.lookahead()) {
            if name.len() < 10 {
                if let Some(ch) = u32::try_from(lexer.lookahead())
                    .ok()
                    .and_then(char::from_u32)
                {
                    name.push(ch.to_ascii_lowercase());
                }
            } else {
                too_long = true;
            }
            self.advance(lexer);
        }
        if name.is_empty() {
            return false;
        }
        let mut tag_closed = false;
        if !too_long {
            let c = lexer.lookahead();
            let next_symbol_valid = is_blank(c) || is_newline(c) || is(c, '>');
            if next_symbol_valid && HTML_TAG_NAMES_RULE_1.contains(&name.as_str()) {
                if starting_slash {
                    if valid(Token::HtmlBlock1End) {
                        lexer.set_result(Token::HtmlBlock1End.symbol());
                        return true;
                    }
                } else if valid(Token::HtmlBlock1Start) {
                    return start(self, lexer, Token::HtmlBlock1Start);
                }
            }
            if !next_symbol_valid && is(c, '/') {
                self.advance(lexer);
                if is(lexer.lookahead(), '>') {
                    self.advance(lexer);
                    tag_closed = true;
                }
            }
            if (next_symbol_valid || tag_closed)
                && HTML_TAG_NAMES_RULE_7.contains(&name.as_str())
                && valid(Token::HtmlBlock6Start)
            {
                return start(self, lexer, Token::HtmlBlock6Start);
            }
        }
        if !valid(Token::HtmlBlock7Start) {
            return false;
        }
        if !tag_closed {
            // The rest of the tag's name.
            while is_ascii_alnum(lexer.lookahead()) || is(lexer.lookahead(), '-') {
                self.advance(lexer);
            }
            if starting_slash {
                while is_blank(lexer.lookahead()) {
                    self.advance(lexer);
                }
            } else if !self.parse_html_attributes(lexer) {
                return false;
            }
            if !is(lexer.lookahead(), '>') {
                return false;
            }
            self.advance(lexer);
        }
        while is_blank(lexer.lookahead()) {
            self.advance(lexer);
        }
        if is_newline(lexer.lookahead()) {
            return start(self, lexer, Token::HtmlBlock7Start);
        }
        false
    }

    /// An open tag's attributes, up to its `>` or `/`: whether they are
    /// well formed.
    fn parse_html_attributes(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let mut had_whitespace = false;
        loop {
            while is_blank(lexer.lookahead()) {
                had_whitespace = true;
                self.advance(lexer);
            }
            if is(lexer.lookahead(), '/') {
                self.advance(lexer);
                return true;
            }
            if is(lexer.lookahead(), '>') {
                return true;
            }
            // An attribute's name.
            if !had_whitespace {
                return false;
            }
            let c = lexer.lookahead();
            if !is_ascii_alpha(c) && !is(c, '_') && !is(c, ':') {
                return false;
            }
            had_whitespace = false;
            self.advance(lexer);
            while {
                let c = lexer.lookahead();
                is_ascii_alnum(c) || is(c, '_') || is(c, '.') || is(c, ':') || is(c, '-')
            } {
                self.advance(lexer);
            }
            while is_blank(lexer.lookahead()) {
                had_whitespace = true;
                self.advance(lexer);
            }
            // Its value.
            if is(lexer.lookahead(), '=') {
                self.advance(lexer);
                had_whitespace = false;
                while is_blank(lexer.lookahead()) {
                    self.advance(lexer);
                }
                let c = lexer.lookahead();
                if is(c, '\'') || is(c, '"') {
                    let delimiter = c;
                    self.advance(lexer);
                    while lexer.lookahead() != delimiter
                        && !is_newline(lexer.lookahead())
                        && !lexer.eof()
                    {
                        self.advance(lexer);
                    }
                    if lexer.lookahead() != delimiter {
                        return false;
                    }
                    self.advance(lexer);
                } else {
                    let mut had_one = false;
                    while {
                        // An unquoted value runs to a blank, a line's end,
                        // a quote, `=`, `<`, `>` or a backtick.
                        let c = lexer.lookahead();
                        let ends = is_blank(c)
                            || is_newline(c)
                            || "\"'=<>`".chars().any(|q| is(c, q))
                            || lexer.eof();
                        !ends
                    } {
                        self.advance(lexer);
                        had_one = true;
                    }
                    if !had_one {
                        return false;
                    }
                }
            }
        }
    }

    /// A pipe table's header row, if the next line is its delimiter row with
    /// as many cells (`PIPE_TABLE_START`, zero width).
    fn parse_pipe_table(&mut self, lexer: &mut Lexer<'_>) -> bool {
        self.mark_end(lexer);
        let mut cell_count: usize = 0;
        let mut starting_pipe = false;
        let mut ending_pipe = false;
        if is(lexer.lookahead(), '|') {
            starting_pipe = true;
            self.advance(lexer);
        }
        while !is_newline(lexer.lookahead()) && !lexer.eof() {
            let c = lexer.lookahead();
            if is(c, '|') {
                cell_count = cell_count.saturating_add(1);
                ending_pipe = true;
                self.advance(lexer);
            } else {
                if !is_blank(c) {
                    ending_pipe = false;
                }
                self.advance(lexer);
                if is(c, '\\') && is_punctuation(lexer.lookahead()) {
                    self.advance(lexer);
                }
            }
        }
        if cell_count == 0 && !(starting_pipe && ending_pipe) {
            return false;
        }
        if !ending_pipe {
            cell_count = cell_count.saturating_add(1);
        }
        // The next line.
        if !is_newline(lexer.lookahead()) {
            return false;
        }
        self.advance_newline(lexer);
        self.indentation = 0;
        self.column = 0;
        while is_blank(lexer.lookahead()) {
            let n = self.advance(lexer);
            self.indentation = self.indentation.wrapping_add(n);
        }
        self.simulate = true;
        let mut matched_temp: u8 = 0;
        while matched_temp < self.open_count() {
            let block = self
                .open_blocks
                .get(usize::from(matched_temp))
                .copied()
                .unwrap_or(ANONYMOUS);
            if !self.match_block(lexer, block) {
                return false;
            }
            matched_temp = matched_temp.wrapping_add(1);
        }
        // The delimiter row: as many cells, and at least one pipe.
        let mut delimiter_cell_count: usize = 0;
        if is(lexer.lookahead(), '|') {
            self.advance(lexer);
        }
        loop {
            while is_blank(lexer.lookahead()) {
                self.advance(lexer);
            }
            if is(lexer.lookahead(), '|') {
                delimiter_cell_count = delimiter_cell_count.saturating_add(1);
                self.advance(lexer);
                continue;
            }
            if is(lexer.lookahead(), ':') {
                self.advance(lexer);
                if !is(lexer.lookahead(), '-') {
                    return false;
                }
            }
            let mut had_one_minus = false;
            while is(lexer.lookahead(), '-') {
                had_one_minus = true;
                self.advance(lexer);
            }
            if had_one_minus {
                delimiter_cell_count = delimiter_cell_count.saturating_add(1);
            }
            if is(lexer.lookahead(), ':') {
                if !had_one_minus {
                    return false;
                }
                self.advance(lexer);
            }
            while is_blank(lexer.lookahead()) {
                self.advance(lexer);
            }
            if is(lexer.lookahead(), '|') {
                if !had_one_minus {
                    delimiter_cell_count = delimiter_cell_count.saturating_add(1);
                }
                self.advance(lexer);
                continue;
            }
            if !is_newline(lexer.lookahead()) {
                return false;
            }
            break;
        }
        if cell_count != delimiter_cell_count {
            return false;
        }
        lexer.set_result(Token::PipeTableStart.symbol());
        true
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "scanner.c's `scan`, ported branch for branch so it can be checked against the C"
    )]
    fn scan_with(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // A grammar rule asked for an error, to end a branch.
        if valid(Token::TriggerError) {
            lexer.set_result(Token::Error.symbol());
            return true;
        }
        // Close the innermost block after the next line break, as asked.
        if valid(Token::CloseBlock) {
            self.state |= STATE_CLOSE_BLOCK;
            lexer.set_result(Token::CloseBlock.symbol());
            return true;
        }
        // At the end, close every block still open.
        if lexer.eof() {
            if valid(Token::Eof) {
                lexer.set_result(Token::Eof.symbol());
                return true;
            }
            if !self.open_blocks.is_empty() {
                lexer.set_result(Token::BlockClose.symbol());
                if !self.simulate {
                    self.pop_block();
                }
                return true;
            }
            return false;
        }

        if self.state & STATE_MATCHING == 0 {
            // Not matching: new blocks start here. The leading blanks first.
            while is_blank(lexer.lookahead()) {
                let n = self.advance(lexer);
                self.indentation = self.indentation.wrapping_add(n);
            }
            if valid(Token::IndentedChunkStart)
                && !valid(Token::NoIndentedChunk)
                && self.indentation >= 4
                && !is_newline(lexer.lookahead())
            {
                lexer.set_result(Token::IndentedChunkStart.symbol());
                if !self.simulate {
                    self.push_block(INDENTED_CODE_BLOCK);
                }
                self.indentation = self.indentation.saturating_sub(4);
                return true;
            }
            let c = lexer.lookahead();
            if is_newline(c) {
                if valid(Token::BlankLineStart) {
                    // Zero width: the line break is not consumed.
                    lexer.set_result(Token::BlankLineStart.symbol());
                    return true;
                }
            } else if is(c, '`') {
                return self.parse_fenced_code_block('`', lexer, &valid);
            } else if is(c, '~') {
                return self.parse_fenced_code_block('~', lexer, &valid);
            } else if is(c, '*') {
                return self.parse_star(lexer, &valid);
            } else if is(c, '_') {
                return self.parse_thematic_break_underscore(lexer, &valid);
            } else if is(c, '>') {
                return self.parse_block_quote(lexer, &valid);
            } else if is(c, '#') {
                return self.parse_atx_heading(lexer, &valid);
            } else if is(c, '=') {
                return self.parse_setext_underline(lexer, &valid);
            } else if is(c, '+') {
                return self.parse_plus(lexer, &valid);
            } else if is_digit(c) {
                return self.parse_ordered_list_marker(lexer, &valid);
            } else if is(c, '-') {
                return self.parse_minus(lexer, &valid);
            } else if is(c, '<') {
                return self.parse_html_block(lexer, &valid);
            }
            if !is_newline(lexer.lookahead()) && valid(Token::PipeTableStart) {
                return self.parse_pipe_table(lexer);
            }
        } else {
            // Matching the blocks open at the start of this line.
            let mut partial_success = false;
            while self.matched < self.open_count() {
                if i32::from(self.matched) == i32::from(self.open_count()).saturating_sub(1)
                    && self.state & STATE_CLOSE_BLOCK != 0
                {
                    if !partial_success {
                        self.state &= !STATE_CLOSE_BLOCK;
                    }
                    break;
                }
                let block = self
                    .open_blocks
                    .get(usize::from(self.matched))
                    .copied()
                    .unwrap_or(ANONYMOUS);
                if self.match_block(lexer, block) {
                    partial_success = true;
                    self.matched = self.matched.wrapping_add(1);
                } else {
                    if self.state & STATE_WAS_SOFT_LINE_BREAK != 0 {
                        self.state &= !STATE_MATCHING;
                    }
                    break;
                }
            }
            if partial_success {
                if self.matched == self.open_count() {
                    self.state &= !STATE_MATCHING;
                }
                lexer.set_result(Token::BlockContinuation.symbol());
                return true;
            }
            if self.state & STATE_WAS_SOFT_LINE_BREAK == 0 {
                lexer.set_result(Token::BlockClose.symbol());
                self.pop_block();
                if self.matched == self.open_count() {
                    self.state &= !STATE_MATCHING;
                }
                return true;
            }
        }

        // A line break: set the state up for the next line.
        if (valid(Token::LineEnding)
            || valid(Token::SoftLineEnding)
            || valid(Token::PipeTableLineEnding))
            && is_newline(lexer.lookahead())
        {
            self.advance_newline(lexer);
            self.indentation = 0;
            self.column = 0;
            if self.state & STATE_CLOSE_BLOCK == 0
                && (valid(Token::SoftLineEnding) || valid(Token::PipeTableLineEnding))
            {
                lexer.mark_end();
                while is_blank(lexer.lookahead()) {
                    let n = self.advance(lexer);
                    self.indentation = self.indentation.wrapping_add(n);
                }
                // What would the next line start? Asked by simulating,
                // which opens no blocks.
                self.simulate = true;
                let matched_temp = self.matched;
                self.matched = 0;
                let mut one_will_be_matched = false;
                while self.matched < self.open_count() {
                    let block = self
                        .open_blocks
                        .get(usize::from(self.matched))
                        .copied()
                        .unwrap_or(ANONYMOUS);
                    if self.match_block(lexer, block) {
                        self.matched = self.matched.wrapping_add(1);
                        one_will_be_matched = true;
                    } else {
                        break;
                    }
                }
                let all_will_be_matched = self.matched == self.open_count();
                if !lexer.eof() && !self.scan_with(lexer, &PARAGRAPH_INTERRUPT_SYMBOLS) {
                    // Nothing interrupts: the line break was soft.
                    self.matched = 0;
                    self.indentation = 0;
                    self.column = 0;
                    if one_will_be_matched {
                        self.state |= STATE_MATCHING;
                    } else {
                        self.state &= !STATE_MATCHING;
                    }
                    if valid(Token::PipeTableLineEnding) {
                        if all_will_be_matched {
                            lexer.set_result(Token::PipeTableLineEnding.symbol());
                            return true;
                        }
                    } else {
                        lexer.set_result(Token::SoftLineEnding.symbol());
                        self.state |= STATE_WAS_SOFT_LINE_BREAK;
                        return true;
                    }
                } else {
                    self.matched = matched_temp;
                }
                self.indentation = 0;
                self.column = 0;
            }
            if valid(Token::LineEnding) {
                self.matched = 0;
                if self.open_blocks.is_empty() {
                    self.state &= !STATE_MATCHING;
                } else {
                    self.state |= STATE_MATCHING;
                }
                self.state &= !STATE_WAS_SOFT_LINE_BREAK;
                lexer.set_result(Token::LineEnding.symbol());
                return true;
            }
        }
        false
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_line_ending",
        "_soft_line_ending",
        "_block_close",
        "block_continuation",
        "_block_quote_start",
        "_indented_chunk_start",
        "atx_h1_marker",
        "atx_h2_marker",
        "atx_h3_marker",
        "atx_h4_marker",
        "atx_h5_marker",
        "atx_h6_marker",
        "setext_h1_underline",
        "setext_h2_underline",
        "_thematic_break",
        "_list_marker_minus",
        "_list_marker_plus",
        "_list_marker_star",
        "_list_marker_parenthesis",
        "_list_marker_dot",
        "_list_marker_minus_dont_interrupt",
        "_list_marker_plus_dont_interrupt",
        "_list_marker_star_dont_interrupt",
        "_list_marker_parenthesis_dont_interrupt",
        "_list_marker_dot_dont_interrupt",
        "_fenced_code_block_start_backtick",
        "_fenced_code_block_start_tilde",
        "_blank_line_start",
        "_fenced_code_block_end_backtick",
        "_fenced_code_block_end_tilde",
        "_html_block_1_start",
        "_html_block_1_end",
        "_html_block_2_start",
        "_html_block_3_start",
        "_html_block_4_start",
        "_html_block_5_start",
        "_html_block_6_start",
        "_html_block_7_start",
        "_close_block",
        "_no_indented_chunk",
        "_error",
        "_trigger_error",
        "_eof",
        "minus_metadata",
        "plus_metadata",
        "_pipe_table_start",
        "_pipe_table_line_ending",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid: &[bool]) -> bool {
        self.simulate = false;
        self.scan_with(lexer, valid)
    }

    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let head = [
            self.state,
            self.matched,
            self.indentation,
            self.column,
            self.fenced_code_block_delimiter_length,
        ];
        let Some(slot) = buffer.get_mut(..head.len()) else {
            return 0;
        };
        slot.copy_from_slice(&head);
        // The open blocks, a byte each, as many as there is room for.
        let rest = buffer.get_mut(head.len()..).unwrap_or_default();
        let n = rest.len().min(self.open_blocks.len());
        if let (Some(dst), Some(src)) = (rest.get_mut(..n), self.open_blocks.get(..n)) {
            dst.copy_from_slice(src);
        }
        head.len().saturating_add(n)
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.open_blocks.clear();
        self.state = 0;
        self.matched = 0;
        self.indentation = 0;
        self.column = 0;
        self.fenced_code_block_delimiter_length = 0;
        if let [state, matched, indentation, column, fenced, blocks @ ..] = bytes {
            self.state = *state;
            self.matched = *matched;
            self.indentation = *indentation;
            self.column = *column;
            self.fenced_code_block_delimiter_length = *fenced;
            self.open_blocks.extend_from_slice(blocks);
        }
    }
}
