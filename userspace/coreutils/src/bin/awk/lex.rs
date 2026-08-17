//! The awk lexer.
//!
//! ## The two things that make this more than a token loop
//!
//! **A newline is sometimes a terminator and sometimes whitespace.** awk has no
//! semicolon rule; a statement ends at a newline. But a newline after `{`, `&&`,
//! `||`, `,`, `do`, `else`, `;` or a comma continues the construct, and so does
//! a backslash at end of line. The lexer resolves this, not the parser, because
//! it is a property of the preceding *token*.
//!
//! **A slash is division or the start of a regular expression, depending on
//! what came before it.** `$1 / 2` divides; `$1 ~ /2/` matches. There is no way
//! to tell without knowing whether the previous token could end an operand, so
//! the lexer tracks exactly that. Getting it backwards turns `a / b / c` into an
//! unterminated regex — which is why this is a rule about the previous token and
//! not a heuristic about the characters ahead.

use crate::value::Str;

/// One token, with the source offset that produced it so a diagnostic can point
/// at the right place in the program text.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: Tok,
    pub at: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// End of program.
    Eof,
    /// A statement terminator: a newline that was not swallowed.
    Newline,
    Semi,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,

    Number(f64),
    /// A string literal, with escapes already resolved.
    Str(Str),
    /// A `/…/` regular-expression literal, with `\/` resolved to `/`.
    Ere(Str),
    /// An identifier that is not a keyword.
    Name(String),
    /// `name(` with no space — awk's rule for a *call*, which is how a user
    /// function call is told from a concatenation with a parenthesised value.
    FuncName(String),
    /// A built-in function name.
    Builtin(&'static str),
    /// A reserved word.
    Keyword(Kw),

    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    Or,
    And,
    Not,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Match,
    NoMatch,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Incr,
    Decr,
    Dollar,
    Question,
    Colon,
    Pipe,
    Append,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    Begin,
    End,
    Function,
    If,
    Else,
    While,
    For,
    Do,
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    Return,
    Delete,
    In,
    Getline,
    Print,
    Printf,
}

/// The built-in functions, and how many arguments each accepts.
///
/// `min` and `max` are checked at parse time rather than at run time so that a
/// program with `substr(s)` in a branch that never executes is still rejected —
/// awk parses the whole program before running any of it, and a script that
/// dies halfway through a report is worse than one that never starts.
pub const BUILTINS: &[(&str, usize, usize)] = &[
    ("length", 0, 1),
    ("substr", 2, 3),
    ("index", 2, 2),
    ("split", 2, 3),
    ("sub", 2, 3),
    ("gsub", 2, 3),
    ("match", 2, 2),
    ("sprintf", 1, usize::MAX),
    ("sin", 1, 1),
    ("cos", 1, 1),
    ("atan2", 2, 2),
    ("exp", 1, 1),
    ("log", 1, 1),
    ("sqrt", 1, 1),
    ("int", 1, 1),
    ("rand", 0, 0),
    ("srand", 0, 1),
    ("tolower", 1, 1),
    ("toupper", 1, 1),
    ("system", 1, 1),
    ("close", 1, 2),
    ("fflush", 0, 1),
];

fn keyword(name: &str) -> Option<Kw> {
    Some(match name {
        "BEGIN" => Kw::Begin,
        "END" => Kw::End,
        "function" | "func" => Kw::Function,
        "if" => Kw::If,
        "else" => Kw::Else,
        "while" => Kw::While,
        "for" => Kw::For,
        "do" => Kw::Do,
        "break" => Kw::Break,
        "continue" => Kw::Continue,
        "next" => Kw::Next,
        "nextfile" => Kw::NextFile,
        "exit" => Kw::Exit,
        "return" => Kw::Return,
        "delete" => Kw::Delete,
        "in" => Kw::In,
        "getline" => Kw::Getline,
        "print" => Kw::Print,
        "printf" => Kw::Printf,
        _ => return None,
    })
}

