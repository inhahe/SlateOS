//! Python: tree-sitter-python 0.25.0 (MIT, Max Brunsfeld and the
//! tree-sitter contributors). `grammars/python/` holds its `parser.c`, its
//! highlight query, its test corpus and its `scanner.c` as published; the
//! scanner is ported below: indentation (indent and dedent tokens), string
//! delimiters with their prefixes, and the escapes of an f-string's braces.
//!
//! # Where the port differs
//!
//! - **Its tokens are named in the grammar's order.** The C's enum calls
//!   token 8 `CLOSE_PAREN` and 9 `CLOSE_BRACKET`, where the grammar has `]`
//!   then `)`; the C only ever asks whether any of the three closers is
//!   valid, so nothing changed but the names.
//! - **Saving never writes past its buffer.** The C writes an indent's two
//!   bytes after checking there is room for one, so a stack deep enough to
//!   fill the buffer wrote a byte past it; here the last indent that does not
//!   fit whole is left out, as the ones after it already were.
//! - **Reading a saved state never reads past it.** The C reads a delimiter
//!   count even from a one-byte state; here what is not there is none.
//! - **No `advanced_once`.** The C sets it after stepping over an f-string
//!   brace and then returns on both branches, so the string scan that reads it
//!   only ever sees it false; the flag is gone and the scan starts with no
//!   content, as it always did.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("python", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/python/highlights.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    Newline,
    Indent,
    Dedent,
    StringStart,
    StringContent,
    EscapeInterpolation,
    StringEnd,
    // `comment`, which the grammar lists and the scanner never produces.
    _Comment,
    CloseBracket,
    CloseParen,
    CloseBrace,
    Except,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// A string's delimiter: which quote, and its prefixes, as flag bits (one
/// byte each, which is also how they are saved).
mod flags {
    pub(super) const SINGLE_QUOTE: u8 = 1 << 0;
    pub(super) const DOUBLE_QUOTE: u8 = 1 << 1;
    pub(super) const BACK_QUOTE: u8 = 1 << 2;
    pub(super) const RAW: u8 = 1 << 3;
    pub(super) const FORMAT: u8 = 1 << 4;
    pub(super) const TRIPLE: u8 = 1 << 5;
    pub(super) const BYTES: u8 = 1 << 6;
}

fn is_format(d: u8) -> bool {
    d & flags::FORMAT != 0
}

fn is_raw(d: u8) -> bool {
    d & flags::RAW != 0
}

fn is_triple(d: u8) -> bool {
    d & flags::TRIPLE != 0
}

fn is_bytes(d: u8) -> bool {
    d & flags::BYTES != 0
}

/// The character that ends a string with delimiter `d`; 0 for none.
fn end_character(d: u8) -> i32 {
    if d & flags::SINGLE_QUOTE != 0 {
        i32::from(b'\'')
    } else if d & flags::DOUBLE_QUOTE != 0 {
        i32::from(b'"')
    } else if d & flags::BACK_QUOTE != 0 {
        i32::from(b'`')
    } else {
        0
    }
}

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// The scanner's state: the indentation of each open block, the delimiters
/// of the strings it is inside, and whether the innermost is an f-string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scanner {
    indents: Vec<u16>,
    delimiters: Vec<u8>,
    inside_interpolated_string: bool,
}

impl Default for Scanner {
    /// What the C's `create` makes: one indent of 0, no strings.
    fn default() -> Self {
        Self {
            indents: vec![0],
            delimiters: Vec::new(),
            inside_interpolated_string: false,
        }
    }
}

