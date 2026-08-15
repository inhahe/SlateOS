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
    AndOr, AndOrOp, ArithClause, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseClause,
    CaseItem,
    CaseTerm, CmdSubBody,
    Command,
    CondBinOp, CondBinary, CondClause, CondUnary, DeclArray,
    CondExpr, ForArithClause, ForClause, FunctionDef, HereDoc, IfClause, Item, ItemSep, LineMap,
    LoopClause,
    ParamOp,
    Pipeline, ProcSubBody, Program,
    Redirect, RedirectOp, ReplaceAnchor, SelectClause, SimpleCommand, SubDelim, SubshellClause,
    Word, WordPart,
};
use crate::assoc::AssocArray;
use crate::bfmt;
use crate::bytes::{self, BStr, Ch, Str};
use crate::lexer::{
    AliasExpansion, CmdSubSpan, DparenCopy, HeredocEof, ParseOpts, Op, ProcRead, ReaderWarning, Seg, Spanned,
    Tok, SubBody, SubOpen, TokSpan, Tokenized, UngatheredHeredoc,
    expand_aliases_tracked,
    tokenize_paren_body, tokenize_deferred, tokenize_spanned, word_is_assignment,
};
use crate::wordscan::BraceEnd;
use std::borrow::Cow;

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

/// The physical source line `chs[at]` sits on, given that `chs[0]` sits on
/// `line`.
///
/// A `${ … }` body is carved into fragments — a pattern, a replacement, an
/// operand — that are each lexed on their own, from their own line 1. bash keeps
/// no such second coordinate system: it reads one stream and reports the
/// physical line, so a `$( … )` that fails in the *replacement* half of
/// `${x/aaa`/`bbb/$(fi)}` is blamed on the line the `$(fi)` is really on. The
/// difference between a fragment's numbering and the physical one is just the
/// newlines in the body before it, which is what this counts.
///
/// Only [`Ch::U`] can hold a newline — [`Ch::B`] is a byte that is *not* part of
/// any valid UTF-8 sequence, and `\n` always is — so [`syn`] sees every one.
fn frag_line(chs: &[Ch], at: usize, line: u32) -> u32 {
    let skipped = chs
        .get(..at)
        .unwrap_or_default()
        .iter()
        .filter(|&&c| syn(c) == '\n')
        .count();
    line.saturating_add(u32::try_from(skipped).unwrap_or(u32::MAX))
}

/// The line to parse text that was *built at runtime* against — the quoted form
/// `${x@Q}` produces, a `PS4` after expansion, the value an indirection
/// resolved to. Such text came from no script line, and bash reports nothing
/// about it, so leaving the fragment's own numbering alone is the honest answer.
const RUNTIME_TEXT: u32 = 1;

/// Renumber a freshly-lexed `${ … }` fragment's segments from the fragment's own
/// line 1 to the physical line it starts on.
///
/// This is the whole of the coordinate fix, and it is deliberately applied to
/// the *segments* rather than to the error that comes back: a segment carries
/// the line a nested body is numbered against (a `$( … )`'s closing delimiter,
/// say), so mapping here fixes both the parse error a bad body raises and the
/// line a well-formed one reports when it fails at *run* time — the
/// `command substitution: line N` of a backtick in `${x:-`/`fi`/`}`.
fn map_frag_segs(segs: &mut [Seg], line: u32) {
    map_segs(segs, &LineMap::Offset(line.saturating_sub(1)));
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
    /// The diagnostic, one entry per message bash would emit — one
    /// `parser_error`/`report_syntax_error` call each, and one `<name>: line N:`
    /// prefix each. Almost always a single entry; see [`ParseError::under`] for
    /// where a second comes from.
    ///
    /// An entry is *not* a physical line: a message may itself span several,
    /// because it quotes back text the user wrote over several. The header of a
    /// malformed `for (( … ))` is the case that shows it — bash prints one
    /// message with the newline still in it, prefixed only at its start:
    ///
    /// ```text
    /// line 1: syntax error: arithmetic expression required
    /// line 1: syntax error: `((0;
    /// 0))'
    /// ```
    ///
    /// Each entry is bytes. Most are fixed text, but the ones that name the
    /// offending construct quote a *shell word* back — the token in `syntax
    /// error near unexpected token \`…'`, or the name in `\`…': not a valid
    /// identifier` — and a shell word may hold any byte. The word therefore
    /// goes back out as the bytes the user wrote rather than through a decode
    /// that would rewrite the very text being blamed.
    pub msgs: Vec<Str>,
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
    /// The entries of `msgs` that are *not* reported at `line`, as
    /// `(index into `msgs`, the line to report it at)`.
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
    /// True when this diagnostic is a *sequel* to the lexer's rather than a
    /// replacement for it: both are printed, this one second.
    ///
    /// bash's `read_token_word` does not abort the parse when a matched-pair
    /// scan runs off the end of the input. It prints its own message and bails
    /// *to the parser* — `return -1; /* Bail immediately. */` (parse.y:5151) —
    /// so the parser is handed a token that is not a `WORD` and objects to it in
    /// turn, from wherever it was. `[[ x =~ ( ]]` is the shape that shows it:
    ///
    /// ```text
    /// line 2: unexpected EOF while looking for matching `)'
    /// line 3: unexpected argument to conditional binary operator
    /// ```
    ///
    /// The second line is `cond_term`'s `else` branch (parse.y:4780), bare
    /// because `error_token_from_token(-1)` can name no token; it is reported at
    /// `line_number`, which the failed scan has by then run to one past the last
    /// line of the input.
    ///
    /// [`IncrementalParser::next_unit`] reads this to know it must print the
    /// parked lexer error and *then* this one, instead of letting the parked one
    /// replace it as it does for every other error a truncated stream provokes.
    pub bail_sequel: bool,
}

impl ParseError {
    /// A parse error with no known line (the common construction site inside
    /// the grammar; [`parse_tokens`] stamps the line afterwards).
    pub fn new(msg: &(impl bytes::PushBytes + ?Sized)) -> Self {
        Self {
            msgs: vec![bfmt![msg]],
            line: None,
            fatal: false,
            recoverable: false,
            line_at: Vec::new(),
            echo: None,
            bail_sequel: false,
        }
    }

    /// This diagnostic printed *under* `first`, as one error rather than two.
    ///
    /// bash has no notion of a compound diagnostic — it simply calls
    /// `parser_error` twice — but osh carries an error to one place that prints
    /// it, so the two messages have to travel joined. `first`'s line and echo
    /// lead, it being the one reported first; this error's own `line_at` entries
    /// shift past `first`'s messages so they keep naming the same text.
    #[must_use]
    fn under(self, first: Self) -> Self {
        let lead = u32::try_from(first.msgs.len()).unwrap_or(u32::MAX);
        let mut line_at = first.line_at;
        line_at.extend(self.line_at.iter().map(|&(i, l)| (i.saturating_add(lead), l)));
        let mut msgs = first.msgs;
        msgs.extend(self.msgs);
        Self {
            msgs,
            line: first.line,
            fatal: first.fatal,
            recoverable: first.recoverable,
            line_at,
            echo: first.echo,
            bail_sequel: false,
        }
    }

    /// The whole diagnostic as one byte string, its messages newline-joined —
    /// for the callers that only want to look at or print the text, and do not
    /// care where the prefixes go.
    #[must_use]
    pub fn msg(&self) -> Str {
        self.msgs.join(&b'\n')
    }

    /// Where the `i`th message is reported, given the error's own line.
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
    ///
    /// Unless the body simply *ran out*. bash decides this on `EOF_Reached`, and
    /// only the other branch ends the shell (parse.y:4169-4194):
    ///
    /// ```c
    ///   if (EOF_Reached)
    ///     {
    ///       ...
    ///       /* yyparse() has already called yyerror() and reset_parser() */
    ///       parser_state |= PST_NOERROR;
    ///       return (&matched_pair_error);
    ///     }
    ///   else if (r != 0)
    ///     {
    ///       /* Non-interactive shells exit on parse error in a command substitution. */
    ///       ...
    ///       if (interactive_shell == 0)
    ///         jump_to_top_level (FORCE_EOF);  /* This is like reader_loop() */
    /// ```
    ///
    /// `&matched_pair_error` is what the *enclosing* reader gets back from any
    /// pair it left open, so a body that ends inside one costs the enclosing
    /// command and nothing more: `echo $(f[1)` is status 2 and the script goes
    /// on, exactly as a bare `f[1` is. A body whose *grammar* is wrong —
    /// `echo $(fi)` — ends the shell instead.
    ///
    /// [`Self::is_incomplete`] is that same distinction read off the message,
    /// which is sound here because bash's two end-of-input diagnostics are
    /// precisely the ones `EOF_Reached` accompanies. Note the normalisation in
    /// [`parse_paren_body_mapped`] runs first: a body that ends mid-construct
    /// with the `)` still in the stream is renamed to an error *on* that `)`,
    /// and so is fatal — which is what bash does with it too, the `)` being a
    /// token `yyparse` really did get.
    fn in_paren_body(mut self) -> Self {
        self.fatal = self.fatal || !self.is_incomplete();
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
        self.msgs.iter().any(|m| {
            bytes::contains(m, b"unexpected end of file")
                || bytes::contains(m, b"unexpected EOF while looking for")
        })
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
            msgs: vec![e.msg],
            line: e.line,
            fatal: false,
            recoverable: e.recoverable,
            line_at: Vec::new(),
            echo: None,
            bail_sequel: false,
        }
    }
}

/// [`ParseError::from`], after giving the substitution bodies the scan read but
/// did not parse their say — the ones it read whole inside a word that then ran
/// out ([`crate::lexer::LexError::eager_bodies`]), and the one it never found
/// the `)` of ([`crate::lexer::SubstBail`]).
///
/// bash parses that body *as it reads it*, so an error in it comes out before
/// the missing paren is ever noticed — ``echo $(fi`` is ``syntax error near
/// unexpected token `fi'``, not ``unexpected EOF while looking for matching
/// `)'``. osh scans first and parses second, so the body arrives here unread
/// and is read now.
///
/// The body is parsed *plainly*, not through [`parse_cmdsub_body`]: that one
/// appends the substitution's `)` where the body's trailing newline would go,
/// because that is the token bash's parser sees next — but here there is no
/// `)`, and blaming one would turn `echo $(a |` into ``near `)'`` where bash
/// says the body simply ran out. A body error that *is* an end-of-input error
/// means exactly that, and is bash's `EOF_Reached` path (parse.y:4170), on
/// which the `)` message stands.
fn resolve_subst_bail(e: crate::lexer::LexError, opts: ParseOpts) -> ParseError {
    // A word that ran out of input reports the bodies it read on the way there
    // first: they sit earlier in the text than whatever the scan finally ran
    // out inside, and bash reports the first failure it meets. See
    // [`crate::lexer::LexError::eager_bodies`].
    if let Some(subs) = e.eager_bodies.as_deref()
        && let Some(err) = eager_body_error(subs, opts)
    {
        return err;
    }
    let Some(bail) = e.bail.clone() else { return ParseError::from(e) };
    bail_body_error(&bail, opts).unwrap_or_else(|| ParseError::from(e))
}

/// The error bash's parse of the substitution bodies inside an unfinished word
/// would raise, or `None` if all of them parse (or merely run out of input
/// themselves).
///
/// The bodies are in reading order and the first failure wins, which is bash
/// reading left to right: `echo " $(fi) $(done)` names `fi`.
///
/// An end-of-input error is passed over for the same reason [`bail_body_error`]
/// passes over one — a body that merely ran out did not *say* anything, and
/// what the word itself gave up on is then the thing to report.
fn eager_body_error(subs: &[crate::lexer::EagerBody], opts: ParseOpts) -> Option<ParseError> {
    subs.iter().find_map(|b| {
        let parsed = if b.procsub {
            parse_procsub_body(&b.src, b.line, opts)
        } else {
            parse_cmdsub_body(&b.src, b.line, opts)
        };
        parsed.err().filter(|e| !e.is_incomplete())
    })
}

/// The error bash's nested parse of an unterminated substitution body would
/// raise, or `None` if that parse would simply have run out of input.
///
/// The body is read left to right, so the two halves are tried in that order:
/// the tokens that *did* lex are parsed first, and only if they get to the end
/// without complaining does the substitution the body itself ran out inside get
/// its turn. `echo $(fi; $(done` is ``near `fi'`` and not ``near `done'``, for
/// the same reason `echo $(fi) $(done` is.
///
/// The recursion terminates because a nested bail's body begins past its own
/// `(`, so each step is a strictly shorter suffix of the input.
fn bail_body_error(bail: &crate::lexer::SubstBail, opts: ParseOpts) -> Option<ParseError> {
    let base = bail.open_line.saturating_sub(1);
    let Tokenized { toks: mut t, lines: mut l, ends, err, dparens, .. } =
        crate::lexer::tokenize_deferred(&bail.body, opts);
    // The body's line 1 is the line its `(` sits on, the two being on the same
    // physical line by construction.
    map_lines(&mut t, &mut l, &LineMap::Offset(base));
    if let Err(inner) = parse_tokens(t, l, Spans::of(&bail.body, ends, &dparens), opts) {
        // An end-of-input error means the body merely ran out, which is bash's
        // `EOF_Reached` path (parse.y:4170) — the one on which the missing `)`
        // is what gets reported. Blaming a token here would also have to invent
        // one: [`parse_cmdsub_body`] can say ``near `)'`` because the `)` really
        // is the next token, and here there is no `)` at all.
        if !inner.is_incomplete() {
            return Some(inner.in_paren_body());
        }
    }
    let mut nested = err?.0.bail?;
    nested.open_line = nested.open_line.saturating_add(base);
    bail_body_error(&nested, opts)
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
    let Spanned { toks, lines, ends, dparens, .. } =
        tokenize_spanned(&src, opts).map_err(|e| resolve_subst_bail(e, opts))?;
    parse_tokens(toks, lines, Spans::of(&src, ends, &dparens), opts)
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
/// is shifted back into the enclosing source's. The shift is from the *opening*
/// delimiter, unlike [`parse_cmdsub_body`]'s — a process substitution runs as a
/// child command, not as a body bash re-reads after the enclosing scan.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error in the body.
pub fn parse_procsub_body(
    src: BStr<'_>,
    open_line: u32,
    opts: ParseOpts,
) -> Result<Program, ParseError> {
    let phys = LineMap::Offset(open_line.saturating_sub(1));
    let Spanned { mut toks, mut lines, ends, dparens, .. } =
        tokenize_paren_body(src, opts).map_err(|e| paren_body_lex_error(e, &phys))?;
    map_lines(&mut toks, &mut lines, &phys);
    parse_tokens_ending(toks, lines, Spans::of(src, ends, &dparens), opts, true)
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
    let Spanned { toks, lines, ends, dparens, .. } =
        crate::lexer::tokenize_spanned_strict(src, opts).map_err(|e| resolve_subst_bail(e, opts))?;
    parse_tokens(toks, lines, Spans::of(src, ends, &dparens), opts)
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
    let Spanned { toks, lines, starts, ends, dparens, .. } =
        tokenize_spanned(src, opts).map_err(|e| resolve_subst_bail(e, opts))?;
    // An alias splices in tokens that were never written where they are being
    // read — but they *were* written somewhere, in the alias's own value, which
    // bash pushes onto the input and reports errors against. So the expansion
    // carries its replacement texts along and they become further sources.
    let (toks, lines, spans) = if aliases.is_empty() {
        (toks, lines, Spans::of(src, ends, &dparens))
    } else {
        let chars: Vec<_> = bytes::chars(src).collect();
        let x = expand_aliases_tracked(&toks, &lines, &starts, &ends, &chars, aliases, opts);
        let spans = Spans::expanded(chars, &x, &dparens);
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

/// Which of bash's two input readers a piece of shell source arrived through.
///
/// The distinction is `bash_input.type` (parse.y), and it is observable at
/// exactly one place: how the reader closes a last line that has no newline of
/// its own. See [`close_last_line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// `st_stream` — the script file named on the command line, or stdin (the
    /// REPL, a pipe, `-s`). bash's `reader_loop` reads these.
    Stream,
    /// `st_string` — everything `parse_and_execute` runs: the `-c` command
    /// string, an `eval` argument, a `.`/`source`d file (read whole, then parsed
    /// as a string), a trap action, a `mapfile -C` callback, an `fc` replay.
    Str,
}

/// Close the last line of an input the way bash's reader does.
///
/// `shell_getc` (parse.y) reads one physical line at a time and, before any
/// scanner sees it, writes back the newline the line did not come with
/// (parse.y:2567):
///
/// ```c
///   /* Add the newline to the end of this string, iff the string does
///      not already end in an EOF character.  */
///   if (shell_input_line_terminator != EOF)
///     {
///       /* Don't add a newline to a string that ends with a backslash if we're
///          going to be removing quoted newlines, since that will eat the
///          backslash.  Add another backslash instead (will be removed by
///          word expansion). */
///       if (bash_input.type == st_string && expanding_alias() == 0 &&
///           last_was_backslash && c == EOF && remove_quoted_newline)
///         shell_input_line[shell_input_line_len] = '\\';
///       else
///         shell_input_line[shell_input_line_len] = '\n';
///     }
/// ```
///
/// So a file that stops mid-line is read as though it ended in a newline — and
/// a final `\` in it is therefore a line continuation, joining it to an input
/// that is not there. `printf 'echo two\' > f; bash f` prints `two`.
///
/// A *string* is the exception, and only when its last backslash is unescaped
/// (`last_was_backslash` is the parity of the trailing run, being reset by every
/// other character). Closing it with a newline would make that backslash a
/// continuation too and eat it, so bash closes it with a second backslash
/// instead: the pair is one quoted, literal backslash. `bash -c 'echo two\'`
/// prints `two\`, and `bash -c 'esac\'` sees the *word* `esac\` rather than the
/// reserved word `esac`.
///
/// The two rules are applied in that order — a stream input is newline-closed
/// first, which leaves nothing for the string rule to do — so every input the
/// rest of the shell parses ends in a newline or in an escaped backslash.
#[must_use]
pub fn close_last_line(src: BStr<'_>, reader: InputKind) -> std::borrow::Cow<'_, [u8]> {
    if src.is_empty() || src.last() == Some(&b'\n') {
        return std::borrow::Cow::Borrowed(src);
    }
    // `last_was_backslash` toggles on `\` and is cleared by anything else, so
    // what it holds at end of input is the parity of the trailing run.
    let odd_backslash = src.iter().rev().take_while(|&&b| b == b'\\').count() % 2 == 1;
    let mut owned = src.to_vec();
    owned.push(if reader == InputKind::Str && odd_backslash { b'\\' } else { b'\n' });
    std::borrow::Cow::Owned(owned)
}

/// Whether `t` is an input [`close_last_line`] closed with a *backslash* rather
/// than with a newline — the one shape in which the shell parses text that ends
/// on no newline at all.
///
/// The signature is exact: the close appends a `\` to an odd trailing run,
/// making the run even, and it appends nothing else without also appending a
/// newline. So "ends in a non-empty even run of backslashes, and not in a
/// newline" names that case and no other. An input the user really did write
/// with two trailing backslashes is newline-closed like anything else — its run
/// was even, so the string rule did not apply.
///
/// It matters because such an input has no newline for a reader to stop on. The
/// look past its last token runs the buffer out and enters `shell_getc`'s fetch
/// block — `line_number++` (parse.y:2361) — where a newline-closed input finds
/// the newline the reader wrote back and pays nothing. And bash has no newline
/// *token* left to hand the grammar either, so what ends the parse is the
/// end-of-file token: `bash -c 'case x in  \'` is an unexpected *end of file*
/// where `bash -c 'case x in  '` is near an unexpected `newline`.
pub(crate) fn closed_with_backslash(t: &[Ch]) -> bool {
    let run = t.iter().rev().take_while(|&&c| c == Ch::U('\\')).count();
    run > 0 && run % 2 == 0
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
    /// Spans of `src` the last expansion's lex read as here-document bodies —
    /// see [`AliasExpansion::taken`]. Empty when no expansion ran.
    work_taken: Vec<(u32, u32)>,
    /// What the last expansion's lex warned about while reading those bodies,
    /// each keyed to the offset in `src` where its body began — see
    /// [`AliasExpansion::warnings`].
    ///
    /// Kept apart from `pending_warnings` for two reasons. Every rebuild re-reads
    /// the same text and so re-raises the same warnings, and a rebuild that
    /// happens after an `alias` has been redefined raises *different* ones, so
    /// this vector is **replaced** by each rebuild rather than added to — which
    /// is what keeps a speculative gather's complaint out of the output. And
    /// unreading a body truncates `orig`, so a token index into it would say
    /// nothing about how far the parse had got.
    alias_warnings: Vec<(u32, ReaderWarning)>,
    /// Whether an expansion has taken lines out of the still-unparsed tail as a
    /// here-document body since the last rebuild, so the next one has to undo it
    /// first — the alias table may have changed under it. See [`Self::rebuild`].
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
    /// The lowest line an end-of-input diagnostic may name, in *mapped* lines.
    /// See [`crate::lexer::Tokenized::dparen_eof_floor`]. Only ever raised — a
    /// re-lex of the tail cannot un-buy a request an earlier `((` already made.
    dparen_eof_floor: u32,
    /// Every `((` handed back to the ordinary grammar as `( ( … ) )`, in push
    /// order, as character offsets into `src`. See [`DparenCopy`]. Kept beside
    /// `orig_conts` and rebased the same way, since like a continuation a copy is
    /// a fact about text that was *read*, which a re-lex of the tail cannot undo
    /// for the head.
    orig_dparens: Vec<DparenCopy>,
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
    /// and the shell's own line less one for a substitution body re-read at
    /// expansion time.
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
            dparen_eof_floor,
            dparens,
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
            work_taken: Vec::new(),
            alias_warnings: Vec::new(),
            alias_gathered: false,
            work_spans: Spans::default(),
            wpos: 0,
            pos: 0,
            last_aliases: None,
            pending_lex_err: err
                .map(|(e, line)| resolve_subst_bail(e, opts).or_line(line))
                .map(|e| ParseError {
                    line: e.line.map(|l| line_map.map(l)),
                    ..e
                }),
            dparen_eof_floor: map_floor(dparen_eof_floor, &line_map),
            orig_dparens: dparens,
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
            dparen_eof_floor,
            dparens,
        } = tokenize_deferred(&tail, opts);
        map_lines(&mut orig, &mut orig_lines, &map);
        self.dparen_eof_floor = self.dparen_eof_floor.max(map_floor(dparen_eof_floor, &map));
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
        self.rebase_dparens(off, delta, &dparens);
        self.pos = 0;
        self.opts = opts;
        self.pending_lex_err = err
            .map(|(e, line)| resolve_subst_bail(e, opts).or_line(line))
            .map(|e| ParseError {
                line: e.line.map(|l| map.map(l)),
                ..e
            });
        // `work` was expanded from the tokens just discarded, so it must be
        // rebuilt whatever the alias state.
        self.last_aliases = None;
    }

    /// Re-expand the unconsumed remainder of the original token stream under
    /// `aliases`, and unread whatever the expansion's own lex then took as a
    /// here-document body.
    ///
    /// The expansion's lex reads the assembled text as bash's reader would, so a
    /// `<<` a replacement brought with it collects its body there and then — but
    /// that body is real input, which *this* parser has already lexed as
    /// commands. Those tokens are not tokens after all, so the tail is re-lexed
    /// from the body's end and the expansion redone over what is left. Each pass
    /// settles one body, so the loop is bounded by their number; the
    /// already-settled prefix of `orig` never changes, which is what makes the
    /// bodies settled by an earlier pass still land on the right tokens after a
    /// later one. See [`AliasExpansion::taken`].
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
        // Every pass settles one body, so the next looks only past it; without
        // that the same span would be found again after its own re-lex whenever
        // the tail carries a genuine lexer error of its own.
        let mut settled = 0usize;
        loop {
            self.rebuild_once(aliases);
            let Some((body, end)) = self.unread_body(settled) else { return };
            // The line that introduced the body is already expanded and about to
            // be parsed, so it stays exactly as it is; its raw text now runs to
            // the end of the body, as it would for a `<<` the lexer gathered.
            let keep = self.orig_offsets.partition_point(|&o| (o as usize) < body);
            let end32 = u32::try_from(end).unwrap_or(u32::MAX);
            if let Some(slot) = keep.checked_sub(1).and_then(|i| self.orig_ends.get_mut(i)) {
                *slot = (*slot).max(end32);
            }
            self.relex_tail_from(keep, end);
            self.alias_gathered = true;
            settled = end;
        }
    }

    /// The first span at or past `settled` that this expansion's lex ate as a
    /// here-document body while the input's own lex read it as something else.
    ///
    /// "Something else" is either commands — `orig` still holds unconsumed
    /// tokens inside the span — or nothing at all, the lex having given up
    /// there on an unclosed construct: text that is a body cannot be either,
    /// and both readings have to be withdrawn. See [`AliasExpansion::taken`].
    fn unread_body(&self, settled: usize) -> Option<(usize, usize)> {
        let rest = self.orig_offsets.get(self.pos..).unwrap_or(&[]);
        self.work_taken
            .iter()
            .map(|&(a, b)| (a as usize, b as usize))
            .filter(|&(a, _)| a >= settled)
            .find(|&(a, b)| {
                rest.iter().any(|&o| (o as usize) >= a && (o as usize) < b)
                    || (self.pending_lex_err.is_some()
                        && self.orig_offsets.last().is_none_or(|&o| (o as usize) < b))
            })
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
        let rest_starts = self.orig_offsets.get(self.pos..).unwrap_or(&[]);
        let mut taken = Vec::new();
        let mut warnings = Vec::new();
        let mut spans = match aliases {
            Some(map) if !map.is_empty() => {
                let x = expand_aliases_tracked(
                    rest,
                    rest_lines,
                    rest_starts,
                    rest_ends,
                    &self.src,
                    map,
                    self.opts,
                );
                let spans = Spans::expanded(self.src.clone(), &x, &self.orig_dparens);
                taken = x.taken;
                warnings = x.warnings;
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
                let mut spans = Spans {
                    srcs: vec![self.src.clone()],
                    parents: Vec::new(),
                    ends: rest_ends.iter().map(|&end| TokSpan { src: 0, end }).collect(),
                    dparen_bases: Vec::new(),
                };
                spans.push_dparens(&self.orig_dparens);
                spans
            }
        };
        self.work_taken = taken;
        self.alias_warnings = warnings;
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
            dparen_eof_floor,
            dparens,
        } = tokenize_deferred(&tail, self.opts);
        map_lines(&mut orig, &mut orig_lines, &map);
        self.dparen_eof_floor = self.dparen_eof_floor.max(map_floor(dparen_eof_floor, &map));
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
        self.rebase_dparens(off, delta, &dparens);
        self.pending_lex_err = err
            .map(|(e, line)| resolve_subst_bail(e, self.opts).or_line(line))
            .map(|e| ParseError { line: e.line.map(|l| map.map(l)), ..e });
        self.last_aliases = None;
    }

    /// Replace the record of the `((`s pushed back past `off` with the re-lex's,
    /// keeping the head's — see [`Self::orig_dparens`]. `delta` is `off` as an
    /// offset, which the tail's own offsets restart before.
    fn rebase_dparens(&mut self, off: usize, delta: u32, dparens: &[DparenCopy]) {
        self.orig_dparens.retain(|c| (c.start as usize) < off);
        self.orig_dparens.extend(dparens.iter().map(|c| DparenCopy {
            start: c.start.saturating_add(delta),
            end: c.end.saturating_add(delta),
        }));
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
                    // An alias-spliced `<<` contributes nothing: it has no `orig`
                    // slot to look a count up in, and needs none. Its body was
                    // read by the *expansion's* lex, which moved that lex's own
                    // reader — so every token the expansion emitted afterwards is
                    // already stamped with the line the gather left bash on. See
                    // [`AliasExpansion::taken`].
                    let lines = match self.work_origin.get(i) {
                        Some(&Some(oi)) => self.orig_heredoc_lines.get(oi).copied().unwrap_or(0),
                        _ => 0,
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

    /// The line bash's reader stands on once it has read the whole input: one
    /// past its last.
    ///
    /// `line_number` counts the lines `shell_getc` has handed over, and a scan
    /// that runs off the end has handed over every one of them — so a diagnostic
    /// printed after such a scan (see [`ParseError::bail_sequel`]) names a line
    /// that does not exist. The input's *last* line is a line even without a
    /// newline of its own: the reader ends it at EOF just as it would at `\n`,
    /// which is why a one-line file with no trailing newline still reports 2.
    fn line_past_end(&self) -> u32 {
        let nl = self.src.iter().filter(|&&c| c == '\n').count();
        let last = u32::try_from(nl)
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(self.src.last().is_some_and(|&c| c != '\n')));
        self.line_map.map(last.max(1)).saturating_add(1)
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
            truncated_at: self.pending_lex_err.is_some().then(|| self.line_past_end()),
            eof_closed_in_list: false,
            eof_floor: self.dparen_eof_floor,
        };
        let mut items = Vec::new();
        // Whether the parse ended by asking for a token the stream did not have.
        // bash's parser pulls tokens one at a time, so an unclosed construct is
        // only *discovered* at the fetch that runs into it: this is the flag that
        // says the fetch happened, and so that a parked lexer error is what this
        // unit really came to. Running dry is not the same as merely stopping at
        // the last token — a unit ended by a newline that happens to be final has
        // fetched nothing beyond it, and is a complete unit that runs.
        let mut ran_dry = false;
        let outcome = loop {
            match p.parse_item(&[]) {
                // End of the token stream. Anything left over is a token no item
                // can start with (a stray `)`), reported as `parse_tokens` does.
                Ok(None) if p.pos == p.toks.len() => {
                    ran_dry = true;
                    break Ok(());
                }
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
                        _ if p.pos == p.toks.len() => {
                            ran_dry = true;
                            break Ok(());
                        }
                        _ => break Err(p.unexpected_here()),
                    }
                }
                // An error the *shape* of running out is one: the parser asked
                // for the token that would close the construct and there was
                // none. So is one raised *because* the stream was cut short and
                // meant to print under the lexer's own message. Any other is a
                // real objection to a token the parser did get — an eagerly-parsed
                // `$( … )` body's own error, say — raised before the fetch that
                // would have found the lexer's.
                Err(e) => {
                    ran_dry = e.is_incomplete() || e.bail_sequel;
                    break Err(e);
                }
            }
        };
        // A grammar error leaves the cursor on the offending token; stamp its
        // line exactly as `parse_tokens` does — including the end-of-file case's
        // extra line, which keys off the message rather than the cursor (an
        // error can name a token from outside this stream).
        let outcome = outcome.map_err(|e| {
            // A here-document still pending at a *top-level* reduction is
            // gathered before the offending token is ever found to be one, and
            // gathering moves bash's reader. See [`Self::heredoc_gather`].
            // Only for an error being stamped here: one that already names a
            // line was raised in a stream of its own (a `$( … )` body), where
            // the reduction never happens.
            let gathered = if e.line.is_none() { self.heredoc_gather(&p) } else { 0 };
            let at = if e.is_incomplete() { p.toks.len() } else { p.pos };
            let line = p.reader_line_at(at).saturating_add(gathered);
            // `read_a_line` (parse.y:2080) reads a here-document body into a
            // buffer of its own and never replaces `shell_input_line`, so the
            // gather moved the *number* and not the text: bash prints the line
            // the error was written on under a prefix naming a later one.
            let pinned = (gathered > 0).then(|| self.tok_line_text(&p)).flatten();
            // An error that already names a line was raised in a stream of its
            // own — a `$( … )` body — which bash parses while still *scanning*,
            // before a `((` copy could have been pushed. See
            // [`Spans::echo_line_at_scan`].
            let echo = if e.line.is_some() { p.reader_echo_at_scan() } else { p.reader_echo() };
            let echo = echo.or(pinned);
            e.or_line(line).or_echo(echo)
        });
        self.wpos = p.pos;
        self.work = p.toks;
        self.work_lines = p.lines;
        self.work_spans = p.spans;
        // Whether this call is about to end the input by reporting the parked
        // lexer error, which it does exactly when the parse ran dry: the fetch
        // that found nothing is the fetch that would have found the error.
        // Sampled before the arm that consumes it.
        let lex_err_now = self.pending_lex_err.is_some() && ran_dry;
        // So a parked error replaces whatever this unit came to — a grammar
        // error the truncated stream provoked (`if true; then` + `echo 'unterm`
        // leaves a `then` with no body), a clean parse of the commands in front
        // of it (`echo one; echo 'unterm` runs neither), or the end of the input
        // itself. What survives is an error raised over a token the parser did
        // get, which bash's would have reached before ever asking for the next:
        // `) echo "` reports the stray `)`.
        //
        // The exception is an error that only exists *because* the stream was cut
        // short — bash's scan bails to the parser rather than aborting the parse,
        // so the parser objects in its turn and both are printed. That one joins
        // the parked error instead of being replaced by it.
        let outcome = match self.pending_lex_err.take_if(|_| ran_dry) {
            Some(lex) => Err(match outcome {
                Err(e) if e.bail_sequel => e.under(lex),
                _ => lex,
            }),
            None => outcome,
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
        // The expansion's own warnings go out under the same rule, but keyed by
        // *offset* rather than by token: the re-lex that unreads a body truncates
        // `orig`, so an index into it says nothing about how far the parse has
        // got. The offset is where the body *began*. The unit that swallowed it
        // was stretched to cover it, so the body's start is at or before that
        // unit's end and past the end of every unit before it — which is what
        // holds the warning back until the line its `<<` stands on has run.
        if !self.alias_warnings.is_empty() {
            let end = if lex_err_now {
                u32::MAX
            } else {
                next_orig.checked_sub(1).and_then(|i| self.orig_ends.get(i)).copied().unwrap_or(0)
            };
            let mut keep = Vec::new();
            for (off, h) in std::mem::take(&mut self.alias_warnings) {
                if off <= end {
                    self.ready_warnings.push(h);
                } else {
                    keep.push((off, h));
                }
            }
            self.alias_warnings = keep;
        }
        self.unit_lines.clear();
        self.unit_raw.clear();
        match outcome {
            // End of input, every unit handed out.
            Ok(()) if items.is_empty() => None,
            // The lexer stopped early on an unclosed construct, and this is the
            // point where bash — having executed every complete line before it —
            // reports it. Whatever this unit had parsed goes with it: the line
            // carrying the construct never runs.
            Err(e) if lex_err_now => {
                // Everything the reader read looking for the close belongs to
                // this unit, however many lines that was: bash echoes each one
                // as `shell_getc` hands it over, so `echo one` / `echo "unterm`
                // / `echo three` echoes the last *two* lines before reporting
                // the quote. The scan gave up mid-token, so the span is taken
                // from the input directly rather than from where it stopped.
                self.split_unit_error_lines(self.orig.len(), self.orig.len());
                self.wpos = self.work.len();
                self.pos = self.orig.len();
                Some(Err(e))
            }
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
                self.split_unit_error_lines(resume, next_orig);
                self.pos = resume;
                Some(Err(e))
            }
            Err(e) => {
                // bash has already *read* the line it could not parse, so it is
                // in the history before the diagnostic is printed — and, for the
                // same reason, echoed by `set -v`. See
                // [`Self::split_unit_error_lines`].
                self.split_unit_error_lines(next_orig, next_orig);
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

    /// The source line the reader stopped on for the unit [`Self::next_unit`]
    /// last returned — the line of the last token it consumed.
    ///
    /// This is where bash's shared `line_number` (`parse.y:1749`) stands once a
    /// unit has parsed and before it runs, which is the value every *later*
    /// unit's line numbers are measured from. A caller that has to reproduce
    /// bash's post-abort line drift needs it; see
    /// `TD-OILS-A-DISCARD-OUT-OF-A-COMPOUND-COMMAND-LOSES-BASH-A-LINE`.
    /// Meaningful only just after a successful `next_unit`; `0` before the
    /// first one.
    #[must_use]
    pub fn last_unit_end_line(&self) -> u32 {
        self.pos.checked_sub(1).and_then(|i| self.orig_lines.get(i).copied()).unwrap_or(0)
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
        self.split_unit_span(end_orig, None);
    }

    /// [`Self::split_unit_lines`], but for a unit that ends in an *error*: the
    /// span is stretched to the end of the physical line `blame` stands on.
    ///
    /// bash's reader hands a whole physical line to the parser and echoes it
    /// (`set -v`) at that moment, before a single token is taken off it — so by
    /// the time yacc looks at a token and finds it cannot be shifted, the line
    /// that token sits on has already been read and echoed. Ending the span at
    /// the last *successfully consumed* token instead cuts the offender off:
    ///
    /// ```text
    /// set -v; echo a; ) bad        bash echoes `echo a; ) bad'   osh echoed `echo a;'
    /// set -v; { echo a \n ) bad    bash echoes both lines        osh echoed the first
    /// ```
    ///
    /// The second shape is why this takes the offending token rather than just
    /// running the existing end out to its line's end: the parse stopped on a
    /// `Newline`, so the old end was *already* at a line boundary — the reader
    /// had simply gone on to read the next line before failing on it.
    fn split_unit_error_lines(&mut self, end_orig: usize, blame: usize) {
        self.split_unit_span(end_orig, Some(blame));
    }

    fn split_unit_span(&mut self, end_orig: usize, blame: Option<usize>) {
        let start = self.hist_cursor;
        let mut end = end_orig
            .checked_sub(1)
            .map_or(0, |last| {
                self.orig_ends.get(last).map_or(self.src.len(), |&e| e as usize)
            })
            .min(self.src.len());
        if let Some(blame) = blame {
            // Past the newline ending the blamed token's line — or, when it is
            // the end of input that is blamed, past everything the reader read.
            let from = self
                .orig_offsets
                .get(blame)
                .map_or(self.src.len(), |&o| (o as usize).min(self.src.len()));
            let line_end = self
                .src
                .get(from..)
                .and_then(|rest| rest.iter().position(|&c| c == Ch::U('\n')))
                .map_or(self.src.len(), |i| from.saturating_add(i).saturating_add(1));
            end = end.max(line_end);
        }
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

/// Parse the body of a `$( … )` substitution — the *eager* read, the one that
/// happens in the enclosing token stream.
///
/// This parse is the one bash throws away. `parse_comsub` keeps only
/// `print_comsub`'s re-print of it and then calls `dispose_command`
/// (parse.y:4219–4233); the re-print is what the word carries, and what
/// `command_substitute` reads back at expansion time. So what this program is
/// *for* is narrow: to raise the syntax error the enclosing scan raises, to be
/// re-printed for `declare -f`, to answer the `$(< file)` peek — and to be the
/// text of the body, via [`crate::unparse::comsub_body`].
///
/// Its lines are therefore the enclosing source's, plainly: a syntax error the
/// enclosing scan raises names the body's true physical line. That is
/// `close_line` less the body's own newlines. (`close_line` comes from the lexer,
/// [`Seg::CmdSub`]'s second field.)
///
/// The lines the body reports when it *runs* are a different question, and they
/// are not this program's — they belong to the re-print, numbered from the line
/// the *shell* stands on at expansion time like any other body bash reads only
/// then. See [`CmdSubBody`].
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
) -> Result<Program, ParseError> {
    parse_cmdsub_body_unmarked(src, close_line, opts).map_err(ParseError::in_paren_body)
}

/// [`parse_cmdsub_body`] with the "fatal to whoever was reading this body" mark
/// left off, so [`ParseError::fatal`] still says where the failure came from: an
/// error that is *already* fatal was raised inside a substitution nested in
/// `src`, and one that is not is `src`'s own.
///
/// bash reports those two from different depths, so the distinction is not
/// cosmetic. A body's own grammar error is `parse_string`'s to answer — it
/// returns `-DISCARD` and the caller loses one parse unit — while a *nested*
/// `$( … )` that will not parse is raised by `parse_comsub` itself:
///
/// ```c
///       /* Non-interactive shells exit on parse error in a command substitution. */
///       if (interactive_shell == 0)
///         jump_to_top_level (FORCE_EOF);  /* This is like reader_loop() */
/// ```
/// (parse.y:4185)
///
/// — which ends the shell. See [`Shell::comsub_reparse_error`].
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error in the body.
pub fn parse_cmdsub_body_unmarked(
    src: BStr<'_>,
    close_line: u32,
    opts: ParseOpts,
) -> Result<Program, ParseError> {
    let phys = cmdsub_body_lines(src, close_line);
    // The body is lexed with the substitution's own `)` standing where the
    // implicit trailing newline would otherwise go, because that is the token
    // bash's parser sees after the body's last command. See
    // [`tokenize_paren_body`].
    let spanned = tokenize_paren_body(src, opts).map_err(|e| paren_body_lex_error(e, &phys))?;
    parse_paren_body_mapped(spanned, src, &phys, opts)
}

/// The renumbering that puts a `$( … )` body's lines back into the enclosing
/// source's.
///
/// The body's line 1 is the line `$(` sits on: the closing `)` is on the body's
/// last line, so stepping back over the body's newlines lands there.
fn cmdsub_body_lines(src: BStr<'_>, close_line: u32) -> LineMap {
    LineMap::Offset(close_line.saturating_sub(newlines(src)).saturating_sub(1))
}

/// A paren body's *lexing* error, renumbered and marked.
///
/// A body that lexes has its tokens renumbered by [`map_lines`] afterwards; one
/// that does not never reaches that, so the shift is applied to the error's own
/// line here. Without it a subscript left open inside a substitution — the one
/// reader error a body can raise once the enclosing scan has already found the
/// `)` — would name the body's line 1 rather than the line it is written on.
fn paren_body_lex_error(e: crate::lexer::LexError, phys: &LineMap) -> ParseError {
    let e = ParseError::from(e);
    ParseError { line: e.line.map(|l| phys.map(l)), ..e }.in_paren_body()
}

/// [`parse_cmdsub_body_unmarked`] from its tokens on, with the renumbering
/// spelled out, for the caller
/// that wants the body's *own* lines rather than the enclosing source's.
///
/// [`comsub_reprint_error`] is that caller: it never reports the error it gets
/// back — only its message — and it needs the line the failure was found on
/// counted from 1, because that is what tells it where bash's reader stopped.
/// See [`ComsubReprintError::stop_line`].
fn parse_paren_body_mapped(
    spanned: Spanned,
    src: BStr<'_>,
    phys: &LineMap,
    opts: ParseOpts,
) -> Result<Program, ParseError> {
    let Spanned { mut toks, mut lines, ends, dparens, .. } = spanned;
    map_lines(&mut toks, &mut lines, phys);
    let prog = parse_tokens_ending(toks, lines, Spans::of(src, ends, &dparens), opts, true)
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
            // Only this parse's own end of input, never a nested body's: a
            // nested [`parse_cmdsub_body`] has already rewritten that same
            // message, so an error arriving from one cannot still carry it.
            if e.msg() == b"syntax error: unexpected end of file" {
                ParseError::new("syntax error near unexpected token `)'")
            } else {
                e
            }
        })?;
    Ok(prog)
}

