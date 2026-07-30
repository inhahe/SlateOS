//! Tokenizer for the OSH shell language.
//!
//! The lexer turns source text into a flat token stream. Words are captured as
//! a list of [`Seg`] fragments that preserve quoting; command/parameter/
//! arithmetic substitutions keep their *raw inner source* so the parser can
//! recursively parse them (this keeps the lexer free of a dependency on the
//! parser).

/// A lexer error with a human-readable message (unbalanced quote, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub msg: String,
    /// 1-based source line to report the error on, when the raise site knows a
    /// better one than the line the enclosing token started on.
    ///
    /// bash reports an unterminated construct on the line where **that
    /// construct opened**, not where the word containing it began and not where
    /// the input ran out — verified against bash 5.2, e.g. a `'` opened on line
    /// 3 inside a `$(` opened on line 2 reports line 3. The single exception is
    /// `$( … )` itself, whose own unterminated-`)` error bash reports at the
    /// *end* of input (its body is re-parsed after the outer scan, so the line
    /// counter has already advanced). Each scanner therefore stamps its own
    /// opening line, and stamping only ever *fills* a `None` — so the innermost
    /// construct, which fails first, keeps its line as the error unwinds.
    pub line: Option<u32>,
    /// The delimiter the lexer was still looking for, for the "unexpected EOF"
    /// errors that name one — the same character the message quotes back.
    ///
    /// Kept structurally rather than re-parsed out of `msg` because it is not
    /// only a diagnostic: history expansion has to know which quote the reader is
    /// *inside* when it expands a continuation line, and this is where that
    /// answer lives (see [`open_quote`]). An error propagates outward untouched,
    /// so the innermost unclosed construct — the one that failed first — is the
    /// one whose delimiter survives, which is the one wanted.
    pub looking_for: Option<char>,
}

impl LexError {
    /// A lexer error with no line preference; the caller's fallback applies.
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), line: None, looking_for: None }
    }

    /// Fill in the reporting line if the raise site did not already choose one.
    /// Never overwrites: an outer construct must not claim an inner one's line.
    pub(crate) fn at(mut self, line: u32) -> Self {
        self.line.get_or_insert(line);
        self
    }
}

impl core::fmt::Display for LexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

/// bash's end-of-input diagnostic for an unclosed quote, substitution, or group.
/// bash names the delimiter it was scanning for, e.g. `unexpected EOF while
/// looking for matching `)'` — a single backtick, the closing char, then a
/// single quote — so a `$(`/`(` reports `)`, `${` reports `}`, `"` reports `"`.
fn eof_matching(close: char) -> LexError {
    LexError {
        msg: format!("unexpected EOF while looking for matching `{close}'"),
        line: None,
        looking_for: Some(close),
    }
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
    /// `<>` — open the target for both reading and writing (create if absent,
    /// no truncation). Default fd is 0.
    LessGreat,
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
    /// A quoted-literal run: the contents of `'…'`/`$'…'`, or a single
    /// backslash-escaped character, which means exactly the same thing
    /// (`a\*b` ≡ `a'*'b`).
    ///
    /// The `bool` is `true` for the backslash spelling. The two are
    /// interchangeable during expansion, but bash prints a stored function
    /// body back in whichever form the source wrote (`declare -f`), so the
    /// distinction has to survive lexing.
    Sq(String, bool),
    /// Double-quoted run of fragments.
    Dq(Vec<Seg>),
    /// `$name` / `$1` / `$?` … a bare parameter reference.
    Param(String),
    /// `${ … }` — raw inner text, parsed later.
    ParamBraced(String),
    /// `$( … )` / `` ` … ` `` — raw inner source, parsed later, plus the 1-based
    /// source line of the substitution's *closing* delimiter.
    ///
    /// bash re-parses a substitution body only after the enclosing command has
    /// been scanned, so `$LINENO` inside the body counts up from the line the
    /// scan had reached — the closing `)` — rather than from the body's own
    /// first line. The parser needs that line to rebase the body's numbering
    /// (see `parser::parse_cmdsub_body`), and only the lexer knows it.
    ///
    /// The third field carries a `` ` … ` `` body's *verbatim source*, and is
    /// `None` for the `$( … )` spelling. The two forms run the same command,
    /// but bash treats them as different constructs everywhere else: it prints
    /// a `$( … )` body back from the parse and a backtick body from the source
    /// text (see [`Lexer::read_backtick`]), and it parses a `$( … )` body in
    /// the enclosing token stream, which changes how an error in it is
    /// reported.
    CmdSub(String, u32, Option<String>),
    /// `$(( … ))` — raw arithmetic expression text.
    /// The `bool` is `true` when the deprecated `$[ … ]` spelling was used. The
    /// two evaluate identically, but bash prints a stored function body back in
    /// whichever form the source wrote, so the distinction must survive here.
    Arith(String, bool),
    /// `<( … )` / `>( … )` process substitution — the `bool` is `true` for the
    /// input form `<(…)`, the `String` is the raw inner command source, and the
    /// `u32` is the 1-based source line the `<(`/`>(` opens on.
    ///
    /// bash blames a syntax error in the body on the body's own line, counted in
    /// the enclosing source; the body is lexed on its own, so the parser needs
    /// the opening line to shift it back (`parser::parse_procsub_body`). Unlike
    /// a `$( … )` body there is no rank-based renumbering — this really is a
    /// plain offset.
    ProcSub(bool, String, u32),
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
    /// immediately after the `<<`/`<<-` operator token that owns it. The body's
    /// swallowed source lines are accounted for by the lexer's per-token line
    /// stamping (see [`Lexer::stamp_lines`]), so no line count is carried here.
    ///
    /// The delimiter word (quoting removed) and whether it was quoted travel
    /// with the body: the parser cannot recover them — the operator token only
    /// records `<<` vs `<<-` — and printing a stored function back out needs a
    /// delimiter to name.
    HereDoc(Vec<Seg>, String, bool),
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

/// Shell options that change how source text is *lexed*, so they must be known
/// before a unit is tokenized rather than when it runs.
///
/// bash reads, parses and executes one unit at a time, so a `shopt` run by unit
/// N is in force for the lexing of unit N+1 — and only from there. The default
/// is bash's own for a non-interactive shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LexOpts {
    /// `shopt -s extglob`: read `?(`, `*(`, `+(`, `@(` and `!(` as the opener of
    /// an extended-pattern group, swallowing the balanced `( … )` into the word.
    /// With it off those are ordinary characters and the `(` is a
    /// metacharacter — which is why `!(cmd)` is a *negated subshell* by default,
    /// and why `echo @(a)` is a syntax error.
    pub extglob: bool,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    /// 1-based source line the cursor currently sits on. Advanced by counting
    /// the newlines each token consumes (including those swallowed *inside* a
    /// token — quoted strings, here-doc bodies, command substitutions — which
    /// produce no `Newline` token of their own) so every token can be stamped
    /// with its true starting line for accurate diagnostics and `$LINENO`.
    line: u32,
    /// Character index at which the current `run` iteration began. `line` is only
    /// advanced once per iteration (in [`Lexer::stamp_lines`]), so mid-token it
    /// still names the line the *token* started on; the newlines consumed since
    /// `iter_start` are what [`Lexer::cur_line`] adds to get the cursor's true
    /// line. Left at 0 by the standalone here-doc lexer, whose lines are
    /// body-relative anyway.
    iter_start: usize,
    /// Here-documents whose bodies are pending collection at the next newline.
    pending_heredocs: Vec<PendingHeredoc>,
    /// Nesting depth of open `[[ … ]]` conditionals. Used to enable regex-word
    /// lexing for the RHS of `=~` (where `(`, `)`, `|`, … are literal regex
    /// metacharacters, not shell operators).
    cond_depth: usize,
    /// Set immediately after emitting a `=~` word inside `[[ … ]]`; the next
    /// word is read in regex mode.
    regex_next: bool,
    /// Options that change lexing (currently just `extglob`).
    opts: LexOpts,
    /// Set immediately after emitting a `==`, `!=` or `=` word inside `[[ … ]]`;
    /// the next word is that operator's *pattern* operand. bash enables extglob
    /// for exactly that word regardless of the `extglob` option, so
    /// `[[ abc == @(abc|x) ]]` works in a default shell while `[[ @(a) == b ]]`
    /// and `[[ -n @(a) ]]` are both syntax errors.
    extpat_next: bool,
    /// When `true`, a here-document whose delimiter is never reached before the
    /// input ends is reported as a [`LexError`] (an "unexpected EOF" incomplete
    /// signal) instead of being lenient-accepted with the partial body. The
    /// interactive REPL uses this mode (via [`tokenize_spanned_strict`]) so a
    /// `cat <<EOF` typed with no body yet keeps prompting for the body rather
    /// than executing an empty here-doc. The normal (lenient) mode matches bash's
    /// script/`-c` behaviour of accepting a here-doc cut off by real EOF.
    strict_heredoc_eof: bool,
    /// When `true`, the source is the body of a `$( … )` or `<( … )`
    /// substitution, so the token that follows it is that construct's own `)`
    /// rather than the implicit newline every other input ends with. See
    /// [`tokenize_paren_body`].
    paren_body: bool,
    /// Character offsets of every `\` whose `\<newline>` this lexer *deleted* as
    /// a line continuation. Only the reader — [`crate::parser::IncrementalParser`],
    /// slicing a parse unit's source for the command history — has any use for
    /// these: bash's history stores the joined line, without the backslash or
    /// the newline, exactly where its own reader dropped them. A lexer run over
    /// a sub-string (a `${…}` replacement, a here-doc body scan) fills this in
    /// too, but its offsets are relative to that string and are simply dropped
    /// with the lexer.
    conts: Vec<u32>,
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

impl Lexer {
    fn new(src: &str, opts: LexOpts) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            iter_start: 0,
            pending_heredocs: Vec::new(),
            cond_depth: 0,
            regex_next: false,
            opts,
            extpat_next: false,
            strict_heredoc_eof: false,
            paren_body: false,
            conts: Vec::new(),
        }
    }

    /// As [`Lexer::new`], but an unterminated here-document is an error rather
    /// than leniently accepted. See [`tokenize_spanned_strict`].
    fn strict_heredoc(src: &str, opts: LexOpts) -> Self {
        Self { strict_heredoc_eof: true, ..Self::new(src, opts) }
    }

    /// As [`Lexer::new`], but the stream is closed with the `)` that ends the
    /// enclosing substitution rather than with an implicit newline. See
    /// [`tokenize_paren_body`].
    fn paren_body(src: &str, opts: LexOpts) -> Self {
        Self { paren_body: true, ..Self::new(src, opts) }
    }
}

