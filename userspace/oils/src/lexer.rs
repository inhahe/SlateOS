//! Tokenizer for the OSH shell language.
//!
//! The lexer turns source text into a flat token stream. Words are captured as
//! a list of [`Seg`] fragments that preserve quoting; command/parameter/
//! arithmetic substitutions keep their *raw inner source* so the parser can
//! recursively parse them (this keeps the lexer free of a dependency on the
//! parser).

/// A lexer error with a human-readable message (unbalanced quote, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError(pub String);

impl core::fmt::Display for LexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Shell operators recognised outside of words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Pipe,
    AndIf,
    OrIf,
    Amp,
    Semi,
    /// `;;` — terminates a `case` arm.
    DSemi,
    LParen,
    RParen,
    Less,
    Great,
    DGreat,
    GreatAnd,
    LessAnd,
    /// `<<` — here-document.
    DLess,
    /// `<<-` — here-document with leading-tab stripping.
    DLessDash,
    /// `<<<` — here-string.
    TLess,
}

/// A word fragment, preserving quoting for later expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    /// Unquoted literal run.
    Lit(String),
    /// Single-quoted run (verbatim).
    Sq(String),
    /// Double-quoted run of fragments.
    Dq(Vec<Seg>),
    /// `$name` / `$1` / `$?` … a bare parameter reference.
    Param(String),
    /// `${ … }` — raw inner text, parsed later.
    ParamBraced(String),
    /// `$( … )` / `` ` … ` `` — raw inner source, parsed later.
    CmdSub(String),
    /// `$(( … ))` — raw arithmetic expression text.
    Arith(String),
}

/// One token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Word(Vec<Seg>),
    /// An IO number: digits immediately preceding a redirection operator.
    Io(i32),
    Op(Op),
    Newline,
    /// A here-document body, captured after its introducing line. Emitted
    /// immediately after the `<<`/`<<-` operator token that owns it.
    HereDoc(Vec<Seg>),
    /// `(( … ))` — an arithmetic command, holding the raw expression text.
    ArithCmd(String),
    /// `name=( … )` / `name+=( … )` — an array assignment. Each element is a
    /// word captured as its own [`Seg`] list.
    ArrayAssign {
        name: String,
        /// `+=` (append) rather than `=`.
        append: bool,
        elems: Vec<Vec<Seg>>,
    },
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    /// Here-documents whose bodies are pending collection at the next newline.
    pending_heredocs: Vec<PendingHeredoc>,
}

/// A here-document awaiting its body (collected when the introducing line ends).
struct PendingHeredoc {
    /// The end delimiter (unquoted form).
    delim: String,
    /// `<<-`: strip leading tabs from body lines and the closing delimiter.
    strip: bool,
    /// Whether the body undergoes parameter/command/arith expansion (false when
    /// the delimiter was quoted).
    expand: bool,
    /// Index into the output token stream of the placeholder to fill in.
    tok_index: usize,
}