impl Scanner {
    /// Inside a string: its content, an escape's start, or its end.
    /// `None` when the C would go on to the indentation.
    fn scan_string(&mut self, lexer: &mut Lexer<'_>) -> Option<bool> {
        let &delimiter = self.delimiters.last()?;
        let end_char = end_character(delimiter);
        let mut has_content = false;
        while lexer.lookahead() != 0 {
            let c = lexer.lookahead();
            if (is(c, '{') || is(c, '}')) && is_format(delimiter) {
                lexer.mark_end();
                lexer.set_result(Token::StringContent.symbol());
                return Some(has_content);
            }
            if is(c, '\\') {
                if is_raw(delimiter) {
                    // Over the backslash, and a quote or backslash it escapes.
                    lexer.advance();
                    let c = lexer.lookahead();
                    if c == end_character(delimiter) || is(c, '\\') {
                        lexer.advance();
                    }
                    // And over a line break.
                    if is(lexer.lookahead(), '\r') {
                        lexer.advance();
                        if is(lexer.lookahead(), '\n') {
                            lexer.advance();
                        }
                    } else if is(lexer.lookahead(), '\n') {
                        lexer.advance();
                    }
                    continue;
                }
                if is_bytes(delimiter) {
                    lexer.mark_end();
                    lexer.advance();
                    let c = lexer.lookahead();
                    // `\N{...}`, `\u` and `\U` are not escapes in bytes.
                    if is(c, 'N') || is(c, 'u') || is(c, 'U') {
                        lexer.advance();
                    } else {
                        lexer.set_result(Token::StringContent.symbol());
                        return Some(has_content);
                    }
                } else {
                    lexer.mark_end();
                    lexer.set_result(Token::StringContent.symbol());
                    return Some(has_content);
                }
            } else if c == end_char {
                if is_triple(delimiter) {
                    lexer.mark_end();
                    lexer.advance();
                    if lexer.lookahead() == end_char {
                        lexer.advance();
                        if lexer.lookahead() == end_char {
                            if has_content {
                                lexer.set_result(Token::StringContent.symbol());
                            } else {
                                lexer.advance();
                                lexer.mark_end();
                                self.delimiters.pop();
                                lexer.set_result(Token::StringEnd.symbol());
                                self.inside_interpolated_string = false;
                            }
                            return Some(true);
                        }
                        lexer.mark_end();
                        lexer.set_result(Token::StringContent.symbol());
                        return Some(true);
                    }
                    lexer.mark_end();
                    lexer.set_result(Token::StringContent.symbol());
                    return Some(true);
                }
                if has_content {
                    lexer.set_result(Token::StringContent.symbol());
                } else {
                    lexer.advance();
                    self.delimiters.pop();
                    lexer.set_result(Token::StringEnd.symbol());
                    self.inside_interpolated_string = false;
                }
                lexer.mark_end();
                return Some(true);
            } else if is(c, '\n') && has_content && !is_triple(delimiter) {
                return Some(false);
            }
            lexer.advance();
            has_content = true;
        }
        None
    }