/// What bash's **second** read of a `$( … )` body finds wrong with the re-print
/// — or `None`, which is the answer for all but a handful of shapes.
///
/// `src` is the re-print and `tail` the rest of the stored word after the `)`.
/// bash hands `xparse_dolparen` (parse.y:4248) the whole of that — `string` is a
/// pointer *into* the word, not a copy of the body — and reads it with
/// `token_to_read = DOLPAREN`, so the grammar in force is
///
/// ```text
/// comsub: DOLPAREN compound_list ')'
/// ```
///
/// A re-print that reads back stops at that `)` and never looks at `tail`, which
/// is why `tail` is normally invisible. Only a re-print that does *not* is
/// affected by it — and then it is affected in one of three ways, which is what
/// [`ComsubReprintError`] distinguishes.
#[must_use]
pub fn comsub_reprint_error(
    src: BStr<'_>,
    tail: BStr<'_>,
    opts: ParseOpts,
) -> Option<ComsubReprintError> {
    match tokenize_paren_body(src, opts) {
        // The body lexed, so the `)` really was the token after its last
        // command and `tail` is out of reach: this is the ordinary re-parse,
        // and any error is the body's own grammar error, named on a token.
        Ok(spanned) => {
            // Renumbered from 1, not from the enclosing source: the line wanted
            // here is the one *within* the text the read was handed, which is
            // both the line bash reports and the line its reader stopped on.
            let err = parse_paren_body_mapped(spanned, src, &LineMap::default(), opts).err()?;
            let fatal = err.fatal;
            // A failure on the body's last line — which is where the `!`/`time`
            // shapes put it, at the `)` the re-print cannot reach — is the whole
            // of `src` consumed and then the composite line the `)` sits on. An
            // error further up stops there instead; see
            // [`ComsubReprintError::stop_line`].
            let line_off = err.line.unwrap_or_else(|| newlines(src).saturating_add(1));
            Some(ComsubReprintError { err, echo: true, line_off, stop_line: line_off, fatal })
        }
        // The body ran out with a construct still open, so bash's lexer did
        // *not* stop at the `)`: it swallowed it and read on into `tail`.
        Err(_) => Some(reprint_read_past_paren(src, tail, opts)),
    }
}

/// What bash's read of a `$( … )` body finds wrong when the text it was written
/// in ran out before the `)` did — always something, because the `)` the
/// `comsub` production needs can never arrive.
///
/// This is the same `xparse_dolparen` read [`comsub_reprint_error`] models, and
/// the same three shapes of failure; only the string differs. There is no `)`
/// to append and no `tail` to run into, because `extract_command_subst` handed
/// the read everything that was left. See [`crate::ast::CmdSubBody::Unread`].
#[must_use]
pub fn comsub_unclosed_error(src: BStr<'_>, opts: ParseOpts) -> ComsubReprintError {
    let spanned = match tokenize_spanned(src, opts) {
        Ok(spanned) => spanned,
        // A construct inside the body left open too, which `parse_matched_pair`
        // reports first and against its own opening line — the same diagnostic
        // and the same accounting as in [`reprint_read_past_paren`].
        Err(lex) => {
            let line_off = lex.line.unwrap_or(1);
            let err = resolve_subst_bail(lex, opts);
            let fatal = err.fatal;
            // The line named is the one the unclosed construct *opened* on, not
            // where the reader gave up — that ran the text out looking for the
            // closer. So the two numbers part company here.
            return ComsubReprintError { err, echo: fatal, line_off, stop_line: u32::MAX, fatal };
        }
    };
    let Spanned { toks, lines, ends, dparens, .. } = spanned;
    match parse_tokens(toks, lines, Spans::of(src, ends, &dparens), opts) {
        // A grammar error found before the input ran out is named on its token
        // and echoed, exactly as in a body that did close.
        Err(e) if !e.is_incomplete() => {
            let line_off = e.line.unwrap_or_else(|| eof_line_off(src));
            let fatal = e.fatal;
            ComsubReprintError { err: e, echo: true, line_off, stop_line: line_off, fatal }
        }
        // Otherwise the read simply ran out with `shell_eof_token` outstanding —
        // whether the text was a complete command (`$(echo`) or not (`$(if`),
        // since neither can produce the `)`. parse.y:6289, with
        // `shell_input_line` already empty, so nothing is echoed under it.
        _ => ComsubReprintError {
            err: ParseError::new("unexpected EOF while looking for matching `)'"),
            echo: false,
            line_off: eof_line_off(src),
            stop_line: u32::MAX,
            fatal: false,
        },
    }
}

/// Which line of `s` a reader that consumed all of it then met end of input on.
///
/// The reader counts a line as it *fetches* it (parse.y:2361), so reading the
/// last line and then finding no more costs one increment each — but text
/// ending in a newline has no partial line after it to fetch, and stops one
/// short.
fn eof_line_off(s: BStr<'_>) -> u32 {
    let lines = newlines(s).saturating_add(1);
    if s.last() == Some(&b'\n') { lines } else { lines.saturating_add(1) }
}

/// [`comsub_reprint_error`] for a re-print whose lex runs past the `)`.
fn reprint_read_past_paren(
    src: BStr<'_>,
    tail: BStr<'_>,
    opts: ParseOpts,
) -> ComsubReprintError {
    let combined = bfmt![src, b")", tail];
    let spanned = match tokenize_spanned(&combined, opts) {
        Ok(spanned) => spanned,
        // Still unterminated at the true end of input. This is
        // `parse_matched_pair`'s own diagnostic (parse.y:3711), a bare
        // `parser_error` that then sets `PST_NOERROR` "avoid redundant error
        // message" — so it is printed alone, with no offending line under it,
        // and it names `start_lineno`: the line the *innermost* unclosed
        // construct opened on, which is what [`crate::lexer::LexError::line`]
        // holds.
        Err(lex) => {
            let line_off = lex.line.unwrap_or(1);
            // `resolve_subst_bail` marks an error it found *inside* an
            // unterminated `$( … )` fatal and leaves the plain
            // unterminated-construct message alone, so the flag doubles as
            // "this one names a token" — and a diagnostic that names a token is
            // the one class bash echoes the offending line under
            // (parse.y:6251-6264).
            let err = resolve_subst_bail(lex, opts);
            let fatal = err.fatal;
            // As in [`comsub_unclosed_error`]: the opening line is named, but the
            // reader itself ran the text out.
            return ComsubReprintError { err, echo: fatal, line_off, stop_line: u32::MAX, fatal };
        }
    };
    let Spanned { toks, lines, ends, dparens, .. } = spanned;
    match parse_tokens(toks, lines, Spans::of(&combined, ends, &dparens), opts) {
        // A grammar error found before the input ran out: bash names the token
        // and echoes the line, from `report_syntax_error`'s first branch.
        Err(e) if !e.is_incomplete() => {
            let line_off = e.line.unwrap_or_else(|| newlines(&combined).saturating_add(1));
            let fatal = e.fatal;
            ComsubReprintError { err: e, echo: true, line_off, stop_line: line_off, fatal }
        }
        // The text either parsed or merely ran out — but either way the `)` the
        // `comsub` production needs was consumed by something else and never
        // arrived, so the parse ends at end of input with `shell_eof_token`
        // still outstanding:
        //
        // ```c
        //   if (EOF_Reached && shell_eof_token && current_token != shell_eof_token)
        //     parser_error (line_number, _("unexpected EOF while looking for matching `%c'"), shell_eof_token);
        // ```
        // (parse.y:6289 — reached with `shell_input_line` empty, so nothing is
        // echoed under it.)
        //
        // `line_number` has by then run one line past the text — see
        // [`eof_line_off`].
        _ => ComsubReprintError {
            err: ParseError::new("unexpected EOF while looking for matching `)'"),
            echo: false,
            line_off: eof_line_off(&combined),
            stop_line: u32::MAX,
            fatal: false,
        },
    }
}

/// How bash's second read of a `$( … )` re-print failed. See
/// [`comsub_reprint_error`].
pub struct ComsubReprintError {
    /// The diagnostic.
    pub err: ParseError,
    /// Whether bash echoes the offending input line under the message.
    ///
    /// Only the branch of `report_syntax_error` that names a token calls
    /// `print_offending_line` (parse.y:6262). An end-of-input diagnostic never
    /// does: `parse_matched_pair` prints its own and suppresses the parser's
    /// with `PST_NOERROR`, and the `shell_eof_token` message is raised from the
    /// branch reached only when `shell_input_line` is already empty.
    pub echo: bool,
    /// The reported line, counted from the line the enclosing command is being
    /// read on. `parse_string` does not renumber (it calls `push_stream (0)`),
    /// so bash's one `line_number` simply runs on from there.
    pub line_off: u32,
    /// Which line of the read's text bash's **reader** had consumed when it
    /// stopped, counted from 1 — or [`u32::MAX`] where it ran the text out.
    ///
    /// This is not the same question as [`Self::line_off`], though the two
    /// usually agree. bash reads a string one line at a time (`shell_getc` fills
    /// `shell_input_line`, `line_number++` counts it — parse.y:2361), so a
    /// diagnostic naming a *token* is reported at the line the reader is on and
    /// echoes that same line: both numbers are this one. A diagnostic naming
    /// something else is not — `parse_matched_pair` names the line its construct
    /// *opened* on while the reader is at the end of the text, and the
    /// `shell_eof_token` message is raised there too.
    ///
    /// It matters because it is the only thing that says how much of the string
    /// a failed read consumed. `xparse_dolparen` reads its own reader's position
    /// back out and hands the caller both the text to run and the index to
    /// resume from:
    ///
    /// ```c
    ///   if (ep[-1] != ')')
    ///     { while (ep > ostring && ep[-1] == '\n') ep--; }
    ///   nc = ep - ostring;
    ///   *indp = ep - base - 1;
    ///   ret = (nc == 0) ? "" : substring (ostring, 0, nc - 1);  /* parse.y:4348-4376 */
    /// ```
    ///
    /// So a body whose failure is not on its last line leaves the rest of the
    /// string to be expanded: measured, `a='A$(fi⏎echo x⏎)B'` under `@P` runs
    /// `f` and expands to `A⏎echo x⏎)B`. See
    /// [`crate::interp::Shell::failed_extent_split`].
    pub stop_line: u32,
    /// Whether the failure ends the shell rather than one parse unit — true
    /// only for a `$( … )` nested in the re-print, which `parse_comsub` answers
    /// with `jump_to_top_level (FORCE_EOF)` (parse.y:4185). The re-print's own
    /// failure is `parse_string`'s, and costs a `DISCARD`.
    pub fatal: bool,
}

/// What bash's tokenizer finds wrong with `src` when it is read as a bare *word
/// list* — `parse_string_to_word_list` (parse.y:6398), the read a compound
/// assignment's value list gets — or `None` when it lexes.
///
/// No grammar is consulted, because that reader consults none: it loops on
/// `read_token` and accepts anything that comes back a `WORD`. A listing built
/// from words bash already read can only fail by *lexing*, which it does when a
/// construct in it is left unclosed — the shape a NUL cut leaves behind. See
/// [`crate::ast::WordPart::TokenText`], and `Shell::array_assign_reparse_error`
/// for the caller.
///
/// [`ParseError::fatal`] distinguishes the two failures the way it does
/// everywhere: an error found *inside* an unterminated `$( … )` is
/// `parse_comsub`'s `jump_to_top_level (FORCE_EOF)`, while the listing's own
/// unclosed construct costs only the `DISCARD` `parse_string_to_word_list`
/// raises at parse.y:6480.
#[must_use]
pub fn word_list_lex_error(src: BStr<'_>, opts: ParseOpts) -> Option<ParseError> {
    tokenize_spanned(src, opts).err().map(|e| resolve_subst_bail(e, opts))
}

/// The number of newlines in `s` — how far a reader that consumed all of it has
/// advanced its line counter.
fn newlines(s: BStr<'_>) -> u32 {
    u32::try_from(s.iter().filter(|&&b| b == b'\n').count()).unwrap_or(u32::MAX)
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
/// Run an end-of-input floor ([`crate::lexer::Tokenized::dparen_eof_floor`])
/// through `map`, leaving the "no floor" sentinel 0 alone — it is not a line and
/// must not be renumbered into one.
fn map_floor(floor: u32, map: &LineMap) -> u32 {
    if floor == 0 { 0 } else { map.map(floor) }
}

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
            // A `(( … ))` collects nested bodies exactly as `$(( … ))` does, so
            // they are renumbered here for the same reason — they are parsed in
            // this token stream, and an error in one is blamed on the enclosing
            // source's line.
            Tok::ArithCmd(_, nested) => map_arith_comsubs(nested, map),
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
            Seg::CmdSub(_, close, body) => {
                *close = map.map(*close);
                if let SubBody::ArithFallback(nested) = body {
                    map_arith_comsubs(nested, map);
                }
            }
            Seg::Arith(_, _, nested) => map_arith_comsubs(nested, map),
            Seg::ParamBraced(_, open, nested, _) => {
                *open = map.map(*open);
                map_arith_comsubs(nested, map);
            }
            Seg::ProcSub(_, _, open, _) => *open = map.map(*open),
            Seg::Dq(inner, _) => map_segs(inner, map),
            _ => {}
        }
    }
}

