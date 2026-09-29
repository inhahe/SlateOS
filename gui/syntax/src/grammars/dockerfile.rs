//! Dockerfiles and Containerfiles: tree-sitter-dockerfile 0.2.0 (MIT, Camden
//! Cheek). `grammars/dockerfile/` holds its `parser.c`, its highlight query,
//! its test corpus and its `scanner.c` as published; the scanner is ported
//! below. It reads heredocs -- `RUN <<EOF`, `COPY <<EOF /dest`, and the lines
//! after the instruction up to one that is `EOF` -- keeping those opened on
//! the instruction's line and not yet ended, first to end first, saved with
//! the parse's state.
//!
//! A `RUN` instruction's command is coloured as Bash, and so is the script
//! of a `RUN <<EOF` -- by `injections.slateos.scm`, this crate's own, as the
//! package publishes no injection query.
//!
//! # Where the port differs
//!
//! - **A heredoc ends at a line that is its delimiter**, the whole line, as
//!   the shell and BuildKit end one: the C ended it at a line that began
//!   with the delimiter -- `EOFX` ended an `EOF` heredoc, and the `X` was an
//!   error.
//! - **A file with Windows line ends has heredocs too**: a carriage return
//!   before the line feed that starts a heredoc's lines, or that ends its
//!   delimiter's line, is taken as the line's end; the C took only `\n`, and
//!   such a file's heredocs were errors.
//! - **`<<-` strips leading tabs**, as the shell does; the C stripped spaces
//!   too, so `    EOF` ended a heredoc the shell reads on past.
//! - **`<<` with no delimiter opens no heredoc**, as BuildKit reads it; the
//!   C opened one with an empty delimiter.
//! - **Heredocs are kept as far as the saved state holds them**: the C
//!   dropped every one past ten on an instruction, and their lines were read
//!   as instructions.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("dockerfile", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/dockerfile/highlights.scm");

/// The injection query, this crate's own -- the package publishes none: a
/// `RUN` instruction's command in Bash, and the script of a `RUN <<EOF` too
/// (`injections.slateos.scm` says which heredocs are scripts).
pub(crate) const INJECTIONS: &str =
    include_str!("../../grammars/dockerfile/injections.slateos.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    HeredocMarker,
    HeredocLine,
    HeredocEnd,
    HeredocNl,
    ErrorSentinel,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// The longest delimiter kept, in bytes, as the C's: a longer one is still a
/// marker, but opens no heredoc the scanner follows.
const MAX_DELIMITER: usize = 509;

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// A blank that is not a line's end: space, tab, carriage return, vertical
/// tab, form feed.
fn is_blank(c: i32) -> bool {
    [' ', '\t', '\r', '\u{b}', '\u{c}']
        .into_iter()
        .any(|b| is(c, b))
}

/// A heredoc opened and not yet ended.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Heredoc {
    /// The line that ends it.
    delimiter: String,
    /// `<<-`: its lines' leading tabs are not its text.
    strips_tabs: bool,
}

/// The scanner's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    /// Whether the lines being read are heredocs' -- what error recovery,
    /// which asks for every token, goes by.
    in_heredoc: bool,
    /// The heredocs opened and not yet ended, first to end first.
    heredocs: Vec<Heredoc>,
}

impl Scanner {
    /// A heredoc's marker, blanks before it passed over: `<<`, a `-` to
    /// strip tabs, and the delimiter -- a word, or quoted with `"` or `'`, a
    /// `\` taking the character after it as it is. It opens a heredoc.
    fn scan_marker(&mut self, lexer: &mut Lexer<'_>) -> bool {
        while is_blank(lexer.lookahead()) {
            lexer.skip();
        }
        for _ in 0..2 {
            if lexer.eof() || !is(lexer.lookahead(), '<') {
                return false;
            }
            lexer.advance();
        }
        let strips_tabs = is(lexer.lookahead(), '-');
        if strips_tabs {
            lexer.advance();
        }
        let quote = ['"', '\''].into_iter().find(|&q| is(lexer.lookahead(), q));
        if quote.is_some() {
            lexer.advance();
        }
        let mut delimiter = String::new();
        let mut too_long = false;
        loop {
            let c = lexer.lookahead();
            let ends = match quote {
                Some(q) => is(c, q),
                None => is_blank(c) || is(c, '\n'),
            };
            if lexer.eof() || ends {
                break;
            }
            if is(c, '\\') {
                lexer.advance();
                if lexer.eof() {
                    return false;
                }
            }
            if let Some(c) = u32::try_from(lexer.lookahead())
                .ok()
                .and_then(char::from_u32)
            {
                if delimiter.len().saturating_add(c.len_utf8()) > MAX_DELIMITER {
                    too_long = true;
                } else if !too_long {
                    delimiter.push(c);
                }
            }
            lexer.advance();
        }
        if let Some(q) = quote {
            if !is(lexer.lookahead(), q) {
                return false;
            }
            lexer.advance();
        }
        if delimiter.is_empty() && !too_long {
            return false;
        }
        if !too_long {
            self.heredocs.push(Heredoc {
                delimiter,
                strips_tabs,
            });
        }
        lexer.set_result(Token::HeredocMarker.symbol());
        true
    }