    /// A string's opening: its prefixes and quotes.
    fn scan_string_start(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let mut delimiter: u8 = 0;
        while lexer.lookahead() != 0 {
            let c = lexer.lookahead();
            if is(c, 'f') || is(c, 'F') || is(c, 't') || is(c, 'T') {
                delimiter |= flags::FORMAT;
            } else if is(c, 'r') || is(c, 'R') {
                delimiter |= flags::RAW;
            } else if is(c, 'b') || is(c, 'B') {
                delimiter |= flags::BYTES;
            } else if !is(c, 'u') && !is(c, 'U') {
                break;
            }
            lexer.advance();
        }
        let c = lexer.lookahead();
        if is(c, '`') {
            delimiter |= flags::BACK_QUOTE;
            lexer.advance();
            lexer.mark_end();
        } else if is(c, '\'') || is(c, '"') {
            delimiter |= if is(c, '\'') {
                flags::SINGLE_QUOTE
            } else {
                flags::DOUBLE_QUOTE
            };
            lexer.advance();
            lexer.mark_end();
            if lexer.lookahead() == c {
                lexer.advance();
                if lexer.lookahead() == c {
                    lexer.advance();
                    lexer.mark_end();
                    delimiter |= flags::TRIPLE;
                }
            }
        }
        if end_character(delimiter) != 0 {
            self.delimiters.push(delimiter);
            lexer.set_result(Token::StringStart.symbol());
            self.inside_interpolated_string = is_format(delimiter);
            return true;
        }
        // Prefix letters with no quote after them are a name, not a string.
        false
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_newline",
        "_indent",
        "_dedent",
        "string_start",
        "_string_content",
        "escape_interpolation",
        "string_end",
        "comment",
        "RBRACK",
        "RPAREN",
        "RBRACE",
        "except",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        let error_recovery_mode = valid(Token::StringContent) && valid(Token::Indent);
        let within_brackets =
            valid(Token::CloseBrace) || valid(Token::CloseParen) || valid(Token::CloseBracket);

        let c = lexer.lookahead();
        if valid(Token::EscapeInterpolation)
            && (is(c, '{') || is(c, '}'))
            && !error_recovery_mode
            && self.delimiters.last().is_some_and(|&d| is_format(d))
        {
            lexer.mark_end();
            let is_left_brace = is(c, '{');
            lexer.advance();
            let next = lexer.lookahead();
            if (is(next, '{') && is_left_brace) || (is(next, '}') && !is_left_brace) {
                lexer.advance();
                lexer.mark_end();
                lexer.set_result(Token::EscapeInterpolation.symbol());
                return true;
            }
            return false;
        }

        if valid(Token::StringContent)
            && !error_recovery_mode
            && let Some(found) = self.scan_string(lexer)
        {
            return found;
        }

        lexer.mark_end();

        let mut found_end_of_line = false;
        let mut indent_length: u16 = 0;
        let mut first_comment_indent_length: i32 = -1;
        loop {
            let c = lexer.lookahead();
            if is(c, '\n') {
                found_end_of_line = true;
                indent_length = 0;
                lexer.skip();
            } else if is(c, ' ') {
                indent_length = indent_length.wrapping_add(1);
                lexer.skip();
            } else if is(c, '\r') || c == 0x0c {
                indent_length = 0;
                lexer.skip();
            } else if is(c, '\t') {
                indent_length = indent_length.wrapping_add(8);
                lexer.skip();
            } else if is(c, '#')
                && (valid(Token::Indent)
                    || valid(Token::Dedent)
                    || valid(Token::Newline)
                    || valid(Token::Except))
            {
                // A comment after code on its line (`x = 1  # why`) makes no
                // indentation token.
                if !found_end_of_line {
                    return false;
                }
                if first_comment_indent_length == -1 {
                    first_comment_indent_length = i32::from(indent_length);
                }
                while lexer.lookahead() != 0 && !is(lexer.lookahead(), '\n') {
                    lexer.skip();
                }
                lexer.skip();
                indent_length = 0;
            } else if is(c, '\\') {
                lexer.skip();
                if is(lexer.lookahead(), '\r') {
                    lexer.skip();
                }
                if is(lexer.lookahead(), '\n') || lexer.eof() {
                    lexer.skip();
                } else {
                    return false;
                }
            } else if lexer.eof() {
                indent_length = 0;
                found_end_of_line = true;
                break;
            } else {
                break;
            }
        }

        if found_end_of_line {
            if let Some(&current_indent_length) = self.indents.last() {
                if valid(Token::Indent) && indent_length > current_indent_length {
                    self.indents.push(indent_length);
                    lexer.set_result(Token::Indent.symbol());
                    return true;
                }
                let c = lexer.lookahead();
                let next_tok_is_string_start = is(c, '"') || is(c, '\'') || is(c, '`');
                // The C's `!newline && !(string_start && quote) && !brackets`,
                // as De Morgan has it.
                if (valid(Token::Dedent)
                    || !(valid(Token::Newline)
                        || within_brackets
                        || (valid(Token::StringStart) && next_tok_is_string_start)))
                    && indent_length < current_indent_length
                    && !self.inside_interpolated_string
                    // Wait to dedent until the comments indented as the
                    // block is are consumed.
                    && first_comment_indent_length < i32::from(current_indent_length)
                {
                    self.indents.pop();
                    lexer.set_result(Token::Dedent.symbol());
                    return true;
                }
            }
            if valid(Token::Newline) && !error_recovery_mode {
                lexer.set_result(Token::Newline.symbol());
                return true;
            }
        }

        if first_comment_indent_length == -1 && valid(Token::StringStart) {
            return self.scan_string_start(lexer);
        }
        false
    }

    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let delimiter_count = self.delimiters.len().min(usize::from(u8::MAX));
        let Some(head) = buffer.get_mut(..2usize.saturating_add(delimiter_count)) else {
            return 0;
        };
        if let [inside, count, rest @ ..] = head {
            *inside = u8::from(self.inside_interpolated_string);
            *count = u8::try_from(delimiter_count).unwrap_or(u8::MAX);
            for (slot, d) in rest.iter_mut().zip(&self.delimiters) {
                *slot = *d;
            }
        }
        let mut size = 2usize.saturating_add(delimiter_count);
        // Every indent but the first, which is always 0.
        for indent in self.indents.iter().skip(1) {
            let Some(pair) = buffer.get_mut(size..size.saturating_add(2)) else {
                break;
            };
            pair.copy_from_slice(&indent.to_le_bytes());
            size = size.saturating_add(2);
        }
        size
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.delimiters.clear();
        self.indents.clear();
        self.indents.push(0);
        // An empty state leaves `inside_interpolated_string` as it was, as
        // the C does.
        let Some((&inside, rest)) = bytes.split_first() else {
            return;
        };
        self.inside_interpolated_string = inside != 0;
        let Some((&count, rest)) = rest.split_first() else {
            return;
        };
        let count = usize::from(count).min(rest.len());
        let (delimiters, indents) = rest.split_at(count);
        self.delimiters.extend_from_slice(delimiters);
        for pair in indents.chunks_exact(2) {
            if let [low, high] = pair {
                self.indents.push(u16::from_le_bytes([*low, *high]));
            }
        }
    }
}
