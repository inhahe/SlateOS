//! The scanner: `scan.l`'s rules, matched as flex matches them.
//!
//! At each position every rule that applies is tried; the longest match
//! wins, and of equal matches the rule written first -- so `define` is the
//! keyword and `defined` a name. The one start condition, the comment after
//! `#`, is inclusive (`%s`): in it the comment's two rules join the others.
//!
//! Input is read as upstream's flex scanner reads it: straight from the
//! descriptor, up to 8192 bytes at a time (`YY_READ_BUF_SIZE`), apart from
//! stdio -- which is what `read()` uses -- so the two read standard input
//! independently, as upstream's do. A match never runs past the end of the
//! file being read; at that end the next file is opened (`yywrap`).

use crate::Bc;
use crate::parse::{Sem, token};

/// `YY_READ_BUF_SIZE`.
const READ_SIZE: usize = 8192;

/// What the scanner reads: a file or standard input.
#[derive(Debug)]
pub enum Source {
    /// A descriptor, closed when the scanner moves on if `close`.
    Fd {
        /// The descriptor.
        fd: i32,
        /// Whether it is the scanner's to close.
        close: bool,
    },
    /// A program in memory, for the tests: its bytes and how far they are
    /// read.
    #[cfg(test)]
    Memory(Vec<u8>, usize),
}

impl Source {
    /// Standard input.
    #[must_use]
    pub const fn stdin() -> Self {
        Self::Fd {
            fd: 0,
            close: false,
        }
    }
}

/// The scanner's state between tokens.
#[derive(Debug, Default)]
pub struct Scanner {
    source: Option<Source>,
    /// Bytes read and not yet scanned, from `pos`.
    buf: Vec<u8>,
    pos: usize,
    /// The source has ended.
    eof: bool,
    /// `<slcomment>`: after a `#`.
    comment: bool,
    /// `yylval`, as the last token that set it left it.
    pub yylval: Sem,
}

impl Scanner {
    /// `fopen (name, "r")`, as a source.
    ///
    /// # Errors
    ///
    /// The `errno` of the failed open.
    pub fn open(name: &[u8]) -> Result<Source, i32> {
        let c = std::ffi::CString::new(name).map_err(|_| libcall::ENOENT)?;
        let fd = libcall::file::openat(libcall::file::AT_FDCWD, &c, libcall::file::O_RDONLY, 0)?;
        Ok(Source::Fd { fd, close: true })
    }

    /// `new_yy_file`, the flex half: the scanner reads `source` from its
    /// start, the old source closed if `close_old` (upstream's `fclose
    /// (yyin)`) and it is a file. Whatever was buffered is dropped, as
    /// flex's restart drops it.
    pub fn switch(&mut self, source: Source, close_old: bool) {
        if close_old {
            if let Some(Source::Fd { fd, close: true }) = self.source {
                // A file read to its end: a failed close has nothing to say.
                let _ = libcall::fd::close(fd);
            }
        }
        self.source = Some(source);
        self.buf.clear();
        self.pos = 0;
        self.eof = false;
    }

    /// More input: `Ok(false)` at the end of the source.
    fn fill(&mut self) -> Result<bool, i32> {
        if self.eof {
            return Ok(false);
        }
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let mut chunk = vec![0u8; READ_SIZE];
        let n = match &mut self.source {
            None => 0,
            Some(Source::Fd { fd, .. }) => loop {
                match libcall::fd::read(*fd, &mut chunk) {
                    Err(e) if e == libcall::EINTR => {}
                    Err(e) => return Err(e),
                    Ok(n) => break n,
                }
            },
            #[cfg(test)]
            Some(Source::Memory(bytes, at)) => {
                let rest = bytes.get(*at..).unwrap_or_default();
                let n = rest.len().min(READ_SIZE);
                if let (Some(dst), Some(src)) = (chunk.get_mut(..n), rest.get(..n)) {
                    dst.copy_from_slice(src);
                }
                *at = at.saturating_add(n);
                n
            }
        };
        if n == 0 {
            self.eof = true;
            return Ok(false);
        }
        self.buf
            .extend_from_slice(chunk.get(..n).unwrap_or_default());
        Ok(true)
    }
}