/// Tokenize `src` into a token stream.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize(src: &str) -> Result<Vec<Tok>, LexError> {
    let mut lx = Lexer {
        chars: src.chars().collect(),
        pos: 0,
        pending_heredocs: Vec::new(),
    };
    lx.run()
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<char> {
        self.chars.get(self.pos + off).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn run(&mut self) -> Result<Vec<Tok>, LexError> {
        let mut out = Vec::new();
        loop {
            // Skip inline blanks (but not newlines — those are tokens).
            while matches!(self.peek(), Some(' ' | '\t')) {
                self.pos += 1;
            }
            let Some(c) = self.peek() else { break };
            match c {
                '\n' => {
                    self.pos += 1;
                    out.push(Tok::Newline);
                    if !self.pending_heredocs.is_empty() {
                        self.collect_heredocs(&mut out)?;
                    }
                }
                '\r' => {
                    // Treat a bare CR (or CRLF) as insignificant whitespace so
                    // CRLF-terminated scripts parse the same as LF ones.
                    self.pos += 1;
                }
                '#' => {
                    // Comment to end of line.
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                }
                '|' => {
                    self.pos += 1;
                    if self.peek() == Some('|') {
                        self.pos += 1;
                        out.push(Tok::Op(Op::OrIf));
                    } else {
                        out.push(Tok::Op(Op::Pipe));
                    }
                }
                '&' => {
                    self.pos += 1;
                    if self.peek() == Some('&') {
                        self.pos += 1;
                        out.push(Tok::Op(Op::AndIf));
                    } else {
                        out.push(Tok::Op(Op::Amp));
                    }
                }
                ';' => {
                    self.pos += 1;
                    if self.peek() == Some(';') {
                        self.pos += 1;
                        out.push(Tok::Op(Op::DSemi));
                    } else {
                        out.push(Tok::Op(Op::Semi));
                    }
                }
                '(' => {
                    self.pos += 1;
                    // `((` (with no intervening space) begins an arithmetic
                    // command; `( (` (a space between) is nested subshells.
                    if self.peek() == Some('(') {
                        self.pos += 1;
                        let raw = self.read_arith()?;
                        out.push(Tok::ArithCmd(raw));
                    } else {
                        out.push(Tok::Op(Op::LParen));
                    }
                }
                ')' => {
                    self.pos += 1;
                    out.push(Tok::Op(Op::RParen));
                }
                '<' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('&') => {
                            self.pos += 1;
                            out.push(Tok::Op(Op::LessAnd));
                        }
                        Some('<') => {
                            self.pos += 1;
                            if self.peek() == Some('<') {
                                // `<<<` here-string: the target is an ordinary
                                // word parsed on this line.
                                self.pos += 1;
                                out.push(Tok::Op(Op::TLess));
                            } else {
                                self.lex_heredoc_op(&mut out);
                            }
                        }
                        _ => out.push(Tok::Op(Op::Less)),
                    }
                }
                '>' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('>') => {
                            self.pos += 1;
                            out.push(Tok::Op(Op::DGreat));
                        }
                        Some('&') => {
                            self.pos += 1;
                            out.push(Tok::Op(Op::GreatAnd));
                        }
                        _ => out.push(Tok::Op(Op::Great)),
                    }
                }
                '0'..='9' => {
                    // Possibly an IO number (digits directly before < or >).
                    let start = self.pos;
                    let mut i = self.pos;
                    while matches!(self.chars.get(i), Some('0'..='9')) {
                        i += 1;
                    }
                    if matches!(self.chars.get(i), Some('<' | '>')) {
                        let digits: String = self.chars[start..i].iter().collect();
                        self.pos = i;
                        // A numeric fd always fits in i32 for realistic input;
                        // fall back to a word if it somehow doesn't parse.
                        if let Ok(n) = digits.parse::<i32>() {
                            out.push(Tok::Io(n));
                        } else {
                            out.push(Tok::Word(vec![Seg::Lit(digits)]));
                        }
                    } else {
                        out.push(Tok::Word(self.read_word()?));
                    }
                }
                c if is_name_start(c) => {
                    // A leading identifier may begin an array assignment
                    // `name=( … )` / `name+=( … )`; otherwise it's a plain word.
                    if let Some(tok) = self.try_array_assign()? {
                        out.push(tok);
                    } else {
                        out.push(Tok::Word(self.read_word()?));
                    }
                }
                _ => out.push(Tok::Word(self.read_word()?)),
            }
        }
        Ok(out)
    }

    /// Try to lex an array assignment `name=( … )` / `name+=( … )` at the current
    /// position. Returns `None` (and restores the position) if the input does
    /// not match that shape, so a plain word is read instead.
    fn try_array_assign(&mut self) -> Result<Option<Tok>, LexError> {
        let start = self.pos;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if is_name_char(c) {
                name.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        let append = self.peek() == Some('+');
        let eq_at = self.pos + usize::from(append);
        if name.is_empty()
            || self.chars.get(eq_at) != Some(&'=')
            || self.chars.get(eq_at + 1) != Some(&'(')
        {
            self.pos = start;
            return Ok(None);
        }
        // Commit: consume the optional `+`, the `=`, and the `(`.
        self.pos = eq_at + 2;
        let mut elems: Vec<Vec<Seg>> = Vec::new();
        loop {
            while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                self.pos += 1;
            }
            match self.peek() {
                Some(')') => {
                    self.pos += 1;
                    break;
                }
                None => {
                    return Err(LexError(
                        "unterminated array assignment (expected ')')".into(),
                    ));
                }
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                }
                _ => {
                    let segs = self.read_word()?;
                    if segs.is_empty() {
                        return Err(LexError(
                            "unexpected operator in array assignment".into(),
                        ));
                    }
                    elems.push(segs);
                }
            }
        }
        Ok(Some(Tok::ArrayAssign {
            name,
            append,
            elems,
        }))
    }

    /// Read one word (until an unquoted operator, blank, or newline).
    fn read_word(&mut self) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' | '|' | '&' | ';' | '(' | ')' | '<' | '>' | '#' => break,
                '\'' => {
                    flush_lit(&mut segs, &mut lit);
                    self.pos += 1;
                    let s = self.read_single_quote()?;
                    segs.push(Seg::Sq(s));
                }
                '"' => {
                    flush_lit(&mut segs, &mut lit);
                    self.pos += 1;
                    let inner = self.read_double_quote()?;
                    segs.push(Seg::Dq(inner));
                }
                '`' => {
                    flush_lit(&mut segs, &mut lit);
                    self.pos += 1;
                    let raw = self.read_backtick()?;
                    segs.push(Seg::CmdSub(raw));
                }
                '\\' => {
                    self.pos += 1;
                    if let Some(next) = self.bump()
                        && next != '\n'
                    {
                        lit.push(next);
                    }
                }
                '$' => {
                    if let Some(seg) = self.read_dollar()? {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    } else {
                        lit.push('$');
                    }
                }
                other => {
                    lit.push(other);
                    self.pos += 1;
                }
            }
        }
        flush_lit(&mut segs, &mut lit);
        Ok(segs)
    }

    fn read_single_quote(&mut self) -> Result<String, LexError> {
        let mut s = String::new();
        loop {
            match self.bump() {
                Some('\'') => return Ok(s),
                Some(c) => s.push(c),
                None => return Err(LexError("unterminated single quote".into())),
            }
        }
    }

    fn read_double_quote(&mut self) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(LexError("unterminated double quote".into()));
            };
            match c {
                '"' => {
                    self.pos += 1;
                    flush_lit(&mut segs, &mut lit);
                    return Ok(segs);
                }
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        // Inside double quotes, backslash only escapes these.
                        Some(n @ ('"' | '\\' | '$' | '`')) => {
                            self.pos += 1;
                            lit.push(n);
                        }
                        Some('\n') => {
                            self.pos += 1;
                        }
                        _ => lit.push('\\'),
                    }
                }
                '`' => {
                    self.pos += 1;
                    flush_lit(&mut segs, &mut lit);
                    let raw = self.read_backtick()?;
                    segs.push(Seg::CmdSub(raw));
                }
                '$' => {
                    if let Some(seg) = self.read_dollar()? {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    } else {
                        lit.push('$');
                    }
                }
                other => {
                    lit.push(other);
                    self.pos += 1;
                }
            }
        }
    }

    /// Handle a `$`. Returns `None` if it is a literal `$` (e.g. `$` at EOL).
    fn read_dollar(&mut self) -> Result<Option<Seg>, LexError> {
        // Consume the `$`.
        self.pos += 1;
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let raw = self.read_balanced('{', '}')?;
                Ok(Some(Seg::ParamBraced(raw)))
            }
            Some('(') => {
                if self.peek_at(1) == Some('(') {
                    self.pos += 2;
                    let raw = self.read_arith()?;
                    Ok(Some(Seg::Arith(raw)))
                } else {
                    self.pos += 1;
                    let raw = self.read_balanced('(', ')')?;
                    Ok(Some(Seg::CmdSub(raw)))
                }
            }
            Some(c) if is_name_start(c) => {
                let mut name = String::new();
                while let Some(n) = self.peek() {
                    if is_name_char(n) {
                        name.push(n);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(Some(Seg::Param(name)))
            }
            Some(c @ ('?' | '#' | '@' | '*' | '!' | '$' | '-')) => {
                self.pos += 1;
                Ok(Some(Seg::Param(c.to_string())))
            }
            Some(c @ '0'..='9') => {
                self.pos += 1;
                Ok(Some(Seg::Param(c.to_string())))
            }
            _ => Ok(None),
        }
    }

    /// Read text until the matching `close`, honoring nested `open`/`close`
    /// and skipping quoted spans. `self.pos` is just past the initial `open`.
    fn read_balanced(&mut self, open: char, close: char) -> Result<String, LexError> {
        let mut depth = 1usize;
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(LexError(format!("unterminated '{open}{close}' expansion")));
            };
            if c == '\'' {
                raw.push(c);
                // Copy verbatim to the closing single quote.
                loop {
                    match self.bump() {
                        Some('\'') => {
                            raw.push('\'');
                            break;
                        }
                        Some(q) => raw.push(q),
                        None => return Err(LexError("unterminated single quote".into())),
                    }
                }
                continue;
            }
            if c == '"' {
                raw.push(c);
                loop {
                    match self.bump() {
                        Some('\\') => {
                            raw.push('\\');
                            if let Some(n) = self.bump() {
                                raw.push(n);
                            }
                        }
                        Some('"') => {
                            raw.push('"');
                            break;
                        }
                        Some(q) => raw.push(q),
                        None => return Err(LexError("unterminated double quote".into())),
                    }
                }
                continue;
            }
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(raw);
                }
            }
            raw.push(c);
        }
    }

    /// Read a `$(( … ))` body (up to the closing `))`).
    fn read_arith(&mut self) -> Result<String, LexError> {
        let mut depth = 0usize;
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(LexError("unterminated arithmetic expansion".into()));
            };
            match c {
                '(' => {
                    depth += 1;
                    raw.push(c);
                }
                ')' => {
                    if depth == 0 {
                        // Expect a second ')'.
                        if self.peek() == Some(')') {
                            self.pos += 1;
                            return Ok(raw);
                        }
                        return Err(LexError("malformed arithmetic expansion".into()));
                    }
                    depth -= 1;
                    raw.push(c);
                }
                _ => raw.push(c),
            }
        }
    }

    /// Handle a `<<` / `<<-` here-document operator: read the delimiter word on
    /// the current line, emit the operator token plus a placeholder body token,
    /// and record the here-doc for body collection at the next newline.
    fn lex_heredoc_op(&mut self, out: &mut Vec<Tok>) {
        let strip = self.peek() == Some('-');
        if strip {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
        let (delim, expand) = self.read_heredoc_delim();
        out.push(Tok::Op(if strip { Op::DLessDash } else { Op::DLess }));
        let tok_index = out.len();
        out.push(Tok::HereDoc(Vec::new()));
        self.pending_heredocs.push(PendingHeredoc {
            delim,
            strip,
            expand,
            tok_index,
        });
    }

    /// Read a here-document delimiter word. Any quoting (`'EOF'`, `"EOF"`,
    /// `\EOF`) disables expansion of the body and is stripped from the delimiter.
    fn read_heredoc_delim(&mut self) -> (String, bool) {
        let mut delim = String::new();
        let mut expand = true;
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '(' | ')' => break,
                '\'' => {
                    expand = false;
                    self.pos += 1;
                    while let Some(q) = self.bump() {
                        if q == '\'' {
                            break;
                        }
                        delim.push(q);
                    }
                }
                '"' => {
                    expand = false;
                    self.pos += 1;
                    while let Some(q) = self.bump() {
                        if q == '"' {
                            break;
                        }
                        delim.push(q);
                    }
                }
                '\\' => {
                    expand = false;
                    self.pos += 1;
                    if let Some(n) = self.bump() {
                        delim.push(n);
                    }
                }
                other => {
                    delim.push(other);
                    self.pos += 1;
                }
            }
        }
        (delim, expand)
    }

    /// Collect the bodies of all pending here-documents from the lines following
    /// the just-consumed newline, in order, filling in their placeholder tokens.
    fn collect_heredocs(&mut self, out: &mut [Tok]) -> Result<(), LexError> {
        let pending = core::mem::take(&mut self.pending_heredocs);
        for ph in pending {
            let mut body = String::new();
            loop {
                if self.pos >= self.chars.len() {
                    break; // EOF before the delimiter: accept what we have.
                }
                let start = self.pos;
                while !matches!(self.peek(), None | Some('\n')) {
                    self.pos += 1;
                }
                let mut line: String = self.chars[start..self.pos].iter().collect();
                if self.peek() == Some('\n') {
                    self.pos += 1;
                }
                if line.ends_with('\r') {
                    line.pop();
                }
                let content = if ph.strip {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if content == ph.delim {
                    break;
                }
                body.push_str(content);
                body.push('\n');
            }
            let segs = scan_heredoc_segs(&body, ph.expand)?;
            if let Some(slot) = out.get_mut(ph.tok_index) {
                *slot = Tok::HereDoc(segs);
            }
        }
        Ok(())
    }

    fn read_backtick(&mut self) -> Result<String, LexError> {
        let mut raw = String::new();
        loop {
            match self.bump() {
                Some('`') => return Ok(raw),
                Some('\\') => {
                    // Inside backticks, `\`` and `\\` and `\$` are unescaped.
                    match self.peek() {
                        Some(n @ ('`' | '\\' | '$')) => {
                            self.pos += 1;
                            raw.push(n);
                        }
                        _ => raw.push('\\'),
                    }
                }
                Some(c) => raw.push(c),
                None => return Err(LexError("unterminated backtick".into())),
            }
        }
    }
}

