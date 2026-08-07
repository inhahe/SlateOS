//! Recursive-descent parser: tokens → [`ast::Program`].
//!
//! The parser also lowers lexer [`Seg`]s into [`ast::WordPart`]s, recursively
//! parsing command/parameter substitutions (their raw inner source is captured
//! by the lexer).
//!
//! # Text vs. bytes
//!
//! Shell *source* is bytes: a script may contain any byte, and a word in it may
//! name a file whose name is not UTF-8. So everything that flows from the source
//! into the AST — literals, here-doc delimiters, substitution bodies, function
//! names — is [`Str`], and the source itself arrives as [`BStr`].
//!
//! Two things stay `String`, deliberately:
//!
//! * **Names.** Variable, alias and reserved-word namespaces are text by
//!   construction (`[A-Za-z_][A-Za-z0-9_]*`), so a name that is not UTF-8 is not
//!   a name at all. Those sites go through [`bytes::as_str`] and reject — or
//!   defer to a run-time "bad substitution" — when it returns `None`, which is
//!   the honest answer rather than a mangled approximation.
//! * **Diagnostics.** [`ParseError::msg`] is still `String`; the word being
//!   complained about is rendered by [`shown`]. Step 10 of TD-OILS-BYTE-STRINGS
//!   converts the diagnostic layer and deletes that helper.
//!
//! Syntax comparisons scan [`Ch`] but are written against an ASCII `char` view
//! ([`syn`]/[`syn_at`], mirroring the lexer's), so a non-ASCII character or a
//! stray byte reads as `'\0'` and matches no operator.

use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseClause, CaseItem,
    CaseTerm, CmdSubBody,
    Command,
    CondBinOp, CondBinary, CondUnary, DeclArray,
    CondExpr, ForArithClause, ForClause, FunctionDef, HereDoc, IfClause, Item, LineMap,
    LoopClause,
    ParamOp,
    Pipeline, Program,
    Redirect, RedirectOp, ReplaceAnchor, SelectClause, SimpleCommand, Word, WordPart,
};
use crate::assoc::AssocArray;
use crate::bfmt;
use crate::bytes::{self, BStr, Ch, Str};
use crate::lexer::{
    AliasExpansion, HeredocEof, HeredocWant, ParseOpts, Op, ReaderWarning, Seg, Spanned, Tok,
    TokSpan, Tokenized, UngatheredHeredoc,
    expand_aliases_tracked, gather_heredocs_at, tokenize,
    tokenize_paren_body, tokenize_deferred, tokenize_spanned, word_is_assignment,
};

/// Whether `s` is a syntactically valid shell identifier.
///
/// Re-exported from the lexer, which owns the definition, so the rest of the
/// crate can keep asking the parser (its historical home) and get one answer.
pub(crate) use crate::lexer::is_valid_name;

/// A shell identifier as text, or `None` when the bytes are not one.
///
/// The variable namespace is ASCII by construction, so a word that is not valid
/// UTF-8 cannot name a variable. Callers reach this having already checked the
/// identifier rule, so `None` is unreachable in practice — but it is the honest
/// return for bytes that are not text, and every caller has a sensible
/// not-an-assignment / not-an-identifier answer for it.
pub(crate) fn name_text(name: BStr<'_>) -> Option<String> {
    bytes::as_str(name).map(str::to_owned)
}

/// The ASCII syntax view of a scanned character, mirroring `lexer::syn`.
///
/// Shell syntax is entirely ASCII, so a non-ASCII character — or a byte that is
/// not part of any character — cannot *be* an operator. Reading it as `'\0'`
/// lets the syntax tests stay written as ordinary `char` comparisons while
/// guaranteeing that no such byte is ever mistaken for punctuation.
fn syn(c: Ch) -> char {
    match c {
        Ch::U(c) if c.is_ascii() => c,
        _ => '\0',
    }
}

/// [`syn`] of the character at `i`, or `'\0'` past the end — so a lookahead
/// never needs a length check of its own.
fn syn_at(chs: &[Ch], i: usize) -> char {
    chs.get(i).copied().map_or('\0', syn)
}

/// A parse error with a human-readable message and, when known, the 1-based
/// source line the error occurred on.
///
/// `line` mirrors the number bash prints in `<name>: line N:`: for a
/// grammar error it is the line of the offending token; for an
/// unexpected-end-of-file error it is one past the last token's line (bash's
/// EOF quirk). It is stamped centrally in [`parse_tokens`] and is `None` for
/// lexer-originated errors (unclosed quotes/substitutions), where the caller
/// falls back to the current execution line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// The message, as bytes. Most are fixed text, but the ones that name the
    /// offending construct quote a *shell word* back — the token in `syntax
    /// error near unexpected token \`…'`, or the name in `\`…': not a valid
    /// identifier` — and a shell word may hold any byte. The word therefore
    /// goes back out as the bytes the user wrote rather than through a decode
    /// that would rewrite the very text being blamed.
    pub msg: Str,
    pub line: Option<u32>,
    /// True when the error was found inside a `$( … )` or `<( … )` body, which
    /// bash treats as fatal to whatever was reading that body.
    ///
    /// bash parses a substitution body by recursing into the parser mid-word,
    /// and a failure there has no grammar-level way back — it calls
    /// `jump_to_top_level(DISCARD)` after `top_level_cleanup()`, so the unwind
    /// runs *past* the handler `eval` and `.`/`source` install and takes the
    /// shell (or the enclosing subshell) with it:
    ///
    /// ```sh
    /// echo A; eval 'echo $( ! )'; echo B    # bash prints A and the error, then exits
    /// echo A; eval 'for';         echo B    # an ordinary syntax error: B still runs
    /// ```
    ///
    /// A backtick body is *not* marked: bash does not parse one until the word
    /// is expanded, so its failure is an expansion-time diagnostic that leaves
    /// the substitution empty and the command running (see
    /// TD-OILS-CMDSUB-ERR-FATALITY item 1, still open).
    pub fatal: bool,
    /// True when the failure costs only the parse unit that holds it: the shell
    /// reports it, scores `$?` **1**, and goes on reading the next unit — where
    /// an ordinary syntax error scores 2 and abandons the rest of the input.
    ///
    /// This is bash's *reader* speaking rather than its grammar. Only a malformed
    /// array literal (`a=(x; y)`, or an unterminated `a=(x`) reaches it: bash's
    /// reader collects the balanced `( … )` before anything looks inside, so a
    /// failure there has a natural place to resume — just past the literal —
    /// which a grammar error does not.
    ///
    /// ```sh
    /// a=(x; y); echo after=$?   # nothing on this unit runs …
    /// echo next=$?              # … but this one does, and sees 1
    /// ```
    ///
    /// The *unit* is what dies, not the line: `echo one; a=(x; y)` never prints
    /// `one`, because `;` chains it into the same unit.
    pub recoverable: bool,
    /// The lines of `msg` that are *not* reported at `line`, as
    /// `(index of the line in `msg`, the line to report it at)`.
    ///
    /// Empty for almost every error, because a diagnostic is normally reported
    /// wholly at one place. bash does not promise that, though: it prints a
    /// multi-line diagnostic from several frames, each passing its *own*
    /// `line_number` to `parser_error`. A conditional failing inside `( … )` is
    /// the case that shows it — every enclosing group contributes an
    /// `expected \`)'` line reported at the line its own `(` was on, which is
    /// not the line the failure was found on:
    ///
    /// ```sh
    /// [[ ( a &&
    /// b ; ]]        # line 2: unexpected token `;', …
    ///               # line 1: expected `)'
    ///               # line 2: syntax error near `;'
    /// ```
    ///
    /// The lines here are absolute and already mapped ([`LineMap`]), the same
    /// as `line`.
    pub line_at: Vec<(u32, u32)>,
    /// The text to echo under a `syntax error near …`, when it is not the
    /// physical source line that `line` names.
    ///
    /// bash echoes `shell_input_line`, and an alias makes that something other
    /// than a line of the script: `push_string` sets `shell_input_line` to the
    /// replacement while it is being read. So an error found inside one is
    /// echoed as the replacement, at the line number the alias word was on:
    ///
    /// ```text
    /// alias A="[[ P;Q"
    /// A ]]          line 2: syntax error near `;Q'
    ///               line 2: `[[ P;Q'
    /// ```
    ///
    /// `None` — the overwhelmingly common case — leaves the caller to echo the
    /// script's own line, which is all it can do for a text it does not hold.
    pub echo: Option<Str>,
}

impl ParseError {
    /// A parse error with no known line (the common construction site inside
    /// the grammar; [`parse_tokens`] stamps the line afterwards).
    pub fn new(msg: &(impl bytes::PushBytes + ?Sized)) -> Self {
        Self {
            msg: bfmt![msg],
            line: None,
            fatal: false,
            recoverable: false,
            line_at: Vec::new(),
            echo: None,
        }
    }

    /// Where the `i`th line of `msg` is reported, given the error's own line.
    /// See [`Self::line_at`].
    #[must_use]
    pub fn line_of(&self, i: usize, line: u32) -> u32 {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        self.line_at
            .iter()
            .find_map(|&(at, l)| (at == i).then_some(l))
            .unwrap_or(line)
    }

    /// Mark this error as costing only its own parse unit. See [`Self::recoverable`].
    fn only_this_unit(mut self) -> Self {
        self.recoverable = true;
        self
    }

    /// Mark this error as raised from inside a `$( … )`/`<( … )` body, so the
    /// caller unwinds instead of merely recording status 2. See [`Self::fatal`].
    fn in_paren_body(mut self) -> Self {
        self.fatal = true;
        self
    }

    /// Attach a source line, but only if one is not already set — so an inner
    /// site that knows its precise line wins over the central fallback.
    fn or_line(mut self, line: u32) -> Self {
        if self.line.is_none() {
            self.line = Some(line);
        }
        self
    }

    /// Attach the text to echo instead of the source line, if there is one and
    /// none is set. See [`Self::echo`].
    fn or_echo(mut self, text: Option<Str>) -> Self {
        if self.echo.is_none() {
            self.echo = text;
        }
        self
    }

    /// True when this error is caused by the input *ending before a construct
    /// was closed* — an unterminated quote/substitution (`echo "…`, `$(…`), an
    /// unfinished compound command (`if …` with no `fi`, `{ …` with no `}`), or
    /// a line ending on a binary operator (`&& `, `| `). Supplying more input
    /// could complete the command, so the interactive REPL keeps reading
    /// continuation lines (PS2) instead of reporting a syntax error on a command
    /// the user is still typing. A genuine syntax error that more input cannot
    /// fix (e.g. a stray `)`) returns `false`.
    ///
    /// This keys off bash's two canonical end-of-input diagnostics, which are
    /// the *only* messages produced when the parser or lexer runs out of tokens
    /// while still expecting more: `unexpected end of file` (grammar reached
    /// EOF) and `unexpected EOF while looking for …` (lexer hit EOF inside an
    /// open quote/substitution). Every other diagnostic names an offending
    /// token and is not continuable.
    #[must_use]
    pub fn is_incomplete(&self) -> bool {
        bytes::contains(&self.msg, b"unexpected end of file")
            || bytes::contains(&self.msg, b"unexpected EOF while looking for")
    }
}

/// Carry a lexer error's line across into the parser's error type.
///
/// Only correct for lexers run over a *whole source*, where the line the lexer
/// counted is the line the caller will print. The fragment lexers
/// ([`crate::lexer::lex_word_verbatim`] and friends) re-lex a substring whose
/// line numbers restart at 1, so those sites drop the line instead and let the
/// enclosing parse stamp its own.
impl From<crate::lexer::LexError> for ParseError {
    fn from(e: crate::lexer::LexError) -> Self {
        Self {
            msg: e.msg,
            line: e.line,
            fatal: false,
            recoverable: e.recoverable,
            line_at: Vec::new(),
            echo: None,
        }
    }
}

/// Parse shell source into a [`Program`].
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse(src: BStr<'_>) -> Result<Program, ParseError> {
    parse_opts(src, ParseOpts::default())
}

/// Parse shell source under explicit lexing options.
///
/// Some shell options change how source is *read* rather than how it runs:
/// `extglob` decides whether `@(` opens an extended-pattern group or is an
/// ordinary character followed by a subshell. Such an option has to be known
/// before the text is tokenized, which is why it travels with the source rather
/// than being consulted at run time. [`parse`] uses bash's non-interactive
/// defaults.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse_opts(src: BStr<'_>, opts: ParseOpts) -> Result<Program, ParseError> {
    // The reader's NUL removal happens ahead of the lexer here too, so that the
    // REPL's "is this line complete yet?" probes judge the same text the run
    // will parse (see [`crate::lexer::strip_nuls`]).
    let src = crate::lexer::strip_nuls(src);
    let Spanned { toks, lines, ends } =
        tokenize_spanned(&src, opts).map_err(ParseError::from)?;
    parse_tokens(toks, lines, Spans::of(&src, ends), opts)
}

/// Parse the body of a `<( … )` / `>( … )` process substitution.
///
/// Like a `$( … )` body, it is read in the enclosing token stream, so what
/// follows its last command is the construct's own `)` — which ends the body's
/// list but is not a `list_terminator`. `<( ! )`, `<( time )` and `<(for)` are
/// therefore syntax errors named on that `)`. See [`tokenize_paren_body`].
///
/// `open_line` is the 1-based line the `<(`/`>(` sits on, in the enclosing
/// source. The body is lexed on its own and so numbers its lines from 1; bash
/// blames an error in it on the line it is *written* on, so the body's numbering
/// is shifted back into the enclosing source's. That is a plain offset, unlike
/// [`parse_cmdsub_body`]'s rank-based renumbering — a process substitution runs
/// as a child command, not as a body bash re-reads after the enclosing scan.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error in the body.
pub fn parse_procsub_body(
    src: BStr<'_>,
    open_line: u32,
    opts: ParseOpts,
) -> Result<Program, ParseError> {
    let Spanned { mut toks, mut lines, ends } = tokenize_paren_body(src, opts)
        .map_err(|e| ParseError::from(e).in_paren_body())?;
    map_lines(&mut toks, &mut lines, &LineMap::Offset(open_line.saturating_sub(1)));
    parse_tokens_ending(toks, lines, Spans::of(src, ends), opts, true)
        .map_err(ParseError::in_paren_body)
}

/// Parse shell source with strict here-document lexing: an unterminated
/// here-document (delimiter never reached before EOF) is reported as an
/// incomplete-input [`ParseError`] rather than leniently accepted. Used only by
/// the interactive REPL's [`crate::Shell::parse_incomplete`] check so a here-doc
/// body typed across continuation lines keeps prompting until its delimiter.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error (including an
/// unterminated here-document).
pub fn parse_strict_heredoc(src: BStr<'_>, opts: ParseOpts) -> Result<Program, ParseError> {
    let Spanned { toks, lines, ends } =
        crate::lexer::tokenize_spanned_strict(src, opts).map_err(ParseError::from)?;
    parse_tokens(toks, lines, Spans::of(src, ends), opts)
}

/// Parse shell source, expanding shell aliases over the token stream first.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse_with_aliases(
    src: BStr<'_>,
    aliases: &AssocArray,
    opts: ParseOpts,
) -> Result<Program, ParseError> {
    let Spanned { toks, lines, ends } = tokenize_spanned(src, opts).map_err(ParseError::from)?;
    // An alias splices in tokens that were never written where they are being
    // read — but they *were* written somewhere, in the alias's own value, which
    // bash pushes onto the input and reports errors against. So the expansion
    // carries its replacement texts along and they become further sources.
    let (toks, lines, spans) = if aliases.is_empty() {
        (toks, lines, Spans::of(src, ends))
    } else {
        let x = expand_aliases_tracked(&toks, &lines, &ends, aliases, opts);
        let spans = Spans::expanded(bytes::chars(src).collect(), &x);
        (x.toks, x.lines, spans)
    };
    parse_tokens(toks, lines, spans, opts)
}

/// A resumable top-level parse: hands back one *parse unit* at a time so the
/// caller can execute it before the next unit is parsed.
///
/// bash reads, parses, and executes a script one complete command at a time, and
/// that ordering is observable in two ways this models:
///
/// * **Alias state is read at parse time.** `shopt -s expand_aliases` (or a
///   plain `alias foo=…`) executed on one line affects how *later* lines parse,
///   which is the only way a non-interactive script can use aliases at all.
///   Parsing the whole script up front freezes the alias decision before the
///   first command runs, so the `shopt` could never take effect.
/// * **A syntax error does not un-run earlier commands.** `echo hi` followed by
///   a malformed line prints `hi` and *then* reports the error (verified against
///   bash 5.2), because the good command was already executed when the bad one
///   was read.
///
/// A **unit** is everything up to and including a terminating newline — i.e. one
/// logical line, which may hold several `;`/`&`-separated commands and may span
/// physical lines when a compound command or a `&&`/`|` continuation does. bash
/// uses the same granularity: in `echo one; echo two )` nothing runs, because
/// the whole line failed to parse, and `alias a=b; a` does not expand `a`.
///
/// * **Lexing options are read at parse time too.** `shopt -s extglob` changes
///   how `@(` is *tokenized*, so like the alias state it can only affect lines
///   read after it runs. When the caller reports a change, the unconsumed tail
///   of the source is thrown away and lexed again from the character offset the
///   next token starts at.
///
/// The caller supplies the alias state and the lexing options per unit (`None`
/// when `expand_aliases` is off). The stream is re-expanded from the *original*
/// tokens whenever the alias state changes, so already-expanded text is never
/// expanded twice.
/// What a top-level physical line of a parse unit is made of.
///
/// The command history keeps a unit's lines verbatim but joins them with a
/// separator that depends on this — and drops two of the four kinds outright.
/// See [`UnitLine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitLineKind {
    /// Whitespace only. Never recorded: bash's history skips a blank line
    /// whether it stands between commands or inside one.
    Blank,
    /// A comment and nothing else. Dropped from a multi-line entry — though it
    /// still forces the following line onto a line of its own — and recorded as
    /// an entry in its own right when it precedes any command.
    Comment,
    /// The body of a here-document introduced by the previous line, delimiter
    /// line and trailing newline included. The lexer swallows it whole, so it
    /// carries no tokens of its own.
    HereDocBody,
    /// A line carrying at least one token.
    Code,
}

/// One top-level physical line of a parse unit, as the command history needs
/// it: bash records a multi-line command as a single entry whose lines are
/// rejoined with `"; "`, `" "` or `"\n"` depending on what the previous line
/// ended with.
///
/// "Top-level" means separated by a [`Tok::Newline`] — a newline the *parser*
/// sees. A newline inside a quoted string, a `$( … )` body or a here-document
/// body is part of a token and stays inside `text` untouched, which is exactly
/// how bash stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitLine {
    /// The line's source text, without its terminating newline.
    pub text: Str,
    pub kind: UnitLineKind,
    /// The line ends in an unquoted `#` comment. bash cannot append `"; "` to
    /// such a line — the next command would be swallowed by the comment — so it
    /// breaks the entry onto a new line instead.
    pub comment: bool,
    /// The line introduces a here-document (`<<`/`<<-`), so the next line is
    /// its body and must start on a line of its own.
    pub heredoc: bool,
    /// The line's last token is one a `;` cannot follow: `;`, `;;`, `;&`,
    /// `;;&`, `&`, `&&`, `|`, `|&`, `||`, `(`, `{`, or a reserved word that
    /// expects a command (`do`, `then`, `else`, `elif`, `in`).
    pub open: bool,
}

/// One physical input line as it is about to be read, offered to a `!`-style
/// history expander by [`IncrementalParser::peek_raw_line`].
///
/// Unlike [`UnitLine`] this has nothing to do with parse units: it is a raw
/// slice of the source between newlines, handed out *before* the line is lexed,
/// because that is when bash performs history expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    /// The line's source text, without its terminating newline.
    pub text: Str,
    /// The line's number after the parser's line map, for a diagnostic.
    pub line: u32,
}

/// What a finished [`IncrementalParser::gather_alias_heredocs`] pass has to put
/// back once the next pass has rebuilt `work` from `orig` again.
///
/// The gather changes tokens the *expansion* does not know how to produce: the
/// here-document bodies it read out of the real input, and the lines a value's
/// tail is stamped with after the reader moved. Re-expanding the settled prefix
/// yields the same tokens at the same indices, so replaying these by index is
/// sound. See [`IncrementalParser::rebuild`].
#[derive(Default)]
struct AliasFills {
    /// `work` index → the collected here-document token and the lines it took.
    bodies: Vec<(usize, Tok, u32)>,
    /// `work` index → the source line to stamp on it.
    lines: Vec<(usize, u32)>,
}

pub struct IncrementalParser {
    /// The source, kept for re-lexing the tail when [`ParseOpts`] change. Held as
    /// characters because the offsets recorded by the lexer are char indices —
    /// [`Ch`], so a byte that is not part of any character still counts as one
    /// position and survives a round trip.
    src: Vec<Ch>,
    /// Applied to every line the lexer reports, so a fragment lexed on its own
    /// still names the lines of the input it came from.
    line_map: LineMap,
    /// The options `orig` was lexed under.
    opts: ParseOpts,
    /// The tokenized source exactly as lexed — never alias-expanded, so it can
    /// be re-expanded from any point under new alias state.
    orig: Vec<Tok>,
    orig_lines: Vec<u32>,
    /// Parallel to `orig`: the char offset into `src` each token starts at.
    orig_offsets: Vec<u32>,
    /// Parallel to `orig`: the char offset into `src` just past each token. A
    /// `Newline` that swallowed a here-document body ends past the body, so a
    /// unit's source is always `src[hist_cursor .. orig_ends[last consumed]]`.
    orig_ends: Vec<u32>,
    /// Ascending char offsets into `src` of the `\` of every `\<newline>` the
    /// lexer joined away. bash's history stores the *joined* line, so
    /// [`Self::line_text`] cuts these spans back out — see [`Tokenized::conts`].
    orig_conts: Vec<u32>,
    /// Parallel to `orig`: how many input lines collecting each here-document's
    /// body consumed. See [`Tokenized::heredoc_lines`] and
    /// [`Self::heredoc_gather`].
    orig_heredoc_lines: Vec<u32>,
    /// Here-documents the lex never reached the body of, because an unclosed
    /// construct after the `<<` swallowed the rest of the input. See
    /// [`UngatheredHeredoc`] and [`Self::ungathered_warnings`].
    orig_ungathered: Vec<UngatheredHeredoc>,
    /// Char offset into `src` of the first character not yet handed out as unit
    /// text. Runs ahead of `pos` only in whole tokens, so it survives a
    /// [`Self::relex`] (which rebases offsets onto the same `src`).
    hist_cursor: usize,
    /// Char offset into `src` of the first character not yet offered to the
    /// caller's history expander by [`Self::peek_raw_line`]. Advances a physical
    /// line at a time, so it runs *ahead* of `hist_cursor` while the lines of a
    /// still-incomplete unit are being expanded; it falls behind again whenever
    /// the parser swallows text no line-reader saw (a here-document body), which
    /// is why the frontier is the greater of the two.
    expand_cursor: usize,
    /// The top-level lines of the unit [`Self::next_unit`] last returned, for
    /// the caller's command history. Empty when the unit consumed no original
    /// token (an alias splice replaying tokens already recorded).
    unit_lines: Vec<UnitLine>,
    /// The *raw* source span the unit [`Self::next_unit`] last returned occupies
    /// — every physical line of it, including the comment and blank lines before
    /// it, any here-document body it swallowed, and the `\<newline>` joins
    /// [`Self::unit_lines`] cuts out. `set -v` echoes exactly this.
    unit_raw: Str,
    /// `orig[pos..]` alias-expanded under `last_aliases`, prefixed by any
    /// alias-spliced tokens carried across the last rebuild.
    work: Vec<Tok>,
    work_lines: Vec<u32>,
    /// Parallel to `work`: the `orig` index each token came from, or `None` for
    /// a token an alias spliced in (which has no original counterpart).
    work_origin: Vec<Option<usize>>,
    /// Parallel to `work`: for an alias-spliced `HereDoc`, the input lines its
    /// body took; 0 for every other token. The `orig`-side counterpart is
    /// `orig_heredoc_lines`, which such a token has no slot in. See
    /// [`Self::heredoc_gather`].
    work_heredoc_lines: Vec<u32>,
    /// Indices into `work` of the `HereDoc` placeholders the last expansion
    /// spliced in from alias replacements — see [`AliasExpansion::heredocs`].
    work_alias_heredocs: Vec<usize>,
    /// Whether [`Self::gather_alias_heredocs`] has taken lines out of the
    /// still-unparsed tail since the last rebuild, so the next one has to undo
    /// it first. See [`Self::rebuild`].
    alias_gathered: bool,
    /// `work`'s view of the source, rebuilt from `work_origin` beside it: the
    /// snapshot of `src` the tokens were lexed from, with each one's end offset
    /// into it. See [`Spans`] for what a diagnostic needs it for.
    work_spans: Spans,
    /// Cursor into `work`.
    wpos: usize,
    /// Index into `orig` of the first token not yet consumed: the origin of the
    /// first `Some`-origin token at or after `wpos`.
    pos: usize,
    /// The alias state `work` was built under, or `None` if never built. The
    /// inner `Option` is the caller's argument (`None` = expansion disabled), so
    /// a `shopt -u expand_aliases` also invalidates.
    last_aliases: Option<Option<AssocArray>>,
    /// An unterminated quote/substitution that ended the input. Held back until
    /// the complete lines before it have been handed out and executed, because
    /// bash reports it only after running them.
    pending_lex_err: Option<ParseError>,
    /// Warnings the reader raised about here-document bodies (see
    /// [`ReaderWarning`]), with their line numbers already run through
    /// `line_map`. Held back for the same reason as `pending_lex_err` and
    /// released by the same rule bash's reader implies: a warning is printed only
    /// once the unit containing its `<<` is handed out, so it lands after the
    /// output of every earlier line and not at all if a syntax error on an
    /// earlier line means that line is never reached.
    pending_warnings: Vec<ReaderWarning>,
    /// Warnings whose unit [`Self::next_unit`] has just handed out, awaiting
    /// [`Self::take_reader_warnings`]. Always drained by the caller before the next
    /// call, which is why a [`Self::relex`] (that discards `pending_warnings`
    /// wholesale) cannot lose one.
    ready_warnings: Vec<ReaderWarning>,
    /// Warnings that belong *after* the error they accompany rather than before
    /// it, awaiting [`Self::take_post_error_warnings`]. Only
    /// [`UngatheredHeredoc`]s land here: the reduction that gathers them runs
    /// after the offending token has already been read and reported, so bash
    /// prints them below the diagnostic instead of above it.
    post_error_warnings: Vec<ReaderWarning>,
}

impl IncrementalParser {
    /// Tokenize `src` in preparation for unit-at-a-time parsing.
    ///
    /// A lexer error does *not* fail this constructor. bash only discovers an
    /// unclosed quote when it reads the line carrying it, by which time every
    /// complete line before that one has already run. [`tokenize_deferred`]
    /// therefore hands back the tokens up to the last complete line *plus* the
    /// error; the error is parked in `pending_lex_err` and surfaced by
    /// [`Self::next_unit`] only after those lines have been handed out.
    ///
    /// `line_map` renumbers every source line, so a fragment lexed on its own
    /// still reports the line numbers of the input it came from. It is
    /// `LineMap::Offset(0)` for a script file or a `-c` string, the count of
    /// lines already consumed for a REPL reading stdin one command at a time,
    /// and [`LineMap::CmdSub`] for a `$( … )` body re-read at expansion time.
    #[must_use]
    pub fn new(src: BStr<'_>, line_map: impl Into<LineMap>, opts: ParseOpts) -> Self {
        // What the reader hands the lexer, not what the caller read: bash drops
        // a NUL in `shell_getc`, before anything downstream can see it. Done
        // here rather than at each of this parser's callers because the byte
        // offsets kept below index the text that was tokenized, and this is that
        // text (see [`strip_nuls`]).
        let src = crate::lexer::strip_nuls(src);
        let src: BStr<'_> = &src;
        let line_map = line_map.into();
        let Tokenized {
            toks: mut orig,
            lines: mut orig_lines,
            offsets: orig_offsets,
            ends: orig_ends,
            conts: orig_conts,
            heredoc_lines: orig_heredoc_lines,
            warnings,
            ungathered: mut orig_ungathered,
            err,
        } = tokenize_deferred(src, opts);
        map_lines(&mut orig, &mut orig_lines, &line_map);
        for u in &mut orig_ungathered {
            u.line = line_map.map(u.line);
        }
        let pending_warnings = map_reader_warnings(warnings, &line_map);
        Self {
            src: bytes::chars(src).collect(),
            opts,
            orig,
            orig_lines,
            orig_offsets,
            orig_ends,
            orig_conts,
            orig_heredoc_lines,
            orig_ungathered,
            hist_cursor: 0,
            expand_cursor: 0,
            unit_lines: Vec::new(),
            unit_raw: Str::new(),
            work: Vec::new(),
            work_lines: Vec::new(),
            work_origin: Vec::new(),
            work_heredoc_lines: Vec::new(),
            work_alias_heredocs: Vec::new(),
            alias_gathered: false,
            work_spans: Spans::default(),
            wpos: 0,
            pos: 0,
            last_aliases: None,
            pending_lex_err: err
                .map(|(e, line)| ParseError::from(e).or_line(line))
                .map(|e| ParseError {
                    line: e.line.map(|l| line_map.map(l)),
                    ..e
                }),
            pending_warnings,
            ready_warnings: Vec::new(),
            post_error_warnings: Vec::new(),
            line_map,
        }
    }

    /// Re-lex the unconsumed remainder of the source under new options.
    ///
    /// Everything already handed out stays as it was — bash cannot un-read a
    /// line — so only the tail starting at the next unconsumed token's character
    /// offset is tokenized again. Its line numbers restart at 1, so the mapping
    /// is composed with the newlines that precede that offset — see
    /// [`LineMap::shifted`]. Alias-spliced tokens
    /// sitting in `work` are untouched and are carried over by the following
    /// [`Self::rebuild`], which is why `last_aliases` is cleared to force one.
    fn relex(&mut self, opts: ParseOpts) {
        let off = self
            .orig_offsets
            .get(self.pos)
            .map_or(self.src.len(), |&o| (o as usize).min(self.src.len()));
        self.relex_from(off, opts);
    }

    /// [`Self::relex`] from an explicit character offset.
    ///
    /// `off` must be at or before the next unconsumed token, so nothing already
    /// handed out is re-lexed. History expansion re-lexes from
    /// [`Self::hist_cursor`] rather than from that token, because the text it
    /// rewrote can lie *before* the token (a `!`-reference on a line the lexer
    /// produced no token for, such as one that is entirely a comment).
    fn relex_from(&mut self, off: usize, opts: ParseOpts) {
        let off = off.min(self.src.len());
        let head = self.src.get(..off).unwrap_or(&[]);
        let newlines =
            u32::try_from(head.iter().filter(|&&c| c == '\n').count()).unwrap_or(u32::MAX);
        let map = self.line_map.shifted(newlines);
        let tail: Str = bytes::from_chars(self.src.get(off..).unwrap_or(&[]).iter().copied());
        let Tokenized {
            toks: mut orig,
            lines: mut orig_lines,
            mut offsets,
            mut ends,
            conts,
            heredoc_lines,
            warnings,
            mut ungathered,
            err,
        } = tokenize_deferred(&tail, opts);
        map_lines(&mut orig, &mut orig_lines, &map);
        // The re-lex is the authoritative read of the tail, so its record replaces
        // the old one outright — the `tok_index`es are indices into the *new*
        // stream, and `pos` is about to restart at 0 to match.
        self.pending_warnings = map_reader_warnings(warnings, &map);
        let delta = u32::try_from(off).unwrap_or(u32::MAX);
        for o in offsets.iter_mut().chain(ends.iter_mut()) {
            *o = o.saturating_add(delta);
        }
        // Same rebase, and the same "the re-lex is authoritative" rule: the tail is
        // where every here-document still ungathered must be, since the head was
        // read and handed out already.
        for u in &mut ungathered {
            u.op_offset = u.op_offset.saturating_add(delta);
            u.line = map.map(u.line);
        }
        self.orig_ungathered = ungathered;
        self.orig = orig;
        self.orig_lines = orig_lines;
        self.orig_offsets = offsets;
        self.orig_ends = ends;
        self.orig_heredoc_lines = heredoc_lines;
        // The head was not re-scanned, so its continuations are still the only
        // record of what the first lex joined away there — text the history may
        // yet slice, since `hist_cursor` can trail `off` by a comment or two.
        self.orig_conts.retain(|&o| (o as usize) < off);
        self.orig_conts
            .extend(conts.iter().map(|&o| o.saturating_add(delta)));
        self.pos = 0;
        self.opts = opts;
        self.pending_lex_err = err
            .map(|(e, line)| ParseError::from(e).or_line(line))
            .map(|e| ParseError {
                line: e.line.map(|l| map.map(l)),
                ..e
            });
        // `work` was expanded from the tokens just discarded, so it must be
        // rebuilt whatever the alias state.
        self.last_aliases = None;
    }

