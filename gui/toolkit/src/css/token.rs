//! The tokens a style is written in: CSS Syntax Level 3's tokenizer, for the
//! part of the language the toolkit reads.
//!
//! Followed closely where it matters to what a style means -- numbers with
//! their signs and exponents, dimensions and percentages, identifiers that
//! begin with a hyphen (custom properties begin with two), hashes, strings
//! with their escapes, comments -- and simplified where it does not: there is
//! no `url(` token (a style has nothing to fetch), and an at-keyword is a
//! token the parser only skips.
//!
//! Every token carries the byte offset it began at, so a warning can say
//! where in the text it is about.

/// One token, and where it began.
#[derive(Clone, Debug, PartialEq)]
pub struct Spanned {
    /// The token.
    pub token: Token,
    /// The byte offset in the text it began at.
    pub at: usize,
}

/// A token of CSS.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// A name: a keyword, a property's name, a custom property's (`--gap`).
    Ident(String),
    /// A name followed at once by `(`, which is consumed with it.
    Function(String),
    /// `@` and a name.
    AtKeyword(String),
    /// `#` and a name: a colour's hex digits, or an id selector.
    Hash(String),
    /// A quoted string, its escapes resolved.
    Str(String),
    /// A string a line break cut off.
    BadString,
    /// A number with no unit.
    Number(f64),
    /// A number and `%`.
    Percentage(f64),
    /// A number and a unit, the unit as written (compare it ignoring case).
    Dimension(f64, String),
    /// Spaces, tabs, line breaks, comments: they separate, and say nothing.
    Whitespace,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `{`
    OpenBrace,
    /// `}`
    CloseBrace,
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `[`
    OpenBracket,
    /// `]`
    CloseBracket,
    /// Any other character on its own: `>`, `.`, `&`, `*`, `/`, `+`, `!` ...
    Delim(char),
}

/// `input`'s tokens, in order. Never fails: what is not CSS comes out as
/// [`Token::Delim`]s and [`Token::BadString`]s for the parser to refuse.
#[must_use]
pub fn tokenize(input: &str) -> Vec<Spanned> {
    let mut lexer = Lexer {
        chars: input.char_indices().collect(),
        pos: 0,
        len: input.len(),
    };
    let mut out = Vec::new();
    while let Some(spanned) = lexer.next_token() {
        // Adjacent whitespace and comments are one separator.
        if spanned.token == Token::Whitespace
            && out
                .last()
                .is_some_and(|last: &Spanned| last.token == Token::Whitespace)
        {
            continue;
        }
        out.push(spanned);
    }
    out
}

/// The characters of the text, and how far through them the lexer is.
struct Lexer {
    chars: Vec<(usize, char)>,
    pos: usize,
    /// The text's length in bytes: the offset of its end.
    len: usize,
}