/// Tokenize `src` into a token stream.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize(src: &str, opts: LexOpts) -> Result<Vec<Tok>, LexError> {
    tokenize_spanned(src, opts).map(|(toks, _lines)| toks)
}

/// Tokenize `src`, returning the token stream alongside a parallel vector giving
/// the 1-based source line each token starts on. The parser stamps these lines
/// onto items for `$LINENO` and error diagnostics; unlike counting `Newline`
/// tokens, this stays correct across newlines swallowed inside quoted strings,
/// here-document bodies, and command substitutions.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize_spanned(src: &str, opts: LexOpts) -> Result<(Vec<Tok>, Vec<u32>), LexError> {
    let mut lx = Lexer::new(src, opts);
    lx.run()
}

/// Tokenize the body of a `$( … )` or `<( … )` substitution, closing the stream
/// with that construct's own `)` instead of the implicit trailing newline
/// [`tokenize_spanned`] appends.
///
/// bash scans such a body in the enclosing token stream, so the token after the
/// body's last command is that `)`. It ends the body's list — `$(echo x)` and
/// `$(echo x; )` are both fine — but it is not a `list_terminator`, so the
/// productions that let `!` and `time` stand alone (`BANG list_terminator`,
/// `timespec list_terminator`) have nothing to match: `$( ! )`, `$( ! ! )`,
/// `$(time)` and `$( echo x; ! )` are all syntax errors named on the `)`, even
/// though `!` alone is a valid command anywhere a real line end can follow it.
/// The same `)` is what bash names for a body that ends mid-construct
/// (`$(for)`, `<(for)`), where a stream ended by a newline would name `newline`.
///
/// The backtick spelling is *not* read this way: bash lexes that body on its
/// own, and accepts `` `!` ``.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize_paren_body(src: &str, opts: LexOpts) -> Result<(Vec<Tok>, Vec<u32>), LexError> {
    let mut lx = Lexer::paren_body(src, opts);
    lx.run()
}

/// A whole-source tokenization that keeps the tokens it managed to lex even
/// when the input ended inside an unclosed construct.
pub struct Tokenized {
    pub toks: Vec<Tok>,
    /// Parallel to `toks`: the 1-based source line each token starts on.
    pub lines: Vec<u32>,
    /// Parallel to `toks`: the character offset into `src` at which each token
    /// begins. [`crate::parser::IncrementalParser`] re-lexes the unconsumed
    /// remainder from here when a `shopt` changes how it must be read.
    pub offsets: Vec<u32>,
    /// Parallel to `toks`: the character offset into `src` just past each
    /// token's last character. For a `Newline` that swallowed a here-document
    /// body this sits past the body, so the span `offsets[i]..ends[i]` always
    /// covers every character the token consumed — which is what
    /// [`crate::parser::IncrementalParser`] slices a parse unit's source out of
    /// for the command history.
    pub ends: Vec<u32>,
    /// Character offsets into `src` of every `\` whose `\<newline>` the lexer
    /// *deleted* as a line continuation, ascending. The command history stores
    /// what bash stores — the joined line — so
    /// [`crate::parser::IncrementalParser`] cuts each of these two-character
    /// (three, for CRLF) spans back out of the text it slices. A continuation
    /// inside `'…'`, `"…"`, `$( … )` or a quoted-delimiter here-document is
    /// *not* deleted and so is not listed here, which is exactly the
    /// distinction bash's own history draws.
    pub conts: Vec<u32>,
    /// `Some((error, line))` when lexing stopped early. `toks` is then cut back
    /// to the last **complete** logical line, because that is the granularity
    /// at which bash stops executing: in `echo two; echo three 'unterm`
    /// nothing on that line runs, but every earlier line already has.
    pub err: Option<(LexError, u32)>,
}

/// Tokenize `src`, deferring an unterminated-construct error instead of
/// discarding the tokens before it.
///
/// bash reads, parses, and executes a script one line at a time, so the
/// commands preceding an unclosed quote have *already run* by the time the
/// quote is reported — `echo hi` on line 1 prints before a line-3 `v='abc`
/// error. Returning only `Err` (as [`tokenize_spanned`] does) silently swallows
/// them. [`crate::parser::IncrementalParser`] uses this and surfaces the error
/// once the good prefix is exhausted.
#[must_use]
pub fn tokenize_deferred(src: &str, opts: LexOpts) -> Tokenized {
    let mut lx = Lexer::new(src, opts);
    let mut toks = Vec::new();
    let mut lines = Vec::new();
    let mut offsets = Vec::new();
    let mut ends = Vec::new();
    let res = lx.run_into(&mut toks, &mut lines, &mut offsets, &mut ends);
    // A here-document body is scanned out of order with respect to the line that
    // introduced it, so sort rather than assume the scan produced them in source
    // order; callers binary-search this list.
    let mut conts = std::mem::take(&mut lx.conts);
    conts.sort_unstable();
    conts.dedup();
    let Err(e) = res else {
        return Tokenized { toks, lines, offsets, ends, conts, err: None };
    };
    // The failing token's own line is the fallback when the raise site did not
    // name one. `Lexer::line` only advances at the end of each `run_into`
    // iteration, so mid-token it still holds the line that token started on.
    let line = e.line.unwrap_or(lx.line);
    // Cut back to the last complete logical line. A here-document still awaiting
    // its body owns its introducing line, so cut before that line too rather
    // than leaving a `<<` whose placeholder token was never filled in.
    let limit = lx
        .pending_heredocs
        .iter()
        .map(|h| h.tok_index)
        .min()
        .unwrap_or(toks.len());
    let keep = toks
        .get(..limit)
        .unwrap_or(&toks)
        .iter()
        .rposition(|t| matches!(t, Tok::Newline))
        .map_or(0, |i| i.saturating_add(1));
    toks.truncate(keep);
    lines.truncate(keep);
    offsets.truncate(keep);
    ends.truncate(keep);
    // The continuations are keyed by source offset rather than by token index, so
    // the ones past the cut simply describe text no caller will slice; leaving
    // them costs nothing and keeps the list a faithful record of the whole scan.
    Tokenized { toks, lines, offsets, ends, conts, err: Some((e, line)) }
}