    /// Re-expand the unconsumed remainder of the original token stream under
    /// `aliases`, and give any here-document an alias replacement declared the
    /// body it is owed.
    ///
    /// The gathering has to happen here rather than in the lexer because the
    /// `<<` does not exist until the expansion runs, and it changes what the
    /// *rest* of the input is — the body's lines are no longer commands — so the
    /// tail is re-lexed and the expansion redone over it. Each pass settles at
    /// least one physical line, so the loop is bounded by the line count; the
    /// already-settled prefix of `orig` never changes, which is what makes the
    /// bodies collected by an earlier pass still land on the right tokens after a
    /// later one. See [`Self::gather_alias_heredocs`].
    ///
    /// The gathering runs ahead of the parse, over lines no command has run
    /// before yet — so a later `alias` can invalidate it (`alias A='cat <<E'`,
    /// used once, then redefined to `cat <<F` and used again: the second use was
    /// gathered for `E`, and swallowed the input hunting for a delimiter that
    /// never comes). Every call therefore starts by putting the unparsed tail
    /// back the way the lexer first read it, so this pass sees the input as
    /// bash's reader would with the table it has *now*.
    fn rebuild(&mut self, aliases: Option<&AssocArray>) {
        if std::mem::take(&mut self.alias_gathered) {
            let off = self.orig_offsets.get(self.pos).map_or(self.src.len(), |&o| o as usize);
            self.relex_tail_from(self.pos, off);
        }
        let mut fills = AliasFills::default();
        // Work index past the last physical line already settled, so a `<<` that
        // has had its body is not gathered for a second time.
        let mut from = 0usize;
        loop {
            self.rebuild_once(aliases);
            for (i, tok, lines) in &fills.bodies {
                if let Some(slot) = self.work.get_mut(*i) {
                    *slot = tok.clone();
                }
                if let Some(slot) = self.work_heredoc_lines.get_mut(*i) {
                    *slot = *lines;
                }
            }
            for &(i, line) in &fills.lines {
                if let Some(slot) = self.work_lines.get_mut(i) {
                    *slot = line;
                }
            }
            let Some(&at) = self.work_alias_heredocs.iter().find(|&&i| i >= from) else {
                return;
            };
            let Some(next) = self.gather_alias_heredocs(at, &mut fills) else {
                return;
            };
            debug_assert!(next > from, "a gather pass must settle a line");
            from = next;
        }
    }

    /// One alias-expansion pass over `orig[pos..]`.
    fn rebuild_once(&mut self, aliases: Option<&AssocArray>) {
        // Alias-spliced tokens we are standing in the middle of have no
        // counterpart in `orig`, so carry them over verbatim: re-expanding from
        // `pos` would replay the part of the splice already executed. (Reachable
        // only when an alias value both contains a `;`/newline and changes the
        // alias state, e.g. `alias a='shopt -s expand_aliases; b'`.)
        let carry = self
            .work_origin
            .get(self.wpos..)
            .unwrap_or(&[])
            .iter()
            .take_while(|o| o.is_none())
            .count();
        let end = self.wpos.saturating_add(carry);
        let mut work = self.work.get(self.wpos..end).unwrap_or(&[]).to_vec();
        let mut work_lines = self.work_lines.get(self.wpos..end).unwrap_or(&[]).to_vec();
        let mut work_origin = vec![None; work.len()];
        // Those tokens keep the alias replacement they were read from, so their
        // diagnostics still quote it; the texts come along below.
        let old = std::mem::take(&mut self.work_spans);
        let carried = old.ends.get(self.wpos..end).unwrap_or(&[]).to_vec();

        let rest = self.orig.get(self.pos..).unwrap_or(&[]);
        let rest_lines = self.orig_lines.get(self.pos..).unwrap_or(&[]);
        // The source is snapshotted rather than borrowed because history
        // expansion can rewrite `src` later, and these offsets index the text as
        // it was lexed.
        let rest_ends = self.orig_ends.get(self.pos..).unwrap_or(&[]);
        let mut alias_heredocs = Vec::new();
        let mut spans = match aliases {
            Some(map) if !map.is_empty() => {
                let x = expand_aliases_tracked(rest, rest_lines, rest_ends, map, self.opts);
                let spans = Spans::expanded(self.src.clone(), &x);
                alias_heredocs.extend(x.heredocs.iter().map(|&i| i.saturating_add(carry)));
                work.extend(x.toks);
                work_lines.extend(x.lines);
                work_origin
                    .extend(x.origin.into_iter().map(|i| i.map(|i| i.saturating_add(self.pos))));
                spans
            }
            _ => {
                work.extend_from_slice(rest);
                work_lines.extend_from_slice(rest_lines);
                work_origin.extend((self.pos..self.orig.len()).map(Some));
                Spans {
                    srcs: vec![self.src.clone()],
                    parents: Vec::new(),
                    ends: rest_ends.iter().map(|&end| TokSpan { src: 0, end }).collect(),
                }
            }
        };
        self.work_heredoc_lines = vec![0; work.len()];
        self.work_alias_heredocs = alias_heredocs;
        self.work = work;
        self.work_lines = work_lines;
        self.work_origin = work_origin;
        // The replacements the carried tokens point at go after the ones this
        // expansion pushed, so both numberings survive side by side. (Their
        // *parents* still name `srcs[0]`, which a history expansion could since
        // have rewritten — a stale offset there is possible in principle, but it
        // takes an alias that both spans a `;` and changes the alias state, and
        // a `!`-reference on the same unit.)
        let shift = u32::try_from(spans.srcs.len()).unwrap_or(u32::MAX).saturating_sub(1);
        spans.srcs.extend(old.srcs.into_iter().skip(1));
        spans.parents.extend(old.parents.iter().map(|&p| p.shifted(shift)));
        spans.ends.splice(0..0, carried.iter().map(|&s| s.shifted(shift)));
        self.work_spans = spans;
        self.wpos = 0;
        self.last_aliases = Some(aliases.cloned());
    }

    /// The 1-based physical line `off` stands at the end of: the line of the
    /// character before it, so an offset just past a `\n` names the line that
    /// newline ended rather than the one after it. That is where a reader that
    /// has just consumed up to `off` sits.
    fn line_at(&self, off: usize) -> u32 {
        let upto = self.src.get(..off.min(self.src.len())).unwrap_or(&[]);
        let nls = upto.iter().filter(|&&c| c == '\n').count();
        let n = nls.saturating_add(usize::from(!matches!(upto.last(), Some(&c) if c == '\n')));
        u32::try_from(n.max(1)).unwrap_or(u32::MAX)
    }

    /// Whether `work[i]` is a newline of the *input*, as opposed to one an alias
    /// replacement contained. Only the former ends a physical line, and it is
    /// physical lines a here-document body is read in.
    fn is_input_newline(&self, i: usize) -> bool {
        matches!(self.work.get(i), Some(Tok::Newline))
            && self.work_origin.get(i).copied().flatten().is_some()
    }

    /// Give every here-document on the physical input line containing `work[at]`
    /// its body, taken from the lines after that one, and take those lines away
    /// from the input still to be parsed.
    ///
    /// `at` names a `<<` an alias replacement spliced in, which is the only kind
    /// the lexer could not gather for itself. The *whole line* is re-collected
    /// rather than that one operator, because bash gathers from a single moving
    /// cursor in the real input: an alias `<<` written before a `<<` of the
    /// line's own takes the earlier body, and the line's own then starts where
    /// that one stopped — so the ones the first lex did collect were collected
    /// from the wrong place and have to be done again in declaration order.
    ///
    /// Returns the `work` index just past the line, or `None` if the collection
    /// failed (an unclosed construct inside an expanding body), which parks the
    /// error and leaves the placeholders empty.
    fn gather_alias_heredocs(&mut self, at: usize, fills: &mut AliasFills) -> Option<usize> {
        let nl = (at..self.work.len()).find(|&i| self.is_input_newline(i));
        let start = (0..at)
            .rev()
            .find(|&i| self.is_input_newline(i))
            .map_or(0, |i| i.saturating_add(1));
        let last = nl.unwrap_or(self.work.len());
        // In declaration order, which is `redir_stack` order.
        let mut idxs = Vec::new();
        let mut wants = Vec::new();
        for i in start..=last.min(self.work.len().saturating_sub(1)) {
            let Some(Tok::HereDoc(_, delim, quoted)) = self.work.get(i) else { continue };
            // `<<-` is carried by the operator token, not by the placeholder.
            let strip = matches!(
                i.checked_sub(1).and_then(|j| self.work.get(j)),
                Some(Tok::Op(Op::DLessDash))
            );
            wants.push(HeredocWant { delim: delim.clone(), strip, expand: !quoted });
            idxs.push(i);
        }
        // Just past the newline that ends the line. bash's reader has the whole
        // line in `shell_input_line` by now, so `read_a_line` — reading the real
        // input directly — starts at the line after it.
        let body_start = match nl.and_then(|i| self.work_origin.get(i).copied().flatten()) {
            Some(oi) => {
                let from = self.orig_offsets.get(oi).map_or(self.src.len(), |&o| o as usize);
                self.src
                    .get(from..)
                    .unwrap_or(&[])
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(self.src.len(), |i| from.saturating_add(i).saturating_add(1))
            }
            None => self.src.len(),
        };
        let g = match gather_heredocs_at(&self.src, body_start, &wants, self.opts) {
            Ok(g) => g,
            Err(e) => {
                // Earlier in the input than anything already parked, so it wins.
                self.pending_lex_err = Some(ParseError::from(e));
                return None;
            }
        };
        for (k, &i) in idxs.iter().enumerate() {
            let Some(tok) = g.toks.get(k) else { continue };
            let lines = g.lines.get(k).copied().unwrap_or(0);
            if let Some(slot) = self.work.get_mut(i) {
                *slot = tok.clone();
            }
            if let Some(slot) = self.work_heredoc_lines.get_mut(i) {
                *slot = lines;
            }
            fills.bodies.push((i, tok.clone(), lines));
            // A here-document of the line's own keeps its `orig` slot, and the
            // count there is the one [`Self::heredoc_gather`] reads; the body it
            // records was collected from the wrong offset, so replace it.
            if let Some(oi) = self.work_origin.get(i).copied().flatten() {
                if let Some(slot) = self.orig_heredoc_lines.get_mut(oi) {
                    *slot = lines;
                }
                self.pending_warnings.retain(|w| w.tok_index() != oi);
            }
        }
        // The warnings are the collection's, so they belong to the unit that ends
        // at this line's newline — which is the token whose release gate they must
        // pass. See [`Self::next_unit`].
        let owner = nl
            .and_then(|i| self.work_origin.get(i).copied().flatten())
            .unwrap_or(self.orig.len());
        for mut w in map_reader_warnings(g.warnings, &self.line_map) {
            w.set_tok_index(owner);
            self.pending_warnings.push(w);
        }
        // Where the collection left bash's reader. `make_here_document` bumps
        // `line_number` once per line it takes, so after the gather it names the
        // delimiter's line (or the input's last, when the delimiter never came) —
        // and it stays there until the next real line is fetched, which is what
        // every token read after the gather is stamped with.
        let post = self.line_map.map(self.line_at(g.end));
        // The line's own newline is the token the reduction that follows it takes
        // its number from, exactly as for a here-document the lexer gathered
        // itself (`cat <<E; nosuch` blames `nosuch` on the delimiter's line).
        if let Some(oi) = nl.and_then(|i| self.work_origin.get(i).copied().flatten())
            && let Some(slot) = self.orig_lines.get_mut(oi)
        {
            *slot = post;
        }
        // The gather fires the moment the reader sees a newline. If the alias
        // value itself contained one, that is the newline — so everything read
        // after it is read with the reader already moved: the rest of the value,
        // and then the rest of the calling line, all sit at `post` until the next
        // real line is fetched. With no newline in the value the gather waits for
        // the calling line's own, and the whole line keeps its number.
        if let Some(vnl) = (at..last.min(self.work.len()))
            .find(|&i| matches!(self.work.get(i), Some(Tok::Newline)) && !self.is_input_newline(i))
        {
            for i in vnl.saturating_add(1)..last.min(self.work.len()) {
                if let Some(slot) = self.work_lines.get_mut(i) {
                    *slot = post;
                }
                fills.lines.push((i, post));
            }
        }
        // The body's text was lexed as commands the first time round, so whatever
        // continuations that read found there describe nothing; the collection's
        // are the true ones.
        self.orig_conts
            .retain(|&o| (o as usize) < body_start || (o as usize) >= g.end);
        self.orig_conts.extend(g.conts.iter().copied());
        self.orig_conts.sort_unstable();
        self.orig_conts.dedup();
        if let Some(oi) = nl.and_then(|i| self.work_origin.get(i).copied().flatten()) {
            // The unit's raw text now runs to the end of the bodies, exactly as it
            // does for a here-document the lexer gathered itself.
            if let Some(slot) = self.orig_ends.get_mut(oi) {
                *slot = u32::try_from(g.end).unwrap_or(u32::MAX);
            }
            self.relex_tail_from(oi.saturating_add(1), g.end);
            self.alias_gathered = true;
        }
        Some(last.saturating_add(1))
    }

    /// Re-lex the source from `off`, keeping the first `keep` tokens of `orig`.
    ///
    /// [`Self::relex_from`]'s sibling for the caller that has to drop a *middle*
    /// of the input rather than a tail: the lines an alias-declared here-document
    /// body took are not commands after all, so the tokens the first lex made of
    /// them must go — while the line that introduced them, already expanded and
    /// about to be parsed, has to stay exactly as it is.
    fn relex_tail_from(&mut self, keep: usize, off: usize) {
        let off = off.min(self.src.len());
        let newlines = u32::try_from(
            self.src.get(..off).unwrap_or(&[]).iter().filter(|&&c| c == '\n').count(),
        )
        .unwrap_or(u32::MAX);
        let map = self.line_map.shifted(newlines);
        let tail: Str = bytes::from_chars(self.src.get(off..).unwrap_or(&[]).iter().copied());
        let Tokenized {
            toks: mut orig,
            lines: mut orig_lines,
            mut offsets,
            mut ends,
            conts,
            heredoc_lines,
            warnings,
            mut ungathered,
            err,
        } = tokenize_deferred(&tail, self.opts);
        map_lines(&mut orig, &mut orig_lines, &map);
        let delta = u32::try_from(off).unwrap_or(u32::MAX);
        for o in offsets.iter_mut().chain(ends.iter_mut()) {
            *o = o.saturating_add(delta);
        }
        for u in &mut ungathered {
            u.op_offset = u.op_offset.saturating_add(delta);
            u.line = map.map(u.line);
        }
        self.orig_ungathered = ungathered;
        // The re-lex is the authoritative read of everything past `keep`, so its
        // warnings replace the old ones there; the head's are untouched, having
        // been raised about text this pass did not look at.
        self.pending_warnings.retain(|w| w.tok_index() < keep);
        for mut w in map_reader_warnings(warnings, &map) {
            w.set_tok_index(w.tok_index().saturating_add(keep));
            self.pending_warnings.push(w);
        }
        self.orig.truncate(keep);
        self.orig.extend(orig);
        self.orig_lines.truncate(keep);
        self.orig_lines.extend(orig_lines);
        self.orig_offsets.truncate(keep);
        self.orig_offsets.extend(offsets);
        self.orig_ends.truncate(keep);
        self.orig_ends.extend(ends);
        self.orig_heredoc_lines.truncate(keep);
        self.orig_heredoc_lines.extend(heredoc_lines);
        self.orig_conts.retain(|&o| (o as usize) < off);
        self.orig_conts
            .extend(conts.iter().map(|&o| o.saturating_add(delta)));
        self.pending_lex_err = err
            .map(|(e, line)| ParseError::from(e).or_line(line))
            .map(|e| ParseError { line: e.line.map(|l| map.map(l)), ..e });
        self.last_aliases = None;
    }

    /// Whether every token has been handed out, so a further [`Self::next_unit`]
    /// can only yield `None`. Meaningful once at least one unit has been
    /// requested (that is what builds the working token stream).
    ///
    /// Lets a caller ask whether the unit it just took was the input's *only*
    /// one — which bash's `$(< file)` fast path needs, since it applies solely
    /// when the redirect is the whole substitution body.
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.wpos >= self.work.len() && self.pending_lex_err.is_none()
    }

    /// Where the next physical line that has not yet been history-expanded
    /// starts. See [`Self::expand_cursor`] for why it is the later of the two
    /// cursors.
    fn expand_frontier(&self) -> usize {
        self.expand_cursor.max(self.hist_cursor).min(self.src.len())
    }

    /// The index just past the end of the line starting at `start`, not counting
    /// its newline.
    fn line_end(&self, start: usize) -> usize {
        self.src
            .get(start..)
            .unwrap_or(&[])
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.src.len(), |i| start.saturating_add(i))
    }

    /// The next physical input line that has not yet been offered for `!`-style
    /// history expansion, or `None` once the input is used up.
    ///
    /// bash expands history on a line as it *reads* it — before the lexer sees
    /// it, and independently of where parse units begin and end — so a caller
    /// doing history expansion drives this and [`Self::commit_raw_line`] in a
    /// loop until the lines it has expanded form a complete command, then calls
    /// [`Self::next_unit`] as usual.
    #[must_use]
    pub fn peek_raw_line(&self) -> Option<RawLine> {
        let start = self.expand_frontier();
        if start >= self.src.len() {
            return None;
        }
        let end = self.line_end(start);
        let newlines = self
            .src
            .get(..start)
            .unwrap_or(&[])
            .iter()
            .filter(|&&c| c == '\n')
            .count();
        let line = u32::try_from(newlines).unwrap_or(u32::MAX).saturating_add(1);
        Some(RawLine {
            text: bytes::from_chars(self.src.get(start..end).unwrap_or(&[]).iter().copied()),
            line: self.line_map.map(line),
        })
    }

    /// Consume the line [`Self::peek_raw_line`] returned, optionally replacing
    /// its text with its history-expanded form.
    ///
    /// `None` leaves the source untouched (the line held no history reference).
    /// `Some(text)` splices `text` in and re-lexes the unparsed remainder, so the
    /// parser goes on to see the expanded line and nothing else — which is also
    /// what puts the *expanded* form into the command history, as bash does.
    pub fn commit_raw_line(&mut self, expanded: Option<BStr<'_>>, opts: ParseOpts) {
        let start = self.expand_frontier();
        if start >= self.src.len() {
            return;
        }
        let end = self.line_end(start);
        let mut new_end = end;
        if let Some(text) = expanded {
            let repl: Vec<Ch> = bytes::chars(text).collect();
            new_end = start.saturating_add(repl.len());
            self.src.splice(start..end, repl);
            // From `hist_cursor`, not from the next token: the rewritten text can
            // precede that token (see [`Self::relex_from`]).
            self.relex_from(self.hist_cursor, opts);
        }
        // Step past the newline ending the line, so the next peek starts on the
        // following one. A replacement containing newlines counts as expanded in
        // full — bash expands a line once, never its own output.
        self.expand_cursor = if self.src.get(new_end).is_some_and(|&c| c == '\n') {
            new_end.saturating_add(1)
        } else {
            self.src.len()
        };
    }

    /// Discard the line [`Self::peek_raw_line`] returned, newline included, as
    /// bash does when a history expansion fails.
    ///
    /// Deleting the newline as well is not an implementation shortcut but the
    /// observable behaviour: bash counts a source line only when its newline
    /// reaches the lexer, so a discarded line never advances `line_number` and
    /// every later diagnostic — and `$LINENO` — is one lower. Cutting the line
    /// out of `src` reproduces that exactly, since line numbers here are likewise
    /// counted from the newlines in `src`.
    pub fn drop_raw_line(&mut self, opts: ParseOpts) {
        let start = self.expand_frontier();
        if start >= self.src.len() {
            return;
        }
        let end = self.line_end(start);
        let cut =
            if self.src.get(end).is_some_and(|&c| c == '\n') { end.saturating_add(1) } else { end };
        self.src.drain(start..cut);
        self.relex_from(self.hist_cursor, opts);
        self.expand_cursor = start;
    }

    /// How many input lines bash's reader would have moved on before blaming the
    /// token `p` is stopped at — the here-documents that token's own reduction
    /// gathers, before it is looked at closely enough to be a syntax error.
    ///
    /// bash gathers a pending here-document from three places. The one this
    /// lexer models is the newline token; the one that matters here is the yacc
    /// action of `simple_list` (parse.y:1217, with the `&`/`;` variants at 1235
    /// and 1250):
    ///
    /// ```text
    /// simple_list:    simple_list1
    ///                 { ...  if (need_here_doc) gather_here_documents ();  ... }
    /// ```
    ///
    /// The reduction needs a lookahead, and a token that cannot continue the
    /// list is exactly that — so the body is read *first* and the error is
    /// blamed on the line the reading left the reader on:
    ///
    /// ```text
    /// cat <<E(          line 3: syntax error near unexpected token `('
    /// body              line 3: `cat <<E('
    /// E
    /// ```
    ///
    /// Two conditions, both of them bash's:
    ///
    /// * **Top level only.** The same action in `compound_list` (1148) is
    ///   guarded by `last_read_token == '\n'`, so inside a `{ }`, `( )`, `if`,
    ///   `while`, `case` arm or function body only the ordinary newline gather
    ///   fires and `{ cat <<E(` is blamed on line 1. See [`Parser::depth`].
    /// * **Still pending.** Only the here-documents declared since the last
    ///   newline and *before* the offending token are gathered, which is why
    ///   `cat <<A( <<B` is blamed on `A`'s delimiter line and not on `B`'s.
    fn heredoc_gather(&self, p: &Parser) -> u32 {
        if p.depth != 0 {
            return 0;
        }
        let mut n = 0u32;
        for i in (0..p.pos.min(p.toks.len())).rev() {
            match p.toks.get(i) {
                Some(Tok::Newline) => break,
                Some(Tok::HereDoc(..)) => {
                    // An alias-spliced `<<` has no `orig` slot to look its count
                    // up in — it was never written in the input. Its body was
                    // collected out of the real input by
                    // [`Self::gather_alias_heredocs`], which parked the count on
                    // the work token itself.
                    let lines = match self.work_origin.get(i) {
                        Some(&Some(oi)) => self.orig_heredoc_lines.get(oi).copied().unwrap_or(0),
                        _ => self.work_heredoc_lines.get(i).copied().unwrap_or(0),
                    };
                    n = n.saturating_add(lines);
                }
                _ => {}
            }
        }
        n
    }

    /// Queue the `here-document … delimited by end-of-file` warning for each
    /// here-document the lex never reached the body of, keeping only the ones the
    /// gathering reduction can actually reach.
    ///
    /// This is [`Self::heredoc_gather`]'s other half. There the reduction moved
    /// the reader past a body it *did* read; here there is no body at all — an
    /// unclosed construct after the `<<` swallowed the input, so the newline that
    /// would have triggered the ordinary gather never arrived. The `simple_list`
    /// action still runs, because yacc performs its default reductions before it
    /// discovers the lookahead is an error, and `make_here_document` then raises
    /// the same warning it would have raised at a newline. Two things about it are
    /// peculiar to this route, and both are observable:
    ///
    /// * it lands **after** the syntax error, since the lexer had already reported
    ///   the unclosed construct by the time the reduction ran — hence
    ///   [`Self::take_post_error_warnings`], separate from the ordinary
    ///   pre-command channel;
    /// * both of its line numbers are the end of the input, because the reader had
    ///   run there looking for the close.
    ///
    /// The gate is the one `compound_list`'s guard (parse.y:1147) implies: only a
    /// *top-level* list reduces this way. [`Parser::depth`] cannot answer it —
    /// the truncation in [`tokenize_deferred`] removes the `<<`'s whole line, so
    /// the parser is standing at depth 0 whatever enclosed it — so ask the source
    /// instead: the `<<` stood at the top level exactly when the text in front of
    /// it is a complete program of its own. `{ cat ` is not one and `echo one`
    /// followed by `cat ` is, which is the distinction wanted. A prefix that is a
    /// genuine syntax error is not one either, and rightly: bash would have
    /// abandoned the parse there, long before any here-document was pending.
    fn ungathered_warnings(&mut self, aliases: Option<&AssocArray>) {
        for u in std::mem::take(&mut self.orig_ungathered) {
            let off = (u.op_offset as usize).min(self.src.len());
            let head: Str =
                bytes::from_chars(self.src.get(..off).unwrap_or(&[]).iter().copied());
            let complete = match aliases {
                Some(a) => parse_with_aliases(&head, a, self.opts).is_ok(),
                None => parse_opts(&head, self.opts).is_ok(),
            };
            if complete {
                self.post_error_warnings
                    .push(ReaderWarning::HeredocEof(HeredocEof {
                        delim: u.delim,
                        body_line: u.line,
                        eof_line: u.line,
                        // Never consulted: this channel is drained whole, having
                        // already passed the only gate it has.
                        tok_index: 0,
                    }));
            }
        }
    }

    /// The physical source line the token `p` is stopped at was written on,
    /// which is what bash echoes under a diagnostic whose line number a
    /// here-document gather has moved past. Keyed off the token's own character
    /// offset rather than its line number, so it is unaffected by the
    /// renumbering `line_map` applies.
    fn tok_line_text(&self, p: &Parser) -> Option<Str> {
        let &Some(oi) = self.work_origin.get(p.pos)? else { return None };
        let off = (*self.orig_offsets.get(oi)? as usize).min(self.src.len());
        let start = self
            .src
            .get(..off)
            .unwrap_or(&[])
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i.saturating_add(1));
        let end = self.line_end(start);
        let mut text = bytes::from_chars(self.src.get(start..end).unwrap_or(&[]).iter().copied());
        if text.ends_with(b"\r") {
            text.pop();
        }
        Some(text)
    }

    /// Parse the next unit under the given alias state, or `None` when the input
    /// is exhausted. A returned [`ParseError`] also ends the iteration: bash
    /// abandons the rest of a script (or `eval`/`source` string) after a syntax
    /// error rather than resynchronising.
    ///
    /// `aliases` is `None` when `expand_aliases` is off. Passing a state that
    /// differs from the previous call re-expands the remaining input, which is
    /// how a mid-script `alias`/`shopt` takes effect. `opts` works the same way
    /// one level down: a change re-*lexes* the remaining input.
    pub fn next_unit(
        &mut self,
        aliases: Option<&AssocArray>,
        opts: ParseOpts,
    ) -> Option<Result<Program, ParseError>> {
        // A lexing option must be applied before aliases, since re-lexing
        // replaces the very tokens the alias pass works from.
        if opts != self.opts {
            self.relex(opts);
        }
        // Compare against the state the current `work` was expanded under. Only
        // a change forces a rebuild, so a script with a stable (or empty) alias
        // table pays for exactly one expansion pass.
        let stale = match (&self.last_aliases, aliases) {
            (None, _) => true,
            (Some(None), None) => false,
            (Some(Some(prev)), Some(now)) => prev != now,
            _ => true,
        };
        if stale {
            self.rebuild(aliases);
        }
        let mut p = Parser {
            toks: std::mem::take(&mut self.work),
            lines: std::mem::take(&mut self.work_lines),
            spans: std::mem::take(&mut self.work_spans),
            pos: self.wpos,
            opts: self.opts,
            depth: 0,
        };
        let mut items = Vec::new();
        let outcome = loop {
            match p.parse_item(&[]) {
                // End of the token stream. Anything left over is a token no item
                // can start with (a stray `)`), reported as `parse_tokens` does.
                Ok(None) if p.pos == p.toks.len() => break Ok(()),
                Ok(None) => break Err(p.unexpected_here()),
                Ok(Some(item)) => {
                    items.push(item);
                    // The separator `parse_item` just consumed decides what
                    // happens next.
                    match p.pos.checked_sub(1).and_then(|i| p.toks.get(i)) {
                        // `;`/`&` chain another command onto the same logical
                        // line, so the unit continues — but only as far as the
                        // end of that line. A separator written *last* on a line
                        // is followed by the newline that ends the unit, and the
                        // newline has to be consumed here because `parse_item`
                        // would otherwise skip over it looking for the next
                        // command and join the two lines into one unit.
                        //
                        // The boundary is observable, which is why the trailing
                        // separator must not blur it: bash's reader takes one
                        // line at a time, so `alias foo=…;` on its own line is a
                        // unit of its own and the `foo` beneath it is parsed
                        // afterwards, with the alias in force. Joined, the alias
                        // would be defined and used inside a single parse and
                        // `foo` would go out as an ordinary command word. The
                        // same boundary decides where finished jobs leave the
                        // table, what `set -v` echoes at a time, what the history
                        // records as one entry, and what `\#` counts.
                        Some(Tok::Op(Op::Semi | Op::Amp)) => {
                            if matches!(p.peek(), Some(Tok::Newline)) {
                                p.bump();
                                break Ok(());
                            }
                        }
                        // A newline ends the unit.
                        Some(Tok::Newline) => break Ok(()),
                        // No separator at all: valid only at end of input.
                        // Otherwise the item stopped on a token that cannot
                        // terminate a top-level list — a stray `)` — which is a
                        // syntax error for the *whole* unit, so it must be
                        // detected here, before the items are handed back to be
                        // executed. (`echo one; echo two )` runs nothing in
                        // bash, not even `echo one`.)
                        _ if p.pos == p.toks.len() => break Ok(()),
                        _ => break Err(p.unexpected_here()),
                    }
                }
                Err(e) => break Err(e),
            }
        };
        // A grammar error leaves the cursor on the offending token; stamp its
        // line exactly as `parse_tokens` does — including the end-of-file case's
        // extra line, which keys off the message rather than the cursor (an
        // error can name a token from outside this stream).
        let ran_out = p.peek().is_none();
        let outcome = outcome.map_err(|e| {
            // A here-document still pending at a *top-level* reduction is
            // gathered before the offending token is ever found to be one, and
            // gathering moves bash's reader. See [`Self::heredoc_gather`].
            // Only for an error being stamped here: one that already names a
            // line was raised in a stream of its own (a `$( … )` body), where
            // the reduction never happens.
            let gathered = if e.line.is_none() { self.heredoc_gather(&p) } else { 0 };
            let line = p
                .reader_line()
                .saturating_add(u32::from(e.is_incomplete()))
                .saturating_add(gathered);
            // `read_a_line` (parse.y:2080) reads a here-document body into a
            // buffer of its own and never replaces `shell_input_line`, so the
            // gather moved the *number* and not the text: bash prints the line
            // the error was written on under a prefix naming a later one.
            let pinned = (gathered > 0).then(|| self.tok_line_text(&p)).flatten();
            let echo = p.reader_echo().or(pinned);
            e.or_line(line).or_echo(echo)
        });
        self.wpos = p.pos;
        self.work = p.toks;
        self.work_lines = p.lines;
        self.work_spans = p.spans;
        // Whether this call is about to end the input by reporting the parked lexer
        // error — either in place of a grammar error that only happened because the
        // stream was truncated, or as the end-of-input result itself. Sampled before
        // the two arms that consume it.
        let lex_err_now =
            self.pending_lex_err.is_some() && (items.is_empty() || (outcome.is_err() && ran_out));
        // A parked lexer error wins over a grammar error that only happened
        // because the token stream was *truncated* at the unclosed construct
        // (`if true; then` + `echo 'unterm` leaves a `then` with no body). Such
        // a failure always runs the stream dry, so requiring `ran_out`
        // preserves a genuine earlier grammar error — `echo one )` on line 1
        // still reports the stray `)`, never the bad quote on line 2.
        let outcome = match outcome {
            Err(e) if ran_out => Err(self.pending_lex_err.take().unwrap_or(e)),
            other => other,
        };
        // Resume at the first not-yet-consumed token that came from the original
        // stream; spliced tokens before it stay in `work`.
        let next_orig = self
            .work_origin
            .get(self.wpos..)
            .unwrap_or(&[])
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(self.orig.len());
        // Release the warnings for every here-document whose `<<` this unit
        // reached. The frontier is `next_orig` rather than `self.pos`, because the
        // error arm below forces `self.pos` to the end of the stream to abandon the
        // rest of the input — and input bash abandons is input it never read, so it
        // never warns about it either. `echo one )` followed by an unterminated
        // here-document prints the syntax error and nothing else.
        //
        // A parked lexer error about to be reported lifts the frontier entirely:
        // reaching it means the reader consumed the whole input, here-document
        // bodies included, and the record's token may not even be *in* the stream —
        // when the failing scan was the one that swallowed the body (an
        // unterminated here-document inside a `$( … )`), the cut back to the last
        // complete line drops the token the record names.
        let frontier = if lex_err_now { usize::MAX } else { next_orig };
        // The other half of the same reduction: the here-documents whose bodies the
        // scan never reached. They are released on exactly the condition that
        // releases the frontier — this call is the one reporting the parked lexer
        // error, so the reader has consumed the whole input and the reduction that
        // gathers them is about to happen.
        if lex_err_now && !self.orig_ungathered.is_empty() {
            self.ungathered_warnings(aliases);
        }
        if !self.pending_warnings.is_empty() {
            let mut keep = Vec::new();
            for h in std::mem::take(&mut self.pending_warnings) {
                if h.tok_index() < frontier {
                    self.ready_warnings.push(h);
                } else {
                    keep.push(h);
                }
            }
            self.pending_warnings = keep;
        }
        self.unit_lines.clear();
        self.unit_raw.clear();
        match outcome {
            // End of input. If the lexer stopped early on an unclosed
            // construct, this is the point where bash — having executed every
            // complete line before it — reports the error.
            Ok(()) if items.is_empty() => self.pending_lex_err.take().map(Err),
            Ok(()) => {
                self.pos = next_orig;
                self.split_unit_lines(next_orig);
                Some(Ok(Program { items }))
            }
            // Only this unit dies, so resync past the newline that ends it and
            // let the next call parse the unit after — where the arm below
            // abandons everything left. See [`ParseError::recoverable`].
            Err(e) if e.recoverable => {
                let mut i = self.wpos;
                while i < self.work.len() && !matches!(self.work.get(i), Some(Tok::Newline)) {
                    i = i.saturating_add(1);
                }
                self.wpos = self.work.len().min(i.saturating_add(1));
                let resume = self
                    .work_origin
                    .get(self.wpos..)
                    .unwrap_or(&[])
                    .iter()
                    .flatten()
                    .next()
                    .copied()
                    .unwrap_or(self.orig.len());
                self.split_unit_lines(resume);
                self.pos = resume;
                Some(Err(e))
            }
            Err(e) => {
                // bash has already *read* the line it could not parse, so it is
                // in the history before the diagnostic is printed.
                self.split_unit_lines(next_orig);
                // Abandon the rest of the input, discarding the units parsed so
                // far in *this* unit — bash never runs a partially-parsed line.
                self.wpos = self.work.len();
                self.pos = self.orig.len();
                // Input bash abandons is input it never reads, so it never warns
                // about a here-document there either. Dropping these here rather
                // than letting the frontier sweep them up is the difference
                // between matching bash on `echo one )` + `cat <<EOF` (silent
                // apart from the syntax error) and warning about a line that was
                // never reached.
                self.pending_warnings.clear();
                Some(Err(e))
            }
        }
    }

    /// The unterminated here-documents belonging to the unit [`Self::next_unit`]
    /// last returned, drained.
    ///
    /// Call this after every `next_unit` and warn about each entry *before*
    /// running the unit (but after a `set -v` echo of it, which bash emits first
    /// — the echo is the reader handing the line over, the warning is the reader
    /// having run out). Entries only appear for a unit that was actually reached,
    /// so a caller that abandons the input on a syntax error need do nothing
    /// special.
    pub fn take_reader_warnings(&mut self) -> Vec<ReaderWarning> {
        std::mem::take(&mut self.ready_warnings)
    }

    /// The warnings belonging *below* the error the unit [`Self::next_unit`] last
    /// returned, drained. See [`Self::ungathered_warnings`] for why these are on
    /// a channel of their own; warn about each one after printing the error.
    pub fn take_post_error_warnings(&mut self) -> Vec<ReaderWarning> {
        std::mem::take(&mut self.post_error_warnings)
    }

    /// The top-level lines of the unit [`Self::next_unit`] last returned, for a
    /// caller keeping a command history. Empty when the unit consumed nothing
    /// from the original stream (an alias splice replaying already-recorded
    /// tokens), and reset by every call to `next_unit`.
    #[must_use]
    pub fn last_unit_lines(&self) -> &[UnitLine] {
        &self.unit_lines
    }

    /// The raw source the unit [`Self::next_unit`] last returned occupies,
    /// newline-terminated unless the input itself ended without one. This is
    /// what `set -v` echoes: bash echoes input as its *reader* consumed it, so
    /// the text is uncooked — leading blanks and comments, here-document bodies
    /// and `\<newline>` joins are all still in it, unlike
    /// [`Self::last_unit_lines`], which cooks them for the history.
    #[must_use]
    pub fn last_unit_raw(&self) -> BStr<'_> {
        &self.unit_raw
    }

    /// Cut the source the just-parsed unit occupies into its top-level lines.
    ///
    /// The span runs from wherever the previous unit's text ended to the end of
    /// the last token this unit consumed — which for a `Newline` that swallowed
    /// a here-document body is past the body, so the body travels with the line
    /// that introduced it. Starting from the previous end rather than from this
    /// unit's first token is what keeps the comment lines *before* a command:
    /// the lexer emits no token for a comment, only for the newline ending it,
    /// so the comment's own text lies before that token's offset.
    fn split_unit_lines(&mut self, end_orig: usize) {
        let Some(last) = end_orig.checked_sub(1) else { return };
        let start = self.hist_cursor;
        let end = self
            .orig_ends
            .get(last)
            .map_or(self.src.len(), |&e| (e as usize).min(self.src.len()));
        if end <= start {
            return;
        }
        self.hist_cursor = end;
        self.unit_raw = bytes::from_chars(self.src.get(start..end).unwrap_or(&[]).iter().copied());
        // Tokens are emitted in source order, so walking the span's tokens once
        // groups them by the newlines between them.
        let mut toks: Vec<usize> = Vec::new();
        let mut line_start = start;
        let mut prev_heredoc = false;
        for i in 0..self.orig.len() {
            let off = self.orig_offsets.get(i).map_or(usize::MAX, |&o| o as usize);
            if off < start {
                continue;
            }
            if off >= end {
                break;
            }
            if !matches!(self.orig.get(i), Some(Tok::Newline)) {
                toks.push(i);
                continue;
            }
            let line = self.classify_line(line_start, off, &toks, prev_heredoc);
            prev_heredoc = line.heredoc;
            self.unit_lines.push(line);
            toks.clear();
            line_start = off.saturating_add(1);
        }
        // Whatever follows the last newline: a here-document body, a final line
        // with no newline of its own, or nothing at all.
        if line_start < end || !toks.is_empty() {
            let line = self.classify_line(line_start, end, &toks, prev_heredoc);
            self.unit_lines.push(line);
        }
    }

    /// Build one [`UnitLine`] from the source range `[from, to)` and the tokens
    /// standing on it.
    fn classify_line(
        &self,
        from: usize,
        to: usize,
        toks: &[usize],
        prev_heredoc: bool,
    ) -> UnitLine {
        let text = self.line_text(from, to);
        let Some(&last) = toks.last() else {
            // No token of its own: the body of the here-document the previous
            // line opened, a comment, or a blank line.
            let kind = if prev_heredoc {
                UnitLineKind::HereDocBody
            } else if bytes::trim(&text).is_empty() {
                UnitLineKind::Blank
            } else {
                UnitLineKind::Comment
            };
            return UnitLine {
                text,
                kind,
                comment: kind == UnitLineKind::Comment,
                heredoc: false,
                open: false,
            };
        };
        // Anything left on the line after its last token can only be a comment:
        // every other run of source is a token.
        let code_end = self.orig_ends.get(last).map_or(to, |&e| (e as usize).min(to));
        // Joined-away continuations do not count as leftover text: `echo a \`
        // followed by an empty line leaves a `\` after the last token that the
        // history never sees.
        let comment = !bytes::trim(&self.line_text(code_end, to)).is_empty();
        let heredoc = toks
            .iter()
            .any(|&i| matches!(self.orig.get(i), Some(Tok::Op(Op::DLess | Op::DLessDash))));
        UnitLine { text, kind: UnitLineKind::Code, comment, heredoc, open: self.line_is_open(last) }
    }

    /// The source range `[from, to)` as the history should store it: with every
    /// `\<newline>` the lexer joined away removed, exactly as bash records the
    /// joined line rather than the two physical ones. A continuation the lexer
    /// *kept* — inside `'…'`, `"…"`, `$( … )`, or a quoted-delimiter
    /// here-document — was never recorded and so survives here too.
    fn line_text(&self, from: usize, to: usize) -> Str {
        let mut text = Str::new();
        let mut i = from;
        while i < to {
            let at = u32::try_from(i).unwrap_or(u32::MAX);
            if self.orig_conts.binary_search(&at).is_ok() {
                // Skip the backslash and the newline it hid, CR included.
                i = i.saturating_add(1);
                if self.src.get(i).is_some_and(|&c| c == '\r') {
                    i = i.saturating_add(1);
                }
                if self.src.get(i).is_some_and(|&c| c == '\n') {
                    i = i.saturating_add(1);
                }
                continue;
            }
            if let Some(&c) = self.src.get(i) {
                c.push_to(&mut text);
            }
            i = i.saturating_add(1);
        }
        text
    }

    /// Whether a `;` may not follow the token at `i` — bash then joins the next
    /// line on with a plain space instead.
    fn line_is_open(&self, i: usize) -> bool {
        match self.orig.get(i) {
            Some(Tok::Op(
                Op::Semi
                | Op::DSemi
                | Op::SemiAmp
                | Op::DSemiAmp
                | Op::Amp
                | Op::AndIf
                | Op::OrIf
                | Op::Pipe
                | Op::PipeAmp
                | Op::LParen,
            )) => true,
            Some(Tok::Word(segs)) => matches!(
                segs.as_slice(),
                [Seg::Lit(s)] if matches!(s.as_slice(), b"{" | b"do" | b"then" | b"else" | b"elif" | b"in")
            ),
            _ => false,
        }
    }
}