impl Bc {
    /// The byte `i` past the scan position, reading more of the source as
    /// needed; `None` past its end. A read error ends the program, as
    /// upstream's `rl_input` does.
    fn at(&mut self, i: usize) -> Option<u8> {
        loop {
            let want = self.scan.pos.saturating_add(i);
            if let Some(&b) = self.scan.buf.get(want) {
                return Some(b);
            }
            match self.scan.fill() {
                Ok(true) => {}
                Ok(false) => return None,
                Err(_) => {
                    self.yyerror(b"read() in flex scanner failed");
                    self.exit(1);
                }
            }
        }
    }

    /// `input ()`: the next byte, past the end of a file into the next one
    /// as flex's `input` goes (`yywrap`); `None` when there is none.
    fn input(&mut self) -> Option<u8> {
        loop {
            if let Some(b) = self.at(0) {
                self.scan.pos = self.scan.pos.saturating_add(1);
                return Some(b);
            }
            if !self.open_new_file() {
                return None;
            }
        }
    }

    /// The length of the longest match of a rule at the scan position, and
    /// the rule: the earliest of the longest.
    #[allow(
        clippy::too_many_lines,
        reason = "scan.l's rules, one by one, in their order"
    )]
    fn longest_match(&mut self) -> (usize, Rule) {
        let mut best = (0usize, Rule::Illegal);
        let consider = |len: usize, rule: Rule, best: &mut (usize, Rule)| {
            if len > best.0 {
                *best = (len, rule);
            }
        };
        let c0 = self.at(0);
        let c1 = self.at(1);
        // "#"
        if c0 == Some(b'#') {
            consider(1, Rule::Hash, &mut best);
        }
        // <slcomment>[^\n]* and <slcomment>"\n"
        if self.scan.comment {
            let mut n = 0usize;
            while let Some(b) = self.at(n) {
                if b == b'\n' {
                    break;
                }
                n = n.saturating_add(1);
            }
            consider(n, Rule::CommentText, &mut best);
            if c0 == Some(b'\n') {
                consider(1, Rule::CommentEnd, &mut best);
            }
        }
        // The keywords.
        for &(word, tok) in KEYWORDS {
            let matches = (0..word.len()).all(|i| self.at(i) == word.get(i).copied());
            if matches {
                consider(word.len(), Rule::Keyword(tok), &mut best);
            }
        }
        match c0 {
            Some(b'.') => consider(1, Rule::Dot, &mut best),
            Some(b'+' | b'-' | b';' | b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b'^') => {
                consider(1, Rule::Char, &mut best);
            }
            _ => {}
        }
        if c0 == Some(b'&') && c1 == Some(b'&') {
            consider(2, Rule::And, &mut best);
        }
        if c0 == Some(b'|') && c1 == Some(b'|') {
            consider(2, Rule::Or, &mut best);
        }
        if c0 == Some(b'!') {
            consider(1, Rule::Not, &mut best);
        }
        if matches!(c0, Some(b'*' | b'/' | b'%' | b'&')) {
            consider(1, Rule::Char2, &mut best);
        }
        // "=" and the op= forms.
        if c0 == Some(b'=') {
            consider(1, Rule::Assign, &mut best);
        }
        if matches!(c0, Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'^')) && c1 == Some(b'=') {
            consider(2, Rule::Assign, &mut best);
        }
        // The old =op forms, of which only the "=" is taken.
        if c0 == Some(b'=') && matches!(c1, Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'^')) {
            consider(2, Rule::OldAssign, &mut best);
        }
        // The comparisons.
        if matches!(c0, Some(b'=' | b'<' | b'>' | b'!')) && c1 == Some(b'=') {
            consider(2, Rule::Rel, &mut best);
        }
        if matches!(c0, Some(b'<' | b'>')) {
            consider(1, Rule::Rel, &mut best);
        }
        if (c0 == Some(b'+') && c1 == Some(b'+')) || (c0 == Some(b'-') && c1 == Some(b'-')) {
            consider(2, Rule::IncrDecr, &mut best);
        }
        if c0 == Some(b'\n') {
            consider(1, Rule::Newline, &mut best);
        }
        if c0 == Some(b'\\') && c1 == Some(b'\n') {
            consider(2, Rule::QuotedNewline, &mut best);
        }
        if matches!(c0, Some(b' ' | b'\t')) {
            let mut n = 1usize;
            while matches!(self.at(n), Some(b' ' | b'\t')) {
                n = n.saturating_add(1);
            }
            consider(n, Rule::Blank, &mut best);
        }
        if c0 == Some(b'/') && c1 == Some(b'*') {
            consider(2, Rule::CComment, &mut best);
        }
        if matches!(c0, Some(b'a'..=b'z')) {
            let mut n = 1usize;
            while matches!(self.at(n), Some(b'a'..=b'z' | b'0'..=b'9' | b'_')) {
                n = n.saturating_add(1);
            }
            consider(n, Rule::Name, &mut best);
        }
        if c0 == Some(b'"') {
            let mut n = 1usize;
            loop {
                match self.at(n) {
                    None => break,
                    Some(b'"') => {
                        consider(n.saturating_add(1), Rule::String, &mut best);
                        break;
                    }
                    Some(_) => n = n.saturating_add(1),
                }
            }
        }
        let number = self.number_length();
        if number > 0 {
            consider(number, Rule::Number, &mut best);
        }
        if c0.is_some() && c0 != Some(b'\n') {
            consider(1, Rule::Illegal, &mut best);
        }
        best
    }

    /// The length of `{DIGIT}({DIGIT}|\\\n)*("."({DIGIT}|\\\n)*)?` or
    /// `"."(\\\n)*{DIGIT}({DIGIT}|\\\n)*` at the scan position, the longer;
    /// 0 for neither. `DIGIT` is `[0-9A-Z]`.
    fn number_length(&mut self) -> usize {
        let digit = |b: Option<u8>| matches!(b, Some(b'0'..=b'9' | b'A'..=b'Z'));
        // Digits and quoted newlines from `n` on: where they end.
        let run = |this: &mut Self, mut n: usize| -> usize {
            loop {
                if digit(this.at(n)) {
                    n = n.saturating_add(1);
                } else if this.at(n) == Some(b'\\') && this.at(n.saturating_add(1)) == Some(b'\n') {
                    n = n.saturating_add(2);
                } else {
                    return n;
                }
            }
        };
        let mut first = 0usize;
        if digit(self.at(0)) {
            first = run(self, 1);
            if self.at(first) == Some(b'.') {
                first = run(self, first.saturating_add(1));
            }
        }
        let mut second = 0usize;
        if self.at(0) == Some(b'.') {
            let mut n = 1usize;
            while self.at(n) == Some(b'\\') && self.at(n.saturating_add(1)) == Some(b'\n') {
                n = n.saturating_add(2);
            }
            if digit(self.at(n)) {
                second = run(self, n.saturating_add(1));
            }
        }
        first.max(second)
    }

    /// `yylex ()`: the next token, `yylval` set as its rule sets it; 0 at
    /// the end of the input.
    #[allow(clippy::too_many_lines, reason = "scan.l's actions, one by one")]
    pub fn yylex(&mut self) -> i32 {
        loop {
            if self.at(0).is_none() {
                // `yywrap`: the next file, or the end.
                if self.open_new_file() {
                    continue;
                }
                return 0;
            }
            let (len, rule) = self.longest_match();
            let start = self.scan.pos;
            let end = start.saturating_add(len);
            let text = self.scan.buf.get(start..end).unwrap_or_default().to_vec();
            self.scan.pos = end;
            let first = text.first().copied().unwrap_or(0);
            match rule {
                Rule::Hash => {
                    if self.std_only {
                        self.yyerror(b"illegal character: #");
                    } else {
                        self.scan.comment = true;
                    }
                }
                Rule::CommentText => self.scan.comment = false,
                Rule::CommentEnd => {
                    self.line_no = self.line_no.saturating_add(1);
                    self.scan.comment = false;
                    return token::ENDOFLINE;
                }
                Rule::Keyword(tok) => return tok,
                Rule::Dot => return token::LAST,
                Rule::Char | Rule::Char2 => {
                    self.scan.yylval = Sem::Char(first);
                    return i32::from(first);
                }
                Rule::And => return token::AND,
                Rule::Or => return token::OR,
                Rule::Not => return token::NOT,
                Rule::Assign => {
                    self.scan.yylval = Sem::Char(first);
                    return token::ASSIGN_OP;
                }
                Rule::OldAssign => {
                    // `yyless (1)`: only the "=" is taken.
                    self.scan.pos = start.saturating_add(1);
                    self.scan.yylval = Sem::Char(b'=');
                    return token::ASSIGN_OP;
                }
                Rule::Rel => {
                    self.scan.yylval = Sem::Str(text);
                    return token::REL_OP;
                }
                Rule::IncrDecr => {
                    self.scan.yylval = Sem::Char(first);
                    return token::INCR_DECR;
                }
                Rule::Newline => {
                    self.line_no = self.line_no.saturating_add(1);
                    return token::ENDOFLINE;
                }
                Rule::QuotedNewline => self.line_no = self.line_no.saturating_add(1),
                Rule::Blank => {}
                Rule::CComment => self.c_comment(),
                Rule::Name => {
                    self.scan.yylval = Sem::Str(text);
                    return token::NAME;
                }
                Rule::String => return self.string_token(&text),
                Rule::Number => {
                    let number = self.number_action(start, end);
                    self.scan.yylval = Sem::Str(number);
                    return token::NUMBER;
                }
                Rule::Illegal => self.illegal(first),
            }
        }
    }

    /// The `"/*"` rule's action: up to `*/`, through `input ()`.
    fn c_comment(&mut self) {
        loop {
            let mut c = self.input();
            while c != Some(b'*') && c.is_some() {
                if c == Some(b'\n') {
                    self.line_no = self.line_no.saturating_add(1);
                }
                c = self.input();
            }
            if c == Some(b'*') {
                loop {
                    c = self.input();
                    if c != Some(b'*') {
                        break;
                    }
                }
                if c == Some(b'/') {
                    break;
                }
                if c == Some(b'\n') {
                    self.line_no = self.line_no.saturating_add(1);
                }
            }
            if c.is_none() {
                self.err.write(b"EOF encountered in a comment.\n");
                self.check_stderr();
                break;
            }
        }
    }

    /// The string rule's action: the text up to its first NUL (as
    /// `strcopyof` copies it), the newlines before that NUL counted, and a
    /// NUL complained of.
    fn string_token(&mut self, text: &[u8]) -> i32 {
        let upto = text.iter().position(|&b| b == 0).unwrap_or(text.len());
        let seen = text.get(..upto).unwrap_or_default();
        let mut quotes = 0usize;
        for &b in seen {
            if b == b'\n' {
                self.line_no = self.line_no.saturating_add(1);
            }
            if b == b'"' {
                quotes = quotes.saturating_add(1);
            }
        }
        self.scan.yylval = Sem::Str(seen.to_vec());
        if quotes != 2 {
            self.yyerror(b"NUL character in string.");
        }
        token::STRING
    }

    /// The number rule's action on the token at `start..end` of the buffer:
    /// a trailing point dropped, leading zeros dropped (all but one of a
    /// zero), quoted newlines removed and counted; the number's text.
    ///
    /// Upstream's loop compacts the token in place, in flex's buffer, where
    /// the token is ended by a NUL put over the byte after it (kept aside,
    /// and put back before the next token) and the data by two more. After
    /// skipping a quoted newline it copies the next byte whatever it is --
    /// so a number that ends in one copies that NUL and runs on, shifting
    /// the rest of the buffer two bytes down, counting each backslash there
    /// as a line (and taking out each comma, a non-standard one), until it
    /// meets a NUL, over which -- two bytes short of it -- it writes
    /// another. What the scanner then reads past the token is that shifted
    /// text, the byte kept aside first: the same memory is laid out here,
    /// and written back to the buffer.
    fn number_action(&mut self, start: usize, end: usize) -> Vec<u8> {
        let data = self.scan.buf.get(start..).unwrap_or_default().to_vec();
        let len = end.saturating_sub(start);
        let mut mem = data.clone();
        mem.extend_from_slice(&[0, 0]);
        let hold = mem.get(len).copied().unwrap_or(0);
        let put = |mem: &mut Vec<u8>, i: usize, b: u8| {
            if let Some(slot) = mem.get_mut(i) {
                *slot = b;
            }
        };
        put(&mut mem, len, 0);
        if mem.get(len.wrapping_sub(1)) == Some(&b'.') {
            put(&mut mem, len.wrapping_sub(1), 0);
        }
        let at = |mem: &Vec<u8>, i: usize| mem.get(i).copied().unwrap_or(0);
        let mut src = 0usize;
        while at(&mem, src) == b'0' {
            src = src.saturating_add(1);
        }
        if at(&mem, src) == 0 {
            src = src.saturating_sub(1);
        }
        let mut dst = 0usize;
        while at(&mem, src) != 0 {
            if at(&mem, src) == b'\\' {
                src = src.saturating_add(2);
                self.line_no = self.line_no.saturating_add(1);
            }
            if at(&mem, src) == b',' {
                src = src.saturating_add(1);
                self.ct_warn(b"Commas in numbers");
            } else {
                let b = at(&mem, src);
                put(&mut mem, dst, b);
                dst = dst.saturating_add(1);
                src = src.saturating_add(1);
            }
        }
        put(&mut mem, dst, 0);
        let number: Vec<u8> = mem.iter().take_while(|&&b| b != 0).copied().collect();
        // flex puts the kept byte back, and reads on from there.
        put(&mut mem, len, hold);
        if let Some(rest) = mem.get(len..data.len()) {
            let rest = rest.to_vec();
            self.scan.buf.truncate(end);
            self.scan.buf.extend_from_slice(&rest);
        }
        number
    }

    /// The `.` rule's action: an illegal character -- a control character as
    /// `^X`, one past `~` as an octal escape of its byte.
    fn illegal(&mut self, b: u8) {
        let msg = if b < b' ' {
            let mut m = b"illegal character: ^".to_vec();
            m.push(b.wrapping_add(b'@'));
            m
        } else if b > b'~' {
            format!("illegal character: \\{b:03o}").into_bytes()
        } else {
            let mut m = b"illegal character: ".to_vec();
            m.push(b);
            m
        };
        self.yyerror(&msg);
    }
}