/// Which quote, if any, the reader is still *inside* after `src` — `Some('\'')`
/// or `Some('"')` when the input ended within an unclosed single- or
/// double-quoted span, `None` otherwise.
///
/// This exists for history expansion. readline expands each physical line of a
/// multi-line command with `history_quoting_state` set from the reader's
/// delimiter stack, so a `!` on a continuation line is expanded knowing it sits
/// inside a quote opened on an earlier line: bash's `!` is inert in `'…'` and
/// live (but with `\!` left alone) in `"…"`, and that stays true across the
/// newline. Resetting the state per line — which is what osh did before this
/// existed — makes `echo 'a` / `!!'` expand a `!!` bash leaves literal.
///
/// The delimiter is read out of the deferred lexer error, which is the only
/// place that knows it, and it is filtered to the two quote characters because
/// those are the only states readline tracks: an unclosed `$(`, `${`, `(` or
/// **backtick** yields `None`. That is not a simplification but the measured
/// behaviour — a quote opened inside `$( … )` *is* carried (bash recurses into
/// the parser there, pushing the inner quote) while one opened inside a
/// backtick body is *not* (that body is scanned as a flat matched pair), and
/// reporting only the innermost unclosed delimiter reproduces both, since the
/// backtick scan never descends into the quote to begin with.
#[must_use]
pub fn open_quote(src: &str, opts: LexOpts) -> Option<char> {
    let (err, _line) = tokenize_deferred(src, opts).err?;
    match err.looking_for {
        Some(q @ ('\'' | '"')) => Some(q),
        _ => None,
    }
}

/// Whether `src` ends in a `\<newline>` that the lexer **deleted** as a line
/// continuation, so a line-at-a-time reader must take another line even though
/// what remains after the deletion may well parse as a complete command.
///
/// This is the other half of the reader state history expansion needs (see
/// [`open_quote`] for the quote half). `echo x \` joined to nothing is just
/// `echo x`, a runnable command — so a caller that decides "do I need another
/// line?" by parsing will stop there and never offer the continuation line for
/// expansion, which is how `echo x \` / `!!` came to run its `!!` literally
/// where bash expands it.
///
/// The test is deliberately not textual. A trailing `\` is *not* always a
/// continuation: inside `'…'` or a quoted-delimiter here-document body the
/// lexer keeps it, and at end of input with no newline after it there is
/// nothing to continue onto. So this asks the lexer what it actually deleted —
/// `conts` records the offset of every such backslash — rather than
/// re-deriving the rule and risking the two drifting apart.
#[must_use]
pub fn ends_in_continuation(src: &str, opts: LexOpts) -> bool {
    let chars: Vec<char> = src.chars().collect();
    // The `\` must be the last character before the final newline.
    let Some(nl) = chars.len().checked_sub(1) else {
        return false;
    };
    if chars.get(nl) != Some(&'\n') {
        return false;
    }
    // Immediately before it — a `\<CR><LF>` is *not* a continuation, because the
    // `\` escapes the CR and the newline then ends the line. Verified against
    // bash: a CRLF script's `echo x \` prints `x \r` and does not join.
    let Some(bs) = nl.checked_sub(1) else {
        return false;
    };
    if chars.get(bs) != Some(&'\\') {
        return false;
    }
    let Ok(bs) = u32::try_from(bs) else {
        return false;
    };
    tokenize_deferred(src, opts).conts.binary_search(&bs).is_ok()
}

/// Like [`tokenize_spanned`] but reports an unterminated here-document (its
/// delimiter never reached before the input ends) as a [`LexError`] instead of
/// leniently accepting the partial body. Used only by the interactive REPL's
/// incompleteness check ([`crate::Shell::parse_incomplete`]) so `cat <<EOF`
/// with no body yet keeps reading the here-doc body across continuation lines.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote, substitution, or here-document.
pub fn tokenize_spanned_strict(src: &str, opts: LexOpts) -> Result<(Vec<Tok>, Vec<u32>), LexError> {
    let mut lx = Lexer::strict_heredoc(src, opts);
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
    let mut lx = Lexer::new(src, LexOpts::default());
    lx.read_word_verbatim(false)
}

/// Lex the *replacement* of `${var/pat/repl}` verbatim, like
/// [`lex_word_verbatim`] but preserving a literal backslash before `&` or `\`
/// (`\&` and `\\`) so the replacement's `&`-scan can later distinguish an
/// escaped ampersand (a literal `&`) from an active one (the matched text).
/// Every other backslash escape is still consumed at lex time (`\n` → `n`),
/// matching bash's replacement quote-removal.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn lex_replacement_verbatim(src: &str) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, LexOpts::default());
    lx.read_word_verbatim(true)
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
pub fn expand_aliases(
    toks: &[Tok],
    lines: &[u32],
    aliases: &std::collections::BTreeMap<String, String>,
    opts: LexOpts,
) -> (Vec<Tok>, Vec<u32>) {
    let (out, out_lines, _) = expand_aliases_tracked(toks, lines, aliases, opts);
    (out, out_lines)
}

/// [`expand_aliases`] plus a parallel *origin* vector: for each output token,
/// the index of the input token it came from, or `None` when the token was
/// spliced in by an alias's replacement text.
///
/// [`crate::parser::IncrementalParser`] needs this to resume: after executing
/// one item it must know which *original* token to continue from, and must not
/// re-expand tokens an alias already produced.
#[must_use]
pub fn expand_aliases_tracked(
    toks: &[Tok],
    lines: &[u32],
    aliases: &std::collections::BTreeMap<String, String>,
    opts: LexOpts,
) -> (Vec<Tok>, Vec<u32>, Vec<Option<usize>>) {
    let mut active = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let mut out_lines = Vec::new();
    let mut out_origin = Vec::new();
    expand_aliases_inner(
        toks,
        lines,
        aliases,
        opts,
        &mut active,
        &mut out,
        &mut out_lines,
        &mut out_origin,
        true,
    );
    (out, out_lines, out_origin)
}