/// Parse the body of a `$( … )` substitution, renumbering its lines the way
/// bash does.
///
/// bash scans the enclosing command first and only then re-parses the captured
/// body, so `$LINENO` inside the body does not start at the body's own first
/// line — it counts up from the line the outer scan had already reached, i.e.
/// the substitution's *closing* delimiter. Measured against bash 5.x (11
/// probes), the rule is:
///
/// > `$LINENO` = `close_line` + (0-based **rank** of the body line among the
/// > body lines that carry a command).
///
/// A rank, not an offset: a blank line inside the body does not advance it, and
/// two commands written on one body line share a number. So
///
/// ```text
/// 1  echo $LINENO     → 1
/// 2  v=$(
/// 3  echo $LINENO     → 6   (close_line 6 + rank 0)
/// 4                         (blank line — carries no command, so no rank)
/// 5  echo $LINENO     → 7   (close_line 6 + rank 1, not rank 2)
/// 6  )
/// ```
///
/// `close_line` comes from the lexer ([`Seg::CmdSub`]'s second field).
///
/// That rank rule is what the *returned map* describes, and it applies to the
/// second read — the one at expansion time, which is the one that runs (see
/// [`CmdSubBody`]). The eager parse this function performs is the *first* read,
/// which happens in the enclosing token stream, and bash numbers it plainly:
/// a syntax error the enclosing scan raises names the body's true physical
/// line, not a ranked one. So the program is renumbered by an ordinary offset
/// from the line the body opens on, which is `close_line` less the body's own
/// newlines. (The eager program's lines are otherwise unobservable — it exists
/// only to raise that error, to be re-printed by `declare -f`, and to answer
/// the `$(< file)` peek — so this is the only thing they have to get right.)
///
/// Nested substitutions are renumbered through the same offset, so their
/// recorded close lines are physical too and a nested body computes its own
/// offset the same way.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error in the body.
pub fn parse_cmdsub_body(
    src: BStr<'_>,
    close_line: u32,
    opts: ParseOpts,
) -> Result<(Program, LineMap), ParseError> {
    // The body is lexed with the substitution's own `)` standing where the
    // implicit trailing newline would otherwise go, because that is the token
    // bash's parser sees after the body's last command. See
    // [`tokenize_paren_body`].
    let Spanned { mut toks, mut lines, ends } = tokenize_paren_body(src, opts)
        .map_err(|e| ParseError::from(e).in_paren_body())?;
    let map = build_cmdsub_line_map(&toks, &lines, close_line);
    // The body's line 1 is the line `$(` sits on: the closing `)` is on the
    // body's last line, so stepping back over the body's newlines lands there.
    let newlines =
        u32::try_from(src.iter().filter(|&&b| b == b'\n').count()).unwrap_or(u32::MAX);
    let phys = LineMap::Offset(close_line.saturating_sub(newlines).saturating_sub(1));
    map_lines(&mut toks, &mut lines, &phys);
    let prog = parse_tokens_ending(toks, lines, Spans::of(src, ends), opts, true)
        .map_err(|e| {
            // A body that ends mid-construct is not an end of file in bash
            // either: the next token is the `)`, and that is what bash names.
            // Most such errors already say so, because the `)` really is in the
            // stream — but a construct that swallows it (an unclosed quote's
            // re-lex, say) can still bottom out at end of input, so the wording
            // is normalised here.
            //
            // The distinction matters beyond wording: an error naming a token
            // is not continuable, so the REPL must not offer a PS2 prompt for
            // one. Neither form can be completed by more input anyway — the
            // lexer already found the closing delimiter.
            if e.msg == b"syntax error: unexpected end of file" {
                ParseError::new("syntax error near unexpected token `)'")
            } else {
                e
            }
        })
        .map_err(ParseError::in_paren_body)?;
    Ok((prog, map))
}

/// Build the rank-based [`LineMap::CmdSub`] for one `$( … )` body.
fn build_cmdsub_line_map(toks: &[Tok], lines: &[u32], close_line: u32) -> LineMap {
    let mut ranked: Vec<(u32, u32)> = Vec::new();
    for (tok, &line) in toks.iter().zip(lines) {
        // Only lines that actually carry a command count towards the rank;
        // a `Newline` token is the *end* of a line, not content on one, so a
        // blank body line contributes nothing.
        if matches!(tok, Tok::Newline) {
            continue;
        }
        if ranked.last().is_none_or(|&(l, _)| l != line) {
            let rank = u32::try_from(ranked.len()).unwrap_or(u32::MAX);
            ranked.push((line, close_line.saturating_add(rank)));
        }
    }
    LineMap::CmdSub { pre: 0, close_line, ranked }
}

/// Renumber every line a reader warning carries through `map`, for the same
/// reason [`map_lines`] exists: the lexer numbers a fragment from 1, and a
/// warning has to name the line of the input the fragment came from.
/// `tok_index` is an index, not a line, and is left alone.
fn map_reader_warnings(mut ws: Vec<ReaderWarning>, map: &LineMap) -> Vec<ReaderWarning> {
    for w in &mut ws {
        match w {
            ReaderWarning::HeredocEof(h) => {
                h.body_line = map.map(h.body_line);
                h.eof_line = map.map(h.eof_line);
            }
            ReaderWarning::SubstHeredoc(s) => s.line = map.map(s.line),
        }
    }
    ws
}

/// Renumber every source line recorded in a token stream through `map`.
///
/// Applying the mapping here, before the parse, is what makes every AST node
/// carry an absolute line — see [`LineMap`] for why a fragment's own numbering
/// is not the one to report.
///
/// The lines recorded on [`Seg::CmdSub`] (its closing delimiter) and
/// [`Seg::ProcSub`] (its opening one) are renumbered too: [`parse_cmdsub_body`]
/// and [`parse_procsub_body`] number a body relative to them, so leaving them
/// alone would reset the body's numbering back to the fragment's. For a nested
/// `$( $( … ) )` that means the inner body is numbered against the outer body's
/// *already-rebased* lines rather than against its own first line.
fn map_lines(toks: &mut [Tok], lines: &mut [u32], map: &LineMap) {
    if map.is_identity() {
        return;
    }
    for l in lines.iter_mut() {
        *l = map.map(*l);
    }
    for t in toks {
        match t {
            Tok::Word(segs) | Tok::HereDoc(segs, ..) => map_segs(segs, map),
            Tok::ArrayAssign { elems, .. } => {
                for e in elems {
                    map_segs(e, map);
                }
            }
            _ => {}
        }
    }
}

fn map_segs(segs: &mut [Seg], map: &LineMap) {
    for seg in segs {
        match seg {
            Seg::CmdSub(_, close, _) => *close = map.map(*close),
            Seg::ProcSub(_, _, open) => *open = map.map(*open),
            Seg::Dq(inner) => map_segs(inner, map),
            _ => {}
        }
    }
}

fn parse_tokens(
    toks: Vec<Tok>,
    lines: Vec<u32>,
    spans: Spans,
    opts: ParseOpts,
) -> Result<Program, ParseError> {
    parse_tokens_ending(toks, lines, spans, opts, false)
}

/// [`parse_tokens`], but `ends_at_paren` says the stream's final token is the
/// `)` that closes a `$( … )` body ([`tokenize_paren_body`]). That token is the
/// end of the program rather than a leftover, so it is excluded from the
/// "everything consumed?" check — while remaining a real token everywhere else,
/// which is what makes `$( ! )` and `$(for)` name it.
fn parse_tokens_ending(
    toks: Vec<Tok>,
    lines: Vec<u32>,
    spans: Spans,
    opts: ParseOpts,
    ends_at_paren: bool,
) -> Result<Program, ParseError> {
    let mut p = Parser {
        toks,
        lines,
        spans,
        pos: 0,
        opts,
        depth: 0,
    };
    // The closing `)` is consumed by nothing, so a complete body leaves the
    // cursor on it rather than past it.
    let end = p.toks.len().saturating_sub(usize::from(ends_at_paren));
    let parsed = match p.parse_program(&[], true) {
        Ok(_prog) if p.pos != end => {
            // Leftover tokens — typically an unmatched `)` or a stray reserved
            // word. bash names the offending token (`near unexpected token \`)'`).
            Err(p.unexpected_here())
        }
        other => other,
    };
    // Stamp the offending line centrally. `pos` is not advanced past a failing
    // token, so the parser's cursor still points at the error site. bash reports
    // the line its *reader* was on for a grammar error — the token's own line
    // unless a `\<newline>` written flush after it dragged the reader onto the
    // next one — but for an *unexpected end of file* error it reports one line
    // past the last token, the position where the missing terminator would go.
    // Key that off the message, not the cursor:
    // an error can name a token that is not in this stream at all (a `$( … )`
    // body reports the substitution's closing `)`), and those still belong on
    // the last token's line.
    parsed.map_err(|e| {
        let line = p.reader_line().saturating_add(u32::from(e.is_incomplete()));
        e.or_line(line).or_echo(p.reader_echo())
    })
}

struct Parser {
    toks: Vec<Tok>,
    /// Parallel to `toks`: the 1-based source line each token *ends* on, as
    /// computed by the lexer. Read via [`Parser::cur_line`] and stamped onto
    /// each [`Item`] to drive `$LINENO` and error line numbers. Using per-token
    /// lines (rather than counting `Newline` tokens) keeps line numbers correct
    /// across newlines swallowed inside here-docs, quoted strings, and command
    /// substitutions.
    lines: Vec<u32>,
    /// The text `toks` was lexed from, for the diagnostics that quote source
    /// rather than tokens. See [`Spans`].
    spans: Spans,
    pos: usize,
    /// The options the tokens were lexed under, carried so that a nested body
    /// re-lexed during parsing (`$( … )`, `<( … )`) is read the same way.
    opts: ParseOpts,
    /// How many *nested* command lists the parser is inside — the body of a
    /// `{ }`, `( )`, `if`, `while`, `for`, `case` arm or function.
    ///
    /// Only one thing asks: bash gathers a pending here-document at the
    /// reduction of `simple_list` (parse.y:1217), which is the **top-level**
    /// list and no other. The same action inside `compound_list` (1148) is
    /// guarded by `last_read_token == '\n'`, so within a compound only the
    /// ordinary newline gather ever fires. That is why `cat <<E(` is blamed on
    /// its here-document's delimiter line and `{ cat <<E(` on its own. See
    /// [`Tokenized::heredoc_lines`](crate::lexer::Tokenized::heredoc_lines).
    ///
    /// A failing parse leaves this raised: what the question wants is the depth
    /// the error was *raised* at, not the depth the cursor unwound to.
    depth: u32,
}

/// The text a token stream was lexed from, with each token's end offset into it.
///
/// bash's `syntax error near \`X'` does not name a token: `error_token_from_text`
/// slices the *input line* around the position the reader had reached. That is
/// why `[[ a>>b ]]` is reported near `a>>b` and `[[ P;Q ]]` near `;Q` — the
/// slice runs back to the nearest ` `, `\n`, `\t`, `;`, `|` or `&` and forward
/// by exactly one character. A token stream cannot answer that on its own,
/// because the whitespace that decides where the slice stops is the very thing
/// lexing threw away.
///
/// There is more than one such text once aliases are in play, because bash reads
/// an alias by pushing its replacement onto the *input*: `push_string` (parse.y)
/// makes the replacement `shell_input_line` outright, and `pop_string` restores
/// what it displaced when the reader passes its end. So a token an alias spliced
/// in does have a text of its own after all — the alias's — and an error found
/// while the reader is still inside it is reported against that text, both in the
/// `near` slice and in the source line echoed underneath:
///
/// ```text
/// alias A="[[ P;Q"
/// A ]]          line 2: syntax error near `;Q'
///               line 2: `[[ P;Q'
/// ```
///
/// Empty when the stream has no text behind it at all, in which case the
/// diagnostics fall back to naming the token.
#[derive(Default)]
struct Spans {
    /// The texts the tokens were read from. `srcs[0]` is the shell input itself
    /// — exactly what was tokenized, NULs already stripped, since the offsets
    /// index that text and not whatever the caller read (see
    /// [`crate::lexer::strip_nuls`]) — and the rest are alias replacements.
    srcs: Vec<Vec<Ch>>,
    /// Parallel to `srcs` minus its first entry: where the reader returns when it
    /// runs off the end of `srcs[i + 1]`, which is bash's `pop_string`. See
    /// [`Spans::reader_stop`].
    parents: Vec<TokSpan>,
    /// Parallel to the token stream: which text each token was read from, and the
    /// character offset into it just past the token's last character. An `end` of
    /// `u32::MAX` marks a token with no text of its own, for which no slice can
    /// be taken.
    ends: Vec<TokSpan>,
}

impl Spans {
    fn of(src: BStr<'_>, ends: Vec<u32>) -> Self {
        Self {
            srcs: vec![bytes::chars(src).collect()],
            parents: Vec::new(),
            ends: ends.into_iter().map(|end| TokSpan { src: 0, end }).collect(),
        }
    }

    /// The spans of an alias-expanded stream: the shell's own text first, then
    /// every replacement the pass pushed into it, in the order it pushed them —
    /// which is the numbering [`crate::lexer::TokSpan::src`] uses.
    fn expanded(src: Vec<Ch>, x: &AliasExpansion) -> Self {
        Self {
            srcs: std::iter::once(src)
                .chain(x.bodies.iter().map(|b| bytes::chars(&b.text).collect()))
                .collect(),
            parents: x.bodies.iter().map(|b| b.parent).collect(),
            ends: x.spans.clone(),
        }
    }

    /// The text with the given [`TokSpan::src`] id.
    fn text(&self, src: u32) -> Option<&[Ch]> {
        self.srcs.get(src as usize).map(Vec::as_slice)
    }

    /// Where the reader goes when it reads past the end of text `src`: bash's
    /// `pop_string`, restoring the line the alias replacement displaced. `None`
    /// for the shell's own input, which has nothing under it.
    fn parent(&self, src: u32) -> Option<TokSpan> {
        self.parents.get((src as usize).checked_sub(1)?).copied()
    }

    /// The text bash would report an error "near", given that the reader had
    /// just finished the token at `pos`.
    ///
    /// A direct port of bash's `error_token_from_text` (parse.y), which is the
    /// only reason the source text has to be carried this far. Starting from the
    /// character just past the token, it steps back over trailing whitespace,
    /// then back again to the nearest *delimiter* — one of ` `, `\n`, `\t`, `;`,
    /// `|`, `&` — and returns everything from there up to and including the one
    /// character that followed the token. Two consequences fall straight out of
    /// that and are exactly what the token alone cannot reproduce:
    ///
    /// - text written flush *before* the token comes along, because the scan
    ///   back does not stop at a token boundary: `[[ a>>b ]]` reports `a>>b`
    ///   and `[[ -n @(a) ]]` reports `@(a` (a `(` is not a delimiter).
    /// - exactly one character written flush *after* it comes along too, and no
    ///   more: `[[ P;QRS ]]` reports `;Q`, while `[[ P; Q ]]` reports only `;`.
    ///
    /// `None` when there is no source to slice — a token with no text of its own,
    /// or one that is not in this stream at all — leaving the caller to name the
    /// token instead.
    fn near(&self, pos: usize, r: Reader) -> Option<Str> {
        // Where the reader stopped, not where the token ended: a `\<newline>`
        // its lookahead deleted is text the parser never had, so
        // `[[ P;\<newline>Q ]]` is reported near `Q`, not near `;\`. And *which*
        // text it stopped in, since an alias replacement it has run off the end
        // of is no longer the current input line. See [`Spans::reader_stop`].
        let (stop, _) = self.reader_stop(pos, r)?;
        let t = self.text(stop.src)?;
        let mut i = stop.end as usize;
        if t.is_empty() || i > t.len() {
            return None;
        }
        // bash reads `t[i]` where `i` may sit on the terminating NUL; stepping
        // back off the end is that same step.
        if i > 0 && i == t.len() {
            i -= 1;
        }
        while i > 0 && is_error_space(t.get(i)) {
            i -= 1;
        }
        let token_end = if i > 0 { i.saturating_add(1) } else { 0 };
        while i > 0 && !is_error_delim(t.get(i)) {
            i -= 1;
        }
        while i != token_end && is_error_space(t.get(i)) {
            i = i.saturating_add(1);
        }
        let slice = if token_end > 0 { t.get(i..token_end)? } else { t.get(..1)? };
        Some(bytes::from_chars(slice.iter().copied()))
    }

    /// How many further input lines the reader had to fetch to get past the
    /// token at `pos` — one for every `\<newline>` written flush after it.
    ///
    /// bash blames a syntax error on `line_number`, the last line its reader has
    /// **fetched**, and having finished a token the reader has always looked at
    /// the character after it, if only to find that the token ends there. A
    /// `\<newline>` in that position is deleted by `shell_getc`, which bumps
    /// `line_number` and fetches the next line in order to do it — so the error
    /// lands a line below the token, and the source echoed under it is that next
    /// line's:
    ///
    /// ```text
    /// echo a ;;\
    /// b            line 2: syntax error near unexpected token `;;'
    ///              line 2: `b'
    /// ```
    ///
    /// Only a continuation written *flush* against the token drags the reader
    /// along. A space in between is the character it looked at, and it stops
    /// there: `echo a ;; \<newline>b` stays on line 1. A plain newline does not
    /// count either — it is the last character of the line it terminates, and
    /// bash bumps `line_number` only on the *fetch* that follows.
    ///
    /// A continuation flush after the token can fall on either side of the
    /// recorded span, because the lexer does not draw that boundary the same way
    /// everywhere: after `;;` it deletes the continuation before stopping, so
    /// the span *ends* in one, while after `)` it stops first and the
    /// continuation is still ahead. Both are the same crossing, so both are
    /// counted — and they cannot double-count, since a span that ends in a
    /// continuation leaves the walk starting past it.
    ///
    /// The line already stamped on the token is the line of the span's *last*
    /// character (see [`Lexer::stamp_lines`](crate::lexer)), which is why a span
    /// ending in a continuation is worth exactly one more and not the whole run.
    ///
    /// Which continuations the reader crossed — and so where it stopped — is
    /// [`Spans::reader_stop`]'s job; this is only its second half.
    fn cont_lines(&self, pos: usize, r: Reader) -> u32 {
        self.reader_stop(pos, r).map_or(0, |(_, n)| n)
    }

    /// Where bash's reader had got to when the token at `pos` was handed over:
    /// the text it was reading and the offset into it it stopped at, plus how
    /// many input lines it fetched getting there.
    ///
    /// Both halves come out of the same walk because both are decided by the
    /// same thing — how far the token's own reading looked past its last
    /// character, and how many `\<newline>` pairs it deleted on the way. Three
    /// cases, in the order they are tested:
    ///
    /// - A multi-character operator completed *by* its own lookahead
    ///   (`r.peeks` false) never looked again. The reader is parked on the
    ///   character after it, and a continuation standing there is text it has
    ///   not reached: `[[ a>>\<newline>Q ]]` errors on line 1, near `a>>\`.
    /// - Anything else looked at least once more, so a continuation flush
    ///   against it is deleted and the reader is dragged onto the next line:
    ///   `[[ a>\<newline>Q ]]` errors on line 2, near `Q`. The lexer may have
    ///   eaten that continuation into the span already ([`ends_in_cont`])
    ///   or left it ahead — after `;;` it eats, after `)` it does not — so both
    ///   sides are counted, and they cannot double-count, since a span that ends
    ///   in one leaves the walk starting past it.
    /// - A word whose *terminator* is `<` or `>` reaches one character further
    ///   still. `read_token_word` reads that terminator and, because `shellexp`
    ///   holds for it, peeks once more to test for a `<( … )` process
    ///   substitution. The peek is with removal on, so it deletes a continuation
    ///   written after the `<`/`>` even though the word then pushes *both*
    ///   characters back — and `shell_ungetc` cannot push past the start of the
    ///   line it has just fetched, so the reader is left at the top of it:
    ///   `[[ 2>\<newline>Q ]]` errors on line 2, near `Q`, while the very same
    ///   token in `[[ 2>Q ]]` errors on line 1, near `2>`.
    ///
    /// A reader that looks past the end of an *alias replacement* leaves it: that
    /// read is the `shell_getc` which finds the pushed string used up and calls
    /// `pop_string`, so the current input line becomes the one the alias word was
    /// written on again, at the offset just past that word. Which is why the same
    /// `;` is blamed on two different texts depending only on whether anything
    /// follows it inside the alias:
    ///
    /// ```text
    /// alias A="[[ P;Q";  A ]]      near `;Q'   line `[[ P;Q'
    /// alias A="[[ P ;";  A Q ]]    near `A'    line `A Q ]]'
    /// ```
    ///
    /// An operator that never looked again (`r.peeks` false) has not made that
    /// read, so it stays inside the replacement even when it ends flush against
    /// its last character — `alias A="[[ a>>"; A b ]]` is still reported near
    /// `a>>` against `[[ a>>`.
    ///
    /// `None` when the token has no source of its own to measure from.
    fn reader_stop(&self, pos: usize, r: Reader) -> Option<(TokSpan, u32)> {
        let mut at = *self.ends.get(pos)?;
        if at.end == u32::MAX {
            return None;
        }
        if !r.peeks {
            return Some((at, 0));
        }
        // The look past the token pops every replacement it has exhausted.
        while at.end as usize >= self.text(at.src)?.len()
            && let Some(parent) = self.parent(at.src)
        {
            at = parent;
        }
        let t = self.text(at.src)?;
        let end = at.end as usize;
        let mut n = u32::from(ends_in_cont(t, end));
        let mut i = end;
        while let Some(len) = cont_len(t.get(i..).unwrap_or(&[])) {
            i = i.saturating_add(len);
            n = n.saturating_add(1);
        }
        if n == 0 && r.word && matches!(t.get(end), Some(&Ch::U('<' | '>'))) {
            let mut j = end.saturating_add(1);
            let mut m = 0u32;
            while let Some(len) = cont_len(t.get(j..).unwrap_or(&[])) {
                j = j.saturating_add(len);
                m = m.saturating_add(1);
            }
            if m > 0 {
                return Some((TokSpan { src: at.src, end: u32::try_from(j).ok()? }, m));
            }
        }
        Some((TokSpan { src: at.src, end: u32::try_from(i).ok()? }, n))
    }

    /// The text bash would echo under the diagnostic for an error at the token
    /// `pos` — `shell_input_line` as it stands where the reader stopped — when
    /// that is an alias replacement rather than the shell's own input.
    ///
    /// `None` for the shell's own input, which the caller echoes by line number
    /// instead: that is the physical line, and the reader's `src` says nothing
    /// about which of them it is on.
    fn echo_line(&self, pos: usize, r: Reader) -> Option<Str> {
        let (stop, _) = self.reader_stop(pos, r)?;
        if stop.src == 0 {
            return None;
        }
        Some(bytes::from_chars(self.text(stop.src)?.iter().copied()))
    }
}

/// Whether the character just before `end` in `t` is the newline of a
/// `\<newline>` the lexer deleted. A *plain* newline is not one: it is the last
/// character of the line it terminates, and bash bumps `line_number` only on the
/// fetch that follows.
fn ends_in_cont(t: &[Ch], end: usize) -> bool {
    if t.get(end.wrapping_sub(1)) != Some(&Ch::U('\n')) {
        return false;
    }
    // `\<newline>`, or the `\<CR><LF>` a CRLF file writes.
    match t.get(end.wrapping_sub(2)) {
        Some(&Ch::U('\\')) => true,
        Some(&Ch::U('\r')) => t.get(end.wrapping_sub(3)) == Some(&Ch::U('\\')),
        _ => false,
    }
}

/// How far a token's own reading looked past its last character — the two facts
/// [`Spans::reader_stop`] needs to place bash's reader.
#[derive(Clone, Copy)]
struct Reader {
    /// Whether the reader looked at the character *after* the token at all,
    /// which decides whether a `\<newline>` written flush against it was
    /// deleted (dragging the reader onto the next line) or is still standing
    /// there unread.
    ///
    /// bash's `read_token` (parse.y) reads one character, and for a shell
    /// metacharacter immediately takes `peek_char = shell_getc (1)` — a read
    /// *with* continuation removal. Where that peeked character completes a
    /// longer operator, the operator is returned right then and the reader stops
    /// on the character after it. Where it does not, the peek is pushed back
    /// with `shell_ungetc` — but the continuation it deleted on the way is gone,
    /// and `line_number` has already moved.
    ///
    /// So the rule is: **a token crosses a flush continuation unless it is a
    /// multi-character operator that its own lookahead completed.** `>>`, `>&`,
    /// `<&`, `<>`, `>|`, `|&`, `;&`, `&&`, `||`, `<<-`, `<<<`, `&>>` and `;;&`
    /// all return the moment the last character is read. `<<`, `;;` and `&>` do
    /// not — each peeks once more (for `<<-`/`<<<`, for `;;&`, for `&>>`) and
    /// pushes that peek back — so they cross, as do the one-character
    /// metacharacters and every word, whose scan reads its own terminator.
    peeks: bool,
    /// Whether the token came from `read_token_word`, whose terminator peek can
    /// reach one character further than the token itself. See the third case in
    /// [`Spans::reader_stop`].
    word: bool,
}

impl Reader {
    fn of(t: Option<&Tok>) -> Self {
        Self {
            peeks: !matches!(
                t,
                Some(Tok::Op(
                    Op::DGreat
                        | Op::GreatAnd
                        | Op::LessAnd
                        | Op::LessGreat
                        | Op::GreatPipe
                        | Op::PipeAmp
                        | Op::AndIf
                        | Op::OrIf
                        | Op::SemiAmp
                        | Op::DSemiAmp
                        | Op::DLessDash
                        | Op::TLess
                        | Op::AmpDGreat
                ))
            ),
            word: matches!(
                t,
                Some(
                    Tok::Word(_)
                        | Tok::Io(_)
                        | Tok::VarFd(_)
                        | Tok::ArrayAssign { .. }
                        | Tok::Invalid(_)
                )
            ),
        }
    }
}

