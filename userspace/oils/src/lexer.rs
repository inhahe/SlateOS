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
    /// `;&` — terminates a `case` arm and falls through to the next arm's body.
    SemiAmp,
    /// `;;&` — terminates a `case` arm and resumes pattern testing at the next.
    DSemiAmp,
    LParen,
    RParen,
    Less,
    Great,
    DGreat,
    /// `>|` — truncate/create, overriding `noclobber`.
    GreatPipe,
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
    /// Nesting depth of open `[[ … ]]` conditionals. Used to enable regex-word
    /// lexing for the RHS of `=~` (where `(`, `)`, `|`, … are literal regex
    /// metacharacters, not shell operators).
    cond_depth: usize,
    /// Set immediately after emitting a `=~` word inside `[[ … ]]`; the next
    /// word is read in regex mode.
    regex_next: bool,
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
        cond_depth: 0,
        regex_next: false,
    };
    lx.run()
}

/// Reserved words after which a new simple command begins — so a following
/// word is in "command position" and eligible for alias expansion.
const CMD_INTRODUCERS: &[&str] = &[
    "if", "then", "elif", "else", "while", "until", "do", "{", "!",
];

/// True when a word following `prev` (the previous kept token) starts a simple
/// command. Bash only alias-expands the command word of a simple command.
fn starts_command(prev: Option<&Tok>) -> bool {
    match prev {
        None | Some(Tok::Newline) => true,
        Some(Tok::Op(op)) => matches!(
            op,
            Op::Pipe
                | Op::AndIf
                | Op::OrIf
                | Op::Amp
                | Op::Semi
                | Op::DSemi
                | Op::SemiAmp
                | Op::DSemiAmp
                | Op::LParen
        ),
        Some(Tok::Word(segs)) => {
            matches!(segs.as_slice(), [Seg::Lit(w)] if CMD_INTRODUCERS.contains(&w.as_str()))
        }
        _ => false,
    }
}

/// Expand shell aliases over a token stream (bash's pre-parse alias pass).
///
/// Only a single unquoted-literal word in command position is a candidate. The
/// alias value is re-tokenized and spliced in; its first word is itself an
/// expansion candidate (guarded against recursion by `active`, so `alias
/// ls='ls -l'` terminates). If an alias value ends in a blank, the *next* word
/// is also checked (bash's trailing-blank rule, enabling `alias sudo='sudo '`).
#[must_use]
pub fn expand_aliases(toks: &[Tok], aliases: &std::collections::BTreeMap<String, String>) -> Vec<Tok> {
    let mut active = std::collections::BTreeSet::new();
    expand_aliases_inner(toks, aliases, &mut active)
}