/// Renumber the nested bodies an arithmetic scan recorded, exactly as a
/// [`Seg::CmdSub`]'s own close line is renumbered: they are parsed in this token
/// stream, so an error in one is blamed on the enclosing source's line and not
/// on the line a fresh lex of the body would have counted.
///
/// Both of the lines a span may carry are renumbered, because the two spellings
/// count from different ends: a `$( … )` body is numbered back from its closing
/// `)` and a `<( … )` / `>( … )` body forward from its opening delimiter (see
/// [`parse_procsub_body`]). Leaving the second one alone is why a procsub in a
/// `${ … }` body used to be blamed on line 1 of an `eval` string while the `$(`
/// spelling beside it named the line the `eval` was written on.
fn map_arith_comsubs(nested: &mut [CmdSubSpan], map: &LineMap) {
    for sub in nested {
        sub.close_line = map.map(sub.close_line);
        if let SubOpen::Proc { open_line, .. } = &mut sub.open {
            *open_line = map.map(*open_line);
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
        truncated_at: None,
        eof_closed_in_list: false,
        eof_floor: 0,
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
    // token, so the parser's cursor still points at the error site, and bash
    // reports the line its *reader* was on — the token's own line unless a
    // `\<newline>` written flush after it dragged the reader onto the next one.
    //
    // An *unexpected end of file* is the one error whose site is not a token at
    // all: the parser asked for one and the reader ran out. Ask for the line at
    // the end of the stream rather than at the cursor, which
    // [`Parser::reader_line_at`] answers with the fetch that found the end of
    // file — usually a line past the last token, but not when that token's own
    // lookahead had already found it. Key that off the message, not the cursor:
    // an error can name a token that is not in this stream at all (a `$( … )`
    // body reports the substitution's closing `)`), and those still belong on
    // the last token's line.
    parsed.map_err(|e| {
        let line =
            if e.is_incomplete() { p.reader_line_at(p.toks.len()) } else { p.reader_line() };
        // See the same test in `IncrementalParser::parse_next`: an error already
        // carrying a line came from a `$( … )` body, which bash's scan parsed
        // before any `((` copy was pushed.
        let echo = if e.line.is_some() { p.reader_echo_at_scan() } else { p.reader_echo() };
        e.or_line(line).or_echo(echo)
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
    /// `Some(line)` when this stream stops because the *lexer* gave up inside an
    /// unclosed construct, rather than because the input ended — carrying the
    /// line bash's reader had run to by then, one past the input's last.
    ///
    /// The distinction matters wherever a diagnostic depends on what the parser
    /// was handed at the end. bash's scan bails *to the parser* on an
    /// unterminated construct (see [`ParseError::bail_sequel`]), so the parser
    /// gets a token — one that is not a `WORD` and cannot be named — where osh's
    /// truncated stream simply has nothing left. `cond_operand_error` is the one
    /// place that can tell the two apart from the outside, and it needs this to
    /// do it: `[[ x =~ ( ]]` is not `[[ x =~` with the file ending.
    ///
    /// `None` for every stream whose end is a real end — including a `$( … )`
    /// body parsed on its own, whose text the enclosing scan already closed.
    truncated_at: Option<u32>,
    /// Set when a `for`/`select` word list was terminated by the **end of input
    /// itself**, which makes the parser ask for one token more than any other
    /// construct does.
    ///
    /// bash's rule is `FOR WORD newline_list IN word_list list_terminator
    /// newline_list DO …`, and `list_terminator` is `'\n' | ';' | yacc_EOF`
    /// (parse.y:517) — the end-of-file token is a terminator in its own right.
    /// So when the input runs out right after the list, the EOF the last word's
    /// scan produced is *consumed* as the terminator, and the `newline_list`
    /// behind it still has to look ahead: that request finds the buffer used up
    /// and pays `line_number++` a second time (parse.y:2361). Every other
    /// compound reaches its EOF through a rule that cannot accept one, so it
    /// asks once and stops.
    ///
    /// Only the `IN` forms carry a `list_terminator`, so `for i\⏎` — no list at
    /// all — is charged once like everything else, and a list closed by a real
    /// `;` or newline leaves the EOF still to be fetched, which is again one
    /// request. Read by [`Parser::reader_line_at`] past the end of the stream.
    eof_closed_in_list: bool,
    /// The lowest line an end-of-input diagnostic may name, from
    /// [`crate::lexer::Tokenized::dparen_eof_floor`]. 0 where nothing constrains
    /// it.
    eof_floor: u32,
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
    /// Which entries of `srcs` are `((` copies (see
    /// [`crate::lexer::DparenCopy`]), and where in `srcs[0]` each was cut from.
    /// A copy is a *slice* of the shell's own input, unlike an alias
    /// replacement, so an offset in one can be turned back into the offset it
    /// had before the push — which is what [`Spans::echo_line_at_scan`] needs.
    dparen_bases: Vec<(u32, u32)>,
}

impl Spans {
    fn of(src: BStr<'_>, ends: Vec<u32>, dparens: &[DparenCopy]) -> Self {
        let mut s = Self {
            srcs: vec![bytes::chars(src).collect()],
            parents: Vec::new(),
            ends: ends.into_iter().map(|end| TokSpan { src: 0, end }).collect(),
            dparen_bases: Vec::new(),
        };
        s.push_dparens(dparens);
        s
    }

    /// The spans of an alias-expanded stream: the shell's own text first, then
    /// every replacement the pass pushed into it, in the order it pushed them —
    /// which is the numbering [`crate::lexer::TokSpan::src`] uses.
    fn expanded(src: Vec<Ch>, x: &AliasExpansion, dparens: &[DparenCopy]) -> Self {
        let mut s = Self {
            srcs: std::iter::once(src)
                .chain(x.bodies.iter().map(|b| bytes::chars(&b.text).collect()))
                .collect(),
            parents: x.bodies.iter().map(|b| b.parent).collect(),
            ends: x.spans.clone(),
            dparen_bases: Vec::new(),
        };
        s.push_dparens(dparens);
        s
    }

    /// Add each `((` copy as a text of its own and move the tokens read out of
    /// it onto that text. See [`DparenCopy`]: bash re-reads the rebuilt text as
    /// a *pushed string*, so it is `shell_input_line` for as long as it lasts —
    /// which is what makes a diagnostic echo the whole copy, embedded newlines
    /// and all, rather than the physical line it was cut from.
    ///
    /// The copies are laid over whatever texts are already here (the shell's
    /// input, plus any alias replacements), so the numbering both schemes hand
    /// out stays disjoint. A token already labelled with a replacement is left
    /// alone: it was read from the alias's own pushed string, which sits *above*
    /// the copy on the same stack.
    fn push_dparens(&mut self, dparens: &[DparenCopy]) {
        if dparens.is_empty() {
            return;
        }
        let base = u32::try_from(self.srcs.len()).unwrap_or(u32::MAX);
        let Some(input) = self.srcs.first().cloned() else {
            return;
        };
        for (i, c) in dparens.iter().enumerate() {
            let from = c.start as usize;
            let to = (c.end as usize).saturating_add(1).min(input.len());
            self.dparen_bases
                .push((u32::try_from(self.srcs.len()).unwrap_or(u32::MAX), c.start));
            self.srcs.push(input.get(from..to).unwrap_or(&[]).to_vec());
            // `pop_string` puts the reader back where the scan left it: just past
            // the character the adjacency test rejected. For a copy pushed while
            // another was being read that offset is *inside the enclosing copy*,
            // since that is the text the scan was reading.
            let back = c.end.saturating_add(1);
            self.parents.push(
                match crate::lexer::dparen_at(dparens.get(..i).unwrap_or(&[]), back) {
                    Some((j, off)) => TokSpan {
                        src: base.saturating_add(u32::try_from(j).unwrap_or(u32::MAX)),
                        end: off,
                    },
                    None => TokSpan { src: 0, end: back },
                },
            );
        }
        for s in &mut self.ends {
            if s.src != 0 || s.end == u32::MAX {
                continue;
            }
            if let Some((i, off)) = crate::lexer::dparen_at(dparens, s.end) {
                *s = TokSpan {
                    src: base.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)),
                    end: off,
                };
            }
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
    /// The scan is over **one line**, because bash's is: it reads
    /// `t = shell_input_line`, which holds a single line and is *replaced* —
    /// not extended — by the fetch that follows a deleted `\<newline>`
    /// (parse.y's `goto restart_read`). So a reader dragged onto a new line is
    /// at index 0 of it, where bash's first loop cannot run and its
    /// one-character branch returns `t[0]`, and no scan can reach back into the
    /// line before. With a non-blank next line that is the same answer a
    /// whole-text walk would give, which is why only a blank one shows the
    /// difference: `[[ a == b )\<newline><newline>` is reported near the
    /// newline itself.
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
        let (stop, fetched) = self.reader_stop(pos, r)?;
        let full = self.text(stop.src)?;
        let at = stop.end as usize;
        if at > full.len() {
            return None;
        }
        // The reader's own line: from just past the previous newline through the
        // one that ends it. An alias replacement has no newline in it, so this
        // is the whole of that text and costs nothing there.
        //
        // An offset sitting *one past* a newline is the one ambiguous case, and
        // the fetch count settles it. `shell_input_line` holds one line
        // including its newline, and an index of `strlen` is the NUL after it —
        // still that line, which is why `error_token_from_text`'s first test
        // steps back onto the newline. Only a **fetch** replaces the buffer and
        // puts the reader at index 0 of the next line, and that is exactly what
        // [`Spans::reader_stop`] counts: with none, an offset past a newline
        // belongs to the line that newline ends. So `for ((i=0;i<1;i++)⏎)` is
        // reported near `;i++)` against line 1, while a deleted `\⏎` — which
        // did fetch — reports against line 2.
        let line_at = if fetched == 0 && full.get(at.wrapping_sub(1)) == Some(&Ch::U('\n')) {
            at.saturating_sub(1)
        } else {
            at
        };
        let start =
            full.get(..line_at)?.iter().rposition(|&c| c == Ch::U('\n')).map_or(0, |n| n + 1);
        let end = full
            .get(start..)?
            .iter()
            .position(|&c| c == Ch::U('\n'))
            .map_or(full.len(), |n| start.saturating_add(n).saturating_add(1));
        let t = full.get(start..end)?;
        let mut i = at.checked_sub(start)?;
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

    /// How many `\<newline>` pairs are still standing in front of the reader
    /// after the token at `pos`, with nothing but further continuations between
    /// them and the end of the shell's input.
    ///
    /// These are the ones [`Spans::cont_lines`] does *not* count: a token bash
    /// completed by its own lookahead (`&&`, `||`, `>&`) never looked again, so
    /// the reader is parked in front of the run rather than past it, and it is
    /// whoever asks for the next token that deletes them — one `line_number++`
    /// each (parse.y:2677). A token that did look has already had its whole run
    /// walked by `reader_stop`, and this answers zero for it.
    ///
    /// Only a run that reaches the end of the input is the *asker's* to pay for.
    /// If anything follows it, the fetch after the last deletion brings that line
    /// in for free and the tokens on it are stamped by their own scan; a blank
    /// last line still ends in a newline, so it is a token here even when bash
    /// discards it.
    ///
    /// Answers zero inside an alias replacement, whose end is not the end of the
    /// shell's input: the read that runs off it pops back to the line the alias
    /// word was written on and carries on there.
    fn pending_conts(&self, pos: usize, r: Reader) -> u32 {
        let Some((stop, _)) = self.reader_stop(pos, r) else {
            return 0;
        };
        if stop.src != 0 {
            return 0;
        }
        let Some(t) = self.text(stop.src) else {
            return 0;
        };
        let mut i = stop.end as usize;
        let mut n = 0u32;
        while let Some(len) = cont_len(t.get(i..).unwrap_or(&[])) {
            i = i.saturating_add(len);
            n = n.saturating_add(1);
        }
        if i >= t.len() { n } else { 0 }
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
        // The look past the *last* token of a backslash-closed string has no
        // newline to stop on, so it runs the buffer out and enters the fetch
        // block: one line, charged to the token's own read rather than to the
        // parser's next request. See [`closed_with_backslash`]. Only the
        // shell's own input is closed that way — an alias replacement is a
        // pushed string, and `pop_string` ends it.
        if at.src == 0 && i >= t.len() && closed_with_backslash(t) {
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

    /// Whether bash's `shell_input_line` is **empty** where the reader stopped
    /// after the token at `pos` — the test `report_syntax_error` makes at
    /// parse.y:6273 before it will look for an offending token to report the
    /// error "near".
    ///
    /// The buffer is emptied by exactly one thing: a fetch that found the end of
    /// the input — and that is the very fetch [`Spans::reader_stop`] already
    /// counts, so the test is "the walk crossed something (`n > 0`) and ran off
    /// the end of the text". Both ways of getting there qualify:
    ///
    /// - a `\<newline>` flush against the token with nothing behind it, deleted
    ///   by the look past the token, which then finds the buffer used up;
    /// - a `-c` string [`close_last_line`] closed with a *backslash*, which has
    ///   no newline for the reader to stop on at all.
    ///
    /// A token merely *at* the end of the text is not one of them: every other
    /// input is newline-closed, so the reader stops on that newline with a line
    /// still in hand. `bash -c '[[ a == b )'` reports near `)` and echoes its
    /// line; so does `bash -c '[[ a == b )\'`, whose `\` is not a continuation
    /// (the close doubled it) — the reader stops on it and reports near `)\`.
    ///
    /// Only the shell's own input ends this way. An alias replacement is a
    /// pushed string that `pop_string` ends, restoring a line that still has
    /// text on it — and `reader_stop` has already popped every replacement the
    /// look exhausted, so `src != 0` here means the reader is still *inside* one.
    fn reader_line_empty(&self, pos: usize, r: Reader) -> bool {
        let Some((stop, n)) = self.reader_stop(pos, r) else {
            return false;
        };
        if stop.src != 0 || n == 0 {
            return false;
        }
        let Some(t) = self.text(stop.src) else {
            return false;
        };
        stop.end as usize >= t.len()
    }

    /// The text bash would echo under the diagnostic for an error at the token
    /// `pos` — `shell_input_line` as it stands where the reader stopped — when
    /// that is an alias replacement rather than the shell's own input.
    ///
    /// `None` for the shell's own input, which the caller echoes by line number
    /// instead: that is the physical line, and the reader's `src` says nothing
    /// about which of them it is on.
    ///
    /// Trailing newlines go, as `print_offending_line` drops them
    /// (`while (token_end && msg[token_end - 1] == '\n') --token_end;`,
    /// parse.y:6218-6226). *Embedded* ones stay, which is what makes a `((`
    /// copy spanning lines echo whole — see [`DparenCopy`].
    fn echo_line(&self, pos: usize, r: Reader) -> Option<Str> {
        let (stop, _) = self.reader_stop(pos, r)?;
        self.echo_of(stop)
    }

    /// [`Spans::echo_line`] as it stood *before* any `((` copy was pushed.
    ///
    /// bash parses a `$( … )` body inside an arithmetic command while it is
    /// still **scanning** for the closing `))`: `parse_matched_pair` hands each
    /// one to `extract_command_subst`, which parses it then and there. A body
    /// that does not parse is therefore reported before `parse_arith_cmd` has
    /// run the adjacency test that would push the copy, with `shell_input_line`
    /// still the physical line — `(( $(fi ))` echoes `(( $(fi ))`, not the copy
    /// `( $(fi ))`. osh lowers those bodies later, while the copy is being
    /// re-read, so the copy has to be unwound to reach the same answer. Every
    /// copy is a slice of `srcs[0]`, so one step does it however deeply the
    /// pushes stacked.
    fn echo_line_at_scan(&self, pos: usize, r: Reader) -> Option<Str> {
        let (stop, _) = self.reader_stop(pos, r)?;
        self.echo_of(self.before_the_push(stop))
    }

    /// A span inside a `((` copy, as the offset it held in the text the copy was
    /// cut from; anything else unchanged. See [`Spans::dparen_bases`].
    fn before_the_push(&self, s: TokSpan) -> TokSpan {
        match self.dparen_bases.iter().find(|&&(src, _)| src == s.src) {
            Some(&(_, base)) => TokSpan { src: 0, end: base.saturating_add(s.end) },
            None => s,
        }
    }

    /// The text `shell_input_line` holds where the reader stopped, or `None` for
    /// the shell's own input — which the caller echoes by line number instead.
    fn echo_of(&self, stop: TokSpan) -> Option<Str> {
        if stop.src == 0 {
            return None;
        }
        let t = self.text(stop.src)?;
        let keep = t.len() - t.iter().rev().take_while(|&&c| c == Ch::U('\n')).count();
        Some(bytes::from_chars(t.get(..keep).unwrap_or(&[]).iter().copied()))
    }
}

/// Whether the character just before `end` in `t` is the newline of a
/// `\<newline>` the lexer deleted. A *plain* newline is not one: it is the last
/// character of the line it terminates, and bash bumps `line_number` only on the
/// fetch that follows.
///
/// A backslash standing in front of the newline only makes it a continuation
/// when the *run* of backslashes it belongs to is odd. `read_token_word` reads
/// the character after a `\` with `shell_getc (0)` — continuation removal off —
/// so the run is consumed in pairs, and only an unpaired last backslash is left
/// to join with the newline. With an even run the newline is an ordinary one and
/// costs nothing here: `printf 'echo 1\nnosuch$LINENO\\\n'` reports `nosuch2`,
/// not `nosuch3`.
fn ends_in_cont(t: &[Ch], end: usize) -> bool {
    if t.get(end.wrapping_sub(1)) != Some(&Ch::U('\n')) {
        return false;
    }
    // `\<newline>`, or the `\<CR><LF>` a CRLF file writes.
    let run_end = match t.get(end.wrapping_sub(2)) {
        Some(&Ch::U('\\')) => end.wrapping_sub(2),
        Some(&Ch::U('\r')) if t.get(end.wrapping_sub(3)) == Some(&Ch::U('\\')) => {
            end.wrapping_sub(3)
        }
        _ => return false,
    };
    let run = t
        .get(..run_end.wrapping_add(1))
        .unwrap_or(&[])
        .iter()
        .rev()
        .take_while(|&&c| c == Ch::U('\\'))
        .count();
    run % 2 == 1
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
    ///
    /// A [`Tok::Refused`] does not cross either: the read that made it *is* the
    /// look past the construct — `parse_arith_cmd`'s `shell_getc (0)`, with
    /// continuation removal explicitly off — and it is already counted in the
    /// token's own span, so nothing follows it to be looked at.
    peeks: bool,
    /// Whether the token came from `read_token_word`, whose terminator peek can
    /// reach one character further than the token itself. See the third case in
    /// [`Spans::reader_stop`].
    ///
    /// A here-document body counts as one: the token this parser carries the
    /// body on stands for the *delimiter word*, and bash reads that delimiter
    /// with `read_token_word` like any other — `<<` is an operator, but what
    /// follows it is a word, and `read_token` returns it through the same path.
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
                )) | Some(Tok::Refused)
            ),
            word: matches!(
                t,
                Some(
                    Tok::Word(_)
                        | Tok::Io(_)
                        | Tok::VarFd(_)
                        | Tok::ArrayAssign { .. }
                        | Tok::HereDoc(..)
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

    /// The line the token at `at` sits on, falling back to [`Self::cur_line`]
    /// when the index is past the end.
    fn line_of(&self, at: usize) -> u32 {
        self.lines.get(at).copied().unwrap_or_else(|| self.cur_line())
    }

    /// The line bash would *report* an error at the current token on: not the
    /// token's own line but the reader's, which is a line further down for every
    /// `\<newline>` written flush after the token. See [`Spans::cont_lines`].
    fn reader_line(&self) -> u32 {
        self.reader_line_at(self.pos)
    }

    /// [`Parser::reader_line`] for an arbitrary position — bash's `line_number`
    /// as it stands the moment `read_token` hands the token at `at` over.
    ///
    /// Past the end of the stream it can be one line *further* still. The read
    /// finds the input buffer used up and enters `shell_getc`'s fetch block,
    /// whose first statement is `line_number++` (parse.y:2361) — reached before
    /// the fetch itself returns end of file, and reached again on every further
    /// request. So merely asking for a token that is not there costs a line:
    /// `printf 'echo 1\nnosuch$LINENO\\' > f` reports `nosuch4` out of a two-line
    /// file — line 2 for the word, line 3 for the continuation the word's own
    /// terminator scan deleted, line 4 for the lookahead the parser needed before
    /// it could reduce the command.
    ///
    /// It is charged only where the last read really did run out. A word's scan
    /// stops *on* the character that ended it (`if (character == EOF) goto
    /// got_token`, parse.y:4904) and pushes nothing back, so the next request is
    /// a fresh fetch. An operator instead peeks with `shell_getc` and returns the
    /// peek with `shell_ungetc`, which at the start of a line stows it in
    /// `eol_ungetc_lookahead` — and `shell_getc` returns that ahead of any fetch.
    /// The end of file the peek already found is therefore handed over a second
    /// time for free, which is why `cat >\⏎` at the end of a file is blamed on
    /// line 3 where `cat <<x\` is blamed on line 4.
    ///
    /// The stow is only free where the peek reached the end of input at all,
    /// which for a token that did not sit against a `\⏎` it means the peek never
    /// ran the buffer out: it found the newline the reader wrote back, pushed
    /// *that* back, and the end of file is still to be fetched. So the three
    /// conditions are read together — the last token peeked, was not a word, and
    /// its peek deleted a continuation running to the end of the input.
    ///
    /// A request that is *not* free can cost more than one line, because it has
    /// to get past whatever the reader is parked in front of first. Every
    /// `\<newline>` it deletes on the way is its own `line_number++`, so a token
    /// bash completed by its own lookahead — `&&`, `||`, `>&`, which never looked
    /// again — leaves the whole trailing run to be charged to the request:
    /// [`Spans::pending_conts`].
    fn reader_line_at(&self, at: usize) -> u32 {
        let Some(tok) = self.toks.get(at) else {
            let Some(last) = self.toks.len().checked_sub(1) else {
                return self.cur_line();
            };
            let r = Reader::of(self.toks.get(last));
            let stowed = r.peeks && !r.word && self.spans.cont_lines(last, r) > 0;
            // The deletions the request makes, or the one fetch it enters when
            // there is nothing to delete — never both, since the fetch that
            // follows a deletion is the deletion's own `goto restart_read` and
            // is not charged again.
            let bump = if stowed { 0 } else { self.spans.pending_conts(last, r).max(1) };
            // …and one more when a `for`/`select` list swallowed the end-of-file
            // token as its `list_terminator`, so that the request being answered
            // here is the *second* one to run the buffer out.
            let bump = bump.saturating_add(u32::from(self.eof_closed_in_list));
            // …and a floor under the lot when a `((` handed its text back as
            // `( ( … ) )`: the copy bought one extra empty request, which shows
            // only where no real line answered it. See
            // [`crate::lexer::Tokenized::dparen_eof_floor`].
            return self.reader_line_at(last).saturating_add(bump).max(self.eof_floor);
        };
        let line = self.lines.get(at).copied().unwrap_or_else(|| self.cur_line());
        line.saturating_add(self.spans.cont_lines(at, Reader::of(Some(tok))))
    }

    /// The text bash would echo under a `syntax error near …` at the current
    /// token, when that is an alias replacement rather than the script itself.
    /// Read from the same cursor as [`Parser::reader_line`], because both ask
    /// the same question — where bash's reader is. See [`Spans::echo_line`].
    fn reader_echo(&self) -> Option<Str> {
        let r = Reader::of(self.toks.get(self.pos));
        self.spans.echo_line(self.pos, r)
    }

    /// [`Parser::reader_echo`] for an error raised in a `$( … )` body of its
    /// own, which bash's *scan* found — before any `((` copy was pushed. See
    /// [`Spans::echo_line_at_scan`].
    fn reader_echo_at_scan(&self) -> Option<Str> {
        let r = Reader::of(self.toks.get(self.pos));
        self.spans.echo_line_at_scan(self.pos, r)
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
    ///
    /// Both callers lower a word, and a word's substitutions are parsed by
    /// bash's *scan*, so the echo is the one from before any `((` copy was
    /// pushed: see [`Spans::echo_line_at_scan`].
    fn echo_at(&self, at: usize) -> Option<Str> {
        self.spans.echo_line_at_scan(at, Reader::of(self.toks.get(at)))
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
    /// [`Parser::cond_near_at`]).
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
            // An `ARITH_CMD` is named by the expression it collected, not by a
            // paren: `error_token_from_token` returns
            // `string_list (yylval.word_list)` (parse.y), which is the same
            // re-printed text the token already carries — verbatim, inner
            // spaces and all. So `[[ a ]] ((  ))` is reported near two spaces
            // and `[[ a ]] (( 1 + 1 ))` near ` 1 + 1 `.
            Some(Tok::ArithCmd(raw, nested)) => parse_arith_comsubs(nested, self.opts)
                .map_or_else(|_| raw.clone(), |subs| splice_reprints(raw, subs)),
            // A `NUMBER` is named by its *value*, not by how it was written:
            // `error_token_from_token` returns `itos (yylval.number)` (parse.y),
            // so `f() 007>x` is reported near `7`.
            Some(Tok::Io(n)) => n.to_string().into_bytes(),
            // An `ASSIGNMENT_WORD` is named by `yylval.word->word`, and for a
            // compound literal that word is a *re-print*, not the source:
            // `read_token_word` builds it as the name, then `=`, `(`, the
            // `string_list` of the collected elements — joined with one space —
            // and `)` (parse.y:5168–5181, 6572–6576). So `f() a=(1   2)` is
            // reported near `a=(1 2)` and `f() a=(  )` near `a=()`, while each
            // element keeps the spelling it was written with.
            Some(Tok::ArrayAssign { name, index, append, elems }) => {
                let mut out = name.as_bytes().to_vec();
                if let Some(i) = index {
                    out.push(b'[');
                    out.extend_from_slice(i);
                    out.push(b']');
                }
                if *append {
                    out.push(b'+');
                }
                out.extend_from_slice(b"=(");
                for (n, segs) in elems.iter().enumerate() {
                    if n > 0 {
                        out.push(b' ');
                    }
                    match word_from_segs(segs, self.opts) {
                        Ok(w) => out.extend_from_slice(&crate::unparse::word_src(&w)),
                        Err(_) => out.extend_from_slice(b"word"),
                    }
                }
                out.push(b')');
                out
            }
            // A construct the lexer refused already carries the spelling to
            // blame — the operator that stood where an array element belonged.
            Some(Tok::Invalid(op)) => op.clone(),
            // Anything else (a here-doc body, which is not a token to bash at
            // all) has no word spelling.
            _ => b"word".to_vec(),
        }
    }

    /// Build bash's canonical "unexpected" parser diagnostic for the current
    /// position: at end of input it is `syntax error: unexpected end of file`;
    /// otherwise `syntax error near unexpected token \`TOKEN'` — bash quotes the
    /// offending token with a leading backtick and a trailing single quote.
    /// bash's `error_token_from_token` (parse.y:6132–6169) — the name a syntax
    /// error gives the token at `pos` — or `None` where bash's switch has no
    /// branch for it and the function returns NULL.
    ///
    /// `REDIR_WORD` is the case that reaches the `None`: the `{v}` of
    /// `{v}>file` is a token bash's parser can be holding when it errors, but
    /// the switch never names it. `report_syntax_error` then falls through to
    /// its *other* branch, and the message changes shape — see
    /// [`Parser::unexpected_here`].
    fn error_token_at(&self, pos: usize) -> Option<Str> {
        match self.toks.get(pos) {
            // A refusal reaches bison as its EOF token and has no name at all —
            // the switch has no branch for it either. See [`Tok::Refused`].
            Some(Tok::VarFd(_) | Tok::Refused) => None,
            _ => Some(self.token_display_at(pos)),
        }
    }

    /// The text bash's text-scanning branch reports an error "near", for a
    /// token that `error_token_from_token` declined to name.
    fn near_at(&self, pos: usize) -> Str {
        self.spans
            .near(pos, Reader::of(self.toks.get(pos)))
            .unwrap_or_else(|| self.token_display_at(pos))
    }

    fn unexpected_here(&self) -> ParseError {
        if self.peek().is_none() {
            return ParseError::new("syntax error: unexpected end of file");
        }
        // bash tries `error_token_from_token` first and only scans the input
        // text when that comes back NULL, and the two branches print *different*
        // messages: `syntax error near unexpected token \`X'` against
        // `syntax error near \`X'` (parse.y:6251, 6276). So a `{v}>` is reported
        // with the shorter one, and by the scan's own text rather than by the
        // token — which is why it comes back with the `>` attached.
        let Some(name) = self.error_token_at(self.pos) else {
            return ParseError::new(&bfmt![b"syntax error near `", self.near_at(self.pos), b"'"]);
        };
        let e = ParseError::new(&bfmt![b"syntax error near unexpected token `", name, b"'"]);
        // A refused construct costs only its own unit, where a grammar error
        // costs the rest of the input. See [`ParseError::recoverable`].
        if matches!(self.peek(), Some(Tok::Invalid(_))) {
            e.only_this_unit()
        } else {
            e
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
        let list = self.parse_and_or()?;
        // Which of the three bash keeps as the connector matters to the
        // deparser, not to execution: see [`ItemSep`]. No separator at all
        // records as `Semi`, which is what it parses and prints as.
        let mut sep = ItemSep::Semi;
        let mut had_sep = false;
        match self.peek() {
            Some(Tok::Op(Op::Amp)) => {
                sep = ItemSep::Amp;
                had_sep = true;
                self.pos += 1;
            }
            Some(Tok::Newline) => {
                sep = ItemSep::Newline;
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
        Ok(Some(Item { list, sep }))
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

    /// bash's `shell_command` production: every compound command, and nothing
    /// else. `Ok(None)` means the current token opens none of them.
    ///
    /// This is factored out because `shell_command` appears twice in bash's
    /// grammar — once inside `command`, and once as the whole of
    /// `function_body` (`function_body: shell_command | shell_command
    /// redirection_list`). A function body is therefore *any* of the eleven, not
    /// just the brace group and the subshell: `f() if true; then echo hi; fi`
    /// and `f() ((1))` both define. See [`Parser::parse_compound_body`].
    ///
    /// The redirection list is deliberately left unconsumed, because the two
    /// callers attach it to different places — see the same doc comment.
    fn parse_shell_command(&mut self) -> Result<Option<Command>, ParseError> {
        if let Some(w) = self.reserved_here() {
            return match w {
                "if" => self.parse_if().map(Some),
                "while" => self.parse_loop(false).map(Some),
                "until" => self.parse_loop(true).map(Some),
                "for" => self.parse_for().map(Some),
                "select" => self.parse_select().map(Some),
                "case" => self.parse_case().map(Some),
                "{" => self.parse_brace_group().map(Some),
                // A command that begins with a stray closing/continuation
                // keyword (`then`, `do`, `fi`, `done`, `esac`, `else`, …):
                // bash reports it as an unexpected token. There is no simple
                // command to fall through to — a reserved word is not a `WORD`
                // — so this is an error in both of the production's positions.
                _ => Err(self.unexpected_here()),
            };
        }
        if self.at_op(Op::LParen) {
            return self.parse_subshell().map(Some);
        }
        // `(( expr ))` arithmetic command (lexed as a single token).
        if let Some(Tok::ArithCmd(raw, nested)) = self.peek() {
            // The string carries the re-print, not what was written:
            // `parse_arith_cmd` reads the body with
            // `parse_matched_pair (0, '(', ')', &ttoklen, P_ARITH)`
            // (parse.y:4519–4530), the same scan `$(( … ))` gets, so the same
            // `APPEND_NESTRET` puts `print_comsub`'s answer into its buffer.
            let expr = splice_reprints(raw, parse_arith_comsubs(nested, self.opts)?);
            // The line the token *ends* on, which is the one bash stamps: the
            // whole `(( … ))` is scanned into a single `ARITH_CMD` before the
            // reduction that records `line_number`. See [`ArithClause::line`].
            let line = self.line_of(self.pos);
            self.pos += 1;
            return Ok(Some(Command::Arith(ArithClause { expr, line })));
        }
        // `[[ expr ]]` conditional expression.
        if self.at_bare_word(b"[[") {
            return self.parse_cond().map(Some);
        }
        Ok(None)
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        if let Some(cmd) = self.parse_shell_command()? {
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
            // The definition's own line is the `)`'s, not the name's: bash sets
            // `function_dstart = line_number` in the lexer at the moment it
            // reads a `)` that closes a `(` after a WORD (parse.y:3580). See
            // [`FunctionDef::line`].
            let line = self.line_of(self.pos + 2);
            self.pos += 3;
            self.skip_newlines();
            // Where the body *opens*, which is what a call stands on. See
            // [`FunctionDef::body_line`].
            let body_line = self.cur_line();
            let body = self.parse_compound_body()?;
            // bash allows redirections after the body (`f() { …; } >log`); they
            // are stored with the function and applied on every invocation.
            let mut redirects = Vec::new();
            while self.at_redirect_start() {
                redirects.push(self.parse_redirect()?);
            }
            return Ok(Command::Function(FunctionDef {
                name,
                definable,
                body,
                line,
                body_line,
                redirects,
            }));
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
        // With the `function` keyword the definition's line is the *name*'s:
        // bash stamps `function_dstart = line_number` from `read_token_word`'s
        // `case FUNCTION:` arm, i.e. on the word right after the keyword
        // (parse.y:5349). See [`FunctionDef::line`].
        let mut line = self.cur_line();
        self.pos += 1; // consume the name word
        // Optional `()` after the name.
        if self.at_op(Op::LParen) {
            if !matches!(self.toks.get(self.pos + 1), Some(Tok::Op(Op::RParen))) {
                return Err(self.unexpected_here());
            }
            // …but a `)` closing a `(` after a WORD stamps it *again*
            // (parse.y:3580), and that arm does not care that the keyword form
            // already set it — so `function g \⏎ () { :; }` is line 2, measured.
            line = self.line_of(self.pos + 1);
            self.pos += 2;
        }
        self.skip_newlines();
        // Where the body *opens*; see [`FunctionDef::body_line`].
        let body_line = self.cur_line();
        let body = self.parse_compound_body()?;
        // bash allows redirections after the body (`function f { …; } >log`);
        // they are stored with the function and applied on every invocation.
        let mut redirects = Vec::new();
        while self.at_redirect_start() {
            redirects.push(self.parse_redirect()?);
        }
        Ok(Command::Function(FunctionDef { name, definable, body, line, body_line, redirects }))
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
        // `COPROC` *is* on `reserved_word_acceptable`'s list (parse.y:5367), so
        // the word after it is looked up in `word_token_alist` like any other
        // word in a command position. bash's production is `COPROC WORD
        // compound_command`, and a reserved word is not a `WORD`: `coproc done
        // { echo y; }` is a syntax error near `done', not a coproc named `done`.
        if self.reserved_here().is_none()
            && let Some(w) = self.bare_word_here()
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
            Some(Tok::Op(Op::LParen)) | Some(Tok::ArithCmd(..)) => true,
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

    /// Parse a function body: bash's `function_body`, which is a whole
    /// `shell_command` and an optional redirection list.
    ///
    /// The brace group is the one arm that is *not* wrapped. bash's
    /// `make_function_def` takes the group's `command` directly, so `declare -f`
    /// re-prints one pair of braces rather than two, and a redirection written
    /// after it lands on the function — which is why the caller keeps consuming
    /// a list of its own after this returns, and why `f() { :; } >/dev/null`
    /// prints `} > /dev/null`.
    ///
    /// Every other arm keeps its own node and its own redirections, wrapped as
    /// the body's single statement. That is not a formality: the parentheses of
    /// a `( … )` body are part of the function, so `f() ( cd /; x=1 )` must leak
    /// neither the `cd` nor the `x` and an `exit` inside must end only the
    /// subshell (osh used to unwrap it, which ran every such function in the
    /// caller's shell). And bash prints the redirection *inside* the braces for
    /// all of them — `f() ( : ) >/dev/null` re-prints as
    /// `f () \n{ \n    ( : ) > /dev/null\n}` — which is what consuming the list
    /// here rather than in the caller produces.
    fn parse_compound_body(&mut self) -> Result<Program, ParseError> {
        if let Some(w) = self.reserved_here()
            && w == "{"
            && let Command::BraceGroup(p) = self.parse_brace_group()?
        {
            return Ok(p);
        }
        // Not a `shell_command`. bash diagnoses this positionally: at EOF
        // (`f()` / `function f` with no body) it reports "unexpected end of
        // file"; otherwise it names the offending token (`f() echo hi` →
        // "unexpected token `echo'"), matching both function-definition forms.
        // A nested definition (`f() f2() { :; }`) and a negation (`f() ! true`)
        // are errors for the same reason: neither is a `shell_command`.
        let Some(cmd) = self.parse_shell_command()? else {
            return Err(self.unexpected_here());
        };
        let cmd = self.with_redirects(cmd)?;
        Ok(Program {
            items: vec![Item {
                list: AndOr {
                    first: Pipeline {
                        negated: false,
                        timed: false,
                        time_posix: false,
                        commands: vec![cmd],
                    },
                    rest: Vec::new(),
                },
                // The sole item of a body: never a connector, so `Semi` — the
                // separator that prints as nothing.
                sep: ItemSep::Semi,
            }],
        })
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
        // Stamped from the `)` and before stepping past it, because bash builds
        // the node at the reduction — once the `)` has been read. See
        // [`SubshellClause::line`].
        let line = self.cur_line();
        self.pos += 1;
        Ok(Command::Subshell(SubshellClause { body, line }))
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
        // single `ArithCmd` token carrying the raw `init; cond; update` text
        // and the `$( … )` bodies its scan stepped over.
        if let Some(Tok::ArithCmd(raw, nested)) = self.peek() {
            // The re-prints are spliced in *before* the header is split on `;`,
            // which is the only order that works: the ranges index into the whole
            // header text the one `P_ARITH` scan built, and each section's own
            // substitutions are already re-printed by the time
            // `parse_for_arith` cuts it into three (parse.y:4519–4530 collects
            // the header, `ARITH_FOR_EXPRS` splits it afterwards).
            let raw = splice_reprints(raw, parse_arith_comsubs(nested, self.opts)?);
            // bash stamps `arith_for_lineno` where the `((` is read
            // (parse.y:4469), not where the command is reduced, so a malformed
            // header is blamed here rather than on the `done` far below — and
            // at the line the `((` opened on, where a token's own line is the
            // one it *ends* on ([`crate::lexer::Lexer::stamp_lines`], which
            // models `$LINENO`). The only newlines the token spans are the ones
            // in the header, so counting those walks the line back.
            let nl = raw.iter().filter(|&&b| b == b'\n').count();
            let line = self.cur_line().saturating_sub(u32::try_from(nl).unwrap_or(u32::MAX));
            self.pos += 1;
            return self.parse_for_arith(&raw, line);
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
        // Stamped from the loop variable and before stepping past it, a token's
        // line being the one it *ends* on. See [`CaseClause::line`].
        let line = self.cur_line();
        self.pos += 1;
        self.skip_newlines();
        let words = self.parse_in_list()?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::For(ForClause { var, words, body, line }))
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
        // `list_terminator` accepts the end-of-file token itself, so the list is
        // closed by the end of the input exactly when there was no separator to
        // skip and nothing left behind it — and then the `newline_list` behind
        // the terminator still has to ask for a token of its own. A real `;` or
        // newline took the terminator's place and leaves that end of file still
        // to be fetched, which is one request like anywhere else. See
        // `Parser::eof_closed_in_list`.
        let before = self.pos;
        self.skip_separators();
        self.eof_closed_in_list |= self.pos == before && before >= self.toks.len();
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
        // Stamped from the menu variable, as `for` and `case` are. See
        // [`CaseClause::line`].
        let line = self.cur_line();
        self.pos += 1;
        self.skip_newlines();
        let words = self.parse_in_list()?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::Select(SelectClause { var, words, body, line }))
    }

    /// Parse the body of a C-style `for (( init; cond; update ))` loop, given
    /// the raw `init; cond; update` text captured from the arithmetic token and
    /// the line the `((` was read on.
    ///
    /// The header is carved by [`arith_for_sections`] and is only *counted*
    /// once the whole command has been read, because that is when bash looks:
    /// `make_arith_for_command` runs from the grammar's reduction of
    /// `FOR ARITH_FOR_EXPRS … DO compound_list DONE` (parse.y:881), so a syntax
    /// error anywhere in the body is met first and reported instead —
    /// `for ((0;0)); do fi; done` names `fi`, not the two-section header.
    fn parse_for_arith(&mut self, raw: BStr<'_>, line: u32) -> Result<Command, ParseError> {
        let secs = arith_for_sections(&bytes::chars(raw).collect::<Vec<_>>());
        // An optional separator (`;`/newline) may precede `do`.
        self.skip_separators();
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        if secs.len() != 3 {
            // `nsemi != 3` (make_cmd.c:309). The count decides only *which*
            // first line is printed; the second, quoting the header back
            // between the `((` and `))` the writer did not have to type twice,
            // is the same either way. Both are `parser_error` calls rather than
            // `report_syntax_error`, so neither echoes the offending source
            // line, and both name the `((`'s own line.
            let first = ParseError::new(if secs.len() < 3 {
                "syntax error: arithmetic expression required"
            } else {
                "syntax error: `;' unexpected"
            });
            return Err(ParseError::new(&bfmt![b"syntax error: `((", raw, b"))'"])
                .under(first)
                .or_line(line));
        }
        let init = secs.first().cloned().unwrap_or_default();
        let cond = secs.get(1).cloned().unwrap_or_default();
        let update = secs.get(2).cloned().unwrap_or_default();
        // The same line the parse errors above are blamed on: bash stamps
        // `arith_for_lineno` and the parse-time `line_number` from one and the
        // same read of the `((`. See [`ForArithClause::line`].
        Ok(Command::ForArith(ForArithClause {
            init,
            cond,
            update,
            body,
            line,
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
        // Stamped from the subject word and before stepping past it: a token's
        // line is the one it *ends* on ([`crate::lexer::Lexer::stamp_lines`]),
        // which is exactly what bash's lexer records here. See
        // [`CaseClause::line`].
        let line = self.cur_line();
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
        Ok(Command::Case(CaseClause { word, items, line }))
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
            let list = self.parse_and_or()?;
            let mut sep = ItemSep::Semi;
            let mut had_sep = false;
            match self.peek() {
                Some(Tok::Op(Op::Amp)) => {
                    sep = ItemSep::Amp;
                    had_sep = true;
                    self.pos += 1;
                }
                Some(Tok::Newline) => {
                    sep = ItemSep::Newline;
                    had_sep = true;
                    self.pos += 1;
                }
                Some(Tok::Op(Op::Semi)) => {
                    had_sep = true;
                    self.pos += 1;
                }
                _ => {}
            }
            // Without a separator the only valid follower is one of the things
            // that *ends the arm*. bash's `compound_list` may reduce with no
            // trailing `;`/`&`/newline at all (`compound_list: newline_list
            // list0 | newline_list list1`), which is why `case a in a) { :; }
            // esac` parses — but what is then allowed after it is fixed by the
            // productions that receive the arm: `;;`, `;&` and `;;&` from
            // `case_clause_sequence`, and `esac` from `case_clause`. A `(`, a
            // `)` or a second command reduces nothing, and bash blames the
            // abutting token itself:
            //
            //     case a in a) echo x( y;; esac
            //     syntax error near unexpected token `('
            //
            // Letting this fall through instead re-entered the loop and parsed
            // the `( y;; esac` as a subshell, so the error surfaced at the `;;`
            // — and `case a in a) ( : ) ( : ) ;; esac`, where the second group
            // is a complete command, was accepted outright.
            //
            // `esac` is asked for through [`Parser::reserved_here`], which
            // already knows it only counts in command position — after a simple
            // command's word it is an ordinary argument, which is exactly why
            // `case a in a) echo x esac` runs off the end of the input in bash
            // rather than closing the `case`.
            if !had_sep {
                let at_ender = self.peek().is_none()
                    || self.at_op(Op::DSemi)
                    || self.at_op(Op::SemiAmp)
                    || self.at_op(Op::DSemiAmp)
                    || self.reserved_here() == Some("esac");
                if !at_ender {
                    return Err(self.unexpected_here());
                }
            }
            items.push(Item { list, sep });
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
                let first = ParseError {
                    line_at: vec![(0, self.cur_line())],
                    ..ParseError::new("unexpected EOF while looking for `]]'")
                };
                return Err(ParseError::new("syntax error: unexpected end of file").under(first));
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
            let head = if matches!(self.peek(), Some(Tok::Op(_))) {
                bfmt![
                    b"syntax error in conditional expression: unexpected token `",
                    &tok,
                    b"'"
                ]
            } else {
                bfmt![b"syntax error in conditional expression"]
            };
            let sequel = self.cond_sequel_at(self.pos);
            // Two `parser_error` calls, so two messages and two `line N:`
            // prefixes — the first at the `[[`'s own line, the second wherever
            // the reader stopped.
            let first = ParseError {
                line_at: vec![(0, open)],
                ..ParseError::new(&head)
            };
            return Err(ParseError::new(&sequel).under(first));
        }
        // The line the command will *run* on, which bash takes from the root
        // node and so reads off the expression's shape rather than off the
        // `]]`. `&&`/`||` are built after the trailing newline skip has already
        // fetched the closing token, so they take its line; everything else was
        // built the moment its own last token was read. See [`CondClause::line`].
        let line = match &expr {
            CondExpr::And(..) | CondExpr::Or(..) => self.cur_line(),
            // Back over the newlines `cond_skip_newlines` swallowed, to the
            // token the term ended on.
            _ => self.line_before(self.pos),
        };
        self.pos += 1;
        Ok(Command::Cond(CondClause { expr, line }))
    }

    /// The line the last real token before `at` ended on, stepping back over
    /// newline tokens — where `line_number` stood before a `cond_skip_newlines`
    /// walked past them.
    fn line_before(&self, at: usize) -> u32 {
        let mut i = at;
        while i > 0 && matches!(self.toks.get(i - 1), Some(Tok::Newline)) {
            i -= 1;
        }
        if i == 0 {
            return self.cur_line();
        }
        self.lines.get(i - 1).copied().unwrap_or_else(|| self.cur_line())
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
            // Waiting for a term and nothing came. bash names the token it did
            // not get, rather than reporting a missing `]]` — that message is
            // reserved for an expression that *was* complete. Built by
            // `cond_operand_error` because bash builds it in the same place: the
            // `else` at the foot of `cond_term` is reached both from here, where
            // `cond_skip_newlines` hands back the token that begins a term, and
            // from the operand slots below. Which of its forms this is depends
            // on whether the stream merely ended or the lexer cut it short.
            return Err(self.cond_operand_error(CondPos::Primary));
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
            // bash's `cond_term` saves `line_number` the moment it has read the
            // `(` and reports its own `expected \`)'` there, however far down
            // the failure inside is.
            //
            // "The moment it has read it" is not the same as the line the `(`
            // sits on. `(` is a `shellmeta`, so `read_token` peeks the next
            // character with `shell_getc (1)` — continuation removal *on* — to
            // see whether this is `((`, and that peek deletes any `\<newline>`
            // written flush against the paren, `line_number++` and all, before
            // pushing the character back. So a group whose `(` is the last thing
            // on its line is reported a line further down than one that has
            // anything at all after it:
            //
            // ```text
            // echo 1⏎[[ (\⏎      line 3: expected `)'
            // echo 1⏎[[ ( (\⏎    line 3: expected `)'   (the inner group)
            //                    line 2: expected `)'   (the outer one)
            // ```
            let open = self.reader_line();
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
                        tail: Some(b"syntax error: unexpected end of file".to_vec()),
                    });
                }
                let tok = self.token_display();
                // The `near` line is the reader's to give, so a group whose
                // failing token emptied the input line loses it and keeps the
                // rest: `echo 1⏎[[ ( a ;\⏎` still reports the token and still
                // reports the group's `expected \`)'` on the `(`'s line, then
                // prints a bare `syntax error`. See [`Parser::cond_sequel_at`].
                let tail = if self.reader_line_empty_at(self.pos) {
                    b"syntax error".to_vec()
                } else {
                    bfmt![b"syntax error near `", &tok, b"'"]
                };
                return Err(CondError::Cond {
                    clauses: vec![(
                        Some(open),
                        bfmt![b"unexpected token `", &tok, b"', expected `)'"],
                    )],
                    tail: Some(tail),
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
            return Err(CondError::new(
                b"conditional binary operator expected".as_slice(),
                &self.cond_sequel_at(self.pos),
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
            return Err(CondError::new(
                &bfmt![
                    b"unexpected token `",
                    tok,
                    b"', conditional binary operator expected"
                ],
                &self.cond_sequel(),
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
            return Err(CondError::new(
                &bfmt![
                    b"unexpected token `",
                    tok,
                    b"', conditional binary operator expected"
                ],
                &self.cond_sequel_at(self.pos),
            ));
        }
        // The end of input is a token in this position like any other. bash's
        // `cond_term` asks `read_token` for it, is handed `yacc_EOF`, and
        // rejects it through the same arm a newline goes through —
        // `error_token_from_token` can name that one, so the clause carries
        // `` `EOF' ``. What it does *not* get is a `near` line: the fetch that
        // discovered the end left `shell_input_line` empty, so
        // `report_syntax_error` falls past both `near` branches to its
        // end-of-file one (parse.y:6273).
        //
        // ```text
        // echo 1⏎[[ a\⏎   line 4: unexpected token `EOF', conditional binary
        //                         operator expected
        //                 line 4: syntax error: unexpected end of file
        // ```
        //
        // This is not the same as a conditional that is *complete* and merely
        // unclosed: `[[ a == b\⏎` wanted nothing more than `]]` and gets
        // ``unexpected EOF while looking for `]]'`` from `parse_cond` instead,
        // on the line the reader gave up rather than on the end-of-file line.
        if self.peek().is_none() {
            return Err(CondError::new(
                b"unexpected token `EOF', conditional binary operator expected".as_slice(),
                b"syntax error: unexpected end of file".as_slice(),
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

    /// The token position a conditional error is reported from.
    ///
    /// bash names the last *word* it read, which is the offending token itself
    /// whenever that is a real word (`[[ -n ]]` reports near `]]`). A newline
    /// never becomes that token, so an error on one is reported near whatever
    /// came before it: `[[ a` reports near `a`, and `[[ a -eq` near `-eq`.
    fn cond_near_pos(&self) -> usize {
        let mut pos = self.pos;
        if matches!(self.peek(), Some(Tok::Newline)) {
            // Walk back over any newlines to the last real token. That is where
            // bash's own scan lands too: its input line ends at the newline, so
            // stepping back off the end skips it and any space before it.
            while pos > 0 && matches!(self.toks.get(pos), None | Some(Tok::Newline)) {
                pos -= 1;
            }
        }
        pos
    }

    /// The second `parser_error` of a conditional failure: `report_syntax_error`
    /// with `current_token` set to the `-1` `read_token` returned after
    /// `cond_error` (parse.y:3402), which `error_token_from_token` cannot name.
    ///
    /// That drops straight past the first branch to the one gated on
    /// `shell_input_line && *shell_input_line` (parse.y:6273). With text left on
    /// the line bash finds an offending token in it and reports `syntax error
    /// near \`X'` — and echoes the line, which `format_parse_error` keys off the
    /// `near`. With the line emptied it falls to the final `else` and prints a
    /// bare `syntax error`, with nothing under it:
    ///
    /// ```text
    /// echo 1⏎[[ a == b )⏎     line 2: syntax error near `)'
    ///                         line 2: `[[ a == b )'
    /// echo 1⏎[[ a == b )\⏎    line 3: syntax error
    /// ```
    ///
    /// `EOF_Reached` is 0 for both, which is why the second is `syntax error`
    /// and not `syntax error: unexpected end of file`: the conditional died on a
    /// token it *had*, and nothing ever asked for one past the close. The slots
    /// that do run out of input are handled before this is reached.
    ///
    /// Note the emptiness is the *reader's*, not the grammar's: it is decided by
    /// where the offending token left the reader, so the very same conditional
    /// keeps its `near` line when anything at all follows the backslash —
    /// `[[ a == b ) \` (a space in between) stops on the space and reports near
    /// `)`, on the `[[`'s own line.
    fn cond_sequel_at(&self, pos: usize) -> Str {
        if self.reader_line_empty_at(pos) {
            return b"syntax error".to_vec();
        }
        bfmt![b"syntax error near `", self.cond_near_at(pos), b"'"]
    }

    /// Whether the reader has run the input out by the time it finished the
    /// token at `pos`, leaving `shell_input_line` empty. See
    /// [`Spans::reader_line_empty`].
    fn reader_line_empty_at(&self, pos: usize) -> bool {
        self.spans.reader_line_empty(pos, Reader::of(self.toks.get(pos)))
    }

    /// [`Parser::cond_sequel_at`] for the current position, with
    /// [`Parser::cond_near_pos`]'s walk back over newlines.
    fn cond_sequel(&self) -> Str {
        self.cond_sequel_at(self.cond_near_pos())
    }

    /// The text a conditional error is reported "near", for a known token position: the source slice bash
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
    /// line (handled by `format_parse_error`); at end of input the token is
    /// `yacc_EOF`, which bash names `EOF` and reports without a `near` line.
    fn cond_operand_error(&self, pos: CondPos) -> CondError {
        // A stream the *lexer* cut short is not a stream that ended. bash's scan
        // bails to the parser with `-1` rather than aborting (see
        // [`ParseError::bail_sequel`]), so `cond_term` is handed a token after
        // all — one `error_token_from_token` cannot name, which is why the two
        // operator forms lose their `` `X' `` and the primary form spells it
        // `%c` of `-1`: the single byte 0xFF. None of the three gets a `near`
        // line, because `COND_RETURN_ERROR` returns straight out and
        // `parse_cond_command` prints nothing more once `cond_token` is
        // `COND_ERROR` (parse.y:4574). Reported where the failed scan left
        // `line_number` — one past the input's last line, not where the operator
        // was.
        if let Some(at) = self.truncated_at.filter(|_| self.peek().is_none()) {
            return match pos {
                CondPos::Primary => CondError::sequel(
                    at,
                    &bfmt![b"unexpected token `", [0xFF].as_slice(), b"' in conditional command"],
                ),
                CondPos::Unary => CondError::sequel(
                    at,
                    b"unexpected argument to conditional unary operator".as_slice(),
                ),
                CondPos::Binary => CondError::sequel(
                    at,
                    b"unexpected argument to conditional binary operator".as_slice(),
                ),
            };
        }
        if self.peek().is_none() {
            // End of input is a token bash can name, so all three slots name it.
            // `cond_term` calls `read_token`, is handed `yacc_EOF`, and each of
            // the three arms spells it the way it spells any other token it
            // rejected there. None of them gets a `near` line: the fetch that
            // found the end left `shell_input_line` empty and
            // `report_syntax_error` falls to its end-of-file branch.
            //
            // ```text
            // echo 1⏎[[ a ==\⏎   unexpected argument `EOF' to conditional
            //                    binary operator
            // echo 1⏎[[ -f\⏎     unexpected argument `EOF' to conditional
            //                    unary operator
            // echo 1⏎[[ a &&\⏎   unexpected token `EOF' in conditional command
            // ```
            let eof = "syntax error: unexpected end of file";
            return match pos {
                CondPos::Primary => {
                    CondError::new("unexpected token `EOF' in conditional command", eof)
                }
                CondPos::Unary => {
                    CondError::new("unexpected argument `EOF' to conditional unary operator", eof)
                }
                CondPos::Binary => {
                    CondError::new("unexpected argument `EOF' to conditional binary operator", eof)
                }
            };
        }
        let tok = self.token_display();
        // A newline never becomes the token bash reports "near", so an operand
        // slot that a line end walked into names the operator instead.
        let near = self.cond_sequel();
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
    ///
    /// What that lookahead costs is the *reader's* line and not the token's own:
    /// a `\⏎` written flush after the second token is deleted by the scan that
    /// finds the token has ended, and the deletion moves `line_number` before the
    /// token is ever handed over. So `echo L$LINENO\` at the end of a file
    /// reports 3 out of two lines. See [`Parser::reader_line_at`], which also
    /// covers the case where the lookahead is not there at all.
    fn simple_command_line(&self) -> u32 {
        let leading_assignment = match self.toks.get(self.pos) {
            Some(Tok::ArrayAssign { .. }) => true,
            Some(Tok::Word(segs)) => word_is_assignment(segs),
            _ => false,
        };
        if leading_assignment {
            return self.cur_line();
        }
        self.reader_line_at(self.pos.saturating_add(1))
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
                    // No reserved-word test here. bash recognises one only where
                    // `reserved_word_acceptable (last_read_token)` holds
                    // (parse.y:5367) — after a separator, an operator, or another
                    // reserved word — and never after a `WORD`. An assignment
                    // prefix *is* a `WORD`, and so is a redirection's filename,
                    // so by the time this loop is running a reserved word is an
                    // ordinary one: `v=1 done` and `>/dev/null then` both run a
                    // command of that name. Every position where bash *would*
                    // accept one is reached through [`Parser::reserved_here`],
                    // which is the single place [`RESERVED`] is consulted; the
                    // loop's first iteration is not one of them, because
                    // [`Parser::parse_command`] has already dispatched or
                    // rejected any reserved word standing there.
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
                    // The elements were parsed one at a time and so were given
                    // tails one at a time, but bash keeps the whole literal as a
                    // single word. Re-attach them across it.
                    crate::unparse::attach_compound_comsub_tails(&mut items);
                    // A subscript is parsed verbatim, as the scalar form's is —
                    // see `Self::try_assignment` for why nothing in it is split
                    // or trimmed.
                    let index = match &index {
                        Some(src) => Some(word_subscript_from_source(src, self.opts, Quoting::Bare)?),
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
                // substitution's operand in it is read with the quotes' rules —
                // but by no parser, which is the other half of
                // [`Quoting::Unread`].
                let q = if quoted { Quoting::Bare } else { Quoting::Unread };
                word_from_segs_in(&segs, self.opts, q)
                    .map_err(|e| e.or_echo(self.echo_at(at)))?
            }
            // A `<<`/`<<-` whose delimiter was read is *always* followed by the
            // `HereDoc` token carrying it — that is the lexer's contract. So an
            // ordinary word standing here is not a delimiter and never was: it
            // is the token that stands where the operator's WORD should be, and
            // the redirection has no target. (Reachable only across an alias
            // seam, where the reader's answer came from the value's text: see
            // TD-OILS-A-COMMENT-IN-AN-ALIAS-VALUE-DOES-NOT-EAT-THE-CALLING-LINE.)
            Some(Tok::Word(segs)) if op != RedirectOp::HereDoc => {
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
                    let idx = word_subscript_from_source(idx_src, self.opts, Quoting::Bare)?;
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
        // How many brackets the scan below stands inside. `[`/`]` nest — bash
        // finds the close with `skipsubscript` (general.c's `assignment`), never
        // with the first `]` it meets — so `c[b[$i]]=R` is an assignment to
        // `c[b[$i]]` and not a command named `c[b[1]]=R`.
        let mut depth = 1usize;
        // The close cannot be in this seg: [`balanced_subscript_end`] counts the
        // same brackets over the same bytes and reported none, which is why this
        // function was called at all. What the seg *can* hold is further opens
        // (`c[b[` stands at depth 2), which is the whole reason for the call.
        let closed_here = subscript_close_in_lit(after_open, &mut depth);
        debug_assert!(closed_here.is_none());
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
            // Only a literal run can carry the close: a `'…'`, a `"…"`, a
            // `$(…)` and a `${…}` are each one segment, and `skipsubscript`
            // steps over each of them whole — `p["b[1]"]=R` keys on `b[1]`.
            if let Seg::Lit(s) = seg
                && let Some(close) = subscript_close_in_lit(s, &mut depth)
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
        // The subscript arrived as segments rather than as source text, but it
        // is the same subscript and wants the same second reading — a `'` in it
        // is not a quote once it reaches arithmetic. See
        // [`attach_subscript_reads`].
        let mut index = self.word_from_segs_at(&sub_segs, self.pos)?;
        attach_subscript_reads(&mut index, self.opts, Quoting::Bare, RUNTIME_TEXT)?;
        Ok(Some(Assignment {
            name,
            index: Some(index),
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
        let index = word_subscript_from_source(
            first.get(1..close).unwrap_or_default(),
            opts,
            Quoting::Bare,
        )?;
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
/// delimiter is the same context — but not the same *read*, which is what
/// [`Quoting::Unread`] is for.
///
/// The patterns, replacements and subscripts beside the operand are read *bare*
/// either way, because a pattern is expanded by `getpattern`, which throws the
/// enclosing context away before it starts:
///
/// ```text
///   pat = expand_string_for_pat (value,
///           (quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) ? Q_PATQUOTE : quoted,
///           (int *)NULL, (int *)NULL);        /* subst.c:5751-5754 */
/// ```
///
/// But *bare* is only half of what this carries. The other half is whether a
/// parser ever read the text, and that half does reach the patterns — so the
/// four states below are two independent bits, not a ladder.
///
/// The two halves have separate causes, and the ANSI-C one is **not**
/// `getpattern`: `expand_word_internal` translates no `$'…'` at all, which is
/// the whole reason bash carries a separate `expand_string_dollar_quote` "for
/// code paths that don't do it" (subst.c:4171-4172). Translation is the
/// *reader's*, and a here-document body had no reader. bash puts it back for
/// exactly one span — the fragment after a pattern-ish operator, which
/// `parameter_brace_expand` re-extracts with `SX_POSIXEXP` when the operator is
/// `#`, `%`, `/`, `^`, `,` or a substring `:` (subst.c:9913) — because such an
/// extraction inside a here-document is routed to
/// `extract_heredoc_dolbrace_string` (subst.c:1828-1832), a function that exists
/// "to handle `$'...'` and `$"..."` quoting in here-documents, since the
/// here-document read path doesn't" (subst.c:1522-1530). `:-`, `:+`, `:=`, `:?`
/// and their `:`-less forms are not on that list — the `:` is consumed as the
/// null-check before `c` is read — so an operand keeps its text.
///
/// Measured in a here-document body with `v=$'a\tb'`: `${v#$'a\tb'}` and
/// `${v%$'a\tb'}` trim to nothing and `${v/$'a\tb'/X}` gives `X`, while
/// `${nope:-$'a\tb'}`, `${nope-$'a\tb'}`, `${v:+$'a\tb'}` and `${nope2:=$'a\tb'}`
/// all print `$'a\tb'` back. The split holds one level down too —
/// `${v#${z:-$'a\tb'}}` trims — because `extract_heredoc_dolbrace_string` scans
/// the whole fragment with `dolbrace_state` pinned at `DOLBRACE_QUOTE` (its only
/// transitions leave `DOLBRACE_PARAM`, which it never reaches), so a nested
/// operand is translated along with the pattern around it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quoting {
    Bare,
    Dquote,
    /// A double-quoted run in text **no parser read as a word** — a
    /// here-document body with an unquoted delimiter, a `PS4`, a `${x@P}`.
    ///
    /// Quoting-wise it is [`Quoting::Dquote`] and is treated as such everywhere
    /// that asks about quotes. What differs is that `read_token_word` never ran
    /// over it (a here-document body goes from `read_secondary_line` straight
    /// into `make_here_document`, make_cmd.c:621), so nothing in it was
    /// translated at parse time and no delimiter was pushed for it: a `$'…'` or
    /// `$"…"` in an operand here stays as written, and a `$( … )` in it is
    /// [`crate::ast::CmdSubBody::Unread`]. See [`crate::lexer::Lexer::here_text`],
    /// which is what this variant selects.
    Unread,
    /// [`Quoting::Unread`]'s read with [`Quoting::Bare`]'s quoting: a pattern,
    /// replacement, subscript or substring offset of a `${ … }` that itself sits
    /// in unread text. `getpattern` has dropped the double-quoting and
    /// `extract_heredoc_dolbrace_string` puts the ANSI-C translation back — so a
    /// `$'…'` here *is* translated — but nothing gives the text back a parse it
    /// never had, so a `$( … )` here is still
    /// [`crate::ast::CmdSubBody::Unread`], read at expansion time by
    /// `extract_command_subst`.
    BareUnread,
    /// A string handed straight to `expand_word_internal` with no quoting at
    /// all: a **runtime array subscript**, the `sub` of an already-expanded
    /// `name[sub]` that reached `unset`, `[ -v ]`, `[[ -v ]]`, `printf -v`,
    /// `read` or a `declare -n` target as a value.
    ///
    /// Quoting-wise it is [`Quoting::Bare`] — bash expands it with
    /// `expand_subscript_string (sub, 0)` for an associative key
    /// (arrayfunc.c:1145) and `expand_arith_string (exp,
    /// Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)` for an index (arrayfunc.c:1354), and
    /// quote *removal* runs either way, so `m['x y']` and `m[x y]` name the same
    /// key. Read-wise it is [`Quoting::Unread`]: no parser saw it, so a `$( … )`
    /// in it is [`crate::ast::CmdSubBody::Unread`].
    ///
    /// What it has that [`Quoting::BareUnread`] does not is that **no reader ever
    /// translates it, at any depth** — there is no here-document here, so the
    /// `SX_POSIXEXP` re-extraction that gives a here-document's pattern its
    /// ANSI-C back never runs. Measured with `declare -A m; m[$'a\tb']=TAB` and
    /// `v=$'a\tb'`: `[[ -v "m[\$'a\tb']" ]]` is false, because the `$` stays a
    /// `$` and the `'…'` is an ordinary single-quoted string that quote removal
    /// then strips — the key named is `$a\tb`. And `[[ -v
    /// 'm[${v#$'\''a\tb'\''}]' ]]` is *true*, the nested pattern having been
    /// untranslated too and so matched nothing. Hence [`Self::as_pattern`]
    /// leaves this state alone.
    Runtime,
}

impl Quoting {
    /// Whether a double-quoted run is in force — the half that decides how a
    /// character is spelled.
    fn dquoted(self) -> bool {
        matches!(self, Self::Dquote | Self::Unread)
    }

    /// Whether the text was read by no parser — the half that decides whether a
    /// `$( … )` has a parse. See [`crate::lexer::Lexer::here_text`].
    fn unread(self) -> bool {
        matches!(self, Self::Unread | Self::BareUnread | Self::Runtime)
    }

    /// The pair the lexer needs: was this read, and was it translated. See
    /// [`crate::lexer::ReadCtx`].
    fn read_ctx(self) -> crate::lexer::ReadCtx {
        crate::lexer::ReadCtx {
            unread: self.unread(),
            // Every state but the two value-shaped ones was translated by
            // whichever reader produced it — the parse for source, the
            // `SX_POSIXEXP` re-extraction for a here-document's fragment.
            ansi_c: !matches!(self, Self::Unread | Self::Runtime),
        }
    }

    /// This read, with the quoting a *pattern* beside the operand is read with:
    /// `getpattern` drops the `Q_DOUBLE_QUOTES`/`Q_HERE_DOCUMENT` bits
    /// (subst.c:5751-5754) and, in a here-document, the `SX_POSIXEXP` extraction
    /// restores the ANSI-C translation the text never got (subst.c:1828-1832) —
    /// but nothing restores the parse. A [`Quoting::Runtime`] string is in no
    /// here-document, so it has no second reader to gain and stays as it is.
    fn as_pattern(self) -> Self {
        match self {
            Self::Runtime => Self::Runtime,
            _ if self.unread() => Self::BareUnread,
            _ => Self::Bare,
        }
    }

    /// This read, for text the parser stepped *over* rather than through — the
    /// interior of a `' … '` in a subscript, which `parse_matched_pair` skips
    /// from the quote to its mate. Whatever the quoting around it was, nothing
    /// in here has a parse, so a `$( … )` is
    /// [`crate::ast::CmdSubBody::Unread`]. See
    /// [`word_subscript_from_source_at`].
    fn as_unread(self) -> Self {
        match self {
            Self::Runtime => Self::Runtime,
            Self::Dquote | Self::Unread => Self::Unread,
            Self::Bare | Self::BareUnread => Self::BareUnread,
        }
    }
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
        seg_to_parts(s, opts, q, &mut parts)?;
    }
    let mut word = Word { parts };
    // Every `$( … )` in the word learns what follows it here, because this is
    // the first point that knows. See [`crate::unparse::attach_comsub_tails`].
    crate::unparse::attach_comsub_tails(&mut word);
    // Only a bare splice can have written text into the word's buffer that the
    // parser did not read out of the source, so only a word that had one pays
    // for its re-print to be built and looked at.
    if segs_hold_a_nul(segs) || segs_splice_past_the_brace(segs) {
        word = word_expanded_from_its_text(word, segs);
    }
    Ok(word)
}

/// Whether the token's text carries a NUL byte, asked of the segments so that
/// the word that does not — every word anyone ever writes — pays a byte scan
/// rather than a re-print.
///
/// A `$( … )` body is the one part of a token that is *not* this token's text:
/// bash parses it with a reader of its own and splices back `print_comsub`'s
/// answer (parse.y:4219–4241), so its words are cut by that parse and what
/// arrives here is already a C string. osh reaches the same place from the other
/// side — the body is re-lexed by [`parse_cmdsub_body`], and the cut happens on
/// *its* words — so the body must not also cut the word holding it.
fn segs_hold_a_nul(segs: &[Seg]) -> bool {
    segs.iter().any(|s| match s {
        Seg::Lit(t) | Seg::Sq { text: t, .. } | Seg::ParamBraced(t, ..) | Seg::Arith(t, ..) => {
            t.contains(&0)
        }
        Seg::Dq(inner, _) => segs_hold_a_nul(inner),
        // An unclosed construct's text is echoed back by a diagnostic rather
        // than expanded, and the diagnostic writes bytes rather than a C string.
        Seg::Param(_) | Seg::CmdSub(..) | Seg::ProcSub(..) | Seg::Unclosed(_) => false,
    })
}

/// Whether a bare splice wrote a `}` that closes the `${ … }` earlier than the
/// parser did — the second way the splice makes the word's text disagree with
/// the tree, asked of the segments for the same reason
/// [`segs_hold_a_nul`] is.
///
/// Where it does, everything after that `}` is ordinary word text, and the
/// quotes in it are the *word's* quotes: a spliced `"` closes the run the
/// `${ … }` was written in, and may go on to swallow the run's own closing
/// quote and run into the word's tail. No split of the segment models that,
/// which is why the whole word goes back to being text — see
/// [`WordPart::TokenText`].
fn segs_splice_past_the_brace(segs: &[Seg]) -> bool {
    segs.iter().any(|s| match s {
        // The splice only happens inside double quotes, so the scan that
        // decides where the expansion ends is the double-quoted one.
        Seg::ParamBraced(raw, _, _, spliced) if !spliced.is_empty() => {
            matches!(crate::wordscan::expansion_body_len(raw, true), BraceEnd::Early(_))
        }
        Seg::Dq(inner, _) => segs_splice_past_the_brace(inner),
        _ => false,
    })
}

/// Hand the word back as the text of its own token buffer, for the expander to
/// read — see [`WordPart::TokenText`].
///
/// The cut is bash's `make_word`: `read_token_word` accumulates a token into a
/// byte buffer with an explicit length and then calls `make_word (token)`, which
/// copies it with `savestring` — one `strlen` and the rest of the buffer is
/// gone. This is the *token* boundary and nowhere else: a `${ … }` operand, a
/// pattern, a subscript are all carved back out of the word's text afterwards,
/// so applying the cut to them separately would cut words bash never cut. The
/// word's *extent* is not affected either: everything the scan consumed it still
/// consumed, and the next word starts where it always did.
///
/// A word whose text holds no NUL after all keeps its tree unless the other
/// producer applies, so the byte scan above staying a scan costs nothing.
fn word_expanded_from_its_text(word: Word, segs: &[Seg]) -> Word {
    let src = crate::unparse::word_src(&word);
    match src.iter().position(|&b| b == 0) {
        Some(nul) => Word {
            parts: vec![WordPart::TokenText(src.get(..nul).unwrap_or_default().to_vec())],
        },
        None if segs_splice_past_the_brace(segs) => {
            Word { parts: vec![WordPart::TokenText(src)] }
        }
        None => word,
    }
}

/// Parse the substitution bodies an arithmetic or `${ … }` scan stepped over,
/// and return each one's re-print beside the range of the scan's text it
/// replaces.
///
/// bash parses them where it meets them — `parse_matched_pair` under `P_ARITH`
/// sends a `$(` to `parse_dollar_word` and from there to `parse_comsub`
/// (parse.y:3931, 3959). What `APPEND_NESTRET` then splices into the scan's
/// buffer is not the source: `parse_comsub` ends
/// `tcmd = print_comsub (parsed_command); … return ret` (parse.y:4219–4241), so
/// the source is thrown away and the parse *re-printed* in its place. That
/// re-print is what a diagnostic quoting the string back shows, and what the
/// body is read from when the expansion runs.
///
/// The parse's other lasting effect is its error, and that error belongs to the
/// enclosing unit: `echo $(( 1 + $(fi) ))` never reaches the arithmetic
/// evaluator, and `if false; then echo $(( 1 + $(fi) )); fi` dies with the
/// branch untaken.
/// A span the scan met in text no parser read is not parsed here at all — there
/// is no eager parse to make an error out of and no re-print to splice. It is
/// read when the text is expanded instead, and reaches that read through
/// [`arith_unread_subs`].
fn parse_arith_comsubs(
    nested: &[CmdSubSpan],
    opts: ParseOpts,
) -> Result<Vec<(core::ops::Range<usize>, Str)>, ParseError> {
    let mut out = Vec::with_capacity(nested.len());
    for sub in nested {
        if sub.kind != SubBody::Eager {
            continue;
        }
        // A process substitution met by the same scan is parsed by the same
        // rule and re-printed into the same buffer — only from its *opening*
        // line, and by the parser that ends a body on a `)` that is not a
        // `list_terminator`. See [`SubOpen`].
        let prog = match sub.open {
            SubOpen::Dollar => parse_cmdsub_body(&sub.src, sub.close_line, opts)?,
            SubOpen::Proc { open_line, .. } => parse_procsub_body(&sub.src, open_line, opts)?,
        };
        out.push((sub.range.clone(), crate::unparse::comsub_reprint(sub.open.delim(), &prog)));
    }
    Ok(out)
}

/// The arithmetic text `expr` cut into the parts the *expansion-time* scan walks
/// it as: literal runs, and one [`CmdSubBody::Unread`] command substitution for
/// each `$( … )` the scan will read there.
///
/// bash reads them from inside the arithmetic's own extent read —
/// `extract_delimited_string` carries `SX_COMMAND` and hands a nested `$(` to
/// `extract_command_subst` over the *whole enclosing string* (subst.c:1431-1437)
/// — so the body is parsed a second time there, and when it will not parse the
/// diagnostic quotes the enclosing string's remainder, not the expression's.
/// Rendering them back as parts is what gets both: the scan
/// ([`crate::interp::Shell::brace_scanned_subs`]) can reach them, and
/// [`crate::unparse::attach_comsub_tails`] measures each one's remainder by
/// rendering the whole word around it.
///
/// The parts render back to exactly `expr` — an unread body is printed as its
/// source, there being no re-print — which is the invariant
/// [`crate::ast::WordPart::ArithSub::parts`] states.
///
/// A span the parser *did* read eagerly contributes no part: its second read is
/// of the re-print already spliced into `expr`, and that read is the arithmetic
/// expansion's own (see `Shell::expand_arith_params`), not the scan's.
///
/// Both arithmetic spellings get parts, because the parts are what any *enclosing*
/// scan walks: a `${ … }` around either one reads every `$( … )` it passes,
/// however deeply spelled. Which of the two scans its own parts is the
/// arithmetic's question, answered in `Shell::arith_extent_scan`.
fn arith_unread_subs(expr: &Str, nested: &[CmdSubSpan]) -> Vec<WordPart> {
    // Only the `$( … )` spelling. A `<( … )` reaches this collection from a
    // `${ … }` the arithmetic scan stepped over, and the expansion-time scan
    // does not recurse into one with a parse the way `extract_command_subst`
    // does — it is text there, and stays text here. See [`SubOpen`].
    let mut spans: Vec<&CmdSubSpan> = nested
        .iter()
        .filter(|s| matches!(s.kind, SubBody::Unread { .. }) && matches!(s.open, SubOpen::Dollar))
        .collect();
    if spans.is_empty() {
        return vec![WordPart::Literal(expr.clone())];
    }
    spans.sort_by_key(|s| s.range.start);
    let mut parts = Vec::with_capacity(spans.len() * 2 + 1);
    let mut at = 0usize;
    for s in spans {
        // A range that does not fit is not something a correct scan produces;
        // skipping it keeps the text whole rather than corrupting it, at the
        // cost of one substitution the scan will not see.
        if s.range.start < at || s.range.end > expr.len() {
            continue;
        }
        let SubBody::Unread { closed, .. } = s.kind else { continue };
        parts.push(WordPart::Literal(expr.get(at..s.range.start).unwrap_or_default().to_vec()));
        parts.push(WordPart::CommandSub {
            body: CmdSubBody::Unread {
                // The spans this walk keeps are filtered to `SubOpen::Dollar`
                // above, so the spelling is never in doubt here.
                delim: SubDelim::Dollar,
                src: s.src.clone(),
                // Filled by `unparse::attach_comsub_tails`, once the word this
                // arithmetic sits in has been assembled.
                tail: Str::new(),
                close_line: s.close_line,
                closed,
            },
        });
        at = s.range.end;
    }
    parts.push(WordPart::Literal(expr.get(at..).unwrap_or_default().to_vec()));
    parts
}

/// The re-prints of the *process* substitutions a `${ … }` scan stepped over —
/// [`parse_arith_comsubs`] restricted to the `<( … )` / `>( … )` spelling.
///
/// The two spellings need separating because only one of them is read a second
/// time. A `$( … )` in a `${ … }` body is met again by the re-lex that carves
/// the operand, pattern or subscript out of the body's text, and the part that
/// re-lex builds re-prints itself; splicing the re-print into the text as well
/// would gather a nested here-document twice. A `<( … )` is not met again —
/// `read_word_verbatim` leaves it as characters on purpose, since osh decides
/// at lex time whether a process substitution is live and none of the fragments
/// that re-lex is one bash performs it in — so for that spelling the splice
/// here is the only thing that carries the parse into the body's text.
///
/// # Errors
/// Returns the first body's [`ParseError`], which is the enclosing unit's:
/// `echo "${z:-<(fi)}"` is a syntax error at `fi`.
fn procsub_reprints(
    nested: &[CmdSubSpan],
    opts: ParseOpts,
) -> Result<Vec<(core::ops::Range<usize>, Str)>, ParseError> {
    let mut out = Vec::new();
    for sub in nested {
        let SubOpen::Proc { open_line, .. } = sub.open else { continue };
        if sub.kind != SubBody::Eager {
            continue;
        }
        let prog = parse_procsub_body(&sub.src, open_line, opts)?;
        out.push((sub.range.clone(), crate::unparse::comsub_reprint(sub.open.delim(), &prog)));
    }
    Ok(out)
}

/// [`splice_reprints`], carrying a set of ranges measured against the same text
/// across the change.
///
/// The ranges are the body's [bare splices](crate::lexer::Lexer::bare_splices) —
/// stretches no parser read — and a re-print that is not the length of the
/// source it replaces moves every one of them that sits after it. Nothing can
/// sit *inside* a re-printed span (a substitution found in spliced text is not
/// recorded, that text having been written rather than read), so each range
/// only ever moves as a whole.
///
/// Returns the text and ranges untouched when there is nothing to splice, which
/// is the overwhelmingly common case.
fn splice_reprints_tracking<'a>(
    text: &'a Str,
    mut reprints: Vec<(core::ops::Range<usize>, Str)>,
    splices: &'a [core::ops::Range<usize>],
) -> (Cow<'a, Str>, Cow<'a, [core::ops::Range<usize>]>) {
    if reprints.is_empty() {
        return (Cow::Borrowed(text), Cow::Borrowed(splices));
    }
    reprints.sort_by_key(|(r, _)| r.start);
    let mut out = text.clone();
    let mut moved = splices.to_vec();
    // Right to left, so a range still to be spliced is still valid; see
    // [`splice_reprints`].
    for (range, rep) in reprints.into_iter().rev() {
        // A range that does not fit is not something a correct scan can
        // produce; dropping the splice keeps the source text rather than
        // corrupting it.
        if range.start > range.end || range.end > out.len() {
            continue;
        }
        let (at, old, new) = (range.start, range.end - range.start, rep.len());
        out.splice(range, rep);
        if new != old {
            let shift = |p: usize| {
                if p <= at { p } else { (p + new).saturating_sub(old) }
            };
            for s in &mut moved {
                *s = shift(s.start)..shift(s.end);
            }
        }
    }
    (Cow::Owned(out), Cow::Owned(moved))
}

/// Write each re-print from [`parse_arith_comsubs`] back over the text it
/// replaces.
///
/// Right to left, because a re-print is rarely the same length as the source it
/// stands in for and an earlier range has to still be valid once a later one has
/// changed size. The ranges are disjoint (each names one `$( … )`) but are
/// sorted rather than assumed ordered, since they reach here from several scans
/// spliced together.
fn splice_reprints(text: &Str, mut reprints: Vec<(core::ops::Range<usize>, Str)>) -> Str {
    if reprints.is_empty() {
        return text.clone();
    }
    reprints.sort_by_key(|(r, _)| r.start);
    let mut out = text.clone();
    for (range, rep) in reprints.into_iter().rev() {
        // A range that does not fit is not something a correct scan can produce;
        // dropping the splice keeps the source text rather than corrupting it.
        if range.start <= range.end && range.end <= out.len() {
            out.splice(range, rep);
        }
    }
    out
}

/// The body text a `${ … }` kept as *unparsed text* rather than building operand
/// words from it — the verdicts that answer the whole expansion (with a runtime
/// `bad substitution`) without ever looking inside. `None` for every other
/// verdict; a body that failed to parse outright never built operand words
/// either, so the caller treats an `Err` the same way.
///
/// The one shape holds the whole body, exactly the bytes `unparse` puts back
/// between `${` and `}` — which is why the re-print splice can be applied to it
/// directly, with offsets that were measured against that same body.
///
/// A bad `@` **transform** used to be one of these and no longer is: its
/// operand is a word now ([`WordPart::BadTransform`]), so the substitutions in
/// it are parsed with it and re-printed from the parse like any other operand's.
fn deferred_body_mut(part: &mut WordPart) -> Option<&mut Str> {
    match part {
        WordPart::BadSubst(raw) => Some(raw),
        _ => None,
    }
}

/// Lower one segment onto `out` — one part, except for the `${ … }` whose body
/// the *expansion* does not read the way the scan that built it did, which is
/// kept as text.
///
/// bash reads a word twice, and only the second read decides where a `${ … }`
/// ends. The first — `parse_matched_pair` — is looking for the end of the
/// *word*, and everything it consumes on the way is accumulated as text,
/// including a `$'…'` it translates and, inside double quotes with the body
/// still in `DOLBRACE_PARAM`/`OP`/`WORD`, splices back **unquoted**
/// (parse.y:3887). It does not re-read what it wrote, so a `}` the translation
/// contributed never terminates anything there. `parameter_brace_expand` then
/// scans the finished word, meets that `}` like any other, and closes on it —
/// leaving the rest as ordinary word text:
///
/// ```text
/// x=; echo "${x:-$'a}b'}"   →   ab}
/// ```
///
/// the expansion being `${x:-a}` and `b}` a literal that follows it. Neither
/// half of that is a tree the parser can build: the leftover's quotes are the
/// *word's*, so a spliced `"` closes the run the `${ … }` was written in and
/// can go on to swallow that run's own closing quote. The body therefore stays
/// text here, and [`word_from_segs_in`] hands the whole word back as text for
/// the expander to read — see [`WordPart::TokenText`].
fn seg_to_parts(
    seg: &Seg,
    opts: ParseOpts,
    q: Quoting,
    out: &mut Vec<WordPart>,
) -> Result<(), ParseError> {
    if let Seg::ParamBraced(raw, _, nested, spliced) = seg
        && !spliced.is_empty()
        // The splice only happens inside double quotes, so the scan that
        // decides is the double-quoted one.
        && !matches!(crate::wordscan::expansion_body_len(raw, true), BraceEnd::Same)
    {
        // Nothing about the body's *shape* can be asked once the expansion
        // stops reading it the parser's way — whether it closes early
        // (`"${x:-$'a}b'}"`, the word `"${x:-a}b}"`) or never closes at all
        // (`"${x:-$'a"b'}"`, the word `"${x:-a"b}"`, which dies on the missing
        // `}`). Either way the body is kept as text. The `$( … )` in it is
        // still parsed, where bash parsed it.
        let text = splice_reprints(raw, parse_arith_comsubs(nested, opts)?);
        out.push(WordPart::BadSubst(text));
        return Ok(());
    }
    out.push(seg_to_part(seg, opts, q)?);
    Ok(())
}

fn seg_to_part(seg: &Seg, opts: ParseOpts, q: Quoting) -> Result<WordPart, ParseError> {
    Ok(match seg {
        Seg::Lit(s) => WordPart::Literal(s.clone()),
        Seg::Sq { text, escaped, closed } => WordPart::SingleQuoted {
            text: text.clone(),
            escaped: *escaped,
            closed: *closed,
            // Filled in afterwards, and only for a subscript or a substring
            // bound. See [`word_subscript_from_source_at`].
            parts: None,
        },
        Seg::Dq(inner, closed) => {
            let mut parts = Vec::with_capacity(inner.len());
            for s in inner {
                seg_to_parts(s, opts, Quoting::Dquote, &mut parts)?;
            }
            WordPart::DoubleQuoted { parts, closed: *closed }
        }
        Seg::Param(n) => WordPart::Param { name: n.clone(), braced: false },
        Seg::Unclosed(u) => WordPart::Unclosed(u.clone()),
        // The body is lexed again in here, from its own line 1, so every
        // fragment of it has to be told the physical line it starts on — see
        // [`frag_line`] and [`map_frag_segs`].
        Seg::ParamBraced(raw, open, nested, spliced) => {
            // A process substitution the scan stepped over is parsed here and
            // its re-print written over the source, because nothing downstream
            // will do either — see [`procsub_reprints`]. So
            // `echo "${z:-<(fi)}"` is a syntax error at `fi` rather than a
            // brace body holding the text `<(fi)`, and
            // `f() { echo "${z:-<(echo   hi)}"; }` prints back with the run of
            // spaces gone.
            let (raw, spliced) =
                splice_reprints_tracking(raw, procsub_reprints(nested, opts)?, spliced);
            let (raw, spliced) = (&*raw, &*spliced);
            let mut part = parse_braced_param_in(raw, opts, q, *open, spliced);
            // A `$( … )` in the body is parsed by bash where it *reads* it, so
            // its syntax error beats every verdict the `${ … }` could reach —
            // a runtime `bad substitution`, an outright refusal of the body's
            // shape, and the branch never being taken. On the paths that do
            // build operand words the body's substitutions are parsed with
            // them, and parsing here as well would gather a nested
            // here-document twice; so this runs only where they are not.
            if part.as_mut().map_or(true, |p| deferred_body_mut(p).is_some()) {
                // bash splices the re-print into the `${ … }` body during the
                // scan that produces it (parse.y:3929 → 3959), so a body kept
                // as text carries the re-print rather than the source: both
                // `declare -f` and the runtime `bad substitution` quote
                // `${#x:-$( ( echo 2 ))}` where the source said
                // `$( (echo 2) )`, and a compound command makes that
                // diagnostic span lines.
                //
                // The body's *lines* stay on the source even so — `frag_line`
                // is fed the unspliced `raw` above, deliberately. Measured
                // against bash 5.2.37: a fragment sitting after a
                // substitution that re-prints to three lines where the source
                // had one is still blamed to its physical line, so the text
                // and the line accounting are two coordinate systems and bash
                // keeps them apart. See `known-issues.md`,
                // TD-OILS-A-DEFERRED-BRACE-BODY-KEEPS-ITS-SUBSTITUTION-AS-WRITTEN.
                let spliced = splice_reprints(raw, parse_arith_comsubs(nested, opts)?);
                let mut part = part?;
                if let Some(text) = deferred_body_mut(&mut part) {
                    *text = spliced;
                }
                return Ok(part);
            }
            part?
        }
        // Only the `$( … )` spelling is parsed here: bash reads the other two
        // when the word is expanded, as an input of their own. See
        // [`CmdSubBody`].
        Seg::CmdSub(raw, close_line, body) => WordPart::CommandSub {
            body: match body {
                // No parser read the text this one was written in, so there is
                // no eager parse to keep and no re-print to run in place of the
                // source. The `tail` is filled by
                // `unparse::attach_comsub_tails` once the word is assembled,
                // exactly as a `Parsed` body's is.
                // A `Seg::CmdSub` is the `$( … )` spelling everywhere a parser
                // read the text, because the other two are lowered to a live
                // [`WordPart::ProcSub`] there. In *unread* text they are not
                // — nothing performs one, and the segment records which
                // delimiter wrote it so the part can print and stand as its
                // own source. See [`SubDelim`].
                SubBody::Unread { closed, delim } => CmdSubBody::Unread {
                    delim: *delim,
                    src: raw.clone(),
                    tail: Str::new(),
                    close_line: *close_line,
                    closed: *closed,
                },
                SubBody::Backtick(verbatim) => CmdSubBody::Backtick {
                    src: raw.clone(),
                    verbatim: verbatim.clone(),
                    // Only the gobbler's copy of the word wants this, and only
                    // it fills it — see [`CmdSubBody::Backtick::tail`].
                    tail: Str::new(),
                },
                // Collected by the same `P_ARITH` scan, so its nested bodies
                // are re-printed into it the same way — the classification
                // that sent this text back to a command substitution happens
                // afterwards and does not undo the splice.
                SubBody::ArithFallback(nested) => CmdSubBody::ArithFallback {
                    src: splice_reprints(raw, parse_arith_comsubs(nested, opts)?),
                    // Filled by `unparse::attach_comsub_tails` once the word is
                    // assembled, as every other extent-reading part's is.
                    tail: Str::new(),
                },
                SubBody::Eager => {
                    // The eager parse is kept — it is what found the `)` and
                    // what raises the fatal syntax error — but the text that
                    // *runs* is its re-print, not `raw`. bash disposes this
                    // parse and keeps only `print_comsub`'s answer
                    // (parse.y:4219–4233), which is what `command_substitute`
                    // reads back when the word is expanded. The same rule the
                    // arithmetic scan follows two arms down, for the same
                    // reason.
                    let prog = parse_cmdsub_body(raw, *close_line, opts)?;
                    let src = crate::unparse::comsub_body(&prog);
                    // The tail is a property of the *word*, not of this part, so
                    // it is filled by `unparse::attach_comsub_tails` once the
                    // whole word is assembled — a segment cannot see its own
                    // siblings from here. It stays `None` for a word the shell
                    // builds at expansion time (`${x@P}`, `PS4`), which bash
                    // reads once and never re-prints; see
                    // [`CmdSubBody::Parsed::tail`].
                    CmdSubBody::Parsed { prog, src, close_line: *close_line, tail: None }
                }
            },
        },
        Seg::Arith(raw, bracket, nested) => {
            // The string carries the re-print, not what was written: it is
            // `APPEND_NESTRET` that puts `parse_comsub`'s answer into the
            // arithmetic scan's buffer, and there is no copy of the source
            // left afterwards. So `$(( 1 + $(echo a>&2) ))` names
            // `1 + $(echo a 1>&2)` when it fails, and re-reads that text when
            // it runs.
            let expr = splice_reprints(raw, parse_arith_comsubs(nested, opts)?);
            // The same text in parts, which is how the expansion-time scan
            // walks it. A collection is all of one kind (see
            // [`CmdSubSpan::kind`]), so the splice above and this never both
            // have work to do and the ranges cannot have moved under it.
            let parts = arith_unread_subs(&expr, nested);
            // The remainder is filled by `unparse::attach_comsub_tails` once
            // the word is assembled — a segment cannot see its own siblings
            // from here. See [`crate::ast::WordPart::ArithSub::tail`].
            WordPart::ArithSub { expr, bracket: *bracket, parts, tail: Str::new() }
        }
        Seg::ProcSub(input, raw, open_line, read) => WordPart::ProcSub {
            input: *input,
            body: match read {
                ProcRead::Eager => {
                    ProcSubBody::Parsed(parse_procsub_body(raw, *open_line, opts)?)
                }
                // Not parsed here: no parser read the text this was written in,
                // so the read that finds it is the `${ … }` scan's, made later
                // and from where a failure is `bad substitution` rather than a
                // script syntax error. The `tail` is filled by
                // `unparse::attach_comsub_tails` once the word is assembled,
                // exactly as an unread `$( … )` body's is.
                ProcRead::Unread { closed } => ProcSubBody::Unread {
                    src: raw.clone(),
                    tail: Str::new(),
                    closed: *closed,
                },
            },
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
/// If a quoted run or a substitution starts at `chs[i]`, the index just past it;
/// otherwise `None`.
///
/// A `${ … }` body reaches the parser as raw text, but the characters in it are
/// not all at the same level: a `/`, a `]` or a `:` inside a nested `$( … )`,
/// `${ … }`, `` `…` `` or a quoted run belongs to that construct and must not be
/// mistaken for the enclosing body's own operator. bash never has to ask —
/// `parse_matched_pair` recorded where each nested construct ended as it read
/// the body — so this recovers the same structure from the text.
///
/// The scan is deliberately shallow: it finds each construct's *extent* and
/// nothing else, which is all a split needs. The halves are lexed properly
/// afterwards, and the body as a whole was already balanced by the lexer, so a
/// construct that does not close here (`None`) is left to be copied a character
/// at a time and reported by that later lex.
fn skip_construct(chs: &[Ch], i: usize) -> Option<usize> {
    match syn_at(chs, i) {
        // An escape covers the next character whatever it is, so a `\'` opens no
        // quoted run and a `\/` ends no pattern.
        '\\' if i + 1 < chs.len() => Some(i + 2),
        // Verbatim to the close: a single-quoted run honours no escape.
        '\'' => skip_quoted(chs, i + 1, '\'', false),
        '"' => skip_quoted(chs, i + 1, '"', true),
        '`' => skip_quoted(chs, i + 1, '`', true),
        // `$((` needs no case of its own: the inner `(` is counted by the outer
        // one, and the `))` closes both. `$[ … ]` deliberately gets none either
        // — bash's `skip_to_delim` does not know it, so `${s/$[4/2]aaa/Y}` really
        // does split at the inner `/` and leaves `$[4` to fail on its own.
        '$' => match syn_at(chs, i + 1) {
            '(' => skip_balanced(chs, i + 2, '(', ')'),
            '{' => skip_balanced(chs, i + 2, '{', '}'),
            // `$'…'` is not a single-quoted run: its `\'` is an ANSI-C escape,
            // and `skip_to_delim` reads it with one (subst.c, the `$` arm's
            // `string[i+1] == '\''` case). Without this, `${s/$'a\'/b'aaa/Y}`
            // splits inside the string and leaves an unbalanced quote.
            '\'' => skip_quoted(chs, i + 2, '\'', true),
            '"' => skip_quoted(chs, i + 2, '"', true),
            _ => None,
        },
        _ => None,
    }
}

/// The index just past the `quote` that closes a run starting at `from`, or
/// `None` if the run does not close. With `escapes`, a `\` covers the next
/// character — which is how `"a\"b"` and `` `echo \`x\`` `` stay whole.
fn skip_quoted(chs: &[Ch], from: usize, quote: char, escapes: bool) -> Option<usize> {
    let mut i = from;
    while i < chs.len() {
        let c = syn_at(chs, i);
        if c == quote {
            return Some(i + 1);
        }
        i += usize::from(escapes && c == '\\') + 1;
    }
    None
}

/// The index just past the `close` matching an `open` already consumed, counting
/// nesting and stepping over any construct met on the way — so a `)` inside a
/// quoted run or a nested `${ … }` closes nothing.
fn skip_balanced(chs: &[Ch], from: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = from;
    while i < chs.len() {
        let c = syn_at(chs, i);
        if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
            i += 1;
        } else if c == open {
            depth += 1;
            i += 1;
        } else if let Some(next) = skip_construct(chs, i) {
            // Always past `i`, so the scan cannot stall.
            i = next;
        } else {
            i += 1;
        }
    }
    None
}

/// Carve a C-style `for (( … ))` header into its `;`-separated sections.
///
/// bash's `make_arith_for_command` (make_cmd.c:278–307) walks the raw header
/// with `skip_to_delim (start, 0, ";", SD_NOJMP|SD_NOPROCSUB)`, taking the run
/// up to each top-level `;` for one section and counting them as it goes — so
/// the result is always at least one section, and three is the only count the
/// caller accepts.
///
/// That flag set is exactly the one [`skip_construct`] models, which is why the
/// scan is shared. A `;` inside a quoted run, a `` ` … ` ``, a `$( … )`, a
/// `${ … }`, a `$'…'` or a `$"…"` belongs to that construct and separates
/// nothing; one inside `[ … ]` (stepping over that wants `SD_GLOB`), `<( … )`
/// (suppressed by `SD_NOPROCSUB`), `$[ … ]` (which `skip_to_delim` does not
/// know) or a bare `( … )` (that wants `SD_ARITHEXP`) really does end a
/// section, however arithmetic the text around it looks:
///
/// ```text
/// for (( ${x:-;}; 0; 0 ))     three sections — the `;` is the expansion's
/// for (( a[1;2]; 0; 0 ))      four — `syntax error: `;' unexpected'
/// for (( $[1;2]; 0; 0 ))      four, for the same reason
/// ```
///
/// Each section begins past the space and tab bash's `whitespace()` skips, and
/// past nothing else — a leading newline is part of the text, and stays there
/// when `declare -f` prints the loop back. Trailing whitespace is never
/// dropped, which is why `for ((  i=0 ; i<2 ; i++  ))` comes back from
/// `declare -f` as `for ((i=0 ; i<2 ; i++  ))`.
fn arith_for_sections(chs: &[Ch]) -> Vec<Str> {
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        while matches!(syn_at(chs, i), ' ' | '\t') {
            i += 1;
        }
        let start = i;
        while i < chs.len() && syn_at(chs, i) != ';' {
            // Always past `i`, so the scan cannot stall.
            i = skip_construct(chs, i).unwrap_or(i + 1);
        }
        out.push(bytes::from_chars(chs.get(start..i).unwrap_or_default().iter().copied()));
        if i >= chs.len() {
            return out;
        }
        i += 1; // step over the `;`
    }
}

fn matching_subscript_close(chs: &[Ch], open: usize) -> Option<usize> {
    // The `]` that closes the subscript is the first one *at the subscript's own
    // level*: `[`/`]` nest, and a `]` inside a quoted run or a substitution
    // belongs to that construct. bash never has to look — `parse_matched_pair`
    // recorded where each nested construct ended as it read the body — so
    // `${h["a]b"]}` keys on `a]b` and `${a[$(echo 1])]}` has the whole
    // `$(echo 1])` for a subscript, which bash then fails to evaluate as
    // *arithmetic* (`1]: invalid arithmetic operator`), not as a truncated
    // substitution. Splitting mid-construct would instead leave an unbalanced
    // one that trips the re-lexer (`unexpected EOF while looking for matching
    // ')'`). See known-issues TD-OILS-SUBSCRIPT-QUOTED-BRACKET and
    // TD-OILS-A-SLASH-INSIDE-A-SUBSTITUTION-SPLITS-A-REPLACEMENT-PATTERN.
    //
    // The caller has already checked that `chs[open]` is the `[`, so the close
    // is one before the index `skip_balanced` reports.
    skip_balanced(chs, open + 1, '[', ']').map(|past| past - 1)
}

/// The verdict of [`split_name_subscript`].
enum NameSubscript {
    /// The body split cleanly into a name, an optional `[subscript]`, and
    /// whatever text followed the subscript.
    ///
    /// The last field is the physical line that remainder *starts* on: a
    /// subscript may span lines (`${a[$(`/`echo 1`/`)]#p}`), so it is not in
    /// general the line the body opened on. See [`frag_line`].
    Split(String, Option<ArrayIndex>, Vec<Ch>, u32),
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
    ///
    /// So does a body that names no parameter at all — an empty one (`${}`) or
    /// one opening on a character that cannot begin a name (`${.}`, `${ }`,
    /// `${:-x}`). See [`is_special_param_char`].
    Deferred,
}

/// Whether `c` alone names a parameter — the seven one-character specials.
///
/// bash keeps this set in its syntax table under `CSPECVAR`, built by
/// `mksyntax.c:247`, `addcstr ("@*#?-$!", CSPECVAR); /* omits $0...$9 and $_ */`,
/// and consults it from `SPECIAL_VAR` (subst.c:125) on the way into
/// `valid_brace_expansion_word` (subst.c). That gate is what makes `${.}` and
/// `${ }` errors rather than references to unset parameters named `.` and ` `:
/// a brace body must be all digits, a special, an array reference, or a legal
/// identifier, and a lone `.` is none of the four.
///
/// The check has to happen here, where the name is carved off, rather than at
/// expansion — because whether an operator follows makes no difference to it.
/// `${.}`, `${.:-x}` and `${.#p}` are all equally refused, so the default `x`
/// is never substituted and the `#p` never strips: bash rejects the *name*
/// before it ever looks at what is being asked of it.
fn is_special_param_char(c: char) -> bool {
    matches!(c, '@' | '*' | '#' | '?' | '-' | '$' | '!')
}

fn split_name_subscript(
    chs: &[Ch],
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<NameSubscript, ParseError> {
    if chs.is_empty() {
        // `${}` names nothing, and bash says so at expansion time, not while
        // parsing: `if false; then echo "${}"; fi` is silent. Its
        // `valid_brace_expansion_word` reads an empty name, and `legal_identifier`
        // rejects it along with every other body that is not a name.
        return Ok(NameSubscript::Deferred);
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
    } else if is_special_param_char(syn_at(chs, 0)) {
        // A special single-char parameter (`@`, `*`, `?`, `#`, `!`, `$`, `-`).
        i = 1;
    } else {
        // Anything else opens no name at all — see [`is_special_param_char`].
        return Ok(NameSubscript::Deferred);
    }
    let raw_name = bytes::from_chars(chs.get(..i).unwrap_or_default().iter().copied());
    // No branch above admits a non-ASCII character any more — `${\xff}` opens no
    // name and is already gone — but keep the guard so the name a caller is
    // handed stays text by construction rather than by an argument about which
    // characters the three branches can reach.
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
            _ => ArrayIndex::Index(Box::new(word_subscript_from_source_at(
                &inner,
                opts,
                q.as_pattern(),
                frag_line(chs, i + 1, line),
            )?)),
        };
        return Ok(NameSubscript::Split(
            name,
            Some(index),
            chs.get(close + 1..).unwrap_or_default().to_vec(),
            frag_line(chs, close + 1, line),
        ));
    }
    // A name has no newline in it, so the remainder starts on the body's own
    // line; only the subscript branch above can push it further down.
    Ok(NameSubscript::Split(name, None, chs.get(i..).unwrap_or_default().to_vec(), line))
}

/// Where the `:` that separates a slice's offset from its length is — bash's
/// `skiparith` (subst.c), which is not `strchr`. Three things hide a colon from
/// it, and all are measured against bash 5.2.37 with `z=abcdef`:
///
/// * **A pending `?`.** One `:` is skipped for each `?` seen, the ternary's own
///   colon belonging to the ternary. `${z:1?2:3}` is `cdef` — the whole text is
///   the offset, `1?2:3` being 2 — while `${z:1?2:3:1}` is `c`, the *second*
///   colon splitting. The count is not capped at one: `${z:1?1?2:3:4}` is
///   `cdef`, two `?` swallowing both colons.
/// * **A paren.** Nothing inside a `( … )` counts at all, colon and `?` alike:
///   `${z:(1?2:3)}` is `cdef` and `${z:(1?2:3):1}` is `c`. An *unbalanced* `(`
///   therefore hides the rest of the text outright.
/// * **A quote.** A `' … '` run, a `" … "` run and a backslash-escape are each
///   stepped over whole, before any of the counting: `${z:"1:2"}` does not
///   split (the evaluator meets `1:2` as one bound and says so) and neither
///   `${z:1"?"2:3}`'s `?` nor `${z:0"("}`'s paren counts. The characters
///   themselves stay in the bound — the quotes are removed later, by the
///   arithmetic reading each half is given — so this is only about the walk.
///
/// The walk is over the text **as written**, which is why an expansion that
/// *yields* an unbalanced paren is none of its business: `p="("; ${z:$p 1}` and
/// `${z:$(echo "(1")}` are ordinary arithmetic errors, the raw text of each
/// being balanced.
///
/// Returns `rest.len()` when no colon splits, which is the "offset only" case,
/// and `true` when a `(` was still open at the end — bash's own
/// `no closing `)'` condition, since the walk ran off looking for its match.
fn slice_split_colon(rest: &[Ch]) -> (usize, bool) {
    let (mut skipcol, mut depth) = (0u32, 0u32);
    let mut i = 0;
    while let Some(&c) = rest.get(i) {
        i += 1;
        match syn(c) {
            // A quoted run is stepped over whole, an unterminated one running
            // to the end. Inside `" … "` a backslash still escapes, inside
            // `' … '` nothing does.
            q @ ('\'' | '"') => {
                while let Some(&d) = rest.get(i) {
                    i += 1;
                    match syn(d) {
                        c if c == q => break,
                        '\\' if q == '"' => i += 1,
                        _ => {}
                    }
                }
            }
            '\\' => i += 1,
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            ':' if skipcol > 0 => skipcol -= 1,
            ':' => return (i - 1, false),
            '?' => skipcol += 1,
            _ => {}
        }
    }
    (rest.len(), depth > 0)
}

/// A slice's two bounds, as [`parse_slice_bounds`] cut them.
struct SliceBounds {
    offset: Box<Word>,
    /// `Some` where a colon cut a length off. Always `None` beside an
    /// `unclosed`, the unbalanced walk having consumed the whole text.
    length: Option<Box<Word>>,
    /// See [`crate::ast::WordPart::ArraySlice`]'s field of the same name.
    unclosed: Option<Str>,
}

/// Parse the `offset[:length]` portion of a substring/slice expansion (the
/// text after the leading `:`). The offset and each length are parsed as
/// arithmetic words. Splits on the one colon [`slice_split_colon`] finds.
///
/// `None` is a **bad substitution**: the text being empty outright is one, and
/// uniformly so — `${z:}`, `${@:}`, `${*:}`, `${a[@]:}` and `${a[1]:}` all
/// report `${…}: bad substitution` in bash 5.2.37, an unset parameter included.
/// It is the *text* that must be non-empty, not what it expands to: `${z:$e}`
/// with `e=` is `abcdef`, and so is `${z:$(echo)}`. A colon and nothing else is
/// fine on both sides of it — `${z::}` is the empty string, offset and length
/// both reading as 0. An unbalanced `(` is a bad substitution too, but a later
/// and differently-worded one, so it rides along as
/// [`SliceBounds::unclosed`] rather than as a `None`.
fn parse_slice_bounds(
    rest: &[Ch],
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Option<SliceBounds>, ParseError> {
    if rest.is_empty() {
        return Ok(None);
    }
    let (split, unbalanced) = slice_split_colon(rest);
    let (off, len) = match Some(split).filter(|&i| i < rest.len()) {
        Some(idx) => (
            rest.get(..idx).unwrap_or_default(),
            // The offset it follows may span lines — `${x:$(`/`echo 1`/`):2}` —
            // so the length's own line is counted past it, not assumed.
            Some((rest.get(idx + 1..).unwrap_or_default(), frag_line(rest, idx + 1, line))),
        ),
        None => (rest, None),
    };
    // Both bounds are arithmetic, and so are read exactly as a subscript is —
    // verbatim, with each top-level `' … '` given its second reading. They are
    // *not* tokenized: bash never tokenizes either bound, it cuts the `${ … }`
    // body at the `:` and hands the characters to `expand_arith_string` and
    // then `evalexp`. Every operator a command tokenizer would have taken for
    // its own is therefore an arithmetic operator here, which is what osh used
    // to lose — measured against bash 5.2.37, `z=abcdef`:
    //
    // | written | bash | osh, tokenized |
    // |---|---|---|
    // | `${z:1<2}` | `bcdef` | `cdef` — an IO number and a redirect |
    // | `${z:1>2}` | `abcdef` | `cdef` — likewise |
    // | `${z:1<=2}` | `bcdef` | `=2: operand expected` |
    // | `${z:1 < (2)}` | `bcdef` | `1 2: syntax error` |
    // | `${z:1;2}` | `;2: invalid arithmetic operator` | `1 2: syntax error` |
    // | `${z:1&2}` | `abcdef` | `1 2: syntax error` |
    // | `${z:1?2:3}` | `cdef` | `` `:' expected `` — the split `:` is the bound's |
    // | `${z:(1}` | ``no closing `)' `` | silently `abcdef` |
    //
    // The last two rows are the ones that show it is not merely a matter of
    // which characters are operators: a tokenizer *drops* what it cannot make a
    // word of, so an unbalanced `(` vanishes instead of being complained about.
    let length = match len {
        Some((s, len_line)) => {
            let text = bytes::from_chars(s.iter().copied());
            Some(Box::new(word_bound_from_source_at(&text, opts, q.as_pattern(), len_line)?))
        }
        None => None,
    };
    let off_text = bytes::from_chars(off.iter().copied());
    let offset = word_bound_from_source_at(&off_text, opts, q.as_pattern(), line)?;
    // The unbalanced text is kept as characters rather than rebuilt from the
    // word: bash quotes back what the writer wrote, and nothing in it has been
    // expanded yet when the complaint is made.
    Ok(Some(SliceBounds {
        offset: Box::new(offset),
        length,
        unclosed: unbalanced.then(|| off_text.clone()),
    }))
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

/// Carry [`parse_braced_param_in`]'s splice ranges into the coordinates of a
/// stretch of text carved out of the body: the `from..from + len` bytes of it,
/// which land at `to` in the buffer being built. Ranges outside that stretch are
/// dropped, and one straddling its edge is clipped to it — an operand only wants
/// to know which of *its own* bytes no scan read.
fn splices_within(
    splices: &[core::ops::Range<usize>],
    from: usize,
    len: usize,
    to: usize,
) -> Vec<core::ops::Range<usize>> {
    if splices.is_empty() {
        return Vec::new();
    }
    let end = from.saturating_add(len);
    splices
        .iter()
        .filter_map(|r| {
            let start = r.start.max(from);
            let stop = r.end.min(end);
            (start < stop).then(|| (start - from + to)..(stop - from + to))
        })
        .collect()
}

/// [`splices_within`] for an operand, which is always a byte *suffix* of the
/// body — everything before it was consumed as a name and an operator, character
/// by character, so its offset is just what the two lengths differ by.
fn operand_splices(
    splices: &[core::ops::Range<usize>],
    raw: BStr<'_>,
    arg: BStr<'_>,
) -> Vec<core::ops::Range<usize>> {
    splices_within(splices, raw.len().saturating_sub(arg.len()), arg.len(), 0)
}

/// Parse a `${ … }` body that reached the expander as *text* rather than as a
/// segment the word lexer had already lowered — an arithmetic string, or a
/// `${!ref…` whose brace never closed. `q` is the quoting the text is being
/// expanded under, which its operand is read with (see [`Quoting`]).
pub(crate) fn parse_braced_param(
    raw: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
) -> Result<WordPart, ParseError> {
    // Text that reached the expander is text no scan of ours spliced into: what
    // a `$'…'` in it becomes is that expansion's business, not a parse's.
    parse_braced_param_in(raw, opts, q, RUNTIME_TEXT, &[])
}

/// [`parse_braced_param`] with the quoting the `${…}` was written in, which
/// only its operand cares about (see [`Quoting`]), and the physical line the
/// body starts on, which every fragment of it is numbered from (see
/// [`frag_line`]).
///
/// `splices` are the stretches of `raw` the word lexer wrote in without reading
/// — a bare `$'…'` translation, bash's third row (parse.y:3887) — as byte ranges
/// into `raw`. Only an operand can hold one and still be parsed, so they are
/// carried no further than [`operand_from_source`], which hands them to
/// [`crate::lexer::lex_operand_in_dquote`].
fn parse_braced_param_in(
    raw: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
    splices: &[core::ops::Range<usize>],
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
        // The `#` just stripped is not a newline, so the rest still starts on
        // the body's own line; the same holds for the `!` stripped below.
        let chs: Vec<Ch> = bytes::chars(after_hash).collect();
        if let NameSubscript::Split(name, subscript, remaining, _) =
            split_name_subscript(&chs, opts, q, line)?
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
        // A trailing `@`/`*` the parameter-name scan ran the whole way up to is
        // not an operator on the reference — it is part of the *name*, either
        // because the scan swallowed it (`*` is not one of its stop characters)
        // or because bash glues it back on (subst.c:9585). What is then done
        // with that name is decided before any other reading a `${!…}` could
        // have — ahead of the array keys and of the indirection proper — and
        // turns only on whether it starts like one. See
        // [`scan_reaches_trailing_mark`].
        if let Some((prefix, star)) = scan_reaches_trailing_mark(after_bang) {
            // `${!prefix*}` / `${!prefix@}` — the names of every set variable
            // beginning with `prefix` (subst.c:9741). It is not a *name* that is
            // asked for, only something that begins like one.
            if let Some(c) = bytes::chars(prefix).next()
                && is_name_start(syn(c))
            {
                return Ok(WordPart::VarNames {
                    prefix: prefix.to_vec(),
                    star,
                });
            }
            // Otherwise the name the scan made is one no variable can be called
            // — `${!1@}` asks after `1@` — and bash refuses the word without
            // ever indirecting through anything: `${!1@}` and `${!1*}` are a bad
            // substitution whether or not `$1` is set, where `${!1@Q}` (whose
            // `@` the scan stopped *at*, so it really is an operator) reads the
            // pointer like any other indirection.
            return Ok(WordPart::BadSubst(raw.to_vec()));
        }
        // `${!name[@]}` / `${!name[*]}` — the keys/indices of an array.
        let chs: Vec<Ch> = bytes::chars(after_bang).collect();
        let NameSubscript::Split(name, subscript, remaining, remaining_line) =
            split_name_subscript(&chs, opts, q, line)?
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
            let rest_bytes = bytes::from_chars(remaining.iter().copied());
            // The modifier text is the tail of the body with a different head
            // glued on, so a splice in that tail moves by the difference.
            let moved = splices_within(
                splices,
                raw.len().saturating_sub(rest_bytes.len()),
                rest_bytes.len(),
                modifier_src.len(),
            );
            modifier_src.extend(rest_bytes);
            // The name spliced on the front is a stand-in with no newline in it,
            // so the modifier text still begins where it did in the real body.
            let mut target = parse_braced_param_in(&modifier_src, opts, q, remaining_line, &moved)?;
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
                    // An `@` operator bash will refuse is still an operator
                    // here: it judges the spelling in `parameter_brace_transform`,
                    // which the indirection reaches only after it has resolved
                    // the pointer — so `${!u[k]@Z}` through an unset target is
                    // simply empty, and `${!nope[k]@Z}` complains about the
                    // *pointer*. See [`WordPart::BadTransform`].
                    | WordPart::BadTransform { .. }
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
    let NameSubscript::Split(name, subscript, rest, rest_line) =
        split_name_subscript(&chs, opts, q, line)?
    else {
        return Ok(WordPart::BadSubst(raw.to_vec()));
    };
    // Every fragment carved out of `rest` below starts on `rest_line` too: what
    // is skipped to reach one is only ever operator characters (`:`, `#`, `%`,
    // `^`, `,`, `~`, `/`, `-`, `=`, `+`, `?`), and none of those is a newline.
    // A fragment moves to a later line in exactly three places, each of which
    // counts the newlines itself: past a multi-line subscript
    // ([`split_name_subscript`]), past a slice's offset ([`parse_slice_bounds`]),
    // and past a replacement's pattern ([`parse_replace_pieces`]).
    //
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
                let Some(b) =
                    parse_slice_bounds(rest.get(1..).unwrap_or_default(), opts, q, rest_line)?
                else {
                    return Ok(WordPart::BadSubst(raw.to_vec()));
                };
                return Ok(WordPart::ArraySlice {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    offset: b.offset,
                    length: b.length,
                    unclosed: b.unclosed,
                });
            }
            // `${a[@]#pat}` / `${a[*]/x/y}` / `${a[@]^^}` / `${a[@]@Q}` — an
            // element-wise transform applied to every element.
            if let Some(op) = parse_bulk_op(&rest, opts, q, rest_line)? {
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
                    op: BulkOp::BadTransform {
                        op: Box::new(bad_transform_operand(&rest, opts, q, rest_line)?),
                    },
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
                arg: Box::new(operand_from_source(
                    &arg_str,
                    opts,
                    q,
                    rest_line,
                    &operand_splices(splices, raw, &arg_str),
                )?),
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
        let Some(b) = parse_slice_bounds(rest.get(1..).unwrap_or_default(), opts, q, rest_line)?
        else {
            return Ok(WordPart::BadSubst(raw.to_vec()));
        };
        return Ok(WordPart::ArraySlice {
            name: name.clone(),
            star: name == "*",
            offset: b.offset,
            length: b.length,
            unclosed: b.unclosed,
        });
    }
    // `${@#pat}` / `${*/x/y}` / `${@^^}` — element-wise transform over the
    // positional parameters.
    if (name == "@" || name == "*")
        && !rest.is_empty()
        && let Some(op) = parse_bulk_op(&rest, opts, q, rest_line)?
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
            op: BulkOp::BadTransform {
                op: Box::new(bad_transform_operand(&rest, opts, q, rest_line)?),
            },
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
                arg: Box::new(operand_from_source(
                    &arg_str,
                    opts,
                    q,
                    rest_line,
                    &operand_splices(splices, raw, &arg_str),
                )?),
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
                pattern: Box::new(word_verbatim_from_source_at(
                    &pat,
                    opts,
                    q.as_pattern(),
                    rest_line,
                )?),
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
                pattern: Box::new(word_verbatim_from_source_at(
                    &pat,
                    opts,
                    q.as_pattern(),
                    rest_line,
                )?),
            })
        }
        // Parameter transformation: `${name@Q}`, `${name@U}`, etc.
        '@' => {
            // An empty (`${x@}`), unknown (`${x@Z}`), or multi-char
            // (`${x@QU}`) operator is *not* a parse-time error in bash: it is
            // deferred to expansion, where it yields empty for an unset
            // parameter but a "bad substitution" for a set one. `BadTransform`
            // keeps the operand so the runtime can reproduce that split.
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
                op: Box::new(bad_transform_operand(&rest, opts, q, rest_line)?),
            })
        }
        // Pattern substitution: `/pat/repl`, `//pat/repl`, `/#…`, `/%…`.
        '/' => parse_param_replace(
            name,
            elem_index,
            rest.get(1..).unwrap_or_default(),
            opts,
            q,
            rest_line,
        ),
        // Substring `:offset[:length]` — but `:` followed by one of -=+? is the
        // use/assign/alt/error operator, handled below.
        ':' if !matches!(syn_at(&rest, 1), '-' | '=' | '+' | '?') => {
            let Some(b) =
                parse_slice_bounds(rest.get(1..).unwrap_or_default(), opts, q, rest_line)?
            else {
                return Ok(WordPart::BadSubst(raw.to_vec()));
            };
            Ok(WordPart::ParamSubstr {
                name,
                index: elem_index,
                offset: b.offset,
                length: b.length,
                unclosed: b.unclosed,
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
                arg: Box::new(operand_from_source(
                    &arg_str,
                    opts,
                    q,
                    rest_line,
                    &operand_splices(splices, raw, &arg_str),
                )?),
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
///
/// The replacement is `None` where the body carried **no separator at all**.
/// That is not the same fact as an empty one: the two expand alike, but bash
/// prints a word back from its saved source text, so `${q/ab}` keeps its shape
/// under `declare -f` while `${q/ab/}` keeps its trailing slash. Folding the
/// two together here is what used to put a slash in the printback that the
/// source never had.
#[allow(clippy::type_complexity)]
fn parse_replace_pieces(
    body: &[Ch],
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<(bool, ReplaceAnchor, Box<Word>, Option<Box<Word>>), ParseError> {
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
    // Pattern runs to the next unescaped '/' *at this level*; the remainder is
    // the replacement.
    let mut pattern = Str::new();
    let mut replacement = Str::new();
    let mut in_repl = false;
    // The pattern may span lines, so the replacement's own line is counted past
    // it rather than assumed: in `${x/aaa`/`bbb/$(fi)}` the `$(fi)` is a line
    // below the `${`, and bash blames the line it is really on.
    let pat_start_line = frag_line(body, i, line);
    let mut repl_line = pat_start_line;
    while let Some(&c) = body.get(i) {
        // A `\/` needs no case of its own: `skip_construct` below already copies
        // an escape and the character it covers whole, which both hides the `/`
        // from the separator test and keeps the backslash in the pattern text.
        //
        // Keeping it is the point. bash's `skip_to_delim` (subst.c:9157) only
        // *locates* the separator — it never rewrites `lpatsub` — so the `\/` is
        // still there when `getpattern` runs `expand_string_for_pat` and
        // `quote_string_for_globbing` over it (subst.c:5754, 5764) and comes out
        // as a glob escape matching a literal `/`. Consuming the backslash here
        // matched the same text but lost it from the printback, and
        // `${q/a\/b/Y}` printed as `${q/a/b/Y}` — which does not even re-parse to
        // the same expansion, since the printed `/` reads as the separator.
        if !in_repl && syn(c) == '/' {
            in_repl = true;
            i += 1;
            repl_line = frag_line(body, i, line);
            continue;
        }
        // A nested construct is copied whole, so a `/` inside one ends nothing:
        // `${s/$(echo a/b)aaa/Y}` has a single separator and its pattern is the
        // whole `$(echo a/b)aaa`. Doing this in the replacement half too is a
        // no-op on the bytes, and keeps the two halves scanned by one rule.
        if let Some(next) = skip_construct(body, i) {
            let dst = if in_repl { &mut replacement } else { &mut pattern };
            for &ch in body.get(i..next).unwrap_or_default() {
                ch.push_to(dst);
            }
            i = next;
            continue;
        }
        if in_repl {
            c.push_to(&mut replacement);
        } else {
            c.push_to(&mut pattern);
        }
        i += 1;
    }
    let repl = if in_repl {
        Some(Box::new(word_replacement_from_source(
            &replacement,
            opts,
            q.as_pattern(),
            repl_line,
        )?))
    } else {
        // No separator was reached, so there is no replacement *text* to parse
        // — not even an empty one. `${q/ab}` deletes the match, which is what
        // the expansion does with `None`.
        None
    };
    Ok((
        all,
        anchor,
        Box::new(word_verbatim_from_source_at(
            &pattern,
            opts,
            q.as_pattern(),
            pat_start_line,
        )?),
        repl,
    ))
}

fn parse_param_replace(
    name: String,
    index: Option<Box<Word>>,
    body: &[Ch],
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<WordPart, ParseError> {
    let (all, anchor, pattern, replacement) = parse_replace_pieces(body, opts, q, line)?;
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
fn parse_bulk_op(
    rest: &[Ch],
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Option<BulkOp>, ParseError> {
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
                pattern: Box::new(word_verbatim_from_source_at(&pat, opts, q.as_pattern(), line)?),
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
                pattern: Box::new(word_verbatim_from_source_at(&pat, opts, q.as_pattern(), line)?),
            }))
        }
        '/' => {
            let (all, anchor, pattern, replacement) =
                parse_replace_pieces(rest.get(1..).unwrap_or_default(), opts, q, line)?;
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

/// The operand of an *invalid* `${x@…}` transform — everything after the `@` —
/// as a word.
///
/// `rest` starts at the `@` itself, so the operand is `rest[1..]`.
///
/// Nothing ever expands this word: the operator is rejected whole, and the only
/// two things ever asked of it are its source text (for the `bad substitution`
/// diagnostic and for `declare -f`) and the substitutions in it, which bash's
/// `${ … }` scan reads before it judges anything — `extract_dollar_brace_string`
/// walks the body to find the `}` and reads a `$( … )` on the way
/// (subst.c:1896-1902), so `A${q@$(fi)}B` reports the failed extent *before* the
/// bad substitution. Reading it as a word rather than keeping it as text is what
/// puts it in front of that scan; see
/// [`crate::interp::Shell::brace_extent_scan`].
///
/// The quoting is the enclosing text's, unchanged: there is no `getpattern` here
/// to drop the double-quoting, and no second reader to give the text a parse or
/// a translation it did not already have.
fn bad_transform_operand(
    rest: &[Ch],
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Word, ParseError> {
    let text = bytes::from_chars(rest.get(1..).unwrap_or_default().iter().copied());
    word_verbatim_from_source_at(&text, opts, q, line)
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
pub(crate) fn word_verbatim_from_source(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
) -> Result<Word, ParseError> {
    word_verbatim_from_source_at(s, opts, q, RUNTIME_TEXT)
}

/// [`word_verbatim_from_source`] for a fragment of a `${ … }` body, which sits
/// on a known physical line. See [`frag_line`].
///
/// `q` is always a *bare* [`Quoting`] — a pattern's quoting is `Q_PATQUOTE`
/// whatever surrounded it — but it still carries the read the enclosing text
/// had, so a `$( … )` in the pattern of an unread `${ … }` stays
/// [`crate::ast::CmdSubBody::Unread`] like the operand beside it. Callers pass
/// `q.as_pattern()`.
pub(crate) fn word_verbatim_from_source_at(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Word, ParseError> {
    verbatim_word_at(s, opts, q, line, Frag::Word)
}

/// Which of the three verbatim readings a fragment gets. They differ in what a
/// `<( … )` written in the fragment means — whether it is *performed*, and
/// whether the `${ … }` scan even *read* it. See
/// [`crate::lexer::lex_subscript_verbatim`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Frag {
    /// A pattern, a replacement or a bare operand — expanded as a word, so a
    /// process substitution in it runs.
    Word,
    /// A subscript — expanded as an arithmetic string
    /// (`Q_DOUBLE_QUOTES|Q_ARITH`), so one does not run; and stepped over whole
    /// by the `${ … }` scan, so one is not read either.
    Arith,
    /// A substring bound — [`Frag::Arith`] except that the scan does reach it,
    /// so a process substitution here is read for its extent. See
    /// [`crate::lexer::Verbatim::Bound`].
    Bound,
}

fn verbatim_word_at(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
    frag: Frag,
) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    // Only the two *modes of reading* reach the lexer: this scan has never
    // consulted `extglob` or `posix` (it was handed `ParseOpts::default()`
    // outright), and neither flag is a shell option, so passing them through
    // changes nothing else.
    let lex_opts =
        ParseOpts { reread: opts.reread, tolerant: opts.tolerant, ..ParseOpts::default() };
    let read = match frag {
        Frag::Word => crate::lexer::lex_word_verbatim_opts,
        Frag::Arith => crate::lexer::lex_subscript_verbatim,
        Frag::Bound => crate::lexer::lex_bound_verbatim,
    };
    let mut segs = read(s, lex_opts, q.read_ctx()).map_err(|e| ParseError::new(&e.msg))?;
    map_frag_segs(&mut segs, line);
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts, q)?);
    }
    Ok(Word { parts })
}

/// [`word_verbatim_from_source_at`] for an **array subscript** or a **substring
/// bound** — the two fragments that may reach the arithmetic evaluator, where a
/// `'` is not a quote.
///
/// Both readings of the fragment are built here, because which one applies is
/// not decided until expansion time: an *index* goes to `expand_arith_string
/// (exp, Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)` (arrayfunc.c:1354) and an
/// *associative key* to `expand_subscript_string (sub, 0)` (arrayfunc.c:1145),
/// and only the array's runtime type says which. The verbatim parse is the
/// key's — a `'` opens a quote whose contents are literal. Beside it, each
/// top-level `' … '` run gets its interior parsed as the surrounding subscript
/// already is, which is the index's reading: `Q_DOUBLE_QUOTES` merely switches
/// the single quote off, so everything else in the run is read exactly as it
/// would be one character to the left of the `'`. Measured, all as `${a[…]}`
/// against bash 5.2.37:
///
/// | written | error token | what it says |
/// |---|---|---|
/// | `'$(echo 1)'` | `'1'` | the substitution ran |
/// | `'$(echo 1 >&2; echo 1)'` | `'1'` (after `1` on stderr) | …really ran |
/// | `'$n'` with `n=2` | `'2'` | a parameter expands too |
/// | `` '`echo 1`' `` | `'1'` | so does a backquote |
/// | `'"1"'` | `'1'` | a *double* quote is still a quote, and comes off |
/// | `'"'` | `''` | …even unterminated, which eats the rest |
/// | `'\1'` | `'\1'` | a backslash follows double-quoting's rules |
/// | `'\$n'` | `'$n'` | …so it disappears before a `$` |
/// | `'~'` | `'~'` | no tilde expansion in an arithmetic string |
///
/// The interior is read as **unread** text. bash's parser stops at the `'` and
/// resumes at its mate (`parse_matched_pair` reads nothing between them), so a
/// `$( … )` in there has no parse and is read by `extract_command_subst` at
/// expansion time: `${a[$(fi)]}` is a script syntax error that never runs,
/// while `${a['$(fi)']}` runs the script and reports `command substitution:
/// line N:` quoting the rest of the *subscript*. An ANSI-C `$'…'` cannot occur
/// in here at all — its own `'` would end the run — so the translation half of
/// the read has nothing to decide.
pub(crate) fn word_subscript_from_source(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
) -> Result<Word, ParseError> {
    word_subscript_from_source_at(s, opts, q, RUNTIME_TEXT)
}

/// [`word_subscript_from_source`] for a subscript that sits on a known physical
/// line. See [`frag_line`].
pub(crate) fn word_subscript_from_source_at(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Word, ParseError> {
    let mut w = verbatim_word_at(s, opts, q, line, Frag::Arith)?;
    attach_subscript_reads(&mut w, opts, q, line)?;
    Ok(w)
}

/// [`word_subscript_from_source_at`] for the other arithmetic fragment, a
/// **substring bound**. The two differ only in whether the `${ … }` scan
/// reached the text — see [`crate::lexer::Verbatim::Bound`] — and a `' … '` run
/// in either is read the same way, the scan having stepped over it in both.
pub(crate) fn word_bound_from_source_at(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Word, ParseError> {
    let mut w = verbatim_word_at(s, opts, q, line, Frag::Bound)?;
    attach_subscript_reads(&mut w, opts, q, line)?;
    Ok(w)
}

/// Give every top-level `' … '` of an already-parsed subscript or substring
/// bound its arithmetic reading. See [`word_subscript_from_source_at`], which
/// documents what the reading is. Both callers reach it through
/// [`word_subscript_from_source_at`] — a subscript and the two bounds of
/// [`parse_slice_bounds`], which are read the same way.
fn attach_subscript_reads(
    w: &mut Word,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<(), ParseError> {
    let inner_q = q.as_unread();
    let mut any = false;
    for part in &mut w.parts {
        let WordPart::SingleQuoted { text, escaped: false, parts, .. } = part else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        // Tolerant, because the run's interior can be cut short of a quote it
        // opened — `'"'` is a `"` with no mate, and bash's expander runs it to
        // the end of the string rather than complaining.
        // Still arithmetic text: `Q_DOUBLE_QUOTES` merely switches the single
        // quote off, so a `<( … )` inside the run is no more performed than one
        // beside it.
        let tolerant = ParseOpts { tolerant: true, ..opts };
        *parts = Some(verbatim_word_at(text, tolerant, inner_q, line, Frag::Arith)?.parts);
        any = true;
    }
    // The interior was parsed on its own and so knows nothing of what follows
    // it. Re-measure the whole fragment now that the run is walkable, which is
    // what gives a `$( … )` in there the rest of the *subscript* — its closing
    // `'` first — for the remainder a failed re-read echoes. A caller that is
    // itself part of a larger word measures again over that word; this is the
    // scope for the ones that are not (a `name[sub]=v` left-hand side).
    if any {
        crate::unparse::attach_comsub_tails(w);
        name_unclosed_after_the_fragment(w);
    }
    Ok(())
}

/// Give a construct the interior parse left open the **fragment** to name,
/// rather than the interior it was parsed out of.
///
/// The interior is a string of osh's making, not of bash's: `Q_DOUBLE_QUOTES` is
/// set for an arithmetic fragment, so `expand_word_internal` never treats the
/// `'` as an opener and walks straight through it — there is one string here and
/// it is the whole fragment. Both "no closing" diagnostics echo the string the
/// scan was handed (`report_error (…, string)`, subst.c:1498 and subst.c:1972),
/// so that is what they must echo here. Measured against bash 5.2.37, with
/// `a=(0 1 2)`:
///
/// ```text
/// echo "[${a['x${m:-']}]"   ->   bad substitution: no closing `}' in 'x${m:-'
/// echo "[${a['x$[1 ']}]"    ->   bad substitution: no closing `]' in 'x$[1 '
/// ```
///
/// — the quotes included, where the interior alone would have been `x${m:-`.
///
/// Only the run's own level is renamed. A `" … "` inside the interior is a run
/// of this same string and `string_extract_double_quoted` carves it out as its
/// own, so a fault found in *there* names the run, exactly as one found in a
/// double-quoted run written a character to the left of the `'` would.
///
/// The backquote reporter is widened the same way but from a different place,
/// because its `%s` is `string + t_index` (subst.c:11269) and not `string`: it
/// runs from the backquote to the end of the fragment, so what is glued on is
/// the run's own closing quote and whatever follows it. Measured:
///
/// ```text
/// echo "[${a['x`fi'y]}]"     ->   bad substitution: no closing "`" in `fi'y
/// echo "[${a['x`fi''q']}]"   ->   bad substitution: no closing "`" in `fi''q'
/// ```
fn name_unclosed_after_the_fragment(w: &mut Word) {
    use crate::lexer::Unclosed;
    let frag = crate::unparse::word_src(w);
    // Where each part's source begins in the fragment, for the reporter that
    // names only the text from its own construct on.
    let mut starts: Vec<usize> = Vec::with_capacity(w.parts.len());
    let mut at = 0usize;
    for p in &w.parts {
        starts.push(at);
        at = at.saturating_add(crate::unparse::part_src(p).len());
    }
    for (part, start) in w.parts.iter_mut().zip(starts) {
        let WordPart::SingleQuoted { parts: Some(inner), .. } = part else {
            continue;
        };
        // Everything the fragment holds past the run's interior: its closing
        // quote, where it has one, and then the rest of the fragment. A scan
        // that gave up ran to the end of that interior, so this is the whole of
        // what its own `src` is short of. The `1` is the opening quote.
        let after = start
            .saturating_add(1)
            .saturating_add(crate::unparse::parts_src(inner).len());
        let tail = frag.get(after..).unwrap_or_default();
        for p in inner.iter_mut() {
            match p {
                WordPart::Unclosed(Unclosed::BadSubst { text, .. }) => text.clone_from(&frag),
                WordPart::Unclosed(Unclosed::Backquote { src, text }) => {
                    *text = bfmt![&*src, tail];
                }
                _ => {}
            }
        }
    }
}

/// The word `expand_word_internal` reads out of `s` — [`word_verbatim_from_source_at`]
/// with [`ParseOpts::tolerant`](crate::lexer::ParseOpts::tolerant) set.
///
/// This is the reader for text bash never read as *source*. A word's token
/// buffer can hold characters the scan wrote into it rather than read out of
/// it — the bare splice of a translated `$'…'` (parse.y:3887) — and can be cut
/// short of its own closing quote by a NUL the same splice carried
/// ([`crate::ast::WordPart::TokenText`]). Either way the expander meets quoting
/// the parse tree does not describe, and reading the buffer back is the only
/// way to get the expander's answer.
///
/// The tolerance is exactly `string_extract_single_quoted`'s and
/// `string_extract_double_quoted`'s: they stop at the end of the string they
/// were handed. An unterminated `` ` `` or `${` is *not* tolerated — bash has a
/// runtime diagnostic for each (subst.c:11290 and subst.c:1980) rather than a
/// silent run to the end — so those still come back `Err` and the caller falls
/// back to the text it had.
pub(crate) fn word_tolerant_from_source_at(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Word, ParseError> {
    word_verbatim_from_source_at(s, ParseOpts { tolerant: true, ..opts }, q, line)
}

/// Parse the *operand* of a substitution — the `w` of `${x:-w}`, `${x:=w}`,
/// `${x:+w}` and `${x:?w}`. Written bare it is a verbatim word like a pattern
/// is; written inside `"…"` it is read with double-quote rules instead, because
/// the quotes around the substitution never stopped applying. See [`Quoting`]
/// and [`crate::lexer::lex_operand_in_dquote`].
fn operand_from_source(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
    splices: &[core::ops::Range<usize>],
) -> Result<Word, ParseError> {
    if !q.dquoted() {
        // A splice only happens inside double quotes (parse.y:3882 requotes
        // otherwise), so there is nothing to carry down this path.
        return word_verbatim_from_source_at(s, opts, q, line);
    }
    if s.is_empty() {
        return Ok(Word::default());
    }
    let mut segs = crate::lexer::lex_operand_in_dquote(s, q.read_ctx(), splices)
        .map_err(|e| ParseError::new(&e.msg))?;
    map_frag_segs(&mut segs, line);
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        // Still inside the quotes, and still inside whichever *read* they came
        // from: a substitution nested in the operand is read the same way its
        // host was, unread host included.
        parts.push(seg_to_part(seg, opts, q)?);
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
        // The whole point of this entry is that `s` is a double-quoted run that
        // reached the shell as a *value*, so an operand inside it is read as one
        // too — quotes in force, and no parser having read any of it.
        parts.push(seg_to_part(seg, opts, Quoting::Unread)?);
    }
    let mut word = Word { parts };
    // A word the shell assembles from a *value* has no re-print, but its
    // substitutions are still re-read: `expand_word_internal` walks this string
    // and hands `extract_command_subst` the whole remainder of it, so a
    // `$( … )` in it wants the same tail a parser's word would give it. Every
    // body here is [`crate::ast::CmdSubBody::Unread`] — that is what
    // [`crate::lexer::lex_dquote_body`] produces — so nothing else is touched.
    crate::unparse::attach_comsub_tails(&mut word);
    Ok(word)
}

/// Like [`dquote_word_from_source`], but reading `s` the way the `${ … }`
/// **scan** walks it rather than the way the expansion after it does: a `<( … )`
/// and a `>( … )` are read for their extent there, exactly as a `$( … )` is
/// (subst.c:1881-1950). See [`crate::lexer::lex_brace_scan_body`].
///
/// The extra parts carry their own spelling and are never performed, so the
/// word expands to the same bytes [`dquote_word_from_source`] would give; the
/// difference is only what an extent walk over it finds to read.
///
/// # Errors
/// Returns [`ParseError`] on an unterminated substitution inside `s`.
pub(crate) fn brace_scan_word_from_source(s: BStr<'_>, opts: ParseOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_brace_scan_body(s).map_err(|e| ParseError::new(&e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts, Quoting::Unread)?);
    }
    let mut word = Word { parts };
    crate::unparse::attach_comsub_tails(&mut word);
    Ok(word)
}

/// Like [`word_verbatim_from_source`] but for the *replacement* half of
/// `${var/pat/repl}`: a literal `\&`/`\\` is preserved (not consumed at lex
/// time) so the runtime `&`-substitution can distinguish an escaped ampersand
/// from an active one. See [`crate::lexer::lex_replacement_verbatim`].
fn word_replacement_from_source(
    s: BStr<'_>,
    opts: ParseOpts,
    q: Quoting,
    line: u32,
) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let mut segs =
        crate::lexer::lex_replacement_verbatim(s, q.read_ctx()).map_err(|e| ParseError::new(&e.msg))?;
    map_frag_segs(&mut segs, line);
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts, q)?);
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

/// Where the `]` closing an already-open subscript falls in the literal run
/// `s`, or `None` when the run does not close it. `depth` arrives holding how
/// many brackets are open and leaves holding how many still are — so a caller
/// walking a word's segments can carry it from one literal run to the next.
///
/// This is [`balanced_subscript_end`]'s counting, resumable: the same rule that
/// `[`/`]` nest, applied to a subscript whose text is spread over several
/// segments because it holds an expansion. The segments in between are quoted
/// runs and substitutions, which `skipsubscript` steps over whole and which
/// therefore contribute no brackets at all — so counting the literal runs is
/// the whole of the job. See [`Parser::spanning_subscript_assignment`].
fn subscript_close_in_lit(s: BStr<'_>, depth: &mut usize) -> Option<usize> {
    for (i, &b) in s.iter().enumerate() {
        match b {
            b'[' => *depth = depth.saturating_add(1),
            b']' => {
                *depth = depth.saturating_sub(1);
                if *depth == 0 {
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

/// The characters that end bash's parameter-name scan
/// (`string_extract (string, &t_index, "#%^,~:-=?+/@}", SX_VARNAME)`,
/// subst.c:9567). Any of these standing in a `${!…}` body — outside a
/// subscript, which the scan steps over — ends the name there, so the body's
/// trailing `@`/`*` is an operator on the reference rather than part of the name
/// it scanned. `*` is deliberately *not* among them, which is why a star form
/// may carry one in the middle as well (`${!a*b*}` lists `a*b`).
const NAME_SCAN_STOPS: &[u8] = b"#%^,~:-=?+/@}";

/// Does bash's parameter-name scan run the whole way through this `${!…}` body
/// (its leading `!` already removed) to a trailing `@`/`*`? Answers the text
/// before that character and whether the star spelling was used.
///
/// This is what decides that the trailing character is part of the **name**
/// rather than an operator on the reference. For `*` that needs no help — it is
/// not one of the scan's stop characters, so the scan simply swallows it — while
/// an `@` bash glues back on afterwards (subst.c:9585):
///
/// ```text
/// else if (*name == '!' && t_index > sindex && string[t_index] == '@' &&
///          string[t_index+1] == RBRACE)
/// ```
///
/// which is why only an `@` sitting immediately before the closing brace counts,
/// and `${!1@Q}` — whose `@` the scan stopped *at* — is an ordinary indirection
/// with a transform on it.
///
/// What the name is then used for turns on one further test, made by the caller
/// (subst.c:9741):
///
/// ```text
/// if (want_indir && string[sindex - 1] == RBRACE &&
///     (string[sindex - 2] == '*' || string[sindex - 2] == '@') &&
///     legal_variable_starter ((unsigned char) name[1]))
/// ```
///
/// so a body that merely *starts* like a name is the name-listing form, and the
/// prefix is taken raw — `temp1 = savestring (name + 1)` with its last character
/// cut off — never expanded and never unquoted. `${!s"v"@}` therefore asks for
/// names beginning with the five characters `s"v"` (there are none), and
/// `${!s[k]@}` for names beginning with `s[k]` — *not* the element `s[k]`
/// indirected through, which is what the same body means with any other operator
/// on it. One that does not start like a name is left as a name no variable can
/// have, and bash refuses the whole word.
///
/// An *empty* prefix is neither: `${!@}` and `${!*}` indirect through the
/// positional list, because the character the scan reached is then the whole of
/// `name + 1` and there is nothing in front of it to judge.
fn scan_reaches_trailing_mark(after_bang: BStr<'_>) -> Option<(BStr<'_>, bool)> {
    let (&last, prefix) = after_bang.split_last()?;
    let star = match last {
        b'*' => true,
        b'@' => false,
        _ => return None,
    };
    if prefix.is_empty() {
        return None;
    }
    let mut i = 0usize;
    while let Some(&c) = prefix.get(i) {
        match c {
            // `string_extract` steps over the escaped character without ever
            // testing it, so a `\@` inside the body does not end the scan.
            b'\\' => i = i.saturating_add(2),
            // …and over a whole balanced subscript, which is what lets `s[k]`
            // and even `s[a:b]` stand in a prefix.
            b'[' => {
                let mut depth = 1usize;
                match subscript_close_in_lit(prefix.get(i.saturating_add(1)..)?, &mut depth) {
                    Some(close) => i = i.saturating_add(close).saturating_add(2),
                    None => i = i.saturating_add(1),
                }
            }
            c if NAME_SCAN_STOPS.contains(&c) => return None,
            _ => i = i.saturating_add(1),
        }
    }
    Some((prefix, star))
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
    ///
    /// `tail` is `None` for the one failure that has no last line: a scan that
    /// bailed to the parser (see [`ParseError::bail_sequel`]) leaves nothing to
    /// point "near", and `report_syntax_error` is never reached at all because
    /// `cond_term` returns through `COND_RETURN_ERROR` and `parse_cond_command`
    /// then skips both of its own messages (parse.y:4574). The group clauses
    /// still accumulate, though — `[[ ( ( x =~ " ) ) ]]` prints the bail message
    /// and two `expected \`)'` lines under it, and nothing else.
    Cond {
        clauses: Vec<(Option<u32>, Str)>,
        tail: Option<Str>,
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
            tail: Some(bfmt![tail]),
        }
    }

    /// A diagnostic that is only its last line — the primary position bash
    /// cannot name a token for (`[[ ( ]]`, whose `]]` leaves through an earlier
    /// arm) prints just the `near` line.
    fn bare(tail: &(impl bytes::PushBytes + ?Sized)) -> Self {
        CondError::Cond {
            clauses: Vec::new(),
            tail: Some(bfmt![tail]),
        }
    }

    /// A diagnostic that is only its *first* line, reported at `line` rather
    /// than wherever the enclosing error is: what `cond_term` prints when the
    /// lexer bailed to it. See the `tail` field and [`ParseError::bail_sequel`].
    fn sequel(line: u32, clause: &(impl bytes::PushBytes + ?Sized)) -> Self {
        CondError::Cond {
            clauses: vec![(Some(line), bfmt![clause])],
            tail: None,
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
        let mut msgs = Vec::new();
        let mut line_at = Vec::new();
        for (i, (line, text)) in clauses.iter().enumerate() {
            if let Some(l) = *line {
                line_at.push((u32::try_from(i).unwrap_or(u32::MAX), l));
            }
            msgs.push(text.clone());
        }
        // No tail is the bail case, which is also the case with no `near` line
        // to end on.
        let bail_sequel = tail.is_none();
        if let Some(tail) = tail {
            msgs.push(tail);
        }
        // Never empty: every constructor gives at least a clause or a tail.
        ParseError { msgs, line_at, bail_sequel, ..ParseError::new(b"") }
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
        String::from_utf8(e.msg()).expect("diagnostic is text")
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
            let prog = u.expect("parses");
            let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
                panic!("expected a simple command");
            };
            lines.push(sc.line);
        }
        assert_eq!(lines, vec![1, 3]);
    }

    /// The one NUL a token buffer can hold ends the *word* there, because
    /// `make_word` copies the buffer with `savestring`. The word's extent is
    /// untouched: what follows it is read exactly as it would have been.
    #[test]
    fn a_nul_spliced_into_a_brace_body_cuts_the_word() {
        let words = |src: &str| -> Vec<Word> {
            let prog = super::parse(src.as_bytes()).expect("parses");
            let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
                panic!("expected a simple command");
            };
            sc.words.clone()
        };

        // The cut word keeps its text up to the NUL and nothing after — not the
        // `}` the script wrote, and not the closing `"`.
        let w = words(r#"echo "${x:-$'a\0b'}" second"#);
        assert_eq!(w.len(), 3);
        let [WordPart::TokenText(raw)] = w[1].parts.as_slice() else {
            panic!("expected one cut part, got {:?}", w[1].parts);
        };
        assert_eq!(text(raw), r#""${x:-a"#);
        // …while the words after it are read as if nothing had happened.
        assert_eq!(text(&crate::unparse::word_src(&w[2])), "second");

        // A NUL that is the whole translation cuts just as hard.
        let w = words(r#"echo "${x:-$'\0'}""#);
        let [WordPart::TokenText(raw)] = w[1].parts.as_slice() else {
            panic!("expected one cut part, got {:?}", w[1].parts);
        };
        assert_eq!(text(raw), r#""${x:-"#);

        // The re-quoted rows hand the translation on as a C string, so no NUL
        // ever reaches the buffer and no word is cut.
        for src in [r#"echo "${q#$'z\0z'}""#, r#"echo ${x:-$'a\0b'}"#, r#"echo x$'a\0b'y"#] {
            let w = words(src);
            assert!(
                !w[1].parts.iter().any(|p| matches!(p, WordPart::TokenText(_))),
                "{src} should not cut"
            );
        }
    }

    /// The splice's *other* effect: a `}` or a quote it wrote lands in the
    /// token buffer live, so the expansion's read of the buffer carves
    /// something the parser's read did not. The word goes back to being text
    /// for the expander to read.
    #[test]
    fn a_brace_body_a_splice_closes_early_is_held_as_text() {
        let words = |src: &str| -> Vec<Word> {
            let prog = super::parse(src.as_bytes()).expect("parses");
            let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
                panic!("expected a simple command");
            };
            sc.words.clone()
        };
        let held = |src: &str| -> String {
            let w = words(src);
            let [WordPart::TokenText(raw)] = w[1].parts.as_slice() else {
                panic!("expected one text part, got {:?}", w[1].parts);
            };
            text(raw)
        };

        // `$'a}b'` is spliced bare, so its `}` closes the expansion where the
        // parser's `}` did not, and `b}` is ordinary word text.
        assert_eq!(held(r#"echo "${x:-$'a}b'}""#), r#""${x:-a}b}""#);
        // A spliced `"` closes the run the `${ … }` was written in, and the
        // script's own closing `"` then *opens* one that nothing closes — the
        // shape only a tolerant reader gets through.
        assert_eq!(held(r#"echo "${x:-$'a}b"'}""#), r#""${x:-a}b"}""#);
        // …and reading it is exactly what the tolerant mode is for: the
        // ordinary reader is the parser's, which is still hunting the end of a
        // word and so refuses text that runs out inside a quote.
        let opts = ParseOpts::default();
        let src = br#""${x:-a}b"}""#;
        assert!(super::word_verbatim_from_source_at(src, opts, super::Quoting::Bare, 1).is_err());
        assert!(super::word_tolerant_from_source_at(src, opts, super::Quoting::Bare, 1).is_ok());

        // A splice that writes nothing the expansion reads differently leaves
        // the tree alone.
        for src in [r#"echo "${x:-$'ab'}""#, r#"echo "${x:-$'a{b'}""#] {
            let w = words(src);
            assert!(
                !w[1].parts.iter().any(|p| matches!(p, WordPart::TokenText(_))),
                "{src} should keep its tree"
            );
        }
    }

    /// bash's second read of a `$( … )` is handed a pointer *into* the stored
    /// word, so what it reads is the re-print, the `)`, and the word's tail.
    #[test]
    fn a_reprint_that_will_not_read_back_is_read_with_its_tail() {
        let opts = ParseOpts::default();
        let err = |src: &str, tail: &str| {
            super::comsub_reprint_error(src.as_bytes(), tail.as_bytes(), opts)
                .map(|f| (emsg(&f.err), f.echo, f.line_off, f.fatal))
        };

        // The overwhelmingly common answer: the re-print reads back, stops at
        // the `)`, and never looks at the tail.
        assert!(err("echo hi", "\"").is_none());
        assert!(err("! false", "").is_none());

        // A re-print that reads back but will not parse names a token, and the
        // offending line is echoed under it.
        let (msg, echo, off, fatal) = err("!", "x").expect("fails");
        assert_eq!(msg, "syntax error near unexpected token `)'");
        assert!(echo);
        assert_eq!(off, 1);
        assert!(!fatal);

        // A re-print left unclosed swallows the `)` and reads on into the tail.
        // Still unterminated at the end of it: `parse_matched_pair`'s own
        // message, printed alone.
        let (msg, echo, off, fatal) = err(r#"echo "${x:-a"#, "\"").expect("fails");
        assert_eq!(msg, "unexpected EOF while looking for matching `\"'");
        assert!(!echo);
        assert_eq!(off, 1);
        assert!(!fatal);

        // A tail that closes it spends the `)` on the construct, so the `comsub`
        // production never gets one — and `line_number` has by then run one line
        // past the text, because the reader counts a line as it fetches it.
        let (msg, echo, off, fatal) = err(r#"echo "${x:-a"#, "}\"").expect("fails");
        assert_eq!(msg, "unexpected EOF while looking for matching `)'");
        assert!(!echo);
        assert_eq!(off, 2);
        assert!(!fatal);
        assert_eq!(err("echo one\necho \"${x:-a", "}\"").expect("fails").2, 3);

        // A `$( … )` nested in the re-print is `parse_comsub`'s own failure, and
        // that one ends the shell.
        let (_, _, _, fatal) = err(r#"echo "y$(! )z""#, "").expect("fails");
        assert!(fatal);
    }

    /// A compound assignment's value list is re-read by a reader that consults
    /// no grammar, so it can only fail by lexing.
    #[test]
    fn a_word_list_can_only_fail_by_lexing() {
        let opts = ParseOpts::default();
        let err = |src: &str| {
            super::word_list_lex_error(src.as_bytes(), opts).map(|e| (emsg(&e), e.fatal))
        };

        // Anything that lexes is a word list, however little sense it makes as a
        // command.
        assert!(err("one two three").is_none());
        assert!(err("do done esac ) ;;").is_none());
        assert!(err(r#"one "two three" $'four\tfive' "$(echo six)""#).is_none());

        // An unclosed construct is the listing's own failure: a discard.
        assert_eq!(
            err(r#""${x:-a tail"#),
            Some(("unexpected EOF while looking for matching `}'".into(), false))
        );
        assert_eq!(
            err("one $(echo two"),
            Some(("unexpected EOF while looking for matching `)'".into(), false))
        );

        // An error found *inside* an unterminated `$( … )` is `parse_comsub`'s,
        // and ends the shell instead.
        assert_eq!(err("one $(!;;").map(|(_, fatal)| fatal), Some(true));
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
        // A `\<newline>` the reader *deletes* still costs a fetch, and the
        // lookahead word's terminator scan is what walks into it. So the count
        // rises even though no token spans the two lines …
        assert_eq!(line_of("echo $LINENO\\\n"), 2);
        assert_eq!(line_of("echo $LINENO\\\n\\\n"), 3);
        // … and it does not rise when the lookahead ends before the deletion,
        // even though a *later* word runs into it.
        assert_eq!(line_of("echo $LINENO A\\\n"), 1);
        // An assignment takes no lookahead at all, so the deletion after the
        // command is none of its business.
        assert_eq!(line_of("v=1 echo $LINENO\\\n"), 1);
        // A backslash in front of the newline only deletes it when the run it
        // belongs to is odd: `read_token_word` reads the character after a `\`
        // with continuation removal off, so the run is consumed in pairs and an
        // even one leaves an ordinary newline behind. bash 5.2.37 numbers the
        // one-word command 3, 1, 3, 1 for runs of 1 to 4.
        assert_eq!(line_of("nosuch$LINENO\\\n"), 3);
        assert_eq!(line_of("nosuch$LINENO\\\\\n"), 1);
        assert_eq!(line_of("nosuch$LINENO\\\\\\\n"), 3);
        assert_eq!(line_of("nosuch$LINENO\\\\\\\\\n"), 1);
        // A *string* whose last line ended on an odd run is closed with another
        // backslash instead of with a newline ([`close_last_line`]), and then
        // there is no newline for the lookahead's terminator scan to stop on:
        // it runs the buffer out and enters the fetch block. (These are the
        // post-close texts — `parse` does not apply the rule itself.) Sourcing
        // a one-line file `echo $LINENO\` from bash 5.2.37 prints `2\`, and
        // `echo $LINENO A\` prints `1 A\`: a second word takes the lookahead's
        // place, so the scan that runs out is no longer the one that stamps.
        assert_eq!(line_of("echo $LINENO\\\\"), 2);
        assert_eq!(line_of("echo $LINENO A\\\\"), 1);
    }

    /// bash's `line_number` counts *fetches*, not lines of text, and deleting a
    /// `\<newline>` is a fetch: `shell_getc` bumps the counter and re-reads
    /// (parse.y:2361). On top of that, the parser's request for a lookahead token
    /// that is not there enters the same fetch block and costs one more. So an
    /// input whose last line is one or more continuations blames its syntax error
    /// several lines past the last line that has any text on it. Every
    /// expectation below is bash 5.2.37's own message for the same script file.
    /// See [`Parser::reader_line_at`] and [`Lexer::reader_at_eof`].
    #[test]
    fn a_continuation_at_end_of_input_moves_the_reader() {
        /// The (line, message) of the first failing unit, parsed the way a script
        /// file is — through the streaming parser, which stamps error lines from
        /// the reader's position rather than the token's.
        fn err(src: &str) -> (u32, String) {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            while let Some(unit) = ip.next_unit(None, opts) {
                let Err(e) = unit else { continue };
                return (e.line.unwrap_or(0), emsg(&e));
            }
            panic!("{src:?} must fail");
        }
        let eof = "syntax error: unexpected end of file";
        // Sources are written as `st_stream` hands them over: a script whose last
        // line is a lone `\` has a `\n` appended to it, so a trailing continuation
        // appears here as a complete `\<newline>`.
        for (src, want) in [
            // The baseline: no continuation. The `>` has nothing to redirect to,
            // and the newline it finds is the offending token on line 2.
            ("echo 1\ncat >\n", (2, "syntax error near unexpected token `newline'")),
            // One continuation deletes that newline, so the error becomes an
            // end-of-file — reported one line further on for each deletion.
            ("echo 1\ncat >\\\n", (3, eof)),
            ("echo 1\ncat >\\\n\\\n", (4, eof)),
            ("echo 1\ncat >\\\n\\\n\\\n", (5, eof)),
            // A compound left open runs out of input either way; the continuations
            // only move the line. Note the no-continuation form is *already* one
            // past the text: the parser's request for the token that never comes
            // is itself a fetch.
            ("echo 1\nif true; then\n", (3, eof)),
            ("echo 1\nif true; then\\\n", (4, eof)),
            ("echo 1\nif true; then\\\n\\\n", (5, eof)),
            ("echo 1\n{ echo hi\n", (3, eof)),
            ("echo 1\n{ echo hi\\\n", (4, eof)),
            ("echo 1\n{ echo hi\\\n\\\n", (5, eof)),
            // `|` is one character but bash still peeks past it, to tell it from
            // `||` and `|&`. That peek deletes every trailing continuation and
            // then finds the end of input — and `shell_ungetc` stows it, since it
            // is at the start of the line just fetched, so the parser's own
            // request gets it back for free. The count is therefore exactly the
            // continuations, with no post-EOF bump on top: the same total as the
            // no-continuation form, which pays only the bump.
            ("echo 1\necho a |\n", (3, eof)),
            ("echo 1\necho a |\\\n", (3, eof)),
            ("echo 1\necho a |\\\n\\\n", (4, eof)),
            // `&&` is the other shape: bash completed it *by* its own lookahead
            // and never looked again, so the reader is parked in front of the
            // whole run and the request has to delete it — one line each, and
            // one line even when there is nothing to delete.
            ("echo 1\necho a &&\n", (3, eof)),
            ("echo 1\necho a &&\\\n", (3, eof)),
            ("echo 1\necho a &&\\\n\\\n", (4, eof)),
            ("echo 1\necho a &&\\\n\\\n\\\n", (5, eof)),
            ("echo 1\necho a 2>&\\\n\\\n", (4, eof)),
            // A run that does *not* reach the end of input is none of the
            // request's business: the fetch after the last deletion brings that
            // line in, and its own newline is the last token instead.
            ("echo 1\necho a &&\\\n   \n", (4, eof)),
            ("echo 1\necho a &&\\\n\\\n   \n", (5, eof)),
            // A token the grammar rejects outright is blamed on the reader's line
            // too, so a continuation flush after it moves the blame even though
            // the token itself did not move.
            ("echo 1\necho )\n", (2, "syntax error near unexpected token `)'")),
            ("echo 1\necho )\\\n", (3, "syntax error near unexpected token `)'")),
            ("echo 1\nesac\\\n", (3, "syntax error near unexpected token `esac'")),
            ("echo 1\ndone\\\n", (3, "syntax error near unexpected token `done'")),
            ("echo 1\nfi\\\n", (3, "syntax error near unexpected token `fi'")),
            ("echo 1\necho a;;\\\n", (3, "syntax error near unexpected token `;;'")),
        ] {
            let (line, msg) = err(src);
            assert_eq!((line, msg.as_str()), want, "src {src:?}");
        }
    }

    /// `for`/`select` ask for one token more than anything else, because their
    /// `list_terminator` (parse.y:517) accepts the end-of-file token itself: the
    /// EOF the word list ran into is *consumed* as the terminator, and the
    /// `newline_list` behind it has to request another, which enters the fetch
    /// block a second time. So the same tail costs one more line after a word
    /// list than after any other header. See [`Parser::eof_closed_in_list`].
    /// Every expectation is bash 5.2.37's own, measured on a script file.
    #[test]
    fn a_for_lists_terminator_can_be_the_end_of_input_itself() {
        fn line_of(src: &str) -> u32 {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            while let Some(unit) = ip.next_unit(None, opts) {
                let Err(e) = unit else { continue };
                assert_eq!(emsg(&e), "syntax error: unexpected end of file", "src {src:?}");
                return e.line.unwrap_or(0);
            }
            panic!("{src:?} must fail");
        }
        for (src, want) in [
            // The reference shape, which has no `list_terminator` at all: one
            // deletion, then one request that finds nothing.
            ("echo 1\nwhile true\\\n", 4),
            ("echo 1\nwhile true\\\n\\\n", 5),
            // The same tail after a word list is one further along, every time.
            ("echo 1\nfor i in a\\\n", 5),
            ("echo 1\nfor i in a\\\n\\\n", 6),
            ("echo 1\nfor i in a b\\\n", 5),
            ("echo 1\nfor i in\\\n", 5),
            ("echo 1\nselect v in a\\\n", 5),
            ("echo 1\nselect v in\\\n", 5),
            // …including where the word ended before the run, so that the extra
            // request is the only thing separating the two.
            ("echo 1\nwhile true  \\\n", 3),
            ("echo 1\nfor i in a  \\\n", 4),
            // `do` is swallowed by the word list (no separator before it), which
            // is why this is an end-of-file rather than a missing `do`.
            ("echo 1\nfor i in a do\\\n", 5),
            // Nesting does not matter: it is the rule being reduced that asks
            // twice, not the depth it is reduced at.
            ("echo 1\nif true; then for i in a\\\n", 5),
            ("echo 1\nfor j in x; do for i in a\\\n", 5),
            // A real terminator takes the end-of-file token's place, and then the
            // `newline_list` request is the *first* to run the buffer out.
            ("echo 1\nfor i in a;\\\n", 3),
            ("echo 1\nfor i in a ;\\\n", 3),
            ("echo 1\nfor i in a\n", 3),
            ("echo 1\nfor i in a", 3),
            // No `in` at all is not a word list, so no `list_terminator` either.
            ("echo 1\nfor i\\\n", 4),
            ("echo 1\nfor i\\\n\\\n", 5),
            ("echo 1\nselect v\\\n", 4),
        ] {
            assert_eq!(line_of(src), want, "src {src:?}");
        }
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
        assert_eq!(e.msg(), b"syntax error near unexpected token `a\xffb'");

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
            e.msg(),
            b"syntax error in conditional expression\nsyntax error near `a\xffb'"
        );

        // A construct left open is reported by the lexer, whose message carries
        // the same bytes and still classifies as *incomplete* — so a REPL line
        // holding one keeps reading rather than erroring out.
        let e = super::parse(b"echo \"a\xffb").unwrap_err();
        assert_eq!(e.msg(), b"unexpected EOF while looking for matching `\"'");
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

    /// An arithmetic `for` header whose closing parentheses are not adjacent is
    /// the one construct outside `[[ … ]]` that bash blames **by position**
    /// rather than by name: `parse_dparen` returns −1, bison reads that as EOF,
    /// `error_token_from_token` declines to name it, and `report_syntax_error`
    /// falls through to slicing the input around where the reader stopped. Every
    /// expectation here is bash 5.2.37's own wording. See [`Tok::Refused`] and
    /// the corpus case of the same name.
    #[test]
    fn a_for_header_that_fails_the_adjacency_test_is_blamed_by_position() {
        fn err(src: &str) -> (String, Option<u32>, Option<String>) {
            let e = parse(src).expect_err("expected a parse error");
            (emsg(&e), e.line, e.echo.as_deref().map(text))
        }
        // The slice runs back to the nearest ` `, `\n`, `\t`, `;`, `|` or `&`
        // and forward to the one character `parse_arith_cmd`'s test read, so a
        // space before the stray `)` cuts it down to that character alone …
        let near = "syntax error near ";
        assert_eq!(
            err("for ((i=0;i<1;i++) ) ; do echo $i; done").0,
            format!("{near}`)'")
        );
        // … while text written flush against the body drags in the tail of the
        // expression, back to the `;` inside it.
        assert_eq!(err("for ((i=0;i<1;i++)x ; do :; done").0, format!("{near}`;i++)x'"));
        assert_eq!(err("for ((i=0;i<1;i++)  ; do :; done").0, format!("{near}`;i++)'"));
        assert_eq!(err("for ((1)  ; do :; done").0, format!("{near}`((1)'"));
        // The character read may be the end of the line. `shell_getc (0)` does
        // not fetch, so the reader is left at the NUL past the line it is on and
        // both the slice and the echoed line are still that line's — a plain
        // newline and a `\<newline>` alike.
        assert_eq!(err("for ((1)").0, format!("{near}`((1)'"));
        let (msg, line, echo) = err("for ((i=0;i<1;i++)\n) ; do :; done");
        assert_eq!(msg, format!("{near}`;i++)'"));
        assert_eq!(line, Some(1));
        assert_eq!(echo, None, "the shell's own input is echoed by line number");
        assert_eq!(err("for ((i=0;i<1;i++)\\\n) ; do :; done").0, format!("{near}`;i++)\\'"));
        // It is a whole-input failure, not a recoverable one: bison is holding
        // EOF and there is nothing to resume from.
        assert!(!parse("for ((1) )").expect_err("error").recoverable);
        assert!(!parse("for ((1) )").expect_err("error").is_incomplete());
        // Only a `for` header. Everywhere else the same failed test falls back
        // to nested subshells, and `((` after a word is not arithmetic at all.
        assert_eq!(
            err("select x in ((1) ) ; do :; done").0,
            "syntax error near unexpected token `('"
        );
        assert!(parse("((1) )").is_ok());
        assert!(parse("for ((i=0;i<2;i++)); do :; done").is_ok());
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
        let err = |src: &str| String::from_utf8_lossy(&parse(src).unwrap_err().msg()).into_owned();
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

    /// An arithmetic command is named in a syntax error by the expression it
    /// collected, not by a paren.
    ///
    /// `error_token_from_token` returns `string_list (yylval.word_list)` for an
    /// `ARITH_CMD` (parse.y), which is the re-printed body — verbatim, inner
    /// spaces and all. So the name is also the proof that the `((` really did
    /// take the arithmetic path: a `((` that stayed two parens is named `(`.
    /// Every expectation is bash 5.2.37's own.
    #[test]
    fn an_arithmetic_command_is_named_by_its_expression() {
        let err = |src: &str| String::from_utf8_lossy(&parse(src).unwrap_err().msg()).into_owned();
        // `]]` is `COND_END`, which is on bash's acceptable list, so the `((`
        // after a conditional is arithmetic and the grammar objects to *it*.
        assert_eq!(err("[[ a ]] ((1))"), "syntax error near unexpected token `1'");
        assert_eq!(err("[[ a ]] (( 1 + 1 ))"), "syntax error near unexpected token ` 1 + 1 '");
        assert_eq!(err("[[ a ]] ((x = 1))"), "syntax error near unexpected token `x = 1'");
        assert_eq!(
            err("[[ a ]] (( $(echo 2) ))"),
            "syntax error near unexpected token ` $(echo 2) '"
        );
        assert_eq!(err("[[ a ]] ((  ))"), "syntax error near unexpected token `  '");
        // Where the `((` was not arithmetic, the paren is still the name.
        assert_eq!(err("echo ]] ((1))"), "syntax error near unexpected token `('");
        assert_eq!(err("echo ((1))"), "syntax error near unexpected token `('");
    }

    /// A `NUMBER` and a compound `ASSIGNMENT_WORD` are named by what
    /// `error_token_from_token` renders them as, and neither is the source text.
    ///
    /// A `NUMBER` goes through `itos (yylval.number)`, so it is the *value*: the
    /// leading zeros of `007` are gone. A compound assignment goes through
    /// `yylval.word->word`, which `read_token_word` built as the name, `=`, `(`,
    /// the `string_list` of the elements — joined with one space, whatever was
    /// written between them — and `)` (parse.y:5168–5181, 6572–6576). Each
    /// element keeps its own spelling; only the gaps are normalised. A function
    /// body is the shortest position that reaches both. Every expectation is
    /// bash 5.2.37's own.
    #[test]
    fn a_number_and_a_compound_assignment_are_named_by_their_reprint() {
        let err = |src: &str| String::from_utf8_lossy(&parse(src).unwrap_err().msg()).into_owned();
        // The value, not the spelling.
        assert_eq!(err("f() 007>x"), "syntax error near unexpected token `7'");
        assert_eq!(err("f() 2>/dev/null"), "syntax error near unexpected token `2'");
        assert_eq!(err("f() 12>&1"), "syntax error near unexpected token `12'");
        // The elements' own spellings, joined by exactly one space.
        assert_eq!(err("f() a=(1   2)"), "syntax error near unexpected token `a=(1 2)'");
        assert_eq!(err("f() a=(  )"), "syntax error near unexpected token `a=()'");
        assert_eq!(
            err("f() a=('x y' \"z\")"),
            "syntax error near unexpected token `a=('x y' \"z\")'"
        );
        assert_eq!(err("f() a=([2]=v x)"), "syntax error near unexpected token `a=([2]=v x)'");
        // The name part is carried through whole: the subscript verbatim, and
        // the `+` of an append still ahead of the `=`.
        assert_eq!(err("f() a+=(p)"), "syntax error near unexpected token `a+=(p)'");
        assert_eq!(err("f() a[1+1]=(q)"), "syntax error near unexpected token `a[1+1]=(q)'");
        assert_eq!(
            err("f() a=($(echo h) ~/t)"),
            "syntax error near unexpected token `a=($(echo h) ~/t)'"
        );
        // A `REDIR_WORD` has no branch in the switch at all, so
        // `error_token_from_token` returns NULL and `report_syntax_error` falls
        // through to its text-scanning branch. Both halves of the message
        // change: `unexpected token` is gone, and the name is the scan's — which
        // reaches one character past the token, so the `>` comes along.
        assert_eq!(err("f() {v}>/dev/null"), "syntax error near `{v}>'");
        assert_eq!(err("f() {v}<in"), "syntax error near `{v}<'");
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

    /// A `case` arm's body ends where any other command list does.
    ///
    /// bash's arm is `pattern ')' compound_list`, and `compound_list` may reduce
    /// with no trailing `;`/`&`/newline at all (`newline_list list1`) — so a
    /// body *can* end bare, but only immediately before something that receives
    /// the arm: `;;`, `;&`, `;;&` (`case_clause_sequence`) or `esac`
    /// (`case_clause`). Anything else is two commands abutting, which is the
    /// same syntax error it is anywhere else and is blamed on the abutting
    /// token — not on the `;;` further along.
    #[test]
    fn case_arm_body_ends_like_any_other_list() {
        for (src, want) in [
            // The bug this pins: the `(` was counted as an *open* rather than
            // rejected, so the error only surfaced when the `;;` arrived.
            ("case a in a) echo x( y;; esac", "syntax error near unexpected token `('"),
            ("case a in a) echo x) y;; esac", "syntax error near unexpected token `)'"),
            ("case a in a) { echo x; }( y;; esac", "syntax error near unexpected token `('"),
            ("case a in a) { echo x; } ) ;; esac", "syntax error near unexpected token `)'"),
            // Two *complete* commands abutting was accepted outright before.
            ("case a in a) ( : ) ( : ) ;; esac", "syntax error near unexpected token `('"),
            ("case a in a) { :; } { :; } ;; esac", "syntax error near unexpected token `{'"),
            // Nested arms follow the same rule.
            (
                "case a in a) case b in b) echo n( m;; esac;; esac",
                "syntax error near unexpected token `('",
            ),
            // `esac` after a simple command's word is an *argument*, not an
            // ender, so the `case` runs off the end of the input instead.
            ("case a in a) echo x esac", "syntax error: unexpected end of file"),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), want, "src {src:?}");
        }
        // A bare body — no separator — is fine before each of the four enders.
        for src in [
            "case a in a) { echo x; } esac",
            "case a in a) ( echo x ) esac",
            "case a in a) { echo x; } ;; esac",
            "case a in a) { echo x; } ;& esac",
            "case a in a) { echo x; } ;;& esac",
            // …as is any body that does carry one.
            "case a in a) echo x\nesac",
            "case a in a) echo x& esac",
            "case a in a) echo x; esac",
            // Reserved-looking words after the command word are arguments.
            "case a in a) echo x fi done then do } y;; esac",
        ] {
            assert!(parse(src).is_ok(), "src {src:?}");
        }
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
    fn a_prefix_spends_the_position_where_a_reserved_word_would_be_acceptable() {
        // `CHECK_FOR_RESERVED_WORD` consults `word_token_alist` only where
        // `reserved_word_acceptable (last_read_token)` holds (parse.y:5367), and
        // that list is separators, operators and other reserved words — never a
        // `WORD`. An assignment prefix is a `WORD`, and so is a redirection's
        // filename, so after either one a reserved word is an ordinary command
        // name: bash runs it and reports `done: command not found`.
        for word in [
            "done", "then", "fi", "do", "esac", "in", "}", "!", "{", "]]", "elif", "else", "while",
            "until", "if", "for", "case", "select",
        ] {
            for src in [format!("v=1 {word}"), format!(">/dev/null {word}")] {
                let prog = parse(&src).unwrap_or_else(|e| panic!("{src:?}: {}", emsg(&e)));
                let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
                    panic!("expected a simple command for {src:?}");
                };
                let [WordPart::Literal(w)] = sc.words[0].parts.as_slice() else {
                    panic!("expected one literal part for {src:?}, got {:?}", sc.words[0].parts);
                };
                assert_eq!(text(w), word, "src {src:?}");
            }
        }

        // The compound openers come along: `v=1 if true; then …` runs a command
        // called `if` with the argument `true`, and it is the `then` after the
        // `;` — a position where a reserved word *is* acceptable — that bash
        // names.
        assert_eq!(
            emsg(&parse("v=1 if true; then echo y; fi").unwrap_err()),
            "syntax error near unexpected token `then'"
        );
        // Same rule from the other side for the keyword function form: after
        // `v=1` the `function` and the name are words, the `{` after them is a
        // word too, and the `}` after the `;` is the token that has no place.
        assert_eq!(
            emsg(&parse("v=1 function f { :; }").unwrap_err()),
            "syntax error near unexpected token `}'"
        );

        // Nothing about the positions where a reserved word *is* acceptable
        // changes: with no prefix in front of it the word still opens (or is
        // rejected as) a compound, including inside a loop body.
        assert_eq!(
            emsg(&parse("for i in 1; do done; done").unwrap_err()),
            "syntax error near unexpected token `done'"
        );
        assert!(parse("for i in 1; do v=1 done; done").is_ok());
        // And a prefixed `while` is a command called `while`, so its `do` — the
        // first token in an acceptable position — is what has no place.
        assert_eq!(
            emsg(&parse("v=1 while true; do :; done").unwrap_err()),
            "syntax error near unexpected token `do'"
        );

        // `COPROC` is itself on the list, so the word after it is looked up like
        // any other. bash's production is `COPROC WORD compound_command` and a
        // reserved word is not a `WORD`, so it cannot be a coproc's name.
        for word in ["done", "esac", "fi", "then"] {
            let src = format!("coproc {word} {{ echo y; }}");
            assert_eq!(
                emsg(&parse(&src).unwrap_err()),
                format!("syntax error near unexpected token `{word}'"),
                "src {src:?}"
            );
        }
        // The compound openers after `coproc` still open their compound, and an
        // ordinary name is still a name.
        for src in [
            "coproc if true; then echo y; fi",
            "coproc while false; do :; done",
            "coproc { echo y; }",
            "coproc mine { echo y; }",
            "coproc echo y",
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

    /// A matched-pair scan that runs off the end of the input does not abort
    /// bash's parse. `read_token_word` prints its own message and bails *to the
    /// parser* — `return -1; /* Bail immediately. */` (parse.y:5151) — which is
    /// handed a token that is not a `WORD` and objects to it in its turn. So a
    /// `[[ … ]]` whose operand dies at EOF draws two diagnostics where every
    /// other truncated construct draws one, and the second is reported at the
    /// line the failed scan ran to: one past the input's last, since `shell_getc`
    /// counted every line it swallowed looking for the close.
    ///
    /// The second names no token, `error_token_from_token(-1)` being able to
    /// spell none — so the two operator forms lose their `` `X' `` and the
    /// primary form falls to a `%c` of `-1`, the byte 0xFF. None of them gets a
    /// `near` line either: `cond_term` leaves through `COND_RETURN_ERROR`, and
    /// `parse_cond_command` prints nothing more once `cond_token` is
    /// `COND_ERROR` (parse.y:4574). Every row is measured against bash 5.2.37.
    #[test]
    fn a_conditional_operand_that_dies_at_eof_draws_a_second_diagnostic() {
        /// The error `src` ends on, each message line tagged with the line it is
        /// reported at — which is the whole point here.
        ///
        /// Read through [`IncrementalParser`] because that is where a parked
        /// lexer error lives; `parse` goes by way of `tokenize_spanned`, which
        /// gives up on one and so never reaches the parser at all.
        fn diag(src: &str) -> String {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            while let Some(unit) = ip.next_unit(None, opts) {
                let Err(e) = unit else { continue };
                let line = e.line.unwrap_or(0);
                return e
                    .msgs
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        // Latin-1, so the one non-ASCII byte these messages can
                        // carry — 0xFF — survives the comparison rather than
                        // being replaced by a decode that cannot spell it.
                        let m: String = m.iter().map(|&b| char::from(b)).collect();
                        format!("{}: {m}", e.line_of(i, line))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            panic!("{src:?} must fail")
        }
        let binary = "unexpected argument to conditional binary operator";
        let unary = "unexpected argument to conditional unary operator";
        let primary = "unexpected token `\u{ff}' in conditional command";
        // Every construct the scan can die inside, in each of the three operand
        // positions. The first line is the scan's, at the line it gave up on.
        for (src, close, second) in [
            ("[[ x =~ ( ]]\n", ")", binary),
            ("[[ x =~ (a ]]\n", ")", binary),
            ("[[ x =~ \" ]]\n", "\"", binary),
            ("[[ x =~ ` ]]\n", "`", binary),
            ("[[ x =~ ${ ]]\n", "}", binary),
            ("[[ -n \" ]]\n", "\"", unary),
            ("[[ \" ]]\n", "\"", primary),
            // `!` and `&&` reach the same slots.
            ("[[ ! x =~ ( ]]\n", ")", binary),
            ("[[ -n x && \" ]]\n", "\"", primary),
            // The input's last line is a line even with no newline of its own:
            // the reader ends it at EOF exactly as it would at `\n`.
            ("[[ x =~ ( ]]", ")", binary),
        ] {
            let want = format!(
                "1: unexpected EOF while looking for matching `{close}'\n2: {second}"
            );
            assert_eq!(diag(src), want, "src {src:?}");
        }
        // A `$( … )` is scanned by a reader of its own, which has already taken
        // the newline by the time it gives up — so *both* lines are the one past.
        assert_eq!(
            diag("[[ x =~ $(echo a ]]\n"),
            format!("2: unexpected EOF while looking for matching `)'\n2: {binary}"),
        );
        assert_eq!(
            diag("[[ $(echo a ]]\n"),
            format!("2: unexpected EOF while looking for matching `)'\n2: {primary}"),
        );
        // Each enclosing group still adds the `)` it never reached, after the
        // bail message and at the line its own `(` was on.
        assert_eq!(
            diag("[[ ( ( x =~ \" ) ) ]]\n"),
            format!(
                "1: unexpected EOF while looking for matching `\"'\n2: {binary}\n1: expected `)'\n1: expected `)'"
            ),
        );
        // The line is where the reader stopped, not where the operator was, so
        // every line the scan swallowed counts.
        assert_eq!(
            diag("echo A\n[[ x =~ (\none\ntwo\n"),
            format!("2: unexpected EOF while looking for matching `)'\n5: {binary}"),
        );
        assert_eq!(
            diag("echo A\n[[ x =~ ( ]]\necho B\necho C\n"),
            format!("2: unexpected EOF while looking for matching `)'\n5: {binary}"),
        );
        // Not every unclosed operand is a bail. `[[ x == ( ]]` has no
        // `PST_REGEXP`, so the `(` is never handed to `parse_matched_pair` and
        // is an ordinary token the parser objects to where it stands; and an
        // input that simply *ends* leaves the grammar to report it, with the
        // end-of-file forms this change must not disturb.
        for (src, want) in [
            (
                "[[ x == ( ]]\n",
                "1: unexpected argument `(' to conditional binary operator\n1: syntax error near `('",
            ),
            ("[[ ( ]]\n", "1: expected `)'\n1: syntax error near `]]'"),
            (
                "[[ a == b\n",
                "1: unexpected EOF while looking for `]]'\n2: syntax error: unexpected end of file",
            ),
            (
                "[[ a &&\n",
                "2: unexpected token `EOF' in conditional command\n2: syntax error: unexpected end of file",
            ),
        ] {
            assert_eq!(diag(src), want, "src {src:?}");
        }
        // And a closed group is no failure at all.
        assert!(parse("[[ x =~ ( ) ]]").is_ok());
    }

    /// The end of input is a *token* the conditional grammar rejects like any
    /// other, not a missing bracket.
    ///
    /// bash has two answers for "the input ended inside `[[ … ]]`" and picks by
    /// whether the conditional grammar still wanted something. If it did,
    /// `cond_term` is handed `yacc_EOF` and objects to it where it stands —
    /// `error_token_from_token` spells that one, so the message carries
    /// `` `EOF' ``. Only a conditional that is *complete* and merely unclosed
    /// gets ``unexpected EOF while looking for `]]'``, and that one is reported
    /// on the line the reader gave up rather than on the end-of-file line.
    ///
    /// None of the EOF forms carries a `near` line: the fetch that found the
    /// end left `shell_input_line` empty, so `report_syntax_error` falls past
    /// both `near` branches (parse.y:6273).
    ///
    /// Every row is bash 5.2.37, measured by sourcing a two-line file whose
    /// second line ends in the given text.
    #[test]
    fn the_end_of_input_is_a_token_the_conditional_grammar_rejects() {
        fn diag(src: &str) -> String {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            while let Some(unit) = ip.next_unit(None, opts) {
                let Err(e) = unit else { continue };
                let line = e.line.unwrap_or(0);
                return e
                    .msgs
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let m: String = m.iter().map(|&b| char::from(b)).collect();
                        format!("{}: {m}", e.line_of(i, line))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            panic!("{src:?} must fail")
        }
        let eof = "syntax error: unexpected end of file";
        for (src, want) in [
            // The three operand slots, each naming the token it was handed.
            (
                "echo 1\n[[ a\\\n",
                format!("4: unexpected token `EOF', conditional binary operator expected\n4: {eof}"),
            ),
            (
                "echo 1\n[[ a ==\\\n",
                format!("4: unexpected argument `EOF' to conditional binary operator\n4: {eof}"),
            ),
            (
                "echo 1\n[[ -f\\\n",
                format!("4: unexpected argument `EOF' to conditional unary operator\n4: {eof}"),
            ),
            (
                "echo 1\n[[ !\\\n",
                format!("4: unexpected token `EOF' in conditional command\n4: {eof}"),
            ),
            // A complete expression wants only `]]`, and says so — on the line
            // the reader stopped, which is *not* the end-of-file line.
            (
                "echo 1\n[[ a == b\\\n",
                format!("2: unexpected EOF while looking for `]]'\n4: {eof}"),
            ),
            // A group adds the `)` it never reached at the line `cond_term`
            // captured once it had read the `(` — which a peek for `((` has
            // already carried past a `\<newline>` written flush against it. So
            // the same group is numbered 3 with the continuation and 2 without.
            (
                "echo 1\n[[ (\\\n",
                format!("3: unexpected token `EOF' in conditional command\n3: expected `)'\n3: {eof}"),
            ),
            (
                "echo 1\n[[ (\n",
                format!("3: unexpected token `EOF' in conditional command\n2: expected `)'\n3: {eof}"),
            ),
            (
                "echo 1\n[[ ( (\\\n",
                format!(
                    "3: unexpected token `EOF' in conditional command\n3: expected `)'\n2: expected `)'\n3: {eof}"
                ),
            ),
            // The failure inside a group is still the group's business, and the
            // binary-position form reaches it too.
            (
                "echo 1\n[[ ( a\\\n",
                format!(
                    "4: unexpected token `EOF', conditional binary operator expected\n2: expected `)'\n4: {eof}"
                ),
            ),
        ] {
            assert_eq!(diag(src), want, "src {src:?}");
        }
    }

    /// The line under `syntax error in conditional expression …` is the
    /// *reader's* to give, and a backslash-closed string leaves it with nothing.
    ///
    /// A conditional failure hands the grammar the `-1` of parse.y:3402, which
    /// `error_token_from_token` cannot name, so `report_syntax_error` skips its
    /// first branch and tries the one gated on `shell_input_line &&
    /// *shell_input_line` (parse.y:6273). With text left it reports `syntax
    /// error near \`X'` and echoes the line; with the line emptied it falls to
    /// the final `else` and prints a bare `syntax error` with nothing under it.
    ///
    /// Only a fetch that found the end of the input empties the buffer, and a
    /// newline-closed string never provokes one — the reader stops on the
    /// newline. So the shape needs a `\<newline>` flush against the offending
    /// token: the look past it deletes the continuation, runs the buffer out and
    /// fetches nothing. One space in between and the read stops there instead,
    /// with the whole line — trailing backslash and all — still in hand.
    ///
    /// `EOF_Reached` is 0 for every row, which is why these say `syntax error`
    /// and not `syntax error: unexpected end of file`: the conditional died on a
    /// token it *had*. Every row is bash 5.2.37.
    #[test]
    fn a_backslash_closed_string_leaves_no_line_to_report_near() {
        fn diag(src: &str) -> String {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            while let Some(unit) = ip.next_unit(None, opts) {
                let Err(e) = unit else { continue };
                let line = e.line.unwrap_or(0);
                return e
                    .msgs
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let m: String = m.iter().map(|&b| char::from(b)).collect();
                        format!("{}: {m}", e.line_of(i, line))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            panic!("{src:?} must fail")
        }
        for (src, want) in [
            // The token ran the input out: no `near` line, and so — since the
            // echo is keyed off one — nothing echoed either.
            (
                "echo 1\n[[ a == b )\\\n",
                "2: syntax error in conditional expression: unexpected token `)'\n3: syntax error",
            ),
            (
                "echo 1\n[[ a == b ;\\\n",
                "2: syntax error in conditional expression: unexpected token `;'\n3: syntax error",
            ),
            ("echo 1\n[[ -z x y\\\n", "2: syntax error in conditional expression\n3: syntax error"),
            ("echo 1\n[[ a b\\\n", "3: conditional binary operator expected\n3: syntax error"),
            // A group keeps its own `expected `)'` — that is a separate
            // `parser_error` from the frame that read the `(`, reported on the
            // `(`'s line and untouched by which branch the sequel takes.
            (
                "echo 1\n[[ ( a ;\\\n",
                "3: unexpected token `;', conditional binary operator expected\n2: expected `)'\n3: syntax error",
            ),
            // One space, and the read stops on it with the line still there —
            // backslash included, since the echo is the raw line.
            (
                "echo 1\n[[ a == b ) \\\n",
                "2: syntax error in conditional expression: unexpected token `)'\n2: syntax error near `)'",
            ),
            (
                "echo 1\n[[ ( a ; \\\n",
                "2: unexpected token `;', conditional binary operator expected\n2: expected `)'\n2: syntax error near `;'",
            ),
            // …and so does a token whose own text is not flush against the
            // continuation, whatever stands between.
            (
                "echo 1\n[[ a -eq b -eq c\\\n",
                "2: syntax error in conditional expression\n2: syntax error near `-eq'",
            ),
            (
                "echo 1\n[[ a b )\\\necho 2\n",
                "2: conditional binary operator expected\n2: syntax error near `b'",
            ),
            // The fetch found a line rather than the end, so the buffer is not
            // empty — it holds *that* line, which is what gets reported.
            (
                "echo 1\n[[ a == b )\\\necho 2\n",
                "2: syntax error in conditional expression: unexpected token `)'\n3: syntax error near `e'",
            ),
            (
                "echo 1\n[[ a == b ;\\\necho 2\n",
                "2: syntax error in conditional expression: unexpected token `;'\n3: syntax error near `e'",
            ),
            // Not a conditional at all: an ordinary error leaves through
            // `report_syntax_error`'s *first* branch, whose
            // `print_offending_line` is unconditional (parse.y:6262). So it
            // keeps both lines and simply echoes an empty one.
            ("echo 1\n;;\\\n", "3: syntax error near unexpected token `;;'"),
            ("echo 1\necho a; )\\\n", "3: syntax error near unexpected token `)'"),
            ("echo 1\n[[ a == b ]] ;;\\\n", "3: syntax error near unexpected token `;;'"),
        ] {
            assert_eq!(diag(src), want, "src {src:?}");
        }

        // A `-c` string closed with a *backslash* is the other way to leave the
        // reader with nothing: it has no newline to stop on at all. Which of the
        // two it is depends on the token, not the input — `[[ a b\` ends on the
        // word, whose scan takes both backslashes and runs the buffer out, while
        // `[[ a == b )\` ends on the `)`, whose look stops *on* the first
        // backslash with the line still in hand. So the same close empties the
        // buffer in one and not the other, and bash echoes the doubled `\` it
        // appended:
        //
        // ```text
        // bash -c '[[ a b\'          line 2: conditional binary operator expected
        //                            line 2: syntax error
        // bash -c '[[ a == b )\'     line 1: syntax error in conditional …
        //                            line 1: syntax error near `)\'
        //                            line 1: `[[ a == b )\\'
        // ```
        let closed = |src: &str| -> String {
            let src = close_last_line(src.as_bytes(), InputKind::Str).into_owned();
            let text: String = src.iter().map(|&b| char::from(b)).collect();
            diag(&text)
        };
        assert_eq!(closed("[[ a b\\"), "2: conditional binary operator expected\n2: syntax error");
        assert_eq!(
            closed("[[ a == b )\\"),
            "1: syntax error in conditional expression: unexpected token `)'\n1: syntax error near `)\\'"
        );
        assert_eq!(
            closed("[[ a == b ) \\"),
            "1: syntax error in conditional expression: unexpected token `)'\n1: syntax error near `)'"
        );
    }

    /// `error_token_from_text` scans **one line**, so the scan has a floor.
    ///
    /// It reads `t = shell_input_line`, which holds a single line, and
    /// `shell_getc` *replaces* that buffer — it does not extend it — after
    /// deleting a `\<newline>` (parse.y's `goto restart_read`). So a reader
    /// dragged onto a fetched line sits at index 0 of a fresh `t`, where
    /// bash's back-scan loops (all guarded by `i > 0`) cannot run at all and
    /// the `token_end == 0` branch returns the single character `t[0]`.
    ///
    /// With a non-blank fetched line that is also what a walk over the whole
    /// input would return — `echo 2` gives `e` either way — so only a blank
    /// one tells the two apart: there the answer is the fetched line's own
    /// `\n` or space, never the token back on the line before. Every row is
    /// bash 5.2.37.
    #[test]
    fn the_near_scan_cannot_reach_back_past_the_readers_own_line() {
        fn diag(src: &str) -> String {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            while let Some(unit) = ip.next_unit(None, opts) {
                let Err(e) = unit else { continue };
                let line = e.line.unwrap_or(0);
                return e
                    .msgs
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let m: String = m.iter().map(|&b| char::from(b)).collect();
                        format!("{}: {m}", e.line_of(i, line))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            panic!("{src:?} must fail")
        }
        for (src, want) in [
            // The fetched line is empty but for its newline, so `t[0]` is that
            // newline — and the echo under it is the empty line.
            (
                "echo 1\n[[ a == b )\\\n\n",
                "2: syntax error in conditional expression: unexpected token `)'\n3: syntax error near `\n'",
            ),
            ("echo 1\n[[ a b\\\n\n", "3: conditional binary operator expected\n3: syntax error near `\n'"),
            (
                "echo 1\n[[ ( a ;\\\n\n",
                "3: unexpected token `;', conditional binary operator expected\n2: expected `)'\n3: syntax error near `\n'",
            ),
            // Whitespace is text, so the fetch found a line and `t[0]` is its
            // first blank — not the `)` a whole-input walk would reach back to.
            (
                "echo 1\n[[ a == b )\\\n   \n",
                "2: syntax error in conditional expression: unexpected token `)'\n3: syntax error near ` '",
            ),
            // The agree-by-accident rows: one line or many, the answer is the
            // same, and they are here to keep the floor from moving.
            (
                "echo 1\n[[ a == b )\\\necho 2\n",
                "2: syntax error in conditional expression: unexpected token `)'\n3: syntax error near `e'",
            ),
            (
                "echo 1\n[[ a == b )\n",
                "2: syntax error in conditional expression: unexpected token `)'\n2: syntax error near `)'",
            ),
            // A read that stopped mid-line never fetched anything, so its line
            // is still the one it started on, backslash and all.
            (
                "echo 1\n[[ a -eq b -eq c\\\n\n",
                "2: syntax error in conditional expression\n2: syntax error near `-eq'",
            ),
        ] {
            assert_eq!(diag(src), want, "src {src:?}");
        }
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

    /// The subscript closes where `skipsubscript` says, so a bracket nested
    /// inside it does not end the word early and leave a command behind. bash
    /// measures it with `skipsubscript` (general.c:469), and it is only the
    /// *spanning* form that ever had to count across segments.
    #[test]
    fn a_nested_bracket_in_a_spanning_subscript_is_still_an_assignment() {
        let assigns_to = |src: &str| {
            let prog = parse(src).unwrap();
            let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
                panic!("{src}");
            };
            (sc.assignments.len(), sc.words.len())
        };
        // The nested `[` raises the depth the closing `]` has to come back
        // through; taking the first `]` instead leaves `]=R` and a command.
        assert_eq!(assigns_to("c[b[$i]]=R"), (1, 0));
        assert_eq!(assigns_to("c[b[$i]]+=R"), (1, 0));
        assert_eq!(assigns_to("c[b[b[$i]]]=R"), (1, 0));
        assert_eq!(assigns_to("c[b[$i]x]=R"), (1, 0));
        // A quoted run is one segment and carries no brackets at all.
        assert_eq!(assigns_to(r#"c["b[1]"$i]=R"#), (1, 0));
        assert_eq!(assigns_to(r#"c[b[$i]"]"]=R"#), (1, 0));
        // Still not an assignment when the `]` is not what the `=` follows.
        assert_eq!(assigns_to("c[b[$i]]x=R"), (0, 1));
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
    /// time: as an input of its own, numbered from one below the line the shell
    /// stands on, one unit at a time.
    fn backtick_unit(src: &str, base_line: u32) -> Result<Program, ParseError> {
        let opts = ParseOpts::default();
        IncrementalParser::new(src.as_bytes(), LineMap::Offset(base_line.saturating_sub(1)), opts)
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
    /// *eager* one — this parse, in the enclosing token stream — numbers them
    /// against the source: an error it raises names the body's true physical
    /// line. (The expansion-time read numbers them against the re-print instead;
    /// see `parse_cmdsub_body`.) Getting this wrong is invisible in a one-line
    /// script and off by the body's length in a real one.
    #[test]
    fn an_eager_cmdsub_body_error_names_its_physical_line() {
        // `for` is on line 4 of the enclosing source; the body's own numbering
        // would call it line 3.
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

    /// A body whose `)` never arrives is parsed all the same, and its own error
    /// is what gets reported — the missing paren never being mentioned. bash
    /// reaches that by parsing the body *as it reads it* (`parse_comsub` runs a
    /// whole nested `yyparse` with `shell_eof_token = ')'`), so the `)` is only
    /// noticed on the `EOF_Reached` path, where that parse ran out rather than
    /// objected. See `resolve_subst_bail`. Every row is measured from bash 5.2.
    #[test]
    fn an_unterminated_substitutions_body_is_parsed_anyway() {
        let near = |t: &str| format!("syntax error near unexpected token `{t}'");
        for (src, tok, line) in [
            ("echo $(fi", "fi", 1),
            ("echo $(;", ";", 1),
            ("echo $(done", "done", 1),
            ("echo $(echo a; fi", "fi", 1),
            ("x=$(fi", "fi", 1),
            // Numbered physically, from the line the `(` sits on — not from the
            // body's own first line.
            ("echo $(\nfi", "fi", 2),
            ("echo one\necho $(fi", "fi", 2),
            ("echo $(echo a\necho b\nfi", "fi", 3),
            // Process substitution is read the same way.
            ("echo <(fi", "fi", 1),
            ("echo >(fi", "fi", 1),
            // The construct that *contained* the substitution never gets to miss
            // its own delimiter: the nested parse dies first.
            ("echo ${x:-$(fi", "fi", 1),
            ("echo $(( $(fi", "fi", 1),
            ("echo \"$(fi", "fi", 1),
            // Left to right, outermost first: the enclosing body is parsed
            // before the substitution it ran out inside gets its turn.
            ("echo $(echo x; $(fi", "fi", 1),
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!(emsg(&e), near(tok), "{src:?}");
            assert_eq!(e.line, Some(line), "{src:?}");
            // Found inside a body, so fatal to whoever was reading it.
            assert!(e.fatal, "{src:?}");
        }
        // A body that merely *ran out* is bash's `EOF_Reached` path, and there
        // the missing `)` is what stands: there is no token to blame, and one
        // would have to be invented to blame it.
        for src in ["echo $(echo a", "echo $(", "echo $(a |", "echo $(!", "echo $(if true"] {
            let e = parse(src).unwrap_err();
            assert_eq!(emsg(&e), "unexpected EOF while looking for matching `)'", "{src:?}");
        }
        // An unterminated quote *inside* the body is that quote's error, since
        // the nested parse's own lexer is what dies on it.
        for (src, msg) in [
            ("echo $(echo \"x", "unexpected EOF while looking for matching `\"'"),
            ("echo $(echo 'x", "unexpected EOF while looking for matching `''"),
            ("echo $(echo `fi", "unexpected EOF while looking for matching ``'"),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), msg, "{src:?}");
        }
    }

    /// The same order for a body that *closed*: it was parsed where it was met,
    /// so its error is reported and whatever the word ran out inside afterwards
    /// never gets to miss its own delimiter. See `LexError::eager_bodies`.
    /// Every row is measured from bash 5.2.37.
    #[test]
    fn a_body_read_inside_a_word_that_ran_out_is_parsed_anyway() {
        let near = |t: &str| format!("syntax error near unexpected token `{t}'");
        for (src, tok) in [
            ("echo \" $(fi)", "fi"),
            // A `${ … }` and a `$(( … ))` step over their nested `$( … )` with
            // the same eager parse, so those bodies count too.
            ("echo \" ${x:-$(fi)}", "fi"),
            ("echo \" $(( $(fi) ))", "fi"),
            // Left to right: the first body that will not parse wins, whether
            // what follows is another whole body or the one the scan ran out in.
            ("echo \" $(fi) $(done)", "fi"),
            ("echo \" $(fi) $(done", "fi"),
            ("echo \" $(fi) `done`", "fi"),
            // The quote is not what makes it eager — the word is. Whatever the
            // word finally ran out inside, the bodies before it still win.
            ("echo $(fi)x$(", "fi"),
            ("echo $(fi)x${y", "fi"),
            ("echo $(fi)x$((1+", "fi"),
            ("echo $(fi)x\"y", "fi"),
            ("echo $(fi)x'y", "fi"),
            ("echo $(fi)x`y", "fi"),
            // A `" … "` that closed nests as a segment and is descended into.
            ("echo \"a$(fi)\"x$(", "fi"),
            // A process substitution is parsed as eagerly, and is numbered from
            // its *opening* delimiter rather than its `)`.
            ("echo <(fi)x$(", "fi"),
            ("echo >(fi)x$(", "fi"),
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!(emsg(&e), near(tok), "{src:?}");
            // Found inside a body, so fatal to whoever was reading it.
            assert!(e.fatal, "{src:?}");
        }
        // A backquote body is read as text to its mate and parsed only at
        // expansion time, so nothing objects before the word runs out — and a
        // body that parses, or that merely ran out itself, says nothing either.
        for (src, msg) in [
            ("echo \" `fi`", "unexpected EOF while looking for matching `\"'"),
            ("echo \"abc", "unexpected EOF while looking for matching `\"'"),
            ("echo \" $(echo ok)", "unexpected EOF while looking for matching `\"'"),
            ("echo \" $(a |", "unexpected EOF while looking for matching `)'"),
            ("echo \" $(echo ok) $(if", "unexpected EOF while looking for matching `)'"),
            ("echo $(echo ok)x$(", "unexpected EOF while looking for matching `)'"),
            ("echo `fi`x$(", "unexpected EOF while looking for matching `)'"),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), msg, "{src:?}");
        }
        // The body is numbered physically — which is what the two different
        // reference lines are for, the `)` for a `$( … )` and the `<(` for a
        // process substitution. Both land on the line `fi` is written on.
        assert_eq!(parse("echo one\necho $(\nfi)x$(").unwrap_err().line, Some(3));
        assert_eq!(parse("echo one\necho <(\nfi)x$(").unwrap_err().line, Some(3));
        // A body in an *earlier* word is not this mechanism's — that word
        // finished, so it is a token the parser reaches on its own. It needs
        // the shell's deferred tokenizer, which keeps a failing line's tokens,
        // and so is measured in the corpus rather than through this `parse`:
        // `echo $(fi)x <(` and `echo a$(fi) b$(` both name `fi` there.
    }

    /// `<(`, `>(` and `$(` are one row of `parse_matched_pair` (parse.y:5028)
    /// and all three go through `parse_comsub`, so a `${ … }` body's scan reads
    /// a process substitution exactly as it reads a command substitution: the
    /// body is parsed there and then, and its error is the enclosing unit's.
    /// Every row is measured from bash 5.2.37.
    #[test]
    fn a_process_substitution_in_a_brace_body_is_read_where_it_is_met() {
        let near = |t: &str| format!("syntax error near unexpected token `{t}'");
        for src in [
            "echo \"${z:-<(fi)}\"",
            // The quotes are not what make it eager, and neither is the `:-`.
            "echo ${z:-<(fi)}",
            "echo \"${z#<(fi)}\"",
            "echo \"${z/x/<(fi)}\"",
            "echo \"${a[<(fi)]}\"",
            // `>(` is the same row.
            "echo \"${z:-a>(fi)b}\"",
            // Beside a `$( … )` the same scan already read.
            "echo \"${z:-$(echo x)<(fi)}\"",
            // A body kept as *text* — the `${#…}` shape has no operand word —
            // is parsed all the same.
            "echo \"${#x:-<(fi)}\"",
            // A nested body's parses are the outer body's too.
            "echo \"${x:-${y:-<(fi)}}\"",
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!(emsg(&e), near("fi"), "{src:?}");
            // Found inside a body, so fatal to whoever was reading it.
            assert!(e.fatal, "{src:?}");
        }
        // A `' … '` run at the top of a brace body is stepped over whole, and a
        // `" … "` inside one is read by a scan that does not have this row at
        // all — `echo "<(fi)"` is the word `<(fi)`. Neither is a parse error.
        for src in ["echo \"${z:-'<(fi)'}\"", "echo \"<(fi)\"", "echo '<(fi)'"] {
            assert!(parse(src).is_ok(), "{src:?}");
        }
        // A body that ran out says nothing, so the enclosing scan's own missing
        // delimiter stands — and which one that is still comes from where the
        // `${` was written, exactly as for a `$(` in the same place.
        for (src, msg) in [
            ("echo \"${z:-<(fi}\"", "unexpected EOF while looking for matching `\"'"),
            ("echo ${z:-<(echo hi}", "unexpected EOF while looking for matching `)'"),
        ] {
            assert_eq!(emsg(&parse(src).unwrap_err()), msg, "{src:?}");
        }
        // Numbered from the `<(`, and so landing on the physical line `fi` is
        // written on.
        assert_eq!(parse("echo one\necho \"${z:-<(\nfi)}\"").unwrap_err().line, Some(3));
    }

    /// Having parsed it, whether bash *performs* it is one test:
    /// `expand_word_internal` reads a process substitution only when the
    /// expansion is not under `Q_DOUBLE_QUOTES` (subst.c:11079). So the
    /// fragments a `${ … }` body is cut into split two ways — a pattern, a
    /// replacement and a bare operand run one, while a double-quoted operand
    /// and the two arithmetic fragments keep the characters — and the split is
    /// taken at lex time, which is where osh knows the quoting. Every row is
    /// measured from bash 5.2.37; see the corpus case
    /// `a-process-substitution-in-a-brace-body-is-performed-unless-the-expansion-is-quoted.sh`.
    #[test]
    fn whether_a_brace_bodys_process_substitution_is_performed_is_the_quoting() {
        let opts = ParseOpts::default();
        let live = |w: &Word| w.parts.iter().any(|p| matches!(p, WordPart::ProcSub { .. }));
        let show = |f: &[u8]| String::from_utf8_lossy(f).into_owned();
        // A pattern and a bare operand are read by the same scan, and it
        // performs.
        for frag in [b"<(echo hi)".as_slice(), b"a<(echo hi)b", b"<(echo a)<(echo b)", b">(cat)"] {
            let w = verbatim_word_at(frag, opts, Quoting::Bare, 1, Frag::Word).unwrap();
            assert!(live(&w), "{}", show(frag));
        }
        // …and the replacement's own reader agrees, `\&` handling aside.
        assert!(live(&word_replacement_from_source(b"<(echo hi)", opts, Quoting::Bare, 1).unwrap()));
        // The arithmetic fragments do not: `Q_DOUBLE_QUOTES|Q_ARITH` is exactly
        // what stops the read, so the evaluator meets the characters and its
        // error names them.
        assert!(!live(&verbatim_word_at(b"<(echo 1)", opts, Quoting::Bare, 1, Frag::Arith).unwrap()));
        assert!(!live(&word_subscript_from_source(b"<(echo 1)", opts, Quoting::Bare).unwrap()));
        // A substring bound is the other arithmetic fragment, and now reaches
        // the very same reader — it used to be tokenized, which is what made it
        // the one context that disagreed with the subscript beside it.
        let bound: Vec<Ch> = bytes::chars(b"<(echo 1)").collect();
        assert!(!live(
            &parse_slice_bounds(&bound, opts, Quoting::Bare, 1).unwrap().unwrap().offset
        ));
        // A double-quoted operand keeps the characters. So does a quoted run
        // inside a *bare* one — the quotes are what the test is about, not
        // which fragment it is.
        assert!(!live(&operand_from_source(b"<(echo hi)", opts, Quoting::Dquote, 1, &[]).unwrap()));
        for frag in [b"\"<(echo hi)\"".as_slice(), b"'<(echo hi)'", b"\\<\\(echo hi\\)"] {
            let w = verbatim_word_at(frag, opts, Quoting::Bare, 1, Frag::Word).unwrap();
            assert!(!live(&w), "{}", show(frag));
        }
    }

    /// A line the reader cannot finish lexing still offers the parser every
    /// token it *did* yield, because bash's parser pulls them one at a time: a
    /// grammar error among them is raised before the fetch that would have run
    /// into the unclosed construct, and wins. `) echo "` is reported on the
    /// stray `)`, the quote behind it never being reached.
    ///
    /// The complement is what running dry means. A fetch that finds nothing is
    /// the fetch that would have found the error, so the parked error replaces
    /// whatever the unit came to — including a clean parse of the commands in
    /// front of it, since nothing on the failing line runs. A newline is not
    /// that fetch: a unit it ended is complete however close the failure sits
    /// behind it. Every row is measured from bash 5.2.37.
    #[test]
    fn a_line_that_fails_to_lex_still_offers_the_tokens_it_had() {
        /// How many units `src` hands out before the error that ends it.
        fn units(src: &str) -> (usize, ParseError) {
            let opts = ParseOpts::default();
            let mut ip = IncrementalParser::new(src.as_bytes(), 0, opts);
            let mut ran = 0;
            while let Some(u) = ip.next_unit(None, opts) {
                match u {
                    Ok(_) => ran += 1,
                    Err(e) => return (ran, e),
                }
            }
            panic!("{src:?} must fail")
        }
        let near = |t: &str| format!("syntax error near unexpected token `{t}'");
        // A token the line already yielded outranks the construct behind it.
        for (src, tok) in [
            (") echo \"", ")"),
            ("echo a; ) echo \"", ")"),
            ("fi \"", "fi"),
            ("done \"unterm", "done"),
            ("esac \"unterm", "esac"),
            ("then \"unterm", "then"),
            ("} \"unterm", "}"),
            // The construct need not be a quote, nor the last thing on the line.
            (") $(", ")"),
            (") cat <<E", ")"),
        ] {
            let (ran, e) = units(src);
            assert_eq!(emsg(&e), near(tok), "{src:?}");
            assert_eq!(e.line, Some(1), "{src:?}");
            assert_eq!(ran, 0, "{src:?}");
        }
        // Nothing objects, so the construct is reported — and the whole line
        // goes with it, commands parsed before it included.
        for (src, ran_want, line) in [
            ("echo \"; )", 0, 1),
            ("echo one; echo \"unterm", 0, 1),
            ("echo one && echo \"unterm", 0, 1),
            ("echo one | echo \"unterm", 0, 1),
            ("for i in 1; do echo \"unterm", 0, 1),
            ("echo one; { echo \"unterm", 0, 1),
            // Earlier lines were handed over whole and have already run.
            ("echo one\necho two; echo \"unterm", 1, 2),
            // Including a backgrounded one, whose output then races the
            // diagnostic — which is why the corpus case leaves this shape here.
            ("echo one &\necho \"unterm", 1, 2),
            ("echo one\necho two\necho three; echo four \"unterm", 2, 3),
            // A truncated compound leaves a grammar error of its own, which the
            // parked one replaces: bash never saw the truncation, only the quote.
            ("if true; then\necho 'unterm", 0, 2),
        ] {
            let (ran, e) = units(src);
            assert!(e.is_incomplete(), "{src:?}: {}", emsg(&e));
            assert_eq!(e.line, Some(line), "{src:?}");
            assert_eq!(ran, ran_want, "{src:?}");
        }
        // The newline that ends a unit ends it however close the failure is: the
        // parser never fetched past it, so both lines run.
        let (ran, e) = units("echo one\necho two\nv='abc\n");
        assert_eq!(emsg(&e), "unexpected EOF while looking for matching `''");
        assert_eq!((ran, e.line), (2, Some(3)));
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
        let Command::Cond(CondClause { expr: CondExpr::Binary(_, op, _), .. }) =
            &prog.items[0].list.first.commands[0]
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
            let Command::Cond(CondClause { expr: CondExpr::Binary(_, op, _), .. }) =
                &prog.items[0].list.first.commands[0]
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
            let Command::Cond(CondClause { expr: CondExpr::Unary(op, _), .. }) =
                &prog.items[0].list.first.commands[0]
            else {
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
        let Command::Cond(CondClause { expr: CondExpr::Or(_, right), .. }) =
            &prog.items[0].list.first.commands[0]
        else {
            panic!("expected top-level Or");
        };
        assert!(matches!(**right, CondExpr::And(_, _)));
    }

    #[test]
    fn cond_regex_parses() {
        let prog = parse("[[ $x =~ foo ]]").unwrap();
        assert!(matches!(
            prog.items[0].list.first.commands[0],
            Command::Cond(CondClause { expr: CondExpr::Regex(_, _), .. })
        ));
    }

    /// The `=~` RHS is one word, but an unquoted `( … )` group inside it holds
    /// on to blanks and shell operators, so the regex can span them. Outside a
    /// group the ordinary word boundaries are back.
    #[test]
    fn cond_regex_group_spans_blanks_and_operators() {
        let regex_of = |src: &str| -> String {
            let prog = parse(src).unwrap_or_else(|e| panic!("{src}: {}", emsg(&e)));
            let Command::Cond(CondClause { expr: CondExpr::Regex(_, rhs), .. }) =
                &prog.items[0].list.first.commands[0]
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
        let Command::Arith(a) = &prog.items[0].list.first.commands[0] else {
            panic!("expected arith command");
        };
        assert_eq!(text(bytes::trim(&a.expr)), "x + 1");
    }

    /// The three sections of a `for (( … ))` header are not `;`-separated
    /// fields: they are whatever bash's
    /// `skip_to_delim (start, 0, ";", SD_NOJMP|SD_NOPROCSUB)` walks past. A `;`
    /// inside a construct that scan steps over belongs to the construct, and one
    /// inside a construct it does *not* step over ends a section however
    /// arithmetic the text around it looks.
    #[test]
    fn an_arith_for_header_is_carved_by_the_scan_and_not_by_semicolons() {
        fn secs(src: &str) -> Vec<String> {
            let chs = bytes::chars(src.as_bytes()).collect::<Vec<_>>();
            arith_for_sections(&chs).iter().map(|s| text(s)).collect()
        }

        assert_eq!(secs("i=0; i<2; i++"), ["i=0", "i<2", "i++"]);

        // Stepped over, so the `;` separates nothing.
        for held in [
            "${x:-;}",
            "\"1;2\"",
            "' 1;2'",
            "`echo 1;2`",
            "$(echo 1;2)",
            "$'a;b'",
            "$\"a;b\"",
            "\\;",
            "$(( 1;2 ))",
        ] {
            assert_eq!(secs(&format!("{held}; 0; 0")), [held, "0", "0"], "{held}");
        }

        // Not stepped over — a subscript wants `SD_GLOB`, a process
        // substitution is suppressed by `SD_NOPROCSUB`, a bare group and a
        // `? :` want `SD_ARITHEXP`, and `$[ … ]` is unknown to the scan.
        for (src, want) in [
            ("a[1;2]", ["a[1", "2]"]),
            ("$[1;2]", ["$[1", "2]"]),
            ("(1;2)", ["(1", "2)"]),
            ("0?1;2:3", ["0?1", "2:3"]),
            ("<(echo 1;2)", ["<(echo 1", "2)"]),
        ] {
            assert_eq!(secs(src), want, "{src}");
        }

        // Each section starts past the space and tab `whitespace()` skips, and
        // past nothing else: trailing blanks stay, and a leading newline is not
        // blank enough to go.
        assert_eq!(secs("  0 ;\t0  ; 0"), ["0 ", "0  ", "0"]);
        assert_eq!(secs("\n0;0;0"), ["\n0", "0", "0"]);

        // The scan always yields at least one section, so an empty header is
        // one empty section rather than none — which is what makes `for (( ))`
        // too *few* sections and not zero of them.
        assert_eq!(secs(""), [""]);
        assert_eq!(secs(";;"), ["", "", ""]);
        assert_eq!(secs("0;0;0;"), ["0", "0", "0", ""]);
    }

    /// A header that does not carve into exactly three sections is two
    /// `parser_error` messages, not one: the count decides the first, and the
    /// second quotes the header back between the `((` and `))` the writer never
    /// typed twice. Both name the line the `((` was read on, however far below
    /// the `done` the parser has reached by the time it counts.
    #[test]
    fn a_miscounted_arith_for_header_is_two_messages_at_the_double_parens_line() {
        let diag = |src: &str| {
            let e = parse(src).expect_err("miscounted header");
            let at = |i: usize| {
                let i = u32::try_from(i).unwrap_or(u32::MAX);
                e.line_at
                    .iter()
                    .find(|&&(j, _)| j == i)
                    .map_or(e.line.unwrap_or(0), |&(_, l)| l)
            };
            e.msgs
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{}: {}", at(i), text(m)))
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert_eq!(
            diag("for ((0;0)); do :; done"),
            "1: syntax error: arithmetic expression required\n1: syntax error: `((0;0))'"
        );
        assert_eq!(
            diag("for ((0;0;0;0)); do :; done"),
            "1: syntax error: `;' unexpected\n1: syntax error: `((0;0;0;0))'"
        );
        // The header is quoted back exactly as written — its newlines included,
        // and the `((`'s own line is the one blamed, not the `done`'s.
        assert_eq!(
            diag("echo hi\nfor ((0;\n0)); do :; done"),
            "2: syntax error: arithmetic expression required\n2: syntax error: `((0;\n0))'"
        );
        // A `;` the scan steps over is not a separator, so this one counts.
        assert!(parse("for (( ${x:-;}; 0; 0 )); do :; done").is_ok());
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