/// The length of the line continuation at the front of `t`, if there is one:
/// two characters for `\<newline>`, three for the `\<CR><LF>` a CRLF file
/// writes. `None` when `t` does not start with one.
fn cont_len(t: &[Ch]) -> Option<usize> {
    if !matches!(t.first(), Some(&Ch::U('\\'))) {
        return None;
    }
    match (t.get(1), t.get(2)) {
        (Some(&Ch::U('\n')), _) => Some(2),
        (Some(&Ch::U('\r')), Some(&Ch::U('\n'))) => Some(3),
        _ => None,
    }
}

/// bash's `whitespace()` plus the newline it tests beside it, over the character
/// the error scan is looking at. A missing character (past the end) is neither.
fn is_error_space(c: Option<&Ch>) -> bool {
    matches!(c, Some(&Ch::U(' ' | '\t' | '\n')))
}

/// bash's `member (c, " \n\t;|&")` — where the scan back from an error stops.
/// Note what is *not* here: parentheses, quotes and redirection characters are
/// all ordinary text to it, which is why they are swept up into the slice.
fn is_error_delim(c: Option<&Ch>) -> bool {
    matches!(c, Some(&Ch::U(' ' | '\n' | '\t' | ';' | '|' | '&')))
}

/// Reserved words that terminate a command list or introduce a compound.
///
/// bash's `word_token_alist` (parse.y:2205), minus the entries this parser
/// recognises by spelling at the one place they can appear — `time`,
/// `function`, `coproc` and `[[`, all of which stay ordinary words elsewhere.
/// `]]` is *not* one of those: `CHECK_FOR_RESERVED_WORD` consults the table on
/// the single condition `reserved_word_acceptable (last_read_token)`, so `]]`
/// is the token `COND_END` wherever a command could start — whether or not a
/// `[[` is open — and the grammar has no production that begins with it. The
/// one line of the macro that mentions the conditional state only clears it:
///
/// ```c
///     else if (word_token_alist[i].token == COND_END) \
///       parser_state &= ~(PST_CONDCMD|PST_CONDEXPR); \
/// ```
///
/// Past the command word `reserved_word_acceptable` is false, so `echo ]]`
/// prints `]]` like any other word. Membership here says exactly that: a
/// reserved word in command position, nothing more.
const RESERVED: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "in", "{", "}",
    "!", "case", "esac", "select", "]]",
];

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// The 1-based source line of the current token. At end of input, falls back
    /// to the last token's line (or 1 for empty input), so an item that reaches
    /// EOF still reports a sensible line.
    fn cur_line(&self) -> u32 {
        self.lines
            .get(self.pos)
            .or_else(|| self.lines.last())
            .copied()
            .unwrap_or(1)
    }

    /// The line bash would *report* an error at the current token on: not the
    /// token's own line but the reader's, which is a line further down for every
    /// `\<newline>` written flush after the token. See [`Spans::cont_lines`].
    fn reader_line(&self) -> u32 {
        let r = Reader::of(self.toks.get(self.pos));
        self.cur_line().saturating_add(self.spans.cont_lines(self.pos, r))
    }

    /// The text bash would echo under a `syntax error near …` at the current
    /// token, when that is an alias replacement rather than the script itself.
    /// Read from the same cursor as [`Parser::reader_line`], because both ask
    /// the same question — where bash's reader is. See [`Spans::echo_line`].
    fn reader_echo(&self) -> Option<Str> {
        let r = Reader::of(self.toks.get(self.pos));
        self.spans.echo_line(self.pos, r)
    }

    /// [`Parser::reader_echo`] for an error found while lowering the token at
    /// `at`, rather than at the token the parser has since moved on to.
    ///
    /// The reader is placed at the end of that token and nowhere further, which
    /// is the whole difference: an alias replacement whose last character the
    /// token consumed has been popped by the look past it, and one with text
    /// still to come has not. So `alias A="echo $( for )"` is echoed against the
    /// script's own line and `alias A="echo $( for ) tail"` against the
    /// replacement — a distinction taking the echo one token later cannot make,
    /// because by then the reader has left the token in every case.
    fn echo_at(&self, at: usize) -> Option<Str> {
        self.spans.echo_line(at, Reader::of(self.toks.get(at)))
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn at_op(&self, op: Op) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    /// If the current token is an unquoted single-literal word naming a
    /// reserved word, return that word.
    ///
    /// The *matched entry of [`RESERVED`]* rather than the token's own bytes, so
    /// the answer is `&'static str` and every caller can go on comparing against
    /// string literals. The two are equal by construction — a reserved word is
    /// ASCII — and the source spelling is never needed here.
    fn reserved_here(&self) -> Option<&'static str> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
        {
            return RESERVED.iter().find(|r| r.as_bytes() == s.as_slice()).copied();
        }
        None
    }

    /// The literal text of a bare word token (single unquoted literal), if any.
    fn bare_word_here(&self) -> Option<Str> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
        {
            return Some(s.clone());
        }
        None
    }

    /// Whether the current token is exactly the bare word `w`.
    ///
    /// The common shape by far: the grammar asks about a fixed ASCII spelling
    /// (`[[`, `time`, `function`, `]]`), so nothing is gained by copying the
    /// token out first.
    fn at_bare_word(&self, w: BStr<'_>) -> bool {
        matches!(self.peek(), Some(Tok::Word(segs))
            if matches!(segs.as_slice(), [Seg::Lit(s)] if s.as_slice() == w))
    }

    /// The text of the word at `pos`, when it is a single unquoted literal
    /// segment — the same shape [`Parser::at_bare_word`] tests for, but read
    /// rather than compared, and at an arbitrary position so a lookahead can
    /// use it. A quoted, escaped or expanded word is `None` however it would
    /// expand (`"-p"`, `\-p` and `$D` all are), which is what bash's own
    /// lookaheads see: they run over the token as it was written, before any
    /// expansion.
    fn bare_word_at(&self, pos: usize) -> Option<BStr<'_>> {
        match self.toks.get(pos) {
            Some(Tok::Word(segs)) => match segs.as_slice() {
                [Seg::Lit(s)] => Some(s.as_slice()),
                _ => None,
            },
            _ => None,
        }
    }

    /// A short human-readable name for the current token, for syntax-error
    /// messages (mirrors bash's `near unexpected token '…'`).
    fn token_display(&self) -> Str {
        self.token_display_at(self.pos)
    }

    /// [`Parser::token_display`] for an arbitrary position, so a diagnostic can
    /// name a token the parser has already moved past (see
    /// [`Parser::cond_near`]).
    fn token_display_at(&self, pos: usize) -> Str {
        match self.toks.get(pos) {
            None => b"end of input".to_vec(),
            Some(Tok::Newline) => b"newline".to_vec(),
            Some(Tok::Op(op)) => match op {
                Op::DSemi => ";;",
                Op::SemiAmp => ";&",
                Op::DSemiAmp => ";;&",
                Op::LParen => "(",
                Op::RParen => ")",
                Op::Pipe => "|",
                Op::Amp => "&",
                Op::Semi => ";",
                Op::AndIf => "&&",
                Op::OrIf => "||",
                Op::PipeAmp => "|&",
                Op::Less => "<",
                Op::Great => ">",
                Op::DGreat => ">>",
                Op::GreatPipe => ">|",
                Op::GreatAnd => ">&",
                Op::LessAnd => "<&",
                Op::LessGreat => "<>",
                Op::AmpGreat => "&>",
                Op::AmpDGreat => "&>>",
                Op::DLess => "<<",
                Op::DLessDash => "<<-",
                Op::TLess => "<<<",
            }
            .as_bytes()
            .to_vec(),
            // bash names the offending word by its *source* spelling, quotes and
            // all: `[[ a '<' b ]]` is reported near `'<'`, not near a bare `<` or
            // a placeholder. Rebuild the word from its segments and print it the
            // way it was written. (One spelling this does not reach is `$'…'`,
            // which the lexer has already decoded — see
            // TD-OILS-ANSIC-ERROR-SPELLING in known-issues.md.)
            Some(Tok::Word(segs)) => word_from_segs(segs, self.opts)
                .map_or_else(|_| b"word".to_vec(), |w| crate::unparse::word_src(&w)),
            // A construct the lexer refused already carries the spelling to
            // blame — the operator that stood where an array element belonged.
            Some(Tok::Invalid(op)) => op.clone(),
            // Anything else (a newline, a here-doc body) has no word spelling.
            _ => b"word".to_vec(),
        }
    }

    /// Build bash's canonical "unexpected" parser diagnostic for the current
    /// position: at end of input it is `syntax error: unexpected end of file`;
    /// otherwise `syntax error near unexpected token \`TOKEN'` — bash quotes the
    /// offending token with a leading backtick and a trailing single quote.
    fn unexpected_here(&self) -> ParseError {
        if self.peek().is_none() {
            ParseError::new("syntax error: unexpected end of file")
        } else {
            let e = ParseError::new(&bfmt![
                b"syntax error near unexpected token `",
                self.token_display(),
                b"'"
            ]);
            // A refused construct costs only its own unit, where a grammar error
            // costs the rest of the input. See [`ParseError::recoverable`].
            if matches!(self.peek(), Some(Tok::Invalid(_))) {
                e.only_this_unit()
            } else {
                e
            }
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.pos += 1;
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.peek(),
            Some(Tok::Newline) | Some(Tok::Op(Op::Semi))
        ) {
            self.pos += 1;
        }
    }

    /// Parse a command list until EOF or one of `stops` (reserved words) or a
    /// closing `)`. When `allow_empty` is false — every compound-command
    /// condition/body, subshell, and brace group — an empty list is a syntax
    /// error, matching bash (`if ; then`, `( )`, `while true; do done`). Only the
    /// top-level program and command substitutions (`$( )`) pass `true`. A bare
    /// separator (`;`/`&`) with no preceding command is likewise rejected
    /// (`; echo`, `echo a ; ; echo b`) — blank *lines* between commands are fine.
    fn parse_program(&mut self, stops: &[&str], allow_empty: bool) -> Result<Program, ParseError> {
        // Every call but the two outermost ones ([`IncrementalParser::next_unit`]
        // drives `parse_item` directly, and `parse_tokens_ending` does not ask)
        // is the body of some compound, so the counter can live here rather than
        // at each of the dozen sites. See [`Parser::depth`] for what asks.
        self.depth = self.depth.saturating_add(1);
        let r = self.parse_program_inner(stops, allow_empty);
        // Unwound only on success: a failure goes straight to the stamping site,
        // which wants the depth the error was raised at.
        if r.is_ok() {
            self.depth = self.depth.saturating_sub(1);
        }
        r
    }

    fn parse_program_inner(
        &mut self,
        stops: &[&str],
        allow_empty: bool,
    ) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        while let Some(item) = self.parse_item(stops)? {
            items.push(item);
        }
        if items.is_empty() && !allow_empty {
            // A compound condition/body reduced to nothing (`if ; then`, `( )`,
            // `then fi`). bash reports the token that follows (the stop keyword,
            // `)`, or EOF).
            return Err(self.unexpected_here());
        }
        Ok(Program { items })
    }

    /// Parse exactly one item (one `;`/`&`/newline-terminated and-or list) of a
    /// command list, leaving the cursor on the token after its separator.
    /// Returns `Ok(None)` at the end of the list — EOF, a closing `)`, or one of
    /// the `stops` reserved words — having consumed only blank lines.
    ///
    /// Split out of [`Parser::parse_program`] so [`IncrementalParser`] can drive
    /// the same grammar one item at a time and execute between items.
    fn parse_item(&mut self, stops: &[&str]) -> Result<Option<Item>, ParseError> {
        // Blank lines between commands are fine; a bare `;`/`&` is not — it
        // denotes an empty command, which bash rejects.
        self.skip_newlines();
        if self.peek().is_none() || self.at_op(Op::RParen) {
            return Ok(None);
        }
        if let Some(w) = self.reserved_here()
            && stops.contains(&w)
        {
            return Ok(None);
        }
        if self.at_op(Op::Semi) || self.at_op(Op::Amp) {
            return Err(self.unexpected_here());
        }
        // Stamp the line on which this item begins (the lexer already accounts
        // for any newlines hidden inside earlier tokens).
        let line = self.cur_line();
        let list = self.parse_and_or()?;
        let mut background = false;
        let mut had_sep = false;
        match self.peek() {
            Some(Tok::Op(Op::Amp)) => {
                background = true;
                had_sep = true;
                self.pos += 1;
            }
            Some(Tok::Newline) => {
                had_sep = true;
                self.pos += 1;
            }
            Some(Tok::Op(Op::Semi)) => {
                had_sep = true;
                self.pos += 1;
            }
            _ => {}
        }
        // Without a separator (`;`, `&`, newline), the only valid follower is a
        // terminator for this context: end of input, a closing `)`, or a stop
        // keyword (`done`, `fi`, `esac`, `}`, …). Anything else — a bare word or
        // a stray reserved word/operator — means two commands abut with no
        // separator, which bash rejects as a syntax error (and which osh
        // previously mis-ran as a second command).
        if !had_sep {
            let at_terminator = self.peek().is_none()
                || self.at_op(Op::RParen)
                || self
                    .reserved_here()
                    .is_some_and(|w| stops.contains(&w));
            if !at_terminator {
                return Err(self.unexpected_here());
            }
        }
        Ok(Some(Item { list, background, line }))
    }

    fn parse_and_or(&mut self) -> Result<AndOr, ParseError> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::AndIf)) => AndOrOp::And,
                Some(Tok::Op(Op::OrIf)) => AndOrOp::Or,
                _ => break,
            };
            self.pos += 1;
            self.skip_newlines();
            let pipe = self.parse_pipeline()?;
            rest.push((op, pipe));
        }
        Ok(AndOr { first, rest })
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline, ParseError> {
        // bash's grammar lets `!` and `time` prefix a pipeline in any order and
        // any number, and lets either stand as a whole command on its own:
        //
        //     pipeline_command : pipeline
        //                      | BANG pipeline_command | BANG list_terminator
        //                      | timespec pipeline_command
        //                      | timespec list_terminator
        //
        // Only three flags survive that, which is why `declare -f` prints them
        // back in a fixed order rather than as written: repeated `time` is
        // idempotent, but each `!` toggles, so `! ! true` is just `true` and
        // `! time true` prints as `time ! true`.
        //
        // `time` is a reserved word only here, at the start of a pipeline; it
        // is deliberately not in RESERVED so it stays an ordinary word
        // elsewhere (`for x in time`, `echo time`).
        let mut negated = false;
        let mut timed = false;
        let mut time_posix = false;
        // Whether *any* prefix was read, which is not the same as any flag
        // being set: `! !` cancels out but is still a prefix, and still stands
        // as a whole command.
        let mut prefixed = false;
        loop {
            if self.reserved_here() == Some("!") {
                negated = !negated;
                prefixed = true;
                self.pos += 1;
                continue;
            }
            if self.at_bare_word(b"time") {
                // In posix mode `time` is the reserved word only when the word
                // after it does not look like an option: bash gives the
                // reserved word up there and searches for an external `time`
                // instead, so `time -p echo hi`, `time -- echo hi`,
                // `time -x echo hi` and even a bare `time -` are all
                // `time: command not found`. The test is on the word *as
                // written* — `time "-p" x`, `time \-p x` and `time $D x` keep
                // the reserved word and run a command named `-p` — which is the
                // same literal-word test `-p` itself is read under below, and
                // is what takes `time`'s own options away in that mode: they
                // are only ever read in this position. `time` with nothing
                // after it is untouched; that is the null-command form.
                if self.opts.posix
                    && self.bare_word_at(self.pos + 1).is_some_and(|w| w.starts_with(b"-"))
                {
                    break;
                }
                timed = true;
                prefixed = true;
                self.pos += 1;
                // `-p` (POSIX output format) and `--` (end of `time`'s own
                // options) are recognised only in this position, at most once
                // each and in this order, and only as literal unquoted words:
                // `time "-p" true` and `time $x true` run a *command* named
                // `-p`. `--` selects the POSIX format as well, so `time -- x`
                // and `time -p x` report identically — and `declare -f` prints
                // both back as `time -p`.
                if self.at_bare_word(b"-p") {
                    time_posix = true;
                    self.pos += 1;
                }
                if self.at_bare_word(b"--") {
                    time_posix = true;
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
        // A prefix with nothing after it is the `list_terminator` case above:
        // the flags apply to a null command, which succeeds — so a bare `!` is
        // status 1 and a bare `time` reports a timing and status 0. Only `;` or
        // a line end may stand here; `! && x`, `time | cat`, `( ! )` and
        // `$( ! )` are all syntax errors, which `parse_command` reports for us.
        // The last of those is why a `$( … )` body's stream ends with the
        // substitution's `)` rather than an implicit newline
        // ([`crate::lexer::tokenize_paren_body`]) — with a newline there, the
        // prefix would find a terminator bash does not give it.
        if prefixed && matches!(self.peek(), None | Some(Tok::Newline) | Some(Tok::Op(Op::Semi))) {
            let commands = vec![Command::Simple(SimpleCommand::default())];
            return Ok(Pipeline { negated, timed, time_posix, commands });
        }
        let mut commands = vec![self.parse_command()?];
        loop {
            // `cmd1 | cmd2` and `cmd1 |& cmd2`. The `|&` form is bash shorthand
            // for `cmd1 2>&1 | cmd2`: the *left* command additionally dups its
            // stderr onto stdout before the pipe carries both.
            let amp = if self.at_op(Op::Pipe) {
                false
            } else if self.at_op(Op::PipeAmp) {
                true
            } else {
                break;
            };
            self.pos += 1;
            if amp && let Some(prev) = commands.pop() {
                let dup = Redirect {
                    fd: 2,
                    op: RedirectOp::DupOut,
                    target: Word::literal("1"),
                    varfd: None,
                    here: None,
                };
                commands.push(attach_redirect(prev, dup));
            }
            self.skip_newlines();
            commands.push(self.parse_command()?);
        }
        Ok(Pipeline { negated, timed, time_posix, commands })
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        if let Some(w) = self.reserved_here() {
            let cmd = match w {
                "if" => self.parse_if()?,
                "while" => self.parse_loop(false)?,
                "until" => self.parse_loop(true)?,
                "for" => self.parse_for()?,
                "select" => self.parse_select()?,
                "case" => self.parse_case()?,
                "{" => self.parse_brace_group()?,
                _ => {
                    // A command that begins with a stray closing/continuation
                    // keyword (`then`, `do`, `fi`, `done`, `esac`, `else`, …):
                    // bash reports it as an unexpected token.
                    return Err(self.unexpected_here());
                }
            };
            return self.with_redirects(cmd);
        }
        if self.at_op(Op::LParen) {
            let cmd = self.parse_subshell()?;
            return self.with_redirects(cmd);
        }
        // `(( expr ))` arithmetic command (lexed as a single token).
        if let Some(Tok::ArithCmd(raw)) = self.peek() {
            let raw = raw.clone();
            self.pos += 1;
            return self.with_redirects(Command::Arith(raw));
        }
        // `[[ expr ]]` conditional expression.
        if self.at_bare_word(b"[[") {
            let cmd = self.parse_cond()?;
            return self.with_redirects(cmd);
        }
        // Function definition: `WORD ( )`.
        //
        // bash's production is `WORD '(' ')' …` — *any* word, not just an
        // identifier. `my-func`, `a.b`, `1f`, `a[b]`, `[b]` and `f*` all define
        // functions. The one shape excluded is an assignment, because the lexer
        // hands that back as an ASSIGNMENT_WORD, which the production does not
        // accept: `f=g() { :; }` is a syntax error. Escaping the `=` demotes the
        // word back to a plain WORD, so `a\=b() { :; }` parses — and then fails
        // at run time, being quoted (see below).
        //
        // A quoted or expanded name parses here too; bash rejects it only when
        // the definition *executes*. See [`FunctionDef::definable`].
        if matches!(self.peek(), Some(Tok::Word(segs)) if !word_is_assignment(segs))
            && matches!(self.toks.get(self.pos + 1), Some(Tok::Op(Op::LParen)))
        {
            // bash's parser has shifted the `(` by the time it looks for the
            // `)`, so from here there is no other production to fall back to:
            // whatever stands where the `)` should be *is* the error, and is
            // the token the diagnostic names. `a ( b` is reported near `b`,
            // and `a (` — where only the line's own newline follows — near
            // `newline`, neither of them near the `(` that was accepted.
            if !matches!(self.toks.get(self.pos + 2), Some(Tok::Op(Op::RParen))) {
                self.pos += 2;
                return Err(self.unexpected_here());
            }
            // A name written as a bare word is definable and *is* its literal;
            // any other spelling is not, and is kept as written so the run-time
            // error can quote it back exactly as typed.
            let bare = self.bare_word_here();
            let definable = bare.is_some();
            // Not definable: the name is only ever quoted back in a run-time
            // error, so its source spelling (a `String` until step 10 of
            // TD-OILS-BYTE-STRINGS) is what there is to store.
            let name = bare.unwrap_or_else(|| self.token_display());
            self.pos += 3;
            self.skip_newlines();
            let body = self.parse_compound_body()?;
            // bash allows redirections after the body (`f() { …; } >log`); they
            // are stored with the function and applied on every invocation.
            let mut redirects = Vec::new();
            while self.at_redirect_start() {
                redirects.push(self.parse_redirect()?);
            }
            return Ok(Command::Function(FunctionDef { name, definable, body, redirects }));
        }
        // `function NAME [()] body` — bash keyword form of a function
        // definition (recognised only at command start).
        if self.at_bare_word(b"function") {
            return self.parse_function_keyword();
        }
        // `coproc [NAME] command` — bash reserved word, recognised only at
        // command start.
        if self.at_bare_word(b"coproc") {
            return self.parse_coproc();
        }
        self.parse_simple()
    }

    /// Parse the bash keyword form of a function definition:
    /// `function NAME [( )] compound-body`. Unlike the POSIX `NAME ( )` form the
    /// parentheses are optional, and an assignment-shaped name is accepted
    /// (`function f=g { …; }` defines `f=g`) because the lexer only forms an
    /// assignment word at the start of a command. Otherwise the name rule is the
    /// same: any word parses, and a quoted or expanded one is refused at run
    /// time — see [`FunctionDef::definable`].
    fn parse_function_keyword(&mut self) -> Result<Command, ParseError> {
        self.pos += 1; // consume `function`
        if !matches!(self.peek(), Some(Tok::Word(_))) {
            return Err(self.unexpected_here());
        }
        let bare = self.bare_word_here();
        let definable = bare.is_some();
        let name = bare.unwrap_or_else(|| self.token_display());
        self.pos += 1; // consume the name word
        // Optional `()` after the name.
        if self.at_op(Op::LParen) {
            if !matches!(self.toks.get(self.pos + 1), Some(Tok::Op(Op::RParen))) {
                return Err(self.unexpected_here());
            }
            self.pos += 2;
        }
        self.skip_newlines();
        let body = self.parse_compound_body()?;
        // bash allows redirections after the body (`function f { …; } >log`);
        // they are stored with the function and applied on every invocation.
        let mut redirects = Vec::new();
        while self.at_redirect_start() {
            redirects.push(self.parse_redirect()?);
        }
        Ok(Command::Function(FunctionDef { name, definable, body, redirects }))
    }

    /// Parse a `coproc [NAME] command`. Grammar (matches bash):
    /// - `coproc simple_command` → default name `COPROC` (an explicit NAME is
    ///   *not* accepted before a simple command).
    /// - `coproc NAME compound_command` → explicit NAME (only when a valid
    ///   identifier is immediately followed by a compound-command starter).
    /// - `coproc compound_command` → default name `COPROC`.
    fn parse_coproc(&mut self) -> Result<Command, ParseError> {
        self.pos += 1; // consume `coproc`
        let mut name = None;
        if let Some(w) = self.bare_word_here()
            && is_valid_name(&w)
            && self.compound_starts_at(self.pos + 1)
        {
            // `is_valid_name` has already established that the word is an ASCII
            // identifier, so the conversion cannot fail; a `None` here would
            // simply leave the coproc unnamed, as if no name had been written.
            name = bytes::as_str(&w).map(str::to_owned);
            self.pos += 1;
        }
        let body = self.parse_command()?;
        Ok(Command::Coproc { name, body: Box::new(body) })
    }

    /// Whether the token at `idx` begins a compound command (`{`, `(`, `((`,
    /// `[[`, or a control keyword). Used to decide whether the word after
    /// `coproc` is an explicit array name or the command itself.
    fn compound_starts_at(&self, idx: usize) -> bool {
        match self.toks.get(idx) {
            Some(Tok::Op(Op::LParen)) | Some(Tok::ArithCmd(_)) => true,
            Some(Tok::Word(segs)) => {
                if let [Seg::Lit(s)] = segs.as_slice() {
                    matches!(
                        s.as_slice(),
                        b"{" | b"[[" | b"if" | b"while" | b"until" | b"for" | b"select" | b"case"
                    )
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Is the current token the start of a redirection (`<`, `>`, `>>`, `2>`, …)?
    fn at_redirect_start(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Io(_))
                | Some(Tok::VarFd(_))
                | Some(Tok::Op(
                    Op::Less
                        | Op::Great
                        | Op::GreatPipe
                        | Op::DGreat
                        | Op::GreatAnd
                        | Op::LessAnd
                        | Op::LessGreat
                        | Op::AmpGreat
                        | Op::AmpDGreat
                        | Op::DLess
                        | Op::DLessDash
                        | Op::TLess,
                ))
        )
    }

    /// Attach any trailing redirections to a compound command, wrapping it in a
    /// [`Command::Redirected`] when at least one is present.
    fn with_redirects(&mut self, inner: Command) -> Result<Command, ParseError> {
        let mut redirects = Vec::new();
        while self.at_redirect_start() {
            redirects.push(self.parse_redirect()?);
        }
        if redirects.is_empty() {
            Ok(inner)
        } else {
            Ok(Command::Redirected {
                inner: Box::new(inner),
                redirects,
            })
        }
    }

    /// Parse a `{ … }` or `( … )` body used as a function body.
    fn parse_compound_body(&mut self) -> Result<Program, ParseError> {
        if let Some(w) = self.reserved_here()
            && w == "{"
            && let Command::BraceGroup(p) = self.parse_brace_group()?
        {
            return Ok(p);
        }
        if self.at_op(Op::LParen) {
            // A `( … )` function body is a *subshell* body, and the parentheses
            // are part of the function, not a wrapper the definition strips:
            // `f() ( cd /; x=1 )` must leak neither the `cd` nor the `x`, and an
            // `exit` inside must end only the subshell. Keep the `Subshell` node
            // as the body's single statement rather than unwrapping it into the
            // function's own `Program` — osh used to unwrap, which made every
            // such function run in the caller's shell. It also renders the way
            // bash's `declare -f` does: brace-wrapped, `( … )` inside.
            let line = self.cur_line();
            let sub = self.parse_subshell()?;
            return Ok(Program {
                items: vec![Item {
                    list: AndOr {
                        first: Pipeline {
                            negated: false,
                            timed: false,
                            time_posix: false,
                            commands: vec![sub],
                        },
                        rest: Vec::new(),
                    },
                    background: false,
                    line,
                }],
            });
        }
        // Not a valid compound body. bash diagnoses this positionally: at EOF
        // (`f()` / `function f` with no body) it reports "unexpected end of
        // file"; otherwise it names the offending token (`f() echo hi` →
        // "unexpected token `echo'"), matching both function-definition forms.
        Err(self.unexpected_here())
    }

    fn parse_brace_group(&mut self) -> Result<Command, ParseError> {
        // Consume `{`.
        self.pos += 1;
        let body = self.parse_program(&["}"], false)?;
        self.expect_reserved("}")?;
        Ok(Command::BraceGroup(body))
    }

    fn parse_subshell(&mut self) -> Result<Command, ParseError> {
        // Consume `(`.
        self.pos += 1;
        let body = self.parse_program(&[], false)?;
        if !self.at_op(Op::RParen) {
            // bash names the offending token here (`near unexpected token \`)'`
            // for `( )`, `unexpected end of file` for an unclosed `( echo hi`).
            return Err(self.unexpected_here());
        }
        self.pos += 1;
        Ok(Command::Subshell(body))
    }

    fn parse_if(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("if")?;
        let cond = self.parse_program(&["then"], false)?;
        self.expect_reserved("then")?;
        let body = self.parse_program(&["elif", "else", "fi"], false)?;
        let mut elifs = Vec::new();
        while self.reserved_here() == Some("elif") {
            self.pos += 1;
            let c = self.parse_program(&["then"], false)?;
            self.expect_reserved("then")?;
            let b = self.parse_program(&["elif", "else", "fi"], false)?;
            elifs.push((c, b));
        }
        let else_body = if self.reserved_here() == Some("else") {
            self.pos += 1;
            Some(self.parse_program(&["fi"], false)?)
        } else {
            None
        };
        self.expect_reserved("fi")?;
        Ok(Command::If(IfClause {
            cond,
            body,
            elifs,
            else_body,
        }))
    }

    fn parse_loop(&mut self, until: bool) -> Result<Command, ParseError> {
        self.expect_reserved(if until { "until" } else { "while" })?;
        let cond = self.parse_program(&["do"], false)?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::Loop(LoopClause { until, cond, body }))
    }

    fn parse_for(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("for")?;
        // C-style `for (( init; cond; update ))` — the `(( … ))` lexes as a
        // single `ArithCmd` token carrying the raw `init; cond; update` text.
        if let Some(Tok::ArithCmd(raw)) = self.peek() {
            let raw = raw.clone();
            self.pos += 1;
            return self.parse_for_arith(&raw);
        }
        if !matches!(self.peek(), Some(Tok::Word(_))) {
            // `for` with no loop variable (`for; do …`, `for` at EOF, `for |`):
            // bash names the unexpected token / reports end of input.
            return Err(self.unexpected_here());
        }
        // *Any* word goes here. bash's grammar asks only for a WORD and leaves
        // identifier-ness to `execute_for_command`, so `for 'a[0]'` parses and
        // then fails at run time with status 1 — where a syntax error would
        // have abandoned the rest of the parse unit. The word is stored by its
        // source spelling because that is what bash checks (`"x"` is refused
        // though `x` is fine) and what it quotes back. See `Shell::exec_for`.
        let var = self.token_display();
        self.pos += 1;
        self.skip_newlines();
        let words = self.parse_in_list()?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::For(ForClause { var, words, body }))
    }

    /// Parse the `in …` word list shared by `for` and `select`, positioned at
    /// the `in` (or at whatever follows the loop variable when there is none).
    ///
    /// A reserved word is **not** recognised here. bash's lexer only promotes a
    /// token to a reserved word in command position, and this list is not one:
    /// `for x in if then do done; do echo "$x"; done` iterates over the four
    /// literals, and `select o in a fi b` offers `fi` as choice 2. The list runs
    /// until the first token that is not a word at all — `;`, a newline, `|`,
    /// `&`, `)` — which is also why `for x in a b do echo hi; done`, with no
    /// separator before `do`, is a *syntax error* in bash: `do` was swallowed by
    /// the list, so the `do` the grammar needs never arrives and the error is
    /// reported at `done`.
    ///
    /// `None` means there was no `in` at all (`for x; do …` iterates `"$@"`),
    /// which is distinct from `Some(vec![])` for an empty list (`for x in; do …`
    /// iterates nothing).
    fn parse_in_list(&mut self) -> Result<Option<Vec<Word>>, ParseError> {
        if self.reserved_here() != Some("in") {
            self.skip_separators();
            return Ok(None);
        }
        self.pos += 1;
        let mut ws = Vec::new();
        while let Some(Tok::Word(segs)) = self.peek() {
            let segs = segs.clone();
            let at = self.pos;
            self.pos += 1;
            ws.push(self.word_from_segs_at(&segs, at)?);
        }
        self.skip_separators();
        Ok(Some(ws))
    }

    /// Parse `select name [in words]; do body; done`. Structurally identical to
    /// the word-list `for` loop; the runtime difference is the interactive menu.
    fn parse_select(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("select")?;
        if !matches!(self.peek(), Some(Tok::Word(_))) {
            return Err(self.unexpected_here());
        }
        // Same rule as the word-list `for` loop: any word parses and the name is
        // checked where the loop runs; see `parse_for`.
        let var = self.token_display();
        self.pos += 1;
        self.skip_newlines();
        let words = self.parse_in_list()?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::Select(SelectClause { var, words, body }))
    }

    /// Parse the body of a C-style `for (( init; cond; update ))` loop, given
    /// the raw `init; cond; update` text captured from the arithmetic token.
    /// The three sections are split on `;`; an omitted section is empty (an
    /// empty condition is treated as always-true at run time).
    fn parse_for_arith(&mut self, raw: BStr<'_>) -> Result<Command, ParseError> {
        let parts: Vec<BStr<'_>> = raw.split(|&b| b == b';').collect();
        if parts.len() != 3 {
            return Err(ParseError::new("C-style for loop requires 'for (( init; cond; update ))'"));
        }
        // Only the *leading* whitespace is dropped. bash keeps each section's
        // source text from its first non-blank character onwards, which shows up
        // when a function is printed back by `declare -f`: `for (( i=0; i<2;
        // i++ ))` comes back as `for ((i=0; i<2; i++ ))`, trailing space and
        // all. The arithmetic evaluator ignores the whitespace either way.
        let init = bytes::trim_start(parts.first().copied().unwrap_or_default()).to_vec();
        let cond = bytes::trim_start(parts.get(1).copied().unwrap_or_default()).to_vec();
        let update = bytes::trim_start(parts.get(2).copied().unwrap_or_default()).to_vec();
        // An optional separator (`;`/newline) may precede `do`.
        self.skip_separators();
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::ForArith(ForArithClause {
            init,
            cond,
            update,
            body,
        }))
    }

    fn parse_case(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("case")?;
        let Some(Tok::Word(segs)) = self.peek() else {
            // bash names the offending token (`case ;` → `near unexpected token
            // \`;'`), or reports end of input.
            return Err(self.unexpected_here());
        };
        let word = self.word_from_segs_at(&segs.clone(), self.pos)?;
        self.pos += 1;
        self.skip_newlines();
        self.expect_reserved("in")?;
        self.skip_newlines();
        let mut items = Vec::new();
        while self.reserved_here() != Some("esac") {
            if self.peek().is_none() {
                // Unterminated `case` at end of input: bash reports
                // `syntax error: unexpected end of file`.
                return Err(self.unexpected_here());
            }
            // Optional leading '(' before the pattern list.
            if self.at_op(Op::LParen) {
                self.pos += 1;
            }
            // Pattern list: word ['|' word]*.
            let mut patterns = Vec::new();
            loop {
                let Some(Tok::Word(segs)) = self.peek() else {
                    // bash names the offending token (`case x in ;&` → `near
                    // unexpected token \`;&'`, `case x in )` → \`)').
                    return Err(self.unexpected_here());
                };
                patterns.push(self.word_from_segs_at(&segs.clone(), self.pos)?);
                self.pos += 1;
                if self.at_op(Op::Pipe) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
            if !self.at_op(Op::RParen) {
                // bash names the offending token (`case x in pat esac` → `near
                // unexpected token \`esac'`).
                return Err(self.unexpected_here());
            }
            self.pos += 1;
            let body = self.parse_case_body()?;
            // Determine the arm terminator: `;;` break, `;&` fall through,
            // `;;&` continue matching. A `;;`-less arm before `esac` breaks.
            let term = if self.at_op(Op::DSemiAmp) {
                self.pos += 1;
                self.skip_newlines();
                CaseTerm::ContinueMatch
            } else if self.at_op(Op::SemiAmp) {
                self.pos += 1;
                self.skip_newlines();
                CaseTerm::FallThrough
            } else if self.at_op(Op::DSemi) {
                self.pos += 1;
                self.skip_newlines();
                CaseTerm::Break
            } else {
                // Only `esac` may legitimately follow a terminator-less arm body.
                self.skip_newlines();
                CaseTerm::Break
            };
            items.push(CaseItem { patterns, body, term });
        }
        self.expect_reserved("esac")?;
        Ok(Command::Case(CaseClause { word, items }))
    }

    /// Parse a `case`-arm body: a command list terminated by `;;` or `esac`.
    fn parse_case_body(&mut self) -> Result<Program, ParseError> {
        // A nested list like any other, but built from `parse_and_or` rather
        // than [`Parser::parse_program`], so it keeps [`Parser::depth`] itself.
        self.depth = self.depth.saturating_add(1);
        let r = self.parse_case_body_inner();
        if r.is_ok() {
            self.depth = self.depth.saturating_sub(1);
        }
        r
    }

    fn parse_case_body_inner(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none()
                || self.at_op(Op::DSemi)
                || self.at_op(Op::SemiAmp)
                || self.at_op(Op::DSemiAmp)
                || self.reserved_here() == Some("esac")
            {
                break;
            }
            let line = self.cur_line();
            let list = self.parse_and_or()?;
            let mut background = false;
            match self.peek() {
                Some(Tok::Op(Op::Amp)) => {
                    background = true;
                    self.pos += 1;
                }
                Some(Tok::Newline) => {
                    self.pos += 1;
                }
                Some(Tok::Op(Op::Semi)) => {
                    self.pos += 1;
                }
                _ => {}
            }
            items.push(Item { list, background, line });
        }
        Ok(Program { items })
    }

    /// Parse a `[[ … ]]` conditional expression. The opening `[[` word is at
    /// the current position; parsing stops at the matching `]]` word.
    fn parse_cond(&mut self) -> Result<Command, ParseError> {
        // bash's `cond_error` — the frame that speaks when a *complete*
        // expression is followed by something that is not `]]` — reports at
        // `cond_lineno`, the line the `[[` itself was on, however far down the
        // stray token is. Only the `near` line that follows it comes from where
        // the reader stopped:
        //
        //   [[ a &&        line 1: syntax error in conditional expression
        //   b -gt c -gt d ]]
        //                  line 2: syntax error near `-gt'
        let open = self.cur_line();
        // Consume `[[`.
        self.pos += 1;
        // Nothing encloses this frame, so whatever comes back is complete.
        let expr = self.parse_cond_or().map_err(CondError::into_parse_error)?;
        if !self.at_bare_word(b"]]") {
            // A complete expression but no closer: bash emits
            // `unexpected EOF while looking for \`]]'` then `syntax error:
            // unexpected end of file` (no source echo). If a stray token sits
            // where `]]` should be, name it the ordinary way.
            //
            // Only the second of those two lines gets bash's end-of-file line
            // number. The first is the reader saying where it gave up, which is
            // the last line it actually read — so `[[ -n x ` on line 1 of a file
            // reports `line 1` and then `line 2`.
            if self.peek().is_none() {
                return Err(ParseError {
                    line_at: vec![(0, self.cur_line())],
                    ..ParseError::new(
                        "unexpected EOF while looking for `]]'\nsyntax error: unexpected end of file",
                    )
                });
            }
            // A complete sub-expression followed by a stray token where `]]` was
            // expected. bash reports this as `syntax error in conditional
            // expression` + `syntax error near \`TOKEN'` (TD-OILS-COND-ERRTEXT).
            // This covers a leftover operator after a finished operand
            // (`[[ 3 -gt 2 -gt 1 ]]`, near `-gt`) and a leftover word after a
            // non-word primary (`[[ -z x y ]]`, near `y`). An *operator* — `)`,
            // `(`, `;`, `|`, `>>`, … — additionally carries bash's
            // `: unexpected token \`X'` suffix on the first line, which is how
            // bash distinguishes a token it can name from a word it cannot.
            let tok = self.token_display();
            if matches!(self.peek(), Some(Tok::Op(_))) {
                let near = self.cond_near_at(self.pos);
                return Err(ParseError {
                    line_at: vec![(0, open)],
                    ..ParseError::new(&bfmt![
                        b"syntax error in conditional expression: unexpected token `",
                        &tok,
                        b"'\nsyntax error near `",
                        near,
                        b"'"
                    ])
                });
            }
            let near = self.cond_near_at(self.pos);
            return Err(ParseError {
                line_at: vec![(0, open)],
                ..ParseError::new(&bfmt![
                    b"syntax error in conditional expression\nsyntax error near `",
                    near,
                    b"'"
                ])
            });
        }
        self.pos += 1;
        Ok(Command::Cond(expr))
    }

    fn parse_cond_or(&mut self) -> Result<CondExpr, CondError> {
        let mut left = self.parse_cond_and()?;
        while self.at_op(Op::OrIf) {
            self.pos += 1;
            let right = self.parse_cond_and()?;
            left = CondExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_and(&mut self) -> Result<CondExpr, CondError> {
        let mut left = self.parse_cond_not()?;
        while self.at_op(Op::AndIf) {
            self.pos += 1;
            let right = self.parse_cond_not()?;
            left = CondExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_not(&mut self) -> Result<CondExpr, CondError> {
        // A term may start on a later line than the operator that introduced it.
        self.skip_cond_newlines();
        if self.peek().is_none() {
            // Waiting for a term and the input ran out. bash names the token it
            // did not get, rather than reporting a missing `]]` — that message
            // is reserved for an expression that *was* complete.
            return Err(CondError::new(
                "unexpected token `EOF' in conditional command",
                "syntax error: unexpected end of file",
            ));
        }
        if self.at_bare_word(b"!") {
            self.pos += 1;
            let inner = self.parse_cond_not()?;
            return Ok(CondExpr::Not(Box::new(inner)));
        }
        self.parse_cond_primary()
    }

    fn parse_cond_primary(&mut self) -> Result<CondExpr, CondError> {
        // Parenthesised sub-expression.
        if self.at_op(Op::LParen) {
            // bash's `cond_term` saves the line it started on and reports its
            // own `expected \`)'` there, however far the failure inside is.
            let open = self.cur_line();
            self.pos += 1;
            let inner = self.parse_cond_or().map_err(|e| e.in_group(open))?;
            if !self.at_op(Op::RParen) {
                // A parsed sub-expression but no `)`: bash says `unexpected
                // token \`X', expected \`)'` (+ the `near \`X'` echo), and spells
                // the token `EOF` when the input simply ran out — with no `near`
                // line then, since there is nothing to point at.
                if self.peek().is_none() {
                    return Err(CondError::Cond {
                        clauses: vec![(Some(open), b"unexpected token `EOF', expected `)'".to_vec())],
                        tail: b"syntax error: unexpected end of file".to_vec(),
                    });
                }
                let tok = self.token_display();
                return Err(CondError::Cond {
                    clauses: vec![(
                        Some(open),
                        bfmt![b"unexpected token `", &tok, b"', expected `)'"],
                    )],
                    tail: bfmt![b"syntax error near `", &tok, b"'"],
                });
            }
            self.pos += 1;
            // A finished term may be followed by a newline before whatever comes
            // next (`&&`, `||`, `]]`).
            self.skip_cond_newlines();
            // Keep the grouping in the tree. It changes nothing about the
            // result — the nesting below already reflects it — but `declare -f`
            // has to print the parentheses back, and without them the
            // expression would re-parse under the default precedence.
            return Ok(CondExpr::Group(Box::new(inner)));
        }
        // Unary operator: `-f WORD`, `-z WORD`, …
        if let Some(text) = self.bare_word_here()
            && let Some(op) = unary_op_from(&text)
        {
            self.pos += 1;
            // No newline skip before the operand: bash reads it directly, so
            // `[[ -n\na ]]` is an error even though `[[ -n a\n]]` is not.
            let operand = self.expect_cond_word(CondPos::Unary)?;
            self.skip_cond_newlines();
            return Ok(CondExpr::Unary(op, operand));
        }
        // Otherwise: WORD [ binop WORD ].
        let left = self.expect_cond_word(CondPos::Primary)?;
        if let Some((op, op_text)) = self.peek_cond_binop() {
            self.advance_cond_binop();
            let right = self.expect_cond_word(CondPos::Binary)?;
            self.skip_cond_newlines();
            if matches!(op, RawBinOp::Regex) {
                return Ok(CondExpr::Regex(Box::new(left), Box::new(right)));
            }
            return Ok(CondExpr::Binary(
                Box::new(left),
                op.into_bin_op(op_text),
                Box::new(right),
            ));
        }
        // A bare word primary must be followed by `]]`, `&&`, `||`, `)`, or end
        // of input. If instead another *word* token sits here — a plain operand
        // (`[[ a b ]]`), a non-`[[` operator like `-a` (`[[ a -a b ]]`), or a
        // unary operator used as an operand (`[[ a -z ]]`) — bash was expecting a
        // binary operator and reports `conditional binary operator expected`
        // followed by `syntax error near \`TOKEN'` (TD-OILS-COND-ERRTEXT). A
        // stray `)` is *not* caught here: it is a structural token that
        // `parse_cond` reports with its own "unexpected token" form.
        if let Some(Tok::Word(segs)) = self.peek()
            && !matches!(segs.as_slice(), [Seg::Lit(s)] if s.as_slice() == b"]]")
        {
            // The `near` text is the source slice here as everywhere else, not
            // the word: bash reaches `report_syntax_error` with its reader
            // parked just past the word, so what comes along is whatever was
            // *written* around it — `[[ a b;c ]]` is near `;`, `[[ a b) ]]`
            // near `b)`, and `[[ a $(echo x) ]]` near `x)`.
            let near = self.cond_near_at(self.pos);
            return Err(CondError::new(
                b"conditional binary operator expected".as_slice(),
                &bfmt![b"syntax error near `", near, b"'"],
            ));
        }
        // A newline here is *not* skipped — this is the one position where the
        // binary operator would go, and bash reads it without skipping so that
        // it can tell `[[ a == b ]]` from a bare-word test. So `[[ a` at end of
        // a line is an error, unlike `[[ -n a` which is a finished term. bash
        // can name the token it found, so the message gains an "unexpected
        // token" clause the word form above does not have.
        if matches!(self.peek(), Some(Tok::Newline)) {
            let tok = self.token_display();
            let near = self.cond_near();
            return Err(CondError::new(
                &bfmt![
                    b"unexpected token `",
                    tok,
                    b"', conditional binary operator expected"
                ],
                &bfmt![b"syntax error near `", near, b"'"],
            ));
        }
        // Any other *operator* here is in the same position and is named the
        // same way — the "unexpected token" clause is what distinguishes an
        // operator from a word, not a newline from everything else. So
        // `[[ a ( b ]]`, `[[ a; b ]]`, `[[ a | b ]]` and `[[ a >> b ]]` all
        // report their own token. The exceptions are the three tokens that may
        // legitimately follow a finished operand: `&&` and `||` continue the
        // expression, and `)` closes a group (or, unmatched, is reported
        // structurally by `parse_cond` with its own message form).
        if let Some(Tok::Op(op)) = self.peek()
            && !matches!(op, Op::AndIf | Op::OrIf | Op::RParen)
        {
            let tok = self.token_display();
            let near = self.cond_near_at(self.pos);
            return Err(CondError::new(
                &bfmt![
                    b"unexpected token `",
                    tok,
                    b"', conditional binary operator expected"
                ],
                &bfmt![b"syntax error near `", near, b"'"],
            ));
        }
        Ok(CondExpr::Word(left))
    }

    /// Skip newline tokens inside `[[ … ]]`.
    ///
    /// bash's conditional parser skips newlines wherever it is waiting for the
    /// *start* of a term, or for whatever follows a finished one, so a long
    /// conditional can be broken across lines around its `&&`/`||`. It does not
    /// skip in the two positions where it reads a token directly: after a bare
    /// word (where a binary operator might follow) and at a binary operator's
    /// right-hand operand. That asymmetry is why `[[ a == b\n]]` parses but
    /// `[[ a\n== b ]]` does not.
    fn skip_cond_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.pos += 1;
        }
    }

    /// The token a conditional error is reported "near".
    ///
    /// bash names the last *word* it read, which is the offending token itself
    /// whenever that is a real word (`[[ -n ]]` reports near `]]`). A newline
    /// never becomes that token, so an error on one is reported near whatever
    /// came before it: `[[ a` reports near `a`, and `[[ a -eq` near `-eq`.
    fn cond_near(&self) -> Str {
        let mut pos = self.pos;
        if matches!(self.peek(), Some(Tok::Newline)) {
            // Walk back over any newlines to the last real token. That is where
            // bash's own scan lands too: its input line ends at the newline, so
            // stepping back off the end skips it and any space before it.
            while pos > 0 && matches!(self.toks.get(pos), None | Some(Tok::Newline)) {
                pos -= 1;
            }
        }
        self.cond_near_at(pos)
    }

    /// [`Parser::cond_near`] for a known token position: the source slice bash
    /// would print, or — when there is no source behind this stream — the
    /// token itself, trimmed the way [`cond_error_near`] describes.
    fn cond_near_at(&self, pos: usize) -> Str {
        self.spans
            .near(pos, Reader::of(self.toks.get(pos)))
            .unwrap_or_else(|| cond_error_near(&self.token_display_at(pos)))
    }

    /// Expect a word operand inside `[[ … ]]` (not an operator/closer). `pos`
    /// tells us what bash would say when the operand is missing: after a unary
    /// or binary operator bash prepends `unexpected argument \`X' to conditional
    /// {unary,binary} operator`, whereas in primary position it reports only
    /// `syntax error near \`X'`.
    fn expect_cond_word(&mut self, pos: CondPos) -> Result<Word, CondError> {
        if let Some(Tok::Word(segs)) = self.peek() {
            // `]]` is the closer, never an operand.
            if !matches!(segs.as_slice(), [Seg::Lit(s)] if s.as_slice() == b"]]") {
                let segs = segs.clone();
                let at = self.pos;
                self.pos += 1;
                return Ok(self.word_from_segs_at(&segs, at)?);
            }
        }
        Err(self.cond_operand_error(pos))
    }

    /// How bash names the offending token in `unexpected token X in conditional
    /// command`, already quoted — or `None` where it prints no such line.
    ///
    /// bash's `cond_term` reaches that message from its final `else`, i.e. for
    /// every token that cannot *begin* a term. In practice that is exactly the
    /// operators: a word is a term, and the one word that is not — the `]]`
    /// closer — leaves through an earlier arm that prints only the `near` line.
    ///
    /// The quoting is bash's `error_token_from_token`, which spells the token
    /// when it can and returns nothing when it cannot. It cannot for
    /// `IO_NUMBER` and `REDIR_WORD` (they carry a number and a word, not a fixed
    /// text), and bash then falls through to a `%d` of the raw yacc token
    /// number — so `[[ 2>Q ]]` really does say `unexpected token 284`, with no
    /// quotes. The two numbers are from bash 5.2's generated `y.tab.h`.
    fn cond_primary_token(&self) -> Option<Str> {
        match self.peek() {
            Some(Tok::VarFd(_)) => Some(b"283".to_vec()),
            Some(Tok::Io(_)) => Some(b"284".to_vec()),
            Some(Tok::Op(_)) => Some(bfmt![b"`", self.token_display(), b"'"]),
            _ => None,
        }
    }

    /// Build bash's diagnostic for a missing/`]]`-filled operand slot inside
    /// `[[ … ]]`. When the offending token is present, bash echoes the source
    /// line (handled by `format_parse_error`); at end of input it uses an
    /// implicit-`newline` model we don't reproduce, so we fall back to a plain
    /// end-of-file diagnostic there.
    fn cond_operand_error(&self, pos: CondPos) -> CondError {
        if self.peek().is_none() {
            // End of input in primary position is the one place bash *does*
            // name the token: `cond_term` calls `read_token`, is handed `EOF`,
            // and falls to the same `else` an operator falls to.
            let eof = "syntax error: unexpected end of file";
            return match pos {
                CondPos::Primary => {
                    CondError::new("unexpected token `EOF' in conditional command", eof)
                }
                CondPos::Unary | CondPos::Binary => CondError::bare(eof),
            };
        }
        let tok = self.token_display();
        // A newline never becomes the token bash reports "near", so an operand
        // slot that a line end walked into names the operator instead.
        let near = bfmt![b"syntax error near `", self.cond_near(), b"'"];
        match pos {
            CondPos::Primary => match self.cond_primary_token() {
                Some(t) => CondError::new(
                    &bfmt![b"unexpected token ", t, b" in conditional command"],
                    &near,
                ),
                None => CondError::bare(&near),
            },
            CondPos::Unary => CondError::new(
                &bfmt![
                    b"unexpected argument `",
                    tok,
                    b"' to conditional unary operator"
                ],
                &near,
            ),
            CondPos::Binary => CondError::new(
                &bfmt![
                    b"unexpected argument `",
                    tok,
                    b"' to conditional binary operator"
                ],
                &near,
            ),
        }
    }

    /// Peek at a binary operator following an operand, without consuming.
    fn peek_cond_binop(&self) -> Option<(RawBinOp, &'static str)> {
        match self.peek() {
            Some(Tok::Op(Op::Less)) => Some((RawBinOp::StrLt, "<")),
            Some(Tok::Op(Op::Great)) => Some((RawBinOp::StrGt, ">")),
            Some(Tok::Word(segs)) => {
                if let [Seg::Lit(s)] = segs.as_slice() {
                    raw_binop_from(s)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn advance_cond_binop(&mut self) {
        self.pos += 1;
    }

    /// The line to stamp on a simple command, matching what bash's parser
    /// records for it.
    ///
    /// bash builds the `SIMPLE_COM` node when its grammar reduces the command's
    /// *first* element, stamping it with `line_number` as it stands at that
    /// moment. For a leading plain word the reduction cannot happen until one
    /// more token has been read: the parser must see whether a `(` follows,
    /// which would make this a function definition (`WORD '(' ')' …`) instead.
    /// A leading redirection is the same shape — its own reduction consumes the
    /// target word — so in both cases the line comes from the token *after* the
    /// first. Only a leading assignment (`v=1 cmd …`, `a=(1) cmd …`) reduces on
    /// sight, and is numbered by itself.
    ///
    /// The distinction is visible whenever the second token spans lines. All of
    /// these were verified against bash 5.2:
    ///
    /// | source | `$LINENO` |
    /// |---|---|
    /// | `echo "a⏎b" $LINENO` | 2 |
    /// | `echo $LINENO "x⏎y"` | 1 |
    /// | `echo \⏎$LINENO` | 2 |
    /// | `v=1 echo "a⏎b" $LINENO` | 1 |
    fn simple_command_line(&self) -> u32 {
        let leading_assignment = match self.toks.get(self.pos) {
            Some(Tok::ArrayAssign { .. }) => true,
            Some(Tok::Word(segs)) => word_is_assignment(segs),
            _ => false,
        };
        if leading_assignment {
            return self.cur_line();
        }
        self.lines
            .get(self.pos.saturating_add(1))
            .copied()
            .unwrap_or_else(|| self.cur_line())
    }

    fn parse_simple(&mut self) -> Result<Command, ParseError> {
        let mut cmd = SimpleCommand {
            line: self.simple_command_line(),
            ..SimpleCommand::default()
        };
        let mut seen_word = false;
        loop {
            match self.peek() {
                Some(Tok::Word(segs)) => {
                    // A reserved word ends the simple command (unless it's an
                    // argument position where reserved words are plain words —
                    // but at the start of a command a reserved word was already
                    // dispatched, so here we only stop for list terminators).
                    if !seen_word
                        && let [Seg::Lit(s)] = segs.as_slice()
                        && RESERVED.iter().any(|r| r.as_bytes() == s.as_slice())
                    {
                        break;
                    }
                    let segs = segs.clone();
                    // Assignment only valid before the first word.
                    if !seen_word
                        && let Some(a) = self.try_assignment(&segs)?
                    {
                        self.pos += 1;
                        cmd.assignments.push(a);
                        continue;
                    }
                    let at = self.pos;
                    self.pos += 1;
                    cmd.words.push(self.word_from_segs_at(&segs, at)?);
                    seen_word = true;
                }
                Some(Tok::ArrayAssign { .. }) => {
                    // After the command word, an array literal is only allowed as
                    // an operand of a declaration builtin (`declare -A m=([k]=v)`);
                    // anywhere else it's a syntax error.
                    //
                    // bash has no rule of its own to cite here: `echo n=(x y)` is
                    // an ordinary word followed by a `(` the grammar had no place
                    // for, so it is the *token* that gets named — and, as for
                    // every `near unexpected token`, the offending source line is
                    // echoed after it. Naming a rule instead would print a
                    // message bash never prints, so this reports the `(`, which
                    // is what our one token stands in for.
                    let is_decl_operand = seen_word && is_declaration_command(&cmd.words);
                    if seen_word && !is_decl_operand {
                        return Err(ParseError::new("syntax error near unexpected token `('"));
                    }
                    let Some(Tok::ArrayAssign {
                        name,
                        index,
                        append,
                        elems,
                    }) = self.bump()
                    else {
                        unreachable!("peek matched ArrayAssign");
                    };
                    let mut items = Vec::with_capacity(elems.len());
                    for segs in &elems {
                        items.push(parse_array_elem(segs, self.opts)?);
                    }
                    // A subscript is parsed verbatim, as the scalar form's is —
                    // see `Self::try_assignment` for why nothing in it is split
                    // or trimmed.
                    let index = match &index {
                        Some(src) => Some(word_verbatim_from_source(src, self.opts)?),
                        None => None,
                    };
                    let assign = Assignment {
                        name,
                        index,
                        append,
                        value: AssignRhs::Array(items),
                    };
                    if is_decl_operand {
                        // Record how many words preceded the operand: the builtin
                        // is handed only its name, but the operand's *position*
                        // among the words survives into `$BASH_COMMAND` and the
                        // `set -x` line (see `ast::DeclArray`).
                        cmd.decl_arrays.push(DeclArray {
                            assign,
                            word_index: cmd.words.len(),
                        });
                    } else {
                        cmd.assignments.push(assign);
                    }
                }
                Some(Tok::Io(_))
                | Some(Tok::VarFd(_))
                | Some(Tok::Op(
                    Op::Less
                    | Op::Great
                    | Op::GreatPipe
                    | Op::DGreat
                    | Op::GreatAnd
                    | Op::LessAnd
                    | Op::LessGreat
                    | Op::AmpGreat
                    | Op::AmpDGreat
                    | Op::DLess
                    | Op::DLessDash
                    | Op::TLess,
                )) => {
                    let r = self.parse_redirect()?;
                    cmd.redirects.push(r);
                }
                _ => break,
            }
        }
        if cmd.words.is_empty() && cmd.assignments.is_empty() && cmd.redirects.is_empty() {
            // A command position that reduced to nothing — e.g. the right side of
            // a dangling pipe (`echo a | | echo b`) or a stray operator. bash
            // names the offending token / reports end of input.
            return Err(self.unexpected_here());
        }
        Ok(Command::Simple(cmd))
    }

    fn parse_redirect(&mut self) -> Result<Redirect, ParseError> {
        // A varfd prefix `{name}` (`{fd}>file`) takes the place of a numeric fd:
        // the executor allocates a free fd ≥ 10 at runtime and binds `name` to it.
        let varfd = if let Some(Tok::VarFd(name)) = self.peek() {
            let name = name.clone();
            self.pos += 1;
            Some(name)
        } else {
            None
        };
        let explicit_fd = if let Some(Tok::Io(n)) = self.peek() {
            let n = *n;
            self.pos += 1;
            Some(n)
        } else {
            None
        };
        // `<<-` strips leading tabs; only the operator token records which
        // spelling was used, so capture it before the token is left behind.
        let mut here_strip = false;
        let op = match self.bump() {
            Some(Tok::Op(Op::Less)) => RedirectOp::Read,
            Some(Tok::Op(Op::Great)) => RedirectOp::Write,
            Some(Tok::Op(Op::GreatPipe)) => RedirectOp::Clobber,
            Some(Tok::Op(Op::DGreat)) => RedirectOp::Append,
            Some(Tok::Op(Op::GreatAnd)) => RedirectOp::DupOut,
            Some(Tok::Op(Op::LessAnd)) => RedirectOp::DupIn,
            Some(Tok::Op(Op::LessGreat)) => RedirectOp::ReadWrite,
            Some(Tok::Op(Op::AmpGreat)) => RedirectOp::WriteBoth,
            Some(Tok::Op(Op::AmpDGreat)) => RedirectOp::AppendBoth,
            Some(Tok::Op(op @ (Op::DLess | Op::DLessDash))) => {
                here_strip = op == Op::DLessDash;
                RedirectOp::HereDoc
            }
            Some(Tok::Op(Op::TLess)) => RedirectOp::HereStr,
            _ => return Err(ParseError::new("expected redirection operator")),
        };
        let fd = explicit_fd.unwrap_or(match op {
            RedirectOp::Read
            | RedirectOp::HereDoc
            | RedirectOp::HereStr
            | RedirectOp::DupIn
            | RedirectOp::ReadWrite => 0,
            _ => 1,
        });
        // Peek (don't `bump`) so that on a bad target the cursor still sits on
        // the offending token and `unexpected_here()` can name it — matching
        // bash's `syntax error near unexpected token \`>'` (etc.). A missing
        // target at end of input reports "unexpected end of file"; bash says
        // "newline" there because of its implicit-trailing-newline model, a
        // divergence noted in known-issues.md (TD-OILS-PARSE-ERR-LOC #4).
        let mut here = None;
        let target = match self.peek() {
            // The lexer emits the here-doc body as its own token right after the
            // `<<`/`<<-` operator, carrying the delimiter it consumed. (The
            // body's swallowed source lines are already accounted for by the
            // lexer's per-token line stamping.)
            Some(Tok::HereDoc(segs, delim, quoted)) => {
                let (segs, delim, quoted) = (segs.clone(), delim.clone(), *quoted);
                here = Some(HereDoc {
                    delim,
                    quoted,
                    strip: here_strip,
                });
                let at = self.pos;
                self.pos = self.pos.saturating_add(1);
                // An unquoted delimiter makes the body a double-quoted run, so a
                // substitution's operand in it is read with the quotes' rules.
                let q = if quoted { Quoting::Bare } else { Quoting::Dquote };
                word_from_segs_in(&segs, self.opts, q)
                    .map_err(|e| e.or_echo(self.echo_at(at)))?
            }
            Some(Tok::Word(segs)) => {
                let segs = segs.clone();
                let at = self.pos;
                self.pos = self.pos.saturating_add(1);
                self.word_from_segs_at(&segs, at)?
            }
            _ => return Err(self.unexpected_here()),
        };
        // `>&file` is deliberately *not* rewritten to `WriteBoth` here, even
        // though that is what it ends up meaning. bash keeps it as its own
        // instruction (`r_duplicating_output_word`) and converts it at
        // redirection time, in `do_redirection_internal`, once the word has been
        // expanded — which is the only point at which `>&$v` can be told apart
        // from `>&2` anyway. Deciding it here as well would work, but it would
        // erase how the redirect was *written*, and three things downstream need
        // that: printing it back (`declare -f` writes `>&out`, not `&> out`),
        // posix mode's redirection-word expansion (a dup word is still globbed,
        // a filename is not), and the fd accounting in `job_holds_sink`.
        Ok(Redirect {
            fd,
            op,
            target,
            varfd,
            here,
        })
    }

    fn expect_reserved(&mut self, w: &str) -> Result<(), ParseError> {
        if self.reserved_here() == Some(w) {
            self.pos += 1;
            Ok(())
        } else {
            // A missing closing keyword (`fi`/`done`/`then`/`esac`/`}`): bash
            // does not name the *expected* word — it reports the token actually
            // found (`syntax error near unexpected token \`done'`) or, at end of
            // input, `unexpected end of file`.
            Err(self.unexpected_here())
        }
    }

    /// Recognise `NAME=value`, `NAME+=value`, or `NAME[index]=value` (before the
    /// first command word).
    fn try_assignment(&self, segs: &[Seg]) -> Result<Option<Assignment>, ParseError> {
        let Some(Seg::Lit(first)) = segs.first() else {
            return Ok(None);
        };
        // Everything that decides the shape is anchored to the *name*: the word
        // is an assignment only if a `[` or an `=` sits immediately after it.
        // Scanning for the first `[` or `=` anywhere in the word instead gets
        // two shapes wrong — `foo=a[b` (an unclosed bracket in the value is
        // just text, not an unfinished subscript) and `a[x=3]=1` (a subscript
        // is arithmetic, so the `=` inside it is not the operator).
        let name_len = name_prefix_len(first);
        if name_len == 0 {
            return Ok(None);
        }
        let name = first.get(..name_len).unwrap_or_default();
        let lhs_tail = first.get(name_len..).unwrap_or_default();
        let (index, after_lhs) = if lhs_tail.first() == Some(&b'[') {
            match balanced_subscript_end(lhs_tail) {
                // A subscript containing expansions spans multiple segments,
                // e.g. `m[$k]=v` → [Lit("m["), Param("k"), Lit("]=v")]; the
                // first segment then has no closing `]` at all.
                None => return self.spanning_subscript_assignment(segs, first, name_len),
                Some(close) => {
                    // An *empty* subscript still makes the word an assignment.
                    // bash recognises `a[]=1` and then rejects it at run time
                    // ("a[]: bad array subscript"); refusing it here instead
                    // would demote the word to a command name and report
                    // "command not found".
                    let idx_src = lhs_tail.get(1..close).unwrap_or_default();
                    // A subscript is parsed verbatim (no word-splitting or
                    // trimming): for an associative array the expanded text —
                    // leading/trailing whitespace included — is the literal key
                    // (bash: `h[ x ]=v` keys on ` x `). For an indexed array the
                    // arithmetic evaluator ignores the whitespace, so preserving
                    // it is harmless.
                    let idx = word_verbatim_from_source(idx_src, self.opts)?;
                    (Some(idx), lhs_tail.get(close + 1..).unwrap_or_default())
                }
            }
        } else {
            (None, lhs_tail)
        };
        // `+=` append. Only these two spellings follow the left-hand side; a
        // word that runs on into anything else is not an assignment.
        let (append, after) = if let Some(rest) = after_lhs.strip_prefix(b"+=") {
            (true, rest)
        } else if let Some(rest) = after_lhs.strip_prefix(b"=") {
            (false, rest)
        } else {
            return Ok(None);
        };
        // Build the value word from the remainder of the first seg plus the
        // rest of the segments.
        let mut value_segs: Vec<Seg> = Vec::new();
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_vec()));
        }
        value_segs.extend_from_slice(segs.get(1..).unwrap_or_default());
        let Some(name) = name_text(name) else {
            return Ok(None);
        };
        Ok(Some(Assignment {
            name,
            index,
            append,
            value: AssignRhs::Scalar(self.word_from_segs_at(&value_segs, self.pos)?),
        }))
    }

    /// Lower lexer segments into an [`ast::Word`], blaming the token at `at` for
    /// whatever goes wrong inside it.
    ///
    /// The index is passed rather than read off [`Parser::pos`] because the two
    /// are not reliably related — some callers step past the word first and some
    /// do not — and getting it wrong is silent: it only shows in the source line
    /// echoed under a diagnostic.
    ///
    /// That echo is why the index is wanted at all. A `$( … )` body is parsed
    /// here, while the word holding it is lowered, so an error from it is found
    /// with bash's reader standing at the end of *that* word. Stamping the echo
    /// centrally from [`Parser::pos`] would place it at the end of the next one
    /// instead, which is a different answer whenever the word came out of an
    /// alias replacement: one more token's worth of reading can pop the
    /// replacement the error should have been echoed against. See
    /// [`Parser::echo_at`].
    fn word_from_segs_at(&self, segs: &[Seg], at: usize) -> Result<Word, ParseError> {
        word_from_segs(segs, self.opts).map_err(|e| e.or_echo(self.echo_at(at)))
    }

    /// Parse `name[SUBSCRIPT]=value` / `name[SUBSCRIPT]+=value` where the
    /// subscript spans multiple segments (contains `$…` expansions). `open` is
    /// the byte offset of `[` in the first (literal) segment.
    fn spanning_subscript_assignment(
        &self,
        segs: &[Seg],
        first: BStr<'_>,
        open: usize,
    ) -> Result<Option<Assignment>, ParseError> {
        let name = first.get(..open).unwrap_or_default();
        if name.is_empty() || !is_valid_name(name) {
            return Ok(None);
        }
        // Subscript segments: the first seg's text after `[`, then whole
        // segments, up to the segment that carries the closing `]`.
        let mut sub_segs: Vec<Seg> = Vec::new();
        let after_open = first.get(open + 1..).unwrap_or_default();
        if !after_open.is_empty() {
            sub_segs.push(Seg::Lit(after_open.to_vec()));
        }
        let mut value_segs: Vec<Seg> = Vec::new();
        let mut append = false;
        let mut found = false;
        for seg in segs.get(1..).unwrap_or_default() {
            if found {
                value_segs.push(seg.clone());
                continue;
            }
            if let Seg::Lit(s) = seg
                && let Some(close) = s.iter().position(|&b| b == b']')
            {
                let before = s.get(..close).unwrap_or_default();
                if !before.is_empty() {
                    sub_segs.push(Seg::Lit(before.to_vec()));
                }
                let rest = s.get(close + 1..).unwrap_or_default();
                let val_lit = if let Some(v) = rest.strip_prefix(b"+=") {
                    append = true;
                    v
                } else if let Some(v) = rest.strip_prefix(b"=") {
                    v
                } else {
                    // `]` not immediately followed by `=` — not an assignment.
                    return Ok(None);
                };
                if !val_lit.is_empty() {
                    value_segs.push(Seg::Lit(val_lit.to_vec()));
                }
                found = true;
                continue;
            }
            sub_segs.push(seg.clone());
        }
        if !found || sub_segs.is_empty() {
            return Ok(None);
        }
        let Some(name) = name_text(name) else {
            return Ok(None);
        };
        Ok(Some(Assignment {
            name,
            index: Some(self.word_from_segs_at(&sub_segs, self.pos)?),
            append,
            value: AssignRhs::Scalar(self.word_from_segs_at(&value_segs, self.pos)?),
        }))
    }
}

/// Where an array-literal element's subscript ends: the offset of the first `]`
/// that is immediately followed by `=` or `+=`, whether that `=` was the append
/// spelling, and the offset just past it where the value text starts.
///
/// Scanning for the *first* `]` that closes an assignment (rather than for the
/// literal two bytes `]=`) is what lets `[a]+=v` be keyed at all, and it keeps
/// `[a]x]=v` keying on `a]x` the way searching for `]=` did.
fn elem_subscript_close(s: BStr<'_>) -> Option<(usize, bool, usize)> {
    let mut from = 0usize;
    loop {
        let rel = s.get(from..)?.iter().position(|&b| b == b']')?;
        let at = from.checked_add(rel)?;
        let rest = s.get(at.checked_add(1)?..).unwrap_or_default();
        if rest.starts_with(b"+=") {
            return Some((at, true, at.checked_add(3)?));
        }
        if rest.starts_with(b"=") {
            return Some((at, false, at.checked_add(2)?));
        }
        from = at.checked_add(1)?;
    }
}

/// Parse one array-literal element: either `[sub]=value` / `[sub]+=value`
/// (keyed) or a bare positional value. A keyed element is recognised when the
/// first segment is a literal that starts with `[` and the subscript closes
/// with `]=` or `]+=` (so the subscript is literal text — an expanded key like
/// `[$k]=v` inside a literal is handled by the general branch below).
fn parse_array_elem(segs: &[Seg], opts: ParseOpts) -> Result<ArrayElem, ParseError> {
    if let Some(Seg::Lit(first)) = segs.first()
        && first.first() == Some(&b'[')
        && let Some((close, append, val_at)) = elem_subscript_close(first)
    {
        // Verbatim: an associative keyed element `[ x ]=v` keys on the literal
        // ` x ` (bash preserves subscript whitespace); indexed elements
        // arithmetic-evaluate, which ignores it.
        let index = word_verbatim_from_source(first.get(1..close).unwrap_or_default(), opts)?;
        let mut value_segs: Vec<Seg> = Vec::new();
        let after = first.get(val_at..).unwrap_or_default();
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_vec()));
        }
        value_segs.extend_from_slice(segs.get(1..).unwrap_or_default());
        return Ok(ArrayElem::Keyed {
            index,
            value: word_from_segs(&value_segs, opts)?,
            append,
        });
    }
    // General keyed element: the subscript spans quoted or expansion segments,
    // so the closing `]=` is not in the same literal as the opening `[`
    // (`["k v"]=1`, `['k']=1`, `[$x]=1`). The opening `[` is the start of the
    // first literal; everything up to the first unquoted `]=` (which lands in a
    // later literal segment) is the key — intervening quoted/expansion segments
    // belong to it and are copied verbatim.
    if let Some(Seg::Lit(first)) = segs.first()
        && first.first() == Some(&b'[')
        && elem_subscript_close(first).is_none()
    {
        let mut key_segs: Vec<Seg> = Vec::new();
        let head = first.get(1..).unwrap_or_default();
        if !head.is_empty() {
            key_segs.push(Seg::Lit(head.to_vec()));
        }
        for (i, seg) in segs.iter().enumerate().skip(1) {
            if let Seg::Lit(s) = seg
                && let Some((pos, append, val_at)) = elem_subscript_close(s)
            {
                let before = s.get(..pos).unwrap_or_default();
                if !before.is_empty() {
                    key_segs.push(Seg::Lit(before.to_vec()));
                }
                let index = word_from_segs(&key_segs, opts)?;
                let mut value_segs: Vec<Seg> = Vec::new();
                let after = s.get(val_at..).unwrap_or_default();
                if !after.is_empty() {
                    value_segs.push(Seg::Lit(after.to_vec()));
                }
                value_segs.extend_from_slice(segs.get(i + 1..).unwrap_or_default());
                return Ok(ArrayElem::Keyed {
                    index,
                    value: word_from_segs(&value_segs, opts)?,
                    append,
                });
            }
            key_segs.push(seg.clone());
        }
    }
    Ok(ArrayElem::Positional(word_from_segs(segs, opts)?))
}