impl Lexer {
    /// The character `ahead` past the next one, if there is one.
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars
            .get(self.pos.saturating_add(ahead))
            .map(|&(_, c)| c)
    }

    /// The byte offset of the next character, or the end.
    fn offset(&self) -> usize {
        self.chars.get(self.pos).map_or(self.len, |&(at, _)| at)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.pos = self.pos.saturating_add(1);
        Some(c)
    }

    fn next_token(&mut self) -> Option<Spanned> {
        let at = self.offset();
        let c = self.peek(0)?;
        let token = match c {
            '/' if self.peek(1) == Some('*') => {
                self.skip_comment();
                Token::Whitespace
            }
            c if c.is_whitespace() => {
                while self.peek(0).is_some_and(char::is_whitespace) {
                    self.bump();
                }
                Token::Whitespace
            }
            '"' | '\'' => {
                self.bump();
                self.string(c)
            }
            '#' => {
                self.bump();
                if self.peek(0).is_some_and(is_name) || self.starts_escape(0) {
                    Token::Hash(self.name())
                } else {
                    Token::Delim('#')
                }
            }
            '@' => {
                self.bump();
                if self.starts_ident(0) {
                    Token::AtKeyword(self.name())
                } else {
                    Token::Delim('@')
                }
            }
            ':' => self.single(Token::Colon),
            ';' => self.single(Token::Semicolon),
            ',' => self.single(Token::Comma),
            '{' => self.single(Token::OpenBrace),
            '}' => self.single(Token::CloseBrace),
            '(' => self.single(Token::OpenParen),
            ')' => self.single(Token::CloseParen),
            '[' => self.single(Token::OpenBracket),
            ']' => self.single(Token::CloseBracket),
            _ if self.starts_number(0) => self.numeric(),
            _ if self.starts_ident(0) => self.ident_like(),
            other => self.single(Token::Delim(other)),
        };
        Some(Spanned { token, at })
    }

    fn single(&mut self, token: Token) -> Token {
        self.bump();
        token
    }

    /// Past a `/* ... */`, or to the end if it is never closed.
    fn skip_comment(&mut self) {
        self.bump();
        self.bump();
        while let Some(c) = self.bump() {
            if c == '*' && self.peek(0) == Some('/') {
                self.bump();
                return;
            }
        }
    }

    /// Whether a `\` escape starts `ahead` characters on: a backslash not
    /// followed by a line break.
    fn starts_escape(&self, ahead: usize) -> bool {
        self.peek(ahead) == Some('\\')
            && self
                .peek(ahead.saturating_add(1))
                .is_some_and(|c| c != '\n' && c != '\r' && c != '\x0C')
    }

    /// Whether an identifier starts `ahead` characters on.
    fn starts_ident(&self, ahead: usize) -> bool {
        match self.peek(ahead) {
            Some('-') => {
                let next = ahead.saturating_add(1);
                self.peek(next)
                    .is_some_and(|c| is_name_start(c) || c == '-')
                    || self.starts_escape(next)
            }
            Some(c) if is_name_start(c) => true,
            Some('\\') => self.starts_escape(ahead),
            _ => false,
        }
    }

    /// Whether a number starts `ahead` characters on.
    fn starts_number(&self, ahead: usize) -> bool {
        let digit = |k: usize| self.peek(k).is_some_and(|c| c.is_ascii_digit());
        let after = ahead.saturating_add(1);
        match self.peek(ahead) {
            Some('+' | '-') => {
                digit(after) || (self.peek(after) == Some('.') && digit(after.saturating_add(1)))
            }
            Some('.') => digit(after),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    /// A name's characters, escapes resolved.
    fn name(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_name(c) => {
                    self.bump();
                    out.push(c);
                }
                Some('\\') if self.starts_escape(0) => {
                    self.bump();
                    out.push(self.escape());
                }
                _ => return out,
            }
        }
    }

    /// The character an escape stands for, its backslash already consumed:
    /// up to six hex digits and one following space, or the character itself.
    fn escape(&mut self) -> char {
        let mut value = 0u32;
        let mut digits = 0u8;
        while digits < 6 {
            match self.peek(0).and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    self.bump();
                    value = value.saturating_mul(16).saturating_add(d);
                    digits = digits.saturating_add(1);
                }
                None => break,
            }
        }
        if digits == 0 {
            return self.bump().unwrap_or('\u{FFFD}');
        }
        if self.peek(0).is_some_and(char::is_whitespace) {
            self.bump();
        }
        match char::from_u32(value) {
            Some(c) if value != 0 => c,
            _ => '\u{FFFD}',
        }
    }

    /// A quoted string, its opening `quote` consumed.
    fn string(&mut self, quote: char) -> Token {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                None => return Token::Str(out),
                Some(c) if c == quote => {
                    self.bump();
                    return Token::Str(out);
                }
                Some('\n' | '\r' | '\x0C') => return Token::BadString,
                Some('\\') => {
                    self.bump();
                    match self.peek(0) {
                        // A backslash at a line break continues the string.
                        Some('\n' | '\x0C') => {
                            self.bump();
                        }
                        Some('\r') => {
                            self.bump();
                            if self.peek(0) == Some('\n') {
                                self.bump();
                            }
                        }
                        None => {}
                        Some(_) => out.push(self.escape()),
                    }
                }
                Some(c) => {
                    self.bump();
                    out.push(c);
                }
            }
        }
    }

    /// A number, and the `%` or unit after it.
    fn numeric(&mut self) -> Token {
        let start = self.pos;
        if matches!(self.peek(0), Some('+' | '-')) {
            self.bump();
        }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        // An exponent only where one follows: `1em` is a dimension, not 1e
        // and an `m`.
        if matches!(self.peek(0), Some('e' | 'E')) {
            let signed = matches!(self.peek(1), Some('+' | '-'));
            let digit_at = if signed { 2 } else { 1 };
            if self.peek(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                for _ in 0..digit_at {
                    self.bump();
                }
                while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
            }
        }
        let text: String = self
            .chars
            .get(start..self.pos)
            .unwrap_or(&[])
            .iter()
            .map(|&(_, c)| c)
            .collect();
        // Only what was just checked to be a number's characters: a parse
        // that failed anyway is a zero rather than a lost token.
        let value = text.parse::<f64>().unwrap_or(0.0);
        if self.peek(0) == Some('%') {
            self.bump();
            Token::Percentage(value)
        } else if self.starts_ident(0) {
            Token::Dimension(value, self.name())
        } else {
            Token::Number(value)
        }
    }

    /// An identifier, or a function's name and its `(`.
    fn ident_like(&mut self) -> Token {
        let name = self.name();
        if self.peek(0) == Some('(') {
            self.bump();
            Token::Function(name)
        } else {
            Token::Ident(name)
        }
    }
}

/// Whether `c` can begin a name: a letter, `_`, or anything past ASCII.
fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

/// Whether `c` can continue a name.
fn is_name(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
