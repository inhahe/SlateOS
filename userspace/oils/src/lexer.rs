//! Tokenizer for the OSH shell language.
//!
//! The lexer turns source text into a flat token stream. Words are captured as
//! a list of [`Seg`] fragments that preserve quoting; command/parameter/
//! arithmetic substitutions keep their *raw inner source* so the parser can
//! recursively parse them (this keeps the lexer free of a dependency on the
//! parser).
//!
//! Source is **bytes**: a shell word may hold any byte, so the scan runs over
//! [`Ch`] — a decoded scalar or one undecodable byte — rather than `char`. The
//! syntax it recognises is entirely ASCII, so [`syn`] gives every scanning site
//! an ASCII view without any of them having to case-split on decodability.

use crate::bfmt;
use crate::bytes::{self, BStr, Ch, Str};

/// A character as shell *syntax*: ASCII as itself, anything else as NUL.
///
/// Every metacharacter, quote, operator and reserved word in the grammar is
/// ASCII, so a character that is not ASCII can never *be* one — and NUL is the
/// one ASCII spelling the grammar assigns no meaning to. Folding the rest onto
/// it means a scan can compare against the character it cares about without
/// first asking whether the input decoded, while the character's own bytes stay
/// available through [`Lexer::bump_ch`] for the literal runs that must keep
/// them.
fn syn(c: Ch) -> char {
    match c {
        Ch::U(c) if c.is_ascii() => c,
        _ => '\0',
    }
}

/// One ASCII syntax character as a byte string.
fn one(c: char) -> Str {
    Ch::U(c).to_str()
}

/// Append one ASCII syntax character.
fn push1(out: &mut Str, c: char) {
    Ch::U(c).push_to(out);
}

/// A here-document body line with its leading tabs removed, for `<<-`.
fn strip_tabs(line: BStr<'_>) -> BStr<'_> {
    let n = line.iter().take_while(|&&b| b == b'\t').count();
    line.get(n..).unwrap_or_default()
}

/// A lexer error with a human-readable message (unbalanced quote, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// The message, as bytes: most are fixed text, but the here-document one
    /// quotes back the delimiter the reader was looking for, and a delimiter is
    /// a shell word — `<<a\xffb` must name the delimiter it actually wanted.
    pub msg: Str,
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
    /// True when the failure costs only the parse unit that holds it, leaving the
    /// shell to read on. See [`crate::parser::ParseError::recoverable`], which
    /// this becomes.
    pub recoverable: bool,
}

impl LexError {
    /// A lexer error with no line preference; the caller's fallback applies.
    pub(crate) fn new(msg: &(impl bytes::PushBytes + ?Sized)) -> Self {
        Self { msg: bfmt![msg], line: None, looking_for: None, recoverable: false }
    }

    /// Fill in the reporting line if the raise site did not already choose one.
    /// Never overwrites: an outer construct must not claim an inner one's line.
    pub(crate) fn at(mut self, line: u32) -> Self {
        self.line.get_or_insert(line);
        self
    }

    /// Mark this error as costing only its own parse unit. See [`Self::recoverable`].
    pub(crate) fn recoverable(mut self) -> Self {
        self.recoverable = true;
        self
    }
}

/// bash's end-of-input diagnostic for an unclosed quote, substitution, or group.
/// bash names the delimiter it was scanning for, e.g. `unexpected EOF while
/// looking for matching `)'` — a single backtick, the closing char, then a
/// single quote — so a `$(`/`(` reports `)`, `${` reports `}`, `"` reports `"`.
/// bash's end-of-input diagnostic for a here-document whose delimiter never
/// arrived.
///
/// The delimiter is a shell word, so it goes back into the message as the bytes
/// the user wrote: `<<a\xffb` names the delimiter it was actually looking for,
/// which is the same byte string it compares the body lines against.
fn unterminated_heredoc(delim: BStr<'_>) -> LexError {
    LexError::new(&bfmt![b"unexpected EOF while looking for `", delim, b"'"])
}

fn eof_matching(close: char) -> LexError {
    LexError {
        msg: bfmt![b"unexpected EOF while looking for matching `", close, b"'"],
        line: None,
        looking_for: Some(close),
        recoverable: false,
    }
}

/// The shell operators, longest first, as bash spells them when it names one in
/// `syntax error near unexpected token \`…'`.
///
/// Longest-first order is what makes the scan pick `;;&` over `;;` over `;`, the
/// way the main token loop's nested lookahead does. Only used to name a token in
/// a diagnostic — the loop itself still recognises operators structurally.
const OPERATOR_SPELLINGS: [&str; 21] = [
    ";;&", "<<<", "<<-", "&&", "||", ";;", ";&", "|&", "<<", ">>", "<&", ">&", "<>", ">|", ";",
    "&", "|", "<", ">", "(", ")",
];

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
    Lit(Str),
    /// A quoted-literal run: the contents of `'…'`/`$'…'`, or a single
    /// backslash-escaped character, which means exactly the same thing
    /// (`a\*b` ≡ `a'*'b`).
    ///
    /// The `bool` is `true` for the backslash spelling. The two are
    /// interchangeable during expansion, but bash prints a stored function
    /// body back in whichever form the source wrote (`declare -f`), so the
    /// distinction has to survive lexing.
    Sq(Str, bool),
    /// Double-quoted run of fragments.
    Dq(Vec<Seg>),
    /// `$name` / `$1` / `$?` … a bare parameter reference.
    Param(String),
    /// `${ … }` — raw inner text, parsed later.
    ParamBraced(Str),
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
    CmdSub(Str, u32, Option<Str>),
    /// `$(( … ))` — raw arithmetic expression text.
    /// The `bool` is `true` when the deprecated `$[ … ]` spelling was used. The
    /// two evaluate identically, but bash prints a stored function body back in
    /// whichever form the source wrote, so the distinction must survive here.
    Arith(Str, bool),
    /// `<( … )` / `>( … )` process substitution — the `bool` is `true` for the
    /// input form `<(…)`, the `String` is the raw inner command source, and the
    /// `u32` is the 1-based source line the `<(`/`>(` opens on.
    ///
    /// bash blames a syntax error in the body on the body's own line, counted in
    /// the enclosing source; the body is lexed on its own, so the parser needs
    /// the opening line to shift it back (`parser::parse_procsub_body`). Unlike
    /// a `$( … )` body there is no rank-based renumbering — this really is a
    /// plain offset.
    ProcSub(bool, Str, u32),
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
    HereDoc(Vec<Seg>, Str, bool),
    /// `(( … ))` — an arithmetic command, holding the raw expression text.
    ArithCmd(Str),
    /// `name=( … )` / `name+=( … )` — an array assignment. Each element is a
    /// word captured as its own [`Seg`] list.
    ArrayAssign {
        name: String,
        /// The subscript of `name[sub]=( … )`, as raw source between the
        /// brackets, for the parser to read as a word.
        ///
        /// bash's word rule for a compound literal is about the **word**, not
        /// the destination: a subscripted name makes an assignment word like any
        /// other, and putting a list in one element is objected to later, when
        /// the value is bound (`n[1]: cannot assign list to array member`). So
        /// the shape has to reach the interpreter rather than being refused
        /// here.
        index: Option<Str>,
        /// `+=` (append) rather than `=`.
        append: bool,
        elems: Vec<Vec<Seg>>,
    },
    /// A construct the lexer refused, standing in for it so that tokenizing can
    /// carry on past it. Holds the spelling of the token to blame.
    ///
    /// Only an array literal produces one today (`a=(x; y)` — an operator where
    /// an element belongs). bash reaches such a literal with its reader, which
    /// has already collected the balanced `( … )`, so a failure *inside* it costs
    /// only the parse unit that holds it: the shell reports the usual `syntax
    /// error near unexpected token` line, scores `$?` 1, and goes on reading.
    /// Returning `Err` here instead would end the whole token stream and take
    /// every later line with it. See [`crate::parser::ParseError::recoverable`].
    Invalid(Str),
}

/// The words after which a reserved word — and so an arithmetic command — is
/// still recognised. bash's `reserved_word_acceptable` (parse.y), plus `for`.
///
/// `for` is there because bash reads the arithmetic `for (( … ))` header by a
/// route of its own; osh gets the same shape from the ordinary `((` token. The
/// others bash leaves out are left out here too: after `in`, `case` or `select`
/// what follows is a pattern or a name, which is why `case x in ((p)` is a
/// pattern that opens with a paren rather than an arithmetic command.
const ARITH_CMD_AFTER: &[&str] = &[
    "{", "}", "!", "do", "done", "elif", "else", "esac", "fi", "for", "if", "then", "time",
    "until", "while", "coproc",
];

/// Whether a `((` standing here opens an arithmetic command rather than two
/// nested subshells.
///
/// bash decides this from the token it has just read: `((` is an `ARITH_CMD`
/// only where a reserved word would be recognised — the start of the input,
/// after a separator, and after the words that open or close a compound
/// command. Anywhere else it hands back a single `(` and reads the second one
/// again as the next token, so `echo ((1))` is a syntax error near `(` and not
/// an arithmetic command. An assignment prefix blocks it too (`x=1 ((2))` is
/// the same error), which falls out of the same rule: bash does not recognise a
/// reserved word after one either.
fn arith_cmd_position(prev: Option<&Tok>) -> bool {
    match prev {
        None | Some(Tok::Newline) => true,
        Some(Tok::Op(op)) => matches!(
            op,
            Op::Semi
                | Op::DSemi
                | Op::SemiAmp
                | Op::DSemiAmp
                | Op::LParen
                | Op::RParen
                | Op::Pipe
                | Op::PipeAmp
                | Op::Amp
                | Op::AndIf
                | Op::OrIf
        ),
        Some(Tok::Word(segs)) => matches!(segs.as_slice(),
            [Seg::Lit(s)] if ARITH_CMD_AFTER.iter().any(|w| w.as_bytes() == s.as_slice())),
        _ => false,
    }
}

/// Shell options that change how source text is *read* — tokenized or parsed —
/// so they must be known before a unit is read rather than when it runs.
///
/// bash reads, parses and executes one unit at a time, so an option set by unit
/// N is in force for the reading of unit N+1 — and only from there. The default
/// is bash's own for a non-interactive shell.
///
/// They travel together because they are sampled at the same moment and by the
/// same caller ([`Shell::parse_opts`](crate::interp::Shell::parse_opts)), even
/// though one is consulted by the lexer and the other by the grammar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOpts {
    /// `shopt -s extglob`: read `?(`, `*(`, `+(`, `@(` and `!(` as the opener of
    /// an extended-pattern group, swallowing the balanced `( … )` into the word.
    /// With it off those are ordinary characters and the `(` is a
    /// metacharacter — which is why `!(cmd)` is a *negated subshell* by default,
    /// and why `echo @(a)` is a syntax error.
    pub extglob: bool,
    /// `set -o posix`: the grammar reads `time` as the reserved word only when
    /// what follows it does not look like an option. bash gives up on the
    /// reserved word and searches for an external `time` instead, so in posix
    /// mode `time -p echo hi` is `time: command not found` — and with it goes
    /// `time`'s own `-p`/`--`, which are only ever read in that position.
    pub posix: bool,
}

struct Lexer {
    chars: Vec<Ch>,
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
    opts: ParseOpts,
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
    /// When `true`, no newline collects a here-document body: the pendings are
    /// left in [`Lexer::pending_heredocs`] for the caller to hand on. The alias
    /// pass lexes this way, because bash reads a here-document body with
    /// `read_a_line`, which calls `yy_getc` — `bash_input.getter`, the *real*
    /// input stream — and so bypasses `shell_input_line` and the pushed alias
    /// string entirely. A `<<` written in an alias value therefore takes its body
    /// from the line after the one the alias word stands on, and a body written
    /// in the value itself is not a body at all but ordinary commands. See
    /// [`tokenize_alias_body`].
    defer_heredocs: bool,
    /// Character offsets of every `\` whose `\<newline>` this lexer *deleted* as
    /// a line continuation. Only the reader — [`crate::parser::IncrementalParser`],
    /// slicing a parse unit's source for the command history — has any use for
    /// these: bash's history stores the joined line, without the backslash or
    /// the newline, exactly where its own reader dropped them. A lexer run over
    /// a sub-string (a `${…}` replacement, a here-doc body scan) fills this in
    /// too, but its offsets are relative to that string and are simply dropped
    /// with the lexer.
    conts: Vec<u32>,
    /// Reader-level warnings raised during the scan, in the order they happened.
    /// The here-document-at-EOF ones are only filled in lenient mode (see
    /// `strict_heredoc_eof`), since strict mode raises instead. See
    /// [`ReaderWarning`].
    warnings: Vec<ReaderWarning>,
    /// Set while the here-document reader has run **ahead** of the token cursor.
    ///
    /// bash reads a line at a time, so gathering a body in the middle of a line
    /// — which is what a `$( … )` closing over an ungathered here-document
    /// forces — fetches the *following* lines without disturbing the line being
    /// parsed. Our cursor is a single index into a flat buffer, so the read-ahead
    /// is recorded here instead: the token cursor stays where it was and jumps to
    /// `pos` the moment it reaches the end of the line it is on. See
    /// [`Lexer::gather_ahead`] and [`Lexer::sync_ahead`].
    hd_ahead: Option<HdAhead>,
    /// Index in the output token stream of the token the current
    /// [`Lexer::run_into`] iteration is about to push. The word readers are not
    /// given `out`, so a reader-level record raised deep inside a word — an
    /// unterminated here-document declared inside a `$( … )`, whose body the
    /// enclosing scan consumes — has no other way to name the token it belongs
    /// to. Left at 0 by the lexers that never run the token loop.
    next_tok_index: usize,
    /// For each here-document body this scan collected: its placeholder token's
    /// index, and the number of input lines the collection consumed. See
    /// [`Tokenized::heredoc_lines`], which is this in the dense form the reader
    /// indexes by token.
    heredoc_lines: Vec<(usize, u32)>,
    /// Set when the scan ran out of input inside a `$( … )`, which is the one
    /// construct that discards the here-documents pending around it. See
    /// [`Lexer::read_subst_body`] and [`UngatheredHeredoc`].
    heredocs_forgotten: bool,
}

/// A warning bash raises from its **reader** rather than from the parse or the
/// run: something about a here-document's body, noticed while fetching lines.
///
/// The two kinds share one ordered channel because their relative order is
/// observable — `x=$(cat <<EOF)` at the end of input warns about the
/// substitution first and about the missing delimiter second — and because they
/// are released by the same rule: only once the parse unit containing their
/// token is handed out, so a warning lands after the output of every earlier
/// line and not at all if a syntax error means bash never reads that far.
#[derive(Clone, Debug)]
pub enum ReaderWarning {
    /// A here-document whose delimiter never arrived.
    HeredocEof(HeredocEof),
    /// A `$( … )` that closed with here-documents still ungathered.
    SubstHeredoc(SubstHeredoc),
}

impl ReaderWarning {
    /// The token whose parse unit this warning is released with.
    #[must_use]
    pub fn tok_index(&self) -> usize {
        match self {
            Self::HeredocEof(h) => h.tok_index,
            Self::SubstHeredoc(s) => s.tok_index,
        }
    }

    /// Re-key this warning onto another token.
    ///
    /// A warning raised by a gather run out of the middle of the input — an
    /// alias-spliced here-document, whose body the expansion pass could not
    /// read — starts out keyed to the index it had inside that run. The caller
    /// knows which token of the real input owns the gather and moves it there,
    /// so the release rule ("once the unit containing the token is handed out")
    /// still names a token the parser will actually reach.
    pub fn set_tok_index(&mut self, i: usize) {
        match self {
            Self::HeredocEof(h) => h.tok_index = i,
            Self::SubstHeredoc(s) => s.tok_index = i,
        }
    }
}

/// A here-document whose delimiter never arrived: the input ran out first.
///
/// bash accepts the partial body and runs the command, but warns — and the
/// warning carries *two* different line numbers, neither of which is the line the
/// `<<` operator sits on. Both are "the last input line bash had **fetched**" at
/// a particular moment, which is what [`Lexer::fetched_line`] computes.
#[derive(Clone, Debug)]
pub struct HeredocEof {
    /// The delimiter that was wanted, in its unquoted form (`<<"EOF"` wants
    /// `EOF`).
    pub delim: Str,
    /// The line named *inside* the message (`here-document at line N`): the last
    /// line fetched when body collection **began**. For a lone here-document that
    /// is the operator's own line, but for the second of two it is the line the
    /// first one's body stopped on — `cat <<A <<B` / `one` / `A` / `two` blames
    /// B on line 3.
    pub body_line: u32,
    /// The line in the message's *prefix*: the last line fetched when the input
    /// ran out, i.e. the number of lines the input has.
    pub eof_line: u32,
    /// Index into the token stream of the here-document's placeholder token — the
    /// same `tok_index` the pending record carried. The reader uses it to decide
    /// *when* to print: bash warns as it reads, so the warning belongs to the
    /// parse unit containing this token and must not be printed before the
    /// commands on earlier lines have run. (Nor at all, if a syntax error on an
    /// earlier line means bash never reaches this line: `echo one )` followed by
    /// an unterminated here-document warns not at all.)
    pub tok_index: usize,
}

/// How far the here-document reader has run ahead of the token cursor.
#[derive(Clone, Copy, Debug)]
struct HdAhead {
    /// Where the read-ahead stopped: the token cursor jumps here when it reaches
    /// the end of the line it is on.
    pos: usize,
    /// The line the reader had fetched when it stopped — [`Lexer::fetched_line`]
    /// answers with this while the token cursor is still behind, because that is
    /// what bash's `line_number` holds.
    line: u32,
}

/// A `$( … )` whose `)` arrived while here-documents declared inside it were
/// still waiting for their bodies — because the `<<` and the `)` are on the same
/// line, so the body can only lie *past* the substitution's own text.
///
/// bash reads whole lines, so this is recoverable: it warns, then fetches the
/// following lines right there and gives the bodies to the substitution anyway,
/// leaving the line it was parsing to resume after the `)`. See
/// [`Lexer::gather_ahead`].
#[derive(Clone, Debug)]
pub struct SubstHeredoc {
    /// How many were outstanding. bash prints the count, and pluralises on it.
    pub count: usize,
    /// The line bash had fetched when the `)` was reached — which for the
    /// *second* such substitution on a line is not that line at all but wherever
    /// the first one's bodies left the reader.
    pub line: u32,
    /// The token the warning is released with; see [`HeredocEof::tok_index`].
    pub tok_index: usize,
}

/// A here-document that was declared but whose body was never even reached: an
/// unterminated construct *after* the `<<` swallowed the rest of the input, so
/// the line that would have ended the introducing line never arrived and the
/// ordinary gather at the newline never ran.
///
/// bash still warns about it, from the *second* gather site — the `simple_list`
/// yacc action (parse.y:1217) — which fires when the error token reduces the
/// top-level list. `gather_here_documents()` then finds the pending record,
/// calls `make_here_document`, and that raises the same
/// `here-document at line N delimited by end-of-file` warning the newline gather
/// would have. Two things distinguish it from the newline case, and both are
/// observable:
///
///   * it is printed **after** the syntax error, not before, because the
///     reduction that gathers happens after the token has been read and
///     reported;
///   * both of its line numbers are the *end* of the input ([`Lexer::eof_line`]),
///     since the reader had already consumed every line looking for the
///     construct's close.
///
/// And it only fires for a *top-level* list: the same action inside
/// `compound_list` (parse.y:1148) is guarded on the previous token having been a
/// newline, so `{ cat <<E "` warns not at all. The lexer cannot tell which,
/// having no reductions of its own, so it records the operator's offset and lets
/// [`crate::parser::IncrementalParser`] decide by re-parsing the prefix.
#[derive(Clone, Debug)]
pub struct UngatheredHeredoc {
    /// The delimiter that was wanted, in its unquoted form.
    pub delim: Str,
    /// The line the warning names, both in its prefix and inside its text: the
    /// last line the input has, plus one if it did not end in a newline.
    pub line: u32,
    /// Character offset into `src` of the `<<` operator's own token. The reader
    /// re-parses `src[..op_offset]` to find out whether the `<<` stood at the top
    /// level, which is the only place the reduction that warns can happen.
    pub op_offset: u32,
}

/// A here-document awaiting its body (collected when the introducing line ends).
struct PendingHeredoc {
    /// The end delimiter (unquoted form).
    delim: Str,
    /// `<<-`: strip leading tabs from body lines and the closing delimiter.
    strip: bool,
    /// Whether the body undergoes parameter/command/arith expansion (false when
    /// the delimiter was quoted).
    expand: bool,
    /// Index into the output token stream of the placeholder to fill in.
    tok_index: usize,
}

/// Where a `case` the substitution-extent scan has walked into currently sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CasePhase {
    /// After `case`, before its `in`. The subject word is being read.
    AwaitIn,
    /// In a pattern list: the next `)` at the `case`'s own depth terminates a
    /// pattern rather than closing a group.
    Pattern,
    /// In a clause body, until `;;` / `;&` / `;;&` starts the next pattern list
    /// or `esac` ends the `case`.
    Body,
}

/// One `case` the scan is inside.
#[derive(Clone, Copy, Debug)]
struct CaseFrame {
    /// The parenthesis depth the `case` was read at, which is the depth a
    /// pattern's `)` sits at. Nested `case`s can share a depth, so this is not a
    /// key — the frames form a stack.
    depth: usize,
    phase: CasePhase,
    /// Whether nothing of the current pattern has been read yet, which is the
    /// only place a `(` is the pattern's *optional open* — it has no mate of its
    /// own, the pattern's terminating `)` stands in for one.
    pat_start: bool,
}

