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

/// bash's end-of-input diagnostic for an unclosed quote, substitution, or group.
/// bash names the delimiter it was scanning for, e.g. `unexpected EOF while
/// looking for matching `)'` — a single backtick, the closing char, then a
/// single quote — so a `$(`/`(` reports `)`, `${` reports `}`, `"` reports `"`.
fn eof_matching(close: char) -> LexError {
    LexError(format!("unexpected EOF while looking for matching `{close}'"))
}

/// Shell operators recognised outside of words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Pipe,
    /// `|&` — pipe both stdout and stderr (shorthand for `2>&1 |`).
    PipeAmp,
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
    /// `&>` — redirect both stdout and stderr (truncate/create).
    AmpGreat,
    /// `&>>` — redirect both stdout and stderr (append).
    AmpDGreat,
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
    /// `<( … )` / `>( … )` process substitution — the `bool` is `true` for the
    /// input form `<(…)`, and the `String` is the raw inner command source.
    ProcSub(bool, String),
}

/// One token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Word(Vec<Seg>),
    /// An IO number: digits immediately preceding a redirection operator.
    Io(i32),
    /// A varfd prefix `{name}` immediately preceding a redirection operator
    /// (`{fd}>file`, `{fd}<file`, `exec {fd}>&-`). bash allocates a free fd
    /// ≥ 10 for the redirect and assigns its number to the shell variable
    /// `name`; a closing `{fd}>&-` reads the fd number back from `name`.
    VarFd(String),
    Op(Op),
    Newline,
    /// A here-document body, captured after its introducing line. Emitted
    /// immediately after the `<<`/`<<-` operator token that owns it. The `u32`
    /// is the number of physical source lines the body (including the closing
    /// delimiter line) consumed, so the parser can keep its line counter in
    /// sync for later diagnostics — those body lines produce no `Newline`
    /// tokens of their own.
    HereDoc(Vec<Seg>, u32),
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

/// Lex `src` as a single word, preserving all literal characters verbatim
/// (whitespace and shell operator characters stay literal) while still
/// processing quotes and `$…`/backtick expansions. Used for the pattern and
/// replacement of `${var/pat/repl}`, where bash neither word-splits nor
/// operator-tokenizes the text — so `${s/ /_}` matches a literal space and
/// `${s/#/hello }` keeps the trailing space in the replacement.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn lex_word_verbatim(src: &str) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer {
        chars: src.chars().collect(),
        pos: 0,
        pending_heredocs: Vec::new(),
        cond_depth: 0,
        regex_next: false,
    };
    lx.read_word_verbatim()
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
                | Op::PipeAmp
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

/// True when `s` is a syntactically valid shell variable name (an identifier).
fn is_valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_name_start(c) => chars.all(is_name_char),
        _ => false,
    }
}

/// True when the previous token leaves us in a position where a leading
/// assignment word (`name=…`, `name[sub]=…`, `name+=…`) is acceptable. bash's
/// tokenizer only slurps an unquoted-space array subscript (`h[a b]=v`) here.
/// This holds at the start of a command *and* immediately after another
/// assignment word (so `h[a b]=1 h[c d]=2` chains).
fn assignment_acceptable(prev: Option<&Tok>) -> bool {
    if starts_command(prev) {
        return true;
    }
    matches!(prev, Some(Tok::Word(segs)) if word_is_assignment(segs))
}