    /// A heredoc's line, or the line that ends it -- its leading tabs passed
    /// over under `<<-`.
    fn scan_content(&mut self, lexer: &mut Lexer<'_>, valid: impl Fn(Token) -> bool) -> bool {
        let Some(first) = self.heredocs.first() else {
            self.in_heredoc = false;
            return false;
        };
        self.in_heredoc = true;
        if first.strips_tabs {
            while is(lexer.lookahead(), '\t') {
                lexer.skip();
            }
        }
        if valid(Token::HeredocEnd) {
            let mut whole = true;
            for c in first.delimiter.chars() {
                if lexer.eof() || !is(lexer.lookahead(), c) {
                    whole = false;
                    break;
                }
                lexer.advance();
            }
            let c = lexer.lookahead();
            if whole && (lexer.eof() || is(c, '\n') || is(c, '\r')) {
                lexer.set_result(Token::HeredocEnd.symbol());
                self.heredocs.remove(0);
                if self.heredocs.is_empty() {
                    self.in_heredoc = false;
                }
                return true;
            }
        }
        if !valid(Token::HeredocLine) {
            return false;
        }
        lexer.set_result(Token::HeredocLine.symbol());
        loop {
            if lexer.eof() {
                self.in_heredoc = false;
                return true;
            }
            if is(lexer.lookahead(), '\n') {
                return true;
            }
            lexer.advance();
        }
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "heredoc_marker",
        "heredoc_line",
        "heredoc_end",
        "heredoc_nl",
        "error_sentinel",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // Error recovery asks for every token, the sentinel among them: go
        // by where the scanner is.
        if valid(Token::ErrorSentinel) {
            return if self.in_heredoc {
                self.scan_content(lexer, valid)
            } else {
                self.scan_marker(lexer)
            };
        }
        // A line's end is a heredoc's start only while one is open: a plain
        // one could end the instruction or start its heredoc. (`\r\n` too,
        // where the C took only `\n`: a file with Windows line ends.)
        if valid(Token::HeredocNl) && !self.heredocs.is_empty() {
            let crlf = is(lexer.lookahead(), '\r');
            if crlf {
                lexer.advance();
            }
            if is(lexer.lookahead(), '\n') {
                lexer.set_result(Token::HeredocNl.symbol());
                lexer.advance();
                return true;
            }
            if crlf {
                return false;
            }
        }
        if valid(Token::HeredocMarker) {
            return self.scan_marker(lexer);
        }
        if valid(Token::HeredocLine) || valid(Token::HeredocEnd) {
            return self.scan_content(lexer, valid);
        }
        false
    }

    /// Whether the lines being read are heredocs' (a byte), how many
    /// heredocs are saved (a `u16`, little-endian), then each -- whether it
    /// strips tabs (a byte), its delimiter's length (a `u16`) and bytes --
    /// for as many as fit.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let mut size: usize = 3;
        let mut saved: u16 = 0;
        for heredoc in &self.heredocs {
            let bytes = heredoc.delimiter.as_bytes();
            let Ok(length) = u16::try_from(bytes.len()) else {
                break;
            };
            let Some(end) = size
                .checked_add(3)
                .and_then(|n| n.checked_add(bytes.len()))
                .filter(|&end| end <= buffer.len())
            else {
                break;
            };
            let Some(out) = buffer.get_mut(size..end) else {
                break;
            };
            let [low, high] = length.to_le_bytes();
            let (head, rest) = out.split_at_mut(3);
            head.copy_from_slice(&[u8::from(heredoc.strips_tabs), low, high]);
            rest.copy_from_slice(bytes);
            size = end;
            saved = saved.saturating_add(1);
        }
        if let Some(head) = buffer.get_mut(..3) {
            let [low, high] = saved.to_le_bytes();
            head.copy_from_slice(&[u8::from(self.in_heredoc), low, high]);
        }
        size.min(buffer.len())
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        *self = Self::default();
        let Some(&[in_heredoc, low, high]) =
            bytes.get(..3).and_then(|b| <&[u8; 3]>::try_from(b).ok())
        else {
            return;
        };
        self.in_heredoc = in_heredoc != 0;
        let mut at: usize = 3;
        for _ in 0..u16::from_le_bytes([low, high]) {
            let Some(heredoc) = Self::saved(bytes, &mut at) else {
                break;
            };
            self.heredocs.push(heredoc);
        }
    }
}

