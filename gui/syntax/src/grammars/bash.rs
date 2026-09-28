//! Bash: tree-sitter-bash 0.25.1 (MIT, Max Brunsfeld, Amaan Qureshi and the
//! tree-sitter contributors). `grammars/bash/` holds its `parser.c`, its
//! highlight query, its test corpus and its `scanner.c` as published; the
//! scanner -- heredocs, and the words, patterns and regular expressions a
//! shell reads by where they are -- is ported below function for function
//! under the C's names. The C's `goto`s jump forward into the sections at
//! the end of `scan`, which then run on into each other; here each section
//! is a method that ends by calling the next (`scan_regex` ->
//! `scan_extglob_pattern` -> `scan_expansion_word` -> `scan_brace_start`),
//! and a `goto` is a call.
//!
//! # Where the port differs
//!
//! - **A heredoc's delimiter is characters, not bytes.** The C keeps it in a
//!   `char` array, so a character past ASCII was cut to its low byte and
//!   compared, sign-extended, with the whole character ahead: a delimiter
//!   such as `FÍN` could never end its heredoc. Here it is the characters,
//!   and it is saved as such.
//! - **Blanks, letters and digits are ASCII**, as a shell's names and blanks
//!   are. The C asks `iswspace`, `iswalpha`, `iswalnum` and `iswdigit`, whose
//!   answers depend on the C library's locale, and hands a code point to
//!   `isdigit`, whose argument must be a byte.
//! - **Saving says so when it cannot.** The C writes a heredoc count of more
//!   than 255 as its low byte and the heredocs after it; here that state is
//!   not saved, as when the heredocs outgrow the buffer.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("bash", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/bash/highlights.scm");

/// The external tokens, in the grammar's order (`TokenType`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Token {
    HeredocStart,
    SimpleHeredocBody,
    HeredocBodyBeginning,
    HeredocContent,
    HeredocEnd,
    FileDescriptor,
    EmptyValue,
    Concat,
    VariableName,
    TestOperator,
    Regex,
    RegexNoSlash,
    RegexNoSpace,
    ExpansionWord,
    ExtglobPattern,
    BareDollar,
    BraceStart,
    ImmediateDoubleHash,
    ExternalExpansionSymHash,
    ExternalExpansionSymBang,
    ExternalExpansionSymEqual,
    ClosingBrace,
    ClosingBracket,
    HeredocArrow,
    HeredocArrowDash,
    Newline,
    OpeningParen,
    #[allow(
        dead_code,
        reason = "valid in `esac`'s place, never produced: its index is its place"
    )]
    Esac,
    ErrorRecovery,
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

/// `iswspace` in the C locale: space, tab, newline, vertical tab, form
/// feed, carriage return.
fn is_space(c: i32) -> bool {
    c == 0x20 || (0x09..=0x0d).contains(&c)
}

fn is_digit(c: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&c)
}

fn is_alpha(c: i32) -> bool {
    u8::try_from(c).is_ok_and(|b| b.is_ascii_alphabetic())
}

fn is_alnum(c: i32) -> bool {
    is_alpha(c) || is_digit(c)
}

/// A depth's low byte: the C stores a `uint32_t` in a `uint8_t`.
fn low_byte(n: u32) -> u8 {
    n.to_le_bytes()[0]
}

/// One heredoc the scanner is inside, or about to be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Heredoc {
    /// Its delimiter was quoted or escaped: no expansions in its body.
    is_raw: bool,
    /// Its body has begun.
    started: bool,
    /// `<<-`: lines may be indented before the delimiter.
    allows_indent: bool,
    /// The word that ends it, as characters; empty until it is read.
    delimiter: Vec<i32>,
    /// The start of the line being read, as far as it matches `delimiter`.
    current_leading_word: Vec<i32>,
}

impl Heredoc {
    /// `reset_heredoc`: the leading word is left as it is, as the C leaves
    /// it.
    fn reset(&mut self) {
        self.is_raw = false;
        self.started = false;
        self.allows_indent = false;
        self.delimiter.clear();
    }
}

/// The scanner's state: the heredocs open, innermost last, and the paren
/// depth an extglob pattern was left at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    last_glob_paren_depth: u8,
    /// Saved and restored as the C does; read by nothing in this version.
    ext_was_in_double_quote: bool,
    /// Saved and restored as the C does; read by nothing in this version.
    ext_saw_outside_quote: bool,
    heredocs: Vec<Heredoc>,
}

/// Reading a saved state back: past its end, zeros.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> u8 {
        let b = self.bytes.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        b
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }

    fn i32(&mut self) -> i32 {
        i32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }

    /// How many bytes are left to read.
    fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
}

/// Whether `t` is valid here.
fn valid(valid_symbols: &[bool], t: Token) -> bool {
    valid_symbols
        .get(usize::from(t.symbol()))
        .copied()
        .unwrap_or(false)
}

/// `in_error_recovery`: the parser asks for every token at once.
fn in_error_recovery(valid_symbols: &[bool]) -> bool {
    valid(valid_symbols, Token::ErrorRecovery)
}

/// Consume a "word" in POSIX's sense, pushing it onto `unquoted_word`
/// unquoted (`advance_word`). An approximation, as the C says: no
/// substitutions, and the default `IFS`.
fn advance_word(lexer: &mut Lexer<'_>, unquoted_word: &mut Vec<i32>) -> bool {
    let mut empty = true;
    let mut quote = 0;
    if is(lexer.lookahead(), '\'') || is(lexer.lookahead(), '"') {
        quote = lexer.lookahead();
        lexer.advance();
    }
    while lexer.lookahead() != 0 && {
        let c = lexer.lookahead();
        let ends = if quote == 0 {
            is_space(c)
        } else {
            c == quote || is(c, '\r') || is(c, '\n')
        };
        !ends
    } {
        if is(lexer.lookahead(), '\\') {
            lexer.advance();
            if lexer.lookahead() == 0 {
                return false;
            }
        }
        empty = false;
        unquoted_word.push(lexer.lookahead());
        lexer.advance();
    }
    if quote != 0 && lexer.lookahead() == quote {
        lexer.advance();
    }
    !empty
}