/// True when the command word (`words[0]`) is a declaration/assignment builtin,
/// so that a following array literal (`declare -A m=([k]=v)`, `readonly a=(1 2)`)
/// is parsed as an operand rather than rejected. The word must be a single
/// unquoted literal. bash treats `declare`/`typeset`/`local`/`export`/`readonly`
/// as assignment builtins that accept `name=(…)` compound-array arguments.
fn is_declaration_command(words: &[Word]) -> bool {
    let Some(first) = words.first() else {
        return false;
    };
    let [WordPart::Literal(name)] = first.parts.as_slice() else {
        return false;
    };
    matches!(
        name.as_slice(),
        b"declare" | b"typeset" | b"local" | b"export" | b"readonly"
    )
}

/// Append a redirection to an already-parsed command. Simple commands carry
/// their own redirect list; a `Command::Redirected` extends its list; every
/// other (compound) form is wrapped. Used to lower the `|&` pipe operator's
/// implicit `2>&1` onto the left-hand command.
fn attach_redirect(cmd: Command, redir: Redirect) -> Command {
    match cmd {
        Command::Simple(mut sc) => {
            sc.redirects.push(redir);
            Command::Simple(sc)
        }
        Command::Redirected { inner, mut redirects } => {
            redirects.push(redir);
            Command::Redirected { inner, redirects }
        }
        other => Command::Redirected {
            inner: Box::new(other),
            redirects: vec![redir],
        },
    }
}