// Three parallel output vectors plus the recursion's bookkeeping. Bundling them
// into a struct would only move the same fields behind another indirection, and
// this is a private helper with exactly one caller pair.
#[allow(clippy::too_many_arguments)]
fn expand_aliases_inner(
    toks: &[Tok],
    lines: &[u32],
    aliases: &std::collections::BTreeMap<String, String>,
    opts: LexOpts,
    active: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<Tok>,
    out_lines: &mut Vec<u32>,
    out_origin: &mut Vec<Option<usize>>,
    // False inside an alias's replacement text: those tokens have no counterpart
    // in the caller's stream, so they record origin `None`.
    from_input: bool,
) {
    // Whether the *next* token must be treated as command position regardless of
    // structure (carried across an alias whose value ended in a blank).
    let mut force = false;
    for (i, tok) in toks.iter().enumerate() {
        // The source line of this token; expanded replacement tokens inherit it
        // so post-alias line numbers stay anchored to the alias's call site.
        let tok_line = lines.get(i).copied().unwrap_or(1);
        let at_cmd = force || starts_command(out.last());
        force = false;
        if at_cmd
            && let Tok::Word(segs) = tok
            && let [Seg::Lit(name)] = segs.as_slice()
            && !active.contains(name)
            && let Some(val) = aliases.get(name)
            && let Ok(mut repl) = tokenize(val, opts)
        {
            // Drop a trailing newline the lexer may append so the splice stays
            // within the current command.
            while matches!(repl.last(), Some(Tok::Newline)) {
                repl.pop();
            }
            // Replacement tokens all inherit the alias word's source line.
            let repl_lines = vec![tok_line; repl.len()];
            let mark = out.len();
            active.insert(name.clone());
            expand_aliases_inner(
                &repl,
                &repl_lines,
                aliases,
                opts,
                active,
                out,
                out_lines,
                out_origin,
                false,
            );
            active.remove(name);
            // The *first* token of the replacement stands in for the alias word
            // itself, so it keeps the alias word's origin; only the tokens after
            // it are origin-less. Without this, a caller resuming at the start of
            // the splice would skip the alias word entirely (it would find the
            // next `Some` origin *past* the replacement) and silently drop the
            // command. An empty replacement contributes no token and needs no
            // mark: resuming past it is correct, since it expands to nothing.
            if from_input
                && let Some(slot) = out_origin.get_mut(mark)
            {
                *slot = Some(i);
            }
            force = val.ends_with(' ') || val.ends_with('\t');
            continue;
        }
        out.push(tok.clone());
        out_lines.push(tok_line);
        out_origin.push(if from_input { Some(i) } else { None });
    }
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
pub(crate) fn word_is_assignment(segs: &[Seg]) -> bool {
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

    /// Record the `\<newline>` the caller just consumed and discarded.
    ///
    /// The cursor sits one past the newline, so the backslash is two back. A
    /// backslash at end of input is not a continuation and is not recorded.
    fn note_continuation(&mut self) {
        if self.chars.get(self.pos.wrapping_sub(1)) != Some(&'\n') {
            return;
        }
        let Some(at) = self.pos.checked_sub(2) else {
            return;
        };
        self.conts.push(u32::try_from(at).unwrap_or(u32::MAX));
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

    fn run(&mut self) -> Result<(Vec<Tok>, Vec<u32>), LexError> {
        let mut out = Vec::new();
        // Parallel to `out`: the 1-based source line each token *ends* on (see
        // `stamp_lines` — that is what bash's `line_number` holds once the token
        // has been read).
        let mut lines: Vec<u32> = Vec::new();
        self.run_into(&mut out, &mut lines, &mut Vec::new(), &mut Vec::new())?;
        Ok((out, lines))
    }

    /// Tokenize the whole input into `out`/`lines`, keeping whatever was lexed
    /// before an error. Split out of [`Lexer::run`] so [`tokenize_deferred`] can
    /// hold on to the good prefix: bash executes every complete line preceding
    /// an unterminated construct before reporting it.
    fn run_into(
        &mut self,
        out: &mut Vec<Tok>,
        lines: &mut Vec<u32>,
        offsets: &mut Vec<u32>,
        ends: &mut Vec<u32>,
    ) -> Result<(), LexError> {
        loop {
            // Skip inline blanks (but not newlines — those are tokens).
            while matches!(self.peek(), Some(' ' | '\t')) {
                self.pos += 1;
            }
            let Some(c) = self.peek() else { break };
            // Every token produced by this iteration starts on `start_line`;
            // `start_pos` lets us count the newlines the iteration consumes (so
            // the counter advances past newlines swallowed inside a token body).
            let start_line = self.line;
            let start_pos = self.pos;
            self.iter_start = start_pos;
            // RHS of `=~`: read the regex as one word so that `|` and the rest
            // of the regex alphabet are literal rather than shell operators,
            // and so a `( … )` group holds on to the blanks inside it.
            if self.regex_next && !matches!(c, '\n' | '\r') {
                self.regex_next = false;
                let segs = self.read_word_regex()?;
                self.emit_word(out, segs);
                self.stamp_lines(out, lines, offsets, ends, start_line, start_pos);
                continue;
            }
            self.regex_next = false;
            // The `[[ … == PATTERN ]]` flag set by the operator word is consumed
            // by whatever token comes next, so take it here rather than leaving
            // it to leak past an operator onto a later word.
            let extpat = std::mem::take(&mut self.extpat_next);
            match c {
                '\n' => {
                    self.pos += 1;
                    out.push(Tok::Newline);
                    if !self.pending_heredocs.is_empty() {
                        self.collect_heredocs(out)?;
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
                    let segs = self.read_word(extpat)?;
                    self.emit_word(out, segs);
                }
                '<' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('&') => {
                            self.pos += 1;
                            out.push(Tok::Op(Op::LessAnd));
                        }
                        Some('>') => {
                            // `<>` — open the target for reading and writing.
                            self.pos += 1;
                            out.push(Tok::Op(Op::LessGreat));
                        }
                        Some('<') => {
                            self.pos += 1;
                            if self.peek() == Some('<') {
                                // `<<<` here-string: the target is an ordinary
                                // word parsed on this line.
                                self.pos += 1;
                                out.push(Tok::Op(Op::TLess));
                            } else {
                                self.lex_heredoc_op(out);
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
                        let segs = self.read_word(extpat)?;
                        self.emit_word(out, segs);
                    }
                }
                c if is_name_start(c) => {
                    // A leading identifier may begin an array assignment
                    // `name=( … )` / `name+=( … )`; otherwise it's a plain word.
                    if let Some(tok) = self.try_array_assign()? {
                        out.push(tok);
                    } else {
                        let assign_ok = assignment_acceptable(out.last());
                        let segs = self.read_word_inner(assign_ok, false, extpat)?;
                        self.emit_word(out, segs);
                    }
                }
                _ => {
                    let segs = self.read_word(extpat)?;
                    self.emit_word(out, segs);
                }
            }
            self.stamp_lines(out, lines, offsets, ends, start_line, start_pos);
        }
        // bash's reader hands the parser a newline when the input runs out, so a
        // script with no trailing newline — and every `-c` string, which never
        // has one — parses exactly as if one were there. This is not cosmetic.
        // A newline is a *token*, and the grammar accepts it only in some
        // positions: `case x in y` is a syntax error near `newline', not an end
        // of file, and the difference decides whether the REPL offers to
        // continue the line. Without this the two spellings of the same script
        // diverge, which they must not.
        //
        // A `$( … )` body is the one input that does *not* end that way: bash
        // reads it in the enclosing token stream, so what follows its last
        // command is the substitution's own `)`. That token closes the body's
        // list but is not a `list_terminator`, which is exactly why `$( ! )`
        // and `$(time)` are syntax errors while a bare `!` on a line of its own
        // is a perfectly good (false) command. Closing the stream with the `)`
        // reproduces both halves of that. (A backtick body really is lexed on
        // its own — bash accepts `` `!` `` — so it keeps the newline.)
        if self.paren_body {
            let start_pos = self.pos;
            if !self.pending_heredocs.is_empty() {
                self.collect_heredocs(out)?;
            }
            out.push(Tok::Op(Op::RParen));
            self.stamp_lines(out, lines, offsets, ends, self.line, start_pos);
        } else if !matches!(out.last(), None | Some(Tok::Newline)) {
            let start_pos = self.pos;
            out.push(Tok::Newline);
            if !self.pending_heredocs.is_empty() {
                self.collect_heredocs(out)?;
            }
            self.stamp_lines(out, lines, offsets, ends, self.line, start_pos);
        }
        Ok(())
    }

    /// The 1-based source line the cursor sits on *right now*, mid-token.
    ///
    /// [`Lexer::line`] is only advanced once per `run` iteration, so during a
    /// word read it still names the line the word *started* on. Adding the
    /// newlines consumed since the iteration began gives the cursor's true
    /// line — which is what a `$( … )` needs, its `$LINENO` base being the line
    /// of the closing paren rather than of the word that contains it.
    fn cur_line(&self) -> u32 {
        let consumed = self
            .chars
            .get(self.iter_start..self.pos)
            .unwrap_or(&[])
            .iter()
            .filter(|&&ch| ch == '\n')
            .count();
        self.line
            .saturating_add(u32::try_from(consumed).unwrap_or(u32::MAX))
    }

    /// The line an *end of input* diagnostic belongs on: one past the source's
    /// last line.
    ///
    /// bash blames an unterminated `$( … )` on the end of input rather than on
    /// its opening line, and the line it names is always one past the last: on
    /// running out it asks for another input line, and that request bumps
    /// `line_number` even though it comes back empty. A source with no trailing
    /// newline still has a final partial line, which [`Lexer::cur_line`] does
    /// not count — hence the adjustment. (Verified against bash 5.2: a 3-line
    /// script whose line 2 opens `cat <(echo a` reports line 4, while
    /// `eval 'v=$(echo a'` on line 2 of a script reports line 3.)
    fn eof_line(&self) -> u32 {
        let ends_with_newline = self.chars.last() == Some(&'\n');
        self.cur_line()
            .saturating_add(u32::from(!ends_with_newline))
    }

    /// After one `run` iteration, stamp every token appended since the iteration
    /// began with the line the iteration *ended* on, then advance `self.line`
    /// past every newline the iteration consumed
    /// (`self.chars[start_pos..self.pos]`). Counting from the consumed character
    /// span — rather than from emitted `Newline` tokens — keeps the line
    /// accurate across newlines hidden inside a token body (a quoted string, a
    /// here-doc body, a command substitution).
    ///
    /// The recorded line is the token's *last* line, not its first, because that
    /// is what bash records: bash's `line_number` names the last input line it
    /// has **fetched**, and a token is only complete once its final character
    /// has been read. So `echo "a<newline>b" $LINENO` prints 2 — the second word
    /// ends on line 2 — and a word reached across a `\<newline>` continuation
    /// (`echo \<newline>$LINENO`) likewise reports the line it ends on.
    ///
    /// Newlines are counted only *strictly before* the iteration's final
    /// character: reading that last character never forces the following line to
    /// be fetched, so a token that ends *at* a newline — the `Newline` token
    /// itself, or the one that swallowed a here-doc body — belongs to the line
    /// it terminates rather than the one after it.
    fn stamp_lines(
        &mut self,
        out: &[Tok],
        lines: &mut Vec<u32>,
        offsets: &mut Vec<u32>,
        ends: &mut Vec<u32>,
        start_line: u32,
        start_pos: usize,
    ) {
        let inner = self
            .chars
            .get(start_pos..self.pos.saturating_sub(1))
            .unwrap_or(&[])
            .iter()
            .filter(|&&ch| ch == '\n')
            .count();
        let end_line = start_line.saturating_add(u32::try_from(inner).unwrap_or(u32::MAX));
        let start = u32::try_from(start_pos).unwrap_or(u32::MAX);
        while lines.len() < out.len() {
            lines.push(end_line);
        }
        while offsets.len() < out.len() {
            offsets.push(start);
        }
        // Every token this iteration produced shares the iteration's span, so
        // they all end where the cursor now stands. For the `Newline` that
        // triggered here-document collection that is past the collected bodies,
        // which is exactly the property the history slicer relies on.
        let end = u32::try_from(self.pos).unwrap_or(u32::MAX);
        while ends.len() < out.len() {
            ends.push(end);
        }
        let consumed = self.chars[start_pos..self.pos]
            .iter()
            .filter(|&&ch| ch == '\n')
            .count();
        self.line = self.line.saturating_add(u32::try_from(consumed).unwrap_or(u32::MAX));
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
        let open = self.cur_line();
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
                    return Err(eof_matching(')').at(open));
                }
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                }
                _ => {
                    let segs = self.read_array_elem_word()?;
                    if segs.is_empty() {
                        return Err(LexError::new("unexpected operator in array assignment"));
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
                // The right-hand side of a `[[ … ]]` match is a *pattern*, and
                // bash lexes extended patterns there whether or not `extglob` is
                // set — so `[[ abc == @(abc|x) ]]` works in a default shell.
                // Only in this position: `[[ @(a) == b ]]` and `[[ -n @(a) ]]`
                // are both syntax errors near `(`.
                "==" | "!=" | "=" if self.cond_depth > 0 => self.extpat_next = true,
                _ => {}
            }
        }
        out.push(Tok::Word(segs));
    }

    /// Read the RHS of `=~` as a single word. `|`, `#`, `{`, `*` and friends are
    /// ordinary regex characters here, and an unquoted `(` opens a group whose
    /// whole contents — whitespace, newlines and shell operators alike — belong
    /// to the regex. Outside such a group the usual word boundaries still apply:
    /// blanks, a newline, and the operators `;`, `&`, `<`, `>` and `)` end the
    /// word and are handed back to the tokenizer, so bash rejects
    /// `[[ a =~ a;b ]]` and `[[ a =~ a) ]]` as conditional-expression syntax
    /// errors while accepting `[[ "a b" =~ (a b) ]]` and `[[ a =~ a|b ]]`.
    /// Quotes and `$…` expansions still apply (the RHS undergoes parameter
    /// expansion in bash), and a quoted or backslash-escaped paren is a literal
    /// one — it does not open or close a group.
    ///
    /// An unclosed group is the lexer's error, not the conditional parser's:
    /// bash's word reader scans to the matching `)` and reports
    /// "unexpected EOF while looking for matching `)'" when the input runs out.
    fn read_word_regex(&mut self) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        // Nesting depth of unquoted `(` … `)` groups. While it is non-zero the
        // word swallows everything, which is the whole point of the construct.
        let mut depth: u32 = 0;
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '<' | '>' if depth == 0 => break,
                ')' if depth == 0 => break,
                '(' => {
                    depth = depth.saturating_add(1);
                    lit.push('(');
                    self.pos += 1;
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    lit.push(')');
                    self.pos += 1;
                }
                '\'' => {
                    flush_lit(&mut segs, &mut lit);
                    self.pos += 1;
                    let s = self.read_single_quote()?;
                    segs.push(Seg::Sq(s, false));
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
                    let (raw, src) = self.read_backtick()?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
                }
                '\\' => {
                    // In the inline `=~` regex, a backslash escapes the next
                    // character *for the regex engine* — bash passes `\X` through
                    // to `regcomp`, so `\+`/`\.`/`\(` match a literal `+`/`.`/`(`.
                    // Keep the backslash (don't strip it as ordinary quote
                    // removal would) so the inline regex behaves exactly like one
                    // supplied via a variable (`re='a\+b'; [[ a+b =~ $re ]]`),
                    // whose backslashes already survive to the ERE. A trailing
                    // `\<newline>` is still a line continuation and is dropped.
                    self.pos += 1;
                    if let Some(next) = self.bump()
                        && next != '\n'
                    {
                        lit.push('\\');
                        lit.push(next);
                    } else {
                        self.note_continuation();
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
        if depth > 0 {
            return Err(eof_matching(')'));
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
    fn read_word_verbatim(&mut self, repl_escapes: bool) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        while let Some(c) = self.peek() {
            match c {
                '\'' => {
                    flush_lit(&mut segs, &mut lit);
                    self.pos += 1;
                    let s = self.read_single_quote()?;
                    segs.push(Seg::Sq(s, false));
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
                    let (raw, src) = self.read_backtick()?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
                }
                '\\' => {
                    self.pos += 1;
                    if let Some(next) = self.bump()
                        && next != '\n'
                    {
                        if repl_escapes && (next == '&' || next == '\\') {
                            // Replacement context: keep `\&`/`\\` intact so the
                            // later `&`-scan can tell an escaped ampersand (a
                            // literal `&`) from an active one.
                            lit.push('\\');
                            lit.push(next);
                        } else {
                            // An escaped character in a `${…#pat}` /
                            // `${…/pat/…}` / `${…^pat}` pattern (or
                            // replacement): emit it as a one-char single-quoted
                            // segment so it is treated as a *literal* by the
                            // pattern matcher — an escaped `*`/`?`/`[` matches
                            // that character, not as a live glob metacharacter
                            // (bash). A letter or digit could not have been a
                            // metacharacter anyway, but keeping its escape as a
                            // segment is what lets `declare -f` print the
                            // pattern back as written. Same rationale and
                            // representation as in `read_word_inner`.
                            flush_lit(&mut segs, &mut lit);
                            segs.push(Seg::Sq(next.to_string(), true));
                        }
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

    fn read_word(&mut self, extpat: bool) -> Result<Vec<Seg>, LexError> {
        self.read_word_inner(false, false, extpat)
    }

    /// Read one array-literal element word. Like [`Self::read_word`] but a
    /// *leading* `[subscript]=value` element (`([ x ]=v)`) is kept as one word
    /// even across unquoted whitespace inside the brackets, matching bash's
    /// array-literal tokenization (`declare -A m=([ x ]=v)` keys on ` x `).
    fn read_array_elem_word(&mut self) -> Result<Vec<Seg>, LexError> {
        self.read_word_inner(false, true, false)
    }

    /// Read one word; when `assign_ok`, an array-subscript at the head of the
    /// word (`name[…]`) is consumed as part of the word even across unquoted
    /// whitespace, matching bash's assignment-word tokenization. When
    /// `array_elem`, a word that *begins* with `[` slurps its `[…]` subscript the
    /// same way (for array-literal keyed elements, which have no name prefix).
    /// `extpat` forces extglob recognition on for this one word regardless of
    /// the `extglob` option — bash does that for the pattern operand of a
    /// `[[ … ]]` match.
    fn read_word_inner(
        &mut self,
        assign_ok: bool,
        array_elem: bool,
        extpat: bool,
    ) -> Result<Vec<Seg>, LexError> {
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
        // word token. Parameter expansion and quoting inside the group are still
        // processed normally.
        //
        // Whether a group is recognised *at all* is a lexing decision bash makes
        // from the `extglob` option, which is why the option is passed in rather
        // than consulted at match time: with it off, `!(cmd)` is a negated
        // subshell and `echo @(a)` is a syntax error near `(`. `extpat` adds
        // bash's one exception — the pattern operand of `[[ … == … ]]`, where
        // extglob is always on.
        let extglob = self.opts.extglob || extpat;
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
            if extglob && matches!(c, '?' | '*' | '+' | '@' | '!') && self.peek_at(1) == Some('(') {
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
                // Like `$( … )`, an unterminated process substitution is
                // reported at the *end of input*, not at the line it opened on
                // (verified against bash 5.2: `cat <(echo a` on line 2 of a
                // 3-line script reports line 4). A nested construct that closed
                // first stamps its own line and `at` will not overwrite it.
                let open_line = self.cur_line();
                let raw = self
                    .read_balanced('(', ')')
                    .map_err(|e| e.at(self.eof_line()))?;
                segs.push(Seg::ProcSub(input, raw, open_line));
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
                    segs.push(Seg::Sq(s, false));
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
                    let (raw, src) = self.read_backtick()?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
                }
                '\\' => {
                    self.pos += 1;
                    if let Some(next) = self.bump()
                        && next != '\n'
                    {
                        // A backslash-escaped character is semantically
                        // identical to that same character single-quoted
                        // (`a\*b` ≡ `a'*'b`): a literal, *quoted* character with
                        // no glob/pattern-metacharacter meaning and no
                        // tilde/parameter expansion. Emitting it as a one-char
                        // `Seg::Sq` routes it through the fully-tested
                        // single-quote path, so an escaped `*`/`?`/`[`/`~`/`$`…
                        // is treated literally in globbing, `case`, and
                        // `[[ == ]]`, matching bash (previously it was folded
                        // into an unquoted literal and wrongly matched as a live
                        // metacharacter).
                        //
                        // This holds for letters and digits too, even though an
                        // escaped one can never be a metacharacter: quoting any
                        // character of a word is what stops the word being read
                        // as a *syntactic* name, so `\if` is an ordinary command
                        // word rather than the keyword, `\a=1` is a command
                        // rather than an assignment, and `\f() { … }` is not a
                        // function definition. All three read the word as a
                        // single flattened `Seg::Lit`, so the escape has to
                        // survive as its own segment for them to say no. The
                        // command *name* is unaffected — it is the expansion,
                        // and `\ls` still expands to `ls`.
                        //
                        // The one carve-out: inside an extglob group (`@( … )`,
                        // `ext_depth > 0`) the body is accumulated as one
                        // contiguous literal, so keep the fold there rather than
                        // split the group across segments.
                        if ext_depth > 0 {
                            lit.push(next);
                        } else {
                            flush_lit(&mut segs, &mut lit);
                            segs.push(Seg::Sq(next.to_string(), true));
                        }
                    } else {
                        self.note_continuation();
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
        let open = self.cur_line();
        let mut s = String::new();
        loop {
            match self.bump() {
                Some('\'') => return Ok(s),
                Some(c) => s.push(c),
                None => return Err(eof_matching('\'').at(open)),
            }
        }
    }

    /// Read the body of a `$'…'` ANSI-C-quoted string. `self.pos` is just past
    /// the opening quote; consumes through the closing quote. The result is a
    /// literal string (no expansion/splitting).
    ///
    /// Scanning and decoding are deliberately two separate passes, mirroring
    /// bash's `parse_matched_pair` + `ansicstr` split. The scan below knows only
    /// one rule — a backslash quotes the next character — and finding the
    /// closing quote must not depend on what the escapes *mean*. Decoding
    /// inline gets the token boundary wrong whenever an escape would swallow a
    /// character that also ends the string: `$'ab\c'` is the four-character word
    /// `ab\c` (a dangling `\c`, not a control escape consuming the quote), and
    /// `$'\c\'` really does run to end-of-input.
    ///
    /// Note: byte escapes (`\xHH`, `\nnn`) naming a value above 0x7F are
    /// materialised as the Unicode code point of that value — the shell stores
    /// words as UTF-8 `String`, not raw bytes, so `$'\xff'` yields U+00FF where
    /// bash yields the single byte 0xff.
    fn read_ansi_c_quote(&mut self) -> Result<String, LexError> {
        let open = self.cur_line();
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(eof_matching('\'').at(open));
            };
            if c == '\'' {
                return Ok(crate::escape::ansi_c_unescape(&raw));
            }
            raw.push(c);
            if c == '\\' {
                let Some(e) = self.bump() else {
                    return Err(eof_matching('\'').at(open));
                };
                raw.push(e);
            }
        }
    }

    fn read_double_quote(&mut self) -> Result<Vec<Seg>, LexError> {
        let open = self.cur_line();
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(eof_matching('"').at(open));
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
                        //
                        // Everything in a double-quoted section is already
                        // quoted, so a one-char `Seg::Sq` means exactly what
                        // folding the character into the literal run meant —
                        // but it also records that the source spelled the
                        // character with a backslash, which is how bash prints
                        // it back (`declare -f` keeps `"a\"b"` as written, and
                        // rendering it as a bare `"` would emit a word that no
                        // longer re-parses).
                        Some(n @ ('"' | '\\' | '$' | '`')) => {
                            self.pos += 1;
                            flush_lit(&mut segs, &mut lit);
                            segs.push(Seg::Sq(n.to_string(), true));
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
                    let (raw, src) = self.read_backtick()?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
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
                Ok(Some(Seg::Sq(s, false)))
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
                let open = self.cur_line();
                let raw = self.read_balanced('[', ']').map_err(|e| e.at(open))?;
                Ok(Some(Seg::Arith(raw, true)))
            }
            Some('(') => {
                if self.peek_at(1) == Some('(') {
                    self.pos += 2;
                    let raw = self.read_arith()?;
                    Ok(Some(Seg::Arith(raw, false)))
                } else {
                    self.pos += 1;
                    // `$( … )` is the one construct bash blames on the *end* of
                    // input rather than its opening line: the body is re-parsed
                    // after the outer scan, by which point the line counter has
                    // moved on. (An unterminated quote *inside* the body still
                    // reports its own line — `at` will not overwrite.)
                    let raw = self
                        .read_balanced('(', ')')
                        .map_err(|e| e.at(self.eof_line()))?;
                    Ok(Some(Seg::CmdSub(raw, self.cur_line(), None)))
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
    ///
    /// The unterminated-`close` error is left **unstamped** (no reporting line):
    /// which line bash blames depends on what opened the group — the opening
    /// line for `$[`/`<(`/`>(`, the end of input for `$(` — so the caller
    /// stamps it. Errors from the nested quote scans below *are* stamped here,
    /// at the quote's own opening line, and `LexError::at` never overwrites, so
    /// the caller's stamp cannot displace them.
    fn read_balanced(&mut self, open: char, close: char) -> Result<String, LexError> {
        let mut depth = 1usize;
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(eof_matching(close));
            };
            if c == '\'' {
                let q_open = self.cur_line();
                raw.push(c);
                // Copy verbatim to the closing single quote.
                loop {
                    match self.bump() {
                        Some('\'') => {
                            raw.push('\'');
                            break;
                        }
                        Some(q) => raw.push(q),
                        None => return Err(eof_matching('\'').at(q_open)),
                    }
                }
                continue;
            }
            if c == '"' {
                let q_open = self.cur_line();
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
                        None => return Err(eof_matching('"').at(q_open)),
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
        let open = self.cur_line();
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(eof_matching('}').at(open));
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
                    let q_open = self.cur_line();
                    raw.push('\'');
                    loop {
                        match self.bump() {
                            Some('\'') => {
                                raw.push('\'');
                                break;
                            }
                            Some(q) => raw.push(q),
                            None => return Err(eof_matching('\'').at(q_open)),
                        }
                    }
                }
                // Double quotes: copy to the closing quote, honoring `\`.
                '"' => {
                    let q_open = self.cur_line();
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
                            None => return Err(eof_matching('"').at(q_open)),
                        }
                    }
                }
                // Backtick command substitution: copy verbatim to the closing
                // backtick (honoring `\``).
                '`' => {
                    let q_open = self.cur_line();
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
                            None => return Err(eof_matching('`').at(q_open)),
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
                                let inner = self
                                    .read_balanced('(', ')')
                                    .map_err(|e| e.at(self.eof_line()))?;
                                raw.push_str(&inner);
                                raw.push(')');
                            }
                        }
                        Some('[') => {
                            raw.push('[');
                            self.pos += 1;
                            let sub_open = self.cur_line();
                            let inner = self.read_balanced('[', ']').map_err(|e| e.at(sub_open))?;
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
        let open = self.cur_line();
        let mut depth = 0usize;
        let mut raw = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(eof_matching(')').at(open));
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
                        return Err(LexError::new("malformed arithmetic expansion"));
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
        out.push(Tok::HereDoc(Vec::new(), delim.clone(), !expand));
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
                    // EOF before the delimiter. In strict mode (REPL
                    // incompleteness check) this is *incomplete input* — the
                    // here-doc body is still being typed — so surface an
                    // "unexpected EOF" that the REPL treats as "keep reading".
                    // In lenient mode (script/`-c`) bash accepts the partial
                    // body, so we do too.
                    if self.strict_heredoc_eof {
                        return Err(LexError::new(format!(
                            "unexpected EOF while looking for `{}'",
                            ph.delim
                        )));
                    }
                    break;
                }
                let start = self.pos;
                while !matches!(self.peek(), None | Some('\n')) {
                    self.pos += 1;
                }
                let eol = self.pos;
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
                // An expanding here-doc (unquoted delimiter) joins a line ending
                // in an unescaped `\` to the next one, so the history has to
                // drop that pair as well. A quoted delimiter makes the body
                // literal, backslashes and all.
                if ph.expand && ends_with_continuation(content) {
                    // The backslash is the last character before the newline.
                    let mut at = eol.saturating_sub(1);
                    if self.chars.get(at) == Some(&'\r') {
                        at = at.saturating_sub(1);
                    }
                    self.conts.push(u32::try_from(at).unwrap_or(u32::MAX));
                }
                body.push_str(content);
                body.push('\n');
            }
            let segs = scan_heredoc_segs(&body, ph.expand)?;
            if let Some(slot) = out.get_mut(ph.tok_index) {
                *slot = Tok::HereDoc(segs, ph.delim.clone(), !ph.expand);
            }
        }
        Ok(())
    }

    /// Read a `` ` … ` `` body, returning two things: the text to *parse*, with
    /// the three escapes bash strips first removed, and the verbatim source
    /// between the backticks, which is what `declare -f` prints back.
    ///
    /// They differ exactly where an escape appeared, and printing the parsed
    /// text instead would not merely lose the spelling — a nested backtick
    /// would come out unescaped, and the result would no longer parse.
    fn read_backtick(&mut self) -> Result<(String, String), LexError> {
        let open = self.cur_line();
        let start = self.pos;
        let mut raw = String::new();
        loop {
            match self.bump() {
                Some('`') => {
                    let src = self.chars.get(start..self.pos.saturating_sub(1));
                    return Ok((raw, src.unwrap_or_default().iter().collect()));
                }
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
                None => return Err(eof_matching('`').at(open)),
            }
        }
    }
}

/// Whether `line` ends in a `\` that is not itself escaped — a line
/// continuation, which the reader joins to the following line.
fn ends_with_continuation(line: &str) -> bool {
    line.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
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
    let mut lx = Lexer::new(body, LexOpts::default());
    let mut segs: Vec<Seg> = Vec::new();
    let mut lit = String::new();
    while let Some(c) = lx.peek() {
        match c {
            '\\' => {
                lx.pos += 1;
                match lx.peek() {
                    Some(n @ ('$' | '`' | '\\')) => {
                        lx.pos += 1;
                        // As in a double-quoted section: a here-doc body is
                        // never split or globbed, so a one-char `Seg::Sq` is
                        // interchangeable with folding the character into the
                        // literal run — but it also records the backslash, so
                        // `declare -f` can print the body back as written.
                        flush_lit(&mut segs, &mut lit);
                        segs.push(Seg::Sq(n.to_string(), true));
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
                let (raw, src) = lx.read_backtick()?;
                segs.push(Seg::CmdSub(raw, lx.cur_line(), Some(src)));
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

    /// Lex under bash's non-interactive defaults, which is what all but the
    /// option-specific tests want. Shadows [`super::tokenize`] so the option
    /// argument does not have to be spelled out 30 times.
    fn tokenize(src: &str) -> Result<Vec<Tok>, LexError> {
        super::tokenize(src, LexOpts::default())
    }

    /// As [`tokenize`], with per-token line numbers.
    fn tokenize_spanned(src: &str) -> Result<(Vec<Tok>, Vec<u32>), LexError> {
        super::tokenize_spanned(src, LexOpts::default())
    }

    /// Tokenize and drop the terminating `Newline`, so a test that counts words
    /// counts only words. Every input carries one — see `Lexer::run_into`.
    fn toks_of(src: &str) -> Vec<Tok> {
        let mut toks = tokenize(src).unwrap();
        assert!(
            matches!(toks.last(), Some(Tok::Newline)),
            "input not newline-terminated: {toks:?}"
        );
        toks.pop();
        toks
    }

    /// [`open_quote`] — the reader state history expansion needs. The `$(` vs
    /// backquote split is bash's own and is the reason this reports only the
    /// *innermost* unclosed delimiter (see the function's docs).
    #[test]
    fn open_quote_reports_the_innermost_unclosed_quote() {
        let q = |src: &str| super::open_quote(src, LexOpts::default());
        assert_eq!(q("echo 'x\n"), Some('\''));
        assert_eq!(q("echo \"x\n"), Some('"'));
        // A quote inside `$( … )` is the reader's state; one inside a backquote
        // body is not, because that body is scanned as a flat matched pair and
        // so the backtick is what the lexer is still looking for.
        assert_eq!(q("echo $(echo 'x\n"), Some('\''));
        assert_eq!(q("echo `echo 'y\n"), None);
        // Innermost wins over the construct enclosing it — but a `'` inside a
        // double-quoted string is not a quote at all, so the state stays `"`.
        // (readline's *in-line* scan does toggle on it, which is a separate rule
        // and is why `echo "a 'b` / `c' !!` leaves the `!!` literal.)
        assert_eq!(q("echo \"a 'b\n"), Some('"'));
        assert_eq!(q("echo $(echo \"a 'b\n"), Some('"'));
        // Everything that is not a quote reports nothing, including the other
        // unclosed constructs and a plain incomplete compound command.
        for src in ["echo $(x\n", "echo ${x\n", "if true; then\n", "echo x |\n", "echo x \\\n"] {
            assert_eq!(q(src), None, "{src:?}");
        }
        // A complete line has no state to carry.
        assert_eq!(q("echo 'x'\n"), None);
    }

    /// [`ends_in_continuation`] — the other half of the reader state. The point
    /// of asking the lexer rather than the text is the negative cases: a `\`
    /// the lexer *keeps* is not a continuation, so the reader must not wait.
    #[test]
    fn ends_in_continuation_asks_which_backslashes_were_deleted() {
        let c = |src: &str| super::ends_in_continuation(src, LexOpts::default());
        assert!(c("echo x \\\n"));
        // `\<CR><LF>` is not one: the `\` escapes the CR. Measured — a CRLF
        // script's `echo x \` prints `x \r` in bash too, joined to nothing.
        assert!(!c("echo x \\\r\n"));
        // An escaped backslash is a literal one, so the line is finished.
        assert!(!c("echo x \\\\\n"));
        assert!(c("echo x \\\\\\\n"));
        // Nothing to continue onto: no newline after the `\`, or no `\` at all.
        assert!(!c("echo x \\"));
        assert!(!c("echo x\n"));
        assert!(!c(""));
        // A `\` the lexer keeps is not a continuation — inside `'…'` it is a
        // literal backslash, and inside a quoted-delimiter here-document body
        // the whole body is literal.
        assert!(!c("echo 'x \\\n"));
        assert!(!c("cat <<'E'\nx \\\n"));
        // …but an unquoted here-doc delimiter makes its body join lines, so there
        // the same text *is* a continuation.
        assert!(c("cat <<E\nx \\\n"));
        // Only the final line counts: an earlier continuation is already joined.
        assert!(!c("echo x \\\ny\n"));
    }

    /// `extglob` is a lexing option, so with it off `@(` is an ordinary `@` word
    /// followed by the `(` *metacharacter* — several tokens, not one. Every
    /// expectation here is bash 5.2.37's own.
    #[test]
    fn extglob_gates_pattern_groups() {
        for open in ['?', '*', '+', '@', '!'] {
            let src = format!("echo {open}(a|b)");
            assert_eq!(
                toks_of(&src),
                vec![
                    Tok::Word(vec![Seg::Lit("echo".into())]),
                    Tok::Word(vec![Seg::Lit(open.to_string())]),
                    Tok::Op(Op::LParen),
                    Tok::Word(vec![Seg::Lit("a".into())]),
                    Tok::Op(Op::Pipe),
                    Tok::Word(vec![Seg::Lit("b".into())]),
                    Tok::Op(Op::RParen),
                ],
                "extglob off: {src}"
            );
            let mut with = super::tokenize(&src, LexOpts { extglob: true }).unwrap();
            with.pop();
            assert_eq!(
                with,
                vec![
                    Tok::Word(vec![Seg::Lit("echo".into())]),
                    Tok::Word(vec![Seg::Lit(format!("{open}(a|b)"))]),
                ],
                "extglob on: {src}"
            );
        }
        // A word may also *end* in one of those characters, which is why the
        // gate matters for more than patterns: `f?() { :; }` is a function
        // definition in bash, and only lexes as one when extglob is off.
        assert_eq!(
            toks_of("f?()"),
            vec![
                Tok::Word(vec![Seg::Lit("f?".into())]),
                Tok::Op(Op::LParen),
                Tok::Op(Op::RParen),
            ]
        );
    }

    /// bash lexes an extended pattern for the operand of a `[[ … ]]` match
    /// whatever the option says — but only there.
    #[test]
    fn extglob_always_on_for_dbracket_pattern() {
        for op in ["==", "!=", "="] {
            assert_eq!(
                toks_of(&format!("[[ abc {op} @(abc|x) ]]")),
                vec![
                    Tok::Word(vec![Seg::Lit("[[".into())]),
                    Tok::Word(vec![Seg::Lit("abc".into())]),
                    Tok::Word(vec![Seg::Lit((*op).into())]),
                    Tok::Word(vec![Seg::Lit("@(abc|x)".into())]),
                    Tok::Word(vec![Seg::Lit("]]".into())]),
                ],
                "operand of {op}"
            );
        }
        // Any other position in the same construct is lexed normally, so the
        // `(` stays a metacharacter and the parser reports a syntax error.
        assert_eq!(
            toks_of("[[ @(a) == b ]]"),
            vec![
                Tok::Word(vec![Seg::Lit("[[".into())]),
                Tok::Word(vec![Seg::Lit("@".into())]),
                Tok::Op(Op::LParen),
                Tok::Word(vec![Seg::Lit("a".into())]),
                Tok::Op(Op::RParen),
                Tok::Word(vec![Seg::Lit("==".into())]),
                Tok::Word(vec![Seg::Lit("b".into())]),
                Tok::Word(vec![Seg::Lit("]]".into())]),
            ]
        );
        // The flag is one-shot: it must not leak past the operand onto the
        // words after it. `[[ a == b && c == @(d) ]]` is nine tokens either
        // way, so check the tail is unaffected by the earlier match.
        assert_eq!(
            toks_of("[[ a == b && c == @(d) ]]").last(),
            Some(&Tok::Word(vec![Seg::Lit("]]".into())]))
        );
    }

    #[test]
    fn simple_words() {
        let toks = tokenize("echo hello world").unwrap();
        // Three words, plus the newline every input is terminated with even
        // when the source has none of its own (see `Lexer::run_into`).
        assert_eq!(toks.len(), 4);
        assert!(matches!(toks[0], Tok::Word(_)));
        assert!(matches!(toks[3], Tok::Newline));
        // A source that already ends in a newline does not gain a second one.
        assert_eq!(tokenize("echo hello world\n").unwrap().len(), 4);
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
        // Four words plus the terminating newline.
        assert_eq!(toks.len(), 5);
    }

    #[test]
    fn command_sub_and_arith() {
        let toks = tokenize("echo $(date) $((1 + 2))").unwrap();
        if let Tok::Word(segs) = &toks[1] {
            assert!(matches!(segs[0], Seg::CmdSub(..)));
        } else {
            panic!("expected word");
        }
        if let Tok::Word(segs) = &toks[2] {
            assert!(matches!(segs[0], Seg::Arith(_, false)));
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
        let toks = toks_of("h[a b]=v");
        assert_eq!(toks.len(), 1);
        match &toks[0] {
            Tok::Word(segs) => assert_eq!(segs.as_slice(), &[Seg::Lit("h[a b]=v".into())]),
            other => panic!("expected single word, got {other:?}"),
        }
        // Chained assignments: the second word is still assignment position.
        let toks = toks_of("h[a b]=1 h[c d]=2");
        assert_eq!(toks.len(), 2, "expected two words, got {toks:?}");
        // In *argument* position the subscript splits normally on the space.
        let toks = toks_of("echo h[a b]=v");
        assert_eq!(toks.len(), 3, "argument-position subscript must split: {toks:?}");
    }

    #[test]
    fn array_literal_keyed_element_keeps_spaces() {
        // Inside an array literal, a keyed element `[ x ]=v` stays one element
        // even with unquoted interior spaces (bash tokenises `([ x ]=v)` as a
        // single subscript-value element). Regression for TD-OILS-ASSOC-KEY-TRIM.
        let toks = toks_of("m=([ x ]=v [y z]=w)");
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
                Seg::CmdSub(raw, close, _) => {
                    assert_eq!(raw, "echo $(echo x)");
                    // Single-line input: the closing paren is on line 1.
                    assert_eq!(*close, 1);
                }
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
            Tok::HereDoc(segs, ..) => Some(segs.clone()),
            _ => None,
        });
        let segs = hd.expect("here-doc token");
        assert_eq!(segs, vec![Seg::Lit("line one\nline two\n".to_string())]);
    }

    #[test]
    fn token_lines_account_for_heredoc_and_quotes() {
        // The lexer stamps each token with its true source line even when
        // earlier tokens swallowed newlines (a here-doc body, a quoted string).
        // `cat <<EOF\nbody\nEOF\nlast` — the `last` word is on physical line 4.
        let (toks, lines) = tokenize_spanned("cat <<EOF\nbody\nEOF\nlast").unwrap();
        let idx = toks
            .iter()
            .position(|t| matches!(t, Tok::Word(segs) if segs.as_slice() == [Seg::Lit("last".into())]))
            .expect("last word");
        assert_eq!(lines[idx], 4);
        // A double-quoted string with an embedded newline (physical lines 1-2);
        // the trailing `y` word therefore sits on line 3.
        let (toks, lines) = tokenize_spanned("x=\"a\nb\"\ny").unwrap();
        let idx = toks
            .iter()
            .position(|t| matches!(t, Tok::Word(segs) if segs.as_slice() == [Seg::Lit("y".into())]))
            .expect("y word");
        assert_eq!(lines[idx], 3);
    }

    #[test]
    fn here_doc_strip_tabs() {
        let toks = tokenize("cat <<-END\n\t\tindented\n\tEND\n").unwrap();
        let segs = toks
            .iter()
            .find_map(|t| match t {
                Tok::HereDoc(segs, ..) => Some(segs.clone()),
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