/// `$` on its own, before a blank, the end, or a quote.
fn scan_bare_dollar(lexer: &mut Lexer<'_>) -> bool {
    while is_space(lexer.lookahead()) && !is(lexer.lookahead(), '\n') && !lexer.eof() {
        lexer.skip();
    }
    if is(lexer.lookahead(), '$') {
        lexer.advance();
        lexer.set_result(Token::BareDollar.symbol());
        lexer.mark_end();
        return is_space(lexer.lookahead()) || lexer.eof() || is(lexer.lookahead(), '"');
    }
    false
}

/// The word after `<<`: the heredoc's delimiter.
fn scan_heredoc_start(heredoc: &mut Heredoc, lexer: &mut Lexer<'_>) -> bool {
    while is_space(lexer.lookahead()) {
        lexer.skip();
    }
    lexer.set_result(Token::HeredocStart.symbol());
    let c = lexer.lookahead();
    heredoc.is_raw = is(c, '\'') || is(c, '"') || is(c, '\\');
    let found_delimiter = advance_word(lexer, &mut heredoc.delimiter);
    if !found_delimiter {
        heredoc.delimiter.clear();
        return false;
    }
    found_delimiter
}

/// Whether the line ahead begins with the heredoc's delimiter -- begins
/// with: what follows it on the line is not looked at, as the C does not.
fn scan_heredoc_end_identifier(heredoc: &mut Heredoc, lexer: &mut Lexer<'_>) -> bool {
    heredoc.current_leading_word.clear();
    let mut size = 0;
    if !heredoc.delimiter.is_empty() {
        while lexer.lookahead() != 0
            && !is(lexer.lookahead(), '\n')
            && heredoc.delimiter.get(size) == Some(&lexer.lookahead())
        {
            heredoc.current_leading_word.push(lexer.lookahead());
            lexer.advance();
            size = size.saturating_add(1);
        }
    }
    !heredoc.delimiter.is_empty() && heredoc.current_leading_word == heredoc.delimiter
}

impl Scanner {
    /// A heredoc's body, up to an expansion (`middle_type`) or its end
    /// (`end_type`).
    fn scan_heredoc_content(
        &mut self,
        lexer: &mut Lexer<'_>,
        middle_type: Token,
        end_type: Token,
    ) -> bool {
        let mut did_advance = false;
        loop {
            let Some(heredoc) = self.heredocs.last_mut() else {
                return false;
            };
            let c = lexer.lookahead();
            if c == 0 {
                if lexer.eof() && did_advance {
                    heredoc.reset();
                    lexer.set_result(end_type.symbol());
                    return true;
                }
                return false;
            } else if is(c, '\\') {
                did_advance = true;
                lexer.advance();
                lexer.advance();
            } else if is(c, '$') {
                if heredoc.is_raw {
                    did_advance = true;
                    lexer.advance();
                    continue;
                }
                if did_advance {
                    lexer.mark_end();
                    lexer.set_result(middle_type.symbol());
                    heredoc.started = true;
                    lexer.advance();
                    let c = lexer.lookahead();
                    if is_alpha(c) || is(c, '{') || is(c, '(') {
                        return true;
                    }
                    continue;
                }
                if middle_type == Token::HeredocBodyBeginning && lexer.column() == 0 {
                    lexer.set_result(middle_type.symbol());
                    heredoc.started = true;
                    return true;
                }
                return false;
            } else if is(c, '\n') {
                if did_advance {
                    lexer.advance();
                } else {
                    lexer.skip();
                }
                did_advance = true;
                if heredoc.allows_indent {
                    while is_space(lexer.lookahead()) {
                        lexer.advance();
                    }
                }
                let result = if heredoc.started {
                    middle_type
                } else {
                    end_type
                };
                lexer.set_result(result.symbol());
                lexer.mark_end();
                if scan_heredoc_end_identifier(heredoc, lexer) {
                    if result == Token::HeredocEnd {
                        self.heredocs.pop();
                    }
                    return true;
                }
            } else {
                if lexer.column() == 0 {
                    // "an alternative is to check the starting column of
                    // the heredoc body and track that statefully"
                    while is_space(lexer.lookahead()) {
                        if did_advance {
                            lexer.advance();
                        } else {
                            lexer.skip();
                        }
                    }
                    if end_type != Token::SimpleHeredocBody {
                        lexer.set_result(middle_type.symbol());
                        if scan_heredoc_end_identifier(heredoc, lexer) {
                            return true;
                        }
                    }
                    if end_type == Token::SimpleHeredocBody {
                        lexer.set_result(end_type.symbol());
                        lexer.mark_end();
                        if scan_heredoc_end_identifier(heredoc, lexer) {
                            return true;
                        }
                    }
                }
                did_advance = true;
                lexer.advance();
            }
        }
    }