impl Scanner {
    /// The saved heredoc at `at` in `bytes`, moving `at` past it.
    fn saved(bytes: &[u8], at: &mut usize) -> Option<Heredoc> {
        let head = bytes.get(*at..at.checked_add(3)?)?;
        let (&strips, length) = head.split_first()?;
        let length = usize::from(u16::from_le_bytes([*length.first()?, *length.get(1)?]));
        let start = at.checked_add(3)?;
        let end = start.checked_add(length)?;
        let delimiter = core::str::from_utf8(bytes.get(start..end)?)
            .ok()?
            .to_owned();
        *at = end;
        Some(Heredoc {
            delimiter,
            strips_tabs: strips != 0,
        })
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

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// **A saved state comes back whole**, and an empty one is a fresh
    /// scanner.
    #[test]
    fn a_saved_state_comes_back_whole() {
        let scanner = Scanner {
            in_heredoc: true,
            heredocs: vec![
                Heredoc {
                    delimiter: "EOF".into(),
                    strips_tabs: false,
                },
                Heredoc {
                    delimiter: "ÉND OF IT".into(),
                    strips_tabs: true,
                },
            ],
        };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        let mut back = Scanner::default();
        back.deserialize(&buffer[..size]);
        assert_eq!(back, scanner);
        back.deserialize(&[]);
        assert_eq!(back, Scanner::default());
    }

    /// **A heredoc ends at a line that is its delimiter, the whole line** --
    /// not at one that only begins with it -- and a carriage return may end
    /// the line.
    #[test]
    fn a_heredoc_ends_at_a_line_that_is_its_delimiter() {
        let tree = sexp("RUN <<EOF\nEOFX\nEOF\nFROM scratch\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(
            tree.contains("(heredoc_block (heredoc_line) (heredoc_end))"),
            "{tree}"
        );
        assert!(tree.contains("(from_instruction"), "{tree}");
        let tree = sexp("RUN <<EOF\r\nhi\r\nEOF\r\nFROM scratch\r\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(tree.contains("(heredoc_end)"), "{tree}");
    }

    /// **`<<-` strips leading tabs, not spaces**: a tab-indented delimiter
    /// ends the heredoc, a space-indented one is a line of it.
    #[test]
    fn a_stripping_heredoc_strips_tabs_only() {
        let tree = sexp("RUN <<-EOF\n\techo hi\n\tEOF\nFROM scratch\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(
            tree.contains("(heredoc_block (heredoc_line) (heredoc_end))"),
            "{tree}"
        );
        let tree = sexp("RUN <<-EOF\n    EOF\nEOF\n");
        assert!(
            tree.contains("(heredoc_block (heredoc_line) (heredoc_end))"),
            "{tree}"
        );
    }

    /// **`<<` with no delimiter opens no heredoc**: the next line is an
    /// instruction.
    #[test]
    fn a_marker_with_no_delimiter_opens_nothing() {
        let tree = sexp("RUN cat << EOF\nFROM scratch\n");
        assert!(!tree.contains("heredoc_block"), "{tree}");
        assert!(tree.contains("(from_instruction"), "{tree}");
    }

    /// **Any number of heredocs on one instruction**: twelve, where the C
    /// kept ten.
    #[test]
    fn any_number_of_heredocs_are_kept() {
        let mut text = String::from("RUN cat");
        for i in 0..12 {
            text.push_str(&format!(" <<D{i}"));
        }
        text.push('\n');
        for i in 0..12 {
            text.push_str(&format!("line {i}\nD{i}\n"));
        }
        text.push_str("FROM scratch\n");
        let tree = sexp(&text);
        assert!(!tree.contains("ERROR"), "{tree}");
        assert_eq!(tree.matches("(heredoc_block").count(), 12, "{tree}");
    }

    /// **A quoted delimiter is the text between its quotes**, and a `\`
    /// takes the character after it as it is.
    #[test]
    fn a_quoted_or_escaped_delimiter_is_read_as_it_is() {
        let tree = sexp("COPY <<\"MY END\" /f\nbody\nMY END\nFROM scratch\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(tree.contains("(heredoc_end)"), "{tree}");
        let tree = sexp("COPY <<E\\ F /f\nbody\nE F\nFROM scratch\n");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(tree.contains("(heredoc_end)"), "{tree}");
        // A quote never closed is no delimiter, and no marker.
        let tree = sexp("COPY <<\"EOF /f\nbody\nEOF\n");
        assert!(!tree.contains("heredoc_marker"), "{tree}");
    }
}