/// The `case`-awareness of [`Lexer::read_subst_body`].
///
/// A `case` pattern's `)` has no opening mate, so a scan that finds the end of a
/// substitution by counting parentheses reaches zero in the middle of the
/// `case` — the single commonest way `$( … )` is written that a counting scan
/// cannot read (`x=$(case $y in a) … esac)`). bash has no such trouble because
/// its extent scan *is* the parser; ours has to know just enough grammar to tell
/// a pattern's `)` from a group's.
///
/// "Just enough" is: which words are the reserved `case`, `in` and `esac`, and
/// where a clause body ends. That in turn needs command position — `case`,
/// `esac` and `do` are reserved words only there, and `$(printf %s case in f)`
/// must keep its `)` — so this tracks the word being read and whether a word
/// starting here would begin a command. Quoting is what makes `"esac"` a plain
/// word, so a word with any quoted character in it is never reserved.
struct CaseScan {
    /// The `case`s the scan is inside, innermost last.
    frames: Vec<CaseFrame>,
    /// The unquoted characters of the word being read.
    word: Str,
    /// Whether every character of that word so far was unquoted. A quoted span
    /// contributes nothing to `word`, so this is also what says a word is being
    /// read at all when `word` is empty (`""` is a word; nothing is not).
    word_pure: bool,
    /// Whether the word being read is the first of a pattern list, which is
    /// where `esac` is reserved. Sampled when the word *begins*, because its own
    /// first character is what ends [`CaseFrame::pat_start`].
    word_pat_start: bool,
    /// Whether a word starting here would be in command position.
    cmd_pos: bool,
}

impl CaseScan {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            word: Str::new(),
            word_pure: true,
            word_pat_start: false,
            cmd_pos: true,
        }
    }

    /// Whether no word is being read, so the next character starts one.
    fn between_words(&self) -> bool {
        self.word.is_empty() && self.word_pure
    }

    /// About to read the first character of a word.
    fn begin_word(&mut self) {
        if self.between_words() {
            self.word_pat_start = self.frames.last().is_some_and(|f| f.pat_start);
        }
    }

    /// A character of the word being read.
    fn push(&mut self, c: char) {
        self.begin_word();
        push1(&mut self.word, c);
        self.pattern_seen();
    }

    /// A quoted or substituted span, which is part of the word being read but
    /// cannot be part of a reserved one.
    fn push_quoted(&mut self) {
        self.begin_word();
        self.word_pure = false;
        self.pattern_seen();
    }

    /// Note that the current pattern is no longer empty, so a `(` from here on
    /// is a group and not the pattern's optional open.
    fn pattern_seen(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            f.pat_start = false;
        }
    }

    /// The word being read has ended (a delimiter was reached). `depth` is the
    /// scan's current parenthesis depth, which a `case` found here is recorded
    /// at.
    fn finish_word(&mut self, depth: usize) {
        if self.between_words() {
            return;
        }
        let word = if self.word_pure {
            core::mem::take(&mut self.word)
        } else {
            Str::new()
        };
        self.word.clear();
        self.word_pure = true;
        let cmd_pos = self.cmd_pos;
        let pat_first = self.word_pat_start;
        self.word_pat_start = false;
        // A word ends a command position unless it is one of the reserved words
        // a command follows.
        self.cmd_pos = matches!(
            word.as_slice(),
            b"if" | b"then"
                | b"elif"
                | b"else"
                | b"while"
                | b"until"
                | b"do"
                | b"{"
                | b"!"
                | b"time"
                | b"coproc"
        );
        if let Some(f) = self.frames.last_mut() {
            match f.phase {
                CasePhase::AwaitIn if word.as_slice() == b"in" => {
                    f.phase = CasePhase::Pattern;
                    f.pat_start = true;
                    return;
                }
                // `esac` is reserved wherever a pattern could start, which is why
                // `case esac in …` is a syntax error in bash rather than a match
                // against the word `esac`. An empty `case x in esac` ends here.
                CasePhase::Pattern if pat_first && word.as_slice() == b"esac" => {
                    self.frames.pop();
                    return;
                }
                CasePhase::Body if cmd_pos && word.as_slice() == b"esac" => {
                    self.frames.pop();
                    return;
                }
                _ => {}
            }
        }
        if word.as_slice() == b"case" && cmd_pos {
            self.frames.push(CaseFrame {
                depth,
                phase: CasePhase::AwaitIn,
                pat_start: false,
            });
        }
    }

    /// A delimiter that ends a command, so the next word is in command position.
    fn command_end(&mut self) {
        self.cmd_pos = true;
    }

    /// A redirection operator: the word after it names a file, never a command.
    fn redirect(&mut self) {
        self.cmd_pos = false;
    }

    /// A `;` was reached, `next` being the character after it: `;;`, `;&` and
    /// `;;&` all end a clause body and start the next pattern list.
    fn semi(&mut self, next: Option<char>) {
        self.cmd_pos = true;
        if matches!(next, Some(';' | '&'))
            && let Some(f) = self.frames.last_mut()
            && f.phase == CasePhase::Body
        {
            f.phase = CasePhase::Pattern;
            f.pat_start = true;
        }
    }

    /// Whether an `(` at `depth` is a pattern's optional open, which takes no
    /// depth of its own because the pattern's `)` is what closes it.
    fn is_pattern_open(&mut self, depth: usize) -> bool {
        let open = self.frames.last().is_some_and(|f| {
            f.phase == CasePhase::Pattern && f.pat_start && f.depth == depth
        });
        if open {
            self.pattern_seen();
        }
        open
    }

    /// Whether a `)` at `depth` terminates a pattern rather than closing a
    /// group. Consumes the terminator when it does.
    fn take_pattern_close(&mut self, depth: usize) -> bool {
        let Some(f) = self.frames.last_mut() else {
            return false;
        };
        if f.phase != CasePhase::Pattern || f.depth != depth {
            return false;
        }
        f.phase = CasePhase::Body;
        f.pat_start = false;
        self.cmd_pos = true;
        true
    }

    /// Whether a `case` is still open where the substitution closes. bash has no
    /// separate diagnostic for this — its parser simply meets the `)` where it
    /// wanted `;;` or `esac` and names it.
    fn open_at_close(&self) -> bool {
        !self.frames.is_empty()
    }
}

impl Lexer {
    fn new(src: BStr<'_>, opts: ParseOpts) -> Self {
        Self {
            chars: bytes::chars(src).collect(),
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
            defer_heredocs: false,
            conts: Vec::new(),
            warnings: Vec::new(),
            hd_ahead: None,
            next_tok_index: 0,
            heredoc_lines: Vec::new(),
            heredocs_forgotten: false,
        }
    }

    /// As [`Lexer::new`], but an unterminated here-document is an error rather
    /// than leniently accepted. See [`tokenize_spanned_strict`].
    fn strict_heredoc(src: BStr<'_>, opts: ParseOpts) -> Self {
        Self { strict_heredoc_eof: true, ..Self::new(src, opts) }
    }

    /// As [`Lexer::new`], but the stream is closed with the `)` that ends the
    /// enclosing substitution rather than with an implicit newline. See
    /// [`tokenize_paren_body`].
    fn paren_body(src: BStr<'_>, opts: ParseOpts) -> Self {
        Self { paren_body: true, ..Self::new(src, opts) }
    }

    /// As [`Lexer::new`], but no newline collects a here-document body. See
    /// [`tokenize_alias_body`].
    fn alias_body(src: BStr<'_>, opts: ParseOpts) -> Self {
        Self { defer_heredocs: true, ..Self::new(src, opts) }
    }

    /// A lexer poised to collect here-document bodies out of the middle of
    /// `chars`, for [`gather_heredocs_at`].
    fn at_offset(chars: &[Ch], from: usize, opts: ParseOpts) -> Self {
        let mut lx = Self { chars: chars.to_vec(), ..Self::new(b"", opts) };
        let from = from.min(lx.chars.len());
        lx.pos = from;
        lx.iter_start = from;
        // `cur_line` counts the newlines since `iter_start`, so the base has to
        // be the physical line the cursor already stands on.
        let before = lx
            .chars
            .get(..from)
            .unwrap_or(&[])
            .iter()
            .filter(|&&c| c == '\n')
            .count();
        lx.line = 1u32.saturating_add(u32::try_from(before).unwrap_or(u32::MAX));
        lx
    }
}

/// Shell source with its NUL bytes removed — what bash's reader hands the lexer.
///
/// `shell_getc` throws a NUL away as it reads it, so one never reaches a token:
/// `echo a<NUL>b` prints `ab`, and a line that is nothing but a NUL is a blank
/// line. Because it is the *reader* that does this, it holds for every way
/// source arrives — a script file, `-c`, `eval`, `.`/`source`, a trap body, a
/// piped REPL, a `$( … )` re-read — which is why this is one function rather
/// than a rule each of those has to remember.
///
/// Source with no NUL, which is all real source, is borrowed through untouched.
/// A caller that keeps byte offsets into the text must tokenize *this* text
/// rather than the original, or the two will disagree about where a token
/// starts.
#[must_use]
pub fn strip_nuls(src: BStr<'_>) -> std::borrow::Cow<'_, [u8]> {
    if src.contains(&0) {
        std::borrow::Cow::Owned(src.iter().copied().filter(|&b| b != 0).collect())
    } else {
        std::borrow::Cow::Borrowed(src)
    }
}

/// Tokenize `src` into a token stream.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize(src: BStr<'_>, opts: ParseOpts) -> Result<Vec<Tok>, LexError> {
    tokenize_spanned(src, opts).map(|s| s.toks)
}

/// A completed tokenization: the token stream with the parallel vectors a parser
/// needs to talk about where each token came from.
pub struct Spanned {
    pub toks: Vec<Tok>,
    /// Parallel to `toks`: the 1-based source line each token *ends* on. The
    /// parser stamps these onto items for `$LINENO` and error diagnostics;
    /// unlike counting `Newline` tokens, this stays correct across newlines
    /// swallowed inside quoted strings, here-document bodies, and command
    /// substitutions.
    pub lines: Vec<u32>,
    /// Parallel to `toks`: the character offset into `src` of the start of the
    /// lexer iteration each token came out of — so for the first token of an
    /// iteration its own first character, and for any further ones an offset at
    /// or before theirs.
    ///
    /// Only a caller that needs to know which *region* of the text a token was
    /// read from wants this; the alias pass uses it to tell a token lexed wholly
    /// out of the calling line from one that began in a spliced-in alias value.
    /// For that the conservative "at or before" is exactly right: a token whose
    /// start falls at or after the splice point began after it.
    pub starts: Vec<u32>,
    /// Parallel to `toks`: the character offset into `src` just past each
    /// token's last character, as [`Tokenized::ends`]. A syntax error needs it
    /// because bash names the error site by slicing its *input line*, not by
    /// printing the token — see [`crate::parser`]'s `Spans`.
    pub ends: Vec<u32>,
}

/// Tokenize `src`, keeping each token's source line and end offset.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize_spanned(src: BStr<'_>, opts: ParseOpts) -> Result<Spanned, LexError> {
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
pub fn tokenize_paren_body(src: BStr<'_>, opts: ParseOpts) -> Result<Spanned, LexError> {
    let mut lx = Lexer::paren_body(src, opts);
    lx.run()
}

/// Tokenize an alias replacement, leaving any here-document it declares
/// *ungathered* — its `HereDoc` token is emitted with an empty body for the
/// caller to fill in from the real input.
///
/// bash expands an alias with `push_string`, which makes the replacement the
/// current `shell_input_line`; the lexer reads it, and a `<<` in it goes on the
/// same `redir_stack` as one written on the line itself. But the *body* is read
/// by `make_here_document` → `read_secondary_line` → `read_a_line`, and
/// `read_a_line` takes its characters from `yy_getc` — `bash_input.getter`, the
/// underlying file or string — never from `shell_input_line`. So the pushed
/// replacement is not a place a body can come from: the body is the next line of
/// the real input, and a body written inside the alias value is read back as
/// ordinary commands. See
/// [`crate::parser::IncrementalParser::gather_alias_heredocs`], which does the
/// filling.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize_alias_body(src: BStr<'_>, opts: ParseOpts) -> Result<Spanned, LexError> {
    Lexer::alias_body(src, opts).run()
}

/// One here-document [`gather_heredocs_at`] is to collect a body for: everything
/// the operator settled and the delimiter word decided.
#[derive(Clone, Debug)]
pub struct HeredocWant {
    /// The delimiter, already unquoted.
    pub delim: Str,
    /// `<<-`: strip leading tabs from each body line and from the delimiter.
    pub strip: bool,
    /// The delimiter was written unquoted, so the body expands.
    pub expand: bool,
}

/// What [`gather_heredocs_at`] collected.
pub struct GatheredHeredocs {
    /// The filled `HereDoc` tokens, one per want, in the order given.
    pub toks: Vec<Tok>,
    /// Offset into the input just past the last body line taken.
    pub end: usize,
    /// Parallel to `toks`: the input lines each collection consumed, as
    /// [`Tokenized::heredoc_lines`] records them.
    pub lines: Vec<u32>,
    /// The `delimited by end-of-file` warnings the collection raised. Their
    /// `tok_index` is the want's index; the caller re-keys them.
    pub warnings: Vec<ReaderWarning>,
    /// Offsets of the `\` of every `\<newline>` a body joined away, as
    /// [`Tokenized::conts`].
    pub conts: Vec<u32>,
}

/// Collect here-document bodies for already-known delimiters out of `chars`,
/// starting at `from`.
///
/// This is the collection bash's reader performs at a newline, exposed for the
/// one caller that cannot get it from the ordinary scan: a `<<` that arrived
/// through an alias, whose body is in the *outer* text and so is not the
/// sub-lexer's to read. Collection is sequential — each body starts where the
/// previous one stopped — so passing the whole line's here-documents in
/// declaration order reproduces bash's `redir_stack` order exactly.
///
/// The line numbers the warnings carry are this input's own, unmapped.
///
/// # Errors
/// Returns [`LexError`] if a body's expansion contains an unclosed construct.
pub fn gather_heredocs_at(
    chars: &[Ch],
    from: usize,
    wants: &[HeredocWant],
    opts: ParseOpts,
) -> Result<GatheredHeredocs, LexError> {
    let mut lx = Lexer::at_offset(chars, from, opts);
    let mut toks: Vec<Tok> = wants
        .iter()
        .map(|w| Tok::HereDoc(Vec::new(), w.delim.clone(), !w.expand))
        .collect();
    lx.pending_heredocs = wants
        .iter()
        .enumerate()
        .map(|(i, w)| PendingHeredoc {
            delim: w.delim.clone(),
            strip: w.strip,
            expand: w.expand,
            tok_index: i,
        })
        .collect();
    lx.collect_heredocs(&mut toks)?;
    let mut lines = vec![0u32; toks.len()];
    for (i, n) in std::mem::take(&mut lx.heredoc_lines) {
        if let Some(slot) = lines.get_mut(i) {
            *slot = n;
        }
    }
    Ok(GatheredHeredocs {
        toks,
        end: lx.pos,
        lines,
        warnings: std::mem::take(&mut lx.warnings),
        conts: std::mem::take(&mut lx.conts),
    })
}

/// What [`read_delim_at`] found where a here-document delimiter was expected.
enum DelimAt {
    /// A word: its text, whether the body still expands, and the offset just
    /// past its last character.
    Word(Str, bool, usize),
    /// A separator, or a comment — there is no word here, and because the reader
    /// got that answer *in this text* it does not go looking in the next one
    /// out. bash's grammar error.
    None,
    /// The text ran out first. bash's reader would `pop_string` and carry on in
    /// whatever text is one level further out, so the caller must try there.
    Exhausted,
}