pub struct Lexer<'a> {
    src: &'a [u8],
    i: usize,
    /// The previous significant token, which decides both whether a newline
    /// terminates a statement and whether `/` starts a regex.
    prev: Option<Tok>,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(src: &'a [u8]) -> Lexer<'a> {
        Lexer {
            src,
            i: 0,
            prev: None,
        }
    }

    /// Tokenise the whole program.
    ///
    /// # Errors
    /// Returns the diagnostic for an unterminated string or regex, or a
    /// character that cannot begin a token.
    pub fn tokens(mut self) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        loop {
            let t = self.next_token()?;
            let end = t.kind == Tok::Eof;
            self.prev = Some(t.kind.clone());
            out.push(t);
            if end {
                return Ok(out);
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }
    fn at(&self, k: usize) -> Option<u8> {
        self.src.get(self.i.saturating_add(k)).copied()
    }
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.i = self.i.saturating_add(1);
        }
        c
    }

    /// Whether a newline here is a statement terminator or just whitespace.
    ///
    /// POSIX lists the tokens a newline may follow without ending anything:
    /// `{ && || do else , ;` and the two `)` cases the *parser* handles (after
    /// `if (…)`, `while (…)`, `for (…)`), which is why `)` is not here.
    fn newline_is_significant(&self) -> bool {
        !matches!(
            self.prev,
            None | Some(
                Tok::LBrace
                    | Tok::And
                    | Tok::Or
                    | Tok::Comma
                    | Tok::Semi
                    | Tok::Newline
                    | Tok::Question
                    | Tok::Colon
                    | Tok::Keyword(Kw::Do | Kw::Else)
            )
        )
    }

    /// Whether a `/` here divides rather than opening a regex.
    ///
    /// It divides exactly when the previous token could have *ended an
    /// operand*. Everywhere else — after an operator, after `(`, at the start
    /// of a statement — a `/` begins a regular expression.
    fn slash_is_division(&self) -> bool {
        matches!(
            self.prev,
            Some(
                Tok::Number(_)
                    | Tok::Str(_)
                    | Tok::Name(_)
                    | Tok::RParen
                    | Tok::RBracket
                    | Tok::Incr
                    | Tok::Decr
                    | Tok::Builtin(_)
            )
        )
    }

    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r') => {
                    self.i = self.i.saturating_add(1);
                }
                // A backslash-newline is a line continuation and vanishes.
                Some(b'\\') if matches!(self.at(1), Some(b'\n')) => {
                    self.i = self.i.saturating_add(2);
                }
                Some(b'\\') if matches!(self.at(1), Some(b'\r')) && self.at(2) == Some(b'\n') => {
                    self.i = self.i.saturating_add(3);
                }
                Some(b'#') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.i = self.i.saturating_add(1);
                    }
                }
                // A newline that terminates nothing is whitespace too, and the
                // *next* newline is judged against the same previous token.
                Some(b'\n') if !self.newline_is_significant() => {
                    self.i = self.i.saturating_add(1);
                }
                _ => return,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, String> {
        self.skip_blanks();
        let at = self.i;
        let Some(c) = self.peek() else {
            return Ok(Token { kind: Tok::Eof, at });
        };

        if c == b'\n' {
            self.i = self.i.saturating_add(1);
            return Ok(Token {
                kind: Tok::Newline,
                at,
            });
        }
        if c == b'"' {
            return Ok(Token {
                kind: Tok::Str(self.string_literal()?),
                at,
            });
        }
        if c == b'/' && !self.slash_is_division() {
            return Ok(Token {
                kind: Tok::Ere(self.ere_literal()?),
                at,
            });
        }
        if c.is_ascii_digit() || (c == b'.' && matches!(self.at(1), Some(d) if d.is_ascii_digit()))
        {
            return Ok(Token {
                kind: Tok::Number(self.number()),
                at,
            });
        }
        if c == b'_' || c.is_ascii_alphabetic() {
            return Ok(Token {
                kind: self.word(),
                at,
            });
        }

        let two: [Option<u8>; 2] = [Some(c), self.at(1)];
        let kind = match (two[0], two[1]) {
            (Some(b'+'), Some(b'=')) => self.take2(Tok::AddAssign),
            (Some(b'-'), Some(b'=')) => self.take2(Tok::SubAssign),
            (Some(b'*'), Some(b'=')) => self.take2(Tok::MulAssign),
            (Some(b'/'), Some(b'=')) => self.take2(Tok::DivAssign),
            (Some(b'%'), Some(b'=')) => self.take2(Tok::ModAssign),
            (Some(b'^'), Some(b'=')) => self.take2(Tok::PowAssign),
            (Some(b'*'), Some(b'*')) => {
                // `**` is `^`, and `**=` is `^=`. Not POSIX, but every awk
                // accepts it and a script using it is not trying to multiply by
                // a dereference.
                self.i = self.i.saturating_add(2);
                if self.peek() == Some(b'=') {
                    self.i = self.i.saturating_add(1);
                    Tok::PowAssign
                } else {
                    Tok::Caret
                }
            }
            (Some(b'='), Some(b'=')) => self.take2(Tok::Eq),
            (Some(b'!'), Some(b'=')) => self.take2(Tok::Ne),
            (Some(b'<'), Some(b'=')) => self.take2(Tok::Le),
            (Some(b'>'), Some(b'=')) => self.take2(Tok::Ge),
            (Some(b'>'), Some(b'>')) => self.take2(Tok::Append),
            (Some(b'&'), Some(b'&')) => self.take2(Tok::And),
            (Some(b'|'), Some(b'|')) => self.take2(Tok::Or),
            (Some(b'+'), Some(b'+')) => self.take2(Tok::Incr),
            (Some(b'-'), Some(b'-')) => self.take2(Tok::Decr),
            (Some(b'!'), Some(b'~')) => self.take2(Tok::NoMatch),
            _ => {
                self.i = self.i.saturating_add(1);
                match c {
                    b'{' => Tok::LBrace,
                    b'}' => Tok::RBrace,
                    b'(' => Tok::LParen,
                    b')' => Tok::RParen,
                    b'[' => Tok::LBracket,
                    b']' => Tok::RBracket,
                    b',' => Tok::Comma,
                    b';' => Tok::Semi,
                    b'=' => Tok::Assign,
                    b'<' => Tok::Lt,
                    b'>' => Tok::Gt,
                    b'!' => Tok::Not,
                    b'~' => Tok::Match,
                    b'+' => Tok::Plus,
                    b'-' => Tok::Minus,
                    b'*' => Tok::Star,
                    b'/' => Tok::Slash,
                    b'%' => Tok::Percent,
                    b'^' => Tok::Caret,
                    b'$' => Tok::Dollar,
                    b'?' => Tok::Question,
                    b':' => Tok::Colon,
                    b'|' => Tok::Pipe,
                    other => {
                        let shown = shown_byte(other);
                        return Err(format!("syntax error at `{shown}'"));
                    }
                }
            }
        };
        Ok(Token { kind, at })
    }

    fn take2(&mut self, t: Tok) -> Tok {
        self.i = self.i.saturating_add(2);
        t
    }

    fn number(&mut self) -> f64 {
        // Hexadecimal constants are not POSIX awk, but every implementation
        // that reads them agrees on the syntax and a program containing `0xff`
        // means it.
        if self.peek() == Some(b'0') && matches!(self.at(1), Some(b'x' | b'X')) {
            let start = self.i.saturating_add(2);
            let mut j = start;
            while matches!(self.src.get(j), Some(d) if d.is_ascii_hexdigit()) {
                j = j.saturating_add(1);
            }
            if j > start {
                let text = self.src.get(start..j).unwrap_or_default();
                self.i = j;
                let mut n: f64 = 0.0;
                for d in text {
                    let v = f64::from(char::from(*d).to_digit(16).unwrap_or(0));
                    n = n.mul_add(16.0, v);
                }
                return n;
            }
        }
        let rest = self.src.get(self.i..).unwrap_or_default();
        match crate::value::num_prefix(rest) {
            Some((n, used)) => {
                self.i = self.i.saturating_add(used);
                n
            }
            None => {
                self.i = self.i.saturating_add(1);
                0.0
            }
        }
    }

    fn word(&mut self) -> Tok {
        let start = self.i;
        while matches!(self.peek(), Some(c) if c == b'_' || c.is_ascii_alphanumeric()) {
            self.i = self.i.saturating_add(1);
        }
        let name =
            String::from_utf8_lossy(self.src.get(start..self.i).unwrap_or_default()).into_owned();
        if let Some(k) = keyword(&name) {
            return Tok::Keyword(k);
        }
        if let Some((b, _, _)) = BUILTINS.iter().find(|(b, _, _)| *b == name) {
            return Tok::Builtin(b);
        }
        // `f(x)` is a call and `f (x)` is a concatenation. This is awk's actual
        // rule and it is why the space matters: without it there would be no
        // way to write the concatenation of a variable with a parenthesised
        // expression.
        if self.peek() == Some(b'(') {
            return Tok::FuncName(name);
        }
        Tok::Name(name)
    }

    fn string_literal(&mut self) -> Result<Str, String> {
        self.i = self.i.saturating_add(1);
        let mut out = Str::new();
        loop {
            match self.bump() {
                None | Some(b'\n') => return Err("newline in string".to_string()),
                Some(b'"') => return Ok(out),
                Some(b'\\') => self.escape(&mut out),
                Some(c) => out.push(c),
            }
        }
    }

    /// One escape sequence, already past the backslash.
    ///
    /// An escape awk does not know keeps its backslash *and* its character —
    /// `"\q"` is a backslash and a `q` — because the sequence may be headed for
    /// a regex, where `\.` and `\(` are meaningful and eating the backslash
    /// here would change what the pattern matches.
    fn escape(&mut self, out: &mut Str) {
        let Some(c) = self.bump() else {
            out.push(b'\\');
            return;
        };
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'/' => out.push(b'/'),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'v' => out.push(0x0b),
            b'0'..=b'7' => {
                // Up to three octal digits, counting the one just consumed.
                let mut v = u32::from(c.wrapping_sub(b'0'));
                let mut n = 1u32;
                while n < 3 {
                    match self.peek() {
                        Some(d @ b'0'..=b'7') => {
                            v = v
                                .saturating_mul(8)
                                .saturating_add(u32::from(d.wrapping_sub(b'0')));
                            self.i = self.i.saturating_add(1);
                            n = n.saturating_add(1);
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(v & 0xff).unwrap_or(0));
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }

    /// A `/…/` literal. Only `\/` is resolved; every other backslash is left
    /// for the regex compiler, which is the one that knows what `\.` means.
    fn ere_literal(&mut self) -> Result<Str, String> {
        self.i = self.i.saturating_add(1);
        let mut out = Str::new();
        let mut in_bracket = false;
        loop {
            match self.bump() {
                None | Some(b'\n') => return Err("unterminated regular expression".to_string()),
                Some(b'\\') => match self.bump() {
                    None => return Err("unterminated regular expression".to_string()),
                    // `\/` is how a slash is written inside a regex literal;
                    // the engine has no such escape, so it is resolved here.
                    Some(b'/') => out.push(b'/'),
                    Some(c) => {
                        out.push(b'\\');
                        out.push(c);
                    }
                },
                // A `/` inside a bracket expression is an ordinary character,
                // exactly as it is for `sed`'s delimiter scan.
                Some(b'[') if !in_bracket => {
                    in_bracket = true;
                    out.push(b'[');
                    if self.peek() == Some(b'^') {
                        out.push(b'^');
                        self.i = self.i.saturating_add(1);
                    }
                    if self.peek() == Some(b']') {
                        out.push(b']');
                        self.i = self.i.saturating_add(1);
                    }
                }
                Some(b']') if in_bracket => {
                    in_bracket = false;
                    out.push(b']');
                }
                Some(b'/') if !in_bracket => {
                    if out.is_empty() {
                        // `//` is the empty regex, which matches everywhere.
                        return Ok(out);
                    }
                    return Ok(out);
                }
                Some(c) => out.push(c),
            }
        }
    }
}

fn shown_byte(b: u8) -> String {
    if b.is_ascii_graphic() || b == b' ' {
        char::from(b).to_string()
    } else {
        format!("\\{b:03o}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        Lexer::new(src.as_bytes())
            .tokens()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn a_slash_divides_after_an_operand_and_opens_a_regex_otherwise() {
        // The case that breaks a naive lexer: three slashes on one line.
        assert_eq!(
            toks("a / b / c"),
            vec![
                Tok::Name("a".into()),
                Tok::Slash,
                Tok::Name("b".into()),
                Tok::Slash,
                Tok::Name("c".into()),
                Tok::Eof
            ]
        );
        assert_eq!(
            toks("$1 ~ /x/"),
            vec![
                Tok::Dollar,
                Tok::Number(1.0),
                Tok::Match,
                Tok::Ere(b"x".to_vec()),
                Tok::Eof
            ]
        );
        // At the start of a rule a slash is always a regex.
        assert_eq!(toks("/x/"), vec![Tok::Ere(b"x".to_vec()), Tok::Eof]);
    }

    #[test]
    fn a_newline_ends_a_statement_but_not_a_continued_one() {
        assert_eq!(
            toks("a\nb"),
            vec![
                Tok::Name("a".into()),
                Tok::Newline,
                Tok::Name("b".into()),
                Tok::Eof
            ]
        );
        // After `&&`, `,`, `{` and a backslash, the newline vanishes.
        assert_eq!(
            toks("a &&\nb"),
            vec![
                Tok::Name("a".into()),
                Tok::And,
                Tok::Name("b".into()),
                Tok::Eof
            ]
        );
        assert_eq!(
            toks("a,\nb"),
            vec![
                Tok::Name("a".into()),
                Tok::Comma,
                Tok::Name("b".into()),
                Tok::Eof
            ]
        );
        assert_eq!(
            toks("a \\\nb"),
            vec![Tok::Name("a".into()), Tok::Name("b".into()), Tok::Eof]
        );
        // A comment runs to the newline, and the newline still counts.
        assert_eq!(
            toks("a # hi\nb"),
            vec![
                Tok::Name("a".into()),
                Tok::Newline,
                Tok::Name("b".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn a_call_is_told_from_a_concatenation_by_the_space() {
        assert_eq!(toks("f(1)").first(), Some(&Tok::FuncName("f".into())));
        assert_eq!(toks("f (1)").first(), Some(&Tok::Name("f".into())));
    }

    #[test]
    fn string_escapes_resolve_but_regex_escapes_do_not() {
        assert_eq!(
            toks(r#""a\tb""#),
            vec![Tok::Str(b"a\tb".to_vec()), Tok::Eof]
        );
        assert_eq!(toks(r#""\101""#), vec![Tok::Str(b"A".to_vec()), Tok::Eof]);
        // An escape awk does not know keeps both characters, because it may be
        // on its way to the regex compiler.
        assert_eq!(
            toks(r#""a\.b""#),
            vec![Tok::Str(br"a\.b".to_vec()), Tok::Eof]
        );
        // In a regex literal only `\/` is resolved.
        assert_eq!(toks(r"/a\/b/"), vec![Tok::Ere(b"a/b".to_vec()), Tok::Eof]);
        assert_eq!(toks(r"/a\.b/"), vec![Tok::Ere(br"a\.b".to_vec()), Tok::Eof]);
    }

    #[test]
    fn a_slash_inside_a_bracket_expression_does_not_end_the_regex() {
        assert_eq!(toks("/[/]/"), vec![Tok::Ere(b"[/]".to_vec()), Tok::Eof]);
        assert_eq!(toks("/[^/]/"), vec![Tok::Ere(b"[^/]".to_vec()), Tok::Eof]);
    }

    #[test]
    fn numbers_in_every_shape_awk_accepts() {
        assert_eq!(
            toks("1 1.5 .5 1e3 1E-2 0x1f"),
            vec![
                Tok::Number(1.0),
                Tok::Number(1.5),
                Tok::Number(0.5),
                Tok::Number(1000.0),
                Tok::Number(0.01),
                Tok::Number(31.0),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn an_unterminated_literal_is_an_error_not_a_guess() {
        assert!(Lexer::new(b"\"abc").tokens().is_err());
        assert!(Lexer::new(b"/abc").tokens().is_err());
    }
}
