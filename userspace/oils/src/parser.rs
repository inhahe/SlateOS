//! Recursive-descent parser: tokens → [`ast::Program`].
//!
//! The parser also lowers lexer [`Seg`]s into [`ast::WordPart`]s, recursively
//! parsing command/parameter substitutions (their raw inner source is captured
//! by the lexer).

use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseClause, CaseItem,
    CaseTerm, CmdSubBody,
    Command,
    CondBinOp,
    CondExpr, ForArithClause, ForClause, FunctionDef, HereDoc, IfClause, Item, LineMap,
    LoopClause,
    ParamOp,
    Pipeline, Program,
    Redirect, RedirectOp, ReplaceAnchor, SelectClause, SimpleCommand, UnaryOp, Word, WordPart,
};
use crate::lexer::{
    LexOpts, Op, Seg, Tok, Tokenized, expand_aliases, expand_aliases_tracked, tokenize,
    tokenize_paren_body, tokenize_deferred, tokenize_spanned, word_is_assignment,
};
use std::collections::BTreeMap;

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
    pub msg: String,
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
}

impl ParseError {
    /// A parse error with no known line (the common construction site inside
    /// the grammar; [`parse_tokens`] stamps the line afterwards).
    pub fn new(msg: String) -> Self {
        Self { msg, line: None, fatal: false }
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
        self.msg.contains("unexpected end of file")
            || self.msg.contains("unexpected EOF while looking for")
    }
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.msg)
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
        }
    }
}