/// bash's `syntax error near \`…'` text for an operator found where a
/// conditional binary operator belonged.
///
/// bash does not name the token there. It re-scans the *source line* backwards
/// from the end of the offending token, stopping at any of `" \n\t;|&"`
/// (`error_token_from_text` in parse.y). For a whitespace-separated operator
/// that comes to the same thing as the token itself — unless the operator's own
/// last character is one of those delimiters, in which case the scan stops at
/// once and only that character is printed. So `;;` is reported near `;`, and
/// `;&`, `;;&`, `|&` and `>&` are all reported near `&`, while `>>` and `<<<`
/// (which contain no delimiter) are printed whole.
///
/// Parentheses are *not* delimiters, which is visible in `[[ -n @(a) ]]`: bash
/// reports that near `@(a`, having scanned back through the `(` to the space.
///
/// The scan is textual, so an operator written flush against the word before it
/// picks that word up too: `[[ a>>b ]]` is reported near `a>>b`. That cannot be
/// recovered from a token, which is why this is only the **fallback**: when the
/// stream has source behind it, [`Spans::near`] runs bash's scan for real and
/// this is never reached. It survives for the one stream that has no source of
/// its own — an alias-expanded parse, whose tokens were never written where
/// they are being read (TD-OILS-COND-ERROR-NEAR-IGNORES-THE-ALIAS-TEXT).
fn cond_error_near(tok: BStr<'_>) -> Str {
    match tok.last() {
        Some(&c @ (b';' | b'|' | b'&')) => vec![c],
        _ => tok.to_vec(),
    }
}

/// Whether the segments being lowered sit inside a double-quoted run.
///
/// It has to be carried down because a substitution's *operand* — the `w` of
/// `${x:-w}` — is read with the enclosing quoting still in force. Inside `"…"`
/// a `'` there is an ordinary character and only the characters double-quoting
/// leaves live can be backslash-escaped, so `"${x:-'a b'}"` keeps its quotes
/// where a bare `${x:-'a b'}` loses them. A here-document body with an unquoted
/// delimiter is the same context, and hands [`Quoting::Dquote`] down too.
///
/// Nothing else about the parse depends on it: the patterns, replacements and
/// subscripts beside the operand are read bare either way, because bash's
/// pattern reader does its own quote removal regardless of the quotes outside.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quoting {
    Bare,
    Dquote,
}

/// Lower lexer segments into an [`ast::Word`] (stateless).
fn word_from_segs(segs: &[Seg], opts: ParseOpts) -> Result<Word, ParseError> {
    word_from_segs_in(segs, opts, Quoting::Bare)
}

/// [`word_from_segs`] for segments already known to sit in a given quoting
/// context — the here-document body, whose unquoted delimiter makes its text a
/// double-quoted run without any quotes being written.
fn word_from_segs_in(segs: &[Seg], opts: ParseOpts, q: Quoting) -> Result<Word, ParseError> {
    let mut parts = Vec::with_capacity(segs.len());
    for s in segs {
        parts.push(seg_to_part(s, opts, q)?);
    }
    Ok(Word { parts })
}

fn seg_to_part(seg: &Seg, opts: ParseOpts, q: Quoting) -> Result<WordPart, ParseError> {
    Ok(match seg {
        Seg::Lit(s) => WordPart::Literal(s.clone()),
        Seg::Sq(s, escaped) => WordPart::SingleQuoted {
            text: s.clone(),
            escaped: *escaped,
        },
        Seg::Dq(inner) => {
            let mut parts = Vec::with_capacity(inner.len());
            for s in inner {
                parts.push(seg_to_part(s, opts, Quoting::Dquote)?);
            }
            WordPart::DoubleQuoted(parts)
        }
        Seg::Param(n) => WordPart::Param { name: n.clone(), braced: false },
        Seg::ParamBraced(raw) => parse_braced_param_in(raw, opts, q)?,
        // A backtick body is not parsed here at all: bash reads it only when the
        // word is expanded, as an input of its own. See [`CmdSubBody`].
        Seg::CmdSub(raw, close_line, src) => WordPart::CommandSub {
            body: match src {
                Some(verbatim) => CmdSubBody::Backtick {
                    src: raw.clone(),
                    verbatim: verbatim.clone(),
                    close_line: *close_line,
                },
                None => {
                    // The eager parse is kept — it is what found the `)` and
                    // what raises the fatal syntax error — but so is the body
                    // text, because bash re-reads it at expansion time.
                    let (prog, map) = parse_cmdsub_body(raw, *close_line, opts)?;
                    CmdSubBody::Parsed { prog, src: raw.clone(), map }
                }
            },
        },
        Seg::Arith(raw, bracket) => WordPart::ArithSub {
            expr: raw.clone(),
            bracket: *bracket,
        },
        Seg::ProcSub(input, raw, open_line) => WordPart::ProcSub {
            input: *input,
            body: parse_procsub_body(raw, *open_line, opts)?,
        },
    })
}