/// Read a here-document delimiter out of `chars` starting at `from`, the way
/// [`Lexer::lex_heredoc_op`] reads one that follows its `<<` in the same text.
///
/// This is for the seam an alias leaves. bash reads the delimiter with the same
/// reader that read the `<<`, and when the pushed replacement runs dry
/// `pop_string` restores the calling line *at the character after the alias
/// word* and the scan simply continues into it — so `alias B='cat <<'` followed
/// by `B E` is one operator-and-delimiter pair spanning two texts. See
/// [`expand_aliases_inner`], the only caller.
///
/// Continuation offsets are not recorded: the text this reads was lexed already,
/// and recorded them then.
fn read_delim_at(chars: &[Ch], from: usize, opts: ParseOpts) -> Result<DelimAt, LexError> {
    let mut lx = Lexer::at_offset(chars, from, opts);
    // The blanks and continuations `lex_heredoc_op` skips before the word.
    loop {
        while matches!(lx.peek(), Some(' ' | '\t')) {
            lx.pos += 1;
        }
        if !lx.skip_conts(false) {
            break;
        }
    }
    if lx.peek().is_none() {
        return Ok(DelimAt::Exhausted);
    }
    let (delim, expand) = lx.read_heredoc_delim(false)?;
    if delim.is_empty() && expand {
        return Ok(DelimAt::None);
    }
    Ok(DelimAt::Word(delim, expand, lx.pos))
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
    /// Parallel to `toks`: for a `HereDoc` token, the number of input lines
    /// collecting its body consumed; 0 for every other token.
    ///
    /// bash gathers a pending here-document from *three* places, not one. The
    /// obvious one is the newline token, which is the only one this lexer can
    /// model — collection happens where the body sits in the input, so a lexer
    /// that reads the whole source up front has no reduction to hang the other
    /// two off. They are `gather_here_documents()` in the yacc actions of
    /// `simple_list` (parse.y:1217, with the `&`/`;` variants at 1235/1250) and
    /// `compound_list` (1148), and the first of them fires on a token that is
    /// merely the *lookahead* of a top-level list reduction — before that token
    /// is found to be a syntax error. So the body is read first and the error is
    /// blamed on the line the reader has been moved to, which is what this
    /// number lets [`crate::parser::IncrementalParser`] add back.
    pub heredoc_lines: Vec<u32>,
    /// Every warning the reader raised, in the order it raised them. The reader
    /// ([`crate::parser::IncrementalParser`]) holds them until the parse unit
    /// owning each `tok_index` is handed out, since bash's warning comes from its
    /// *reader* and so lands after the output of every earlier line.
    pub warnings: Vec<ReaderWarning>,
    /// Here-documents still pending when the scan died inside an unclosed
    /// construct, in declaration order — see [`UngatheredHeredoc`]. Their
    /// introducing line is cut from `toks` (a `<<` whose placeholder was never
    /// filled in must not reach the parser), so this is the only trace of them,
    /// and the reader needs it to reproduce bash's post-error warning.
    pub ungathered: Vec<UngatheredHeredoc>,
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
pub fn tokenize_deferred(src: BStr<'_>, opts: ParseOpts) -> Tokenized {
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
    let warnings = std::mem::take(&mut lx.warnings);
    let mut heredoc_lines = vec![0u32; toks.len()];
    for (i, n) in std::mem::take(&mut lx.heredoc_lines) {
        if let Some(slot) = heredoc_lines.get_mut(i) {
            *slot = n;
        }
    }
    let Err(e) = res else {
        return Tokenized {
            toks,
            lines,
            offsets,
            ends,
            conts,
            heredoc_lines,
            warnings,
            ungathered: Vec::new(),
            err: None,
        };
    };
    // Record the here-documents the scan never got to collect, before the cut
    // below removes the tokens that name them. Both numbers in bash's warning are
    // the end of the input here: the reader ran to EOF looking for the unclosed
    // construct's close, so `line_number` was already there when the reduction
    // finally gathered.
    let eof_line = lx.eof_line();
    let ungathered: Vec<UngatheredHeredoc> = if lx.heredocs_forgotten {
        Vec::new()
    } else {
        lx.pending_heredocs
            .iter()
            .map(|h| UngatheredHeredoc {
                delim: h.delim.clone(),
                line: eof_line,
                op_offset: offsets.get(h.tok_index).copied().unwrap_or(0),
            })
            .collect()
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
    heredoc_lines.truncate(keep);
    // The continuations are keyed by source offset rather than by token index, so
    // the ones past the cut simply describe text no caller will slice; leaving
    // them costs nothing and keeps the list a faithful record of the whole scan.
    // A here-document that reached EOF cannot coexist with a deferred lexer error
    // — the scan would have had to run past the unclosed construct to get there —
    // but if one ever did, its token is beyond the cut and so names input that
    // never runs. The reader's `tok_index` gate keeps it quiet on its own, which is
    // the same rule that keeps bash quiet after `echo one )`.
    Tokenized {
        toks,
        lines,
        offsets,
        ends,
        conts,
        heredoc_lines,
        warnings,
        ungathered,
        err: Some((e, line)),
    }
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
pub fn open_quote(src: BStr<'_>, opts: ParseOpts) -> Option<char> {
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
pub fn ends_in_continuation(src: BStr<'_>, opts: ParseOpts) -> bool {
    let chars: Vec<Ch> = bytes::chars(src).collect();
    // The `\` must be the last character before the final newline.
    let Some(nl) = chars.len().checked_sub(1) else {
        return false;
    };
    if chars.get(nl).copied() != Some(Ch::U('\n')) {
        return false;
    }
    // Immediately before it — a `\<CR><LF>` is *not* a continuation, because the
    // `\` escapes the CR and the newline then ends the line. Verified against
    // bash: a CRLF script's `echo x \` prints `x \r` and does not join.
    let Some(bs) = nl.checked_sub(1) else {
        return false;
    };
    if chars.get(bs).copied() != Some(Ch::U('\\')) {
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
pub fn tokenize_spanned_strict(src: BStr<'_>, opts: ParseOpts) -> Result<Spanned, LexError> {
    let mut lx = Lexer::strict_heredoc(src, opts);
    lx.run()
}

/// What a verbatim word's quotes and backslashes mean. The three contexts that
/// read a word out of already-scanned source agree on everything else — `$…`,
/// `` `…` `` and a nested `"…"` are live in all of them, and no character is an
/// operator — and differ only here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verbatim {
    /// A pattern (`${var#pat}`, `${var/pat/…}`) or a subscript: written bare, so
    /// `'…'` quotes and every backslash escapes the character after it.
    Bare,
    /// The *replacement* of `${var/pat/repl}`: as [`Verbatim::Bare`], except that
    /// `\&` and `\\` keep their backslash for the later `&`-scan.
    Replacement,
    /// The operand of a substitution that is itself inside `"…"` — the `w` of
    /// `"${x:-w}"`. bash reads that operand with the *enclosing* quoting still in
    /// force, so a `'` is an ordinary character and only the characters
    /// double-quoting leaves live can be escaped.
    Dquote,
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
pub fn lex_word_verbatim(src: BStr<'_>) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.read_word_verbatim(Verbatim::Bare)
}

/// Lex `src` as if it were the body of a double-quoted string that runs to the
/// end of the input — for the strings bash expands in `Q_DOUBLE_QUOTES` context
/// without any quotes actually delimiting them (`PS4` before each `set -x`
/// line, and `${x@P}`). `$…`, `` `…` `` and the double-quote backslash escapes
/// are live; a `"`, a `'` and any other backslash are literal.
///
/// # Errors
/// Returns [`LexError`] on an unterminated substitution.
pub fn lex_dquote_body(src: BStr<'_>) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.read_double_quote_until(false)
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
pub fn lex_replacement_verbatim(src: BStr<'_>) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.read_word_verbatim(Verbatim::Replacement)
}

/// Lex `src` as the operand of a substitution written inside double quotes — the
/// `w` of `"${x:-w}"`. The quotes around the substitution are still in force
/// inside it, so `'a b'` is three literal characters and a space, and a
/// backslash escapes only `$`, `` ` ``, `"`, `\` and `}` (and a newline, which is
/// a line continuation); before anything else it stays a literal backslash.
///
/// This is not [`lex_dquote_body`]: a real double-quoted body would leave
/// `$'…'` and `$"…"` alone, but an operand still expands them, because it is a
/// *word* being read — the quoting only says how its characters are spelled.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn lex_operand_in_dquote(src: BStr<'_>) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.read_word_verbatim(Verbatim::Dquote)
}

/// Reserved words after which a new command begins, so a word following one of
/// them is in "command position". bash's `reserved_word_acceptable` list, less
/// `time` (which is [`Prev::Time`], because its own `-p`/`--` extend it) and
/// less the punctuation, which is matched as operators.
///
/// `}`, `done`, `esac` and `fi` are in the list even though the grammar always
/// wants a separator after them; bash lists them, and a word can never actually
/// follow one without a syntax error, so they cost nothing either way.
const CMD_INTRODUCERS: &[&[u8]] = &[
    b"if", b"then", b"elif", b"else", b"fi", b"while", b"until", b"do", b"done", b"esac", b"{",
    b"}", b"!",
];

/// What the previously kept token was, to the precision the command-position
/// test needs. This is osh's reading of bash's `last_read_token`, which is a
/// *parser* token — so a word's classification depends on where it sat, and the
/// state has to be carried rather than read back off the output stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    /// Nothing yet, or a separator: a command begins here.
    Start,
    /// `;;`, `;&` or `;;&`. A reserved word would be accepted here, but what
    /// actually follows is the *next `case` arm's pattern* — which is why bash
    /// excludes these three from `command_token_position` alone.
    CaseArmEnd,
    /// A reserved word from [`CMD_INTRODUCERS`].
    Introducer,
    /// The `time` reserved word, or one of the `-p`/`--` that belong to it.
    Time,
    /// An assignment word (`n=v`, `n+=v`, `n=(…)`) in a position where one was
    /// accepted. A command word may still follow it.
    Assignment,
    /// A redirection operator: what follows is its target, never a command.
    RedirOp,
    /// The word that was a redirection's target.
    RedirTarget,
    /// An ordinary word, or anything else after which no command begins.
    Other,
}

/// Which text a token was read from, and where in it the token ends.
///
/// An alias pass produces tokens from several texts at once, because bash reads
/// an alias by *pushing its replacement onto the input*: `push_string` (parse.y)
/// sets `shell_input_line` to the replacement outright, and `pop_string` puts
/// the old line back when the reader passes its end. So while the reader is
/// inside a replacement, "the current input line" — the thing every `syntax
/// error near …` slice and every echoed source line comes from — *is* the
/// replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokSpan {
    /// Which text this token was read from. `0` is the text the alias pass was
    /// handed; `n + 1` is `AliasExpansion::bodies[n]`.
    pub src: u32,
    /// The character offset into that text just past the token's last
    /// character, or [`u32::MAX`] for a token with no text of its own.
    pub end: u32,
}

impl TokSpan {
    /// The same span against a source table whose replacements have been
    /// renumbered `by` places further along. `0` is the shell's own input in
    /// every table, so it never moves.
    #[must_use]
    pub fn shifted(self, by: u32) -> Self {
        if self.src == 0 {
            self
        } else {
            Self { src: self.src.saturating_add(by), ..self }
        }
    }
}

/// An alias replacement that was spliced into a token stream, and where the
/// reader returns when it runs off the end of it.
#[derive(Clone, Debug)]
pub struct AliasBody {
    /// The replacement text, exactly as it was tokenized.
    pub text: Str,
    /// bash's `pop_string`: the saved `shell_input_line` and
    /// `shell_input_line_index` this push displaced — that is, the text the
    /// alias word itself was written in and the offset just past that word.
    pub parent: TokSpan,
}

/// What [`expand_aliases_tracked`] hands back: the expanded stream plus enough
/// bookkeeping to resume the parse and to blame the right text for an error.
pub struct AliasExpansion {
    pub toks: Vec<Tok>,
    /// Parallel to `toks`: the 1-based source line of each token. A replacement's
    /// tokens all inherit the alias word's line, as bash's do.
    pub lines: Vec<u32>,
    /// Parallel to `toks`: the index of the input token each came from, or
    /// `None` when an alias's replacement text spliced it in.
    pub origin: Vec<Option<usize>>,
    /// Parallel to `toks`: the text each token was read from. See [`TokSpan`].
    pub spans: Vec<TokSpan>,
    /// The replacement texts that were pushed, in the order they were pushed.
    pub bodies: Vec<AliasBody>,
    /// Indices into `toks` of every `HereDoc` placeholder a replacement text
    /// spliced in, ascending. Their bodies are still to come: they are in the
    /// *outer* input, which the sub-lex could not read. See
    /// [`tokenize_alias_body`].
    pub heredocs: Vec<usize>,
}

/// The output of an alias pass, plus the running state its command-position
/// test needs. The four vectors are parallel and always pushed together.
struct AliasOut {
    toks: Vec<Tok>,
    lines: Vec<u32>,
    /// For each output token, the index of the input token it came from, or
    /// `None` when an alias's replacement text spliced it in.
    origin: Vec<Option<usize>>,
    /// For each output token, the text it was read from. See [`TokSpan`].
    spans: Vec<TokSpan>,
    /// The replacement texts pushed so far; a token's [`TokSpan::src`] of
    /// `n + 1` names `bodies[n]`.
    bodies: Vec<AliasBody>,
    /// [`AliasExpansion::heredocs`], filled as the replacements are walked.
    heredocs: Vec<usize>,
    prev: Prev,
    /// bash's `PST_REDIRLIST`: everything read of the simple command so far has
    /// been a redirection, so the word after one is still the *command* word.
    /// Reading any other word clears it, which is why `>f c` alias-expands `c`
    /// but `x=1 >f c` does not.
    redir_list: bool,
}

impl AliasOut {
    fn new() -> Self {
        Self {
            toks: Vec::new(),
            lines: Vec::new(),
            origin: Vec::new(),
            spans: Vec::new(),
            bodies: Vec::new(),
            heredocs: Vec::new(),
            prev: Prev::Start,
            redir_list: true,
        }
    }

    /// True when a reserved word would be recognised here — bash's
    /// `reserved_word_acceptable`. It is *not* the same question as command
    /// position: `x=1 if` is a command named `if`, and `case x in y) …` accepts
    /// a pattern rather than a reserved word after `;;`.
    fn reserved_ok(&self) -> bool {
        matches!(
            self.prev,
            Prev::Start | Prev::CaseArmEnd | Prev::Introducer | Prev::Time
        )
    }

    /// True when a word here is the command word of a simple command, and so is
    /// a candidate for alias expansion — bash's `command_token_position`.
    fn at_command(&self) -> bool {
        match self.prev {
            // A reserved word is accepted after `;;`, but a *command* is not.
            Prev::CaseArmEnd => false,
            // Assignments precede the command word: `x=1 c` expands `c`.
            Prev::Assignment => true,
            // Only while nothing but redirections has been read.
            Prev::RedirTarget => self.redir_list,
            _ => self.reserved_ok(),
        }
    }

    /// Record `tok`, which has just been pushed, as the new previous token.
    /// `was_cmd` is what [`Self::at_command`] said about it before the push —
    /// a word is only a reserved word, or an assignment, where one was allowed.
    fn advance(&mut self, tok: &Tok, was_cmd: bool) {
        let reserved_ok = self.reserved_ok();
        self.prev = match tok {
            Tok::Newline => Prev::Start,
            Tok::Op(
                Op::Pipe
                | Op::PipeAmp
                | Op::AndIf
                | Op::OrIf
                | Op::Amp
                | Op::Semi
                | Op::LParen
                // A `)` closes a `case` pattern, so the arm's body starts here.
                | Op::RParen,
            ) => Prev::Start,
            Tok::Op(Op::DSemi | Op::SemiAmp | Op::DSemiAmp) => Prev::CaseArmEnd,
            // A redirection, or one of the two prefixes that introduce one: an
            // io number (`2>&1`) or a varfd (`{v}>f`). The prefixes are part of
            // the redirection, so they must not end a run of them — `2>&1 c`
            // alias-expands `c` exactly as `>f c` does.
            Tok::Op(_) | Tok::Io(_) | Tok::VarFd(_) => Prev::RedirOp,
            Tok::ArrayAssign { .. } if was_cmd => Prev::Assignment,
            Tok::Word(_) if self.prev == Prev::RedirOp => Prev::RedirTarget,
            Tok::Word(segs) => match segs.as_slice() {
                [Seg::Lit(w)] if reserved_ok && w.as_slice() == b"time" => Prev::Time,
                // `time`'s own options, which bash lexes as TIMEOPT/TIMEIGN and
                // accepts a command after just as it does after `time` itself.
                [Seg::Lit(w)]
                    if self.prev == Prev::Time && matches!(w.as_slice(), b"-p" | b"--") =>
                {
                    Prev::Time
                }
                [Seg::Lit(w)] if reserved_ok && CMD_INTRODUCERS.contains(&w.as_slice()) => {
                    Prev::Introducer
                }
                _ if was_cmd && word_is_assignment(segs) => Prev::Assignment,
                _ => Prev::Other,
            },
            // The body arrives after the delimiter word, and is the tail of the
            // same redirection — so it leaves the run of them running.
            Tok::HereDoc(..) => Prev::RedirTarget,
            // An arithmetic command, an array assignment out of position, …
            _ => Prev::Other,
        };
        // A new command can begin here, so its redirections start over; reading
        // an actual word of one ends the run.
        self.redir_list = match self.prev {
            Prev::Start | Prev::CaseArmEnd | Prev::Introducer | Prev::Time => true,
            Prev::RedirOp | Prev::RedirTarget => self.redir_list,
            Prev::Assignment | Prev::Other => false,
        };
    }
}

/// A coarse "does a command begin after `prev`?" test, from the previous token
/// alone. The *lexer* needs one while tokenizing, where none of the parser state
/// [`AliasOut`] carries exists yet, and its one caller only wants to know
/// whether to slurp an unquoted-space array subscript. The alias pass uses
/// [`AliasOut::at_command`] instead, which is bash's real test.
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
            matches!(segs.as_slice(), [Seg::Lit(w)] if CMD_INTRODUCERS.contains(&w.as_slice()))
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
///
/// `ends` is parallel to `toks`: the offset into the text they were lexed from
/// just past each token. It is carried through so that a diagnostic raised while
/// the reader is inside a replacement can quote *that* text — see [`TokSpan`].
/// Pass an empty slice where no such text exists.
///
/// Beside the expanded stream comes an *origin* vector: for each output token,
/// the index of the input token it came from, or `None` when the token was
/// spliced in by an alias's replacement text.
/// [`crate::parser::IncrementalParser`] needs it to resume — after executing one
/// item it must know which *original* token to continue from, and must not
/// re-expand tokens an alias already produced.
#[must_use]
pub fn expand_aliases_tracked(
    toks: &[Tok],
    lines: &[u32],
    ends: &[u32],
    text: &[Ch],
    aliases: &crate::assoc::AssocArray,
    opts: ParseOpts,
) -> AliasExpansion {
    let mut active = std::collections::BTreeSet::new();
    let mut out = AliasOut::new();
    let input = AliasInput { toks, lines, ends, text, src: 0 };
    expand_aliases_inner(&input, aliases, opts, &mut active, &mut out, true);
    AliasExpansion {
        toks: out.toks,
        lines: out.lines,
        origin: out.origin,
        spans: out.spans,
        bodies: out.bodies,
        heredocs: out.heredocs,
    }
}

/// One text's worth of tokens for the alias pass to walk: the stream, the line
/// and end offset of each token, and which text those offsets index. The pass
/// recurses on this — a replacement is just another text to read.
struct AliasInput<'a> {
    toks: &'a [Tok],
    lines: &'a [u32],
    /// Parallel to `toks`: where each token ends in the text named by `src`.
    /// Empty when that text is unknown, which makes every token here
    /// unsliceable.
    ends: &'a [u32],
    /// The characters `ends` indexes: the text these tokens were read from.
    /// Empty when it is unknown. [`read_delim_at`] reads out of it, at the seam
    /// where a replacement ran dry and bash's reader would have carried on here.
    text: &'a [Ch],
    /// Which text these were read from, as a [`TokSpan::src`].
    src: u32,
}

fn expand_aliases_inner(
    inp: &AliasInput<'_>,
    aliases: &crate::assoc::AssocArray,
    opts: ParseOpts,
    active: &mut std::collections::BTreeSet<Str>,
    out: &mut AliasOut,
    // False inside an alias's replacement text: those tokens have no counterpart
    // in the caller's stream, so they record origin `None`.
    from_input: bool,
) -> bool {
    let &AliasInput { toks, lines, ends, text, src } = inp;
    let span_of = |i: usize| TokSpan {
        src,
        end: ends.get(i).copied().unwrap_or(u32::MAX),
    };
    // Whether the *next* token must be treated as command position regardless of
    // structure (carried across an alias whose value ended in a blank).
    let mut force = false;
    // Tokens up to here were read as some here-document's delimiter and are not
    // the pass's to emit again. See `take_dangling_delim`.
    let mut skip_until = 0usize;
    // The last dangling `<<` this text was asked about got its answer *here* — a
    // separator or a comment stands where the word would be — so the chain of
    // `pop_string`s stops and the caller must not be told to keep looking.
    let mut sealed = false;
    let entry = out.toks.len();
    for (i, tok) in toks.iter().enumerate() {
        if i < skip_until {
            continue;
        }
        // The source line of this token; expanded replacement tokens inherit it
        // so post-alias line numbers stay anchored to the alias's call site.
        let tok_line = lines.get(i).copied().unwrap_or(1);
        let at_cmd = force || out.at_command();
        force = false;
        if at_cmd
            && let Tok::Word(segs) = tok
            && let [Seg::Lit(name)] = segs.as_slice()
            && !active.contains(name)
            && let Some(val) = aliases.get(name)
            && let Ok(Spanned { toks: mut repl, ends: mut repl_ends, .. }) =
                tokenize_alias_body(val, opts)
        {
            // Drop a trailing newline the lexer may append so the splice stays
            // within the current command.
            while matches!(repl.last(), Some(Tok::Newline)) {
                repl.pop();
                repl_ends.pop();
            }
            // The lex above numbered the replacement's lines from 1, and a
            // `$( … )` inside it recorded one of those numbers. It is not a line
            // of the script, so it becomes the alias word's like everything else
            // the replacement produces.
            for t in &mut repl {
                reline_tok(t, tok_line);
            }
            // Replacement tokens all inherit the alias word's source line.
            let repl_lines = vec![tok_line; repl.len()];
            let mark = out.toks.len();
            // bash's `push_string`: the replacement becomes the current input
            // line, and the line it displaced — with the reader's index just
            // past the alias word — is stacked for `pop_string` to restore.
            out.bodies.push(AliasBody { text: val.to_vec(), parent: span_of(i) });
            let body_src = u32::try_from(out.bodies.len()).unwrap_or(u32::MAX);
            active.insert(name.clone());
            let repl_text: Vec<Ch> = bytes::chars(val).collect();
            let body = AliasInput {
                toks: &repl,
                lines: &repl_lines,
                ends: &repl_ends,
                text: &repl_text,
                src: body_src,
            };
            let dangling = expand_aliases_inner(&body, aliases, opts, active, out, false);
            active.remove(name);
            // The replacement ended *at* a `<<`, so the delimiter that belongs to
            // it is not in the value: bash's reader ran off the end of the pushed
            // string and `pop_string` put it back on this text, just past the
            // alias word, where the scan simply carried on. Do the same. If this
            // text is spent too, say so and let the caller — one text further
            // out — try, which is the rest of that `pop_string` chain.
            let mut took_delim = false;
            if dangling {
                let at = ends.get(i).map(|&e| e as usize);
                match at.map_or(Dangle::Spent, |end| {
                    take_dangling_delim(out, text, end, tok_line, opts)
                }) {
                    // The delimiter's characters were lexed into this text's
                    // tokens as well; they belong to the delimiter, not to words
                    // of their own. Skipping by *offset* is safe even if the two
                    // scans were to disagree about where the word ends, since a
                    // token reaching past the delimiter is never skipped.
                    Dangle::Filled(delim_end) => {
                        took_delim = true;
                        sealed = false;
                        skip_until = i.saturating_add(1);
                        while ends.get(skip_until).is_some_and(|&e| (e as usize) <= delim_end) {
                            skip_until = skip_until.saturating_add(1);
                        }
                    }
                    Dangle::Sealed => sealed = true,
                    Dangle::Spent => sealed = false,
                }
            }
            // The *first* token of the replacement stands in for the alias word
            // itself, so it keeps the alias word's origin; only the tokens after
            // it are origin-less. Without this, a caller resuming at the start of
            // the splice would skip the alias word entirely (it would find the
            // next `Some` origin *past* the replacement) and silently drop the
            // command. An empty replacement contributes no token and needs no
            // mark: resuming past it is correct, since it expands to nothing.
            if from_input
                && let Some(slot) = out.origin.get_mut(mark)
            {
                *slot = Some(i);
            }
            // A value ending in a blank makes the *next* word a candidate for
            // expansion however the structure reads (bash's `PST_ALEXPNEXT`) —
            // unless the delimiter just above already took that word, since it
            // was consumed by the same reader that would have expanded it. What
            // bash does then, and osh does not, is expand it *as the delimiter*:
            // see TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE.
            force = !took_delim && (val.ends_with(b" ") || val.ends_with(b"\t"));
            continue;
        }
        // A `<<` a replacement text spliced in leaves an empty placeholder: the
        // sub-lex had no body to read, because bash reads one from the real input
        // and not from the pushed string. Note where it landed so the reader can
        // fill it in. See [`tokenize_alias_body`].
        if !from_input && matches!(tok, Tok::HereDoc(..)) {
            out.heredocs.push(out.toks.len());
        }
        out.toks.push(tok.clone());
        out.lines.push(tok_line);
        out.origin.push(if from_input { Some(i) } else { None });
        out.spans.push(span_of(i));
        out.advance(tok, at_cmd);
    }
    // This text ended *at* a `<<` — no delimiter followed it here, and there is
    // no more text to look in. bash's reader would `pop_string` and go on
    // reading in the text one level out, so tell the caller to continue the scan
    // there. At the outermost text nobody is listening, and the bare operator is
    // then the grammar error bash reports. A `sealed` answer is not that: the
    // reader already found a separator, so it stops asking.
    !sealed
        && out.toks.len() > entry
        && matches!(out.toks.last(), Some(Tok::Op(Op::DLess | Op::DLessDash)))
}

/// What completing a dangling `<<` from the calling text came to.
enum Dangle {
    /// The delimiter was found and appended; the offset just past it.
    Filled(usize),
    /// A separator or a comment stands where the word would be, in *this* text.
    /// bash's reader has its answer, so the `pop_string` chain stops here and
    /// the bare operator is left to the parser as the grammar error it is.
    Sealed,
    /// This text is spent; the caller one level out must try.
    Spent,
}

/// Complete a `<<` that an alias replacement left without a delimiter, reading
/// the delimiter out of `text` from `end` — the character just past the alias
/// word, which is exactly where bash's `pop_string` resumes.
///
/// On success the `HereDoc` token is appended (and registered in
/// [`AliasExpansion::heredocs`], since its *body* still has to come from the
/// real input) and the offset just past the delimiter is returned.
///
/// A delimiter that fails to lex counts as [`Dangle::Sealed`]: the word does
/// start in this text, so the reader is not going to go looking in the next one
/// out — the failure belongs here, where the parser will report it.
fn take_dangling_delim(
    out: &mut AliasOut,
    text: &[Ch],
    end: usize,
    line: u32,
    opts: ParseOpts,
) -> Dangle {
    let delim_at = match read_delim_at(text, end, opts) {
        Ok(d) => d,
        Err(_) => return Dangle::Sealed,
    };
    let (delim, expand, delim_end) = match delim_at {
        DelimAt::Word(delim, expand, delim_end) => (delim, expand, delim_end),
        DelimAt::None => return Dangle::Sealed,
        DelimAt::Exhausted => return Dangle::Spent,
    };
    // The body is not this pass's to read — it is in the real input, after the
    // line the alias word stands on, like every other alias-spliced
    // here-document. See [`tokenize_alias_body`].
    out.heredocs.push(out.toks.len());
    let tok = Tok::HereDoc(Vec::new(), delim, !expand);
    out.advance(&tok, false);
    out.toks.push(tok);
    out.lines.push(line);
    // The delimiter's characters are in the calling text, but the operator they
    // complete is not, and the pair is one redirection. Recording it as
    // origin-less keeps it out of the resume scan, exactly as the operator is.
    out.origin.push(None);
    out.spans.push(TokSpan { src: 0, end: u32::MAX });
    Dangle::Filled(delim_end)
}

/// Re-anchor every source line recorded *inside* a token to `line`.
///
/// A `$( … )` remembers the line its `)` sits on and a `<( … )` the line its `(`
/// does, in the numbering of whatever text they were lexed from. An alias
/// replacement is lexed on its own and so numbers from 1 — but a replacement is
/// not a line of the script and has none of its own: bash reads one by swapping
/// `shell_input_line` for it, and `line_number` is bumped only by *fetching* a
/// line (parse.y 2346), which reading a pushed string never does. So a
/// substitution written in an alias value reports the line the alias word was
/// on, and this puts that line where the parse will find it.
///
/// The counterpart for the tokens themselves is the `repl_lines` the caller
/// builds; this is for the lines a token carries as payload, which no parallel
/// array reaches.
fn reline_tok(tok: &mut Tok, line: u32) {
    match tok {
        Tok::Word(segs) | Tok::HereDoc(segs, ..) => reline_segs(segs, line),
        Tok::ArrayAssign { elems, .. } => {
            for e in elems {
                reline_segs(e, line);
            }
        }
        _ => {}
    }
}

fn reline_segs(segs: &mut [Seg], line: u32) {
    for seg in segs {
        match seg {
            Seg::CmdSub(_, close, _) => *close = line,
            Seg::ProcSub(_, _, open) => *open = line,
            Seg::Dq(inner) => reline_segs(inner, line),
            _ => {}
        }
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// True when `s` is a syntactically valid shell variable name (an identifier).
///
/// Defined here, next to the scanner that decides what an identifier looks
/// like, and re-exported from `parser` for the rest of the crate so there is
/// exactly one answer to the question.
pub(crate) fn is_valid_name(s: BStr<'_>) -> bool {
    let mut chars = bytes::chars(s).map(syn);
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
    let b: Vec<char> = bytes::chars(s).map(syn).collect();
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
    /// The character at `i` as shell syntax. See [`syn`].
    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied().map(syn)
    }

    fn peek(&self) -> Option<char> {
        self.at(self.pos)
    }

    fn peek_at(&self, off: usize) -> Option<char> {
        self.at(self.pos + off)
    }

    /// Consume the character at the cursor, keeping its own bytes.
    fn bump_ch(&mut self) -> Option<Ch> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        // Reaching the end of a line is where a read-ahead is redeemed: the lines
        // the here-document reader already took lie behind the cursor now, and it
        // must not read them a second time as input. See [`Lexer::sync_ahead`].
        if c == Some(Ch::U('\n')) {
            self.sync_ahead();
        }
        c
    }

    /// Consume the character at the cursor and append its bytes to `out`.
    fn take_into(&mut self, out: &mut Str) {
        if let Some(c) = self.bump_ch() {
            c.push_to(out);
        }
    }

    /// The source spanning `[from, to)` as bytes.
    fn slice(&self, from: usize, to: usize) -> Str {
        bytes::from_chars(self.chars.get(from..to).unwrap_or(&[]).iter().copied())
    }

    /// Move the token cursor to wherever the here-document reader has already
    /// read, now that the cursor has finished the line it was standing on.
    ///
    /// A no-op unless a `$( … )` closed over an ungathered here-document, which
    /// is the only thing that puts the reader ahead. See [`Lexer::gather_ahead`].
    fn sync_ahead(&mut self) {
        if let Some(a) = self.hd_ahead.take() {
            self.pos = self.pos.max(a.pos);
        }
    }

    /// Record the `\<newline>` the caller just consumed and discarded.
    ///
    /// The cursor sits one past the newline, so the backslash is two back. A
    /// backslash at end of input is not a continuation and is not recorded.
    fn note_continuation(&mut self) {
        if self.at(self.pos.wrapping_sub(1)) != Some('\n') {
            return;
        }
        let Some(at) = self.pos.checked_sub(2) else {
            return;
        };
        self.conts.push(u32::try_from(at).unwrap_or(u32::MAX));
    }

    /// The length of the line continuation at `i`, if one is there: two
    /// characters for `\<newline>`, three for the `\<CR><LF>` a CRLF file writes.
    fn cont_len_at(&self, i: usize) -> Option<usize> {
        if self.at(i) != Some('\\') {
            return None;
        }
        match (self.at(i + 1), self.at(i + 2)) {
            (Some('\n'), _) => Some(2),
            (Some('\r'), Some('\n')) => Some(3),
            _ => None,
        }
    }

    /// `i`, advanced past every line continuation standing at it.
    ///
    /// The read-only half of [`Lexer::eat_conts`], for the scans that look ahead
    /// by index before deciding to move the cursor at all.
    fn cont_skip(&self, mut i: usize) -> usize {
        while let Some(n) = self.cont_len_at(i) {
            i = i.saturating_add(n);
        }
        i
    }

    /// Delete the line continuations standing at the cursor, as bash's *reader*
    /// does, and return how many newlines went with them.
    ///
    /// `shell_getc` (parse.y) throws a `\<newline>` away and reads on, so no
    /// scanner in bash ever sees one. That is not a detail of word scanning: the
    /// pair can sit between the two characters of an operator (`|\<newline>|` is
    /// `||`), between a `$` and what it introduces (`$\<newline>(` opens a
    /// substitution), or anywhere else two characters have to be adjacent. Every
    /// site that reads the next character calls this first, which is what makes
    /// those sites agree with bash.
    ///
    /// Runs are deleted together, since the reader simply keeps reading. The
    /// count is returned because `stamp_lines` derives the line from the newlines
    /// inside a token's span: a caller that eats a continuation *before* the span
    /// begins must advance `Lexer::line` itself.
    fn eat_conts(&mut self) -> usize {
        let mut n = 0usize;
        while let Some(len) = self.cont_len_at(self.pos) {
            self.conts.push(u32::try_from(self.pos).unwrap_or(u32::MAX));
            self.pos = self.pos.saturating_add(len);
            // The newline is gone, but it was still *read* — so a here-document
            // read ahead on it is redeemed exactly as `bump_ch` redeems it.
            self.sync_ahead();
            n = n.saturating_add(1);
        }
        n
    }

    /// Consume the character at the cursor and delete whatever line continuation
    /// follows it, so the next `peek` sees the character bash's scanner would.
    /// The step to take inside a multi-character operator.
    fn adv(&mut self) {
        self.pos = self.pos.saturating_add(1);
        self.eat_conts();
    }

    /// Move the cursor to `to`, recording every line continuation on the way as
    /// deleted. For the scans that settle where to land by index first — jumping
    /// the cursor straight there would leave those continuations unrecorded, and
    /// the command history reads that record (see [`Tokenized::conts`]).
    fn seek(&mut self, to: usize) {
        while self.pos < to {
            if self.eat_conts() == 0 {
                self.pos = self.pos.saturating_add(1);
            }
        }
    }

    /// Step over the line continuations at the cursor; `true` if any were there.
    ///
    /// `record` says whether they are being *deleted* or merely passed. The
    /// `$( … )` raw scan copies its source verbatim for a re-lex later, so a
    /// continuation it walks over is still present in the text it stored and
    /// must not be cut out of the command history (see [`Tokenized::conts`]) —
    /// only the top-level scan deletes for real.
    fn skip_conts(&mut self, record: bool) -> bool {
        if record {
            return self.eat_conts() != 0;
        }
        let to = self.cont_skip(self.pos);
        let moved = to != self.pos;
        self.pos = to;
        moved
    }

    /// If the cursor sits on a varfd redirect prefix `{name}` immediately
    /// followed by a redirection operator (`{fd}>`, `{fd}<`), return the name
    /// and the index just past the closing `}`. Returns `None` otherwise, so a
    /// brace group (`{ …; }`) or brace expansion (`{a,b}`) falls through to the
    /// normal word/reserved-word path. The `{` at `self.pos` is assumed.
    fn varfd_prefix(&self) -> Option<(String, usize)> {
        debug_assert_eq!(self.at(self.pos), Some('{'));
        // “Immediately followed by” is judged after the reader's deletions, so a
        // line continuation anywhere in here is simply not there: `{fd}\<newline>>f`
        // is the same prefix as `{fd}>f`. See [`Lexer::eat_conts`].
        let mut i = self.cont_skip(self.pos.saturating_add(1));
        // Name characters only, so the name is text by construction — which is
        // what makes it a shell variable the redirect can assign the fd to.
        let mut name = String::new();
        // First name char must be a name-start (letter or `_`).
        match self.at(i) {
            Some(c) if is_name_start(c) => {
                name.push(c);
                i = self.cont_skip(i.saturating_add(1));
            }
            _ => return None,
        }
        while matches!(self.at(i), Some(c) if is_name_char(c)) {
            if let Some(c) = self.at(i) {
                name.push(c);
            }
            i = self.cont_skip(i.saturating_add(1));
        }
        if self.at(i) != Some('}') {
            return None;
        }
        i = self.cont_skip(i.saturating_add(1));
        // The `}` must be immediately followed by a redirection operator for
        // this to be a varfd prefix rather than an ordinary `{word}` token.
        if !matches!(self.at(i), Some('<' | '>')) {
            return None;
        }
        Some((name, i))
    }

    fn run(&mut self) -> Result<Spanned, LexError> {
        let mut toks = Vec::new();
        // Parallel to `toks`: the 1-based source line each token *ends* on (see
        // `stamp_lines` — that is what bash's `line_number` holds once the token
        // has been read).
        let mut lines: Vec<u32> = Vec::new();
        let mut ends: Vec<u32> = Vec::new();
        let mut starts: Vec<u32> = Vec::new();
        self.run_into(&mut toks, &mut lines, &mut starts, &mut ends)?;
        Ok(Spanned { toks, lines, starts, ends })
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
            if self.peek().is_none() {
                break;
            }
            // Every token produced by this iteration starts on `start_line`;
            // `start_pos` lets us count the newlines the iteration consumes (so
            // the counter advances past newlines swallowed inside a token body).
            let start_line = self.line;
            let start_pos = self.pos;
            self.iter_start = start_pos;
            self.next_tok_index = out.len();
            // A line continuation in front of the token is not there at all —
            // bash's reader deleted it (see [`Lexer::eat_conts`]) — but it is
            // eaten *after* `start_pos` so the newline falls inside this
            // iteration's span, where `stamp_lines` counts it. Blanks can follow
            // one, and another can follow those, so alternate until neither is at
            // the cursor.
            loop {
                if self.eat_conts() == 0 {
                    break;
                }
                while matches!(self.peek(), Some(' ' | '\t')) {
                    self.pos += 1;
                }
            }
            let Some(c) = self.peek() else { break };
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
                    // Before the collection below, so a here-document still
                    // pending from earlier on the line reads from *after* the
                    // bodies a `$( … )` on it already took, not from the top of
                    // them again.
                    self.sync_ahead();
                    out.push(Tok::Newline);
                    if !self.pending_heredocs.is_empty() && !self.defer_heredocs {
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
                    self.adv();
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
                    self.adv();
                    if self.peek() == Some('&') {
                        self.pos += 1;
                        out.push(Tok::Op(Op::AndIf));
                    } else if self.peek() == Some('>') {
                        // `&>file` / `&>>file`: redirect both stdout and stderr.
                        self.adv();
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
                    self.adv();
                    if self.peek() == Some(';') {
                        self.adv();
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
                    self.adv();
                    // `((` (with no intervening space) begins an arithmetic
                    // command; `( (` (a space between) is nested subshells. So
                    // does `((` standing where no command can start — there it
                    // is a plain `(`, and the second one is read again as the
                    // next token.
                    if self.peek() == Some('(') && arith_cmd_position(out.last()) {
                        // …and so is a `((` whose two closing parentheses are
                        // not adjacent. bash's `parse_arith_cmd` tests for the
                        // second `)` with a read of its own, and on failing it
                        // hands the text back to the ordinary grammar as
                        // `( ( … ) )` — so `((echo hi) )` *runs* `echo hi`.
                        // Rewind to the second `(` and emit a plain `(`; the
                        // continuations the abandoned body scan deleted go back
                        // too, since the same text is about to be read again.
                        //
                        // A `for` header is the exception. bash tests it for
                        // the same adjacency, but through the `ARITH_FOR_EXPRS`
                        // arm, which has nothing to fall back to: `for` cannot
                        // be followed by a subshell, so a header that fails the
                        // test is simply an error.
                        let for_header = matches!(out.last(), Some(Tok::Word(segs))
                            if matches!(segs.as_slice(), [Seg::Lit(s)] if s.as_slice() == b"for"));
                        let arith_from = self.pos;
                        let conts_from = self.conts.len();
                        self.pos += 1;
                        match self.read_arith_body(true)? {
                            Some(raw) => out.push(Tok::ArithCmd(raw)),
                            None if for_header => {
                                return Err(LexError::new("malformed arithmetic expansion"));
                            }
                            None => {
                                self.conts.truncate(conts_from);
                                self.pos = arith_from;
                                out.push(Tok::Op(Op::LParen));
                            }
                        }
                    } else {
                        out.push(Tok::Op(Op::LParen));
                    }
                }
                ')' => {
                    self.pos += 1;
                    out.push(Tok::Op(Op::RParen));
                }
                '<' | '>' if self.at(self.cont_skip(self.pos + 1)) == Some('(') => {
                    // Process substitution `<(cmd)` / `>(cmd)`: a word (filename),
                    // not a redirection operator. `read_word` consumes the whole
                    // `<(…)`/`>(…)` group as a `Seg::ProcSub` (and allows adjacent
                    // literals to concatenate).
                    let segs = self.read_word(extpat)?;
                    self.emit_word(out, segs);
                }
                '<' => {
                    self.adv();
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
                            self.adv();
                            if self.peek() == Some('<') {
                                // `<<<` here-string: the target is an ordinary
                                // word parsed on this line.
                                self.pos += 1;
                                out.push(Tok::Op(Op::TLess));
                            } else {
                                self.lex_heredoc_op(out)?;
                            }
                        }
                        _ => out.push(Tok::Op(Op::Less)),
                    }
                }
                '>' => {
                    self.adv();
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
                        self.seek(end);
                        out.push(Tok::VarFd(name));
                    }
                }
                '0'..='9' => {
                    // Possibly an IO number (digits directly before < or >).
                    // “Directly” is again judged after the reader's deletions, so
                    // the digits are gathered rather than sliced: a line
                    // continuation may sit between two of them, or between the
                    // last one and the operator. See [`Lexer::eat_conts`].
                    let mut i = self.pos;
                    let mut digits = Str::new();
                    while let Some(d @ '0'..='9') = self.at(i) {
                        digits.push(u8::try_from(d).unwrap_or(b'0'));
                        i = self.cont_skip(i.saturating_add(1));
                    }
                    if matches!(self.at(i), Some('<' | '>')) {
                        self.seek(i);
                        // Decimal digits only, so this is text by construction. A
                        // numeric fd always fits in i32 for realistic input; fall
                        // back to a word if it somehow doesn't parse.
                        if let Some(n) = bytes::as_str(&digits).and_then(|s| s.parse::<i32>().ok())
                        {
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
                '-' if matches!(out.last(), Some(Tok::Op(Op::LessAnd | Op::GreatAnd))) => {
                    // The `-` of `<&-` / `>&-` is a token in its own right, not
                    // the first character of the target word. bash settles that
                    // in the lexer, before `read_token_word` is ever entered
                    // (`read_token`, `parse.y`):
                    //
                    //     /* Hack <&- (close stdin) case.  Also <&N- (dup and
                    //        close). */
                    //     if MBTEST(character == '-' &&
                    //               (last_read_token == LESS_AND ||
                    //                last_read_token == GREATER_AND))
                    //       return (character);
                    //
                    // and its grammar then has literal `'-'` productions
                    // (`GREATER_AND '-'`, `NUMBER LESS_AND '-'`, `REDIR_WORD
                    // GREATER_AND '-'`, …) to receive it.
                    //
                    // So the target is *exactly* `-`, and whatever follows
                    // begins a fresh word: `true 1>&--` is a close plus an
                    // argument `-`, and `echo z 1>&-x` writes `z` to a closed
                    // fd 1 and passes `x` along. Only a *leading* `-` is taken
                    // this way — `1>&2-x` never reaches here, because the `2`
                    // starts an ordinary word that swallows the rest, leaving
                    // the whole `2-x` to fail as a dup target.
                    //
                    // Whitespace before it makes no difference either, since
                    // bash skips blanks before this test: `1>& --` splits the
                    // same way `1>&--` does.
                    self.adv();
                    self.emit_word(out, vec![Seg::Lit(b"-".to_vec())]);
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
            if !self.pending_heredocs.is_empty() && !self.defer_heredocs {
                self.collect_heredocs(out)?;
            }
            out.push(Tok::Op(Op::RParen));
            self.stamp_lines(out, lines, offsets, ends, self.line, start_pos);
        } else if !matches!(out.last(), None | Some(Tok::Newline)) {
            let start_pos = self.pos;
            out.push(Tok::Newline);
            if !self.pending_heredocs.is_empty() && !self.defer_heredocs {
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
        let ends_with_newline = self.chars.last().copied() == Some(Ch::U('\n'));
        self.cur_line()
            .saturating_add(u32::from(!ends_with_newline))
    }

    /// The last input line bash would have **fetched** by the time its reader
    /// reached the cursor — the value its `line_number` holds, and so the line
    /// every unterminated-here-document warning is stamped with.
    ///
    /// It differs from [`Lexer::cur_line`] by whether the cursor's own line
    /// counts. bash reads a whole line at a time, so a cursor *anywhere within* a
    /// line means that line has been fetched; but a cursor sitting exactly at a
    /// line's start has only consumed the previous line's newline, and the line it
    /// is poised on has not been asked for yet. Hence: `cur_line()` normally,
    /// one less at a line boundary. (End of input needs no special case — a
    /// source with no trailing newline leaves the cursor mid-line, so its final
    /// partial line counts, and one with a trailing newline leaves it at a
    /// boundary, so the empty line past the end does not. That is exactly why
    /// `cat <<EOF` / `body` and `cat <<EOF` / `body` / `` both say `line 2`.)
    ///
    /// This is *not* [`Lexer::eof_line`]'s rule, which is one past the last line:
    /// bash bumps `line_number` for the input line it asks for and does not get
    /// when a `$( … )` is left unclosed, but a here-document's reader has already
    /// stopped by then and reports the line it last had.
    fn fetched_line(&self) -> u32 {
        // While the here-document reader is ahead of the token cursor, *it* is
        // what has fetched lines — the cursor's own line says nothing about how
        // far the input has been read. This is why the second `$(cat <<X)` on a
        // line is blamed past the first one's body rather than on its own line.
        if let Some(a) = &self.hd_ahead {
            return a.line;
        }
        let at_line_start = self.pos == 0 || self.at(self.pos.wrapping_sub(1)) == Some('\n');
        self.cur_line()
            .saturating_sub(u32::from(at_line_start))
            .max(1)
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
        let mut end_line = start_line.saturating_add(u32::try_from(inner).unwrap_or(u32::MAX));
        // A here-document gathered from past the `)` of a `$( … )` leaves the
        // reader ahead of the cursor, and the line a token carries is the last
        // line the *reader* had fetched — so the rest of this line is stamped
        // with the body's last line, not with its own. (`x=$(cat <<EOF); echo
        // $LINENO` over a two-line body reports 3.) See [`Lexer::gather_ahead`].
        if let Some(a) = &self.hd_ahead {
            end_line = end_line.max(a.line);
        }
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
        // A subscript may sit between the name and the `=`. It is taken as raw
        // source and read as a word by the parser, exactly as the scalar form's
        // is — see [`crate::parser::Parser::try_assignment`], whose
        // `balanced_subscript_end` this mirrors: brackets are counted and
        // nothing else is looked at, so `n[a[0]]=(v)` and `n[1 ]=(v)` are both
        // the subscript they read as.
        let index = if self.peek() == Some('[') {
            let Some(close) = self.balanced_subscript_end() else {
                self.pos = start;
                return Ok(None);
            };
            let src = self.slice(self.pos.saturating_add(1), close);
            self.pos = close.saturating_add(1);
            Some(src)
        } else {
            None
        };
        let append = self.peek() == Some('+');
        let eq_at = self.pos + usize::from(append);
        if name.is_empty()
            || self.at(eq_at) != Some('=')
            || self.at(eq_at + 1) != Some('(')
        {
            self.pos = start;
            return Ok(None);
        }
        // Commit: consume the optional `+`, the `=`, and the `(`.
        self.pos = eq_at + 2;
        let open = self.cur_line();
        let mut elems: Vec<Vec<Seg>> = Vec::new();
        // The first operator found where an element belongs, and the offset just
        // past it. Recorded rather than returned so the loop still runs to the
        // literal's closing `)`: bash's reader collects the balanced `( … )`
        // before looking inside it, so everything after the literal must still
        // tokenize. See [`Tok::Invalid`].
        let mut bad: Option<(Str, usize)> = None;
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
                    // A literal that never closes still blames the bad element
                    // ahead of the missing `)`, and resumes right after it —
                    // `a=(x <<EOF` / `body` / `EOF` names `<<` and then runs
                    // `body` and `EOF` as commands, rather than swallowing them
                    // as a here-document. Rewinding is safe because the line
                    // counter is derived from the cursor (see [`Self::cur_line`]).
                    if let Some((op, resume)) = bad {
                        self.pos = resume;
                        return Ok(Some(Tok::Invalid(op)));
                    }
                    return Err(eof_matching(')').at(open).recoverable());
                }
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                }
                _ => {
                    // Whatever goes wrong while the literal is being collected
                    // costs only this unit, not the input — an unterminated
                    // quote, `${`, backquote or `$(` inside a literal is worth 1
                    // under bash where the same thing outside one is worth 2.
                    let segs = self.read_array_elem_word().map_err(LexError::recoverable)?;
                    if segs.is_empty() {
                        // An operator (or, defensively, anything else the word
                        // reader could not start on). Step over it either way, so
                        // the loop cannot spin.
                        let op = self.take_operator();
                        if bad.is_none() {
                            bad = Some((op, self.pos));
                        }
                    } else if bad.is_none() {
                        elems.push(segs);
                    }
                }
            }
        }
        if let Some((op, _)) = bad {
            return Ok(Some(Tok::Invalid(op)));
        }
        Ok(Some(Tok::ArrayAssign {
            name,
            index,
            append,
            elems,
        }))
    }

    /// The offset of the `]` closing the subscript the cursor stands on, or
    /// `None` if it never closes.
    ///
    /// Brackets are counted and nothing else is examined — no quoting, no
    /// blanks, no operators — which is what makes `n[a[0]]=(v)` nest and
    /// `n[1 ]=(v)` hold its space. The counterpart for a word the lexer has
    /// already split into segments is `parser::balanced_subscript_end`.
    fn balanced_subscript_end(&self) -> Option<usize> {
        debug_assert!(self.peek() == Some('['));
        let mut depth = 0usize;
        let mut i = self.pos;
        while let Some(c) = self.at(i) {
            match c {
                '[' => depth = depth.saturating_add(1),
                ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// Read one word (until an unquoted operator, blank, or newline).
    /// Push a plain word token, tracking `[[ … ]]` depth and the `=~` regex
    /// trigger so the RHS is lexed in regex mode.
    fn emit_word(&mut self, out: &mut Vec<Tok>, segs: Vec<Seg>) {
        // Detect the bare-literal words `[[`, `]]`, and `=~` to drive the
        // regex-RHS lexing mode. A word is "bare" when it is a single unquoted
        // literal segment.
        if let [Seg::Lit(s)] = segs.as_slice() {
            match s.as_slice() {
                b"[[" => self.cond_depth = self.cond_depth.saturating_add(1),
                b"]]" => self.cond_depth = self.cond_depth.saturating_sub(1),
                b"=~" if self.cond_depth > 0 => self.regex_next = true,
                // The right-hand side of a `[[ … ]]` match is a *pattern*, and
                // bash lexes extended patterns there whether or not `extglob` is
                // set — so `[[ abc == @(abc|x) ]]` works in a default shell.
                // Only in this position: `[[ @(a) == b ]]` and `[[ -n @(a) ]]`
                // are both syntax errors near `(`.
                b"==" | b"!=" | b"=" if self.cond_depth > 0 => self.extpat_next = true,
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
        let mut lit = Str::new();
        // Nesting depth of unquoted `(` … `)` groups. While it is non-zero the
        // word swallows everything, which is the whole point of the construct.
        let mut depth: u32 = 0;
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '<' | '>' if depth == 0 => break,
                ')' if depth == 0 => break,
                '(' => {
                    depth = depth.saturating_add(1);
                    lit.push(b'(');
                    self.pos += 1;
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    lit.push(b')');
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
                    let (raw, src) = self.read_backtick(false)?;
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
                    if let Some(next) = self.bump_ch()
                        && next != '\n'
                    {
                        lit.push(b'\\');
                        next.push_to(&mut lit);
                    } else {
                        self.note_continuation();
                    }
                }
                '$' => {
                    if let Some(seg) = self.read_dollar(false)? {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    } else {
                        lit.push(b'$');
                    }
                }
                _ => self.take_into(&mut lit),
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
    fn read_word_verbatim(&mut self, mode: Verbatim) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = Str::new();
        while let Some(c) = self.peek() {
            match c {
                // Inside quotes a `'` opens nothing — it is a character like any
                // other, and `"${nope:-'a b'}"` keeps both of them.
                '\'' if mode != Verbatim::Dquote => {
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
                    let (raw, src) = self.read_backtick(false)?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
                }
                // A backslash inside quotes reaches only as far as double
                // quoting itself does: the four characters that stay live there,
                // plus the `}` that ends the substitution, plus a newline (a line
                // continuation). Before anything else it is not an escape at all,
                // so `"${nope:-a\tb}"` keeps the backslash and the `t` both.
                '\\' if mode == Verbatim::Dquote => {
                    self.pos += 1;
                    match self.peek() {
                        Some('\n') => {
                            self.pos += 1;
                        }
                        Some('$' | '`' | '"' | '\\' | '}') => {
                            let next = self.bump_ch();
                            flush_lit(&mut segs, &mut lit);
                            if let Some(next) = next {
                                // Quoted, so the character it protected is not
                                // read again as a substitution or a quote.
                                segs.push(Seg::Sq(next.to_str(), true));
                            }
                        }
                        // Not an escape: the backslash stands for itself, and the
                        // character after it is read as it would have been.
                        _ => lit.push(b'\\'),
                    }
                }
                '\\' => {
                    self.pos += 1;
                    if let Some(next) = self.bump_ch()
                        && next != '\n'
                    {
                        if mode == Verbatim::Replacement && (next == '&' || next == '\\') {
                            // Replacement context: keep `\&`/`\\` intact so the
                            // later `&`-scan can tell an escaped ampersand (a
                            // literal `&`) from an active one.
                            lit.push(b'\\');
                            next.push_to(&mut lit);
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
                            segs.push(Seg::Sq(next.to_str(), true));
                        }
                    }
                }
                '$' => {
                    if let Some(seg) = self.read_dollar(false)? {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    } else {
                        lit.push(b'$');
                    }
                }
                _ => self.take_into(&mut lit),
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

    /// Consume the shell operator at the current position and return its
    /// spelling, for a diagnostic that has to name it.
    ///
    /// Always consumes at least one character, so a caller looping until it makes
    /// progress cannot spin on something that is not an operator at all. Such a
    /// character can only reach here through a word reader that declined to start
    /// on it, and naming it verbatim is still the most useful thing to say.
    fn take_operator(&mut self) -> Str {
        for op in OPERATOR_SPELLINGS {
            if op.chars().enumerate().all(|(i, c)| self.at(self.pos.saturating_add(i)) == Some(c)) {
                self.pos = self.pos.saturating_add(op.chars().count());
                return op.as_bytes().to_vec();
            }
        }
        let mut one = Str::new();
        if let Some(c) = self.peek() {
            self.pos = self.pos.saturating_add(1);
            let mut buf = [0u8; 4];
            one.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        one
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
        let mut lit = Str::new();
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
                lit.push(b'[');
                self.pos += 1;
                sub_depth += 1;
                continue;
            }
            if sub_depth > 0 {
                match c {
                    '[' => {
                        lit.push(b'[');
                        sub_depth += 1;
                        self.pos += 1;
                        continue;
                    }
                    ']' => {
                        lit.push(b']');
                        sub_depth -= 1;
                        self.pos += 1;
                        continue;
                    }
                    // Quotes, expansion and escapes keep their normal processing
                    // (fall through to the outer match); everything else — spaces,
                    // operators — is literal subscript content.
                    '\'' | '"' | '`' | '\\' | '$' => {}
                    _ => {
                        self.take_into(&mut lit);
                        continue;
                    }
                }
            }
            // Opener: `X(` where X ∈ ?*+@! (unquoted). Begins/nests a group.
            if extglob && matches!(c, '?' | '*' | '+' | '@' | '!') && self.peek_at(1) == Some('(') {
                push1(&mut lit, c);
                lit.push(b'(');
                self.pos += 2;
                ext_depth += 1;
                continue;
            }
            if ext_depth > 0 {
                match c {
                    '(' => {
                        lit.push(b'(');
                        ext_depth += 1;
                        self.pos += 1;
                        continue;
                    }
                    ')' => {
                        lit.push(b')');
                        ext_depth -= 1;
                        self.pos += 1;
                        continue;
                    }
                    // Quotes, expansion and escapes still get their normal
                    // processing (fall through to the outer match below).
                    '\'' | '"' | '`' | '\\' | '$' => {}
                    // Everything else — including `|`, whitespace, `<`, `>`, `&`,
                    // `;`, `#` — is literal pattern content inside the group.
                    _ => {
                        self.take_into(&mut lit);
                        continue;
                    }
                }
            }
            // Process substitution `<(cmd)` / `>(cmd)` (outside an extglob group):
            // read the balanced `(…)` body as one segment. Handled before the
            // `<`/`>` word-break below so `diff <(a) <(b)` and concatenated forms
            // like `pre<(cmd)` both work.
            if ext_depth == 0
                && matches!(c, '<' | '>')
                && self.at(self.cont_skip(self.pos + 1)) == Some('(')
            {
                let input = c == '<';
                // Consume `<`/`>` and `(` — with whatever the reader deleted
                // between them, since `<\<newline>(cmd)` is a process
                // substitution (see [`Lexer::eat_conts`]).
                self.adv();
                self.pos += 1;
                flush_lit(&mut segs, &mut lit);
                // Like `$( … )`, an unterminated process substitution is
                // reported at the *end of input*, not at the line it opened on
                // (verified against bash 5.2: `cat <(echo a` on line 2 of a
                // 3-line script reports line 4). A nested construct that closed
                // first stamps its own line and `at` will not overwrite it.
                let open_line = self.cur_line();
                let raw = self.read_subst_body().map_err(|e| e.at(self.eof_line()))?;
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
                    let (raw, src) = self.read_backtick(false)?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
                }
                '\\' => {
                    self.pos += 1;
                    if let Some(next) = self.bump_ch()
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
                            next.push_to(&mut lit);
                        } else {
                            flush_lit(&mut segs, &mut lit);
                            segs.push(Seg::Sq(next.to_str(), true));
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
                        lit.push(b'$');
                    }
                }
                _ => self.take_into(&mut lit),
            }
        }
        flush_lit(&mut segs, &mut lit);
        Ok(segs)
    }

    fn read_single_quote(&mut self) -> Result<Str, LexError> {
        let open = self.cur_line();
        let mut s = Str::new();
        loop {
            match self.peek() {
                Some('\'') => {
                    self.pos += 1;
                    return Ok(s);
                }
                Some(_) => self.take_into(&mut s),
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
    /// Note: byte escapes (`\xHH`, `\nnn`) naming a value above 0x7F decode to
    /// that single byte, as bash does, and the word keeps it — `$'\xa9'` is one
    /// byte `a9`, not the two of U+00A9 and not the three of U+FFFD.
    fn read_ansi_c_quote(&mut self) -> Result<Str, LexError> {
        let open = self.cur_line();
        let mut raw = Str::new();
        loop {
            let Some(c) = self.bump_ch() else {
                return Err(eof_matching('\'').at(open));
            };
            if c == '\'' {
                return Ok(crate::escape::ansi_c_unescape(&raw));
            }
            c.push_to(&mut raw);
            if c == '\\' {
                let Some(e) = self.bump_ch() else {
                    return Err(eof_matching('\'').at(open));
                };
                e.push_to(&mut raw);
            }
        }
    }

    fn read_double_quote(&mut self) -> Result<Vec<Seg>, LexError> {
        self.read_double_quote_until(true)
    }

    /// Read the body of a double-quoted string. With `closed` the body ends at
    /// the matching `"` (the normal case: reaching EOF first is an error); with
    /// `closed` false it ends at EOF and a `"` is just another literal
    /// character — which is how bash expands a string that is *implicitly* in
    /// double-quote context rather than delimited by quotes (`Q_DOUBLE_QUOTES`:
    /// `PS4`, `${x@P}`), where a bare `"` in the value stays a `"`.
    fn read_double_quote_until(&mut self, closed: bool) -> Result<Vec<Seg>, LexError> {
        let open = self.cur_line();
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = Str::new();
        loop {
            let Some(c) = self.peek() else {
                if closed {
                    return Err(eof_matching('"').at(open));
                }
                flush_lit(&mut segs, &mut lit);
                return Ok(segs);
            };
            match c {
                '"' if closed => {
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
                            segs.push(Seg::Sq(one(n), true));
                        }
                        Some('\n') => {
                            self.pos += 1;
                        }
                        _ => lit.push(b'\\'),
                    }
                }
                '`' => {
                    self.pos += 1;
                    flush_lit(&mut segs, &mut lit);
                    let (raw, src) = self.read_backtick(true)?;
                    let close = self.cur_line();
                    segs.push(Seg::CmdSub(raw, close, Some(src)));
                }
                '$' => {
                    if let Some(seg) = self.read_dollar(true)? {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    } else {
                        lit.push(b'$');
                    }
                }
                _ => self.take_into(&mut lit),
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
        // Consume the `$`. What follows it is judged after the reader's
        // deletions, so `$<backslash><newline>(` opens a substitution and
        // `$<backslash><newline>{` a braced parameter. See [`Lexer::eat_conts`].
        self.adv();
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
                if self.at(self.cont_skip(self.pos + 1)) == Some('(') {
                    // `$((` is ambiguous: arithmetic, or a substitution whose
                    // body opens with a parenthesised group (`$(( cmd ) | cmd )`,
                    // which bash runs). Nothing local tells them apart, so read it
                    // as arithmetic and, if that does not reach a `))`, rewind and
                    // read it again as a substitution — the backtrack bash's
                    // `parse_dollar_paren` does. Note what does *not* backtrack:
                    // `$(( echo a ))` reaches its `))`, so it stays arithmetic and
                    // fails at evaluation, in bash too.
                    let subst_from = self.cont_skip(self.pos.saturating_add(1));
                    let conts_from = self.conts.len();
                    let open = self.cur_line();
                    self.adv();
                    self.adv();
                    if let Ok(raw) = self.read_arith() {
                        return Ok(Some(Seg::Arith(raw, false)));
                    }
                    // `self.pos` and the continuations the abandoned scan deleted
                    // are all that moved: `cur_line` is derived from the cursor,
                    // and the arithmetic scan records nothing else. The
                    // continuations have to go back too — the same text is about
                    // to be read as a substitution body, which keeps them.
                    self.conts.truncate(conts_from);
                    self.pos = subst_from;
                    // Blamed on the opening line, unlike a plain `$( … )` — bash
                    // has already failed the arithmetic reading by this point, and
                    // that error is stamped where the `$((` is.
                    //
                    // `read_balanced_inner` rather than [`Lexer::read_subst_body`]
                    // because this re-read is not where bash's `parse_comsub`
                    // starts: seeing a second `(`, it returns straight into
                    // `parse_matched_pair` with `P_ARITH` (parse.y:4103), *above*
                    // the `need_here_doc = 0`. So a `$((` that runs out of input
                    // keeps its pending here-documents however it is finally read
                    // — `cat <<E $((`, `cat <<E $(( 1 +` and `cat <<E $(( echo a )`
                    // all warn, while `cat <<E $(` does not.
                    let raw = self
                        .read_balanced_inner('(', ')', true)
                        .map_err(|e| e.at(open))?;
                    Ok(Some(Seg::CmdSub(raw, self.cur_line(), None)))
                } else {
                    self.pos += 1;
                    // `$( … )` is the one construct bash blames on the *end* of
                    // input rather than its opening line: the body is re-parsed
                    // after the outer scan, by which point the line counter has
                    // moved on. (An unterminated quote *inside* the body still
                    // reports its own line — `at` will not overwrite.)
                    let raw = self.read_subst_body().map_err(|e| e.at(self.eof_line()))?;
                    Ok(Some(Seg::CmdSub(raw, self.cur_line(), None)))
                }
            }
            Some(c) if is_name_start(c) => {
                let mut name = String::new();
                while let Some(n) = self.peek() {
                    if is_name_char(n) {
                        name.push(n);
                        // A continuation inside the name is not a break in it:
                        // `$v<backslash><newline>x` reads the variable `vx`.
                        self.adv();
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
    /// See [`Lexer::read_subst_body`] for the substitution form, which counts
    /// parentheses only where the grammar says a `)` closes a group.
    ///
    /// The unterminated-`close` error is left **unstamped** (no reporting line):
    /// which line bash blames depends on what opened the group — the opening
    /// line for `$[`/`<(`/`>(`, the end of input for `$(` — so the caller
    /// stamps it. Errors from the nested quote scans below *are* stamped here,
    /// at the quote's own opening line, and `LexError::at` never overwrites, so
    /// the caller's stamp cannot displace them.
    fn read_balanced(&mut self, open: char, close: char) -> Result<Str, LexError> {
        self.read_balanced_inner(open, close, false)
    }

    /// [`Lexer::read_balanced`] for the body of a `$( … )` or `<( … )`
    /// substitution, which bash reads in the **enclosing** input stream — so a
    /// here-document declared inside the body takes its body from the lines that
    /// follow, and those lines can run straight past the `)`:
    ///
    /// ```text
    /// x=$(cat <<EOF        # the body of the here-document is `body\n); echo hi\n`,
    /// body                 # so the `)` is body text: the substitution never closes
    /// ); echo hi           # and bash reports an unmatched `)` at end of input.
    /// EOF
    /// ```
    ///
    /// The bodies are consumed here and appended to the raw text verbatim, which
    /// puts them exactly where an inline here-document's body would have sat — so
    /// the later re-lex of that text finds them — while this scan carries on
    /// looking for the `)` from *after* them.
    ///
    /// Having to find a `<<` is also why this mode must recognise the places one
    /// can sit without being a here-document operator: a comment, a `<<<`
    /// here-string, a `<<` shift inside `$(( … ))`, a `${ … }` body, a backtick
    /// body (bash lexes those on their own — the here-document inside one is *its*
    /// business, and its closing backtick is found lexically), and a backslash
    /// escape. Those are copied whole rather than character by character, which
    /// incidentally makes `$(echo \))` and `` $(echo `echo )`) `` scan correctly.
    fn read_subst_body(&mut self) -> Result<Str, LexError> {
        let r = self.read_balanced_inner('(', ')', true);
        if r.is_err() {
            // bash reads this body with a whole nested `yyparse`, and `parse_comsub`
            // (parse.y:4133) zeroes `need_here_doc` before starting it. The saved
            // copy is put back by `restore_parser_state` — but only on the path that
            // gets there, and the `EOF_Reached` path (4170) returns before it. So an
            // input that runs out inside a `$( … )` forgets every here-document
            // declared before it, and the reduction that would have warned about
            // them finds nothing pending. No other unclosed construct does this:
            // `"`, `'`, `` ` ``, `${` and `$((` are all read by `parse_matched_pair`,
            // which leaves the counter alone. See [`UngatheredHeredoc`].
            self.heredocs_forgotten = true;
        }
        r
    }

    fn read_balanced_inner(
        &mut self,
        open: char,
        close: char,
        heredocs: bool,
    ) -> Result<Str, LexError> {
        let mut depth = 1usize;
        let mut raw = Str::new();
        // Here-documents declared in this body whose bodies the next newline will
        // bring. Deliberately *not* `self.pending_heredocs`: the enclosing lexer's
        // own pending here-documents belong to the line the substitution sits on
        // and must still be collected after it, in that order (`cat <<A $(cat <<B`
        // collects B first, then A).
        let mut pending: Vec<(Str, bool)> = Vec::new();
        // `#` starts a comment, and `<<` a here-document, only at a word's start.
        let mut word_start = true;
        // The paren depth the innermost `$(( … ))` / `(( … ))` began at, if any. A
        // `<<` in there is a left shift and a `#` a base marker (`16#ff`), so
        // neither introduces anything.
        let mut arith_from: Option<usize> = None;
        // The depths opened by a *nested* `$(`, `<(` or `>(`, each paired with the
        // length `pending` had when it opened — so the entries declared inside it
        // are the ones from that mark on. A plain `( … )` group is not on this
        // stack: its text is part of this body and is re-lexed with it, so a
        // here-document in it is found inline like any other.
        let mut nested: Vec<(usize, usize)> = Vec::new();
        // How many of `pending` were declared directly in this body, and so are
        // this scan's to warn about. See [`Lexer::gather_ahead`].
        let mut own = 0usize;
        // Which `)` closes a group and which terminates a `case` pattern.
        let mut cases = CaseScan::new();
        loop {
            let Some(cx) = self.bump_ch() else {
                return Err(eof_matching(close));
            };
            let c = syn(cx);
            if c == '\'' {
                let q_open = self.cur_line();
                cx.push_to(&mut raw);
                // Copy verbatim to the closing single quote.
                loop {
                    match self.peek() {
                        Some('\'') => {
                            self.pos += 1;
                            raw.push(b'\'');
                            break;
                        }
                        Some(_) => self.take_into(&mut raw),
                        None => return Err(eof_matching('\'').at(q_open)),
                    }
                }
                word_start = false;
                cases.push_quoted();
                continue;
            }
            if c == '"' {
                let q_open = self.cur_line();
                cx.push_to(&mut raw);
                // A double-quoted span is *not* opaque to this scan. Substitution
                // still happens inside it, so a `)` in there may be a nested
                // substitution's — and a `<<` in there may be a here-document
                // whose body has to be fetched from past the enclosing `)`.
                // bash's `parse_matched_pair` recurses into the same constructs
                // with the same reader; copying to the closing quote verbatim
                // instead would lose `"$(cat <<B)"` entirely.
                loop {
                    match self.peek() {
                        Some('\\') => {
                            self.pos += 1;
                            raw.push(b'\\');
                            self.take_into(&mut raw);
                        }
                        Some('"') => {
                            self.pos += 1;
                            raw.push(b'"');
                            break;
                        }
                        // Lexed on its own, exactly as outside the quotes — and
                        // bash does not fetch a here-document declared in one
                        // either, so nothing here does.
                        Some('`') => {
                            self.pos += 1;
                            let (_, verbatim) = self.read_backtick(false)?;
                            raw.push(b'`');
                            raw.extend_from_slice(&verbatim);
                            raw.push(b'`');
                        }
                        Some('$') if self.peek_at(1) == Some('{') => {
                            self.pos += 2;
                            let inner = self.read_dollar_brace()?;
                            raw.extend_from_slice(b"${");
                            raw.extend_from_slice(&inner);
                            raw.push(b'}');
                        }
                        // `$(( … ))`: balanced parens with no here-document in
                        // them — a `<<` there is a left shift. The cursor is left
                        // on the inner `(`, which the balanced read counts as a
                        // level of its own, so it stops on the second `)`.
                        Some('$')
                            if self.peek_at(1) == Some('(') && self.peek_at(2) == Some('(') =>
                        {
                            self.pos += 2;
                            let inner = self.read_balanced_inner('(', ')', false)?;
                            raw.extend_from_slice(b"$(");
                            raw.extend_from_slice(&inner);
                            raw.push(b')');
                        }
                        // A nested substitution, read by this same scan so that
                        // its here-documents are gathered at *its* `)` like any
                        // other's. Recursing rather than counting depth in the
                        // quoted span is what keeps a bare `)` in the text — as
                        // in `"a)b"` — from closing anything.
                        Some('$') if self.peek_at(1) == Some('(') => {
                            self.pos += 2;
                            let inner = self.read_balanced_inner('(', ')', heredocs)?;
                            raw.extend_from_slice(b"$(");
                            raw.extend_from_slice(&inner);
                            raw.push(b')');
                        }
                        Some(_) => self.take_into(&mut raw),
                        None => return Err(eof_matching('"').at(q_open)),
                    }
                }
                word_start = false;
                cases.push_quoted();
                continue;
            }
            if heredocs {
                let in_arith = arith_from.is_some();
                match c {
                    '\n' => {
                        raw.push(b'\n');
                        if !pending.is_empty() {
                            self.consume_subst_heredoc_bodies(&mut pending, &mut raw)?;
                            own = 0;
                        }
                        word_start = true;
                        cases.finish_word(depth);
                        cases.command_end();
                        continue;
                    }
                    // Copied verbatim so the re-lex still sees the comment, but not
                    // scanned: a `<<` inside one introduces nothing.
                    '#' if word_start && !in_arith => {
                        raw.push(b'#');
                        while !matches!(self.peek(), None | Some('\n')) {
                            self.take_into(&mut raw);
                        }
                        continue;
                    }
                    // A backslash escapes the next character, `)` included.
                    '\\' => {
                        raw.push(b'\\');
                        self.take_into(&mut raw);
                        word_start = false;
                        cases.push_quoted();
                        continue;
                    }
                    '`' => {
                        let (_, verbatim) = self.read_backtick(false)?;
                        raw.push(b'`');
                        raw.extend_from_slice(&verbatim);
                        raw.push(b'`');
                        word_start = false;
                        cases.push_quoted();
                        continue;
                    }
                    '$' if self.peek() == Some('{') => {
                        self.pos += 1;
                        let inner = self.read_dollar_brace()?;
                        raw.extend_from_slice(b"${");
                        raw.extend_from_slice(&inner);
                        raw.push(b'}');
                        word_start = false;
                        cases.push_quoted();
                        continue;
                    }
                    // `$((` / `((` open an arithmetic span. The parens are left to
                    // the depth counting below, which is what tells us where the
                    // span ends; only the suppression is recorded here.
                    '$' if self.peek() == Some('(') && self.peek_at(1) == Some('(') => {
                        arith_from = arith_from.or(Some(depth));
                        raw.push(b'$');
                        word_start = false;
                        cases.push_quoted();
                        continue;
                    }
                    '(' if word_start && self.peek() == Some('(') => {
                        arith_from = arith_from.or(Some(depth));
                    }
                    // A `<<<` here-string is consumed whole, so that its second `<`
                    // is never mistaken for the first of a `<<`.
                    '<' if self.peek() == Some('<') && self.peek_at(1) == Some('<') => {
                        self.pos += 2;
                        raw.extend_from_slice(b"<<<");
                        word_start = false;
                        cases.finish_word(depth);
                        continue;
                    }
                    // `<<`: a here-document, whose body the next newline brings from
                    // the enclosing input.
                    '<' if !in_arith && self.peek() == Some('<') => {
                        self.pos += 1;
                        raw.extend_from_slice(b"<<");
                        cases.finish_word(depth);
                        // Read past the reader's deletions the way
                        // `lex_heredoc_op` does. Nothing is recorded as deleted
                        // here: this scan is copying source for a later re-lex,
                        // which will run the same deletions again. `<<` and `-`
                        // are written into `raw` rather than copied from it, so
                        // a continuation between them simply does not reach the
                        // re-lex; the delimiter below *is* copied verbatim, so
                        // its own continuations travel and are deleted there.
                        self.skip_conts(false);
                        let strip = self.peek() == Some('-');
                        if strip {
                            self.pos += 1;
                            raw.push(b'-');
                        }
                        loop {
                            while matches!(self.peek(), Some(' ' | '\t')) {
                                self.take_into(&mut raw);
                            }
                            if !self.skip_conts(false) {
                                break;
                            }
                        }
                        // Copy the delimiter word as written — quotes and all, since
                        // the re-lex has to draw the same expand/no-expand
                        // conclusion from it that `read_heredoc_delim` just did.
                        let word = self.pos;
                        let (delim, _) = self.read_heredoc_delim(false)?;
                        let written = self.slice(word, self.pos);
                        raw.extend_from_slice(&written);
                        pending.push((delim, strip));
                        if nested.is_empty() {
                            own += 1;
                        }
                        word_start = false;
                        continue;
                    }
                    _ => {}
                }
            }
            if heredocs {
                // Feed the `case` tracker. Every delimiter ends the word being
                // read, and the reserved words are only recognised there.
                match c {
                    ' ' | '\t' => cases.finish_word(depth),
                    ';' => {
                        cases.finish_word(depth);
                        cases.semi(self.peek());
                    }
                    '&' | '|' => {
                        cases.finish_word(depth);
                        cases.command_end();
                    }
                    // A redirection: the word after it names a file, not a
                    // command, so `case` there is not the reserved word.
                    '<' | '>' => {
                        cases.finish_word(depth);
                        cases.redirect();
                    }
                    '(' => {
                        cases.finish_word(depth);
                        cases.command_end();
                    }
                    ')' => cases.finish_word(depth),
                    _ => cases.push(c),
                }
            }
            if c == open {
                // A `case` pattern's optional `(` has no mate of its own.
                if !(heredocs && cases.is_pattern_open(depth)) {
                    depth += 1;
                    // `$(`, `<(` and `>(` — the sigil is the character before the
                    // paren the cursor has just stepped over. An arithmetic `$((`
                    // is excluded: it opens no reader of its own, and a `<<` in it
                    // is a left shift anyway.
                    if heredocs
                        && arith_from.is_none()
                        && matches!(self.at(self.pos.wrapping_sub(2)), Some('$' | '<' | '>'))
                    {
                        nested.push((depth, pending.len()));
                    }
                }
            } else if c == close {
                // A pattern's `)` closes the pattern, not a group — this is the
                // whole reason the scan tracks `case` at all.
                if !(heredocs && cases.take_pattern_close(depth)) {
                    depth -= 1;
                    if depth == 0 {
                        // A `case` still open here is one bash's parser would have
                        // met this `)` in the middle of, so hand the `)` to the
                        // body parse and let it name the token — which also puts
                        // the failure where bash puts it, in the substitution
                        // rather than in the enclosing input (the two exit
                        // differently: 1 for a substitution, 2 for the input).
                        if heredocs && cases.open_at_close() {
                            push1(&mut raw, close);
                        }
                        // A here-document declared in this body but never
                        // delimited inside it: its `<<` and this `)` are on one
                        // line, so the body can only lie past the substitution's
                        // own text. bash reads whole lines, so it fetches it
                        // anyway — after warning that it had to.
                        if !pending.is_empty() {
                            self.gather_ahead(&mut pending, own, &mut raw)?;
                        }
                        return Ok(raw);
                    }
                    if nested.last().map(|&(d, _)| d) == Some(depth + 1) {
                        // The nested substitution ends here, so anything it
                        // declared and did not delimit is *its* here-document
                        // past *its* close — and bash's reader, which is one
                        // line-based reader shared across the whole nesting,
                        // fetches those bodies now, before the outer scan goes
                        // on. Doing it here rather than deferring to the re-lex
                        // is what puts the reader on the right line for every
                        // warning that follows: in
                        // `x=$(cat <<A; echo $(cat <<B) mid)` bash blames the
                        // nested one on line 1 and the outer one on the line B's
                        // body ran to, not on line 1 twice.
                        //
                        // The body text is spliced in ahead of the `)` that is
                        // about to be copied, so the re-lex of this body finds
                        // the nested here-document *inline* — which is also why
                        // that re-lex has nothing left to warn about.
                        //
                        // `own` needs no adjustment: it counts entries declared
                        // with `nested` empty, which are the ones ahead of every
                        // mark, so the split never touches them. A newline inside
                        // the nested body drains `pending` and zeroes `own`
                        // together, which leaves the mark past the end — hence
                        // the clamp, and nothing to gather.
                        let mark = nested.pop().map_or(0, |(_, m)| m);
                        let mut inner = pending.split_off(mark.min(pending.len()));
                        if !inner.is_empty() {
                            let count = inner.len();
                            self.gather_ahead(&mut inner, count, &mut raw)?;
                        }
                    }
                    if arith_from == Some(depth) {
                        arith_from = None;
                    }
                }
            }
            cx.push_to(&mut raw);
            if heredocs {
                word_start = matches!(c, ' ' | '\t' | ';' | '&' | '|' | '(' | ')');
            }
        }
    }

    /// Fetch the bodies of here-documents that a `$( … )` closed over, from the
    /// lines *after* the one the cursor is parsing, and append them to the
    /// substitution's raw text.
    ///
    /// `x=$(cat <<EOF); echo "$x"` / `body` / `EOF` is the shape: the `<<` and the
    /// `)` share a line, so the body is not inside the substitution at all. bash
    /// reads a line at a time, so its reader can simply fetch the next ones and
    /// hand them over — which it does, after warning that it had to. The line
    /// being parsed then resumes after the `)`, unaffected.
    ///
    /// Our cursor is one index into a flat buffer, so "read the next lines
    /// without disturbing this one" is done by moving it to the next line, doing
    /// the ordinary gather, and recording where it got to as a read-ahead before
    /// putting it back. [`Lexer::sync_ahead`] redeems that record when the cursor
    /// reaches the end of its line, so the lines are read exactly once.
    ///
    /// `own` counts the here-documents the warning is about; 0 fetches silently.
    /// The scan calls this twice over for a nested substitution — once at the
    /// nested `)` for what that one declared, once at its own `)` for the rest —
    /// so that the reader advances in bash's order and each warning names the
    /// line the reader had reached when it was raised.
    fn gather_ahead(
        &mut self,
        pending: &mut Vec<(Str, bool)>,
        own: usize,
        raw: &mut Str,
    ) -> Result<(), LexError> {
        if own > 0 {
            self.warnings.push(ReaderWarning::SubstHeredoc(SubstHeredoc {
                count: own,
                line: self.fetched_line(),
                tok_index: self.next_tok_index,
            }));
        }
        let resume = self.pos;
        // An earlier substitution on this line may already have taken lines; carry
        // on from where it stopped rather than re-reading them.
        self.pos = match self.hd_ahead.take() {
            Some(a) => a.pos,
            None => {
                let mut p = self.pos;
                while !matches!(self.at(p), None | Some('\n')) {
                    p += 1;
                }
                p.saturating_add(usize::from(p < self.chars.len()))
            }
        };
        // The body belongs where an inline here-document's would have sat: on the
        // lines after the command, which the re-lex of this text finds only if the
        // command is terminated first. The scan stopped at `)`, so it never
        // reached a newline of its own to supply that.
        raw.push(b'\n');
        let gathered = self.consume_subst_heredoc_bodies(pending, raw);
        // A body whose last line *is* the last line of the input has no newline
        // of its own to copy, and this splice may not be the last thing in the
        // text: the `)` of the substitution that closed here is appended right
        // after it, and `B` and `)` run together into `B)` — a delimiter the
        // re-lex can never find. Only the copy gets the newline the input
        // omitted; the reader has not moved.
        if !raw.ends_with(b"\n") {
            raw.push(b'\n');
        }
        self.hd_ahead = Some(HdAhead { pos: self.pos, line: self.fetched_line() });
        self.pos = resume;
        gathered
    }

    /// Consume the bodies of here-documents declared inside a `$( … )` / `<( … )`
    /// body, appending every line **verbatim** — the delimiter line included — to
    /// the substitution's raw text. See [`Lexer::read_subst_body`].
    fn consume_subst_heredoc_bodies(
        &mut self,
        pending: &mut Vec<(Str, bool)>,
        raw: &mut Str,
    ) -> Result<(), LexError> {
        for (delim, strip) in core::mem::take(pending) {
            // The moment bash's warning names; see [`Lexer::collect_heredocs`],
            // which captures the same thing for a here-document read inline.
            let body_line = self.fetched_line();
            loop {
                if self.pos >= self.chars.len() {
                    if self.strict_heredoc_eof {
                        return Err(unterminated_heredoc(&delim));
                    }
                    self.warnings.push(ReaderWarning::HeredocEof(HeredocEof {
                        delim: delim.clone(),
                        body_line,
                        eof_line: self.fetched_line(),
                        tok_index: self.next_tok_index,
                    }));
                    // Close the body off with the delimiter the input never
                    // supplied. This text is re-lexed when the substitution runs,
                    // and that lex would otherwise run out of input in the same
                    // place and warn a second time about the one here-document —
                    // whereas the warning already recorded here is the reader's,
                    // raised once, which is what bash prints.
                    raw.extend_from_slice(&delim);
                    raw.push(b'\n');
                    break;
                }
                let start = self.pos;
                while !matches!(self.peek(), None | Some('\n')) {
                    self.pos += 1;
                }
                let eol = self.pos;
                if self.peek() == Some('\n') {
                    self.pos += 1;
                }
                let taken = self.slice(start, self.pos);
                raw.extend_from_slice(&taken);
                let mut line = self.slice(start, eol);
                if line.ends_with(b"\r") {
                    line.pop();
                }
                let content = if strip { strip_tabs(&line) } else { line.as_slice() };
                if content == delim.as_slice() {
                    break;
                }
            }
        }
        Ok(())
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
    fn read_dollar_brace(&mut self) -> Result<Str, LexError> {
        let open = self.cur_line();
        let mut raw = Str::new();
        loop {
            let Some(cx) = self.bump_ch() else {
                return Err(eof_matching('}').at(open));
            };
            match syn(cx) {
                // First unescaped, unquoted, non-nested `}` closes the span.
                '}' => return Ok(raw),
                // Backslash escapes the next character (both are preserved
                // verbatim so later re-parsing sees the escape) — unless that
                // character is a newline, in which case the reader deleted the
                // pair before this scan ever ran and neither reaches the raw
                // text: `${v\<newline>x}` names the variable `vx`.
                '\\' => {
                    if self.cont_len_at(self.pos.saturating_sub(1)).is_some() {
                        self.pos = self.pos.saturating_sub(1);
                        self.eat_conts();
                    } else {
                        raw.push(b'\\');
                        self.take_into(&mut raw);
                    }
                }
                // Single quotes: copy verbatim to the closing quote.
                '\'' => {
                    let q_open = self.cur_line();
                    raw.push(b'\'');
                    loop {
                        match self.peek() {
                            Some('\'') => {
                                self.pos += 1;
                                raw.push(b'\'');
                                break;
                            }
                            Some(_) => self.take_into(&mut raw),
                            None => return Err(eof_matching('\'').at(q_open)),
                        }
                    }
                }
                // Double quotes: copy to the closing quote, honoring `\`.
                '"' => {
                    let q_open = self.cur_line();
                    raw.push(b'"');
                    loop {
                        match self.peek() {
                            Some('\\') => {
                                self.pos += 1;
                                raw.push(b'\\');
                                self.take_into(&mut raw);
                            }
                            Some('"') => {
                                self.pos += 1;
                                raw.push(b'"');
                                break;
                            }
                            Some(_) => self.take_into(&mut raw),
                            None => return Err(eof_matching('"').at(q_open)),
                        }
                    }
                }
                // Backtick command substitution: copy verbatim to the closing
                // backtick (honoring `\``).
                '`' => {
                    let q_open = self.cur_line();
                    raw.push(b'`');
                    loop {
                        match self.peek() {
                            Some('\\') => {
                                self.pos += 1;
                                raw.push(b'\\');
                                self.take_into(&mut raw);
                            }
                            Some('`') => {
                                self.pos += 1;
                                raw.push(b'`');
                                break;
                            }
                            Some(_) => self.take_into(&mut raw),
                            None => return Err(eof_matching('`').at(q_open)),
                        }
                    }
                }
                // `$…` may begin a nested construct that must balance with its
                // own terminator; consume it whole so a `}` or `)` inside it is
                // not mistaken for our terminator.
                '$' => {
                    raw.push(b'$');
                    match self.peek() {
                        Some('{') => {
                            raw.push(b'{');
                            self.pos += 1;
                            let inner = self.read_dollar_brace()?;
                            raw.extend_from_slice(&inner);
                            raw.push(b'}');
                        }
                        Some('(') => {
                            raw.push(b'(');
                            self.pos += 1;
                            if self.peek() == Some('(') {
                                raw.push(b'(');
                                self.pos += 1;
                                let inner = self.read_arith()?;
                                raw.extend_from_slice(&inner);
                                raw.extend_from_slice(b"))");
                            } else {
                                // The same substitution as anywhere else, so it
                                // reads a here-document declared inside it the same
                                // way — the body lines land in this `${ … }`'s raw
                                // text, right where the nested re-lex expects them.
                                let inner =
                                    self.read_subst_body().map_err(|e| e.at(self.eof_line()))?;
                                raw.extend_from_slice(&inner);
                                raw.push(b')');
                            }
                        }
                        Some('[') => {
                            raw.push(b'[');
                            self.pos += 1;
                            let sub_open = self.cur_line();
                            let inner = self.read_balanced('[', ']').map_err(|e| e.at(sub_open))?;
                            raw.extend_from_slice(&inner);
                            raw.push(b']');
                        }
                        _ => {}
                    }
                }
                _ => cx.push_to(&mut raw),
            }
        }
    }

    /// Read a `$(( … ))` body (up to the closing `))`).
    fn read_arith(&mut self) -> Result<Str, LexError> {
        self.read_arith_body(false)?
            .ok_or_else(|| LexError::new("malformed arithmetic expansion"))
    }

    /// Read an arithmetic body up to its closing `))`.
    ///
    /// `Ok(None)` means the body was balanced but the second `)` was not where
    /// the caller requires it. That is a plain error for the `$(( … ))`
    /// expansion, but for the `(( … ))` *command* it is bash's cue to re-read
    /// the whole thing as nested subshells, so it is reported rather than
    /// raised. `Err` is reserved for running out of input, which is an error
    /// either way — bash's `parse_matched_pair` fails outright there and
    /// `parse_arith_cmd` passes the failure straight on without rewinding.
    ///
    /// `adjacent` picks which rule the second `)` is held to. The command form
    /// requires it to be the very next character: bash reads it with
    /// `shell_getc (0)`, which does *not* delete a `\<newline>`, so nothing at
    /// all may come between the two — not a space, not a tab, not a newline,
    /// not a continuation. The expansion form has no such test (it goes through
    /// `parse_matched_pair`, whose removal is on), so it tolerates a
    /// continuation there, as `echo $((1+1)\<newline>)` shows. The *body* is
    /// read the same way for both, which is why `((1 +\<newline>1))` is
    /// arithmetic.
    fn read_arith_body(&mut self, adjacent: bool) -> Result<Option<Str>, LexError> {
        let open = self.cur_line();
        let mut depth = 0usize;
        let mut raw = Str::new();
        loop {
            let Some(cx) = self.bump_ch() else {
                return Err(eof_matching(')').at(open));
            };
            match syn(cx) {
                '(' => {
                    depth += 1;
                    raw.push(b'(');
                }
                // The reader deleted a `\<newline>` before this scan saw it, so
                // it is neither text of the expression nor something that can
                // come between the two closing parentheses.
                '\\' => {
                    if self.cont_len_at(self.pos.saturating_sub(1)).is_some() {
                        self.pos = self.pos.saturating_sub(1);
                        self.eat_conts();
                    } else {
                        raw.push(b'\\');
                    }
                }
                ')' => {
                    if depth == 0 {
                        // Expect a second ')'.
                        if !adjacent {
                            self.eat_conts();
                        }
                        if self.peek() == Some(')') {
                            self.pos += 1;
                            return Ok(Some(raw));
                        }
                        return Ok(None);
                    }
                    depth -= 1;
                    raw.push(b')');
                }
                _ => cx.push_to(&mut raw),
            }
        }
    }

    /// Handle a `<<` / `<<-` here-document operator: read the delimiter word on
    /// the current line, emit the operator token plus a placeholder body token,
    /// and record the here-doc for body collection at the next newline.
    fn lex_heredoc_op(&mut self, out: &mut Vec<Tok>) -> Result<(), LexError> {
        // Everything up to the delimiter is read after the reader's deletions,
        // so `<<-` may be written with a continuation inside it and blanks and
        // continuations may alternate freely before the word (see
        // [`Lexer::eat_conts`]).
        let strip = self.peek() == Some('-');
        if strip {
            self.adv();
        }
        loop {
            while matches!(self.peek(), Some(' ' | '\t')) {
                self.pos += 1;
            }
            if !self.skip_conts(true) {
                break;
            }
        }
        let (delim, expand) = self.read_heredoc_delim(true)?;
        out.push(Tok::Op(if strip { Op::DLessDash } else { Op::DLess }));
        let tok_index = out.len();
        // Nothing was read at all: the scan stopped on a separator, or on the end
        // of the text, before a single delimiter character. Only quoting can
        // produce an *empty* delimiter that consumed something — and quoting
        // clears `expand` — so this pair of conditions is exactly "no word here".
        //
        // bash's `<<` is a redirection operator whose target is an ordinary WORD
        // (parse.y's `redirection: … '<' '<' WORD`), so a missing one is not a
        // here-document with an odd delimiter, it is a grammar error at whatever
        // token stands in the WORD's place: `cat << ; echo` says ``syntax error
        // near unexpected token `;'``. Emitting the operator alone, with no
        // `HereDoc` after it and nothing on `pending_heredocs`, is what makes the
        // parser say so — it diagnoses a `<<` with no target exactly as it
        // already diagnoses a `<` with none.
        //
        // It also leaves the operator visible to the alias pass, which reads the
        // delimiter that belongs to it out of the calling text. See
        // [`expand_aliases_inner`].
        if delim.is_empty() && expand {
            return Ok(());
        }
        out.push(Tok::HereDoc(Vec::new(), delim.clone(), !expand));
        self.pending_heredocs.push(PendingHeredoc {
            delim,
            strip,
            expand,
            tok_index,
        });
        Ok(())
    }

    /// Read a here-document delimiter word. Any quoting (`'EOF'`, `"EOF"`,
    /// `\EOF`) disables expansion of the body and is stripped from the delimiter.
    ///
    /// A line continuation anywhere in the word was deleted before this scan —
    /// so `<<E\<newline>OF` wants `EOF` and still expands the body, while
    /// `<<\E\<newline>OF` wants `EOF` and does not (the `\E` quoted it, the
    /// continuation is simply absent). The one exemption is the reader's own:
    /// inside `'…'` the pair is data, so `<<'E\<newline>OF'` wants a delimiter
    /// with a newline in it, which no line can equal. `record` is
    /// [`Lexer::skip_conts`]'s.
    ///
    /// The word is a *shell word*, which is why a `$( … )`, `$(( … ))`,
    /// `${ … }` or `` ` … ` `` in it is taken whole rather than stopping the
    /// scan. bash reads the delimiter with `read_token_word` like any other
    /// word, so those are scanned as matched pairs and become part of its text;
    /// nothing is ever *expanded* — the text is matched against each body line
    /// literally, so `<<E$(echo 1)` wants a line reading `E$(echo 1)`. A
    /// separator inside such a group is data: `<<E$(a b)` is one delimiter, not
    /// a delimiter `E$(a` followed by a word `b)`. Nor does a group quote the
    /// word: a quote *inside* one — `<<E$(echo "a b")` — leaves the body
    /// expanding, unlike a quote around the delimiter itself.
    ///
    /// A group that never closes is fatal, and is [`Lexer::take_delim_group`]'s
    /// to report.
    fn read_heredoc_delim(&mut self, record: bool) -> Result<(Str, bool), LexError> {
        let mut delim = Str::new();
        let mut expand = true;
        // Nothing has gone into the word yet. bash reads the delimiter with
        // `read_token`, whose first act is the comment test — a `#` that *starts*
        // a token opens a comment — so `cat <<#c` and `cat << #c` have no
        // delimiter at all and are grammar errors, while `cat <<E#c` and
        // `cat <<''#c` want `E#c` and `#c`. A line continuation does not start
        // the word (the reader deleted it), so a `#` after one is a comment too.
        let mut first = true;
        loop {
            self.skip_conts(record);
            let Some(c) = self.peek() else { break };
            match c {
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '(' | ')' => break,
                '#' if first => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                    break;
                }
                '`' | '$' if self.delim_group_at() => {
                    self.take_delim_group_at(&mut delim, record)?;
                }
                '\'' => {
                    expand = false;
                    self.pos += 1;
                    while let Some(q) = self.bump_ch() {
                        if q == '\'' {
                            break;
                        }
                        q.push_to(&mut delim);
                    }
                }
                '"' => {
                    expand = false;
                    self.pos += 1;
                    loop {
                        self.skip_conts(record);
                        let Some(q) = self.bump_ch() else { break };
                        if q == '"' {
                            break;
                        }
                        q.push_to(&mut delim);
                    }
                }
                // A `\` that ends the input quotes nothing and is left out of the
                // word here, which is not what bash does with it — see
                // TD-OILS-A-HERE-DOCUMENT-DELIMITER-OF-A-LONE-TRAILING-BACKSLASH.
                '\\' => {
                    expand = false;
                    self.pos += 1;
                    self.take_into(&mut delim);
                }
                _ => self.take_into(&mut delim),
            }
            first = false;
        }
        Ok((delim, expand))
    }

    /// Whether the cursor is on the opener of a group — `` ` ``, `$(`, `$((` or
    /// `${` — that [`Lexer::take_delim_group_at`] would take whole.
    fn delim_group_at(&self) -> bool {
        match self.peek() {
            Some('`') => true,
            Some('$') => matches!(self.peek_at(1), Some('(' | '{')),
            _ => false,
        }
    }

    /// Append a group opened at the cursor — its opener, body and closer,
    /// verbatim — to `out`.
    ///
    /// Which of bash's readers an opener goes to decides which line an
    /// unterminated one is blamed on, so the openers are kept apart here rather
    /// than lumped into one paren scan:
    ///
    /// * `${` and `` ` `` are `parse_matched_pair`, which reports at the
    ///   `start_lineno` it captured on entry — so the blame is the *opener's*
    ///   line. Nesting makes that visible: `<<E${` / `body` / `E${` dies on
    ///   line 3, whose `${` opened the innermost unclosed group.
    /// * `$((` is `parse_matched_pair` as well (`P_ARITH`), so likewise the
    ///   opener's line: `echo $((1+1` / `a` / `b` is blamed on line 1.
    /// * `$(` alone is `parse_comsub`, which parses the body as a command and so
    ///   reports where *that* parse died — the end of the input. `echo $(1` /
    ///   `a` / `b` is blamed on line 4, one past the last line of the file.
    fn take_delim_group_at(&mut self, out: &mut Str, record: bool) -> Result<(), LexError> {
        let opened = self.cur_line();
        if self.peek() == Some('`') {
            self.take_into(out);
            return self.take_delim_group(out, '`', '`', record, 1, Some(opened));
        }
        self.take_into(out);
        let opener = self.peek();
        self.take_into(out);
        match opener {
            Some('{') => self.take_delim_group(out, '{', '}', record, 1, Some(opened)),
            // `$((` opens two parens at once, which is why the scan below has to
            // count down to the second `)`.
            _ if self.peek() == Some('(') => {
                self.take_into(out);
                self.take_delim_group(out, '(', ')', record, 2, Some(opened))
            }
            _ => self.take_delim_group(out, '(', ')', record, 1, None),
        }
    }

    /// Append the body and closer of a group already opened in a here-document
    /// delimiter word, verbatim, to `out`.
    ///
    /// The opener has been consumed by [`Lexer::take_delim_group_at`], which
    /// also fixed the starting `depth` and the `stamp` an end-of-input error
    /// takes. This reads to the matching `close`, stepping over `\`-escapes and
    /// single- and double-quoted runs, in which a `close` is data.
    ///
    /// `open == close` (a backquote) never nests, and nothing nests inside it —
    /// bash guards its `$(`/`${` branch with `open != '`'`. So `` <<E`a `` /
    /// `body` / `` E`a `` closes on line 3's backquote and wants a delimiter
    /// with two newlines in it, which no line can equal.
    ///
    /// A group *inside* the group is a reader of its own and recurses, carrying
    /// its own stamp — which is what moves the blame for `<<E${` / `body` /
    /// `E${` from line 1 to line 3. A bare `open` is only a count, and not even
    /// that everywhere: bash passes `P_FIRSTCLOSE` for `${ … }`, so a lone `{`
    /// in there closes nothing and opens nothing, while inside `$(( … ))` every
    /// `(` counts.
    ///
    /// Running out of input is bash's end-of-input error, `unexpected EOF while
    /// looking for matching \`)'`, and it is fatal — the delimiter word is *not*
    /// whatever was read up to there.
    fn take_delim_group(
        &mut self,
        out: &mut Str,
        open: char,
        close: char,
        record: bool,
        mut depth: usize,
        stamp: Option<u32>,
    ) -> Result<(), LexError> {
        loop {
            self.skip_conts(record);
            let Some(c) = self.peek() else {
                // `stamp` is the opener's line for the `parse_matched_pair`
                // readers. A `$( … )` has none and takes the end of the input,
                // the same stamp every other unterminated substitution body
                // gets (see [`Lexer::read_subst_body`]'s call sites).
                return Err(eof_matching(close).at(stamp.unwrap_or_else(|| self.eof_line())));
            };
            if open != '`' && self.delim_group_at() {
                self.take_delim_group_at(out, record)?;
                continue;
            }
            self.take_into(out);
            match c {
                // A `\` takes the next character with it, whatever it is. At the
                // end of input there is nothing to take, and the loop's own EOF
                // check on the next turn raises.
                '\\' if self.peek().is_some() => self.take_into(out),
                '\'' | '"' => {
                    let quote = c;
                    let q_open = self.cur_line();
                    loop {
                        self.skip_conts(record);
                        let Some(q) = self.peek() else {
                            return Err(eof_matching(quote).at(q_open));
                        };
                        self.take_into(out);
                        if q == quote {
                            break;
                        }
                        // A `\` escapes inside `"…"` and is literal inside `'…'`.
                        if q == '\\' && quote == '"' && self.peek().is_some() {
                            self.take_into(out);
                        }
                    }
                }
                _ if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ if c == open && open == '(' => depth += 1,
                _ => {}
            }
        }
    }

    /// Collect the bodies of all pending here-documents from the lines following
    /// the just-consumed newline, in order, filling in their placeholder tokens.
    fn collect_heredocs(&mut self, out: &mut [Tok]) -> Result<(), LexError> {
        let pending = core::mem::take(&mut self.pending_heredocs);
        for ph in pending {
            let mut body = Str::new();
            // Captured before the first body line is read, because that is the
            // moment bash's message names — and it cannot be recovered afterwards.
            // For the *second* of two here-documents it is not the operator's line
            // at all but wherever the first one's body left the cursor, which is
            // why `cat <<A <<B` is the shape to test against.
            let body_line = self.fetched_line();
            // Where this body's collection starts, so the lines it consumes can
            // be counted off against it below.
            let from = self.pos;
            loop {
                if self.pos >= self.chars.len() {
                    // EOF before the delimiter. In strict mode (REPL
                    // incompleteness check) this is *incomplete input* — the
                    // here-doc body is still being typed — so surface an
                    // "unexpected EOF" that the REPL treats as "keep reading".
                    // In lenient mode (script/`-c`) bash accepts the partial
                    // body, so we do too — but it also warns, so record what the
                    // warning needs. See [`HeredocEof`].
                    if self.strict_heredoc_eof {
                        return Err(unterminated_heredoc(&ph.delim));
                    }
                    self.warnings.push(ReaderWarning::HeredocEof(HeredocEof {
                        delim: ph.delim.clone(),
                        body_line,
                        eof_line: self.fetched_line(),
                        tok_index: ph.tok_index,
                    }));
                    break;
                }
                let start = self.pos;
                while !matches!(self.peek(), None | Some('\n')) {
                    self.pos += 1;
                }
                let eol = self.pos;
                let mut line = self.slice(start, self.pos);
                if self.peek() == Some('\n') {
                    self.pos += 1;
                }
                if line.ends_with(b"\r") {
                    line.pop();
                }
                let content = if ph.strip { strip_tabs(&line) } else { line.as_slice() };
                if content == ph.delim.as_slice() {
                    break;
                }
                // An expanding here-doc (unquoted delimiter) joins a line ending
                // in an unescaped `\` to the next one, so the history has to
                // drop that pair as well. A quoted delimiter makes the body
                // literal, backslashes and all.
                if ph.expand && ends_with_continuation(content) {
                    // The backslash is the last character before the newline.
                    let mut at = eol.saturating_sub(1);
                    if self.at(at) == Some('\r') {
                        at = at.saturating_sub(1);
                    }
                    self.conts.push(u32::try_from(at).unwrap_or(u32::MAX));
                }
                body.extend_from_slice(content);
                body.push(b'\n');
            }
            // What the collection moved bash's reader by. `make_here_document`
            // (make_cmd.c:621) bumps `line_number` once per line `read_a_line`
            // hands it — the delimiter's line included — and `read_a_line` bumps
            // it again for each `\<newline>` it joins away inside an expanding
            // body, so the two together come to exactly the physical lines
            // consumed. See [`Tokenized::heredoc_lines`] for who needs this.
            let taken = self.chars.get(from..self.pos).unwrap_or(&[]);
            let lines = taken.iter().filter(|&&c| c == '\n').count()
                + usize::from(taken.last().is_some_and(|&c| c != '\n'));
            self.heredoc_lines.push((ph.tok_index, u32::try_from(lines).unwrap_or(u32::MAX)));
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
    fn read_backtick(&mut self, in_dquote: bool) -> Result<(Str, Str), LexError> {
        let open = self.cur_line();
        let start = self.pos;
        let mut raw = Str::new();
        loop {
            match self.peek() {
                Some('`') => {
                    self.pos += 1;
                    let src = self.slice(start, self.pos.saturating_sub(1));
                    return Ok((raw, src));
                }
                Some('\\') => {
                    self.pos += 1;
                    // Inside backticks, `\`` and `\\` and `\$` are unescaped.
                    // Inside *double quotes* `\"` is too, because the escape
                    // belongs to the enclosing quoted string and is removed
                    // before the body is ever parsed as a command — so it is
                    // stripped even where the body would have quoted it:
                    //
                    // ```sh
                    // echo "`echo \"x\"`"      # x    — not "x"
                    // echo "`echo '\"'`"       # "    — the body is echo '"'
                    // echo `echo \"x\"`        # "x"  — unquoted, so kept
                    // ```
                    match self.peek() {
                        Some(n @ ('`' | '\\' | '$')) => {
                            self.pos += 1;
                            push1(&mut raw, n);
                        }
                        Some('"') if in_dquote => {
                            self.pos += 1;
                            raw.push(b'"');
                        }
                        _ => raw.push(b'\\'),
                    }
                }
                Some(_) => self.take_into(&mut raw),
                None => return Err(eof_matching('`').at(open)),
            }
        }
    }
}

/// Whether `line` ends in a `\` that is not itself escaped — a line
/// continuation, which the reader joins to the following line.
fn ends_with_continuation(line: BStr<'_>) -> bool {
    line.iter().rev().take_while(|&&b| b == b'\\').count() % 2 == 1
}

fn flush_lit(segs: &mut Vec<Seg>, lit: &mut Str) {
    if !lit.is_empty() {
        segs.push(Seg::Lit(core::mem::take(lit)));
    }
}

/// Lower a here-document body into segments. When `expand` is false (quoted
/// delimiter) the whole body is a single literal; otherwise it is scanned like a
/// double-quoted context (parameter/command/arith expansion, `"` literal).
fn scan_heredoc_segs(body: BStr<'_>, expand: bool) -> Result<Vec<Seg>, LexError> {
    if !expand {
        return Ok(vec![Seg::Lit(body.to_vec())]);
    }
    let mut lx = Lexer::new(body, ParseOpts::default());
    let mut segs: Vec<Seg> = Vec::new();
    let mut lit = Str::new();
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
                        segs.push(Seg::Sq(one(n), true));
                    }
                    Some('\n') => {
                        lx.pos += 1;
                    }
                    _ => lit.push(b'\\'),
                }
            }
            '`' => {
                lx.pos += 1;
                flush_lit(&mut segs, &mut lit);
                // `false`: a here-doc body is a double-quoted *context*, but it
                // is not inside a double-quoted string, and bash keeps the
                // backslash there — `<<EOF` … `` `echo \"x\"` `` prints `"x"`.
                let (raw, src) = lx.read_backtick(false)?;
                segs.push(Seg::CmdSub(raw, lx.cur_line(), Some(src)));
            }
            '$' => {
                if let Some(seg) = lx.read_dollar(true)? {
                    flush_lit(&mut segs, &mut lit);
                    segs.push(seg);
                } else {
                    lit.push(b'$');
                }
            }
            _ => lx.take_into(&mut lit),
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
        super::tokenize(src.as_bytes(), ParseOpts::default())
    }

    /// As [`tokenize`], with per-token line numbers.
    fn tokenize_spanned(src: &str) -> Result<(Vec<Tok>, Vec<u32>), LexError> {
        super::tokenize_spanned(src.as_bytes(), ParseOpts::default())
            .map(|s| (s.toks, s.lines))
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
        let q = |src: &str| super::open_quote(src.as_bytes(), ParseOpts::default());
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
        let c = |src: &str| super::ends_in_continuation(src.as_bytes(), ParseOpts::default());
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
                    Tok::Word(vec![Seg::Lit(open.to_string().into_bytes())]),
                    Tok::Op(Op::LParen),
                    Tok::Word(vec![Seg::Lit("a".into())]),
                    Tok::Op(Op::Pipe),
                    Tok::Word(vec![Seg::Lit("b".into())]),
                    Tok::Op(Op::RParen),
                ],
                "extglob off: {src}"
            );
            let mut with = super::tokenize(src.as_bytes(), ParseOpts { extglob: true, posix: false }).unwrap();
            with.pop();
            assert_eq!(
                with,
                vec![
                    Tok::Word(vec![Seg::Lit("echo".into())]),
                    Tok::Word(vec![Seg::Lit(format!("{open}(a|b)").into_bytes())]),
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
    fn dollar_paren_paren_backtracks_to_a_substitution() {
        // `$((` is two constructs sharing a prefix. The reading commits to
        // arithmetic and only rewinds when the arithmetic scan fails to reach
        // its `))`, so what decides the shape is the *whole* text, not the
        // next character — same as bash's `parse_dollar_paren`.
        let seg = |src: &str| -> Seg {
            let toks = tokenize(src).unwrap();
            match &toks[1] {
                Tok::Word(segs) => segs[0].clone(),
                other => panic!("expected a word, got {other:?}"),
            }
        };

        // Arithmetic, because the scan reaches `))`.
        for src in [
            "echo $((1 + 2))",
            // A parenthesised sub-expression is still arithmetic.
            "echo $(((1 + 2) * 3))",
            // Nothing here is a valid *expression*, but the scan does reach
            // `))`, so it stays arithmetic and fails at evaluation — as it
            // does in bash. Reaching the end is the whole test.
            "echo $(( echo a ))",
        ] {
            assert!(
                matches!(seg(src), Seg::Arith(_, false)),
                "{src} should read as arithmetic, got {:?}",
                seg(src)
            );
        }

        // A substitution whose body opens with a group: the arithmetic scan
        // runs off the end, and the rewind re-reads from the inner `(`.
        for (src, body) in [
            (
                "echo $(( echo a; echo b ) | tr a-z A-Z)",
                "( echo a; echo b ) | tr a-z A-Z",
            ),
            ("echo $(( echo a ) && echo b)", "( echo a ) && echo b"),
            ("echo $(( echo a ); echo b)", "( echo a ); echo b"),
            ("echo $(( echo a ); ( echo b ))", "( echo a ); ( echo b )"),
        ] {
            match seg(src) {
                Seg::CmdSub(raw, ..) => assert_eq!(raw, body.as_bytes(), "body of {src}"),
                other => panic!("{src} should read as a substitution, got {other:?}"),
            }
        }

        // The rewind restores only `self.pos`, so a later construct on the
        // same input still lexes.
        let toks = tokenize("echo $(( echo a ) | cat) $((1 + 1))\necho done").unwrap();
        assert!(matches!(&toks[1], Tok::Word(s) if matches!(s[0], Seg::CmdSub(..))));
        assert!(matches!(&toks[2], Tok::Word(s) if matches!(s[0], Seg::Arith(_, false))));
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
                    assert_eq!(raw, b"echo $(echo x)");
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
        assert_eq!(segs, vec![Seg::Lit(b"line one\nline two\n".to_vec())]);
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
        assert_eq!(segs, vec![Seg::Lit(b"indented\n".to_vec())]);
    }

    #[test]
    fn here_string_op() {
        let toks = tokenize("cmd <<< word").unwrap();
        assert!(toks.iter().any(|t| matches!(t, Tok::Op(Op::TLess))));
    }

    /// A here-document delimiter is a whole shell word, so a `$( … )`,
    /// `$(( … ))`, `${ … }` or `` ` … ` `` in it is scanned as a matched pair
    /// and becomes part of the delimiter — separators and all — rather than
    /// stopping the scan. Nothing is expanded: the text goes on to be compared
    /// against each body line literally. Measured from bash 5.2; see
    /// [`Lexer::take_delim_group_at`].
    #[test]
    fn a_here_document_delimiter_takes_a_group_whole() {
        // (source, the delimiter word bash wants)
        let delims: &[(&str, &str)] = &[
            // The substitution is never run, only scanned, so the delimiter is
            // the text as written.
            ("cat <<E$(echo 1)\n", "E$(echo 1)"),
            // A separator inside the group is data: one delimiter, not `E$(a`
            // followed by a stray word `b)`.
            ("cat <<E$(a b)\n", "E$(a b)"),
            // Arithmetic closes on the second paren, not the first.
            ("cat <<E$((1+1))\n", "E$((1+1))"),
            ("cat <<E${y:-a b}\n", "E${y:-a b}"),
            ("cat <<E`a b`\n", "E`a b`"),
            // A group can begin the word and can be the whole of it.
            ("cat <<$(echo x)\n", "$(echo x)"),
            // Nested groups of every combination.
            ("cat <<E$(f $(g))\n", "E$(f $(g))"),
            ("cat <<E$(f `g`)\n", "E$(f `g`)"),
            ("cat <<E${x:-$(y)}\n", "E${x:-$(y)}"),
            // A `close` inside a quoted run is data…
            ("cat <<E$(echo \"a )b\")\n", "E$(echo \"a )b\")"),
            ("cat <<E$(echo 'a )b')\n", "E$(echo 'a )b')"),
            // …as is one behind a backslash.
            ("cat <<E$(echo \\))\n", "E$(echo \\))"),
            // A backquote pair never nests, so this closes on the second one and
            // the `c` that follows is a separate word.
            ("cat <<E`a `b c\n", "E`a `b"),
            // A group is not a quote — not even one with quotes inside it. Only
            // quoting the delimiter *itself* turns off expansion in the body,
            // which is why the parallel unquoted/quoted pair is here.
            ("cat <<E$(echo \"a b\")\n", "E$(echo \"a b\")"),
            ("cat <<\"E$(echo 1)\"\n", "E$(echo 1)"),
        ];
        for (src, want) in delims {
            let toks = tokenize(src).unwrap();
            let got = toks
                .iter()
                .find_map(|t| match t {
                    Tok::HereDoc(_, delim, _) => Some(delim.clone()),
                    _ => None,
                })
                .expect("here-doc token");
            assert_eq!(crate::bytes::as_str(&got), Some(*want), "{src:?}");
        }
    }

    /// A group in a here-document delimiter that never closes is fatal — the
    /// delimiter is *not* whatever was read up to there. Which line bash blames
    /// says which of its readers took the group: `${`, `` ` `` and `$((` are
    /// `parse_matched_pair`, which reports at the `start_lineno` it captured on
    /// entry, while `$(` alone is `parse_comsub`, which parses the body as a
    /// command and so reports where that parse died — the end of the input.
    /// Measured from bash 5.2.
    #[test]
    fn an_unclosed_group_in_a_here_document_delimiter_is_fatal() {
        // (source, the character named, the line blamed)
        let cases: &[(&str, char, u32)] = &[
            // `parse_comsub`: the end of the input, however far away it is.
            ("cat <<E$(\n", ')', 2),
            ("cat <<E$(\nbody\nE$(\n", ')', 4),
            ("cat <<E$(x\nbody\nE$(x\necho tail\n", ')', 5),
            // …and nesting does not move it, there being no `start_lineno` in
            // play at all.
            ("cat <<E$({\na\nb\nc\n", ')', 5),
            // `parse_matched_pair`: the opener's own line.
            ("cat <<E${\na\nb\nc\nd\ne\n", '}', 1),
            ("cat <<E`x\na\nb\n", '`', 1),
            ("cat <<E$((\na\nb\nc\n", ')', 1),
            // …and there nesting *does* move it, each level being a recursive
            // call with a `start_lineno` of its own.
            ("cat <<E${\nbody\nE${\n", '}', 3),
            ("cat <<E${\na\n${\nb\n", '}', 3),
            // A quoted run inside a group is a reader of its own too, and is
            // blamed on the quote's line rather than the group's.
            ("cat <<E$(echo \"a\nb\n", '"', 1),
            ("cat <<E$(echo 'a\nb\n", '\'', 1),
        ];
        for (src, close, line) in cases {
            let err = tokenize(src).expect_err(&format!("{src:?} must not lex"));
            let want = format!("unexpected EOF while looking for matching `{close}'");
            assert_eq!(crate::bytes::as_str(&err.msg), Some(want.as_str()), "{src:?}");
            assert_eq!(err.line, Some(*line), "{src:?}");
        }
    }

    /// The unterminated-here-document warnings of a scan, in order, as
    /// (delimiter, body line, EOF line). The other kind of reader warning is
    /// filtered out so a test that is about here-document lines does not have to
    /// spell out the enum.
    fn heredoc_eofs(tk: &Tokenized) -> Vec<(&str, u32, u32)> {
        tk.warnings
            .iter()
            .filter_map(|w| match w {
                // Every delimiter these tests use is ASCII, so reading it
                // back as text is exact; a non-text one would compare unequal
                // to any expectation here rather than being approximated.
                ReaderWarning::HeredocEof(h) => Some((
                    crate::bytes::as_str(&h.delim).unwrap_or_default(),
                    h.body_line,
                    h.eof_line,
                )),
                ReaderWarning::SubstHeredoc(_) => None,
            })
            .collect()
    }

    /// The two line numbers bash's unterminated-here-document warning carries are
    /// both "the last line the reader had **fetched**" — one at the moment body
    /// collection began, one at the moment the input ran out — and neither is the
    /// line the `<<` sits on. Every row below is measured from bash 5.2.
    #[test]
    fn unterminated_heredoc_records_both_warning_lines() {
        /// One expected warning: the delimiter wanted, the line named inside the
        /// message, and the line named in its prefix.
        type Want<'a> = (&'a str, u32, u32);
        // (source, the warnings it must produce, in order)
        let cases: &[(&str, &[Want<'_>])] = &[
            // The base shape. The body began after line 1 and input ran out on 2.
            ("cat <<EOF\nbody", &[("EOF", 1, 2)]),
            // A trailing newline adds no line: the cursor stops at a line
            // *boundary*, and the empty line past the end is never fetched.
            ("cat <<EOF\nbody\n", &[("EOF", 1, 2)]),
            // Conversely a source with no trailing newline still has a final
            // partial line, and that one *has* been fetched.
            ("echo hi\ncat <<EOF\nbody", &[("EOF", 2, 3)]),
            // An empty body: the operator's line is the last one either way, so
            // both numbers coincide — the case that proves `eof_line` is not
            // `body_line + 1`.
            ("echo hi\necho ho\ncat <<EOF", &[("EOF", 3, 3)]),
            // The second of two here-documents is blamed on wherever the *first*
            // one's body left the cursor, not on the shared operator line. This is
            // the shape that forces `body_line` to be captured when collection
            // begins rather than reconstructed from the `<<`.
            ("cat <<A <<B\none\nA\ntwo", &[("B", 3, 4)]),
            // Both unterminated: A stops at EOF mid-line 2, so B's body "begins"
            // there and is blamed on line 2 — while both share the same eof line.
            ("cat <<A <<B\none", &[("A", 1, 2), ("B", 2, 2)]),
            // The same with a trailing newline: A's body ends at a boundary, so B
            // is blamed on line 2 by the boundary rule instead.
            ("cat <<A <<B\none\n", &[("A", 1, 2), ("B", 2, 2)]),
            // A terminated here-document earlier in the input does not shift the
            // arithmetic — the numbers are absolute, not relative to the operator.
            ("cat <<EOF\nbody\nEOF\ncat <<X\nmore", &[("X", 4, 5)]),
            // A quoted delimiter is wanted in its *unquoted* spelling, and `<<-`
            // changes nothing about the lines.
            ("cat <<\"EOF\"\nbody", &[("EOF", 1, 2)]),
            ("cat <<-EOF\n\tbody", &[("EOF", 1, 2)]),
            // CRLF: the `\r` is insignificant whitespace and must not be counted
            // as a line of its own.
            ("cat <<EOF\r\nbody\r\n", &[("EOF", 1, 2)]),
            // A here-document inside a construct the input also cuts off still
            // reports its own lines; the syntax error is a separate diagnostic.
            ("if true; then\ncat <<EOF\nbody", &[("EOF", 2, 3)]),
            // Terminated: no record at all.
            ("cat <<EOF\nbody\nEOF\n", &[]),
        ];
        for (src, want) in cases {
            let tk = tokenize_deferred(src.as_bytes(), ParseOpts::default());
            let got: Vec<Want<'_>> = heredoc_eofs(&tk);
            assert_eq!(got, *want, "{src:?}");
        }
    }

    /// A here-document declared inside a `$( … )` or `<( … )` takes its body from
    /// the *enclosing* input, so the body lines — and not the first `)` after the
    /// operator — decide where the substitution ends. Every expectation is measured
    /// from bash 5.2. See [`Lexer::read_subst_body`].
    #[test]
    fn substitution_body_reaches_past_a_here_document() {
        // (source, the raw substitution body the scan must capture)
        let bodies: &[(&str, &str)] = &[
            // The common idiom: the `)` sits after the delimiter line, so the body
            // is everything up to it — which is what a naive paren scan also gets.
            ("x=$(cat <<EOF\nbody\nEOF\n)", "cat <<EOF\nbody\nEOF\n"),
            // Now the `)` is *inside* the here-document's body, so it is body text
            // and the substitution runs on to the next one.
            ("x=$(cat <<EOF\nbody\n); echo hi\nEOF\n)", "cat <<EOF\nbody\n); echo hi\nEOF\n"),
            // `<<-` and a quoted delimiter behave the same, and the delimiter word
            // must be copied *as written* so the re-lex draws the same conclusion
            // about expansion from it.
            ("x=$(cat <<-\"EOF\"\n\t)\n\tEOF\n)", "cat <<-\"EOF\"\n\t)\n\tEOF\n"),
            // Two here-documents in one body, collected in order at the newline.
            ("x=$(cat <<A <<B\n)\nA\n)\nB\n)", "cat <<A <<B\n)\nA\n)\nB\n"),
            // Places a `<<` can sit without being an operator. Each of these would
            // otherwise send the scan hunting for a delimiter that never comes and
            // swallow the rest of the input.
            ("x=$(cat <<<')'\n)", "cat <<<')'\n"),
            ("x=$(echo hi # <<Z\n)", "echo hi # <<Z\n"),
            ("x=$(echo $((1 << 3))\n)", "echo $((1 << 3))\n"),
            ("x=$(echo ${y:-<<Z}\n)", "echo ${y:-<<Z}\n"),
            // A backtick body is lexed on its own, so a here-document inside one is
            // that body's business and its closing backtick is found lexically.
            ("x=$(echo `cat <<Z` hi\n)", "echo `cat <<Z` hi\n"),
            // A backslash escapes the `)` rather than closing the substitution.
            ("x=$(echo \\)); echo hi", "echo \\)"),
            // Process substitution reads its body the same way.
            ("cat <(cat <<EOF\n)\nEOF\n)", "cat <<EOF\n)\nEOF\n"),
        ];
        for (src, want) in bodies {
            let tk = tokenize_deferred(src.as_bytes(), ParseOpts::default());
            assert!(tk.err.is_none(), "{src:?} should lex: {:?}", tk.err);
            let raw = tk
                .toks
                .iter()
                .find_map(|t| match t {
                    Tok::Word(segs) => segs.iter().find_map(|s| match s {
                        Seg::CmdSub(raw, _, None) | Seg::ProcSub(_, raw, _) => Some(raw.clone()),
                        _ => None,
                    }),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no substitution segment in {src:?}: {:?}", tk.toks));
            assert_eq!(raw, want.as_bytes(), "{src:?}");
        }
        // When the body runs to end of input the substitution never closes, and
        // bash reports the unmatched `)` — one line past the last, as it does for
        // every unterminated `$( … )`. The here-document warning is recorded too,
        // in the *enclosing* line numbers, and comes out first.
        for src in ["x=$(cat <<EOF\nbody\n); echo hi", "cat <(cat <<EOF\nbody\n); echo hi"] {
            let tk = tokenize_deferred(src.as_bytes(), ParseOpts::default());
            let (e, line) = tk.err.as_ref().unwrap_or_else(|| panic!("{src:?} must fail"));
            assert_eq!(e.msg, b"unexpected EOF while looking for matching `)'");
            assert_eq!(*line, 4, "{src:?}");
            assert_eq!(heredoc_eofs(&tk), vec![("EOF", 1, 3)], "{src:?}");
        }
    }

    /// A here-document whose `<<` and whose substitution's `)` share a line has
    /// nowhere inside the substitution to put its body. bash's reader fetches the
    /// following lines anyway — **at the `)`**, not at the enclosing newline — and
    /// warns that it had to. Every expectation is measured from bash 5.2. See
    /// [`Lexer::gather_ahead`].
    #[test]
    fn a_substitution_gathers_a_here_document_from_past_its_close() {
        /// The raw text of each substitution in the input, then the
        /// `(count, line)` of each `command substitution:` warning raised.
        type Want<'a> = (&'a [&'a str], &'a [(usize, u32)]);
        let cases: &[(&str, Want<'_>)] = &[
            // The base shape: the body sits after the line the `)` is on, and
            // the substitution's text ends up holding it all the same — with a
            // newline spliced in, since the scan stopped at `)` and never
            // reached one to terminate the command with.
            (
                "x=$(cat <<EOF); echo hi\nbody\nEOF\n",
                (&["cat <<EOF\nbody\nEOF\n"], &[(1, 1)]),
            ),
            // Two of them are one warning naming both, and the bodies follow in
            // declaration order.
            (
                "x=$(cat <<A; cat <<B); echo hi\naaa\nA\nbbb\nB\n",
                (&["cat <<A; cat <<B\naaa\nA\nbbb\nB\n"], &[(2, 1)]),
            ),
            // Two substitutions on one line take successive pairs of lines: the
            // second gather resumes where the first stopped rather than
            // re-reading from the line being parsed. That is also why the second
            // warning is blamed on line 3 — the reader was already there.
            (
                "x=$(cat <<A) $(cat <<C); echo hi\naaa\nA\nccc\nC\n",
                (&["cat <<A\naaa\nA\n", "cat <<C\nccc\nC\n"], &[(1, 1), (1, 3)]),
            ),
            // A nested substitution's here-document is fetched at the *nested*
            // `)`, which is where bash's one shared reader reaches it, and the
            // body is spliced in there — so the copied text holds it inline and
            // the re-lex has nothing left to gather or to warn about.
            (
                "x=$(echo $(cat <<A) tail); echo hi\naaa\nA\n",
                (&["echo $(cat <<A\naaa\nA\n) tail"], &[(1, 1)]),
            ),
            // Both levels at once: the inner gather runs first and leaves the
            // reader past B's body, so the outer warning names line 5 and not
            // line 1. See TD-OILS-CMDSUB-HEREDOC-NESTED-GATHER-ORDER.
            (
                "x=$(cat <<A; echo $(cat <<B) mid); echo hi\naaa\nA\nbbb\nB\nA\n",
                (
                    &["cat <<A; echo $(cat <<B\naaa\nA\nbbb\nB\n) mid\nA\n"],
                    &[(1, 1), (1, 5)],
                ),
            ),
        ];
        for (src, (raws, warned)) in cases {
            let tk = tokenize_deferred(src.as_bytes(), ParseOpts::default());
            assert!(tk.err.is_none(), "{src:?} should lex: {:?}", tk.err);
            let got: Vec<crate::bytes::Str> = tk
                .toks
                .iter()
                .filter_map(|t| match t {
                    Tok::Word(segs) => Some(segs),
                    _ => None,
                })
                .flatten()
                .filter_map(|s| match s {
                    Seg::CmdSub(raw, _, None) => Some(raw.clone()),
                    _ => None,
                })
                .collect();
            let want: Vec<crate::bytes::Str> =
                raws.iter().map(|r| r.as_bytes().to_vec()).collect();
            assert_eq!(got, want, "{src:?}");
            let got: Vec<(usize, u32)> = tk
                .warnings
                .iter()
                .filter_map(|w| match w {
                    ReaderWarning::SubstHeredoc(s) => Some((s.count, s.line)),
                    ReaderWarning::HeredocEof(_) => None,
                })
                .collect();
            assert_eq!(got, *warned, "{src:?}");
            // The fetched lines were consumed by the gather, so nothing on them
            // is tokenized a second time as a command of the enclosing input.
            assert_eq!(
                tk.toks.iter().filter(|t| matches!(t, Tok::Newline)).count(),
                1,
                "{src:?} must yield one logical line: {:?}",
                tk.toks
            );
        }

        // The reader ends up ahead of the cursor, and a token's line is the last
        // line the *reader* has fetched — so the rest of the substitution's line
        // is stamped with the body's last line, and the next line follows from
        // there. (`$LINENO` reports 3 and 4 for this input, measured.)
        let src = "x=$(cat <<EOF); echo hi\nbody\nEOF\necho ho\n";
        let tk = tokenize_deferred(src.as_bytes(), ParseOpts::default());
        let after: Vec<u32> = tk
            .toks
            .iter()
            .zip(&tk.lines)
            .filter(|(t, _)| {
                matches!(t, Tok::Word(w)
                    if matches!(w.first(), Some(Seg::Lit(l)) if l == b"hi" || l == b"ho"))
            })
            .map(|(_, &l)| l)
            .collect();
        assert_eq!(after, vec![3, 4]);
    }

    /// A `case` pattern's `)` has no opening mate, so the extent scan cannot end
    /// a substitution on a bare count of parentheses — it has to know where the
    /// reserved words `case`, `in` and `esac` are reserved, which is command
    /// position and only there.
    #[test]
    fn substitution_body_ends_at_the_grammars_close_paren() {
        // (source, the raw substitution body the scan must capture)
        let bodies: &[(&str, &str)] = &[
            // The shape that motivates all of this.
            (
                "x=$(case b in a) echo A;; b) echo B;; esac)",
                "case b in a) echo A;; b) echo B;; esac",
            ),
            // A pattern's optional `(` takes no depth of its own: the pattern's
            // `)` is what closes it.
            (
                "x=$(case b in (a) echo A;; (b) echo B;; esac)",
                "case b in (a) echo A;; (b) echo B;; esac",
            ),
            // A bare `;` does not end a clause body, so `esac` after one is still
            // the reserved word; `;&` and `;;&` do end one.
            ("x=$(case b in b) echo B; esac)", "case b in b) echo B; esac"),
            (
                "x=$(case b in b) echo B;& c) echo C;; esac)",
                "case b in b) echo B;& c) echo C;; esac",
            ),
            (
                "x=$(case b in b) echo B;;& b*) echo B2;; esac)",
                "case b in b) echo B;;& b*) echo B2;; esac",
            ),
            // `esac` is reserved wherever a pattern could start, so an empty
            // `case` ends at once.
            ("x=$(case b in esac)", "case b in esac"),
            // Two `case`s can share a depth, so the frames are a stack and not
            // keyed by it.
            (
                "x=$(case b in b) case c in c) echo d;; esac;; esac)",
                "case b in b) case c in c) echo d;; esac;; esac",
            ),
            // A group inside a clause body is still a group.
            ("x=$(case b in b) (echo s);; esac)", "case b in b) (echo s);; esac"),
            // …and a `)` that is not the grammar's at all: quoted, or an extglob
            // pattern's, which closes itself.
            (
                "x=$(case \")\" in \")\") echo p;; esac)",
                "case \")\" in \")\") echo p;; esac",
            ),
            ("x=$(case b in @(a|b)) echo X;; esac)", "case b in @(a|b)) echo X;; esac"),
            // Command position is what makes a word reserved: none of these three
            // `case`s is one, so the first `)` still closes the substitution.
            ("x=$(printf %s case in f); echo hi", "printf %s case in f"),
            ("x=$(echo a case b in c); echo hi", "echo a case b in c"),
            ("x=$(echo $((1 + 1)) case); echo hi", "echo $((1 + 1)) case"),
            // Quoting is the other thing that unmakes a reserved word.
            (
                "x=$(case b in b) echo \"esac\";; esac)",
                "case b in b) echo \"esac\";; esac",
            ),
            // Every place a command position arises has to be one.
            ("x=$(true | case b in b) echo p;; esac)", "true | case b in b) echo p;; esac"),
            (
                "x=$(for i in 1 2; do case $i in 1) echo o;; esac; done)",
                "for i in 1 2; do case $i in 1) echo o;; esac; done",
            ),
            (
                "x=$(f() { case b in b) echo fn;; esac; }; f)",
                "f() { case b in b) echo fn;; esac; }; f",
            ),
            // Layout: the `in` and the patterns can be on their own lines, and a
            // comment can sit between them.
            ("x=$(case b\nin\nb) echo B;;\nesac)", "case b\nin\nb) echo B;;\nesac"),
            (
                "x=$(case b in # c\nb) echo B;; esac)",
                "case b in # c\nb) echo B;; esac",
            ),
            // A here-document in a clause body still takes its body from the
            // enclosing input.
            (
                "x=$(case b in b) cat <<EOF\nhd\nEOF\n;; esac)",
                "case b in b) cat <<EOF\nhd\nEOF\n;; esac",
            ),
            // Process substitution reads its body the same way.
            ("cat <(case b in b) echo B;; esac)", "case b in b) echo B;; esac"),
        ];
        // `@(a|b)` is only a pattern with `extglob` on.
        let opts = ParseOpts { extglob: true, posix: false };
        for (src, want) in bodies {
            let tk = tokenize_deferred(src.as_bytes(), opts);
            assert!(tk.err.is_none(), "{src:?} should lex: {:?}", tk.err);
            let raw = tk
                .toks
                .iter()
                .find_map(|t| match t {
                    Tok::Word(segs) => segs.iter().find_map(|s| match s {
                        Seg::CmdSub(raw, _, None) | Seg::ProcSub(_, raw, _) => Some(raw.clone()),
                        _ => None,
                    }),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no substitution segment in {src:?}: {:?}", tk.toks));
            assert_eq!(raw, want.as_bytes(), "{src:?}");
        }
        // A `case` still open where the substitution closes: the `)` is where
        // bash's parser wanted `;;` or `esac`, so it goes *into* the body for the
        // body parse to name — which also makes the failure the substitution's
        // (exit 1) rather than the enclosing input's (exit 2).
        let tk = tokenize_deferred("x=$(case b in b) echo B); echo hi".as_bytes(), ParseOpts::default());
        assert!(tk.err.is_none(), "{:?}", tk.err);
        let raw = tk
            .toks
            .iter()
            .find_map(|t| match t {
                Tok::Word(segs) => segs.iter().find_map(|s| match s {
                    Seg::CmdSub(raw, _, None) => Some(raw.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .expect("a substitution segment");
        assert_eq!(raw, b"case b in b) echo B)");
    }
}