/// Which rule matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Hash,
    CommentText,
    CommentEnd,
    Keyword(i32),
    Dot,
    Char,
    And,
    Or,
    Not,
    Char2,
    Assign,
    OldAssign,
    Rel,
    IncrDecr,
    Newline,
    QuotedNewline,
    Blank,
    CComment,
    Name,
    String,
    Number,
    Illegal,
}

/// The keywords, in `scan.l`'s order. `history` is readline's build's.
const KEYWORDS: &[(&[u8], i32)] = &[
    (b"define", token::DEFINE),
    (b"break", token::BREAK),
    (b"quit", token::QUIT),
    (b"length", token::LENGTH),
    (b"return", token::RETURN),
    (b"for", token::FOR),
    (b"if", token::IF),
    (b"while", token::WHILE),
    (b"sqrt", token::SQRT),
    (b"scale", token::SCALE),
    (b"ibase", token::IBASE),
    (b"obase", token::OBASE),
    (b"auto", token::AUTO),
    (b"else", token::ELSE),
    (b"read", token::READ),
    (b"random", token::RANDOM),
    (b"halt", token::HALT),
    (b"last", token::LAST),
    (b"void", token::VOID),
    (b"history", token::HISTORY_VAR),
    (b"warranty", token::WARRANTY),
    (b"continue", token::CONTINUE),
    (b"print", token::PRINT),
    (b"limits", token::LIMITS),
];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::Bc;
    use crate::io::{Input, Output, Sink, Source};

    fn bc_over(buf: &[u8]) -> Bc {
        let mut bc = Bc::new(
            Output::new(Sink::Memory(Vec::new()), false, false),
            Output::new(Sink::Memory(Vec::new()), true, false),
            Input::new(Source::Memory(Vec::new(), 0)),
        );
        bc.scan.buf = buf.to_vec();
        bc.scan.eof = true;
        bc.is_std_in = true;
        bc
    }

    #[test]
    fn a_number_is_compacted_in_place() {
        let mut bc = bc_over(b"0012.;");
        assert_eq!(bc.number_action(0, 5), b"12");
        assert_eq!(bc.scan.buf, b"0012.;", "nothing past the token moves");
    }

    /// Ending in a quoted newline, the copy runs past the token: the rest
    /// shifts two bytes down, two of its bytes are lost, a NUL is left in
    /// it, and its last byte is read twice.
    #[test]
    fn a_number_ending_in_a_quoted_newline_runs_on_through_the_buffer() {
        let input = [b'1', b'0', 92, b'\n', b'X', b'Y', b'Z', b';'];
        let mut bc = bc_over(&input);
        let lines = bc.line_no;
        assert_eq!(bc.number_action(0, 4), b"10");
        assert_eq!(bc.line_no, lines + 1);
        assert_eq!(bc.scan.buf.get(4..).unwrap(), [b'X', b';', 0, b';']);
    }

    #[test]
    fn an_illegal_byte_is_named_as_upstream_names_it() {
        for (byte, said) in [
            (1u8, &b"^A"[..]),
            (0, b"^@"),
            (0x7f, b"\\177"),
            (0x80, b"\\200"),
            (0xff, b"\\377"),
            (b'@', b"@"),
        ] {
            let mut bc = bc_over(b"");
            bc.illegal(byte);
            let mut want = b"(standard_in) 1: illegal character: ".to_vec();
            want.extend_from_slice(said);
            want.push(b'\n');
            assert_eq!(bc.err.take_written(), want, "{byte:#x}");
        }
    }
}