/// Parse shell source into a [`Program`].
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    parse_opts(src, LexOpts::default())
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
pub fn parse_opts(src: &str, opts: LexOpts) -> Result<Program, ParseError> {
    let (toks, lines) = tokenize_spanned(src, opts).map_err(ParseError::from)?;
    parse_tokens(toks, lines, opts)
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
pub fn parse_procsub_body(src: &str, open_line: u32, opts: LexOpts) -> Result<Program, ParseError> {
    let (mut toks, mut lines) = tokenize_paren_body(src, opts)
        .map_err(|e| ParseError::from(e).in_paren_body())?;
    map_lines(&mut toks, &mut lines, &LineMap::Offset(open_line.saturating_sub(1)));
    parse_tokens_ending(toks, lines, opts, true).map_err(ParseError::in_paren_body)
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
pub fn parse_strict_heredoc(src: &str, opts: LexOpts) -> Result<Program, ParseError> {
    let (toks, lines) =
        crate::lexer::tokenize_spanned_strict(src, opts).map_err(ParseError::from)?;
    parse_tokens(toks, lines, opts)
}

/// Parse shell source, expanding shell aliases over the token stream first.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse_with_aliases(
    src: &str,
    aliases: &BTreeMap<String, String>,
    opts: LexOpts,
) -> Result<Program, ParseError> {
    let (toks, lines) = tokenize_spanned(src, opts).map_err(ParseError::from)?;
    let (toks, lines) = if aliases.is_empty() {
        (toks, lines)
    } else {
        expand_aliases(&toks, &lines, aliases, opts)
    };
    parse_tokens(toks, lines, opts)
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
    pub text: String,
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
    pub text: String,
    /// The line's number after the parser's line map, for a diagnostic.
    pub line: u32,
}

pub struct IncrementalParser {
    /// The source, kept for re-lexing the tail when [`LexOpts`] change. Held as
    /// chars because the offsets recorded by the lexer are char indices.
    src: Vec<char>,
    /// Applied to every line the lexer reports, so a fragment lexed on its own
    /// still names the lines of the input it came from.
    line_map: LineMap,
    /// The options `orig` was lexed under.
    opts: LexOpts,
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
    unit_raw: String,
    /// `orig[pos..]` alias-expanded under `last_aliases`, prefixed by any
    /// alias-spliced tokens carried across the last rebuild.
    work: Vec<Tok>,
    work_lines: Vec<u32>,
    /// Parallel to `work`: the `orig` index each token came from, or `None` for
    /// a token an alias spliced in (which has no original counterpart).
    work_origin: Vec<Option<usize>>,
    /// Cursor into `work`.
    wpos: usize,
    /// Index into `orig` of the first token not yet consumed: the origin of the
    /// first `Some`-origin token at or after `wpos`.
    pos: usize,
    /// The alias state `work` was built under, or `None` if never built. The
    /// inner `Option` is the caller's argument (`None` = expansion disabled), so
    /// a `shopt -u expand_aliases` also invalidates.
    last_aliases: Option<Option<BTreeMap<String, String>>>,
    /// An unterminated quote/substitution that ended the input. Held back until
    /// the complete lines before it have been handed out and executed, because
    /// bash reports it only after running them.
    pending_lex_err: Option<ParseError>,
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
    pub fn new(src: &str, line_map: impl Into<LineMap>, opts: LexOpts) -> Self {
        let line_map = line_map.into();
        let Tokenized {
            toks: mut orig,
            lines: mut orig_lines,
            offsets: orig_offsets,
            ends: orig_ends,
            conts: orig_conts,
            err,
        } = tokenize_deferred(src, opts);
        map_lines(&mut orig, &mut orig_lines, &line_map);
        Self {
            src: src.chars().collect(),
            opts,
            orig,
            orig_lines,
            orig_offsets,
            orig_ends,
            orig_conts,
            hist_cursor: 0,
            expand_cursor: 0,
            unit_lines: Vec::new(),
            unit_raw: String::new(),
            work: Vec::new(),
            work_lines: Vec::new(),
            work_origin: Vec::new(),
            wpos: 0,
            pos: 0,
            last_aliases: None,
            pending_lex_err: err
                .map(|(e, line)| ParseError::from(e).or_line(line))
                .map(|e| ParseError {
                    line: e.line.map(|l| line_map.map(l)),
                    ..e
                }),
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
    fn relex(&mut self, opts: LexOpts) {
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
    fn relex_from(&mut self, off: usize, opts: LexOpts) {
        let off = off.min(self.src.len());
        let head = self.src.get(..off).unwrap_or(&[]);
        let newlines =
            u32::try_from(head.iter().filter(|&&c| c == '\n').count()).unwrap_or(u32::MAX);
        let map = self.line_map.shifted(newlines);
        let tail: String = self.src.get(off..).unwrap_or(&[]).iter().collect();
        let Tokenized {
            toks: mut orig,
            lines: mut orig_lines,
            mut offsets,
            mut ends,
            conts,
            err,
        } = tokenize_deferred(&tail, opts);
        map_lines(&mut orig, &mut orig_lines, &map);
        let delta = u32::try_from(off).unwrap_or(u32::MAX);
        for o in offsets.iter_mut().chain(ends.iter_mut()) {
            *o = o.saturating_add(delta);
        }
        self.orig = orig;
        self.orig_lines = orig_lines;
        self.orig_offsets = offsets;
        self.orig_ends = ends;
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
    /// `aliases`.
    fn rebuild(&mut self, aliases: Option<&BTreeMap<String, String>>) {
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

        let rest = self.orig.get(self.pos..).unwrap_or(&[]);
        let rest_lines = self.orig_lines.get(self.pos..).unwrap_or(&[]);
        match aliases {
            Some(map) if !map.is_empty() => {
                let (t, l, o) = expand_aliases_tracked(rest, rest_lines, map, self.opts);
                work.extend(t);
                work_lines.extend(l);
                work_origin.extend(o.into_iter().map(|i| i.map(|i| i.saturating_add(self.pos))));
            }
            _ => {
                work.extend_from_slice(rest);
                work_lines.extend_from_slice(rest_lines);
                work_origin.extend((self.pos..self.orig.len()).map(Some));
            }
        }
        self.work = work;
        self.work_lines = work_lines;
        self.work_origin = work_origin;
        self.wpos = 0;
        self.last_aliases = Some(aliases.cloned());
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
            text: self.src.get(start..end).unwrap_or(&[]).iter().collect(),
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
    pub fn commit_raw_line(&mut self, expanded: Option<&str>, opts: LexOpts) {
        let start = self.expand_frontier();
        if start >= self.src.len() {
            return;
        }
        let end = self.line_end(start);
        let mut new_end = end;
        if let Some(text) = expanded {
            let repl: Vec<char> = text.chars().collect();
            new_end = start.saturating_add(repl.len());
            self.src.splice(start..end, repl);
            // From `hist_cursor`, not from the next token: the rewritten text can
            // precede that token (see [`Self::relex_from`]).
            self.relex_from(self.hist_cursor, opts);
        }
        // Step past the newline ending the line, so the next peek starts on the
        // following one. A replacement containing newlines counts as expanded in
        // full — bash expands a line once, never its own output.
        self.expand_cursor = if self.src.get(new_end) == Some(&'\n') {
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
    pub fn drop_raw_line(&mut self, opts: LexOpts) {
        let start = self.expand_frontier();
        if start >= self.src.len() {
            return;
        }
        let end = self.line_end(start);
        let cut = if self.src.get(end) == Some(&'\n') { end.saturating_add(1) } else { end };
        self.src.drain(start..cut);
        self.relex_from(self.hist_cursor, opts);
        self.expand_cursor = start;
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
        aliases: Option<&BTreeMap<String, String>>,
        opts: LexOpts,
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
            pos: self.wpos,
            opts: self.opts,
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
                        // line, so the unit continues.
                        Some(Tok::Op(Op::Semi | Op::Amp)) => {}
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
            let line = p.cur_line().saturating_add(u32::from(e.is_incomplete()));
            e.or_line(line)
        });
        self.wpos = p.pos;
        self.work = p.toks;
        self.work_lines = p.lines;
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
            Err(e) => {
                // bash has already *read* the line it could not parse, so it is
                // in the history before the diagnostic is printed.
                self.split_unit_lines(next_orig);
                // Abandon the rest of the input, discarding the units parsed so
                // far in *this* unit — bash never runs a partially-parsed line.
                self.wpos = self.work.len();
                self.pos = self.orig.len();
                Some(Err(e))
            }
        }
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
    pub fn last_unit_raw(&self) -> &str {
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
        self.unit_raw = self.src.get(start..end).unwrap_or(&[]).iter().collect();
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
            } else if text.trim().is_empty() {
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
        let comment = self.line_text(code_end, to).contains(|c: char| !c.is_whitespace());
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
    fn line_text(&self, from: usize, to: usize) -> String {
        let mut text = String::new();
        let mut i = from;
        while i < to {
            let at = u32::try_from(i).unwrap_or(u32::MAX);
            if self.orig_conts.binary_search(&at).is_ok() {
                // Skip the backslash and the newline it hid, CR included.
                i = i.saturating_add(1);
                if self.src.get(i) == Some(&'\r') {
                    i = i.saturating_add(1);
                }
                if self.src.get(i) == Some(&'\n') {
                    i = i.saturating_add(1);
                }
                continue;
            }
            if let Some(&c) = self.src.get(i) {
                text.push(c);
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
                [Seg::Lit(s)] if matches!(s.as_str(), "{" | "do" | "then" | "else" | "elif" | "in")
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
    src: &str,
    close_line: u32,
    opts: LexOpts,
) -> Result<(Program, LineMap), ParseError> {
    // The body is lexed with the substitution's own `)` standing where the
    // implicit trailing newline would otherwise go, because that is the token
    // bash's parser sees after the body's last command. See
    // [`tokenize_paren_body`].
    let (mut toks, mut lines) = tokenize_paren_body(src, opts)
        .map_err(|e| ParseError::from(e).in_paren_body())?;
    let map = build_cmdsub_line_map(&toks, &lines, close_line);
    // The body's line 1 is the line `$(` sits on: the closing `)` is on the
    // body's last line, so stepping back over the body's newlines lands there.
    let newlines =
        u32::try_from(src.bytes().filter(|&b| b == b'\n').count()).unwrap_or(u32::MAX);
    let phys = LineMap::Offset(close_line.saturating_sub(newlines).saturating_sub(1));
    map_lines(&mut toks, &mut lines, &phys);
    let prog = parse_tokens_ending(toks, lines, opts, true)
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
            if e.msg == "syntax error: unexpected end of file" {
                ParseError::new("syntax error near unexpected token `)'".to_string())
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

fn parse_tokens(toks: Vec<Tok>, lines: Vec<u32>, opts: LexOpts) -> Result<Program, ParseError> {
    parse_tokens_ending(toks, lines, opts, false)
}

/// [`parse_tokens`], but `ends_at_paren` says the stream's final token is the
/// `)` that closes a `$( … )` body ([`tokenize_paren_body`]). That token is the
/// end of the program rather than a leftover, so it is excluded from the
/// "everything consumed?" check — while remaining a real token everywhere else,
/// which is what makes `$( ! )` and `$(for)` name it.
fn parse_tokens_ending(
    toks: Vec<Tok>,
    lines: Vec<u32>,
    opts: LexOpts,
    ends_at_paren: bool,
) -> Result<Program, ParseError> {
    let mut p = Parser {
        toks,
        lines,
        pos: 0,
        opts,
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
    // the token's own line for a grammar error, but for an *unexpected end of
    // file* error it reports one line past the last token — the position where
    // the missing terminator would go. Key that off the message, not the cursor:
    // an error can name a token that is not in this stream at all (a `$( … )`
    // body reports the substitution's closing `)`), and those still belong on
    // the last token's line.
    parsed.map_err(|e| {
        let line = p.cur_line().saturating_add(u32::from(e.is_incomplete()));
        e.or_line(line)
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
    pos: usize,
    /// The options the tokens were lexed under, carried so that a nested body
    /// re-lexed during parsing (`$( … )`, `<( … )`) is read the same way.
    opts: LexOpts,
}

/// Reserved words that terminate a command list or introduce a compound.
const RESERVED: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "in", "{", "}",
    "!", "case", "esac", "select",
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

    /// If the current token is an unquoted single-literal word, return it.
    fn reserved_here(&self) -> Option<String> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
            && RESERVED.contains(&s.as_str())
        {
            return Some(s.clone());
        }
        None
    }

    /// The literal text of a bare word token (single unquoted literal), if any.
    fn bare_word_here(&self) -> Option<String> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
        {
            return Some(s.clone());
        }
        None
    }

    /// A short human-readable name for the current token, for syntax-error
    /// messages (mirrors bash's `near unexpected token '…'`).
    fn token_display(&self) -> String {
        self.token_display_at(self.pos)
    }

    /// [`Parser::token_display`] for an arbitrary position, so a diagnostic can
    /// name a token the parser has already moved past (see
    /// [`Parser::cond_near`]).
    fn token_display_at(&self, pos: usize) -> String {
        match self.toks.get(pos) {
            None => "end of input".to_string(),
            Some(Tok::Newline) => "newline".to_string(),
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
            .to_string(),
            // bash names the offending word by its *source* spelling, quotes and
            // all: `[[ a '<' b ]]` is reported near `'<'`, not near a bare `<` or
            // a placeholder. Rebuild the word from its segments and print it the
            // way it was written. (One spelling this does not reach is `$'…'`,
            // which the lexer has already decoded — see
            // TD-OILS-ANSIC-ERROR-SPELLING in known-issues.md.)
            Some(Tok::Word(segs)) => word_from_segs(segs, self.opts)
                .map_or_else(|_| "word".to_string(), |w| crate::unparse::word_src(&w)),
            // Anything else (a newline, a here-doc body) has no word spelling.
            _ => "word".to_string(),
        }
    }

    /// Build bash's canonical "unexpected" parser diagnostic for the current
    /// position: at end of input it is `syntax error: unexpected end of file`;
    /// otherwise `syntax error near unexpected token \`TOKEN'` — bash quotes the
    /// offending token with a leading backtick and a trailing single quote.
    fn unexpected_here(&self) -> ParseError {
        if self.peek().is_none() {
            ParseError::new("syntax error: unexpected end of file".to_string())
        } else {
            ParseError::new(format!(
                "syntax error near unexpected token `{}'",
                self.token_display()
            ))
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
            && stops.contains(&w.as_str())
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
                    .is_some_and(|w| stops.contains(&w.as_str()));
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
            if self.reserved_here().as_deref() == Some("!") {
                negated = !negated;
                prefixed = true;
                self.pos += 1;
                continue;
            }
            if self.bare_word_here().as_deref() == Some("time") {
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
                if self.bare_word_here().as_deref() == Some("-p") {
                    time_posix = true;
                    self.pos += 1;
                }
                if self.bare_word_here().as_deref() == Some("--") {
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
            let cmd = match w.as_str() {
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
        if self.bare_word_here().as_deref() == Some("[[") {
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
            && matches!(self.toks.get(self.pos + 2), Some(Tok::Op(Op::RParen)))
        {
            // A name written as a bare word is definable and *is* its literal;
            // any other spelling is not, and is kept as written so the run-time
            // error can quote it back exactly as typed.
            let bare = self.bare_word_here();
            let definable = bare.is_some();
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
        if self.bare_word_here().as_deref() == Some("function") {
            return self.parse_function_keyword();
        }
        // `coproc [NAME] command` — bash reserved word, recognised only at
        // command start.
        if self.bare_word_here().as_deref() == Some("coproc") {
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
            name = Some(w);
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
                        s.as_str(),
                        "{" | "[[" | "if" | "while" | "until" | "for" | "select" | "case"
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
        while self.reserved_here().as_deref() == Some("elif") {
            self.pos += 1;
            let c = self.parse_program(&["then"], false)?;
            self.expect_reserved("then")?;
            let b = self.parse_program(&["elif", "else", "fi"], false)?;
            elifs.push((c, b));
        }
        let else_body = if self.reserved_here().as_deref() == Some("else") {
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
        let Some(var) = self.bare_word_here() else {
            // `for` with no loop variable (`for; do …`, `for` at EOF, `for |`):
            // bash names the unexpected token / reports end of input.
            return Err(self.unexpected_here());
        };
        if !is_valid_name(&var) {
            return Err(ParseError::new(format!("`{var}': not a valid identifier")));
        }
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
        if self.reserved_here().as_deref() != Some("in") {
            self.skip_separators();
            return Ok(None);
        }
        self.pos += 1;
        let mut ws = Vec::new();
        while let Some(Tok::Word(segs)) = self.peek() {
            let segs = segs.clone();
            self.pos += 1;
            ws.push(self.word_from_segs(&segs)?);
        }
        self.skip_separators();
        Ok(Some(ws))
    }

    /// Parse `select name [in words]; do body; done`. Structurally identical to
    /// the word-list `for` loop; the runtime difference is the interactive menu.
    fn parse_select(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("select")?;
        let Some(var) = self.bare_word_here() else {
            return Err(self.unexpected_here());
        };
        if !is_valid_name(&var) {
            return Err(ParseError::new(format!("`{var}': not a valid identifier")));
        }
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
    fn parse_for_arith(&mut self, raw: &str) -> Result<Command, ParseError> {
        let parts: Vec<&str> = raw.split(';').collect();
        if parts.len() != 3 {
            return Err(ParseError::new(
                "C-style for loop requires 'for (( init; cond; update ))'".into(),
            ));
        }
        // Only the *leading* whitespace is dropped. bash keeps each section's
        // source text from its first non-blank character onwards, which shows up
        // when a function is printed back by `declare -f`: `for (( i=0; i<2;
        // i++ ))` comes back as `for ((i=0; i<2; i++ ))`, trailing space and
        // all. The arithmetic evaluator ignores the whitespace either way.
        let init = parts[0].trim_start().to_string();
        let cond = parts[1].trim_start().to_string();
        let update = parts[2].trim_start().to_string();
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
        let word = self.word_from_segs(&segs.clone())?;
        self.pos += 1;
        self.skip_newlines();
        self.expect_reserved("in")?;
        self.skip_newlines();
        let mut items = Vec::new();
        while self.reserved_here().as_deref() != Some("esac") {
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
                patterns.push(self.word_from_segs(&segs.clone())?);
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
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none()
                || self.at_op(Op::DSemi)
                || self.at_op(Op::SemiAmp)
                || self.at_op(Op::DSemiAmp)
                || self.reserved_here().as_deref() == Some("esac")
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
        // Consume `[[`.
        self.pos += 1;
        let expr = self.parse_cond_or()?;
        if self.bare_word_here().as_deref() != Some("]]") {
            // A complete expression but no closer: bash emits
            // `unexpected EOF while looking for \`]]'` then `syntax error:
            // unexpected end of file` (no source echo). If a stray token sits
            // where `]]` should be, name it the ordinary way.
            if self.peek().is_none() {
                return Err(ParseError::new(
                    "unexpected EOF while looking for `]]'\nsyntax error: unexpected end of file"
                        .to_string(),
                ));
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
                let near = cond_error_near(&tok);
                return Err(ParseError::new(format!(
                    "syntax error in conditional expression: unexpected token `{tok}'\nsyntax error near `{near}'"
                )));
            }
            return Err(ParseError::new(format!(
                "syntax error in conditional expression\nsyntax error near `{tok}'"
            )));
        }
        self.pos += 1;
        Ok(Command::Cond(expr))
    }

    fn parse_cond_or(&mut self) -> Result<CondExpr, ParseError> {
        let mut left = self.parse_cond_and()?;
        while self.at_op(Op::OrIf) {
            self.pos += 1;
            let right = self.parse_cond_and()?;
            left = CondExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_and(&mut self) -> Result<CondExpr, ParseError> {
        let mut left = self.parse_cond_not()?;
        while self.at_op(Op::AndIf) {
            self.pos += 1;
            let right = self.parse_cond_not()?;
            left = CondExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_not(&mut self) -> Result<CondExpr, ParseError> {
        // A term may start on a later line than the operator that introduced it.
        self.skip_cond_newlines();
        if self.peek().is_none() {
            // Waiting for a term and the input ran out. bash names the token it
            // did not get, rather than reporting a missing `]]` — that message
            // is reserved for an expression that *was* complete.
            return Err(ParseError::new(
                "unexpected token `EOF' in conditional command\nsyntax error: unexpected end of file"
                    .to_string(),
            ));
        }
        if self.bare_word_here().as_deref() == Some("!") {
            self.pos += 1;
            let inner = self.parse_cond_not()?;
            return Ok(CondExpr::Not(Box::new(inner)));
        }
        self.parse_cond_primary()
    }

    fn parse_cond_primary(&mut self) -> Result<CondExpr, ParseError> {
        // Parenthesised sub-expression.
        if self.at_op(Op::LParen) {
            self.pos += 1;
            let inner = self.parse_cond_or()?;
            if !self.at_op(Op::RParen) {
                // A parsed sub-expression but no `)`: bash says `unexpected
                // token \`X', expected \`)'` (+ the `near \`X'` echo). At end of
                // input it falls back to its implicit-newline model, which we
                // don't reproduce.
                if self.peek().is_none() {
                    return Err(ParseError::new("syntax error: unexpected end of file".to_string()));
                }
                let tok = self.token_display();
                return Err(ParseError::new(format!(
                    "unexpected token `{tok}', expected `)'\nsyntax error near `{tok}'"
                )));
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
        if let Some(op) = self.peek_cond_binop() {
            self.advance_cond_binop();
            let right = self.expect_cond_word(CondPos::Binary)?;
            self.skip_cond_newlines();
            if matches!(op, RawBinOp::Regex) {
                return Ok(CondExpr::Regex(Box::new(left), Box::new(right)));
            }
            return Ok(CondExpr::Binary(
                Box::new(left),
                op.into_bin_op(),
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
            && !matches!(segs.as_slice(), [Seg::Lit(s)] if s == "]]")
        {
            let tok = self.token_display();
            return Err(ParseError::new(format!(
                "conditional binary operator expected\nsyntax error near `{tok}'"
            )));
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
            return Err(ParseError::new(format!(
                "unexpected token `{tok}', conditional binary operator expected\nsyntax error near `{near}'"
            )));
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
            let near = cond_error_near(&tok);
            return Err(ParseError::new(format!(
                "unexpected token `{tok}', conditional binary operator expected\nsyntax error near `{near}'"
            )));
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
    fn cond_near(&self) -> String {
        if !matches!(self.peek(), Some(Tok::Newline)) {
            return self.token_display();
        }
        // Walk back over any newlines to the last real token.
        let mut pos = self.pos;
        while pos > 0 && matches!(self.toks.get(pos), None | Some(Tok::Newline)) {
            pos -= 1;
        }
        self.token_display_at(pos)
    }

    /// Expect a word operand inside `[[ … ]]` (not an operator/closer). `pos`
    /// tells us what bash would say when the operand is missing: after a unary
    /// or binary operator bash prepends `unexpected argument \`X' to conditional
    /// {unary,binary} operator`, whereas in primary position it reports only
    /// `syntax error near \`X'`.
    fn expect_cond_word(&mut self, pos: CondPos) -> Result<Word, ParseError> {
        if let Some(Tok::Word(segs)) = self.peek() {
            // `]]` is the closer, never an operand.
            if !matches!(segs.as_slice(), [Seg::Lit(s)] if s == "]]") {
                let segs = segs.clone();
                self.pos += 1;
                return self.word_from_segs(&segs);
            }
        }
        Err(self.cond_operand_error(pos))
    }

    /// Build bash's diagnostic for a missing/`]]`-filled operand slot inside
    /// `[[ … ]]`. When the offending token is present, bash echoes the source
    /// line (handled by `format_parse_error`); at end of input it uses an
    /// implicit-`newline` model we don't reproduce, so we fall back to a plain
    /// end-of-file diagnostic there.
    fn cond_operand_error(&self, pos: CondPos) -> ParseError {
        if self.peek().is_none() {
            return ParseError::new("syntax error: unexpected end of file".to_string());
        }
        let tok = self.token_display();
        // A newline never becomes the token bash reports "near", so an operand
        // slot that a line end walked into names the operator instead.
        let near = format!("syntax error near `{}'", self.cond_near());
        let msg = match pos {
            CondPos::Primary => near,
            CondPos::Unary => {
                format!("unexpected argument `{tok}' to conditional unary operator\n{near}")
            }
            CondPos::Binary => {
                format!("unexpected argument `{tok}' to conditional binary operator\n{near}")
            }
        };
        ParseError::new(msg)
    }

    /// Peek at a binary operator following an operand, without consuming.
    fn peek_cond_binop(&self) -> Option<RawBinOp> {
        match self.peek() {
            Some(Tok::Op(Op::Less)) => Some(RawBinOp::StrLt),
            Some(Tok::Op(Op::Great)) => Some(RawBinOp::StrGt),
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
                        && RESERVED.contains(&s.as_str())
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
                    self.pos += 1;
                    cmd.words.push(self.word_from_segs(&segs)?);
                    seen_word = true;
                }
                Some(Tok::ArrayAssign { .. }) => {
                    // After the command word, an array literal is only allowed as
                    // an operand of a declaration builtin (`declare -A m=([k]=v)`);
                    // anywhere else it's a syntax error.
                    let is_decl_operand = seen_word && is_declaration_command(&cmd.words);
                    if seen_word && !is_decl_operand {
                        return Err(ParseError::new(
                            "array assignment is only valid before the command word".into(),
                        ));
                    }
                    let Some(Tok::ArrayAssign {
                        name,
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
                    let assign = Assignment {
                        name,
                        index: None,
                        append,
                        value: AssignRhs::Array(items),
                    };
                    if is_decl_operand {
                        cmd.decl_arrays.push(assign);
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
        // `>&` (GreatAnd) is `RedirectOp::DupOut` when its target is a numeric fd
        // (`>&1`) or `-` (`>&-`), but redirects *both* stdout and stderr to a
        // file when the target is a filename (`>&file`). We resolve that after
        // parsing the target below.
        let mut was_great_and = false;
        // `<<-` strips leading tabs; only the operator token records which
        // spelling was used, so capture it before the token is left behind.
        let mut here_strip = false;
        let op = match self.bump() {
            Some(Tok::Op(Op::Less)) => RedirectOp::Read,
            Some(Tok::Op(Op::Great)) => RedirectOp::Write,
            Some(Tok::Op(Op::GreatPipe)) => RedirectOp::Clobber,
            Some(Tok::Op(Op::DGreat)) => RedirectOp::Append,
            Some(Tok::Op(Op::GreatAnd)) => {
                was_great_and = true;
                RedirectOp::DupOut
            }
            Some(Tok::Op(Op::LessAnd)) => RedirectOp::DupIn,
            Some(Tok::Op(Op::LessGreat)) => RedirectOp::ReadWrite,
            Some(Tok::Op(Op::AmpGreat)) => RedirectOp::WriteBoth,
            Some(Tok::Op(Op::AmpDGreat)) => RedirectOp::AppendBoth,
            Some(Tok::Op(op @ (Op::DLess | Op::DLessDash))) => {
                here_strip = op == Op::DLessDash;
                RedirectOp::HereDoc
            }
            Some(Tok::Op(Op::TLess)) => RedirectOp::HereStr,
            _ => return Err(ParseError::new("expected redirection operator".into())),
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
                self.pos = self.pos.saturating_add(1);
                self.word_from_segs(&segs)?
            }
            Some(Tok::Word(segs)) => {
                let segs = segs.clone();
                self.pos = self.pos.saturating_add(1);
                self.word_from_segs(&segs)?
            }
            _ => return Err(self.unexpected_here()),
        };
        // `>&file` (non-numeric *literal* target, no explicit/var fd) means
        // "both fds to file". A `{v}>&…` form keeps its dup semantics (varfd is
        // not "both"). When the target contains expansions (`>&$v`) we cannot
        // classify it at parse time — it must be resolved at runtime: a numeric
        // expansion is a dup, a non-numeric one is an ambiguous redirect (or
        // "both to file" for the `1>&` corner). So keep it as `DupOut` and let
        // `resolve_redirects` decide.
        let target_is_literal = target
            .parts
            .iter()
            .all(|p| matches!(p, WordPart::Literal(_)));
        let op = if was_great_and
            && explicit_fd.is_none()
            && varfd.is_none()
            && target_is_literal
            && !dup_target_is_fd(&target)
        {
            RedirectOp::WriteBoth
        } else {
            op
        };
        Ok(Redirect {
            fd,
            op,
            target,
            varfd,
            here,
        })
    }

    fn expect_reserved(&mut self, w: &str) -> Result<(), ParseError> {
        if self.reserved_here().as_deref() == Some(w) {
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
        let name = &first[..name_len];
        let (index, after_lhs) = if first[name_len..].starts_with('[') {
            match balanced_subscript_end(&first[name_len..]) {
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
                    let idx_src = &first[name_len + 1..name_len + close];
                    // A subscript is parsed verbatim (no word-splitting or
                    // trimming): for an associative array the expanded text —
                    // leading/trailing whitespace included — is the literal key
                    // (bash: `h[ x ]=v` keys on ` x `). For an indexed array the
                    // arithmetic evaluator ignores the whitespace, so preserving
                    // it is harmless.
                    let idx = word_verbatim_from_source(idx_src, self.opts)?;
                    (Some(idx), &first[name_len + close + 1..])
                }
            }
        } else {
            (None, &first[name_len..])
        };
        // `+=` append. Only these two spellings follow the left-hand side; a
        // word that runs on into anything else is not an assignment.
        let (append, after) = if let Some(rest) = after_lhs.strip_prefix("+=") {
            (true, rest)
        } else if let Some(rest) = after_lhs.strip_prefix('=') {
            (false, rest)
        } else {
            return Ok(None);
        };
        // Build the value word from the remainder of the first seg plus the
        // rest of the segments.
        let mut value_segs: Vec<Seg> = Vec::new();
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_string()));
        }
        value_segs.extend_from_slice(&segs[1..]);
        Ok(Some(Assignment {
            name: name.to_string(),
            index,
            append,
            value: AssignRhs::Scalar(self.word_from_segs(&value_segs)?),
        }))
    }

    /// Lower lexer segments into an [`ast::Word`].
    fn word_from_segs(&self, segs: &[Seg]) -> Result<Word, ParseError> {
        word_from_segs(segs, self.opts)
    }

    /// Parse `name[SUBSCRIPT]=value` / `name[SUBSCRIPT]+=value` where the
    /// subscript spans multiple segments (contains `$…` expansions). `open` is
    /// the byte offset of `[` in the first (literal) segment.
    fn spanning_subscript_assignment(
        &self,
        segs: &[Seg],
        first: &str,
        open: usize,
    ) -> Result<Option<Assignment>, ParseError> {
        let name = &first[..open];
        if name.is_empty() || !is_valid_name(name) {
            return Ok(None);
        }
        // Subscript segments: the first seg's text after `[`, then whole
        // segments, up to the segment that carries the closing `]`.
        let mut sub_segs: Vec<Seg> = Vec::new();
        let after_open = &first[open + 1..];
        if !after_open.is_empty() {
            sub_segs.push(Seg::Lit(after_open.to_string()));
        }
        let mut value_segs: Vec<Seg> = Vec::new();
        let mut append = false;
        let mut found = false;
        for seg in &segs[1..] {
            if found {
                value_segs.push(seg.clone());
                continue;
            }
            if let Seg::Lit(s) = seg
                && let Some(close) = s.find(']')
            {
                let before = &s[..close];
                if !before.is_empty() {
                    sub_segs.push(Seg::Lit(before.to_string()));
                }
                let rest = &s[close + 1..];
                let val_lit = if let Some(v) = rest.strip_prefix("+=") {
                    append = true;
                    v
                } else if let Some(v) = rest.strip_prefix('=') {
                    v
                } else {
                    // `]` not immediately followed by `=` — not an assignment.
                    return Ok(None);
                };
                if !val_lit.is_empty() {
                    value_segs.push(Seg::Lit(val_lit.to_string()));
                }
                found = true;
                continue;
            }
            sub_segs.push(seg.clone());
        }
        if !found || sub_segs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Assignment {
            name: name.to_string(),
            index: Some(self.word_from_segs(&sub_segs)?),
            append,
            value: AssignRhs::Scalar(self.word_from_segs(&value_segs)?),
        }))
    }
}

/// Parse one array-literal element: either `[sub]=value` (keyed) or a bare
/// positional value. A keyed element is recognised when the first segment is a
/// literal that starts with `[` and contains `]=` (so the subscript is literal
/// text — an expanded key like `[$k]=v` inside a literal falls back to
/// positional; use element assignment `m[$k]=v` for that).
fn parse_array_elem(segs: &[Seg], opts: LexOpts) -> Result<ArrayElem, ParseError> {
    if let Some(Seg::Lit(first)) = segs.first()
        && first.starts_with('[')
        && let Some(close_eq) = first.find("]=")
    {
        // Verbatim: an associative keyed element `[ x ]=v` keys on the literal
        // ` x ` (bash preserves subscript whitespace); indexed elements
        // arithmetic-evaluate, which ignores it.
        let index = word_verbatim_from_source(&first[1..close_eq], opts)?;
        let mut value_segs: Vec<Seg> = Vec::new();
        let after = &first[close_eq + 2..];
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_string()));
        }
        value_segs.extend_from_slice(&segs[1..]);
        return Ok(ArrayElem::Keyed {
            index,
            value: word_from_segs(&value_segs, opts)?,
        });
    }
    // General keyed element: the subscript spans quoted or expansion segments,
    // so the closing `]=` is not in the same literal as the opening `[`
    // (`["k v"]=1`, `['k']=1`, `[$x]=1`). The opening `[` is the start of the
    // first literal; everything up to the first unquoted `]=` (which lands in a
    // later literal segment) is the key — intervening quoted/expansion segments
    // belong to it and are copied verbatim.
    if let Some(Seg::Lit(first)) = segs.first()
        && first.starts_with('[')
        && !first.contains("]=")
    {
        let mut key_segs: Vec<Seg> = Vec::new();
        let head = &first[1..];
        if !head.is_empty() {
            key_segs.push(Seg::Lit(head.to_string()));
        }
        for (i, seg) in segs.iter().enumerate().skip(1) {
            if let Seg::Lit(s) = seg
                && let Some(pos) = s.find("]=")
            {
                if !s[..pos].is_empty() {
                    key_segs.push(Seg::Lit(s[..pos].to_string()));
                }
                let index = word_from_segs(&key_segs, opts)?;
                let mut value_segs: Vec<Seg> = Vec::new();
                let after = &s[pos + 2..];
                if !after.is_empty() {
                    value_segs.push(Seg::Lit(after.to_string()));
                }
                value_segs.extend_from_slice(&segs[i + 1..]);
                return Ok(ArrayElem::Keyed {
                    index,
                    value: word_from_segs(&value_segs, opts)?,
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
        name.as_str(),
        "declare" | "typeset" | "local" | "export" | "readonly"
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
/// picks that word up too: `[[ a>>b ]]` is reported near `a>>b`. Reproducing
/// that needs the source text, not the token — see TD-OILS-COND-TOKEN-SPELLING.
fn cond_error_near(tok: &str) -> String {
    match tok.chars().next_back() {
        Some(c @ (';' | '|' | '&')) => c.to_string(),
        _ => tok.to_string(),
    }
}

/// Lower lexer segments into an [`ast::Word`] (stateless).
fn word_from_segs(segs: &[Seg], opts: LexOpts) -> Result<Word, ParseError> {
    let mut parts = Vec::with_capacity(segs.len());
    for s in segs {
        parts.push(seg_to_part(s, opts)?);
    }
    Ok(Word { parts })
}

fn seg_to_part(seg: &Seg, opts: LexOpts) -> Result<WordPart, ParseError> {
    Ok(match seg {
        Seg::Lit(s) => WordPart::Literal(s.clone()),
        Seg::Sq(s, escaped) => WordPart::SingleQuoted {
            text: s.clone(),
            escaped: *escaped,
        },
        Seg::Dq(inner) => {
            let mut parts = Vec::with_capacity(inner.len());
            for s in inner {
                parts.push(seg_to_part(s, opts)?);
            }
            WordPart::DoubleQuoted(parts)
        }
        Seg::Param(n) => WordPart::Param(n.clone()),
        Seg::ParamBraced(raw) => parse_braced_param(raw, opts)?,
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
fn matching_subscript_close(bytes: &[char], open: usize) -> Option<usize> {
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
    while i < bytes.len() {
        match bytes[i] {
            // A backslash escapes the next character (skip both).
            '\\' => i += 1,
            // Single-quoted run: verbatim to the closing quote (no escapes).
            '\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != '\'' {
                    i += 1;
                }
            }
            // Double-quoted run: to the closing quote, honoring `\`.
            '"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != '"' {
                    if bytes[i] == '\\' {
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
    Split(String, Option<ArrayIndex>, Vec<char>),
    /// The body is well-formed enough to *parse* but cannot be interpreted, so
    /// bash defers the complaint to expansion time. The only such case here is
    /// an empty subscript (`${a[]}`), which bash parses happily — a guarded
    /// `if false; then echo "${a[]}"; fi` is silent — and only rejects when the
    /// word is expanded, as a runtime "bad substitution". Every caller turns
    /// this into [`WordPart::BadSubst`]. Note the check is purely lexical:
    /// `${a[  ]}` is *not* empty, and arithmetic-evaluates its blanks to index 0.
    Deferred,
}

fn split_name_subscript(
    bytes: &[char],
    opts: LexOpts,
) -> Result<NameSubscript, ParseError> {
    if bytes.is_empty() {
        return Err(ParseError::new("empty '${}' expansion".into()));
    }
    let mut i = 0;
    if bytes[0].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else if is_name_start(bytes[0]) {
        while i < bytes.len() && is_name_char(bytes[i]) {
            i += 1;
        }
    } else {
        // A special single-char parameter (`@`, `*`, `?`, `#`, `!`, `$`, …).
        i = 1;
    }
    let name: String = bytes[..i].iter().collect();
    if bytes.get(i) == Some(&'[')
        && let Some(close) = matching_subscript_close(bytes, i)
    {
        let inner: String = bytes[i + 1..close].iter().collect();
        let index = match inner.as_str() {
            "@" => ArrayIndex::All,
            "*" => ArrayIndex::Star,
            "" => return Ok(NameSubscript::Deferred),
            // Verbatim so an associative read `${h[ x ]}` keys on the literal
            // ` x ` (bash preserves subscript whitespace); indexed reads
            // arithmetic-evaluate, which ignores the whitespace.
            _ => ArrayIndex::Index(Box::new(word_verbatim_from_source(&inner, opts)?)),
        };
        return Ok(NameSubscript::Split(
            name,
            Some(index),
            bytes[close + 1..].to_vec(),
        ));
    }
    Ok(NameSubscript::Split(name, None, bytes[i..].to_vec()))
}

/// Parse the `offset[:length]` portion of a substring/slice expansion (the
/// text after the leading `:`). The offset and each length are parsed as
/// arithmetic words. Splits on the *first* unescaped `:` only.
fn parse_slice_bounds(
    rest: &[char],
    opts: LexOpts,
) -> Result<(Box<Word>, Option<Box<Word>>), ParseError> {
    let body: String = rest.iter().collect();
    let (off_str, len_str) = match body.find(':') {
        Some(idx) => (body[..idx].to_string(), Some(body[idx + 1..].to_string())),
        None => (body, None),
    };
    let length = match len_str {
        Some(s) => Some(Box::new(word_from_source(&s, opts)?)),
        None => None,
    };
    Ok((Box::new(word_from_source(&off_str, opts)?), length))
}

pub(crate) fn parse_braced_param(raw: &str, opts: LexOpts) -> Result<WordPart, ParseError> {
    if let Some(after_hash) = raw.strip_prefix('#') {
        if after_hash.is_empty() {
            // `${#}` is the positional-parameter count — treat as `$#`.
            return Ok(WordPart::Param("#".into()));
        }
        let bytes: Vec<char> = after_hash.chars().collect();
        let NameSubscript::Split(name, subscript, remaining) =
            split_name_subscript(&bytes, opts)?
        else {
            return Ok(WordPart::BadSubst(raw.to_string()));
        };
        if let Some(index) = subscript {
            if !remaining.is_empty() {
                // bash accepts this at parse time and rejects it only during
                // expansion as a runtime "bad substitution" (DISCARD-class).
                return Ok(WordPart::BadSubst(raw.to_string()));
            }
            // `${#name[@]}` / `${#name[i]}` — array element count / element length.
            return Ok(WordPart::ArrayRef {
                name,
                index,
                length: true,
            });
        }
        return Ok(WordPart::Length(after_hash.to_string()));
    }
    if let Some(after_bang) = raw.strip_prefix('!') {
        // `${!prefix*}` / `${!prefix@}` — names of set variables beginning with
        // `prefix`. Distinguished from the array-keys form (`${!a[@]}`) by
        // ending in a bare `*`/`@` (no closing `]`). A valid name prefix is
        // required so we don't mistake other expansions.
        // A *non-empty* prefix is required for the name-listing form: a bare
        // `${!*}`/`${!@}` is instead indirect expansion through the positional
        // list (`$*`/`$@`), handled below, not a listing of every variable.
        if let Some(prefix) = after_bang.strip_suffix('*')
            && !prefix.is_empty()
            && !prefix.contains('[')
            && is_valid_name(prefix)
        {
            return Ok(WordPart::VarNames {
                prefix: prefix.to_string(),
                star: true,
            });
        }
        if let Some(prefix) = after_bang.strip_suffix('@')
            && !prefix.is_empty()
            && !prefix.contains('[')
            && is_valid_name(prefix)
        {
            return Ok(WordPart::VarNames {
                prefix: prefix.to_string(),
                star: false,
            });
        }
        // `${!name[@]}` / `${!name[*]}` — the keys/indices of an array.
        let bytes: Vec<char> = after_bang.chars().collect();
        let NameSubscript::Split(name, subscript, remaining) =
            split_name_subscript(&bytes, opts)?
        else {
            return Ok(WordPart::BadSubst(raw.to_string()));
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
        // specific subscript can point — `[@]`/`[*]` were the key listing just
        // above — and only a plain name can carry one, no positional or special
        // parameter being an array.
        let index: Option<Box<Word>> = match subscript {
            None => None,
            Some(ArrayIndex::Index(w)) if is_valid_name(&name) => Some(w),
            Some(_) => return Ok(WordPart::BadSubst(raw.to_string())),
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
            // expansion time. Only scalar modifiers combine with indirection,
            // and only a plain-name referent may carry a trailing modifier.
            if is_valid_name(&name) {
                let modifier_src: String =
                    name.chars().chain(remaining.iter().copied()).collect();
                let target = parse_braced_param(&modifier_src, opts)?;
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
        }
        // bash accepts this at parse time and rejects it only during expansion
        // as a runtime "bad substitution" (DISCARD-class).
        return Ok(WordPart::BadSubst(raw.to_string()));
    }
    let bytes: Vec<char> = raw.chars().collect();
    let NameSubscript::Split(name, subscript, rest) = split_name_subscript(&bytes, opts)? else {
        return Ok(WordPart::BadSubst(raw.to_string()));
    };
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
            if rest[0] == ':' && !matches!(rest.get(1), Some('-' | '=' | '+' | '?')) {
                let (offset, length) = parse_slice_bounds(&rest[1..], opts)?;
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
            if rest.first() == Some(&'@') {
                return Ok(WordPart::ArrayBulk {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    op: BulkOp::BadTransform {
                        raw: raw.to_string(),
                    },
                });
            }
            // `${a[@]:-x}` / `${a[*]:+x}` / `${a[@]:?msg}` — use/alternate/error
            // operators on a whole-array reference. Bash treats `[@]`/`[*]` like
            // `$@`: substitute the elements when active, else the operand word.
            let star = matches!(index, ArrayIndex::Star);
            let mut chs = rest.iter();
            let mut c = *chs.next().unwrap_or(&'\0');
            let colon = c == ':';
            if colon {
                c = *chs.next().unwrap_or(&'\0');
            }
            let arg_str: String = chs.collect();
            let op = match c {
                '-' => ParamOp::UseDefault,
                '=' => ParamOp::AssignDefault,
                '+' => ParamOp::UseAlternate,
                '?' => ParamOp::ErrorIfUnset,
                _ => {
                    // bash accepts this at parse time and rejects it only during
                    // expansion as a runtime "bad substitution" (DISCARD-class).
                    return Ok(WordPart::BadSubst(raw.to_string()));
                }
            };
            return Ok(WordPart::ArrayOp {
                name,
                star,
                op,
                colon,
                arg: Box::new(word_verbatim_from_source(&arg_str, opts)?),
            });
        }
    };
    // `${@:off:len}` / `${*:off:len}` — positional-parameter slice (same `:`
    // rule as the array form; distinguished from string substring because the
    // parameter names the whole positional list).
    if (name == "@" || name == "*")
        && !rest.is_empty()
        && rest[0] == ':'
        && !matches!(rest.get(1), Some('-' | '=' | '+' | '?'))
    {
        let (offset, length) = parse_slice_bounds(&rest[1..], opts)?;
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
    if (name == "@" || name == "*") && rest.first() == Some(&'@') {
        return Ok(WordPart::ArrayBulk {
            name: name.clone(),
            star: name == "*",
            op: BulkOp::BadTransform {
                raw: raw.to_string(),
            },
        });
    }
    if rest.is_empty() {
        return Ok(WordPart::Param(name));
    }
    match rest[0] {
        // Prefix / suffix trimming: `#`, `##`, `%`, `%%`.
        '#' | '%' => {
            let suffix = rest[0] == '%';
            let longest = rest.get(1) == Some(&rest[0]);
            let pat_start = if longest { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
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
            let mode = match rest[0] {
                '^' => crate::ast::CaseMode::Upper,
                ',' => crate::ast::CaseMode::Lower,
                _ => crate::ast::CaseMode::Toggle,
            };
            let all = rest.get(1) == Some(&rest[0]);
            let pat_start = if all { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
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
            if rest.len() == 2 && is_valid_transform_op(rest[1]) {
                return Ok(WordPart::ParamTransform {
                    name,
                    index: elem_index,
                    op: rest[1],
                });
            }
            Ok(WordPart::BadTransform {
                name,
                index: elem_index,
                raw: raw.to_string(),
            })
        }
        // Pattern substitution: `/pat/repl`, `//pat/repl`, `/#…`, `/%…`.
        '/' => parse_param_replace(name, elem_index, &rest[1..], opts),
        // Substring `:offset[:length]` — but `:` followed by one of -=+? is the
        // use/assign/alt/error operator, handled below.
        ':' if !matches!(rest.get(1), Some('-' | '=' | '+' | '?')) => {
            let (offset, length) = parse_slice_bounds(&rest[1..], opts)?;
            Ok(WordPart::ParamSubstr {
                name,
                index: elem_index,
                offset,
                length,
            })
        }
        // `:-`, `:=`, `:+`, `:?` and the colon-less `-=+?` forms.
        _ => {
            let mut chs = rest.iter();
            let mut c = *chs.next().unwrap_or(&'\0');
            // A leading `:` selects the null-or-unset (colon) form; without it the
            // operator acts only when the parameter is genuinely unset.
            let colon = c == ':';
            if colon {
                c = *chs.next().unwrap_or(&'\0');
            }
            let arg_str: String = chs.collect();
            let op = match c {
                '-' => ParamOp::UseDefault,
                '=' => ParamOp::AssignDefault,
                '+' => ParamOp::UseAlternate,
                '?' => ParamOp::ErrorIfUnset,
                _ => {
                    // bash accepts this at parse time and rejects it only during
                    // expansion as a runtime "bad substitution" (DISCARD-class).
                    return Ok(WordPart::BadSubst(raw.to_string()));
                }
            };
            Ok(WordPart::ParamOp {
                name,
                index: elem_index,
                op,
                colon,
                arg: Box::new(word_verbatim_from_source(&arg_str, opts)?),
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
    body: &[char],
    opts: LexOpts,
) -> Result<(bool, ReplaceAnchor, Box<Word>, Box<Word>), ParseError> {
    let mut i = 0;
    let mut all = false;
    let mut anchor = ReplaceAnchor::None;
    match body.first() {
        Some('/') => {
            all = true;
            i = 1;
        }
        Some('#') => {
            anchor = ReplaceAnchor::Start;
            i = 1;
        }
        Some('%') => {
            anchor = ReplaceAnchor::End;
            i = 1;
        }
        _ => {}
    }
    // Pattern runs to the next unescaped '/'; the remainder is the replacement.
    let mut pattern = String::new();
    let mut replacement = String::new();
    let mut in_repl = false;
    while i < body.len() {
        let c = body[i];
        if !in_repl && c == '\\' && body.get(i + 1) == Some(&'/') {
            pattern.push('/');
            i += 2;
            continue;
        }
        if !in_repl && c == '/' {
            in_repl = true;
            i += 1;
            continue;
        }
        if in_repl {
            replacement.push(c);
        } else {
            pattern.push(c);
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
    body: &[char],
    opts: LexOpts,
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
fn parse_bulk_op(rest: &[char], opts: LexOpts) -> Result<Option<BulkOp>, ParseError> {
    if rest.is_empty() {
        return Ok(None);
    }
    match rest[0] {
        '#' | '%' => {
            let suffix = rest[0] == '%';
            let longest = rest.get(1) == Some(&rest[0]);
            let pat_start = if longest { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(Some(BulkOp::Trim {
                suffix,
                longest,
                pattern: Box::new(word_verbatim_from_source(&pat, opts)?),
            }))
        }
        '^' | ',' | '~' => {
            let mode = match rest[0] {
                '^' => crate::ast::CaseMode::Upper,
                ',' => crate::ast::CaseMode::Lower,
                _ => crate::ast::CaseMode::Toggle,
            };
            let all = rest.get(1) == Some(&rest[0]);
            let pat_start = if all { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(Some(BulkOp::Case {
                mode,
                all,
                pattern: Box::new(word_verbatim_from_source(&pat, opts)?),
            }))
        }
        '/' => {
            let (all, anchor, pattern, replacement) = parse_replace_pieces(&rest[1..], opts)?;
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
        '@' if rest.len() == 2 && is_valid_transform_op(rest[1]) => {
            Ok(Some(BulkOp::Transform { op: rest[1] }))
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
pub(crate) fn word_verbatim_from_source(s: &str, opts: LexOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_word_verbatim(s).map_err(|e| ParseError::new(e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts)?);
    }
    Ok(Word { parts })
}

/// Like [`word_verbatim_from_source`] but for the *replacement* half of
/// `${var/pat/repl}`: a literal `\&`/`\\` is preserved (not consumed at lex
/// time) so the runtime `&`-substitution can distinguish an escaped ampersand
/// from an active one. See [`crate::lexer::lex_replacement_verbatim`].
fn word_replacement_from_source(s: &str, opts: LexOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_replacement_verbatim(s).map_err(|e| ParseError::new(e.msg))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg, opts)?);
    }
    Ok(Word { parts })
}

fn word_from_source(s: &str, opts: LexOpts) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let toks = tokenize(s, opts).map_err(|e| ParseError::new(e.msg))?;
    let mut parts: Vec<WordPart> = Vec::new();
    let mut first = true;
    for t in &toks {
        if let Tok::Word(segs) = t {
            if !first {
                parts.push(WordPart::Literal(" ".into()));
            }
            first = false;
            for seg in segs {
                parts.push(seg_to_part(seg, opts)?);
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

pub(crate) fn is_valid_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if is_name_start(c) => {}
        _ => return false,
    }
    it.all(is_name_char)
}

/// Byte length of the identifier `s` begins with, or `0` if it does not begin
/// with one. Used to anchor assignment-word recognition to the name, so that a
/// `[` or `=` further right — in the *value* — cannot be mistaken for the start
/// of a subscript or for the assignment operator.
fn name_prefix_len(s: &str) -> usize {
    let mut len = 0;
    for (i, c) in s.char_indices() {
        if i == 0 {
            if !is_name_start(c) {
                return 0;
            }
        } else if !is_name_char(c) {
            break;
        }
        len = i + c.len_utf8();
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
fn balanced_subscript_end(s: &str) -> Option<usize> {
    debug_assert!(s.starts_with('['));
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
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

/// A referent usable in a *bare* indirect expansion `${!name}`: a plain
/// identifier, a positional parameter (all digits, `${!1}`), or a special
/// single-char parameter. bash accepts `#`, `?`, and `-` here but **rejects**
/// `$` and `!` (`${!$}`/`${!!}` are a "bad substitution"). A bare `@`/`*` is
/// indirect expansion through the positional list: `${!@}` / `${!*}` treat
/// each positional parameter's *value* as a variable name to indirect through
/// (bash then rejects them as "invalid variable name" unless empty). Only a
/// *prefixed* `@`/`*` (`${!prefix@}`) is the variable-name listing form.
fn is_indirect_referent(name: &str) -> bool {
    is_valid_name(name)
        || (!name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
        || matches!(name, "#" | "?" | "-" | "@" | "*")
}

/// True when a `>&`/`<&` target denotes an fd duplication (a bare number or
/// `-`) rather than a filename. Only a single unquoted literal qualifies, so
/// `>&$var` or `>&"file"` are treated as filenames (redirect both).
fn dup_target_is_fd(target: &Word) -> bool {
    if let [WordPart::Literal(s)] = target.parts.as_slice() {
        s == "-" || (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    } else {
        false
    }
}

/// Map a `[[ … ]]` unary operator string to its [`UnaryOp`].
fn unary_op_from(s: &str) -> Option<UnaryOp> {
    Some(match s {
        "-e" => UnaryOp::Exists,
        "-f" => UnaryOp::File,
        "-d" => UnaryOp::Dir,
        "-r" => UnaryOp::Readable,
        "-w" => UnaryOp::Writable,
        "-x" => UnaryOp::Executable,
        "-s" => UnaryOp::NonEmptyFile,
        "-z" => UnaryOp::ZeroLen,
        "-n" => UnaryOp::NonZeroLen,
        "-v" => UnaryOp::VarSet,
        "-o" => UnaryOp::OptionSet,
        "-L" | "-h" => UnaryOp::Symlink,
        "-t" => UnaryOp::Terminal,
        _ => return None,
    })
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
    fn into_bin_op(self) -> CondBinOp {
        match self {
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
        }
    }
}

/// Map a `[[ … ]]` binary operator word to its [`RawBinOp`].
fn raw_binop_from(s: &str) -> Option<RawBinOp> {
    Some(match s {
        "==" | "=" => RawBinOp::StrEq,
        "!=" => RawBinOp::StrNe,
        "=~" => RawBinOp::Regex,
        "-eq" => RawBinOp::NumEq,
        "-ne" => RawBinOp::NumNe,
        "-lt" => RawBinOp::NumLt,
        "-le" => RawBinOp::NumLe,
        "-gt" => RawBinOp::NumGt,
        "-ge" => RawBinOp::NumGe,
        "-nt" => RawBinOp::FileNewer,
        "-ot" => RawBinOp::FileOlder,
        "-ef" => RawBinOp::SameFile,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// bash names the offending word in a syntax error by its *source* spelling,
    /// so the message shows the quotes the user typed rather than the string
    /// they stand for. Every expectation here is bash 5.2.37's own wording.
    #[test]
    fn syntax_error_names_the_token_as_written() {
        fn err(src: &str) -> String {
            parse(src).expect_err("expected a parse error").msg
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
            parse(src).expect_err("expected a parse error").msg
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
            parse(src).unwrap_or_else(|e| panic!("{src}: {}", e.msg));
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
        assert_eq!(f.var, "x");
        assert_eq!(f.words.as_ref().unwrap().len(), 3);
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
        assert_eq!(f.name, "greet");
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
            (f.name.clone(), f.definable)
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
        assert_eq!(f.name, "greet");

        // bash keyword form WITH parentheses.
        let prog = parse("function greet() { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "greet");

        // A non-identifier name is permitted in the keyword form.
        let prog = parse("function foo-bar { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "foo-bar");

        // Multi-line body and a subshell body.
        assert!(parse("function f {\necho a\necho b\n}").is_ok());
        let prog = parse("function f() ( echo sub )").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "f");

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
            parse("function f").unwrap_err().msg,
            "syntax error: unexpected end of file"
        );
        assert_eq!(
            parse("f()").unwrap_err().msg,
            "syntax error: unexpected end of file"
        );
        // A non-body token after the header names the offending token.
        assert_eq!(
            parse("f() echo hi").unwrap_err().msg,
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
            assert_eq!(parse(src).unwrap_err().msg, want, "src {src:?}");
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
            assert_eq!(parse(src).unwrap_err().msg, want, "src {src:?}");
        }
        // A well-formed redirection still parses.
        assert!(parse("echo hi > out.txt").is_ok());
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
            assert_eq!(parse(src).unwrap_err().msg, want, "src {src:?}");
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
        assert_eq!(sc.decl_arrays[0].name, "m");
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
        let opts = LexOpts::default();
        IncrementalParser::new(src, LineMap::Offset(close_line.saturating_sub(1)), opts)
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
            assert_eq!(e.msg, "syntax error near unexpected token `)'", "{src}");
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
            backtick_unit("for", 1).unwrap_err().msg,
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
        assert_eq!(*op, CondBinOp::StrEq);
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
            let prog = parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
            let Command::Cond(CondExpr::Regex(_, rhs)) = &prog.items[0].list.first.commands[0]
            else {
                panic!("{src}: expected a regex conditional");
            };
            match rhs.parts.as_slice() {
                [WordPart::Literal(s)] => s.clone(),
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
        let e = parse("[[ $x =~ (a(b) ]]").unwrap_err().to_string();
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
        assert_eq!(raw.trim(), "x + 1");
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
