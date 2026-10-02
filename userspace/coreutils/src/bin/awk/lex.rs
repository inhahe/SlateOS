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
use ere::awk::{self as escape, Warnings};

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
    /// A `/…/` regular-expression literal.
    Ere {
        /// The text between the slashes as written, continuations removed:
        /// what a diagnostic quotes, as gawk's does.
        source: Str,
        /// `source` through awk's escape layer ([`escape::regexp`]): what the
        /// regex compiler is given.
        pattern: Str,
    },
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
        // Not `func`: POSIX does not reserve it, so a program may name a
        // variable `func`, and gawk --posix reads it as a name too.
        "function" => Kw::Function,
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
    /// What the escape layers had to say about the strings and regexes read
    /// so far; see [`Lexer::tokenize`].
    warnings: Warnings,
    /// Those warnings once said, each with the offset of the token that
    /// earned it, so the caller can name the line as gawk does.
    said: Vec<(usize, Str)>,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(src: &'a [u8]) -> Lexer<'a> {
        Lexer {
            src,
            i: 0,
            prev: None,
            warnings: Warnings::default(),
            said: Vec::new(),
        }
    }

    /// Tokenise the whole program, its escape warnings going to `warnings`'
    /// tables and to `said`, each message beside the offset of its token.
    ///
    /// The warnings are gawk's, each said once per run, so the tables are the
    /// run's one [`Warnings`] rather than this lexer's: the interpreter carries
    /// on with the same one, and a `\q` the program text has already warned
    /// about does not warn again when a dynamic regex repeats it.
    ///
    /// Every token the lexer made, and the error it stopped at, if it did:
    /// with the offset it had reached. The tokens end in an `Eof` either way,
    /// at that offset when the lexer stopped early, so the parser can run over
    /// what came before the error -- which is what gawk, whose parser asks
    /// for one token at a time, has read when its lexer complains; see
    /// `parse::parse` for why the order matters.
    pub fn tokenize(
        src: &'a [u8],
        warnings: &mut Warnings,
        said: &mut Vec<(usize, Str)>,
    ) -> (Vec<Token>, Option<(usize, String)>) {
        let mut lx = Lexer::new(src);
        lx.warnings = std::mem::take(warnings);
        let mut toks = Vec::new();
        let stopped = lx.run(&mut toks).err().map(|e| {
            toks.push(Token {
                kind: Tok::Eof,
                at: lx.i,
            });
            (lx.i, e)
        });
        *warnings = std::mem::take(&mut lx.warnings);
        said.append(&mut lx.said);
        (toks, stopped)
    }

    /// Tokenise the whole program, dropping any escape warnings.
    ///
    /// # Errors
    /// Returns the diagnostic for an unterminated string or regex, or a
    /// character that cannot begin a token.
    #[cfg(test)]
    pub fn tokens(mut self) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        self.run(&mut out)?;
        Ok(out)
    }

    /// Lex onto the end of `out`, up to and including the `Eof`, or to the
    /// first error, leaving behind it every token made before.
    fn run(&mut self, out: &mut Vec<Token>) -> Result<(), String> {
        loop {
            let t = self.next_token()?;
            // Whatever a string or regex literal earned, it earned at this
            // token's offset.
            for message in self.warnings.take() {
                self.said.push((t.at, message));
            }
            let end = t.kind == Tok::Eof;
            self.prev = Some(t.kind.clone());
            out.push(t);
            if end {
                return Ok(());
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
    /// `if (…)`, `while (…)`, `for (…)`), which is why `)` is not here. `?`
    /// and `:` are not here either: gawk lets a newline follow them only
    /// outside `--posix`.
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
                kind: self.ere_literal()?,
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
                    _ => {
                        // `at` is the token's first byte and nothing has
                        // advanced past it yet, so the rest of the program
                        // starts with the whole offending character.
                        let shown = shown_char(self.src.get(at..).unwrap_or_default());
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

    /// A numeric constant: decimal only. gawk's `--posix` implies its
    /// `--traditional`, under which its scanner stops a number at the `x` of
    /// `0x1A` (and reads `011` as eleven), so `print 0x1A` prints `0` joined
    /// to the variable `x1A`. Measured; this read hexadecimal until
    /// 2026-10-01.
    fn number(&mut self) -> f64 {
        let rest = self.src.get(self.i..).unwrap_or_default();
        match crate::value::decimal_prefix(rest) {
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

    /// A `"…"` literal: gawk's lexer, then `make_str_node` on what it kept.
    ///
    /// The lexer only finds the end. A backslash and the character after it
    /// are both kept, so that `\"` does not end the string, and the escapes are
    /// resolved afterwards by [`escape::string`] -- which is where an escape
    /// awk does not know becomes the character itself, with a warning. (It
    /// used to keep its backslash, `"\q"` being a backslash and a `q`; gawk
    /// makes it a `q`, and so does every awk `awk-diff.sh` was ever run
    /// against.)
    ///
    /// A backslash before a newline is a continuation in gawk, and under
    /// `--posix` an error: "POSIX does not allow physical newlines in string
    /// values" -- a fatal, which is why the message carries `fatal: `.
    fn string_literal(&mut self) -> Result<Str, String> {
        self.i = self.i.saturating_add(1);
        let mut raw = Str::new();
        loop {
            match self.bump() {
                None | Some(b'\n') => return Err("unterminated string".to_string()),
                Some(b'"') => return Ok(escape::string(&raw, false, &mut self.warnings)),
                Some(b'\\') => {
                    let mut c = self.bump();
                    // gawk's "allow MS-DOS files. bleah": a CR after the
                    // backslash is dropped.
                    if c == Some(b'\r') {
                        c = self.bump();
                    }
                    match c {
                        None => return Err("unterminated string".to_string()),
                        Some(b'\n') => {
                            // gawk raises this before it counts the newline, so
                            // the line it names is the backslash's. The error is
                            // placed at the lexer's position, which `bump` has
                            // already moved past the newline: step back onto it.
                            self.i = self.i.saturating_sub(1);
                            return Err(
                                "fatal: POSIX does not allow physical newlines in string values"
                                    .to_string(),
                            );
                        }
                        Some(c) => {
                            raw.push(b'\\');
                            raw.push(c);
                        }
                    }
                }
                Some(c) => raw.push(c),
            }
        }
    }

    /// A `/…/` literal: gawk's `yylex` finding its end, then its escapes
    /// resolved by [`escape::regexp`] -- here, as gawk does as soon as the
    /// token is read, so that its warnings come out in program order with the
    /// strings' (gawk's arrive interleaved, in the order the text has them).
    ///
    /// Finding the end is gawk's bracket count, transcribed with its quirks.
    /// A `/` inside a bracket expression does not end the regex, so the count
    /// has to know where brackets are: a `[` opens one when none is open, and
    /// a `[` followed by `:` opens another inside it (`[[:alpha:]/]`); a `]`
    /// closes one, except as the first member (`[]/]`, `[^]/]`). `[.` and
    /// `[=` are not counted -- so `/[[.x.]/]/` ends early in gawk, and here.
    /// A backslash keeps itself and the next character (`\/`, `\]`, `\[`), and
    /// before a newline it is a continuation and both vanish.
    fn ere_literal(&mut self) -> Result<Tok, String> {
        self.i = self.i.saturating_add(1);
        let mut source = Str::new();
        // Signed, as gawk's `int` is: a `]` with no bracket open takes it
        // below zero, and then a plain `[` no longer opens one -- so in
        // `/a]b[/` the second slash ends the regex, which then fails to
        // compile. A count that stopped at zero would read on past it.
        let mut in_brack: isize = 0;
        // Where in `source` the outermost `[` is, for the first-member rule.
        let mut b_index: Option<usize> = None;
        loop {
            let cur_index = source.len();
            let Some(c) = self.bump() else {
                return Err("unterminated regexp at end of file".to_string());
            };
            match c {
                b'[' => {
                    if self.peek() == Some(b':') || in_brack == 0 {
                        in_brack = in_brack.saturating_add(1);
                        if in_brack == 1 {
                            b_index = Some(cur_index);
                        }
                    }
                }
                b']' => {
                    let first_member = in_brack > 0
                        && b_index.is_some_and(|b| {
                            cur_index == b.saturating_add(1)
                                || (cur_index == b.saturating_add(2)
                                    && source.last() == Some(&b'^'))
                        });
                    if !first_member {
                        in_brack = in_brack.saturating_sub(1);
                        if in_brack == 0 {
                            b_index = None;
                        }
                    }
                }
                b'\\' => {
                    let mut e = self.bump();
                    if e == Some(b'\r') {
                        e = self.bump();
                    }
                    match e {
                        None => {
                            return Err(
                                "unterminated regexp ends with `\\' at end of file".to_string()
                            );
                        }
                        // A continuation: the backslash and the newline go.
                        Some(b'\n') => {}
                        Some(e) => {
                            source.push(b'\\');
                            source.push(e);
                        }
                    }
                    continue;
                }
                b'/' if in_brack <= 0 => {
                    let pattern = escape::regexp(&source, &mut self.warnings)
                        .map_err(|e| format!("fatal: {}", e.message()))?;
                    return Ok(Tok::Ere { source, pattern });
                }
                b'\n' => return Err("unterminated regexp".to_string()),
                _ => {}
            }
            source.push(c);
        }
    }
}

/// How the offending character is named in a syntax error.
///
/// Takes the *rest of the program text* rather than one byte, because the
/// answer is a character and a character can be several bytes. Our awk counts
/// characters everywhere else — `length`, `substr`, `index` and `toupper` all
/// diverge from gawk-in-the-C-locale for exactly that reason, and
/// `scripts/awk-diff.sh` records each divergence as deliberate — so naming the
/// *lead byte* here would be the one place it changed its mind, and would
/// report `\303` for a program whose stray character is `é`.
///
/// Bytes that decode to no character still show as one octal escape each, and
/// so does a character that is not printable: this defers the whole question
/// to `design-decisions.md` §357 rather than keeping a second opinion about
/// it. For every input the old byte-wise version could see — ASCII graphic,
/// space, control, or a byte that does not decode — the answer is unchanged.
fn shown_char(rest: &[u8]) -> String {
    let width = coreutils::quote::first_char(rest).map_or(1, |(_, n)| n);
    coreutils::quote::escape_unprintable(rest.get(..width).unwrap_or(rest))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
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

    /// A regex token whose escape layer changed nothing.
    fn ere(text: &[u8]) -> Tok {
        Tok::Ere {
            source: text.to_vec(),
            pattern: text.to_vec(),
        }
    }

    /// The tokens of `src`, and every warning the lexing produced.
    fn toks_warned(src: &[u8]) -> (Vec<Tok>, Vec<String>) {
        let mut w = Warnings::default();
        let mut said = Vec::new();
        let (toks, stopped) = Lexer::tokenize(src, &mut w, &mut said);
        assert!(stopped.is_none(), "{stopped:?}");
        let toks = toks.into_iter().map(|t| t.kind).collect();
        let said = said
            .into_iter()
            .map(|(_, m)| String::from_utf8(m).unwrap())
            .collect();
        (toks, said)
    }

    /// Each warning keeps the offset of the token that earned it.
    #[test]
    fn a_warning_is_tied_to_its_tokens_offset() {
        let mut w = Warnings::default();
        let mut said = Vec::new();
        let (_, stopped) = Lexer::tokenize(b"x = 1\ny = \"\\q\"\n", &mut w, &mut said);
        assert!(stopped.is_none());
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, 10, "the offset of the string literal");
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
                ere(b"x"),
                Tok::Eof
            ]
        );
        // At the start of a rule a slash is always a regex.
        assert_eq!(toks("/x/"), vec![ere(b"x"), Tok::Eof]);
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

    /// gawk 5.2.1 `--posix`, measured: `print "a\.b"` prints `a.b` and warns
    /// "escape sequence `\.' treated as plain `.'", once.
    #[test]
    fn string_escapes_are_gawks() {
        assert_eq!(
            toks(r#""a\tb""#),
            vec![Tok::Str(b"a\tb".to_vec()), Tok::Eof]
        );
        assert_eq!(toks(r#""\101""#), vec![Tok::Str(b"A".to_vec()), Tok::Eof]);
        // The low byte of the octal value, as gawk's `char` keeps.
        assert_eq!(toks(r#""\777""#), vec![Tok::Str(vec![0xff]), Tok::Eof]);
        assert_eq!(toks(r#""\400""#), vec![Tok::Str(vec![0]), Tok::Eof]);
        // `--posix`: `\x` is an `x`, and its digits are digits.
        assert_eq!(toks(r#""\x41""#), vec![Tok::Str(b"x41".to_vec()), Tok::Eof]);
        assert_eq!(
            toks(r#""a\"b""#),
            vec![Tok::Str(b"a\"b".to_vec()), Tok::Eof]
        );
        assert_eq!(
            toks(r#""a\\b""#),
            vec![Tok::Str(br"a\b".to_vec()), Tok::Eof]
        );
        // An escape awk does not know is the character itself, with a
        // warning -- said once for each character, however often it recurs.
        let (t, said) = toks_warned(br#"x = "a\.b\.c\q\/""#);
        assert_eq!(t.get(2), Some(&Tok::Str(b"a.b.cq/".to_vec())));
        assert_eq!(
            said,
            [
                "escape sequence `\\.' treated as plain `.'",
                "escape sequence `\\q' treated as plain `q'",
                "escape sequence `\\/' treated as plain `/'",
            ]
        );
    }

    /// A regex literal keeps its text for diagnostics, and hands the compiler
    /// that text through awk's escape layer. Measured against gawk 5.2.1
    /// `--posix`: `/^a\tb$/` matches a tab, `/^\101$/` matches `A`,
    /// `/^\x41$/` matches `x41`, and `/\y/` warns and matches `y`.
    #[test]
    fn regex_literal_escapes_are_gawks() {
        let lit = |source: &[u8], pattern: &[u8]| {
            vec![
                Tok::Ere {
                    source: source.to_vec(),
                    pattern: pattern.to_vec(),
                },
                Tok::Eof,
            ]
        };
        assert_eq!(toks(r"/a\tb/"), lit(br"a\tb", b"a\tb"));
        assert_eq!(toks(r"/\101/"), lit(br"\101", b"A"));
        assert_eq!(toks(r"/\x41/"), lit(br"\x41", b"x41"));
        // `\1` is octal here, not a backreference -- POSIX's awk table.
        assert_eq!(toks(r"/(.)\1/"), lit(br"(.)\1", b"(.)\x01"));
        // The regex compiler's own escapes go through untouched.
        assert_eq!(toks(r"/a\.b/"), lit(br"a\.b", br"a\.b"));
        assert_eq!(toks(r"/a\/b/"), lit(br"a\/b", br"a\/b"));
        assert_eq!(toks(r"/[\]]/"), lit(br"[\]]", br"[\]]"));
        // `\8` has no octal digit: the digit, its backslash gone.
        let (t, said) = toks_warned(br"/\8\y\w\./");
        assert_eq!(
            t.first(),
            Some(&Tok::Ere {
                source: br"\8\y\w\.".to_vec(),
                pattern: br"8\y\w\.".to_vec(),
            })
        );
        assert_eq!(
            said,
            [
                "regexp escape sequence `\\8' treated as plain `8'",
                "regexp escape sequence `\\y' is not a known regexp operator",
                "regexp escape sequence `\\w' is not a known regexp operator",
            ]
        );
    }

    /// The two layers keep separate "said it" tables, as gawk's do: `\q` in a
    /// regex and `\q` in a string each warn once.
    #[test]
    fn a_string_and_a_regex_each_warn_once() {
        let (_, said) = toks_warned(br#"/\q/ { s = "\q" } /\q/ { t = "\q" }"#);
        assert_eq!(
            said,
            [
                "regexp escape sequence `\\q' is not a known regexp operator",
                "escape sequence `\\q' treated as plain `q'",
            ]
        );
    }

    #[test]
    fn a_backslash_newline_in_a_string_is_fatal_under_posix() {
        assert_eq!(
            Lexer::new(b"\"a\\\nb\"").tokens().unwrap_err(),
            "fatal: POSIX does not allow physical newlines in string values"
        );
    }

    #[test]
    fn a_backslash_newline_in_a_regex_is_a_continuation() {
        assert_eq!(toks("/a\\\nb/"), vec![ere(b"ab"), Tok::Eof]);
        assert_eq!(toks("/a\\\r\nb/"), vec![ere(b"ab"), Tok::Eof]);
    }

    /// gawk's bracket count, quirks included; each measured against gawk
    /// 5.2.1 `--posix`.
    #[test]
    fn the_regex_end_is_found_by_gawks_bracket_count() {
        // A `[:` opens a second level, so its `]` does not close the bracket.
        assert_eq!(toks("/[[:alpha:]/]/"), vec![ere(b"[[:alpha:]/]"), Tok::Eof]);
        // A `]` first, or first after `^`, is a member.
        assert_eq!(toks("/[]/]/"), vec![ere(b"[]/]"), Tok::Eof]);
        assert_eq!(toks("/[^]/]/"), vec![ere(b"[^]/]"), Tok::Eof]);
        // A quoted `]` does not close it either.
        assert_eq!(toks(r"/[\]/]/"), vec![ere(br"[\]/]"), Tok::Eof]);
        // A stray `]` takes the count below zero, and a plain `[` after it
        // opens nothing: the second slash ends the regex.
        assert_eq!(
            toks("/a]b[/ x"),
            vec![ere(b"a]b["), Tok::Name("x".into()), Tok::Eof]
        );
    }

    #[test]
    fn a_slash_inside_a_bracket_expression_does_not_end_the_regex() {
        assert_eq!(toks("/[/]/"), vec![ere(b"[/]"), Tok::Eof]);
        assert_eq!(toks("/[^/]/"), vec![ere(b"[^/]"), Tok::Eof]);
    }

    #[test]
    fn numbers_in_every_shape_awk_accepts() {
        assert_eq!(
            toks("1 1.5 .5 1e3 1E-2 5. 011"),
            vec![
                Tok::Number(1.0),
                Tok::Number(1.5),
                Tok::Number(0.5),
                Tok::Number(1000.0),
                Tok::Number(0.01),
                Tok::Number(5.0),
                // Decimal, not octal: gawk's `--posix` is `--traditional`.
                Tok::Number(11.0),
                Tok::Eof
            ]
        );
    }

    /// gawk's `--posix` scanner stops a number at the `x` of `0x1f`, so it is
    /// the number 0 and then the name `x1f`: `print 0x1A` prints `0`.
    #[test]
    fn a_hexadecimal_constant_is_a_zero_and_a_name() {
        assert_eq!(
            toks("0x1f 1e 1.2.3"),
            vec![
                Tok::Number(0.0),
                Tok::Name("x1f".into()),
                Tok::Number(1.0),
                Tok::Name("e".into()),
                Tok::Number(1.2),
                Tok::Number(0.3),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn an_unterminated_literal_is_an_error_not_a_guess() {
        assert!(Lexer::new(b"\"abc").tokens().is_err());
        assert!(Lexer::new(b"/abc").tokens().is_err());
    }

    /// The stray character is named as a *character*, which is the same rule
    /// this awk applies to `length`, `substr` and `index`. Before this, a
    /// program containing `é` was reported as `syntax error at `\303'` — the
    /// lead byte, which is not something the user typed.
    #[test]
    fn a_stray_character_is_named_whole_not_by_its_lead_byte() {
        let err = |src: &[u8]| Lexer::new(src).tokens().unwrap_err();
        assert_eq!(err("BEGIN { é }".as_bytes()), "syntax error at `é'");
        assert_eq!(err("BEGIN { € }".as_bytes()), "syntax error at `€'");
        assert_eq!(err("BEGIN { 😀 }".as_bytes()), "syntax error at `😀'");
        // A byte that decodes to nothing still gets one octal escape, and only
        // its own: the `z` after it is a token, not part of the escape.
        assert_eq!(err(b"BEGIN { \xff }"), r"syntax error at `\377'");
        assert_eq!(err(b"BEGIN { \xc3z }"), r"syntax error at `\303'");
        // Every answer the byte-wise version used to give is unchanged.
        assert_eq!(err(b"BEGIN { @ }"), "syntax error at `@'");
        assert_eq!(err(b"BEGIN { \x01 }"), r"syntax error at `\001'");
        // A character that decodes but is not printable is escaped per byte,
        // exactly as `design-decisions.md` §357 requires.
        assert_eq!(
            err("BEGIN { \u{0080} }".as_bytes()),
            r"syntax error at `\302\200'"
        );
    }
}