/// Parse the inner text of a `${ … }` expansion.
/// Split a `${…}` body into `(name, optional-subscript, remaining-chars)`.
///
/// The name is a run of name chars, a run of digits, or a single special
/// character. If a `[…]` subscript immediately follows the name, it is parsed
/// into an [`ArrayIndex`] and the characters after the closing `]` are returned
/// as the remainder (for operator forms). The closing bracket is taken as the
/// last `]` in the body so arithmetic subscripts like `arr[i+1]` still parse.
/// Given `bytes[open] == '['`, return the index of the `]` that closes it,
/// balancing nested `[`/`]` (arithmetic subscripts like `a[b[0]]`). This is
/// deliberately *not* "the last `]` in the body": characters after the
/// subscript can contain their own `]` — e.g. a slice offset with a nested
/// parameter expansion `${a[@]:${#a[@]}-2}`, where the `]` inside `${#a[@]}`
/// must not be mistaken for the subscript's close. Brackets inside any valid
/// nested `${…}`/`$(…)` are themselves balanced, so plain depth counting over
/// `[`/`]` handles those correctly too.
fn matching_subscript_close(chs: &[Ch], open: usize) -> Option<usize> {
    // Scan for the `]` that closes the subscript opened at `open`, tracking
    // `[`/`]` nesting and *skipping quoted spans* so a quoted `]` inside an
    // associative key (`${h["a]b"]}`, `${h['a]b']}`) is not mistaken for the
    // terminator — matching bash's subscript scanner, which does not treat a
    // quoted `]` as the close. Without this the subscript would split mid-quote
    // (`"a` for `"a]b"`), leaving an unbalanced quote that trips the re-lexer
    // (`unexpected EOF while looking for matching '"'`). See known-issues
    // TD-OILS-SUBSCRIPT-QUOTED-BRACKET.
    let mut depth = 0usize;
    let mut i = open;
    while i < chs.len() {
        match syn_at(chs, i) {
            // A backslash escapes the next character (skip both).
            '\\' => i += 1,
            // Single-quoted run: verbatim to the closing quote (no escapes).
            '\'' => {
                i += 1;
                while i < chs.len() && syn_at(chs, i) != '\'' {
                    i += 1;
                }
            }
            // Double-quoted run: to the closing quote, honoring `\`.
            '"' => {
                i += 1;
                while i < chs.len() && syn_at(chs, i) != '"' {
                    if syn_at(chs, i) == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The verdict of [`split_name_subscript`].
enum NameSubscript {
    /// The body split cleanly into a name, an optional `[subscript]`, and
    /// whatever text followed the subscript.
    Split(String, Option<ArrayIndex>, Vec<Ch>),
    /// The body is well-formed enough to *parse* but cannot be interpreted, so
    /// bash defers the complaint to expansion time. The only such case here is
    /// an empty subscript (`${a[]}`), which bash parses happily — a guarded
    /// `if false; then echo "${a[]}"; fi` is silent — and only rejects when the
    /// word is expanded, as a runtime "bad substitution". Every caller turns
    /// this into [`WordPart::BadSubst`]. Note the check is purely lexical:
    /// `${a[  ]}` is *not* empty, and arithmetic-evaluates its blanks to index 0.
    ///
    /// A parameter *name* that is not text takes the same route: the variable
    /// namespace is ASCII, so `${\xff}` names nothing and bash's answer is the
    /// same run-time "bad substitution".
    Deferred,
}

fn split_name_subscript(chs: &[Ch], opts: ParseOpts) -> Result<NameSubscript, ParseError> {
    if chs.is_empty() {
        return Err(ParseError::new("empty '${}' expansion"));
    }
    let mut i = 0;
    // Only a genuine identifier may carry a `[subscript]`. bash refuses
    // `${@[0]}`, `${1[0]}` and `${#[0]}` outright, so for those the `[` is not
    // part of the parameter at all: it stays in the remainder, where the
    // operator dispatch finds no operator it opens and reports the refusal.
    let mut identifier = false;
    if syn_at(chs, 0).is_ascii_digit() {
        while i < chs.len() && syn_at(chs, i).is_ascii_digit() {
            i += 1;
        }
    } else if is_name_start(syn_at(chs, 0)) {
        identifier = true;
        while i < chs.len() && is_name_char(syn_at(chs, i)) {
            i += 1;
        }
    } else {
        // A special single-char parameter (`@`, `*`, `?`, `#`, `!`, `$`, …).
        i = 1;
    }
    let raw_name = bytes::from_chars(chs.get(..i).unwrap_or_default().iter().copied());
    // Only the special-parameter branch above can pick up a non-ASCII character,
    // and no such parameter exists — `${\xff}` names nothing, which bash reports
    // when the word is expanded.
    let Some(name) = name_text(&raw_name) else {
        return Ok(NameSubscript::Deferred);
    };
    if identifier
        && syn_at(chs, i) == '['
        && let Some(close) = matching_subscript_close(chs, i)
    {
        let inner =
            bytes::from_chars(chs.get(i + 1..close).unwrap_or_default().iter().copied());
        let index = match inner.as_slice() {
            b"@" => ArrayIndex::All,
            b"*" => ArrayIndex::Star,
            b"" => return Ok(NameSubscript::Deferred),
            // Verbatim so an associative read `${h[ x ]}` keys on the literal
            // ` x ` (bash preserves subscript whitespace); indexed reads
            // arithmetic-evaluate, which ignores the whitespace.
            _ => ArrayIndex::Index(Box::new(word_verbatim_from_source(&inner, opts)?)),
        };
        return Ok(NameSubscript::Split(
            name,
            Some(index),
            chs.get(close + 1..).unwrap_or_default().to_vec(),
        ));
    }
    Ok(NameSubscript::Split(name, None, chs.get(i..).unwrap_or_default().to_vec()))
}

/// Parse the `offset[:length]` portion of a substring/slice expansion (the
/// text after the leading `:`). The offset and each length are parsed as
/// arithmetic words. Splits on the *first* unescaped `:` only.
fn parse_slice_bounds(
    rest: &[Ch],
    opts: ParseOpts,
) -> Result<(Box<Word>, Option<Box<Word>>), ParseError> {
    let (off, len) = match rest.iter().position(|&c| syn(c) == ':') {
        Some(idx) => (
            rest.get(..idx).unwrap_or_default(),
            Some(rest.get(idx + 1..).unwrap_or_default()),
        ),
        None => (rest, None),
    };
    let length = match len {
        Some(s) => {
            let text = bytes::from_chars(s.iter().copied());
            Some(Box::new(word_from_source(&text, opts)?))
        }
        None => None,
    };
    let off_text = bytes::from_chars(off.iter().copied());
    Ok((Box::new(word_from_source(&off_text, opts)?), length))
}

/// Is `name` a parameter that `${#…}` may take the length of?
///
/// An identifier, a positional number, or one of the special parameters that
/// can stand alone after the `#`. Notably absent are the operator characters
/// (`^`, `,`, `:`, `/`, …): `${#^}` is not a length, because `^` names no
/// parameter.
fn is_length_target(name: &str) -> bool {
    matches!(name, "@" | "*" | "#" | "?" | "-" | "$" | "!")
        || is_valid_name(name.as_bytes())
        || (!name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn parse_braced_param(raw: BStr<'_>, opts: ParseOpts) -> Result<WordPart, ParseError> {
    parse_braced_param_in(raw, opts, Quoting::Bare)
}

/// [`parse_braced_param`] with the quoting the `${…}` was written in, which
/// only its operand cares about. See [`Quoting`].
fn parse_braced_param_in(
    raw: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
) -> Result<WordPart, ParseError> {
    if let Some(after_hash) = raw.strip_prefix(b"#") {
        if after_hash.is_empty() {
            // `${#}` is the positional-parameter count — treat as `$#`.
            return Ok(WordPart::Param { name: "#".into(), braced: true });
        }
        // The length operator wants a *complete* parameter reference after the
        // `#` — nothing may be left over. When what follows is anything less,
        // bash reads the `#` itself as the parameter and the remainder as an
        // operator on it: `${#+x}` is `$#` with `+x`, and `${##a}` strips a
        // leading `a` from `$#`. Hence the fall-through to the general path
        // below rather than an immediate refusal.
        let chs: Vec<Ch> = bytes::chars(after_hash).collect();
        if let NameSubscript::Split(name, subscript, remaining) = split_name_subscript(&chs, opts)?
            && remaining.is_empty()
            && is_length_target(&name)
        {
            return Ok(match subscript {
                // `${#name[@]}` / `${#name[i]}` — element count / element length.
                Some(index) => WordPart::ArrayRef {
                    name,
                    index,
                    length: true,
                },
                None => WordPart::Length(name),
            });
        }
    }
    // Indirection needs something that could name a parameter right after the
    // `!`. Where there is none — `${!}`, `${!-x}`, `${!:0:1}` — bash reads the
    // `!` itself as the parameter (`$!`, the last background job's pid) and the
    // rest as an operator on it, so those fall through to the general path
    // below instead of being refused here.
    if let Some(after_bang) = raw.strip_prefix(b"!")
        && let Some(first) = bytes::chars(after_bang).next()
        && is_indirection_starter(syn(first))
    {
        // `${!prefix*}` / `${!prefix@}` — names of set variables beginning with
        // `prefix`. Distinguished from the array-keys form (`${!a[@]}`) by
        // ending in a bare `*`/`@` (no closing `]`). A valid name prefix is
        // required so we don't mistake other expansions.
        // A *non-empty* prefix is required for the name-listing form: a bare
        // `${!*}`/`${!@}` is instead indirect expansion through the positional
        // list (`$*`/`$@`), handled below, not a listing of every variable.
        if let Some(prefix) = after_bang.strip_suffix(b"*")
            && !prefix.is_empty()
            && !prefix.contains(&b'[')
            && is_valid_name(prefix)
            && let Some(prefix) = name_text(prefix)
        {
            return Ok(WordPart::VarNames { prefix, star: true });
        }
        if let Some(prefix) = after_bang.strip_suffix(b"@")
            && !prefix.is_empty()
            && !prefix.contains(&b'[')
            && is_valid_name(prefix)
            && let Some(prefix) = name_text(prefix)
        {
            return Ok(WordPart::VarNames { prefix, star: false });
        }
        // `${!name[@]}` / `${!name[*]}` — the keys/indices of an array.
        let chs: Vec<Ch> = bytes::chars(after_bang).collect();
        let NameSubscript::Split(name, subscript, remaining) =
            split_name_subscript(&chs, opts)?
        else {
            return Ok(WordPart::BadSubst(raw.to_vec()));
        };
        if let Some(index) = &subscript
            && remaining.is_empty()
            && matches!(index, ArrayIndex::All | ArrayIndex::Star)
        {
            return Ok(WordPart::ArrayKeys {
                star: matches!(index, ArrayIndex::Star),
                name,
            });
        }
        // `${!name}` — indirect expansion. The referent (`name`) may be a plain
        // identifier, a positional parameter (`${!1}`), or a special parameter
        // (`${!#}`, `${!$}`, …). Its *value* is then used as the parameter name
        // to expand. The named target may itself carry a subscript.
        //
        // The *pointer* may carry one too (`${!a[0]}`): what is read is then the
        // element's value, and from there on the expansion is the same. Only a
        // plain name can carry one, no positional or special parameter being an
        // array.
        //
        // `[@]`/`[*]` point as well, and reaching here with one means an
        // operator followed — the bare spelling was claimed by the key listing
        // just above. That is bash's rule and it is easy to misread as a typo:
        // `${!a[@]}` is the keys, but `${!a[@]#x}` reads the *elements* as the
        // target's name, so `one=(v); v=hello; ${!one[@]#h}` is `ello` and
        // `n=(x y z); ${!n[@]#x}` complains that `x y z` is not a variable name.
        let index: Option<ArrayIndex> = match subscript {
            None => None,
            Some(_) if !is_valid_name(name.as_bytes()) => {
                return Ok(WordPart::BadSubst(raw.to_vec()));
            }
            Some(i) => Some(i),
        };
        if is_indirect_referent(&name) {
            if remaining.is_empty() {
                return Ok(WordPart::Indirect {
                    refname: name,
                    index,
                });
            }
            // `${!ref<op>}` — indirect expansion combined with a modifier
            // (`${!ref:-def}`, `${!ref^^}`, `${!ref#pat}`, `${!ref/a/b}`, …).
            // Parse the modifier as if it were written against `ref` directly;
            // the placeholder name is rewritten to the resolved target at
            // expansion time. Any referent may carry one — `${!#:-z}` and
            // `${!1^}` are as good as `${!name^}` — so re-parsing against the
            // referent is also what draws the line: it is a modifier only if
            // what comes back is one, which is how `${!@Q}` (a `Q` that opens
            // no operator on `$@`) and `${!#1}` (a length, not a modifier) end
            // up refused.
            //
            // `@`/`*` are the exception, because the indirection collapses the
            // whole positional list to one name before the modifier ever runs:
            // `${!@:0:1}` takes a *substring* of the value, where `${@:0:1}`
            // would slice the list. So those parse against a stand-in name and
            // have the referent put back afterwards. Only case modification
            // does not survive the round trip — bash refuses `${!@^^}` while
            // accepting `${!*^^}`, and that asymmetry is bash's own.
            let positional = matches!(name.as_str(), "@" | "*");
            let mut modifier_src = if positional {
                PLACEHOLDER_REFERENT.to_vec()
            } else {
                name.clone().into_bytes()
            };
            modifier_src.extend(bytes::from_chars(remaining.iter().copied()));
            let mut target = parse_braced_param_in(&modifier_src, opts, q)?;
            if positional {
                if name == "@" && matches!(target, WordPart::ParamCase { .. }) {
                    return Ok(WordPart::BadSubst(raw.to_vec()));
                }
                target.set_param_name(name.clone());
            }
            if matches!(
                target,
                WordPart::ParamOp { .. }
                    | WordPart::ParamTrim { .. }
                    | WordPart::ParamSubstr { .. }
                    | WordPart::ParamReplace { .. }
                    | WordPart::ParamCase { .. }
                    | WordPart::ParamTransform { .. }
            ) {
                return Ok(WordPart::IndirectOp {
                    refname: name,
                    index,
                    target: Box::new(target),
                });
            }
        }
        // bash accepts this at parse time and rejects it only during expansion
        // as a runtime "bad substitution" (DISCARD-class).
        return Ok(WordPart::BadSubst(raw.to_vec()));
    }
    let chs: Vec<Ch> = bytes::chars(raw).collect();
    let NameSubscript::Split(name, subscript, rest) = split_name_subscript(&chs, opts)? else {
        return Ok(WordPart::BadSubst(raw.to_vec()));
    };
    // `$#`, `$?` and `$-` are the three specials that `${#…}` also spells as a
    // length, and bash lets only an operator follow one — anything else is a bad
    // substitution, so `${#^}` and `${?[0]}` are refused where `${@^}` and
    // `${v^}` are fine. The stop-set is exactly the characters that open an
    // operator; a subscript cannot appear here, since none of the three is an
    // identifier.
    if matches!(name.as_str(), "#" | "?" | "-")
        && !rest.is_empty()
        && !matches!(syn_at(&rest, 0), '#' | '%' | ':' | '-' | '=' | '?' | '+' | '/' | '@')
    {
        return Ok(WordPart::BadSubst(raw.to_vec()));
    }
    // A subscript may be combined with an operator: `${a[i]:-def}`, `${a[i]#pat}`,
    // etc. Only a specific `[expr]` index is allowed with an operator — `[@]`/`[*]`
    // + operator (bulk transform) is not supported.
    let elem_index: Option<Box<Word>> = match subscript {
        None => None,
        Some(ArrayIndex::Index(w)) => {
            if rest.is_empty() {
                return Ok(WordPart::ArrayRef {
                    name,
                    index: ArrayIndex::Index(w),
                    length: false,
                });
            }
            Some(w)
        }
        Some(index @ (ArrayIndex::All | ArrayIndex::Star)) => {
            if rest.is_empty() {
                return Ok(WordPart::ArrayRef {
                    name,
                    index,
                    length: false,
                });
            }
            // `${a[@]:off:len}` / `${a[*]:off:len}` — array slice (a `:` not
            // followed by a `-=+?` operator char).
            if syn_at(&rest, 0) == ':' && !matches!(syn_at(&rest, 1), '-' | '=' | '+' | '?') {
                let (offset, length) =
                    parse_slice_bounds(rest.get(1..).unwrap_or_default(), opts)?;
                return Ok(WordPart::ArraySlice {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    offset,
                    length,
                });
            }
            // `${a[@]#pat}` / `${a[*]/x/y}` / `${a[@]^^}` / `${a[@]@Q}` — an
            // element-wise transform applied to every element.
            if let Some(op) = parse_bulk_op(&rest, opts)? {
                return Ok(WordPart::ArrayBulk {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    op,
                });
            }
            // `${a[@]@Z}` / `${a[@]@}` — an empty/unknown/multi-char `@`
            // transform operator: empty for an empty array (or unset), but a
            // "bad substitution" when the array has ≥1 element.
            if syn_at(&rest, 0) == '@' {
                return Ok(WordPart::ArrayBulk {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    op: BulkOp::BadTransform { raw: raw.to_vec() },
                });
            }
            // `${a[@]:-x}` / `${a[*]:+x}` / `${a[@]:?msg}` — use/alternate/error
            // operators on a whole-array reference. Bash treats `[@]`/`[*]` like
            // `$@`: substitute the elements when active, else the operand word.
            let star = matches!(index, ArrayIndex::Star);
            let mut it = rest.iter().copied();
            let mut c = it.next().map_or('\0', syn);
            let colon = c == ':';
            if colon {
                c = it.next().map_or('\0', syn);
            }
            let arg_str = bytes::from_chars(it);
            let op = match c {
                '-' => ParamOp::UseDefault,
                '=' => ParamOp::AssignDefault,
                '+' => ParamOp::UseAlternate,
                '?' => ParamOp::ErrorIfUnset,
                _ => {
                    // bash accepts this at parse time and rejects it only during
                    // expansion as a runtime "bad substitution" (DISCARD-class).
                    return Ok(WordPart::BadSubst(raw.to_vec()));
                }
            };
            return Ok(WordPart::ArrayOp {
                name,
                star,
                op,
                colon,
                arg: Box::new(operand_from_source(&arg_str, opts, q)?),
            });
        }
    };
    // `${@:off:len}` / `${*:off:len}` — positional-parameter slice (same `:`
    // rule as the array form; distinguished from string substring because the
    // parameter names the whole positional list).
    if (name == "@" || name == "*")
        && syn_at(&rest, 0) == ':'
        && !matches!(syn_at(&rest, 1), '-' | '=' | '+' | '?')
    {
        let (offset, length) = parse_slice_bounds(rest.get(1..).unwrap_or_default(), opts)?;
        return Ok(WordPart::ArraySlice {
            name: name.clone(),
            star: name == "*",
            offset,
            length,
        });
    }
    // `${@#pat}` / `${*/x/y}` / `${@^^}` — element-wise transform over the
    // positional parameters.
    if (name == "@" || name == "*")
        && !rest.is_empty()
        && let Some(op) = parse_bulk_op(&rest, opts)?
    {
        return Ok(WordPart::ArrayBulk {
            name: name.clone(),
            star: name == "*",
            op,
        });
    }
    // `${@@Z}` / `${@@}` — invalid `@` transform over the positionals: empty
    // when there are no positional parameters, "bad substitution" otherwise.
    if (name == "@" || name == "*") && syn_at(&rest, 0) == '@' {
        return Ok(WordPart::ArrayBulk {
            name: name.clone(),
            star: name == "*",
            op: BulkOp::BadTransform { raw: raw.to_vec() },
        });
    }
    // `${@:-w}` / `${*:+w}` / `${@?msg}` — use/alternate/error on the
    // positionals. bash answers these with the positional *list* (`set -- p q r;
    // ${@:-d}` is three fields, not one), exactly as it answers `${a[@]:-w}`
    // with the array's elements, so the two are one node: `$@` and `a[@]` are
    // the same case in bash's expander, and the differences that remain — no
    // variable to assign a default to, and a complaint spelled `@` rather than
    // `a[@]` — belong to the runtime, not to the shape of the word.
    if (name == "@" || name == "*") && !rest.is_empty() {
        let mut it = rest.iter().copied();
        let mut c = it.next().map_or('\0', syn);
        let colon = c == ':';
        if colon {
            c = it.next().map_or('\0', syn);
        }
        // Anything else is one of the forms handled below (or, after them, a
        // "bad substitution"), so this recognises rather than rejects.
        let op = match c {
            '-' => Some(ParamOp::UseDefault),
            '=' => Some(ParamOp::AssignDefault),
            '+' => Some(ParamOp::UseAlternate),
            '?' => Some(ParamOp::ErrorIfUnset),
            _ => None,
        };
        if let Some(op) = op {
            let arg_str = bytes::from_chars(it);
            return Ok(WordPart::ArrayOp {
                star: name == "*",
                name,
                op,
                colon,
                arg: Box::new(operand_from_source(&arg_str, opts, q)?),
            });
        }
    }
    if rest.is_empty() {
        return Ok(WordPart::Param { name, braced: true });
    }
    match syn_at(&rest, 0) {
        // Prefix / suffix trimming: `#`, `##`, `%`, `%%`.
        '#' | '%' => {
            let suffix = syn_at(&rest, 0) == '%';
            let longest = syn_at(&rest, 1) == syn_at(&rest, 0);
            let pat_start = if longest { 2 } else { 1 };
            let pat = bytes::from_chars(rest.get(pat_start..).unwrap_or_default().iter().copied());
            Ok(WordPart::ParamTrim {
                name,
                index: elem_index,
                suffix,
                longest,
                pattern: Box::new(word_verbatim_from_source(&pat, opts)?),
            })
        }
        // Case modification: `^`/`^^` (upper), `,`/`,,` (lower), `~`/`~~` (toggle).
        '^' | ',' | '~' => {
            let mode = match syn_at(&rest, 0) {
                '^' => crate::ast::CaseMode::Upper,
                ',' => crate::ast::CaseMode::Lower,
                _ => crate::ast::CaseMode::Toggle,
            };
            let all = syn_at(&rest, 1) == syn_at(&rest, 0);
            let pat_start = if all { 2 } else { 1 };
            let pat = bytes::from_chars(rest.get(pat_start..).unwrap_or_default().iter().copied());
            Ok(WordPart::ParamCase {
                name,
                index: elem_index,
                mode,
                all,
                pattern: Box::new(word_verbatim_from_source(&pat, opts)?),
            })
        }
        // Parameter transformation: `${name@Q}`, `${name@U}`, etc.
        '@' => {
            // An empty (`${x@}`), unknown (`${x@Z}`), or multi-char
            // (`${x@QU}`) operator is *not* a parse-time error in bash: it is
            // deferred to expansion, where it yields empty for an unset
            // parameter but a "bad substitution" for a set one. `BadTransform`
            // carries the raw source so the runtime can reproduce that split.
            if rest.len() == 2 && is_valid_transform_op(syn_at(&rest, 1)) {
                return Ok(WordPart::ParamTransform {
                    name,
                    index: elem_index,
                    op: syn_at(&rest, 1),
                });
            }
            Ok(WordPart::BadTransform {
                name,
                index: elem_index,
                raw: raw.to_vec(),
            })
        }
        // Pattern substitution: `/pat/repl`, `//pat/repl`, `/#…`, `/%…`.
        '/' => parse_param_replace(name, elem_index, rest.get(1..).unwrap_or_default(), opts),
        // Substring `:offset[:length]` — but `:` followed by one of -=+? is the
        // use/assign/alt/error operator, handled below.
        ':' if !matches!(syn_at(&rest, 1), '-' | '=' | '+' | '?') => {
            let (offset, length) = parse_slice_bounds(rest.get(1..).unwrap_or_default(), opts)?;
            Ok(WordPart::ParamSubstr {
                name,
                index: elem_index,
                offset,
                length,
            })
        }
        // `:-`, `:=`, `:+`, `:?` and the colon-less `-=+?` forms.
        _ => {
            let mut it = rest.iter().copied();
            let mut c = it.next().map_or('\0', syn);
            // A leading `:` selects the null-or-unset (colon) form; without it the
            // operator acts only when the parameter is genuinely unset.
            let colon = c == ':';
            if colon {
                c = it.next().map_or('\0', syn);
            }
            let arg_str = bytes::from_chars(it);
            let op = match c {
                '-' => ParamOp::UseDefault,
                '=' => ParamOp::AssignDefault,
                '+' => ParamOp::UseAlternate,
                '?' => ParamOp::ErrorIfUnset,
                _ => {
                    // bash accepts this at parse time and rejects it only during
                    // expansion as a runtime "bad substitution" (DISCARD-class).
                    return Ok(WordPart::BadSubst(raw.to_vec()));
                }
            };
            Ok(WordPart::ParamOp {
                name,
                index: elem_index,
                op,
                colon,
                arg: Box::new(operand_from_source(&arg_str, opts, q)?),
                // Written directly, the name read and the name complained about
                // are the same one; only indirection separates them.
                label: None,
            })
        }
    }
}

/// Parse the body of a `${name/…}` substitution (chars after the first `/`).
/// Parse the `[/|#|%]pat/repl` body of a substitution into its component pieces
/// (`all`, anchor, pattern, replacement), shared by the scalar and bulk-array
/// substitution parsers.
#[allow(clippy::type_complexity)]
fn parse_replace_pieces(
    body: &[Ch],
    opts: ParseOpts,
) -> Result<(bool, ReplaceAnchor, Box<Word>, Box<Word>), ParseError> {
    let mut i = 0;
    let mut all = false;
    let mut anchor = ReplaceAnchor::None;
    match syn_at(body, 0) {
        '/' => {
            all = true;
            i = 1;
        }
        '#' => {
            anchor = ReplaceAnchor::Start;
            i = 1;
        }
        '%' => {
            anchor = ReplaceAnchor::End;
            i = 1;
        }
        _ => {}
    }
    // Pattern runs to the next unescaped '/'; the remainder is the replacement.
    let mut pattern = Str::new();
    let mut replacement = Str::new();
    let mut in_repl = false;
    while let Some(&c) = body.get(i) {
        if !in_repl && syn(c) == '\\' && syn_at(body, i + 1) == '/' {
            pattern.push(b'/');
            i += 2;
            continue;
        }
        if !in_repl && syn(c) == '/' {
            in_repl = true;
            i += 1;
            continue;
        }
        if in_repl {
            c.push_to(&mut replacement);
        } else {
            c.push_to(&mut pattern);
        }
        i += 1;
    }
    Ok((
        all,
        anchor,
        Box::new(word_verbatim_from_source(&pattern, opts)?),
        Box::new(word_replacement_from_source(&replacement, opts)?),
    ))
}

fn parse_param_replace(
    name: String,
    index: Option<Box<Word>>,
    body: &[Ch],
    opts: ParseOpts,
) -> Result<WordPart, ParseError> {
    let (all, anchor, pattern, replacement) = parse_replace_pieces(body, opts)?;
    Ok(WordPart::ParamReplace {
        name,
        index,
        all,
        anchor,
        pattern,
        replacement,
    })
}

/// Parse the operator portion of a bulk array expansion (`${a[@]OP}`) into a
/// [`BulkOp`], or `None` when `rest` is not a recognized element-wise operator
/// (e.g. the `:-`/`:=` default operators, which do not apply to `[@]`).
fn parse_bulk_op(rest: &[Ch], opts: ParseOpts) -> Result<Option<BulkOp>, ParseError> {
    if rest.is_empty() {
        return Ok(None);
    }
    match syn_at(rest, 0) {
        '#' | '%' => {
            let suffix = syn_at(rest, 0) == '%';
            let longest = syn_at(rest, 1) == syn_at(rest, 0);
            let pat_start = if longest { 2 } else { 1 };
            let pat = bytes::from_chars(rest.get(pat_start..).unwrap_or_default().iter().copied());
            Ok(Some(BulkOp::Trim {
                suffix,
                longest,
                pattern: Box::new(word_verbatim_from_source(&pat, opts)?),
            }))
        }
        '^' | ',' | '~' => {
            let mode = match syn_at(rest, 0) {
                '^' => crate::ast::CaseMode::Upper,
                ',' => crate::ast::CaseMode::Lower,
                _ => crate::ast::CaseMode::Toggle,
            };
            let all = syn_at(rest, 1) == syn_at(rest, 0);
            let pat_start = if all { 2 } else { 1 };
            let pat = bytes::from_chars(rest.get(pat_start..).unwrap_or_default().iter().copied());
            Ok(Some(BulkOp::Case {
                mode,
                all,
                pattern: Box::new(word_verbatim_from_source(&pat, opts)?),
            }))
        }
        '/' => {
            let (all, anchor, pattern, replacement) =
                parse_replace_pieces(rest.get(1..).unwrap_or_default(), opts)?;
            Ok(Some(BulkOp::Replace {
                all,
                anchor,
                pattern,
                replacement,
            }))
        }
        // Only a valid single-char operator is a real per-element transform.
        // An empty (`@`), unknown (`@Z`), or multi-char (`@QU`) operator is
        // left to the caller, which builds a `BulkOp::BadTransform` (bash
        // defers the empty/invalid case to expansion time — see below).
        '@' if rest.len() == 2 && is_valid_transform_op(syn_at(rest, 1)) => {
            Ok(Some(BulkOp::Transform { op: syn_at(rest, 1) }))
        }
        _ => Ok(None),
    }
}

/// The single-character operators accepted by a `${var@OP}` transform. Any
/// other operator (or an empty/multi-char one) is an *invalid* transform: bash
/// treats it as empty for an unset parameter but a "bad substitution" for a set
/// one (see [`WordPart::BadTransform`] / [`BulkOp::BadTransform`]).
fn is_valid_transform_op(op: char) -> bool {
    // Bash's set (5.2): Q E P A a K k U u L. Note there is no lowercase-first
    // `@l` — `${x@l}` is a "bad substitution", so `l` is deliberately absent.
    matches!(op, 'Q' | 'U' | 'u' | 'L' | 'E' | 'K' | 'k' | 'A' | 'a' | 'P')
}

/// Build a single [`Word`] from arbitrary source text (used for the argument of
/// a parameter expansion). Words separated by blanks are joined with a literal
/// space — a best-effort reconstruction adequate for `${x:-a b}`.
/// Parse `s` as a single word preserving literal whitespace (no word-splitting
/// or operator tokenization) — for the pattern and replacement of
/// `${var/pat/repl}`, where bash applies only expansion and quote removal.
pub(crate) fn word_verbatim_from_source(s: BStr<'_>, opts: ParseOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_word_verbatim(s).map_err(|e| ParseError::new(&e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts, Quoting::Bare)?);
    }
    Ok(Word { parts })
}

/// Parse the *operand* of a substitution — the `w` of `${x:-w}`, `${x:=w}`,
/// `${x:+w}` and `${x:?w}`. Written bare it is a verbatim word like a pattern
/// is; written inside `"…"` it is read with double-quote rules instead, because
/// the quotes around the substitution never stopped applying. See [`Quoting`]
/// and [`crate::lexer::lex_operand_in_dquote`].
fn operand_from_source(s: BStr<'_>, opts: ParseOpts, q: Quoting) -> Result<Word, ParseError> {
    if q == Quoting::Bare {
        return word_verbatim_from_source(s, opts);
    }
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_operand_in_dquote(s).map_err(|e| ParseError::new(&e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        // Still inside the quotes: a substitution nested in the operand is read
        // the same way its host was.
        parts.push(seg_to_part(seg, opts, Quoting::Dquote)?);
    }
    Ok(Word { parts })
}

/// Parse `s` as the body of an unterminated double-quoted string, giving a
/// [`Word`] whose expansion is what bash produces for a string it expands in
/// `Q_DOUBLE_QUOTES` context — `PS4` and `${x@P}`. Every part comes back
/// already quoted, so the result is never split or globbed.
///
/// # Errors
/// Returns [`ParseError`] on an unterminated substitution inside `s`.
pub(crate) fn dquote_word_from_source(s: BStr<'_>, opts: ParseOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_dquote_body(s).map_err(|e| ParseError::new(&e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        // The whole point of this entry is that `s` is a double-quoted run, so
        // an operand inside it is read as one too.
        parts.push(seg_to_part(seg, opts, Quoting::Dquote)?);
    }
    Ok(Word { parts })
}

/// Like [`word_verbatim_from_source`] but for the *replacement* half of
/// `${var/pat/repl}`: a literal `\&`/`\\` is preserved (not consumed at lex
/// time) so the runtime `&`-substitution can distinguish an escaped ampersand
/// from an active one. See [`crate::lexer::lex_replacement_verbatim`].
fn word_replacement_from_source(s: BStr<'_>, opts: ParseOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_replacement_verbatim(s).map_err(|e| ParseError::new(&e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts, Quoting::Bare)?);
    }
    Ok(Word { parts })
}

fn word_from_source(s: BStr<'_>, opts: ParseOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let toks = tokenize(s, opts).map_err(|e| ParseError::new(&e.msg))?;
    let mut parts: Vec<WordPart> = Vec::new();
    let mut first = true;
    for t in &toks {
        if let Tok::Word(segs) = t {
            if !first {
                parts.push(WordPart::Literal(" ".into()));
            }
            first = false;
            for seg in segs {
                parts.push(seg_to_part(seg, opts, Quoting::Bare)?);
            }
        }
    }
    Ok(Word { parts })
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Byte length of the identifier `s` begins with, or `0` if it does not begin
/// with one. Used to anchor assignment-word recognition to the name, so that a
/// `[` or `=` further right — in the *value* — cannot be mistaken for the start
/// of a subscript or for the assignment operator.
fn name_prefix_len(s: BStr<'_>) -> usize {
    let mut len = 0;
    // Byte-wise: every identifier character is ASCII, so a byte that is part of
    // a multi-byte character (or is not text at all) fails the test and ends the
    // name — it can never be mistaken for one.
    for (i, &b) in s.iter().enumerate() {
        let c = char::from(b);
        if i == 0 {
            if !is_name_start(c) {
                return 0;
            }
        } else if !is_name_char(c) {
            break;
        }
        len = i + 1;
    }
    len
}

/// Byte offset of the `]` closing the subscript `s` opens with, counting nested
/// brackets (`a[b[0]]=v`). `None` when it never closes inside `s` — which for
/// an assignment word means the subscript carries on into the next segment
/// (`m[$k]=v`), not that the word is malformed.
///
/// The subscript body is *not* scanned for quoting: bash reads it as arithmetic
/// (or, for an associative array, as a literal key) after the brackets have
/// already been matched, so a `]` inside it is a `]` here too.
fn balanced_subscript_end(s: BStr<'_>) -> Option<usize> {
    debug_assert!(s.first() == Some(&b'['));
    let mut depth = 0usize;
    for (i, &b) in s.iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The stand-in name a `${!@<op>}` / `${!*<op>}` modifier is parsed against, so
/// that it comes out as the scalar operator bash applies rather than the
/// whole-list one the same text would mean written directly. It never survives
/// into the tree — the referent replaces it immediately — so its only
/// requirement is to be a plain identifier.
const PLACEHOLDER_REFERENT: &[u8] = b"x";

/// Could `c`, standing right after the `!` of a `${!…}`, begin the name of the
/// parameter being indirected through?
///
/// This is the whole test bash makes before reading a `${!…}` as an indirection
/// at all: a name, a digit, or one of `# ? @ *`. Notably absent is `-`, so
/// `${!-x}` is `$!` with a default rather than an indirection through `$-`, as
/// are all the operator characters — `${!:0:1}` slices `$!`.
fn is_indirection_starter(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || matches!(c, '#' | '?' | '@' | '*')
}

/// A referent usable in a *bare* indirect expansion `${!name}`: a plain
/// identifier, a positional parameter (all digits, `${!1}`), or one of the
/// special parameters [`is_indirection_starter`] admits. A bare `@`/`*` is
/// indirect expansion through the positional list: `${!@}` / `${!*}` treat
/// each positional parameter's *value* as a variable name to indirect through
/// (bash then rejects them as "invalid variable name" unless empty). Only a
/// *prefixed* `@`/`*` (`${!prefix@}`) is the variable-name listing form.
fn is_indirect_referent(name: &str) -> bool {
    is_valid_name(name.as_bytes())
        || (!name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
        || matches!(name, "#" | "?" | "@" | "*")
}

/// Map a `[[ … ]]` unary operator string to its [`CondUnary`], keeping the
/// spelling it was written with (`-h` and `-L` are the same test but must print
/// back differently).
///
/// The set is [`crate::ast::unary_op_text`] — the same one the `test`/`[` builtin
/// recognises, because bash's is the same for both.
fn unary_op_from(s: BStr<'_>) -> Option<CondUnary> {
    crate::ast::unary_op_text(s).map(|text| CondUnary { text })
}

/// Where inside a `[[ … ]]` conditional an operand was expected — selects the
/// bash diagnostic emitted when the slot is empty (see `cond_operand_error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CondPos {
    /// Start of a primary (or after `!`/`&&`/`||`): `syntax error near \`X'`.
    Primary,
    /// Right after a unary operator (`-f`, `-z`, …): `unexpected argument \`X'
    /// to conditional unary operator`.
    Unary,
    /// Right after a binary operator (`==`, `-eq`, …): `unexpected argument
    /// \`X' to conditional binary operator`.
    Binary,
}

/// A failure inside `[[ … ]]`, kept in pieces until it is printed.
///
/// bash builds a conditional diagnostic from more than one frame. The frame
/// that could not parse says what it saw; each enclosing `( … )` group the
/// error passes through then adds the `)` it never reached; and only afterwards
/// is the closing `syntax error near \`X'` line appended. So the pieces arrive
/// in an order a rendered string cannot express — a group's clause belongs
/// *between* the inner diagnostic and the `near` line, and gluing the `near`
/// line on at the raising frame would leave it nowhere to go:
///
/// ```sh
/// [[ ( ;Q ) ]]   # unexpected token `;' in conditional command
///                # expected `)'
///                # syntax error near `;Q'
/// ```
///
/// Each clause also carries the line *it* is reported at, because a group's is
/// the line of its own `(` rather than the line the failure was found on (see
/// [`ParseError::line_at`]).
enum CondError {
    /// A conditional diagnostic: the clauses in the order bash prints them,
    /// then the last line — `syntax error near \`X'`, or the end-of-input one.
    Cond {
        clauses: Vec<(Option<u32>, Str)>,
        tail: Str,
    },
    /// A failure that is not part of that model, raised by something the
    /// conditional grammar merely calls — the word lowering underneath it. bash
    /// reports those from elsewhere entirely, so no group adds anything to them.
    Other(ParseError),
}

impl From<ParseError> for CondError {
    fn from(e: ParseError) -> Self {
        CondError::Other(e)
    }
}

impl CondError {
    /// A diagnostic with one clause of its own, reported wherever the enclosing
    /// error is, followed by `tail`.
    fn new(clause: &(impl bytes::PushBytes + ?Sized), tail: &(impl bytes::PushBytes + ?Sized)) -> Self {
        CondError::Cond {
            clauses: vec![(None, bfmt![clause])],
            tail: bfmt![tail],
        }
    }

    /// A diagnostic that is only its last line — the primary position bash
    /// cannot name a token for (`[[ ( ]]`, whose `]]` leaves through an earlier
    /// arm) prints just the `near` line.
    fn bare(tail: &(impl bytes::PushBytes + ?Sized)) -> Self {
        CondError::Cond {
            clauses: Vec::new(),
            tail: bfmt![tail],
        }
    }

    /// Note that this error passed out through a `( … )` group whose `(` was on
    /// line `open`: bash's `cond_term` checks for the `)` it never reached and
    /// reports that too, at the line the group started on.
    fn in_group(self, open: u32) -> Self {
        match self {
            CondError::Cond { mut clauses, tail } => {
                clauses.push((Some(open), b"expected `)'".to_vec()));
                CondError::Cond { clauses, tail }
            }
            other => other,
        }
    }

    /// Render, now that no more clauses can arrive.
    fn into_parse_error(self) -> ParseError {
        let (clauses, tail) = match self {
            CondError::Other(e) => return e,
            CondError::Cond { clauses, tail } => (clauses, tail),
        };
        let mut msg = Str::new();
        let mut line_at = Vec::new();
        for (i, (line, text)) in clauses.iter().enumerate() {
            if let Some(l) = *line {
                line_at.push((u32::try_from(i).unwrap_or(u32::MAX), l));
            }
            msg.extend_from_slice(text);
            msg.push(b'\n');
        }
        msg.extend_from_slice(&tail);
        ParseError { line_at, ..ParseError::new(&msg) }
    }
}

/// Raw binary operator recognised inside `[[ … ]]` (before lowering; `Regex`
/// is recognised so it can be rejected with a clear message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawBinOp {
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    Regex,
    NumEq,
    NumNe,
    NumLt,
    NumLe,
    NumGt,
    NumGe,
    FileNewer,
    FileOlder,
    SameFile,
}

impl RawBinOp {
    fn into_bin_op(self, text: &'static str) -> CondBinary {
        let op = match self {
            RawBinOp::StrEq => CondBinOp::StrEq,
            RawBinOp::StrNe => CondBinOp::StrNe,
            RawBinOp::StrLt => CondBinOp::StrLt,
            RawBinOp::StrGt => CondBinOp::StrGt,
            // `Regex` is rejected before lowering; map defensively to StrEq.
            RawBinOp::Regex => CondBinOp::StrEq,
            RawBinOp::NumEq => CondBinOp::NumEq,
            RawBinOp::NumNe => CondBinOp::NumNe,
            RawBinOp::NumLt => CondBinOp::NumLt,
            RawBinOp::NumLe => CondBinOp::NumLe,
            RawBinOp::NumGt => CondBinOp::NumGt,
            RawBinOp::NumGe => CondBinOp::NumGe,
            RawBinOp::FileNewer => CondBinOp::FileNewer,
            RawBinOp::FileOlder => CondBinOp::FileOlder,
            RawBinOp::SameFile => CondBinOp::SameFile,
        };
        CondBinary { op, text }
    }
}

/// Map a `[[ … ]]` binary operator word to its [`RawBinOp`] and the spelling it
/// was written with — `==` and `=` are the same comparison under two names, and
/// bash prints back whichever one the source used.
fn raw_binop_from(s: BStr<'_>) -> Option<(RawBinOp, &'static str)> {
    const OPS: &[(&str, RawBinOp)] = &[
        ("==", RawBinOp::StrEq),
        ("=", RawBinOp::StrEq),
        ("!=", RawBinOp::StrNe),
        ("=~", RawBinOp::Regex),
        ("-eq", RawBinOp::NumEq),
        ("-ne", RawBinOp::NumNe),
        ("-lt", RawBinOp::NumLt),
        ("-le", RawBinOp::NumLe),
        ("-gt", RawBinOp::NumGt),
        ("-ge", RawBinOp::NumGe),
        ("-nt", RawBinOp::FileNewer),
        ("-ot", RawBinOp::FileOlder),
        ("-ef", RawBinOp::SameFile),
    ];
    OPS.iter().find(|(text, _)| text.as_bytes() == s).map(|&(text, op)| (op, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests spell their input as Rust source, which is UTF-8 by
    /// construction, so they call the byte-string parser through this shadow
    /// rather than writing `b"..."` at every site. A local item shadows the
    /// glob import above, so `parse` inside this module means this function.
    fn parse(src: &str) -> Result<Program, ParseError> {
        super::parse(src.as_bytes())
    }

    /// A [`ParseError`]'s message as text, for assertions.
    ///
    /// The cases below spell bash's exact diagnostics as Rust string literals
    /// while the parser speaks bytes, so this panics rather than approximating:
    /// a diagnostic that stopped being text is a failure of the case, not
    /// something to paper over. The one case that *is* about a non-text
    /// diagnostic compares bytes directly.
    #[track_caller]
    fn emsg(e: &ParseError) -> String {
        String::from_utf8(e.msg.clone()).expect("diagnostic is text")
    }

    /// The text of a byte string an assertion wants to compare against a
    /// literal. Every such value in these tests is ASCII by construction, so a
    /// non-UTF-8 one is a test bug and should fail loudly.
    fn text(s: BStr<'_>) -> String {
        bytes::as_str(s)
            .unwrap_or_else(|| panic!("expected UTF-8, got {s:?}"))
            .to_owned()
    }

    /// A NUL never reaches a token: the reader drops it, as bash's `shell_getc`
    /// does, so it can neither split a word nor end up inside one
    /// (TD-OILS-NUL-IN-SOURCE).
    #[test]
    fn the_reader_drops_a_nul_before_the_lexer_sees_it() {
        assert_eq!(&*crate::lexer::strip_nuls(b"echo hi"), b"echo hi");
        assert_eq!(&*crate::lexer::strip_nuls(b"a\0b\0"), b"ab");

        // Inside a word, the two halves join rather than becoming two words.
        let prog = super::parse(b"echo a\0b").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected a simple command");
        };
        assert_eq!(sc.words.len(), 2);
        let [WordPart::Literal(w)] = sc.words[1].parts.as_slice() else {
            panic!("expected one literal part, got {:?}", sc.words[1].parts);
        };
        assert_eq!(text(w), "ab");

        // A line that is nothing but a NUL is a blank line, so the units either
        // side of it are still two commands on lines 1 and 3.
        let mut ip = IncrementalParser::new(b"echo one\n\0\necho two\n", 0, ParseOpts::default());
        let opts = ParseOpts::default();
        let mut lines = Vec::new();
        while let Some(u) = ip.next_unit(None, opts) {
            lines.push(u.expect("parses").items[0].line);
        }
        assert_eq!(lines, vec![1, 3]);
    }

    #[test]
    fn simple_command() {
        let prog = parse("echo hello world").unwrap();
        assert_eq!(prog.items.len(), 1);
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected simple command");
        };
        assert_eq!(sc.words.len(), 3);
    }

    /// bash numbers a simple command by `line_number` at the moment its first
    /// grammar element reduces — which for a leading plain word is *after* one
    /// token of lookahead (needed to rule out a function definition), and for a
    /// leading assignment is immediately. Every expectation is bash 5.2.37's
    /// own `$LINENO` for the same source. See [`Parser::simple_command_line`].
    #[test]
    fn simple_command_line_follows_bashs_lookahead_rule() {
        fn line_of(src: &str) -> u32 {
            let prog = parse(src).unwrap();
            let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
                panic!("expected simple command");
            };
            sc.line
        }
        // The lookahead word ends on line 2, so the command is numbered 2 —
        // even though it *starts* on line 1.
        assert_eq!(line_of("echo \"a\nb\" $LINENO"), 2);
        // A multi-line word that comes after the lookahead does not count.
        assert_eq!(line_of("echo $LINENO \"x\ny\""), 1);
        // A `\<newline>` between two words belongs to neither, so a lookahead
        // word that precedes it still ends on line 1 …
        assert_eq!(line_of("echo A \\\nB"), 1);
        // … while one reached across it ends on line 2.
        assert_eq!(line_of("echo \\\nB"), 2);
        // Assignments reduce without lookahead: numbered by their own extent.
        assert_eq!(line_of("v=1 echo \"a\nb\""), 1);
        assert_eq!(line_of("v=\"a\nb\" echo c"), 2);
        assert_eq!(line_of("a=(1) echo \"x\ny\""), 1);
        assert_eq!(line_of("a=(1\n2) echo x"), 2);
        // A lone word has only the newline to look ahead at, and reading that
        // never fetches the following line.
        assert_eq!(line_of("echo \"a\nb\"\ncmd\n"), 2);
    }

    /// Every parser diagnostic that quotes a shell construct back quotes the
    /// *bytes* the user wrote. A word may hold any byte but NUL, so a message
    /// that decoded one would blame something other than what was typed — and
    /// for the here-document case it would name a delimiter that is not the one
    /// the reader was actually comparing body lines against.
    ///
    /// These go through `super::parse` on a byte literal, not the text-shadowing
    /// `parse`/[`emsg`] pair the cases above use, since decoding is the thing
    /// under test.
    #[test]
    fn diagnostics_quote_the_source_bytes_back() {
        // The offending token, named by its source spelling.
        let e = super::parse(b"f() a\xffb").unwrap_err();
        assert_eq!(e.msg, b"syntax error near unexpected token `a\xffb'");

        // A loop variable that is not an identifier — and cannot be one, since a
        // name is ASCII by construction — *parses*; the refusal is bash's at run
        // time. The clause therefore has to carry the bytes through unchanged so
        // that message can quote them (see the corpus case
        // `a-for-loops-variable-is-a-word-until-the-loop-runs.sh`).
        let prog = super::parse(b"for a\xffb in x; do :; done").unwrap();
        let Command::For(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected for");
        };
        assert_eq!(f.var, b"a\xffb");
        let prog = super::parse(b"select a\xffb in x; do :; done").unwrap();
        let Command::Select(s) = &prog.items[0].list.first.commands[0] else {
            panic!("expected select");
        };
        assert_eq!(s.var, b"a\xffb");

        // The `[[ … ]]` family names its token the same way, on the second line.
        let e = super::parse(b"[[ -z x a\xffb ]]").unwrap_err();
        assert_eq!(
            e.msg,
            b"syntax error in conditional expression\nsyntax error near `a\xffb'"
        );

        // A construct left open is reported by the lexer, whose message carries
        // the same bytes and still classifies as *incomplete* — so a REPL line
        // holding one keeps reading rather than erroring out.
        let e = super::parse(b"echo \"a\xffb").unwrap_err();
        assert_eq!(e.msg, b"unexpected EOF while looking for matching `\"'");
        assert!(e.is_incomplete());
    }

    /// bash names the offending word in a syntax error by its *source* spelling,
    /// so the message shows the quotes the user typed rather than the string
    /// they stand for. Every expectation here is bash 5.2.37's own wording.
    #[test]
    fn syntax_error_names_the_token_as_written() {
        fn err(src: &str) -> String {
            emsg(&parse(src).expect_err("expected a parse error"))
        }
        // Inside `[[ ]]` bash prefixes the complaint with what it wanted, then
        // names the word it found. Each quoting keeps its own spelling: it is
        // `'<'` that is reported, not the `<` the quotes stand for.
        let want = "conditional binary operator expected\nsyntax error near ";
        assert_eq!(err("[[ a '<' b ]]"), format!("{want}`'<''"));
        assert_eq!(err("[[ a \"<\" b ]]"), format!("{want}`\"<\"'"));
        assert_eq!(err("[[ a \\< b ]]"), format!("{want}`\\<'"));
        // An unexpanded parameter is named as written, not as expanded.
        assert_eq!(err("[[ a $x-q b ]]"), format!("{want}`$x-q'"));
        // A word that needs no quoting is still spelled plainly, and quoting
        // elsewhere in the test does not leak into the name of the offender.
        assert_eq!(err("[[ a -q b ]]"), format!("{want}`-q'"));
        assert_eq!(err("[[ \"a b\" -q c ]]"), format!("{want}`-q'"));
        // Outside a conditional, an operator token still names itself.
        assert_eq!(err("echo hi; ;"), "syntax error near unexpected token `;'");
    }

    /// Inside `[[ ]]` bash treats an *operator* it finds in the wrong place
    /// differently from a word: the operator gets an `unexpected token \`X''
    /// clause naming it, the word does not. That holds in both positions where a
    /// stray token can turn up, and the `syntax error near' line that follows
    /// spells the token by bash's own backward source scan rather than by name.
    #[test]
    fn conditional_error_names_operators_but_not_words() {
        fn err(src: &str) -> String {
            emsg(&parse(src).expect_err("expected a parse error"))
        }
        // Where a binary operator was expected, an operator is named …
        let want = "conditional binary operator expected\nsyntax error near ";
        assert_eq!(err("[[ a ( b ]]"), format!("unexpected token `(', {want}`('"));
        assert_eq!(err("[[ a ; b ]]"), format!("unexpected token `;', {want}`;'"));
        assert_eq!(err("[[ a | b ]]"), format!("unexpected token `|', {want}`|'"));
        assert_eq!(err("[[ a & b ]]"), format!("unexpected token `&', {want}`&'"));
        // … while a word in the same place is only reported, never named.
        assert_eq!(err("[[ a b ]]"), format!("{want}`b'"));
        // Where the expression was already complete, the same split applies.
        let stray = "syntax error in conditional expression";
        assert_eq!(
            err("[[ -n a ; b ]]"),
            format!("{stray}: unexpected token `;'\nsyntax error near `;'")
        );
        assert_eq!(err("[[ -z x y ]]"), format!("{stray}\nsyntax error near `y'"));
        // A compound operator names itself in full, but the `near' line prints
        // only what bash's backward scan reaches: it halts on `;', `|' or `&',
        // so `;;' shows `;' and `;&', `;;&', `|&' and `>&' all show `&'. An
        // operator holding none of those delimiters is printed whole.
        for (src, tok, near) in [
            (";;", ";;", ";"),
            (";&", ";&", "&"),
            (";;&", ";;&", "&"),
            ("|&", "|&", "&"),
            (">&", ">&", "&"),
            (">>", ">>", ">>"),
            ("<<<", "<<<", "<<<"),
        ] {
            assert_eq!(
                err(&format!("[[ a {src} b ]]")),
                format!("unexpected token `{tok}', {want}`{near}'"),
                "binary-operator position: {src}"
            );
            assert_eq!(
                err(&format!("[[ -n a {src} b ]]")),
                format!("{stray}: unexpected token `{tok}'\nsyntax error near `{near}'"),
                "stray-token position: {src}"
            );
        }
        // The tokens that may legitimately follow a finished operand are still
        // accepted rather than swept up by the operator rule.
        for src in ["[[ a && b ]]", "[[ a || b ]]", "[[ ( a ) ]]", "[[ ( a || b ) ]]"] {
            parse(src).unwrap_or_else(|e| panic!("{src}: {}", emsg(&e)));
        }
    }

    #[test]
    fn pipeline_and_andor() {
        let prog = parse("a | b && c || d").unwrap();
        let ao = &prog.items[0].list;
        assert_eq!(ao.first.commands.len(), 2);
        assert_eq!(ao.rest.len(), 2);
    }

    #[test]
    fn assignment_and_redirect() {
        let prog = parse("FOO=bar cmd arg > out 2> err").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.redirects.len(), 2);
        assert_eq!(sc.redirects[1].fd, 2);
    }

    #[test]
    fn if_clause() {
        let prog = parse("if true; then echo yes; else echo no; fi").unwrap();
        let Command::If(_) = &prog.items[0].list.first.commands[0] else {
            panic!("expected if");
        };
    }

    #[test]
    fn for_loop() {
        let prog = parse("for x in a b c; do echo $x; done").unwrap();
        let Command::For(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected for");
        };
        assert_eq!(f.var, b"x");
        assert_eq!(f.words.as_ref().unwrap().len(), 3);
    }

    /// bash's `for`/`select` productions accept any WORD and leave the name
    /// check to execution, so the parser's job is only to record the spelling.
    /// Every expectation is bash 5.2.37's own — the word it quotes back in
    /// `` `WORD': not a valid identifier ``.
    #[test]
    fn loop_variable_is_any_word() {
        let var = |kw: &str, src: &str| {
            let prog = parse(&format!("{kw} {src} in a; do :; done")).unwrap();
            match &prog.items[0].list.first.commands[0] {
                Command::For(f) => text(&f.var),
                Command::Select(s) => text(&s.var),
                _ => panic!("expected a loop: {src}"),
            }
        };
        // Quoted, escaped, expanded, an assignment, a tilde — all parse, and all
        // keep the spelling that will be refused at run time.
        for name in
            ["1x", "'a[0]'", "\"a[0]\"", "a=b", "a\\[0\\]", "''", "'a b'", "~", "a.b", "$v", "\\x"]
        {
            assert_eq!(var("for", name), name.to_string(), "for {name}");
            assert_eq!(var("select", name), name.to_string(), "select {name}");
        }
        // A reserved word is only reserved in command position, so it is an
        // ordinary — and perfectly valid — loop variable here.
        for name in ["do", "in", "if", "time", "x"] {
            assert_eq!(var("for", name), name.to_string(), "for {name}");
        }
        // What is *not* a word at all is still a syntax error, as in bash.
        for src in ["for; do :; done", "for >f in a; do :; done", "for"] {
            assert!(parse(src).is_err(), "should not have parsed: {src}");
        }
    }

    #[test]
    fn while_loop() {
        let prog = parse("while false; do echo x; done").unwrap();
        let Command::Loop(l) = &prog.items[0].list.first.commands[0] else {
            panic!("expected loop");
        };
        assert!(!l.until);
    }

    #[test]
    fn case_statement() {
        let prog =
            parse("case $x in a|b) echo ab;; *.txt) echo text;; *) echo default;; esac").unwrap();
        let Command::Case(c) = &prog.items[0].list.first.commands[0] else {
            panic!("expected case");
        };
        assert_eq!(c.items.len(), 3);
        assert_eq!(c.items[0].patterns.len(), 2);
        assert_eq!(c.items[2].patterns.len(), 1);
    }

    #[test]
    fn case_empty_and_final_no_dsemi() {
        // Last arm may omit `;;`; an empty body is allowed.
        let prog = parse("case y in x) ;; y) echo hit\nesac").unwrap();
        let Command::Case(c) = &prog.items[0].list.first.commands[0] else {
            panic!("expected case");
        };
        assert_eq!(c.items.len(), 2);
        assert!(c.items[0].body.items.is_empty());
    }

    #[test]
    fn here_doc_and_here_string() {
        let prog = parse("cat <<EOF\nhi\nEOF\n").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected simple");
        };
        assert_eq!(sc.redirects.len(), 1);
        assert!(matches!(sc.redirects[0].op, RedirectOp::HereDoc));
        assert_eq!(sc.redirects[0].fd, 0);

        let prog2 = parse("cat <<< hello").unwrap();
        let Command::Simple(sc2) = &prog2.items[0].list.first.commands[0] else {
            panic!("expected simple");
        };
        assert!(matches!(sc2.redirects[0].op, RedirectOp::HereStr));
    }

    #[test]
    fn function_def() {
        let prog = parse("greet() { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(text(&f.name), "greet");
    }

    /// A `(` after a word is the start of a function definition and nothing
    /// else, so once it has been read the `)` is compulsory and whatever stands
    /// in its place is what the diagnostic names — never the `(` itself. Every
    /// expectation is bash 5.2.37's own.
    #[test]
    fn a_paren_after_a_word_commits_to_a_function_definition() {
        let err = |src: &str| String::from_utf8_lossy(&parse(src).unwrap_err().msg).into_owned();
        assert_eq!(err("a ( b"), "syntax error near unexpected token `b'");
        assert_eq!(err("a (b)"), "syntax error near unexpected token `b'");
        assert_eq!(err("a ( ;"), "syntax error near unexpected token `;'");
        assert_eq!(err("a (\nb"), "syntax error near unexpected token `newline'");
        // The unit's own closing newline counts as the token that was found.
        assert_eq!(err("a ("), "syntax error near unexpected token `newline'");
        // `((` where no command can start is two `(` tokens, so the second one
        // is the token found where the `)` was wanted.
        assert_eq!(err("a (("), "syntax error near unexpected token `('");
        assert_eq!(err("echo ((1))"), "syntax error near unexpected token `('");
        assert_eq!(err("x=1 ((2))"), "syntax error near unexpected token `('");
        // An assignment word is not part of the production, so `(` after one is
        // still reported as itself.
        assert_eq!(err("a=b ( c"), "syntax error near unexpected token `('");
        // And the definition itself still parses.
        assert!(parse("a ( ) { :; }").is_ok());
    }

    /// `((` opens an arithmetic command only where a reserved word would be
    /// recognised. Everywhere else it is a `(` followed by another `(`.
    #[test]
    fn arith_command_only_where_a_command_can_start() {
        for src in [
            "((1))",
            "if ((1)); then :; fi",
            "while ((1)); do :; done",
            "true && ((1))",
            "true | ((1))",
            "true; ((1))",
            "true &\n((1))",
            "{ ((1)); }",
            "( ((1)) )",
            "! ((0))",
            "time ((1))",
            "for ((i=0;i<2;i++)); do :; done",
            "case x in a) ((1));; esac",
        ] {
            assert!(parse(src).is_ok(), "should have parsed: {src}");
        }
        // After a word, an assignment, or `in`, the `((` is two parens — which
        // is a syntax error in each of these, as it is in bash.
        for src in ["echo ((1))", "x=1 ((2))", "case x in ((x) :;; esac"] {
            assert!(parse(src).is_err(), "should not have parsed: {src}");
        }
    }

    /// bash's `WORD ( )` production accepts any word, and defers the name check
    /// to execution, so the parser's job is only to record what was written and
    /// whether it was written bare. Every expectation is bash 5.2.37's own.
    #[test]
    fn function_name_is_any_word() {
        let def = |src: &str| {
            let prog = parse(src).unwrap();
            let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
                panic!("expected function: {src}");
            };
            (text(&f.name), f.definable)
        };
        // Not an identifier, but a perfectly good function name.
        for name in ["my-func", "a.b", "1f", "a/b", "a,b", ".f", "f%", "[b]", "a[b]"] {
            assert_eq!(def(&format!("{name}() {{ :; }}")), (name.to_string(), true));
        }
        // Quoted or expanded: parses, keeps the spelling, and is not definable.
        for name in ["\\f", "\"f\"", "'f'", "f\\g", "$x"] {
            assert_eq!(def(&format!("{name}() {{ :; }}")), (name.to_string(), false));
            assert_eq!(def(&format!("function {name} {{ :; }}")), (name.to_string(), false));
        }
        // An assignment word is not a WORD, so the POSIX form rejects it — but
        // the lexer only forms one at the start of a command, so the keyword
        // form takes the same spelling as an ordinary name.
        assert!(parse("f=g() { :; }").is_err());
        assert_eq!(def("function f=g { :; }"), ("f=g".to_string(), true));
        // Escaping the `=` demotes the word, so it parses — and, being quoted,
        // it is left for execution to refuse.
        assert_eq!(def("a\\=b() { :; }"), ("a\\=b".to_string(), false));
        // A reserved word is still the keyword, not a name.
        assert!(parse("if() { :; }").is_err());
        assert!(parse("time() { :; }").is_err());
    }

    #[test]
    fn function_keyword_forms() {
        // bash keyword form without parentheses.
        let prog = parse("function greet { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(text(&f.name), "greet");

        // bash keyword form WITH parentheses.
        let prog = parse("function greet() { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(text(&f.name), "greet");

        // A non-identifier name is permitted in the keyword form.
        let prog = parse("function foo-bar { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(text(&f.name), "foo-bar");

        // Multi-line body and a subshell body.
        assert!(parse("function f {\necho a\necho b\n}").is_ok());
        let prog = parse("function f() ( echo sub )").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(text(&f.name), "f");

        // Trailing redirection is attached to the definition.
        let prog = parse("function f { echo a; } >/dev/null").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.redirects.len(), 1);
    }

    #[test]
    fn function_missing_body_errors_like_bash() {
        // At EOF, both the keyword form and the POSIX form report bash's
        // canonical "unexpected end of file" (not a bespoke message).
        assert_eq!(
            emsg(&parse("function f").unwrap_err()),
            "syntax error: unexpected end of file"
        );
        assert_eq!(
            emsg(&parse("f()").unwrap_err()),
            "syntax error: unexpected end of file"
        );
        // A non-body token after the header names the offending token.
        assert_eq!(
            emsg(&parse("f() echo hi").unwrap_err()),
            "syntax error near unexpected token `echo'"
        );
    }

    #[test]
    fn case_and_subshell_errors_name_the_offending_token() {
        // bash reports these as `near unexpected token \`X'` (or `unexpected end
        // of file` at EOF), naming the token its cursor sits on — not a bespoke
        // fragment. osh's parser now matches: the cursor already points at the
        // culprit, so each site defers to `unexpected_here`.
        for (src, want) in [
            ("case x in ;& esac", "syntax error near unexpected token `;&'"),
            ("case x in pat esac", "syntax error near unexpected token `esac'"),
            ("case x in )", "syntax error near unexpected token `)'"),
            ("case ; in", "syntax error near unexpected token `;'"),
            ("case )", "syntax error near unexpected token `)'"),
            ("( )", "syntax error near unexpected token `)'"),
            ("( echo hi", "syntax error: unexpected end of file"),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), want, "src {src:?}");
        }
        // A well-formed `case` still parses (guard against over-eager erroring).
        assert!(parse("case x in a) echo 1;; b) echo 2;; esac").is_ok());
    }

    #[test]
    fn redirect_target_errors_name_the_offending_token() {
        // A redirection whose target slot is occupied by an operator reports
        // that operator by its literal spelling, exactly like bash's
        // `near unexpected token \`>'`. The redirect site now peeks (rather than
        // bumps) so `unexpected_here` sees the culprit; `token_display` spells
        // out every redirect operator.
        for (src, want) in [
            ("echo > >", "syntax error near unexpected token `>'"),
            ("echo > >>", "syntax error near unexpected token `>>'"),
            ("echo > >&", "syntax error near unexpected token `>&'"),
            ("echo > <&", "syntax error near unexpected token `<&'"),
            ("echo > <>", "syntax error near unexpected token `<>'"),
            ("echo > &>", "syntax error near unexpected token `&>'"),
            ("echo > &>>", "syntax error near unexpected token `&>>'"),
            ("echo > <<<", "syntax error near unexpected token `<<<'"),
            ("echo > >|", "syntax error near unexpected token `>|'"),
            ("echo > <<", "syntax error near unexpected token `<<'"),
            ("echo > |", "syntax error near unexpected token `|'"),
            ("echo > &&", "syntax error near unexpected token `&&'"),
            ("echo > ;", "syntax error near unexpected token `;'"),
            ("echo > )", "syntax error near unexpected token `)'"),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), want, "src {src:?}");
        }
        // A well-formed redirection still parses.
        assert!(parse("echo hi > out.txt").is_ok());
    }

    #[test]
    fn a_lone_conditional_end_is_a_reserved_word_wherever_a_command_could_start() {
        // `CHECK_FOR_RESERVED_WORD` looks `]]` up in `word_token_alist` on the
        // single condition `reserved_word_acceptable (last_read_token)`, so it
        // is the token `COND_END` wherever a command could start — no open `[[`
        // required — and the grammar has no production that begins with one.
        for src in [
            "]]",
            "true; ]]",
            "]] ]]",
            "true && ]]",
            "true || ]]",
            "true | ]]",
            "{ ]]; }",
            "( ]] )",
            "if ]]; then :; fi",
            "while ]]; do :; done",
            "until ]]; do break; done",
            "for x in a; do ]]; done",
            "case a in a) ]];; esac",
        ] {
            assert_eq!(
                emsg(&parse(src).unwrap_err()),
                "syntax error near unexpected token `]]'",
                "src {src:?}"
            );
        }
        // Past the command word `reserved_word_acceptable` is false, so `]]` is
        // an ordinary word — and the pairing it exists for still closes.
        for src in [
            "echo ]]",
            "echo a ]] b",
            "x=]]",
            "for x in ]]; do echo $x; done",
            "case ]] in ]]) echo hit;; esac",
            "echo \"]]\" ${x-]]}",
            "[[ x ]]",
        ] {
            assert!(parse(src).is_ok(), "src {src:?}");
        }
    }

    #[test]
    fn cond_expr_errors_match_bash_phrasing() {
        // `[[ … ]]` uses bash's conditional-specific diagnostics: a bare
        // `syntax error near \`X'` in primary position, an `unexpected argument
        // \`X' to conditional {unary,binary} operator` (with the `near` line
        // appended) after an operator, and the `unexpected EOF while looking
        // for \`]]'` form when the closer is missing at end of input.
        for (src, want) in [
            ("[[ ]]", "syntax error near `]]'"),
            ("[[ a && ]]", "syntax error near `]]'"),
            ("[[ a || ]]", "syntax error near `]]'"),
            ("[[ ! ]]", "syntax error near `]]'"),
            (
                "[[ a == ]]",
                "unexpected argument `]]' to conditional binary operator\nsyntax error near `]]'",
            ),
            (
                "[[ a -eq ]]",
                "unexpected argument `]]' to conditional binary operator\nsyntax error near `]]'",
            ),
            (
                "[[ -f ]]",
                "unexpected argument `]]' to conditional unary operator\nsyntax error near `]]'",
            ),
            (
                "[[ ( a ]]",
                "unexpected token `]]', expected `)'\nsyntax error near `]]'",
            ),
            (
                "[[ a == b",
                "unexpected EOF while looking for `]]'\nsyntax error: unexpected end of file",
            ),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), want, "src {src:?}");
        }
        // Well-formed conditionals still parse.
        assert!(parse("[[ -f /etc ]]").is_ok());
        assert!(parse("[[ a == a && ( b == b || c == c ) ]]").is_ok());
        assert!(parse("[[ ! -z x ]]").is_ok());
    }

    #[test]
    fn param_expansions() {
        let prog = parse("echo ${x:-default} ${#y}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(sc.words[1].parts[0], WordPart::ParamOp { .. }));
        assert!(matches!(sc.words[2].parts[0], WordPart::Length(_)));
    }

    #[test]
    fn array_ref_parsing() {
        let prog = parse("echo ${a[0]} ${a[@]} ${a[*]} ${#a[@]}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(
            sc.words[1].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::Index(_),
                length: false,
                ..
            }
        ));
        assert!(matches!(
            sc.words[2].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::All,
                length: false,
                ..
            }
        ));
        assert!(matches!(
            sc.words[3].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::Star,
                ..
            }
        ));
        assert!(matches!(
            sc.words[4].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::All,
                length: true,
                ..
            }
        ));
    }

    #[test]
    fn array_assignment_parsing() {
        let prog = parse("a=(one two three)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        let AssignRhs::Array(words) = &sc.assignments[0].value else {
            panic!("expected array assignment");
        };
        assert_eq!(words.len(), 3);
        assert!(!sc.assignments[0].append);
    }

    #[test]
    fn array_append_and_index_assignment() {
        let prog = parse("a+=(x); a[2]=y").unwrap();
        let Command::Simple(app) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(app.assignments[0].append);
        let idx = parse("a[2]=y").unwrap();
        let Command::Simple(sc) = &idx.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments[0].index.is_some());
        assert!(matches!(sc.assignments[0].value, AssignRhs::Scalar(_)));
    }

    #[test]
    fn assoc_keyed_literal_and_keys_parsing() {
        // `${!m[@]}` → ArrayKeys.
        let prog = parse("echo ${!m[@]} ${!m[*]}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(
            sc.words[1].parts[0],
            WordPart::ArrayKeys { star: false, .. }
        ));
        assert!(matches!(
            sc.words[2].parts[0],
            WordPart::ArrayKeys { star: true, .. }
        ));
        // Keyed array-literal element `[k]=v`.
        let prog = parse("m=([a]=1 plain)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        let AssignRhs::Array(items) = &sc.assignments[0].value else {
            panic!("expected array literal");
        };
        assert!(matches!(items[0], ArrayElem::Keyed { .. }));
        assert!(matches!(items[1], ArrayElem::Positional(_)));
    }

    #[test]
    fn spanning_subscript_assignment_parsing() {
        // `m[$k]=v` — subscript spans segments; still recognised as assignment.
        let prog = parse("m[$k]=v").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments[0].index.is_some());
        assert!(!sc.assignments[0].append);
        assert!(sc.words.is_empty());
    }

    #[test]
    fn declare_array_operand_parsing() {
        // `declare -A m=([k]=v)` — the array literal after the command word is
        // captured as a declaration operand, not a leading prefix assignment.
        let prog = parse("declare -A m=([k]=v)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments.is_empty());
        assert_eq!(sc.decl_arrays.len(), 1);
        assert_eq!(sc.decl_arrays[0].assign.name, "m");
        // The operand followed both words (`declare` and `-A`), which is what the
        // `set -x` line and `$BASH_COMMAND` need to place it back.
        assert_eq!(sc.decl_arrays[0].word_index, 2);
        // The command word and its flag are ordinary words.
        assert_eq!(sc.words.len(), 2);
    }

    #[test]
    fn array_literal_after_plain_command_rejected() {
        // Only declaration builtins may take an array-literal operand.
        assert!(parse("foo m=(a b)").is_err());
    }

    #[test]
    fn stray_word_after_compound_command_rejected() {
        // A compound command cannot be followed by a bare word without a
        // separator; bash rejects this and osh previously mis-ran the trailing
        // word(s) as a second command.
        assert!(parse("for i in 1 2; do echo $i; done extra").is_err());
        assert!(parse("while false; do :; done foo bar").is_err());
        assert!(parse("if true; then echo hi; fi extra").is_err());
        assert!(parse("{ echo a; } extra").is_err());
        assert!(parse("case x in x) :; esac extra").is_err());
        assert!(parse("( echo a ) extra").is_err());
        // A stray `;;` outside a case arm is likewise an error.
        assert!(parse("echo a ;;").is_err());
        // But legitimate followers (separators, redirects, pipes, `&&`, a
        // closing `)`/keyword) must still parse.
        assert!(parse("for i in 1; do echo $i; done > /dev/null").is_ok());
        assert!(parse("for i in 1; do echo $i; done | cat").is_ok());
        assert!(parse("while false; do :; done && echo ok").is_ok());
        assert!(parse("{ echo a; }; echo b").is_ok());
        assert!(parse("( echo a ); echo b").is_ok());
        assert!(parse("x=$(for i in 1 2; do echo $i; done)").is_ok());
    }

    #[test]
    fn empty_compound_list_rejected() {
        // A compound-command condition or body that reduces to nothing is a
        // syntax error in bash; osh previously accepted these (and an empty
        // `while` condition even looped forever).
        assert!(parse("( )").is_err());
        assert!(parse("{ }").is_err());
        assert!(parse("if true; then fi").is_err());
        assert!(parse("if ; then echo x; fi").is_err());
        assert!(parse("while ; do echo x; done").is_err());
        assert!(parse("while false; do done").is_err());
        assert!(parse("until false; do done").is_err());
        assert!(parse("for x in a; do done").is_err());
        // But an empty *command substitution* / top-level program is fine, as is
        // any non-empty compound body.
        assert!(parse("echo $()").is_ok());
        assert!(parse("echo $( )").is_ok());
        assert!(parse("").is_ok());
        assert!(parse("( : )").is_ok());
        assert!(parse("{ :; }").is_ok());
        assert!(parse("if true; then :; fi").is_ok());
    }

    #[test]
    fn bare_separator_rejected() {
        // A `;` or `&` with no preceding command denotes an empty command, which
        // bash rejects — but blank lines between commands are fine.
        assert!(parse("; echo hi").is_err());
        assert!(parse("& echo hi").is_err());
        assert!(parse("echo a ; ; echo b").is_err());
        assert!(parse("echo a\n\n\necho b").is_ok());
        assert!(parse("echo a ; echo b ;").is_ok());
        assert!(parse("echo a; echo b").is_ok());
    }

    #[test]
    fn command_substitution() {
        let prog = parse("echo $(echo nested)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(sc.words[1].parts[0], WordPart::CommandSub { .. }));
    }

    /// Parse a backtick body the way `Shell::command_sub_body` does at expansion
    /// time: as an input of its own, numbered from `close_line - 1`, one unit
    /// at a time.
    fn backtick_unit(src: &str, close_line: u32) -> Result<Program, ParseError> {
        let opts = ParseOpts::default();
        IncrementalParser::new(src.as_bytes(), LineMap::Offset(close_line.saturating_sub(1)), opts)
            .next_unit(None, opts)
            .unwrap_or_else(|| Ok(Program { items: Vec::new() }))
    }

    /// A `$( … )` body is closed by the substitution's `)`, which ends its list
    /// but is not a `list_terminator`. So the body needs no terminator of its
    /// own, yet `!`/`time` standing alone — which do need one — are refused,
    /// and a body that stops mid-construct is blamed on the `)` rather than on
    /// the implicit newline every other input ends with.
    #[test]
    fn cmdsub_body_is_terminated_by_its_closing_paren() {
        for src in [
            "echo $(echo x)",
            "echo $(echo x; )",
            "echo $( )",
            "echo $( !; echo x )",
            "cat <(echo x)",
            "cat <( !; echo x )",
        ] {
            assert!(parse(src).is_ok(), "{src} should parse");
        }
        for src in [
            "echo $( ! )",
            "echo $(!)",
            "echo $( ! ! )",
            "echo $(time)",
            "echo $( time -p )",
            "echo $( echo x; ! )",
            "echo $(for)",
            "echo $(case)",
            // A process substitution body is read the same way.
            "cat <( ! )",
            "cat <( time )",
            "cat <(for)",
            "cat >( ! )",
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!(emsg(&e), "syntax error near unexpected token `)'", "{src}");
            // Found inside the body, so it is fatal to whoever was reading it.
            assert!(e.fatal, "{src} should be a fatal substitution error");
        }
        // A backtick body is not parsed here at all — bash reads it only when
        // the word is expanded — so even a malformed one leaves the enclosing
        // command parsing cleanly. Parsed on its own it really is lexed as its
        // own input, so the implicit trailing newline stands.
        for src in ["echo `!`", "echo `for`", "echo `$( ! )`"] {
            assert!(parse(src).is_ok(), "{src} should parse (body deferred)");
        }
        assert_eq!(
            emsg(&backtick_unit("for", 1).unwrap_err()),
            "syntax error near unexpected token `newline'"
        );
        assert!(backtick_unit("!", 1).is_ok());
    }

    /// The two reads of a `$( … )` body number its lines differently, and the
    /// *eager* one — this parse, in the enclosing token stream — is the plain
    /// one: an error it raises names the body's true physical line. (The
    /// rank-based numbering belongs to the map this parse returns, which the
    /// interpreter uses for the expansion-time re-read; see
    /// `parse_cmdsub_body`.) Getting this wrong is invisible in a one-line
    /// script and off by the body's length in a real one.
    #[test]
    fn an_eager_cmdsub_body_error_names_its_physical_line() {
        // `for` is on line 4 of the enclosing source; the body's own numbering
        // would call it line 3, and the rank rule would call it 7.
        let src = "echo one\nx=$(echo a\necho b\nfor\necho d)\n";
        assert_eq!(parse(src).unwrap_err().line, Some(4));
        // The body's first line, and a body that opens on a line of its own.
        assert_eq!(parse("echo one\nx=$(for\necho b)\n").unwrap_err().line, Some(2));
        assert_eq!(parse("echo one\nx=$(\necho a\n\nfor\n)\n").unwrap_err().line, Some(5));
        // A nested body is numbered against the outer body's physical lines,
        // not against its own first line.
        let nested = "echo one\nx=$(echo a\necho $(echo b\nfor\necho c)\necho d)\n";
        assert_eq!(parse(nested).unwrap_err().line, Some(4));
        // A process substitution body was always numbered this way.
        assert_eq!(parse("echo one\ncat <(echo a\nfor\n)\n").unwrap_err().line, Some(3));
    }

    #[test]
    fn only_a_paren_body_error_is_fatal_to_its_reader() {
        // bash unwinds past `eval`/`source` for a `$( … )`/`<( … )` body error
        // but not for an ordinary one; `ParseError::fatal` is what carries that
        // apart. A backtick body is not marked: bash defers parsing it to
        // expansion time, so its failure never reaches the reader at all.
        for src in ["echo $( ! )", "cat <( ! )", "cat >(for)", "echo $( echo $(if) )"] {
            assert!(parse(src).unwrap_err().fatal, "{src}");
        }
        for src in ["for", "echo a | | b", "echo )", "if true"] {
            assert!(!parse(src).unwrap_err().fatal, "{src}");
        }
        // A backtick body never reaches the reader — it is parsed at expansion
        // time — so its errors cannot be fatal to *this* parse. Within that
        // deferred parse the flag still marks a nested `$( … )` body's error,
        // which is how `Shell::command_sub_body` picks the status bash gives:
        // 1 for the nested-paren case, 2 for an ordinary one.
        assert!(backtick_unit("echo $( ! )", 1).unwrap_err().fatal);
        assert!(!backtick_unit("for", 1).unwrap_err().fatal);
    }

    #[test]
    fn negated_pipeline() {
        let prog = parse("! false").unwrap();
        assert!(prog.items[0].list.first.negated);
    }

    #[test]
    fn time_keyword_pipeline() {
        let prog = parse("time echo hi").unwrap();
        let p = &prog.items[0].list.first;
        assert!(p.timed);
        assert!(!p.time_posix);
        // The `time` word is consumed, so the body is just `echo hi`.
        let Command::Simple(sc) = &p.commands[0] else { panic!() };
        assert_eq!(sc.words[0].parts.len(), 1);

        let prog = parse("time -p sleep 0 | cat").unwrap();
        let p = &prog.items[0].list.first;
        assert!(p.timed);
        assert!(p.time_posix);
        assert_eq!(p.commands.len(), 2);

        // The two prefixes interleave freely, and only the flags survive: each
        // `!` toggles, and a second `time` adds nothing.
        for src in ["time ! false", "! time false", "time time ! false"] {
            let p = &parse(src).unwrap().items[0].list.first;
            assert!(p.timed, "{src}");
            assert!(p.negated, "{src}");
        }
        assert!(!parse("! ! true").unwrap().items[0].list.first.negated);

        // `--` ends `time`'s options and selects the POSIX format, just as
        // `-p` does; both are recognised only as literal unquoted words.
        for src in ["time -- true", "time -p -- true"] {
            assert!(parse(src).unwrap().items[0].list.first.time_posix, "{src}");
        }

        // A bare `time` is still the reserved word: it times a null command
        // (there is no way to reach an external `time` without quoting it).
        let p = &parse("time").unwrap().items[0].list.first;
        assert!(p.timed);
        assert_eq!(p.commands, vec![Command::Simple(SimpleCommand::default())]);

        // `time` inside a `for … in` list stays a plain word.
        let prog = parse("for x in time now; do echo $x; done").unwrap();
        assert!(!prog.items[0].list.first.timed);
    }

    #[test]
    fn cond_expression() {
        let prog = parse("[[ $x == foo ]]").unwrap();
        let Command::Cond(CondExpr::Binary(_, op, _)) = &prog.items[0].list.first.commands[0]
        else {
            panic!("expected cond binary");
        };
        assert_eq!(op.op, CondBinOp::StrEq);
        assert_eq!(op.text, "==");
    }

    /// A synonym must keep the spelling it was written with: bash echoes the
    /// operator's source word back verbatim, both when `declare -f` reprints a
    /// function and in a `set -x` trace, so `-h` never becomes `-L` nor `=`
    /// become `==`.
    #[test]
    fn cond_operator_keeps_its_spelling() {
        let binop = |src: &str| {
            let prog = parse(src).unwrap();
            let Command::Cond(CondExpr::Binary(_, op, _)) = &prog.items[0].list.first.commands[0]
            else {
                panic!("expected cond binary");
            };
            (op.op, op.text)
        };
        assert_eq!(binop("[[ a = b ]]"), (CondBinOp::StrEq, "="));
        assert_eq!(binop("[[ a == b ]]"), (CondBinOp::StrEq, "=="));
        assert_eq!(binop("[[ a < b ]]"), (CondBinOp::StrLt, "<"));
        assert_eq!(binop("[[ a -ef b ]]"), (CondBinOp::SameFile, "-ef"));

        let unop = |src: &str| {
            let prog = parse(src).unwrap();
            let Command::Cond(CondExpr::Unary(op, _)) = &prog.items[0].list.first.commands[0] else {
                panic!("expected cond unary");
            };
            op.text
        };
        // The synonyms keep their own spelling rather than normalising to one
        // of the pair — that spelling is what a trace and a `declare -f` reprint
        // have to show, and now also what selects the test.
        assert_eq!(unop("[[ -h f ]]"), "-h");
        assert_eq!(unop("[[ -L f ]]"), "-L");
        assert_eq!(unop("[[ -n f ]]"), "-n");
        // The whole `test`/`[` set parses here too, which is the property that
        // sharing one table buys: these used to be syntax errors in `[[ ]]`.
        for op in ["-a", "-b", "-c", "-g", "-k", "-p", "-u", "-G", "-N", "-O", "-R", "-S"] {
            assert_eq!(unop(&format!("[[ {op} f ]]")), op);
        }
    }

    #[test]
    fn cond_logical_precedence() {
        // `||` binds looser than `&&`: a || b && c parses as a || (b && c).
        let prog = parse("[[ 1 -eq 1 || 2 -eq 2 && 3 -eq 3 ]]").unwrap();
        let Command::Cond(CondExpr::Or(_, right)) = &prog.items[0].list.first.commands[0] else {
            panic!("expected top-level Or");
        };
        assert!(matches!(**right, CondExpr::And(_, _)));
    }

    #[test]
    fn cond_regex_parses() {
        let prog = parse("[[ $x =~ foo ]]").unwrap();
        assert!(matches!(
            prog.items[0].list.first.commands[0],
            Command::Cond(CondExpr::Regex(_, _))
        ));
    }

    /// The `=~` RHS is one word, but an unquoted `( … )` group inside it holds
    /// on to blanks and shell operators, so the regex can span them. Outside a
    /// group the ordinary word boundaries are back.
    #[test]
    fn cond_regex_group_spans_blanks_and_operators() {
        let regex_of = |src: &str| -> String {
            let prog = parse(src).unwrap_or_else(|e| panic!("{src}: {}", emsg(&e)));
            let Command::Cond(CondExpr::Regex(_, rhs)) = &prog.items[0].list.first.commands[0]
            else {
                panic!("{src}: expected a regex conditional");
            };
            match rhs.parts.as_slice() {
                [WordPart::Literal(s)] => text(s),
                other => panic!("{src}: expected one literal part, got {other:?}"),
            }
        };
        assert_eq!(regex_of("[[ $x =~ (a b) ]]"), "(a b)");
        assert_eq!(regex_of("[[ $x =~ x(a b)y ]]"), "x(a b)y");
        assert_eq!(regex_of("[[ $x =~ ((a b)) ]]"), "((a b))");
        assert_eq!(regex_of("[[ $x =~ (a;b&c<d>e) ]]"), "(a;b&c<d>e)");
        assert_eq!(regex_of("[[ $x =~ (a\nb) ]]"), "(a\nb)");
        // `|`, `#` and `}` are regex characters, not shell syntax, at any depth.
        assert_eq!(regex_of("[[ $x =~ a|b#c}d ]]"), "a|b#c}d");
        // An escaped paren is a literal one: it neither opens nor closes.
        assert_eq!(regex_of("[[ $x =~ (a\\)b) ]]"), "(a\\)b)");
        // Once the group closes, an operator ends the word again.
        assert!(parse("[[ $x =~ (a b) c ]]").is_err());
        assert!(parse("[[ $x =~ a;b ]]").is_err());
        assert!(parse("[[ $x =~ a) ]]").is_err());
        // A group left open is the word reader's error, named as bash names it.
        let e = emsg(&parse("[[ $x =~ (a(b) ]]").unwrap_err());
        assert!(
            e.contains("unexpected EOF while looking for matching `)'"),
            "got {e}"
        );
    }

    #[test]
    fn arith_command() {
        let prog = parse("(( x + 1 ))").unwrap();
        let Command::Arith(raw) = &prog.items[0].list.first.commands[0] else {
            panic!("expected arith command");
        };
        assert_eq!(text(bytes::trim(raw)), "x + 1");
    }

    #[test]
    fn double_paren_vs_nested_subshell() {
        // `((` = arithmetic; `( (` = nested subshell.
        assert!(matches!(
            parse("(( 1 ))").unwrap().items[0].list.first.commands[0],
            Command::Arith(_)
        ));
        assert!(matches!(
            parse("( ( echo ) )").unwrap().items[0].list.first.commands[0],
            Command::Subshell(_)
        ));
    }
}