    /// `scan`, up to the C's `regex:` label.
    #[allow(
        clippy::too_many_lines,
        reason = "the C's scan, section for section; split further it would stop reading \
                  as the C does, which is how it is checked"
    )]
    fn scan_main(&mut self, lexer: &mut Lexer<'_>, vs: &[bool]) -> bool {
        let recovering = in_error_recovery(vs);
        if valid(vs, Token::Concat) && !recovering {
            let c = lexer.lookahead();
            if !(c == 0
                || is_space(c)
                || is(c, '>')
                || is(c, '<')
                || is(c, ')')
                || is(c, '(')
                || is(c, ';')
                || is(c, '&')
                || is(c, '|')
                || (is(c, '}') && valid(vs, Token::ClosingBrace))
                || (is(c, ']') && valid(vs, Token::ClosingBracket)))
            {
                lexer.set_result(Token::Concat.symbol());
                // So for a`b`, we want to return a concat. We check if the
                // 2nd backtick has whitespace after it, and if it does we
                // return concat.
                if is(c, '`') {
                    lexer.mark_end();
                    lexer.advance();
                    while !is(lexer.lookahead(), '`') && !lexer.eof() {
                        lexer.advance();
                    }
                    if lexer.eof() {
                        return false;
                    }
                    if is(lexer.lookahead(), '`') {
                        lexer.advance();
                    }
                    return is_space(lexer.lookahead()) || lexer.eof();
                }
                // Strings with expansions that contain escaped quotes or
                // backslashes need this to return a concat.
                if is(c, '\\') {
                    lexer.mark_end();
                    lexer.advance();
                    let c = lexer.lookahead();
                    if is(c, '"') || is(c, '\'') || is(c, '\\') {
                        return true;
                    }
                    if lexer.eof() {
                        return false;
                    }
                } else {
                    return true;
                }
            }
            if is_space(lexer.lookahead())
                && valid(vs, Token::ClosingBrace)
                && !valid(vs, Token::ExpansionWord)
            {
                lexer.set_result(Token::Concat.symbol());
                return true;
            }
        }

        if valid(vs, Token::ImmediateDoubleHash) && !recovering && is(lexer.lookahead(), '#') {
            // Advance two `#`s and make sure no `}` follows.
            lexer.mark_end();
            lexer.advance();
            if is(lexer.lookahead(), '#') {
                lexer.advance();
                if !is(lexer.lookahead(), '}') {
                    lexer.set_result(Token::ImmediateDoubleHash.symbol());
                    lexer.mark_end();
                    return true;
                }
            }
        }

        if valid(vs, Token::ExternalExpansionSymHash) && !recovering {
            let c = lexer.lookahead();
            if is(c, '#') || is(c, '=') || is(c, '!') {
                let token = if is(c, '#') {
                    Token::ExternalExpansionSymHash
                } else if is(c, '!') {
                    Token::ExternalExpansionSymBang
                } else {
                    Token::ExternalExpansionSymEqual
                };
                lexer.set_result(token.symbol());
                lexer.advance();
                lexer.mark_end();
                while is(lexer.lookahead(), '#')
                    || is(lexer.lookahead(), '=')
                    || is(lexer.lookahead(), '!')
                {
                    lexer.advance();
                }
                while is_space(lexer.lookahead()) {
                    lexer.skip();
                }
                return is(lexer.lookahead(), '}');
            }
        }

        if valid(vs, Token::EmptyValue) {
            let c = lexer.lookahead();
            if is_space(c) || lexer.eof() || is(c, ';') || is(c, '&') {
                lexer.set_result(Token::EmptyValue.symbol());
                return true;
            }
        }

        if (valid(vs, Token::HeredocBodyBeginning) || valid(vs, Token::SimpleHeredocBody))
            && self.heredocs.last().is_some_and(|h| !h.started)
            && !recovering
        {
            return self.scan_heredoc_content(
                lexer,
                Token::HeredocBodyBeginning,
                Token::SimpleHeredocBody,
            );
        }

        if valid(vs, Token::HeredocEnd)
            && let Some(heredoc) = self.heredocs.last_mut()
            && scan_heredoc_end_identifier(heredoc, lexer)
        {
            self.heredocs.pop();
            lexer.set_result(Token::HeredocEnd.symbol());
            return true;
        }

        if valid(vs, Token::HeredocContent)
            && self.heredocs.last().is_some_and(|h| h.started)
            && !recovering
        {
            return self.scan_heredoc_content(lexer, Token::HeredocContent, Token::HeredocEnd);
        }

        if valid(vs, Token::HeredocStart)
            && !recovering
            && let Some(heredoc) = self.heredocs.last_mut()
        {
            return scan_heredoc_start(heredoc, lexer);
        }

        if valid(vs, Token::TestOperator) && !valid(vs, Token::ExpansionWord) {
            while is_space(lexer.lookahead()) && !is(lexer.lookahead(), '\n') {
                lexer.skip();
            }
            if is(lexer.lookahead(), '\\') {
                if valid(vs, Token::ExtglobPattern) {
                    return self.scan_extglob_pattern(lexer, vs);
                }
                if valid(vs, Token::RegexNoSpace) {
                    return self.scan_regex(lexer, vs);
                }
                lexer.skip();
                if lexer.eof() {
                    return false;
                }
                if is(lexer.lookahead(), '\r') {
                    lexer.skip();
                    if is(lexer.lookahead(), '\n') {
                        lexer.skip();
                    }
                } else if is(lexer.lookahead(), '\n') {
                    lexer.skip();
                } else {
                    return false;
                }
                while is_space(lexer.lookahead()) {
                    lexer.skip();
                }
            }
            if is(lexer.lookahead(), '\n') && !valid(vs, Token::Newline) {
                lexer.skip();
                while is_space(lexer.lookahead()) {
                    lexer.skip();
                }
            }
            if is(lexer.lookahead(), '-') {
                lexer.advance();
                let mut advanced_once = false;
                while is_alpha(lexer.lookahead()) {
                    advanced_once = true;
                    lexer.advance();
                }
                if is_space(lexer.lookahead()) && advanced_once {
                    lexer.mark_end();
                    lexer.advance();
                    if is(lexer.lookahead(), '}') && valid(vs, Token::ClosingBrace) {
                        if valid(vs, Token::ExpansionWord) {
                            lexer.mark_end();
                            lexer.set_result(Token::ExpansionWord.symbol());
                            return true;
                        }
                        return false;
                    }
                    lexer.set_result(Token::TestOperator.symbol());
                    return true;
                }
                if is_space(lexer.lookahead()) && valid(vs, Token::ExtglobPattern) {
                    lexer.set_result(Token::ExtglobPattern.symbol());
                    return true;
                }
            }
            if valid(vs, Token::BareDollar) && !recovering && scan_bare_dollar(lexer) {
                return true;
            }
        }

        if (valid(vs, Token::VariableName)
            || valid(vs, Token::FileDescriptor)
            || valid(vs, Token::HeredocArrow))
            && !valid(vs, Token::RegexNoSlash)
            && !recovering
        {
            return self.scan_variable_name(lexer, vs);
        }

        if valid(vs, Token::BareDollar) && !recovering && scan_bare_dollar(lexer) {
            return true;
        }
        self.scan_regex(lexer, vs)
    }

    /// The C's block for a variable's name, a file descriptor, or a heredoc
    /// arrow -- which always answers, or jumps.
    fn scan_variable_name(&mut self, lexer: &mut Lexer<'_>, vs: &[bool]) -> bool {
        loop {
            let c = lexer.lookahead();
            if (is(c, ' ')
                || is(c, '\t')
                || is(c, '\r')
                || (is(c, '\n') && !valid(vs, Token::Newline)))
                && !valid(vs, Token::ExpansionWord)
            {
                lexer.skip();
            } else if is(c, '\\') {
                lexer.skip();
                if lexer.eof() {
                    lexer.mark_end();
                    lexer.set_result(Token::VariableName.symbol());
                    return true;
                }
                if is(lexer.lookahead(), '\r') {
                    lexer.skip();
                }
                if is(lexer.lookahead(), '\n') {
                    lexer.skip();
                } else {
                    if is(lexer.lookahead(), '\\') && valid(vs, Token::ExpansionWord) {
                        return Self::scan_expansion_word(lexer, vs);
                    }
                    return false;
                }
            } else {
                break;
            }
        }

        // No '*', '@', '?', '-', '$', '0', '_'.
        let c = lexer.lookahead();
        if !valid(vs, Token::ExpansionWord)
            && (is(c, '*') || is(c, '@') || is(c, '?') || is(c, '-') || is(c, '0') || is(c, '_'))
        {
            lexer.mark_end();
            lexer.advance();
            let c = lexer.lookahead();
            if is(c, '=')
                || is(c, '[')
                || is(c, ':')
                || is(c, '-')
                || is(c, '%')
                || is(c, '#')
                || is(c, '/')
            {
                return false;
            }
            if valid(vs, Token::ExtglobPattern) && is_space(c) {
                lexer.mark_end();
                lexer.set_result(Token::ExtglobPattern.symbol());
                return true;
            }
        }

        if valid(vs, Token::HeredocArrow) && is(lexer.lookahead(), '<') {
            lexer.advance();
            if is(lexer.lookahead(), '<') {
                lexer.advance();
                if is(lexer.lookahead(), '-') {
                    lexer.advance();
                    self.heredocs.push(Heredoc {
                        allows_indent: true,
                        ..Heredoc::default()
                    });
                    lexer.set_result(Token::HeredocArrowDash.symbol());
                } else if is(lexer.lookahead(), '<') || is(lexer.lookahead(), '=') {
                    return false;
                } else {
                    self.heredocs.push(Heredoc::default());
                    lexer.set_result(Token::HeredocArrow.symbol());
                }
                return true;
            }
            return false;
        }

        let mut is_number = true;
        let c = lexer.lookahead();
        if is_digit(c) {
            lexer.advance();
        } else if is_alpha(c) || is(c, '_') {
            is_number = false;
            lexer.advance();
        } else {
            if is(c, '{') {
                return Self::scan_brace_start(lexer, vs);
            }
            if valid(vs, Token::ExpansionWord) {
                return Self::scan_expansion_word(lexer, vs);
            }
            if valid(vs, Token::ExtglobPattern) {
                return self.scan_extglob_pattern(lexer, vs);
            }
            return false;
        }

        loop {
            let c = lexer.lookahead();
            if is_digit(c) {
                lexer.advance();
            } else if is_alpha(c) || is(c, '_') {
                is_number = false;
                lexer.advance();
            } else {
                break;
            }
        }

        let c = lexer.lookahead();
        if is_number && valid(vs, Token::FileDescriptor) && (is(c, '>') || is(c, '<')) {
            lexer.set_result(Token::FileDescriptor.symbol());
            return true;
        }

        if valid(vs, Token::VariableName) {
            if is(c, '+') {
                lexer.mark_end();
                lexer.advance();
                let c = lexer.lookahead();
                if is(c, '=') || is(c, ':') || valid(vs, Token::ClosingBrace) {
                    lexer.set_result(Token::VariableName.symbol());
                    return true;
                }
                return false;
            }
            if is(c, '/') {
                return false;
            }
            if is(c, '=')
                || is(c, '[')
                || (is(c, ':')
                    && !valid(vs, Token::ClosingBrace)
                    && !valid(vs, Token::OpeningParen))
                || is(c, '%')
                || (is(c, '#') && !is_number)
                || is(c, '@')
                || (is(c, '-') && valid(vs, Token::ClosingBrace))
            {
                lexer.mark_end();
                lexer.set_result(Token::VariableName.symbol());
                return true;
            }
            if is(c, '?') {
                lexer.mark_end();
                lexer.advance();
                lexer.set_result(Token::VariableName.symbol());
                return is_alpha(lexer.lookahead());
            }
        }
        false
    }

    /// The C's `regex:` section, then on into the extglob pattern's.
    #[allow(
        clippy::too_many_lines,
        reason = "one section of the C's scan, kept whole to read as the C does"
    )]
    fn scan_regex(&mut self, lexer: &mut Lexer<'_>, vs: &[bool]) -> bool {
        let wanted = valid(vs, Token::Regex)
            || valid(vs, Token::RegexNoSlash)
            || valid(vs, Token::RegexNoSpace);
        if !wanted || in_error_recovery(vs) {
            return self.scan_extglob_pattern(lexer, vs);
        }
        if valid(vs, Token::Regex) || valid(vs, Token::RegexNoSpace) {
            while is_space(lexer.lookahead()) {
                lexer.skip();
            }
        }
        let c = lexer.lookahead();
        let starts = (!is(c, '"') && !is(c, '\''))
            || ((is(c, '$') || is(c, '\'')) && valid(vs, Token::RegexNoSlash))
            || (is(c, '\'') && valid(vs, Token::RegexNoSpace));
        if !starts {
            return self.scan_extglob_pattern(lexer, vs);
        }

        if is(c, '$') && valid(vs, Token::RegexNoSlash) {
            lexer.mark_end();
            lexer.advance();
            if is(lexer.lookahead(), '(') {
                return false;
            }
        }
        lexer.mark_end();

        let mut done = false;
        let mut advanced_once = false;
        let mut found_non_alnumdollarunderdash = false;
        let mut last_was_escape = false;
        let mut in_single_quote = false;
        let mut paren_depth: u32 = 0;
        let mut bracket_depth: u32 = 0;
        let mut brace_depth: u32 = 0;
        while !done {
            if in_single_quote && is(lexer.lookahead(), '\'') {
                in_single_quote = false;
                lexer.advance();
                lexer.mark_end();
            }
            let c = lexer.lookahead();
            if is(c, '\\') {
                last_was_escape = true;
            } else if c == 0 {
                return false;
            } else if is(c, '(') {
                paren_depth = paren_depth.wrapping_add(1);
                last_was_escape = false;
            } else if is(c, '[') {
                bracket_depth = bracket_depth.wrapping_add(1);
                last_was_escape = false;
            } else if is(c, '{') {
                if !last_was_escape {
                    brace_depth = brace_depth.wrapping_add(1);
                }
                last_was_escape = false;
            } else if is(c, ')') {
                done |= paren_depth == 0;
                paren_depth = paren_depth.wrapping_sub(1);
                last_was_escape = false;
            } else if is(c, ']') {
                done |= bracket_depth == 0;
                bracket_depth = bracket_depth.wrapping_sub(1);
                last_was_escape = false;
            } else if is(c, '}') {
                done |= brace_depth == 0;
                brace_depth = brace_depth.wrapping_sub(1);
                last_was_escape = false;
            } else if is(c, '\'') {
                // Enter or leave a single-quoted string.
                in_single_quote = !in_single_quote;
                lexer.advance();
                advanced_once = true;
                last_was_escape = false;
                continue;
            } else {
                last_was_escape = false;
            }

            if done {
                continue;
            }
            if valid(vs, Token::Regex) {
                let was_space = !in_single_quote && is_space(lexer.lookahead());
                lexer.advance();
                advanced_once = true;
                if !was_space || paren_depth > 0 {
                    lexer.mark_end();
                }
            } else if valid(vs, Token::RegexNoSlash) {
                if is(lexer.lookahead(), '/') {
                    lexer.mark_end();
                    lexer.set_result(Token::RegexNoSlash.symbol());
                    return advanced_once;
                }
                if is(lexer.lookahead(), '\\') {
                    lexer.advance();
                    advanced_once = true;
                    if !lexer.eof() && !is(lexer.lookahead(), '[') && !is(lexer.lookahead(), '/') {
                        lexer.advance();
                        lexer.mark_end();
                    }
                } else {
                    let was_space = !in_single_quote && is_space(lexer.lookahead());
                    lexer.advance();
                    advanced_once = true;
                    if !was_space {
                        lexer.mark_end();
                    }
                }
            } else if valid(vs, Token::RegexNoSpace) {
                let c = lexer.lookahead();
                if is(c, '\\') {
                    found_non_alnumdollarunderdash = true;
                    lexer.advance();
                    if !lexer.eof() {
                        lexer.advance();
                    }
                } else if is(c, '$') {
                    lexer.mark_end();
                    lexer.advance();
                    // Do not read a command substitution.
                    if is(lexer.lookahead(), '(') {
                        return false;
                    }
                    // A `$` at the end is always a regular expression, as
                    // in `99999999$`.
                    if is_space(lexer.lookahead()) {
                        lexer.set_result(Token::RegexNoSpace.symbol());
                        lexer.mark_end();
                        return true;
                    }
                } else {
                    let was_space = !in_single_quote && is_space(c);
                    if was_space && paren_depth == 0 {
                        lexer.mark_end();
                        lexer.set_result(Token::RegexNoSpace.symbol());
                        return found_non_alnumdollarunderdash;
                    }
                    if !is_alnum(c) && !is(c, '$') && !is(c, '-') && !is(c, '_') {
                        found_non_alnumdollarunderdash = true;
                    }
                    lexer.advance();
                }
            }
        }

        let result = if valid(vs, Token::RegexNoSlash) {
            Token::RegexNoSlash
        } else if valid(vs, Token::RegexNoSpace) {
            Token::RegexNoSpace
        } else {
            Token::Regex
        };
        lexer.set_result(result.symbol());
        !valid(vs, Token::Regex) || advanced_once
    }

    /// The C's `extglob_pattern:` section, then on into the expansion
    /// word's.
    #[allow(
        clippy::too_many_lines,
        reason = "one section of the C's scan, kept whole to read as the C does"
    )]
    fn scan_extglob_pattern(&mut self, lexer: &mut Lexer<'_>, vs: &[bool]) -> bool {
        if !valid(vs, Token::ExtglobPattern) || in_error_recovery(vs) {
            return Self::scan_expansion_word(lexer, vs);
        }
        // First skip blanks, then look for ? * + @ !
        while is_space(lexer.lookahead()) {
            lexer.skip();
        }
        let c = lexer.lookahead();
        let begins = is(c, '?')
            || is(c, '*')
            || is(c, '+')
            || is(c, '@')
            || is(c, '!')
            || is(c, '-')
            || is(c, ')')
            || is(c, '\\')
            || is(c, '.')
            || is(c, '[')
            || is_alpha(c);
        if !begins {
            self.last_glob_paren_depth = 0;
            return false;
        }

        if is(c, '\\') {
            lexer.advance();
            let c = lexer.lookahead();
            if (is_space(c) || is(c, '"')) && !is(c, '\r') && !is(c, '\n') {
                lexer.advance();
            } else {
                return false;
            }
        }

        if is(lexer.lookahead(), ')') && self.last_glob_paren_depth == 0 {
            lexer.mark_end();
            lexer.advance();
            if is_space(lexer.lookahead()) {
                return false;
            }
        }

        lexer.mark_end();
        let was_non_alpha = !is_alpha(lexer.lookahead());
        if !is(lexer.lookahead(), '[') {
            // No `esac`.
            if is(lexer.lookahead(), 'e') {
                lexer.mark_end();
                lexer.advance();
                if is(lexer.lookahead(), 's') {
                    lexer.advance();
                    if is(lexer.lookahead(), 'a') {
                        lexer.advance();
                        if is(lexer.lookahead(), 'c') {
                            lexer.advance();
                            if is_space(lexer.lookahead()) {
                                return false;
                            }
                        }
                    }
                }
            } else {
                lexer.advance();
            }
        }

        // `-\w` is just a word: look for something else special.
        if is(lexer.lookahead(), '-') {
            lexer.mark_end();
            lexer.advance();
            while is_alnum(lexer.lookahead()) {
                lexer.advance();
            }
            let c = lexer.lookahead();
            if is(c, ')') || is(c, '\\') || is(c, '.') {
                return false;
            }
            lexer.mark_end();
        }

        // A case item: `-)` or `*)`.
        if is(lexer.lookahead(), ')') && self.last_glob_paren_depth == 0 {
            lexer.mark_end();
            lexer.advance();
            if is_space(lexer.lookahead()) {
                lexer.set_result(Token::ExtglobPattern.symbol());
                return was_non_alpha;
            }
        }

        if is_space(lexer.lookahead()) {
            lexer.mark_end();
            lexer.set_result(Token::ExtglobPattern.symbol());
            self.last_glob_paren_depth = 0;
            return true;
        }

        if is(lexer.lookahead(), '$') {
            lexer.mark_end();
            lexer.advance();
            if is(lexer.lookahead(), '{') || is(lexer.lookahead(), '(') {
                lexer.set_result(Token::ExtglobPattern.symbol());
                return true;
            }
        }

        if is(lexer.lookahead(), '|') {
            lexer.mark_end();
            lexer.advance();
            lexer.set_result(Token::ExtglobPattern.symbol());
            return true;
        }

        let c = lexer.lookahead();
        if !is_alnum(c)
            && !is(c, '(')
            && !is(c, '"')
            && !is(c, '[')
            && !is(c, '?')
            && !is(c, '/')
            && !is(c, '\\')
            && !is(c, '_')
            && !is(c, '*')
        {
            return false;
        }

        let mut done = false;
        let mut saw_non_alphadot = was_non_alpha;
        let mut paren_depth = u32::from(self.last_glob_paren_depth);
        let mut bracket_depth: u32 = 0;
        let mut brace_depth: u32 = 0;
        while !done {
            let c = lexer.lookahead();
            if c == 0 {
                return false;
            } else if is(c, '(') {
                paren_depth = paren_depth.wrapping_add(1);
            } else if is(c, '[') {
                bracket_depth = bracket_depth.wrapping_add(1);
            } else if is(c, '{') {
                brace_depth = brace_depth.wrapping_add(1);
            } else if is(c, ')') {
                done |= paren_depth == 0;
                paren_depth = paren_depth.wrapping_sub(1);
            } else if is(c, ']') {
                done |= bracket_depth == 0;
                bracket_depth = bracket_depth.wrapping_sub(1);
            } else if is(c, '}') {
                done |= brace_depth == 0;
                brace_depth = brace_depth.wrapping_sub(1);
            }

            if is(lexer.lookahead(), '|') {
                lexer.mark_end();
                lexer.advance();
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                    lexer.set_result(Token::ExtglobPattern.symbol());
                    return true;
                }
            }

            if done {
                continue;
            }
            let was_space = is_space(lexer.lookahead());
            if is(lexer.lookahead(), '$') {
                lexer.mark_end();
                // The character ahead is the `$`, so this always holds, as
                // it does in the C.
                let c = lexer.lookahead();
                if !is_alpha(c) && !is(c, '.') && !is(c, '\\') {
                    saw_non_alphadot = true;
                }
                lexer.advance();
                if is(lexer.lookahead(), '(') || is(lexer.lookahead(), '{') {
                    lexer.set_result(Token::ExtglobPattern.symbol());
                    self.last_glob_paren_depth = low_byte(paren_depth);
                    return saw_non_alphadot;
                }
            }
            if was_space {
                lexer.mark_end();
                lexer.set_result(Token::ExtglobPattern.symbol());
                self.last_glob_paren_depth = 0;
                return saw_non_alphadot;
            }
            if is(lexer.lookahead(), '"') {
                lexer.mark_end();
                lexer.set_result(Token::ExtglobPattern.symbol());
                self.last_glob_paren_depth = 0;
                return saw_non_alphadot;
            }
            let c = lexer.lookahead();
            if is(c, '\\') {
                // Never true of a `\`, as in the C.
                if !is_alpha(c) && !is(c, '.') && !is(c, '\\') {
                    saw_non_alphadot = true;
                }
                lexer.advance();
                if is_space(lexer.lookahead()) || is(lexer.lookahead(), '"') {
                    lexer.advance();
                }
            } else {
                if !is_alpha(c) && !is(c, '.') && !is(c, '\\') {
                    saw_non_alphadot = true;
                }
                lexer.advance();
            }
            if !was_space {
                lexer.mark_end();
            }
        }

        lexer.set_result(Token::ExtglobPattern.symbol());
        self.last_glob_paren_depth = 0;
        saw_non_alphadot
    }

    /// The C's `expansion_word:` section, then on into the brace start's.
    fn scan_expansion_word(lexer: &mut Lexer<'_>, vs: &[bool]) -> bool {
        if !valid(vs, Token::ExpansionWord) {
            return Self::scan_brace_start(lexer, vs);
        }
        let mut advanced_once = false;
        let mut advance_once_space = false;
        let starts_expansion = |c: i32| is(c, '{') || is(c, '(') || is(c, '\'') || is_alnum(c);
        loop {
            if is(lexer.lookahead(), '"') {
                return false;
            }
            if is(lexer.lookahead(), '$') {
                lexer.mark_end();
                lexer.advance();
                if starts_expansion(lexer.lookahead()) {
                    lexer.set_result(Token::ExpansionWord.symbol());
                    return advanced_once;
                }
                advanced_once = true;
            }

            if is(lexer.lookahead(), '}') {
                lexer.mark_end();
                lexer.set_result(Token::ExpansionWord.symbol());
                return advanced_once || advance_once_space;
            }

            if is(lexer.lookahead(), '(') && !(advanced_once || advance_once_space) {
                lexer.mark_end();
                lexer.advance();
                while !is(lexer.lookahead(), ')') && !lexer.eof() {
                    // A `$(` or `${` here: assume this is valid, a garbage
                    // concatenation of some weird word and an expansion.
                    if is(lexer.lookahead(), '$') {
                        lexer.mark_end();
                        lexer.advance();
                        if starts_expansion(lexer.lookahead()) {
                            lexer.set_result(Token::ExpansionWord.symbol());
                            return advanced_once;
                        }
                        advanced_once = true;
                    } else {
                        advanced_once = advanced_once || !is_space(lexer.lookahead());
                        advance_once_space = advance_once_space || is_space(lexer.lookahead());
                        lexer.advance();
                    }
                }
                lexer.mark_end();
                if is(lexer.lookahead(), ')') {
                    advanced_once = true;
                    lexer.advance();
                    lexer.mark_end();
                    if is(lexer.lookahead(), '}') {
                        return false;
                    }
                } else {
                    return false;
                }
            }

            if is(lexer.lookahead(), '\'') {
                return false;
            }
            if lexer.eof() {
                return false;
            }
            advanced_once = advanced_once || !is_space(lexer.lookahead());
            advance_once_space = advance_once_space || is_space(lexer.lookahead());
            lexer.advance();
        }
    }

    /// The C's `brace_start:` section: `{1..10}`.
    fn scan_brace_start(lexer: &mut Lexer<'_>, vs: &[bool]) -> bool {
        if !valid(vs, Token::BraceStart) || in_error_recovery(vs) {
            return false;
        }
        while is_space(lexer.lookahead()) {
            lexer.skip();
        }
        if !is(lexer.lookahead(), '{') {
            return false;
        }
        lexer.advance();
        lexer.mark_end();
        while is_digit(lexer.lookahead()) {
            lexer.advance();
        }
        if !is(lexer.lookahead(), '.') {
            return false;
        }
        lexer.advance();
        if !is(lexer.lookahead(), '.') {
            return false;
        }
        lexer.advance();
        while is_digit(lexer.lookahead()) {
            lexer.advance();
        }
        if !is(lexer.lookahead(), '}') {
            return false;
        }
        lexer.set_result(Token::BraceStart.symbol());
        true
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "heredoc_start",
        "heredoc_body",
        "_heredoc_body_beginning",
        "heredoc_content",
        "heredoc_end",
        "file_descriptor",
        "_empty_value",
        "_concat",
        "variable_name",
        "test_operator",
        "regex",
        "regex",
        "regex",
        "word",
        "extglob_pattern",
        "$",
        "{",
        "##",
        "#",
        "!",
        "=",
        "}",
        "]",
        "<<",
        "<<-",
        "heredoc_redirect_token1",
        "(",
        "esac",
        "__error_recovery",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        self.scan_main(lexer, valid_symbols)
    }

    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let Ok(count) = u8::try_from(self.heredocs.len()) else {
            return 0;
        };
        let mut out: Vec<u8> = vec![
            self.last_glob_paren_depth,
            u8::from(self.ext_was_in_double_quote),
            u8::from(self.ext_saw_outside_quote),
            count,
        ];
        for heredoc in &self.heredocs {
            let Ok(len) = u32::try_from(heredoc.delimiter.len()) else {
                return 0;
            };
            // As the C: all or nothing, when a heredoc does not fit.
            let needed = out
                .len()
                .saturating_add(3 + 4)
                .saturating_add(heredoc.delimiter.len().saturating_mul(4));
            if needed >= buffer.len() {
                return 0;
            }
            out.extend([
                u8::from(heredoc.is_raw),
                u8::from(heredoc.started),
                u8::from(heredoc.allows_indent),
            ]);
            out.extend(len.to_le_bytes());
            for c in &heredoc.delimiter {
                out.extend(c.to_le_bytes());
            }
        }
        match buffer.get_mut(..out.len()) {
            Some(slot) => {
                slot.copy_from_slice(&out);
                out.len()
            }
            None => 0,
        }
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            // `reset`: each heredoc reset in place, the rest left as it
            // is -- as the C does.
            for heredoc in &mut self.heredocs {
                heredoc.reset();
            }
            return;
        }
        let mut r = Reader { bytes, at: 0 };
        self.last_glob_paren_depth = r.byte();
        self.ext_was_in_double_quote = r.byte() != 0;
        self.ext_saw_outside_quote = r.byte() != 0;
        let count = usize::from(r.byte());
        for i in 0..count {
            // The C reuses the heredocs it has, and keeps any past the
            // count it reads.
            if i >= self.heredocs.len() {
                self.heredocs.push(Heredoc::default());
            }
            let is_raw = r.byte() != 0;
            let started = r.byte() != 0;
            let allows_indent = r.byte() != 0;
            let len = usize::try_from(r.u32()).unwrap_or(usize::MAX);
            // Never more characters than there are bytes for.
            let len = len.min(r.left() / 4);
            let delimiter: Vec<i32> = (0..len).map(|_| r.i32()).collect();
            if let Some(heredoc) = self.heredocs.get_mut(i) {
                heredoc.is_raw = is_raw;
                heredoc.started = started;
                heredoc.allows_indent = allows_indent;
                heredoc.delimiter = delimiter;
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a parse that fails, or a slice out of range, is the failure"
)]
mod tests {
    fn tree(text: &str) -> tree_sitter::Tree {
        let language = crate::Language::named("bash").unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language.ts_language()).unwrap();
        parser.parse(text, None).unwrap()
    }

    /// **A heredoc whose delimiter is not ASCII still ends** -- where the
    /// C, which keeps the delimiter as bytes, never matched it -- and what
    /// follows it is a command again.
    #[test]
    fn a_heredoc_delimiter_past_ascii_ends_its_heredoc() {
        let tree = tree("cat <<FÍN\nhello\nFÍN\necho done\n");
        let root = tree.root_node();
        assert!(!root.has_error(), "{}", root.to_sexp());
        let sexp = root.to_sexp();
        assert!(sexp.contains("(heredoc_end)"), "{sexp}");
        assert_eq!(root.named_child_count(), 2, "{sexp}");
    }

    /// The first node of `kind` in `tree`, depth first.
    fn first<'t>(tree: &'t tree_sitter::Tree, kind: &str) -> Option<tree_sitter::Node<'t>> {
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.kind() == kind {
                return Some(node);
            }
            let mut cursor = node.walk();
            let children: Vec<_> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
        None
    }

    /// **A regular expression keeps the blanks inside its parentheses**: in
    /// `[[ $x =~ (a b) ]]` all of `(a b)` is the regex, as bash reads it,
    /// where a blank outside a group ends it. (The corpus has no blank
    /// inside a regex's group.)
    #[test]
    fn a_regex_keeps_the_blanks_inside_its_parentheses() {
        let text = "[[ $x =~ (a b) ]]\n";
        let tree = tree(text);
        let regex = first(&tree, "regex").unwrap();
        assert_eq!(
            &text[regex.byte_range()],
            "(a b)",
            "{}",
            tree.root_node().to_sexp()
        );
    }

    /// **Saving and restoring keeps every heredoc's delimiter** -- a
    /// character past ASCII included -- and the flags beside them.
    #[test]
    fn a_saved_state_comes_back_whole() {
        use crate::ffi::ExternalScanner as _;
        let scanner = super::Scanner {
            last_glob_paren_depth: 2,
            ext_was_in_double_quote: true,
            ext_saw_outside_quote: false,
            heredocs: vec![
                super::Heredoc {
                    is_raw: true,
                    started: false,
                    allows_indent: true,
                    delimiter: "FÍN".chars().map(|c| c as i32).collect(),
                    current_leading_word: Vec::new(),
                },
                super::Heredoc::default(),
            ],
        };
        let mut buffer = [0u8; crate::ffi::SERIALIZATION_BUFFER_SIZE];
        let n = scanner.serialize(&mut buffer);
        assert!(n > 0);
        let mut back = super::Scanner::default();
        back.deserialize(&buffer[..n]);
        assert_eq!(back, scanner);
        // A delimiter too long for the buffer: nothing is saved, as the C
        // saves nothing.
        let huge = super::Scanner {
            heredocs: vec![super::Heredoc {
                delimiter: vec![0x41; 300],
                ..super::Heredoc::default()
            }],
            ..super::Scanner::default()
        };
        assert_eq!(huge.serialize(&mut buffer), 0);
    }
}