fn flush_lit(segs: &mut Vec<Seg>, lit: &mut String) {
    if !lit.is_empty() {
        segs.push(Seg::Lit(core::mem::take(lit)));
    }
}

/// Lower a here-document body into segments. When `expand` is false (quoted
/// delimiter) the whole body is a single literal; otherwise it is scanned like a
/// double-quoted context (parameter/command/arith expansion, `"` literal).
fn scan_heredoc_segs(body: &str, expand: bool) -> Result<Vec<Seg>, LexError> {
    if !expand {
        return Ok(vec![Seg::Lit(body.to_string())]);
    }
    let mut lx = Lexer {
        chars: body.chars().collect(),
        pos: 0,
        pending_heredocs: Vec::new(),
    };
    let mut segs: Vec<Seg> = Vec::new();
    let mut lit = String::new();
    while let Some(c) = lx.peek() {
        match c {
            '\\' => {
                lx.pos += 1;
                match lx.peek() {
                    Some(n @ ('$' | '`' | '\\')) => {
                        lx.pos += 1;
                        lit.push(n);
                    }
                    Some('\n') => {
                        lx.pos += 1;
                    }
                    _ => lit.push('\\'),
                }
            }
            '`' => {
                lx.pos += 1;
                flush_lit(&mut segs, &mut lit);
                segs.push(Seg::CmdSub(lx.read_backtick()?));
            }
            '$' => {
                if let Some(seg) = lx.read_dollar()? {
                    flush_lit(&mut segs, &mut lit);
                    segs.push(seg);
                } else {
                    lit.push('$');
                }
            }
            other => {
                lit.push(other);
                lx.pos += 1;
            }
        }
    }
    flush_lit(&mut segs, &mut lit);
    Ok(segs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_words() {
        let toks = tokenize("echo hello world").unwrap();
        assert_eq!(toks.len(), 3);
        assert!(matches!(toks[0], Tok::Word(_)));
    }

    #[test]
    fn operators() {
        let toks = tokenize("a | b && c || d ; e &").unwrap();
        let ops: Vec<_> = toks
            .iter()
            .filter_map(|t| if let Tok::Op(o) = t { Some(*o) } else { None })
            .collect();
        assert_eq!(
            ops,
            vec![Op::Pipe, Op::AndIf, Op::OrIf, Op::Semi, Op::Amp]
        );
    }

    #[test]
    fn quotes_and_params() {
        let toks = tokenize(r#"echo "hi $name" 'raw $x' $y"#).unwrap();
        assert!(matches!(toks[0], Tok::Word(_)));
        assert_eq!(toks.len(), 4);
    }

    #[test]
    fn command_sub_and_arith() {
        let toks = tokenize("echo $(date) $((1 + 2))").unwrap();
        if let Tok::Word(segs) = &toks[1] {
            assert!(matches!(segs[0], Seg::CmdSub(_)));
        } else {
            panic!("expected word");
        }
        if let Tok::Word(segs) = &toks[2] {
            assert!(matches!(segs[0], Seg::Arith(_)));
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn io_number() {
        let toks = tokenize("cmd 2> err").unwrap();
        assert!(toks.iter().any(|t| matches!(t, Tok::Io(2))));
    }

    #[test]
    fn nested_command_sub() {
        let toks = tokenize("echo $(echo $(echo x))").unwrap();
        if let Tok::Word(segs) = &toks[1] {
            match &segs[0] {
                Seg::CmdSub(raw) => assert_eq!(raw, "echo $(echo x)"),
                other => panic!("expected cmdsub, got {other:?}"),
            }
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn unterminated_quote_errors() {
        assert!(tokenize("echo 'oops").is_err());
        assert!(tokenize(r#"echo "oops"#).is_err());
    }

    #[test]
    fn double_semicolon() {
        let toks = tokenize("a ;; b").unwrap();
        assert!(toks.iter().any(|t| matches!(t, Tok::Op(Op::DSemi))));
    }

    #[test]
    fn here_doc_body() {
        let toks = tokenize("cat <<EOF\nline one\nline two\nEOF\n").unwrap();
        // Op::DLess followed by a HereDoc token carrying the body.
        let hd = toks.iter().find_map(|t| match t {
            Tok::HereDoc(segs) => Some(segs.clone()),
            _ => None,
        });
        let segs = hd.expect("here-doc token");
        assert_eq!(segs, vec![Seg::Lit("line one\nline two\n".to_string())]);
    }

    #[test]
    fn here_doc_strip_tabs() {
        let toks = tokenize("cat <<-END\n\t\tindented\n\tEND\n").unwrap();
        let segs = toks
            .iter()
            .find_map(|t| match t {
                Tok::HereDoc(segs) => Some(segs.clone()),
                _ => None,
            })
            .expect("here-doc token");
        assert_eq!(segs, vec![Seg::Lit("indented\n".to_string())]);
    }

    #[test]
    fn here_string_op() {
        let toks = tokenize("cmd <<< word").unwrap();
        assert!(toks.iter().any(|t| matches!(t, Tok::Op(Op::TLess))));
    }
}