/// Heuristic: does this word token have the shape of an assignment
/// (`name=`, `name[subscript]=`, or `name+=`)? Only the first literal segment
/// is inspected; a subscript containing an expansion (`h[$i]=…`) is not chained
/// past, which is an acceptable limitation for the rare "chained assignments
/// with an expanded subscript" case.
fn word_is_assignment(segs: &[Seg]) -> bool {
    let Some(Seg::Lit(s)) = segs.first() else {
        return false;
    };
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    if b.first().is_none_or(|&c| !is_name_start(c)) {
        return false;
    }
    while i < b.len() && is_name_char(b[i]) {
        i += 1;
    }
    // Optional `[subscript]` with balanced brackets inside this literal.
    if b.get(i) == Some(&'[') {
        let mut depth = 0usize;
        while i < b.len() {
            match b[i] {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }
    if b.get(i) == Some(&'+') {
        i += 1;
    }
    b.get(i) == Some(&'=')
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

    /// If the cursor sits on a varfd redirect prefix `{name}` immediately
    /// followed by a redirection operator (`{fd}>`, `{fd}<`), return the name
    /// and the index just past the closing `}`. Returns `None` otherwise, so a
    /// brace group (`{ …; }`) or brace expansion (`{a,b}`) falls through to the
    /// normal word/reserved-word path. The `{` at `self.pos` is assumed.
    fn varfd_prefix(&self) -> Option<(String, usize)> {
        debug_assert_eq!(self.chars.get(self.pos), Some(&'{'));
        let mut i = self.pos + 1;
        // First name char must be a name-start (letter or `_`).
        match self.chars.get(i) {
            Some(&c) if is_name_start(c) => i += 1,
            _ => return None,
        }
        while matches!(self.chars.get(i), Some(&c) if is_name_char(c)) {
            i += 1;
        }
        if self.chars.get(i) != Some(&'}') {
            return None;
        }
        let close = i;
        i += 1;
        // The `}` must be immediately followed by a redirection operator for
        // this to be a varfd prefix rather than an ordinary `{word}` token.
        if !matches!(self.chars.get(i), Some('<' | '>')) {
            return None;
        }
        let name: String = self.chars[self.pos + 1..close].iter().collect();
        Some((name, close + 1))
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
                    } else if self.peek() == Some('&') {
                        // `|&` — pipe both stdout and stderr (bash: `2>&1 |`).
                        self.pos += 1;
                        out.push(Tok::Op(Op::PipeAmp));
                    } else {
                        out.push(Tok::Op(Op::Pipe));
                    }
                }
                '&' => {
                    self.pos += 1;
                    if self.peek() == Some('&') {
                        self.pos += 1;
                        out.push(Tok::Op(Op::AndIf));
                    } else if self.peek() == Some('>') {
                        // `&>file` / `&>>file`: redirect both stdout and stderr.
                        self.pos += 1;
                        if self.peek() == Some('>') {
                            self.pos += 1;
                            out.push(Tok::Op(Op::AmpDGreat));
                        } else {
                            out.push(Tok::Op(Op::AmpGreat));
                        }
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
                '<' | '>' if self.peek_at(1) == Some('(') => {
                    // Process substitution `<(cmd)` / `>(cmd)`: a word (filename),
                    // not a redirection operator. `read_word` consumes the whole
                    // `<(…)`/`>(…)` group as a `Seg::ProcSub` (and allows adjacent
                    // literals to concatenate).
                    let segs = self.read_word()?;
                    self.emit_word(&mut out, segs);
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
                '{' if self.varfd_prefix().is_some() => {
                    // `{name}>file` / `{name}<file`: a varfd redirect prefix. The
                    // guard confirmed `{` + a valid name + `}` is immediately
                    // followed by a redirection operator (no spaces), which never
                    // collides with a brace group (`{ …; }` has a space) or brace
                    // expansion (`{a,b}` is not followed by `<`/`>`).
                    if let Some((name, end)) = self.varfd_prefix() {
                        self.pos = end;
                        out.push(Tok::VarFd(name));
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
                        let assign_ok = assignment_acceptable(out.last());
                        let segs = self.read_word_inner(assign_ok, false)?;
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
                    return Err(eof_matching(')'));
                }
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                }
                _ => {
                    let segs = self.read_array_elem_word()?;
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
                    if let Some(seg) = self.read_dollar(false)? {
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

    /// Read the entire remaining input as a single word, preserving *all*
    /// literal characters verbatim — including whitespace and shell operator
    /// characters — while still processing quotes and `$…`/backtick expansions.
    /// Used for the pattern and replacement of `${var/pat/repl}`, where bash
    /// applies expansion and quote removal but neither word-splitting nor
    /// operator tokenization, so embedded/leading/trailing spaces are literal.
    fn read_word_verbatim(&mut self) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        while let Some(c) = self.peek() {
            match c {
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
                    if let Some(seg) = self.read_dollar(false)? {
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
        self.read_word_inner(false, false)
    }

    /// Read one array-literal element word. Like [`Self::read_word`] but a
    /// *leading* `[subscript]=value` element (`([ x ]=v)`) is kept as one word
    /// even across unquoted whitespace inside the brackets, matching bash's
    /// array-literal tokenization (`declare -A m=([ x ]=v)` keys on ` x `).
    fn read_array_elem_word(&mut self) -> Result<Vec<Seg>, LexError> {
        self.read_word_inner(false, true)
    }

    /// Read one word; when `assign_ok`, an array-subscript at the head of the
    /// word (`name[…]`) is consumed as part of the word even across unquoted
    /// whitespace, matching bash's assignment-word tokenization. When
    /// `array_elem`, a word that *begins* with `[` slurps its `[…]` subscript the
    /// same way (for array-literal keyed elements, which have no name prefix).
    fn read_word_inner(&mut self, assign_ok: bool, array_elem: bool) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        // Bracket-nesting depth while consuming a leading `name[subscript]`
        // subscript. While > 0, unquoted whitespace and operator characters are
        // literal content; only balanced `]` closes it. Quotes/expansions inside
        // are still processed normally.
        let mut sub_depth = 0usize;
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
            // Array-subscript assignment head: when this word begins with a valid
            // name immediately followed by `[`, bash consumes the whole `[…]`
            // subscript — including unquoted spaces — as part of the word, so
            // `h[a b]=v` stays one assignment word. Only in assignment-acceptable
            // position (`assign_ok`), and only for the leading subscript (segs
            // still empty, `lit` a valid identifier).
            if sub_depth == 0
                && c == '['
                && segs.is_empty()
                && ((assign_ok && is_valid_name(&lit)) || (array_elem && lit.is_empty()))
            {
                lit.push('[');
                self.pos += 1;
                sub_depth += 1;
                continue;
            }
            if sub_depth > 0 {
                match c {
                    '[' => {
                        lit.push('[');
                        sub_depth += 1;
                        self.pos += 1;
                        continue;
                    }
                    ']' => {
                        lit.push(']');
                        sub_depth -= 1;
                        self.pos += 1;
                        continue;
                    }
                    // Quotes, expansion and escapes keep their normal processing
                    // (fall through to the outer match); everything else — spaces,
                    // operators — is literal subscript content.
                    '\'' | '"' | '`' | '\\' | '$' => {}
                    other => {
                        lit.push(other);
                        self.pos += 1;
                        continue;
                    }
                }
            }
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
            // Process substitution `<(cmd)` / `>(cmd)` (outside an extglob group):
            // read the balanced `(…)` body as one segment. Handled before the
            // `<`/`>` word-break below so `diff <(a) <(b)` and concatenated forms
            // like `pre<(cmd)` both work.
            if ext_depth == 0 && matches!(c, '<' | '>') && self.peek_at(1) == Some('(') {
                let input = c == '<';
                self.pos += 2; // consume `<`/`>` and `(`
                flush_lit(&mut segs, &mut lit);
                let raw = self.read_balanced('(', ')')?;
                segs.push(Seg::ProcSub(input, raw));
                continue;
            }
            match c {
                // `#` is NOT a terminator here: a comment only begins when `#`
                // is at the *start* of a word, which the main token loop catches
                // before `read_word` is ever entered. Mid-word (`abc#def`,
                // `n=16#ff`) the `#` is a literal character, matching bash/POSIX.
                ' ' | '\t' | '\n' | '\r' | '|' | '&' | ';' | '(' | ')' | '<' | '>' => break,
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
                    if let Some(seg) = self.read_dollar(false)? {
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
                None => return Err(eof_matching('\'')),
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
                return Err(eof_matching('\''));
            };
            if c == '\'' {
                return Ok(s);
            }
            if c != '\\' {
                s.push(c);
                continue;
            }
            let Some(e) = self.bump() else {
                return Err(eof_matching('\''));
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
                return Err(eof_matching('"'));
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
                    if let Some(seg) = self.read_dollar(true)? {
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
    ///
    /// `in_dquote` is set when the `$` is being read from *inside* a
    /// double-quoted string. In that context `$'…'`/`$"…"` are NOT the
    /// ANSI-C-quote / locale-translation forms — the `$` is a literal
    /// character and the following quote is handled by the enclosing
    /// double-quote scanner (bash: `"a$'b'"` is the 6 literal chars `a$'b'`,
    /// and a `$` right before the closing `"` is a literal `$`).
    fn read_dollar(&mut self, in_dquote: bool) -> Result<Option<Seg>, LexError> {
        // Consume the `$`.
        self.pos += 1;
        match self.peek() {
            Some('\'') if !in_dquote => {
                // `$'…'` — ANSI-C quoting: a literal string with backslash
                // escapes processed (no expansion/splitting — like `'…'`).
                self.pos += 1;
                let s = self.read_ansi_c_quote()?;
                Ok(Some(Seg::Sq(s)))
            }
            Some('"') if !in_dquote => {
                // `$"…"` — locale translation. We have no message catalogs, so
                // it behaves as a plain double-quoted string (bash's fallback).
                self.pos += 1;
                let inner = self.read_double_quote()?;
                Ok(Some(Seg::Dq(inner)))
            }
            Some('{') => {
                self.pos += 1;
                let raw = self.read_dollar_brace()?;
                Ok(Some(Seg::ParamBraced(raw)))
            }
            Some('[') => {
                // `$[ … ]` — the deprecated (pre-`$(( ))`) arithmetic expansion.
                // bash still accepts it as an alias for `$(( … ))`.
                self.pos += 1;
                let raw = self.read_balanced('[', ']')?;
                Ok(Some(Seg::Arith(raw)))
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
                return Err(eof_matching(close));
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
                        None => return Err(eof_matching('\'')),
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
                        None => return Err(eof_matching('"')),
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

    /// Read the body of a `${ … }` parameter expansion (`self.pos` is just
    /// past the opening `{`), returning the raw inner text without the closing
    /// `}`.
    ///
    /// This mirrors bash's `${…}` scanner (`parse_matched_pair` with the
    /// `P_DOLBRACE` flag), which differs from a naive brace-balancer in one
    /// important way: a **bare** `{` does NOT open a new nesting level, so the
    /// expansion closes at the first unquoted, unescaped `}` that is not part
    /// of a nested `$…` construct. That is why `${x//[{}]/_}` closes at the `}`
    /// inside `[{}]` (bash: pattern `[{`), and why `${x/\}/X}` treats the
    /// backslash-escaped `}` as a literal rather than a terminator. Only
    /// `${`, `$(`, `$((`, and backtick command substitutions start nested
    /// spans that must balance with their own terminators; single/double
    /// quotes protect their contents; a backslash escapes the next character.
    fn read_dollar_brace(&mut self) -> Result<String, LexError> {
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(eof_matching('}'));
            };
            match c {
                // First unescaped, unquoted, non-nested `}` closes the span.
                '}' => return Ok(raw),
                // Backslash escapes the next character (both are preserved
                // verbatim so later re-parsing sees the escape).
                '\\' => {
                    raw.push('\\');
                    if let Some(n) = self.bump() {
                        raw.push(n);
                    }
                }
                // Single quotes: copy verbatim to the closing quote.
                '\'' => {
                    raw.push('\'');
                    loop {
                        match self.bump() {
                            Some('\'') => {
                                raw.push('\'');
                                break;
                            }
                            Some(q) => raw.push(q),
                            None => return Err(eof_matching('\'')),
                        }
                    }
                }
                // Double quotes: copy to the closing quote, honoring `\`.
                '"' => {
                    raw.push('"');
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
                            None => return Err(eof_matching('"')),
                        }
                    }
                }
                // Backtick command substitution: copy verbatim to the closing
                // backtick (honoring `\``).
                '`' => {
                    raw.push('`');
                    loop {
                        match self.bump() {
                            Some('\\') => {
                                raw.push('\\');
                                if let Some(n) = self.bump() {
                                    raw.push(n);
                                }
                            }
                            Some('`') => {
                                raw.push('`');
                                break;
                            }
                            Some(q) => raw.push(q),
                            None => return Err(eof_matching('`')),
                        }
                    }
                }
                // `$…` may begin a nested construct that must balance with its
                // own terminator; consume it whole so a `}` or `)` inside it is
                // not mistaken for our terminator.
                '$' => {
                    raw.push('$');
                    match self.peek() {
                        Some('{') => {
                            raw.push('{');
                            self.pos += 1;
                            let inner = self.read_dollar_brace()?;
                            raw.push_str(&inner);
                            raw.push('}');
                        }
                        Some('(') => {
                            raw.push('(');
                            self.pos += 1;
                            if self.peek() == Some('(') {
                                raw.push('(');
                                self.pos += 1;
                                let inner = self.read_arith()?;
                                raw.push_str(&inner);
                                raw.push_str("))");
                            } else {
                                let inner = self.read_balanced('(', ')')?;
                                raw.push_str(&inner);
                                raw.push(')');
                            }
                        }
                        Some('[') => {
                            raw.push('[');
                            self.pos += 1;
                            let inner = self.read_balanced('[', ']')?;
                            raw.push_str(&inner);
                            raw.push(']');
                        }
                        _ => {}
                    }
                }
                _ => raw.push(c),
            }
        }
    }

    /// Read a `$(( … ))` body (up to the closing `))`).
    fn read_arith(&mut self) -> Result<String, LexError> {
        let mut depth = 0usize;
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(eof_matching(')'));
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
        out.push(Tok::HereDoc(Vec::new(), 0));
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
            // Count the physical lines this here-document consumes (body lines
            // plus the closing delimiter line). These lines are swallowed here
            // and never become `Newline` tokens, so the parser must be told how
            // many there were to keep its line counter accurate.
            let mut lines: u32 = 0;
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
                    lines = lines.saturating_add(1);
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
                *slot = Tok::HereDoc(segs, lines);
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
                None => return Err(eof_matching('`')),
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
                if let Some(seg) = lx.read_dollar(true)? {
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
    fn array_subscript_assignment_keeps_spaces() {
        // In assignment position, a `name[…]` subscript is one word even with
        // unquoted spaces inside the brackets (bash's tokenizer behaviour).
        let toks = tokenize("h[a b]=v").unwrap();
        assert_eq!(toks.len(), 1);
        match &toks[0] {
            Tok::Word(segs) => assert_eq!(segs.as_slice(), &[Seg::Lit("h[a b]=v".into())]),
            other => panic!("expected single word, got {other:?}"),
        }
        // Chained assignments: the second word is still assignment position.
        let toks = tokenize("h[a b]=1 h[c d]=2").unwrap();
        assert_eq!(toks.len(), 2, "expected two words, got {toks:?}");
        // In *argument* position the subscript splits normally on the space.
        let toks = tokenize("echo h[a b]=v").unwrap();
        assert_eq!(toks.len(), 3, "argument-position subscript must split: {toks:?}");
    }

    #[test]
    fn array_literal_keyed_element_keeps_spaces() {
        // Inside an array literal, a keyed element `[ x ]=v` stays one element
        // even with unquoted interior spaces (bash tokenises `([ x ]=v)` as a
        // single subscript-value element). Regression for TD-OILS-ASSOC-KEY-TRIM.
        let toks = tokenize("m=([ x ]=v [y z]=w)").unwrap();
        assert_eq!(toks.len(), 1, "expected single ArrayAssign token, got {toks:?}");
        match &toks[0] {
            Tok::ArrayAssign { name, elems, .. } => {
                assert_eq!(name, "m");
                assert_eq!(elems.len(), 2, "expected two elements, got {elems:?}");
                assert_eq!(elems[0].as_slice(), &[Seg::Lit("[ x ]=v".into())]);
                assert_eq!(elems[1].as_slice(), &[Seg::Lit("[y z]=w".into())]);
            }
            other => panic!("expected ArrayAssign, got {other:?}"),
        }
        // A positional element that merely starts with `[` also stays one word.
        let toks = tokenize("a=([a b])").unwrap();
        match &toks[0] {
            Tok::ArrayAssign { elems, .. } => {
                assert_eq!(elems.len(), 1, "positional [a b] must be one element: {elems:?}");
            }
            other => panic!("expected ArrayAssign, got {other:?}"),
        }
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
            Tok::HereDoc(segs, lines) => Some((segs.clone(), *lines)),
            _ => None,
        });
        let (segs, lines) = hd.expect("here-doc token");
        assert_eq!(segs, vec![Seg::Lit("line one\nline two\n".to_string())]);
        // Two body lines + the closing `EOF` line = 3 physical lines consumed;
        // the parser relies on this to keep later line numbers accurate.
        assert_eq!(lines, 3);
    }

    #[test]
    fn here_doc_strip_tabs() {
        let toks = tokenize("cat <<-END\n\t\tindented\n\tEND\n").unwrap();
        let segs = toks
            .iter()
            .find_map(|t| match t {
                Tok::HereDoc(segs, _) => Some(segs.clone()),
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