fn expand_aliases_inner(
    toks: &[Tok],
    aliases: &std::collections::BTreeMap<String, String>,
    active: &mut std::collections::BTreeSet<String>,
) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    // Whether the *next* token must be treated as command position regardless of
    // structure (carried across an alias whose value ended in a blank).
    let mut force = false;
    for tok in toks {
        let at_cmd = force || starts_command(out.last());
        force = false;
        if at_cmd
            && let Tok::Word(segs) = tok
            && let [Seg::Lit(name)] = segs.as_slice()
            && !active.contains(name)
            && let Some(val) = aliases.get(name)
            && let Ok(mut repl) = tokenize(val)
        {
            // Drop a trailing newline the lexer may append so the splice stays
            // within the current command.
            while matches!(repl.last(), Some(Tok::Newline)) {
                repl.pop();
            }
            active.insert(name.clone());
            let expanded = expand_aliases_inner(&repl, aliases, active);
            active.remove(name);
            out.extend(expanded);
            force = val.ends_with(' ') || val.ends_with('\t');
            continue;
        }
        out.push(tok.clone());
    }
    out
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
            // RHS of `=~`: read the whole regex as one word so that `(`, `)`,
            // `|`, `<`, `>` are literal metacharacters rather than shell
            // operators. Only unquoted whitespace terminates it (bash semantics).
            if self.regex_next {
                self.regex_next = false;
                if !matches!(c, '\n' | '\r') {
                    let segs = self.read_word_regex()?;
                    self.emit_word(&mut out, segs);
                    continue;
                }
            }
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
                        if self.peek() == Some('&') {
                            self.pos += 1;
                            out.push(Tok::Op(Op::DSemiAmp)); // `;;&`
                        } else {
                            out.push(Tok::Op(Op::DSemi)); // `;;`
                        }
                    } else if self.peek() == Some('&') {
                        self.pos += 1;
                        out.push(Tok::Op(Op::SemiAmp)); // `;&`
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
                        Some('|') => {
                            self.pos += 1;
                            out.push(Tok::Op(Op::GreatPipe));
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
                        let segs = self.read_word()?;
                        self.emit_word(&mut out, segs);
                    }
                }
                c if is_name_start(c) => {
                    // A leading identifier may begin an array assignment
                    // `name=( … )` / `name+=( … )`; otherwise it's a plain word.
                    if let Some(tok) = self.try_array_assign()? {
                        out.push(tok);
                    } else {
                        let segs = self.read_word()?;
                        self.emit_word(&mut out, segs);
                    }
                }
                _ => {
                    let segs = self.read_word()?;
                    self.emit_word(&mut out, segs);
                }
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
    /// Push a plain word token, tracking `[[ … ]]` depth and the `=~` regex
    /// trigger so the RHS is lexed in regex mode.
    fn emit_word(&mut self, out: &mut Vec<Tok>, segs: Vec<Seg>) {
        // Detect the bare-literal words `[[`, `]]`, and `=~` to drive the
        // regex-RHS lexing mode. A word is "bare" when it is a single unquoted
        // literal segment.
        if let [Seg::Lit(s)] = segs.as_slice() {
            match s.as_str() {
                "[[" => self.cond_depth = self.cond_depth.saturating_add(1),
                "]]" => self.cond_depth = self.cond_depth.saturating_sub(1),
                "=~" if self.cond_depth > 0 => self.regex_next = true,
                _ => {}
            }
        }
        out.push(Tok::Word(segs));
    }

    /// Read the RHS of `=~` as a single word. Regex metacharacters (`(`, `)`,
    /// `|`, `<`, `>`, `#`, `;`, `&`) are literal; only unquoted whitespace or a
    /// newline terminates the word. Quotes and `$…` expansions still apply
    /// (the RHS undergoes parameter expansion in bash).
    fn read_word_regex(&mut self) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' => break,
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

    fn read_word(&mut self) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        // Depth of nested `extglob` groups. Inside a group the pattern
        // metacharacters `(`, `)`, `|`, whitespace, etc. are literal word content
        // rather than word/operator delimiters, so the whole `@(a|b c)` stays one
        // word token. The extglob decision is deferred to match time (compile_glob
        // only treats these specially when `shopt -s extglob` is set); without
        // extglob the group is matched literally. Parameter expansion and quoting
        // inside the group are still processed normally. NOTE: because parsing is
        // independent of runtime `shopt`, `!(cmd)` written with no space is now a
        // pattern word, not a negated subshell — use `! (cmd)` for the latter.
        let mut ext_depth = 0usize;
        while let Some(c) = self.peek() {
            // Opener: `X(` where X ∈ ?*+@! (unquoted). Begins/nests a group.
            if matches!(c, '?' | '*' | '+' | '@' | '!') && self.peek_at(1) == Some('(') {
                lit.push(c);
                lit.push('(');
                self.pos += 2;
                ext_depth += 1;
                continue;
            }
            if ext_depth > 0 {
                match c {
                    '(' => {
                        lit.push('(');
                        ext_depth += 1;
                        self.pos += 1;
                        continue;
                    }
                    ')' => {
                        lit.push(')');
                        ext_depth -= 1;
                        self.pos += 1;
                        continue;
                    }
                    // Quotes, expansion and escapes still get their normal
                    // processing (fall through to the outer match below).
                    '\'' | '"' | '`' | '\\' | '$' => {}
                    // Everything else — including `|`, whitespace, `<`, `>`, `&`,
                    // `;`, `#` — is literal pattern content inside the group.
                    other => {
                        lit.push(other);
                        self.pos += 1;
                        continue;
                    }
                }
            }
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

    /// Read the body of a `$'…'` ANSI-C-quoted string, processing backslash
    /// escapes. `self.pos` is just past the opening quote; consumes through the
    /// closing quote. The result is a literal string (no expansion/splitting).
    ///
    /// Note: byte escapes (`\xHH`, `\nnn`) naming a value above 0x7F are
    /// materialised as the Unicode code point of that value — the shell stores
    /// words as UTF-8 `String`, not raw bytes, so `$'\xff'` yields U+00FF.
    fn read_ansi_c_quote(&mut self) -> Result<String, LexError> {
        let mut s = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(LexError("unterminated $'…' quote".into()));
            };
            if c == '\'' {
                return Ok(s);
            }
            if c != '\\' {
                s.push(c);
                continue;
            }
            let Some(e) = self.bump() else {
                return Err(LexError("unterminated $'…' quote".into()));
            };
            match e {
                'a' => s.push('\u{07}'),
                'b' => s.push('\u{08}'),
                'e' | 'E' => s.push('\u{1b}'),
                'f' => s.push('\u{0c}'),
                'n' => s.push('\n'),
                'r' => s.push('\r'),
                't' => s.push('\t'),
                'v' => s.push('\u{0b}'),
                '\\' => s.push('\\'),
                '\'' => s.push('\''),
                '"' => s.push('"'),
                '?' => s.push('?'),
                'x' => match self.read_hex_escape(2) {
                    Some(v) => push_code(&mut s, v),
                    None => {
                        s.push('\\');
                        s.push('x');
                    }
                },
                'u' => match self.read_hex_escape(4) {
                    Some(v) => push_code(&mut s, v),
                    None => {
                        s.push('\\');
                        s.push('u');
                    }
                },
                'U' => match self.read_hex_escape(8) {
                    Some(v) => push_code(&mut s, v),
                    None => {
                        s.push('\\');
                        s.push('U');
                    }
                },
                'c' => {
                    // Control character: `\cx` → `x & 0x1f`.
                    if let Some(ctrl) = self.bump() {
                        push_code(&mut s, (ctrl as u32) & 0x1f);
                    } else {
                        s.push('\\');
                        s.push('c');
                    }
                }
                d @ '0'..='7' => {
                    // Octal, 1–3 digits (the first is already consumed).
                    let mut val = d.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        match self.peek().and_then(|n| n.to_digit(8)) {
                            Some(n) => {
                                val = val.wrapping_mul(8).wrapping_add(n);
                                self.pos += 1;
                            }
                            None => break,
                        }
                    }
                    push_code(&mut s, val);
                }
                other => {
                    // Unknown escape: bash keeps the backslash and the char.
                    s.push('\\');
                    s.push(other);
                }
            }
        }
    }

    /// Read up to `max` hex digits at the cursor, returning their value, or
    /// `None` if there was no hex digit (so the caller can keep the escape
    /// literal).
    fn read_hex_escape(&mut self, max: usize) -> Option<u32> {
        let mut val: u32 = 0;
        let mut count = 0;
        while count < max {
            match self.peek().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    val = val.wrapping_mul(16).wrapping_add(d);
                    self.pos += 1;
                    count += 1;
                }
                None => break,
            }
        }
        if count == 0 { None } else { Some(val) }
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
            Some('\'') => {
                // `$'…'` — ANSI-C quoting: a literal string with backslash
                // escapes processed (no expansion/splitting — like `'…'`).
                self.pos += 1;
                let s = self.read_ansi_c_quote()?;
                Ok(Some(Seg::Sq(s)))
            }
            Some('"') => {
                // `$"…"` — locale translation. We have no message catalogs, so
                // it behaves as a plain double-quoted string (bash's fallback).
                self.pos += 1;
                let inner = self.read_double_quote()?;
                Ok(Some(Seg::Dq(inner)))
            }
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

/// Append the character named by a code point (from a `$'…'` numeric escape) to
/// `s`, if it is a valid Unicode scalar value. An invalid code point (e.g. a
/// surrogate from `\uD800`) is dropped, matching bash's leniency.
fn push_code(s: &mut String, code: u32) {
    if let Some(ch) = char::from_u32(code) {
        s.push(ch);
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
        cond_depth: 0,
        regex_next: false,
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
