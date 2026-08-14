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

/// Does this text read as an arithmetic expression rather than as commands?
///
/// bash's `chk_arithsub` (subst.c:9487), which is what decides whether a
/// `$(( … ))` is arithmetic at all. Parentheses must balance and must never dip
/// below zero, with `\X`, `'…'` and `"…"` stepped over.
///
/// It is deliberately *not* the set the scan that found the text steps over: a
/// backtick is skipped there and not here. So `` $(( 1 + `echo 2)3` )) `` — whose
/// scan the backtick did protect, giving it the `))` it needed — fails this test
/// and is run as a command substitution, printing `1: command not found`. The
/// asymmetry is bash's, it is observable, and it is the reason the two steps are
/// written separately.
fn is_arith_expr(s: &[u8]) -> bool {
    let mut depth = 0i64;
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        i += 1;
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            // The escaped character travels with its backslash, whatever it is.
            b'\\' => i += 1,
            b'\'' | b'"' => {
                while let Some(&q) = s.get(i) {
                    i += 1;
                    // Only a double-quoted span honours the escape; inside single
                    // quotes a backslash is an ordinary character.
                    if q == b'\\' && c == b'"' {
                        i += 1;
                    } else if q == c {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    depth == 0
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
    /// The body of the `$( … )` / `<( … )` this error ran out of input inside,
    /// and the line its `(` sat on. See [`SubstBail`].
    pub bail: Option<SubstBail>,
    /// Set when the construct that ran out of input was written in text no
    /// parser read, which makes the failure a *runtime* one rather than this
    /// error. See [`UnreadEof`]; the scan that meets it turns the error back
    /// into a segment.
    ///
    /// Boxed because it is the largest thing an error can carry and the rarest:
    /// only text no parser read can set it, so the allocation never happens
    /// while lexing a script, and every other error stays cheap to move.
    pub unclosed: Option<Box<UnreadEof>>,
}

/// Which segment a scan that ran out of input in unread text becomes.
///
/// Both are failures of the *expansion* rather than of any parse — see
/// [`Unclosed`] — but they are reported by different parts of bash and so by
/// different parts of this shell, which is the whole of the distinction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnreadEof {
    /// A `${ … }`, `$(( … ))`, `$[ … ]` or `` ` … ` `` — one of `subst.c`'s own
    /// scanners, reported by `Shell::expand_unclosed`.
    Subst(Unclosed),
    /// A `$( … )`, carrying everything after its `$(`. This one is not a scan at
    /// all: `extract_command_subst` runs `xparse_dolparen`, a real parse, over
    /// the rest of the text, so the failure and its diagnostic are the ordinary
    /// ones an unreadable body earns — see [`crate::ast::CmdSubBody::Unread`].
    ///
    /// It wins over any [`UnreadEof::Subst`] an enclosing scan would have
    /// raised, because that scan is what *reaches* the `$(`: a `${ … }` whose
    /// body holds one is abandoned mid-scan by the nested parse's own jump.
    CmdSub(Str),
}

/// A construct left open in text no parser ever read as a word — a
/// here-document body, a `PS4`, a `${x@P}`.
///
/// It cannot be a *parse* error, because no parse ever looked at the text. The
/// scanner that meets it is the expansion-time one in `subst.c`, so the failure
/// happens when the word is expanded: it kills only the enclosing command and
/// lets the script carry on. Two reporters share the job, and which of them
/// fires is decided by whichever scan gives up first — reading left to right and
/// innermost-out, except that a scan which *skips* a nested construct rather
/// than reading it never reports at all. `${x:-`echo` is the brace's failure,
/// not the backquote's, because `extract_dollar_brace_string` steps over a
/// backquote with `string_extract`, which runs quietly to the end of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unclosed {
    /// `${ … }` (`extract_dollar_brace_string`, subst.c:1785, 1980),
    /// `$(( … ))` and `$[ … ]` (`extract_delimited_string`, subst.c:1498). All
    /// three say ``bad substitution: no closing `%s' in %s`` at the shell's
    /// plain line and then `exp_jump_to_top_level (DISCARD)`.
    BadSubst {
        /// The delimiter the scan was still looking for: `}`, `)` or `]`.
        close: char,
        /// The construct as written, from its `$` to the end of the text — what
        /// a `declare -f` prints back, and the only part of the text this
        /// segment stands for.
        src: Str,
        /// The `%s`, which is the **whole text being expanded** — bash's
        /// `string`, not the construct — so a body of three lines echoes all
        /// three, and text before the construct is echoed with it.
        text: Str,
    },
    /// `` ` … ` `` met at the top level of the text, which `param_expand`
    /// reports itself (subst.c:11290) with a message of its own:
    /// ``bad substitution: no closing "`" in %s``. Unlike the other three it
    /// does not test `no_longjmp_on_fatal_error`, so a prompt expansion prints
    /// it too.
    Backquote {
        /// The construct as written, from the backquote to the end of the text.
        /// It is also the `%s`, which here is `string + t_index` — only the text
        /// from the backquote on, where the other reporter echoes the whole
        /// of it.
        src: Str,
    },
}

impl Unclosed {
    /// The text this stands for, as written — so that a here-document body
    /// holding one prints back unchanged.
    #[must_use]
    pub fn src(&self) -> BStr<'_> {
        match self {
            Self::BadSubst { src, .. } | Self::Backquote { src } => src,
        }
    }
}

/// A substitution body the scan never found the `)` of, kept so that the error
/// can be *reconsidered* once someone able to parse it gets hold of it.
///
/// bash never scans for that `)` in the first place. `parse_comsub`
/// (parse.y:4083) runs a whole nested `yyparse` over the rest of the input with
/// `shell_eof_token = ')'`, so the body is parsed as it is read and the missing
/// paren is only noticed if that parse survives to end of input:
///
/// ```c
///   token_to_read = DOLPAREN;     /* let's trick the parser */
///   r = yyparse ();
///   …
///   if (EOF_Reached)
///     {
///       parser_state |= PST_NOERROR;
///       return (&matched_pair_error);
///     }
/// ```
///
/// An error *in* the body therefore comes out first and the `)` is never
/// mentioned: `echo $(fi` is ``syntax error near unexpected token `fi'``, not
/// ``unexpected EOF while looking for matching `)'``. The enclosing construct
/// is not consulted either — a `${`, a `$((` or a `"` around the substitution
/// never gets to miss its own delimiter, because the nested parse dies first.
///
/// osh scans for the `)` and parses the body afterwards, which is the opposite
/// order, so the body has to be carried out on the error to be given its say.
/// [`crate::parser`] is where that happens: it owns the lexer-error-to-parse-
/// error conversion and holds the [`ParseOpts`] a parse needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstBail {
    /// Everything from just past the `(` to the end of the input — which is the
    /// whole of the body, since the scan only bails when the input ran out.
    pub body: Str,
    /// The line the `(` was on, for numbering the body physically.
    pub open_line: u32,
}

impl LexError {
    /// A lexer error with no line preference; the caller's fallback applies.
    pub(crate) fn new(msg: &(impl bytes::PushBytes + ?Sized)) -> Self {
        Self {
            msg: bfmt![msg],
            line: None,
            looking_for: None,
            recoverable: false,
            bail: None,
            unclosed: None,
        }
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

    /// Mark this error as a construct left open in unread text, naming the
    /// reporter that will raise it at expansion time. Always *overwrites*: an
    /// enclosing scan that runs off the end after an inner one did is the one
    /// bash reports from, because the inner one it stepped over said nothing.
    /// See [`UnreadEof`].
    pub(crate) fn unclosed(mut self, what: UnreadEof) -> Self {
        self.unclosed = Some(Box::new(what));
        self
    }
}

/// bash's end-of-input diagnostic for a here-document whose delimiter never
/// arrived.
///
/// The delimiter is a shell word, so it goes back into the message as the bytes
/// the user wrote: `<<a\xffb` names the delimiter it was actually looking for,
/// which is the same byte string it compares the body lines against.
fn unterminated_heredoc(delim: BStr<'_>) -> LexError {
    LexError::new(&bfmt![b"unexpected EOF while looking for `", delim, b"'"])
}

/// bash's end-of-input diagnostic for an unclosed quote, substitution, or group.
/// bash names the delimiter it was scanning for, e.g. `unexpected EOF while
/// looking for matching `)'` — a single backtick, the closing char, then a
/// single quote — so a `$(`/`(` reports `)`, `${` reports `}`, `"` reports `"`.
fn eof_matching(close: char) -> LexError {
    LexError {
        msg: bfmt![b"unexpected EOF while looking for matching `", close, b"'"],
        line: None,
        looking_for: Some(close),
        recoverable: false,
        bail: None,
        unclosed: None,
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

/// When bash reads a command substitution's body, which is what the three
/// spellings of one actually differ in.
///
/// They all run the same command in the end. But only `$( … )` is parsed with
/// the enclosing input, and the other two are parsed at expansion time, per
/// expansion — so a body that does not parse is a *syntax error in the
/// enclosing unit* for the first and a diagnostic from the substitution for the
/// others, one that leaves the enclosing command running:
///
/// ```sh
/// if false; then echo $(fi);    fi   # syntax error: the `if` never runs
/// if false; then echo `fi`;     fi   # silence: the body is never read
/// if false; then echo $(( fi ) ); fi # silence, for the same reason
/// ```
///
/// What the body's lines are numbered from is the same for all three — the line
/// the *shell* stands on when the word is expanded, which is what `$LINENO`
/// reports there — because it is `command_substitute` that does the numbering
/// and all three reach it (subst.c:6986; see `Shell::command_sub_body_inner`).
/// What differs is the text those lines are counted over: a `$( … )` re-reads
/// bash's *re-print* of the body, which has no blank or continuation lines to
/// count, and the other two re-read the source, which does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubBody {
    /// `$( … )` — parsed in the enclosing token stream, and printed back from
    /// that parse.
    Eager,
    /// `$( … )` written in text that no parser ever read as a *word* — a
    /// here-document body, a `PS4`, a `${x@P}`. bash collected that text without
    /// calling `read_token_word` (a here-doc body comes from
    /// `read_secondary_line` straight into `make_here_document`,
    /// make_cmd.c:621), so `parse_matched_pair` never ran over it and
    /// `parse_comsub` — with it the re-print — never ran either. The body is
    /// found only when the text is *expanded*, by `extract_command_subst`, and
    /// what it hands on is the **source**.
    ///
    /// Same spelling as [`SubBody::Eager`], then, but the whole of its parse
    /// happens at expansion time, which is observable three ways: `declare -f`
    /// prints the body back as written rather than re-printed, a syntax error in
    /// it is a runtime `command substitution:` diagnostic rather than a parse
    /// error, and the enclosing script survives it.
    ///
    /// `closed` is false for a `$(` this scan never found a `)` for, which in
    /// unread text is not a failure of *this* scan either: `extract_command_subst`
    /// takes the body to be everything up to the end of the text and leaves the
    /// complaint to the expansion's own read. See
    /// [`crate::ast::CmdSubBody::Unread::closed`].
    Unread { closed: bool },
    /// `` ` … ` `` — parsed at expansion time. Carries the body *exactly as
    /// written*, backslashes and all: bash echoes a backtick body rather than
    /// re-printing it, and re-printing is not merely untidy — a nested `` \` ``
    /// would lose its backslash and the result would no longer parse.
    Backtick(Str),
    /// `$(( … )` — a `$((` whose body did not read as an arithmetic expression,
    /// so bash ran it as a command substitution instead.
    ///
    /// The fallback is `param_expand`'s (subst.c:10580): the `$((` scan only
    /// ever found the extent, and it is `chk_arithsub` at *expansion* time that
    /// asks whether the text is an expression at all. When it is not, bash
    /// hands that same text to `command_substitute` — the very call a backtick
    /// body makes — so the body is read then and not before, and there is no
    /// body text to print back other than the one that runs.
    ///
    /// Carries the nested substitutions the scan stepped over, for the same
    /// reason [`Seg::Arith`] does: the eager parse happens during the scan and
    /// so before the classification that chose this variant.
    ArithFallback(Vec<CmdSubSpan>),
}

/// A `$( … )` body an *arithmetic* scan stepped over, which bash parses in place
/// (parse.y:3937 → 3959) even though the text around it is an expression.
///
/// The parse's failure is not its only lasting effect. `parse_comsub` ends
/// `tcmd = print_comsub (parsed_command); … return ret` (parse.y:4219–4241), so
/// what it appends to the enclosing scan is the parse *re-printed*, and the
/// source text is thrown away. That is what a diagnostic quoting an arithmetic
/// string back shows, and what the body is read from when the expansion runs.
/// See [`Lexer::arith_comsubs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdSubSpan {
    /// The body, without the delimiters.
    pub src: Str,
    /// The line the body's own `)` sits on, for the same *parse-time*
    /// renumbering an eager `$( … )` gets — the one that puts a syntax error in
    /// the body on its true physical line. What the body reports when it *runs*
    /// comes from elsewhere; see [`crate::parser::parse_cmdsub_body`].
    pub close_line: u32,
    /// Where `src` sits in the text the enclosing scan built, *including* the
    /// `$(` and `)` around it — the range the re-print replaces. Offsets are
    /// into the buffer this span was collected for, so a scan that splices its
    /// buffer into a longer one shifts them as it does so.
    pub range: core::ops::Range<usize>,
    /// Whether a parser read the text this span was found in.
    ///
    /// [`SubBody::Eager`] is the ordinary case described above: bash parsed the
    /// body where it met it, so the parse's error is the enclosing unit's and
    /// its re-print replaces `range`.
    ///
    /// [`SubBody::Unread`] is the same `$( … )` written in text no parser ever
    /// read as a word — a here-document body, a `PS4`, a `${x@P}`. There was no
    /// eager parse, so no re-print and no parse-time error. It is recorded all
    /// the same, because the *expansion-time* scan has to be able to see it:
    /// `extract_delimited_string` carries `SX_COMMAND` and recurses into a
    /// `$( … )` with a real parse (subst.c:1431-1437), so a body that will not
    /// parse is reported from inside the arithmetic's extent read —
    /// `A$((1+$(fi)))B` under `${x@P}` names the *string's* remainder,
    /// `` `fi)))B' ``.
    ///
    /// One collection is all of one kind: the question is [`Lexer::here_text`],
    /// which nothing changes under a scan that is already running.
    pub kind: SubBody,
}

/// What [`Lexer::read_dollar_brace`] reads out of a `${ … }`: the body's raw
/// text, the `$( … )` spans the scan stepped over inside it, and the stretches
/// of that text the scan *wrote* rather than read — see [`Lexer::bare_splices`].
/// All three are scoped to the one body and measured against it.
type BracedBody = (Str, Vec<CmdSubSpan>, Vec<core::ops::Range<usize>>);

/// Move a collection of spans from the buffer they were gathered in to the
/// buffer that buffer is being spliced into, `by` bytes along.
///
/// A range that would run past `usize` is dropped rather than wrapped — it can
/// only come from a buffer longer than the address space, but the arithmetic
/// has to be total either way, and a missing span costs a re-print, not
/// correctness of the parse.
fn shift_spans(nested: Vec<CmdSubSpan>, by: usize) -> impl Iterator<Item = CmdSubSpan> {
    nested.into_iter().filter_map(move |s| {
        let start = s.range.start.checked_add(by)?;
        let end = s.range.end.checked_add(by)?;
        Some(CmdSubSpan { range: start..end, ..s })
    })
}

/// The same shift for a body's [`Lexer::bare_splices`], which are ranges into
/// the buffer they were collected for and so move with it.
fn shift_ranges(
    ranges: Vec<core::ops::Range<usize>>,
    by: usize,
) -> impl Iterator<Item = core::ops::Range<usize>> {
    ranges.into_iter().filter_map(move |r| {
        let start = r.start.checked_add(by)?;
        let end = r.end.checked_add(by)?;
        Some(start..end)
    })
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
    /// `${ … }` — raw inner text, parsed later, plus the 1-based source line the
    /// opening `${` sits on.
    ///
    /// The body is lexed again when it is parsed, and that second lex numbers
    /// its lines from 1 — so an error in it would be blamed on the body's own
    /// line rather than the script's. bash blames the physical line: with the
    /// `${` on line 3, `echo ${x:-$(\nfi\n)}` is named (and echoed) at line 4.
    /// The body's line 1 is the line the `${` opens on, so the two differ by a
    /// plain offset, which is what this field supplies.
    ///
    /// The third field holds the `$( … )` bodies met while the body was read.
    /// bash parses each of them *there*, as it reads the body, so their syntax
    /// errors beat every verdict the enclosing `${ … }` could reach — see
    /// [`Lexer::read_dollar_brace`] and the `ParamBraced` arm of the parser's
    /// `seg_to_part`.
    ///
    /// The fourth field is [`Lexer::bare_splices`]: the ranges of the body the
    /// scan wrote but never read. Two readers ask about them. The `}` the
    /// *expansion* stops at may lie inside one, which is
    /// [`crate::wordscan::expansion_body_len`]'s question; and a `$( … )` inside
    /// one was never parsed, which is the operand lexer's — see
    /// [`crate::parser::operand_from_source`].
    ParamBraced(Str, u32, Vec<CmdSubSpan>, Vec<core::ops::Range<usize>>),
    /// `$( … )` / `` ` … ` `` — raw inner source, parsed later, plus the 1-based
    /// source line of the substitution's *closing* delimiter.
    ///
    /// The parser needs that line to number the body *physically*, so that a
    /// syntax error the enclosing scan raises in it names the body's true line
    /// in the enclosing source rather than its own line 1 (see
    /// [`crate::parser::parse_cmdsub_body`]); only the lexer knows it. What the
    /// body reports when it *runs* is a different line entirely — the one the
    /// shell stands on at expansion time — and comes from the run, not from
    /// here.
    ///
    /// The third field says *when* bash reads the body, and so how it prints it
    /// back — see [`SubBody`].
    CmdSub(Str, u32, SubBody),
    /// `$(( … ))` — raw arithmetic expression text.
    /// The `bool` is `true` when the deprecated `$[ … ]` spelling was used. The
    /// two evaluate identically, but bash prints a stored function body back in
    /// whichever form the source wrote, so the distinction must survive here.
    ///
    /// The third field is the nested `$( … )` bodies the scan stepped over,
    /// which bash parses *there and then* rather than at expansion time — see
    /// [`Lexer::arith_comsubs`].
    Arith(Str, bool, Vec<CmdSubSpan>),
    /// `<( … )` / `>( … )` process substitution — the `bool` is `true` for the
    /// input form `<(…)`, the `String` is the raw inner command source, and the
    /// `u32` is the 1-based source line the `<(`/`>(` opens on.
    ///
    /// bash blames a syntax error in the body on the body's own line, counted in
    /// the enclosing source; the body is lexed on its own, so the parser needs
    /// the opening line to shift it back (`parser::parse_procsub_body`). The
    /// shift is from the *opening* delimiter, not the closing one: a process
    /// substitution runs as a child command rather than as a body bash re-reads
    /// after the enclosing scan.
    ProcSub(bool, Str, u32),
    /// A construct left open in text no parser read, which is not a lexing
    /// failure at all but a *runtime* one — see [`Unclosed`].
    ///
    /// Always the last segment of the run it appears in, because a construct
    /// only fails to close by running out of text. The segments before it stand
    /// and are expanded: bash walks the text left to right, so a whole `$( … )`
    /// written before the offending construct has already run by the time the
    /// scan gives up. Measured: `cat <<E` / `$(touch f) ${x:-a` / `E` reports
    /// the bad substitution *and* leaves `f` behind.
    Unclosed(Unclosed),
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
    /// `(( … ))` — an arithmetic command, holding the raw expression text and
    /// the `$( … )` bodies the scan that collected it stepped over.
    ///
    /// The bodies travel separately for the same reason [`Seg::Arith`]'s do: the
    /// text bash keeps is not the source but `print_comsub`'s re-print of each
    /// parsed body spliced back over it, and only the parser can produce that.
    /// See [`CmdSubSpan`].
    ArithCmd(Str, Vec<CmdSubSpan>),
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
    /// A construct the lexer refused with **no name to blame it on**, ending the
    /// token stream where it stands.
    ///
    /// bash's `read_token` returns −1 for one (parse.y:3531–3534), which bison
    /// reads as its EOF token while `EOF_Reached` stays clear. So
    /// `report_syntax_error` finds `current_token` non-zero but
    /// `error_token_from_token` with no branch for it, returning NULL
    /// (parse.y:6251), and falls through to its *text-scanning* branch: `syntax
    /// error near \`X'` — with `X` sliced out of the input around the offset the
    /// reader stopped at — and the offending line echoed under it (parse.y:6276).
    /// That is the whole difference from [`Tok::Invalid`], which carries a name
    /// and so gets `near unexpected token \`X'`.
    ///
    /// Only an arithmetic `for` header produces one: `for ((i=0;i<1;i++) )`
    /// fails `parse_arith_cmd`'s adjacency test inside the `ARITH_FOR_EXPRS` arm
    /// of `parse_dparen`, which — unlike the arithmetic *command* arm right
    /// below it — has no nested subshell to fall back to and simply returns −1
    /// (parse.y:4463–4478). It costs the whole input rather than just its own
    /// unit, because bison is holding EOF and there is nothing left to resume
    /// from; the lexer stops reading here for the same reason bash's reader
    /// never fetches another line.
    Refused,
}

/// The conditional's own brackets, whose classification the reserved-word fold
/// turns on: `[[` is `COND_START` and `]]` is `COND_END` in bash's
/// `word_token_alist`, and only the second of the two is acceptable after.
const COND_START: &[u8] = b"[[";
/// See [`COND_START`].
const COND_END: &[u8] = b"]]";

/// Every spelling bash will turn into a reserved word: its `word_token_alist`
/// (parse.y). Membership here decides only that the word *is* one — whether one
/// may stand here at all is [`CmdPos::reserved_ok`], and what it leaves behind
/// is [`RW_LEAVES_ACCEPTABLE`].
const RESERVED_WORDS: &[&[u8]] = &[
    b"if", b"then", b"else", b"elif", b"fi", b"case", b"esac", b"for", b"select", b"while",
    b"until", b"do", b"done", b"in", b"function", b"time", b"{", b"}", b"!", b"[[", b"]]",
    b"coproc",
];

/// The reserved words after which another one is still recognised: bash's
/// `reserved_word_acceptable` (parse.y:5367) intersected with the table above.
///
/// The ones bash leaves out are left out here too: after `in`, `case`, `for` or
/// `select` what follows is a pattern or a name. So `case x in ((p)` never
/// reaches `parse_dparen`'s second branch, and the `((` stays two `(` tokens —
/// of which a `case` arm may take only one, which is why bash reports a syntax
/// error at the second. `[[` is absent for the same reason and `]]` present —
/// see [`CmdPos::advance`].
///
/// `for` is the one that is *also* an arithmetic-command position, by a branch
/// of its own rather than by this list — see [`Prev::For`].
const RW_LEAVES_ACCEPTABLE: &[&[u8]] = &[
    b"{", b"}", b"!", b"do", b"done", b"elif", b"else", b"esac", b"fi", b"if", b"then", b"time",
    b"until", b"while", b"coproc", b"]]",
];

/// A word's text when it is a single unquoted literal segment, which is the
/// only shape bash will match against a reserved word or an operator spelling.
fn bare_word(segs: &[Seg]) -> Option<&[u8]> {
    match segs {
        [Seg::Lit(s)] => Some(s.as_slice()),
        _ => None,
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
    /// Not a shell option: this read is the **second** one bash gives a word.
    ///
    /// bash reads a word twice. The parser reads it to find where it ends, and
    /// `expand_word_internal` re-derives everything else from the word's source
    /// when the word is expanded — including the name of a `${ … }`, which
    /// `parameter_brace_expand` (subst.c:9539) carves out again with
    /// `string_extract` under `SX_VARNAME` (subst.c:795). That scan has a rule
    /// the parser has not: at a `[` it jumps to the matching `]` wherever it
    /// is, and the `}` the parser closed at may be inside the jump.
    ///
    /// osh parses once and expands the tree, so it gets that second read by
    /// re-parsing the word's source with this set — and only where the two
    /// reads disagree, which [`crate::wordscan`] decides. Off, the `[` is an
    /// ordinary character and a `${a[}` closes at its `}` as the parser's read
    /// requires.
    pub reread: bool,
    /// Not a shell option either: read this text with `expand_word_internal`'s
    /// **tolerance** for a construct that never closes.
    ///
    /// The parser refuses a word with an unterminated `'` or `"` because it is
    /// still looking for the end of the *word* and the input ran out. The
    /// expander is not looking for anything — it was handed a finished word —
    /// so `string_extract_single_quoted` (subst.c:1131) and
    /// `string_extract_double_quoted` (subst.c:963) simply stop at the end of
    /// it and the run they carved is whatever they got. Nothing complains.
    ///
    /// That only matters for text bash never read as source: the token buffer
    /// can hold characters the scan wrote into it rather than read out of it —
    /// the bare splice of a translated `$'…'` (parse.y:3887) — and the buffer
    /// can be cut short of its closing quote by a NUL the same splice carried
    /// (see [`crate::ast::WordPart::TokenText`]). Either way the expander meets
    /// a quote the parser never saw, and it is the expander's rules that
    /// answer for it.
    ///
    /// A `` ` `` and a `${` are *not* tolerated — each has its own runtime
    /// diagnostic rather than a silent run to the end — so those still fail the
    /// read. See [`crate::parser::word_tolerant_from_source_at`].
    pub tolerant: bool,
}

/// One `push_string` bash performed when a `(( … ))` failed the test for its
/// second closing parenthesis and was handed back to the ordinary grammar as
/// `( ( … ) )`.
///
/// `parse_arith_cmd` (parse.y:4519-4562) rebuilds the text it has just scanned —
/// with the *first* `(` dropped and the character that failed the test appended
/// — and hands it to the reader with `push_string (wval, 0, NULL)`. Reading it
/// back is not reading input: `line_number` is neither rewound before the push
/// nor advanced by the newlines the copy contains, so it stands at the line the
/// scan gave up on for the whole re-read.
#[derive(Clone, Copy, Debug)]
struct DparenPush {
    /// Offset of the copy's first character: the *second* `(`, one past the
    /// dropped one.
    start: usize,
    /// Offset of the copy's last character: the one the adjacency test read and
    /// rejected, which the rebuilt text ends with.
    end: usize,
    /// The line `line_number` was parked on when the scan gave up — the line
    /// every token inside the copy is blamed on.
    line: u32,
    /// Whether the rejected character was the last of its input line — a
    /// newline, or the end of input.
    ///
    /// When it was, the copy is exhausted with the reader's index parked on the
    /// buffer's terminating NUL, and `shell_getc`'s pop path
    /// (`uc = shell_input_line[shell_input_line_index]`, parse.y:2667-2669)
    /// hands that NUL straight back instead of fetching. read_token_word takes
    /// it for a word, so end of input is discovered *twice* — once ending that
    /// spurious empty word, once answering the request after it — and each
    /// discovery is its own `line_number++`. A real line answers the first for
    /// free; only at the end of input does the extra ask cost a line.
    eof_charge: bool,
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
    /// The `HereDoc` token this iteration emitted and the offset its *delimiter
    /// word* begins at, for [`Lexer::stamp_lines`] to stamp in place of the
    /// iteration's own start. See [`Lexer::lex_heredoc_op`].
    hd_delim: Option<(usize, u32)>,
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
    /// Character offsets of every `\` whose `\<newline>` this lexer *deleted* as
    /// a line continuation. Only the reader — [`crate::parser::IncrementalParser`],
    /// slicing a parse unit's source for the command history — has any use for
    /// these: bash's history stores the joined line, without the backslash or
    /// the newline, exactly where its own reader dropped them. A lexer run over
    /// a sub-string (a `${…}` replacement, a here-doc body scan) fills this in
    /// too, but its offsets are relative to that string and are simply dropped
    /// with the lexer.
    conts: Vec<u32>,
    /// The nested `$( … )` bodies the arithmetic scan currently running has
    /// stepped over, in the order it met them.
    ///
    /// bash does not step over one: under `P_ARITH` a `$(` goes to
    /// `parse_dollar_word` and from there to `parse_comsub` (parse.y:3937,
    /// 3959), a whole nested parse, so a body that will not parse is a **fatal
    /// syntax error in the enclosing unit** — raised while the enclosing line is
    /// still being read, long before any arithmetic is evaluated. Only its
    /// *text* is kept (`APPEND_NESTRET`); the parse's sole lasting effect is the
    /// error, and the body is read again when the expansion runs.
    ///
    /// The parse itself belongs to [`crate::parser`], which the lexer must not
    /// depend on, so the scan records the bodies here and the parser drains them
    /// out of the [`Seg`] — see `seg_to_part`. Both spellings of the result
    /// carry them, because the eager parse happens before the classification
    /// that tells them apart: `echo $(( echo $(fi) ) )` falls back to a command
    /// substitution *and* still dies on `fi`.
    ///
    /// Non-empty only inside a `$((`/`$[` scan. Every construct that starts a
    /// scan of its own — the two producers, and a nested `$( … )` whose body is
    /// commands — swaps this list out for the duration, so a body's own nested
    /// substitutions are parsed with that body and not a second time out here.
    arith_comsubs: Vec<CmdSubSpan>,
    /// Reader-level warnings raised during the scan, in the order they happened.
    /// The here-document-at-EOF ones are only filled in lenient mode (see
    /// `strict_heredoc_eof`), since strict mode raises instead. See
    /// [`ReaderWarning`].
    warnings: Vec<ReaderWarning>,
    /// Parallel to `warnings`: where the gather that raised each one was reading
    /// the body from. Filled by [`Lexer::warn`]; see [`Spanned::warn_from`].
    warn_from: Vec<u32>,
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
    /// Set once a [`Tok::Refused`] has been emitted, which ends the scan.
    ///
    /// bash reads a line at a time and hands each token to bison as it goes, so
    /// a token that makes bison error is the last one the reader is ever asked
    /// for — nothing after it is fetched, let alone lexed. Our scan reads the
    /// whole input up front, so the stop has to be explicit.
    refused: bool,
    /// Every `((` that failed its closing adjacency test and was handed back to
    /// the ordinary grammar, innermost last. See [`DparenPush`].
    dparens: Vec<DparenPush>,
    /// Set when the newline just consumed was a copy's last character, so the
    /// word that starts on the popped buffer's NUL is still owed. See
    /// [`DparenPush::eof_charge`] and [`Lexer::take_nul_word`].
    nul_word: bool,
    /// Which runs of `chars` are the real input and which are alias values
    /// spliced into it.
    ///
    /// A single `src == 0` run covering everything for every ordinary lex, where
    /// the text *is* the input. Only the alias pass's spliced text has more. See
    /// [`Lexer::raw`].
    map: TextMap,
    /// The **raw-input cursor**: where the next here-document body starts.
    ///
    /// bash has two cursors. The parser's (`shell_input_line` plus the pushed
    /// alias strings layered over it) is `pos`; the raw input's — `bash_input`,
    /// which both "fetch the next line" and `read_a_line` draw on — is this one.
    /// They coincide until an alias value is pushed, and again whenever a body is
    /// read from the middle of a line.
    ///
    /// The invariant is that `pos` never lies in the *real input* below `raw`:
    /// that span is input the body reader has already eaten, so the token scan
    /// skips it the moment it arrives (see [`Lexer::run_into`]). Inside an alias
    /// value `pos` is below `raw` all the time, and means nothing of the sort.
    raw: usize,
    /// Where the most recent here-document gather *started*, so that
    /// `raw_from .. raw` is the input the latest gather ate.
    ///
    /// Not the same as the alias value's end: bash has the whole calling line in
    /// `shell_input_line` before it expands anything on it, so a body taken from
    /// inside a value starts at the line *after* — and the rest of the calling
    /// line, which the reader still has, is parsed when the value runs dry.
    raw_from: usize,
    /// Every `raw_from .. raw` a gather has closed, in order. See
    /// [`Spanned::taken`].
    taken: Vec<(u32, u32)>,
    /// Where [`Lexer::read_dollar_brace_body`] spliced a translated `$'…'` into
    /// the body **unquoted** — bash's third row (parse.y:3887).
    ///
    /// The splice writes text into the body that this scan never read back, and
    /// two things follow from that. The body may reach past the `}` the
    /// *expansion* will close at, which is what
    /// [`crate::wordscan::expansion_body_len`] is asked about. And a `$( … )`
    /// the translation produced was never parsed, so it is
    /// [`crate::ast::CmdSubBody::Unread`] — met for the first time by whatever
    /// scans the stored word, exactly as one written inside a `' … '` run is.
    /// Telling those bytes from the ones the scan *did* read is why this is a
    /// list of ranges rather than a flag.
    ///
    /// Offsets are into the body being built by the scan that owns them, so a
    /// scan that splices its buffer into a longer one shifts them as it does so
    /// — the same rule [`CmdSubSpan::range`] follows, and the same
    /// save-and-restore around every construct that starts a buffer of its own.
    bare_splices: Vec<core::ops::Range<usize>>,
    /// The text this scan is reading is text bash **never read as a word** — a
    /// here-document body, a `PS4`, a `${x@P}` value.
    ///
    /// A here-document body is collected by `read_secondary_line` into
    /// `make_here_document` (make_cmd.c:621) as plain lines: no
    /// `read_token_word`, so no `parse_matched_pair`, so nothing in it was
    /// translated at parse time and no delimiter was pushed for it. A `$( … )`
    /// here is therefore parsed by `command_substitute` at expansion time, from
    /// a fresh reader — [`SubBody::Unread`] rather than [`SubBody::Eager`] — and
    /// the delimiter stack that reader starts from is *empty*, so its `${ … }`
    /// re-quotes. That is why the flag is cleared going into a substitution body
    /// — see [`Lexer::read_subst_body`] — and why `` cat <<E `` /
    /// `$(echo ${x:-$'a\tb'})` prints a tab rather than splitting on one.
    ///
    /// This is the *reader's* half of the question only. Whether a `$'…'` in the
    /// text survives as written is [`Lexer::ansi_c_quote`]'s, and the two come
    /// apart — see there.
    here_text: bool,
    /// `$'…'` and `$"…"` are **quote forms** in the text this scan is reading.
    ///
    /// Not the same question as [`Lexer::here_text`], and not derivable from it.
    /// `expand_word_internal` translates no `$'…'` at all — that is the reader's
    /// job, which is why bash carries a separate `expand_string_dollar_quote`
    /// "for code paths that don't do it" (subst.c:4171-4172). So the flag asks
    /// whether *a reader that translates* ever ran over this text, and there are
    /// exactly two such readers:
    ///
    /// - `read_token_word`/`parse_matched_pair`, i.e. the parse. Set for ordinary
    ///   source, clear for a here-document body, a `PS4`, a `${x@P}` and any
    ///   other string that reached the expansion as a *value*: `` cat <<E `` /
    ///   `${x:-$'a\tb'}` prints `$'a\tb'` back where the same word in a real
    ///   double-quoted string prints a tab.
    /// - `extract_heredoc_dolbrace_string`, which exists "to handle `$'...'` and
    ///   `$"..."` quoting in here-documents, since the here-document read path
    ///   doesn't" (subst.c:1522-1530). `parameter_brace_expand` re-extracts the
    ///   fragment after a `#`, `%`, `/`, `^`, `,` or substring `:` with
    ///   `SX_POSIXEXP` (subst.c:9913), and inside a here-document that extraction
    ///   is routed there (subst.c:1828-1832) — so the flag comes back *on* for a
    ///   pattern even though the body around it has it off. `:-`/`:+`/`:=`/`:?`
    ///   are not on that list, the `:` being consumed as the null-check before
    ///   the operator is read, so an operand keeps its text.
    ///
    /// Measured against bash 5.2.37, in a here-document body with `y=$'a\tb'`:
    /// `${nope:-$'a\tb'}` prints `$'a\tb'` while `${y#$'a\tb'}` prints nothing at
    /// all, the trim having matched. The second reader is a here-document's
    /// alone, so the same pattern in a `${x@P}` or in a runtime array subscript
    /// stays untranslated — `m[${v#$'a\tb'}]` with a real-tab `v` looks up the
    /// real-tab key, the trim having matched nothing.
    ansi_c_quote: bool,
    /// Where in a simple command the token loop stands, which is what decides
    /// whether a `[` at the head of a word opens a subscript, whether an alias is
    /// looked up, and whether a `((` opens an arithmetic command. See [`CmdPos`],
    /// [`Lexer::assignment_acceptable`] and [`Lexer::arith_cmd_position`].
    ///
    /// Distinct from `cond_depth`, which tracks a *lexing mode* and so counts
    /// every bare `[[`; [`CmdPos::cond`] holds only the `[[` bash would have
    /// classified as `COND_START`.
    cmd_pos: CmdPos,
    /// How many of the output tokens [`Lexer::cmd_pos`] has been advanced over.
    /// The scan pushes tokens from a dozen places, so the state is brought up to
    /// date lazily at the places that read it rather than at every push.
    cmd_pos_upto: usize,
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

    /// Renumber every line this warning names, for a caller whose lex ran over a
    /// text that is not the script — an alias pass's assembled input, whose
    /// replacement values add lines the script does not have.
    pub fn map_lines(&mut self, f: impl Fn(u32) -> u32) {
        match self {
            Self::HeredocEof(h) => {
                h.body_line = f(h.body_line);
                h.eof_line = f(h.eof_line);
            }
            Self::SubstHeredoc(s) => s.line = f(s.line),
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
    /// Character offset of the `<<` operator itself. The placeholder's own
    /// offset is the *delimiter*'s (see [`Lexer::lex_heredoc_op`]), and what
    /// [`UngatheredHeredoc::op_offset`] wants is the text in front of the
    /// operator, so it is kept separately rather than read back off the token.
    op_at: u32,
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

/// How far along an assignment's head the word being scanned has got — bash's
/// `assignment` (general.c), run over the token buffer as it stands:
///
/// ```c
/// if (legal_variable_starter (c) == 0)  return (0);
/// while (c = string[indx])
///   {
///     if (c == '=')  return (indx);           /* `=' at index 0 is not one */
///     if (c == '[')
///       {
///         newi = skipsubscript (string, indx, 0);
///         if (string[newi++] != ']')  return (0);
///         if (string[newi] == '+' && string[newi+1] == '=')  return (newi + 1);
///         return ((string[newi] == '=') ? newi : 0);
///       }
///     if (c == '+' && string[indx+1] == '=')  return (indx + 1);
///     if (legal_variable_char (c) == 0)  return (0);
///     indx++;
///   }
/// ```
///
/// It reads the buffer bash keeps, which holds the source *as written* — quotes
/// and all. So a quoted character in the **name** spoils the assignment
/// (`"h"=v` starts on a `"`, which is no variable starter) while one in the
/// **value** cannot (`h="v"` has already returned at the `=`). That asymmetry is
/// the whole reason this is a state machine over the characters rather than a
/// test on the finished word: [`CaseScan`] keeps only the unquoted ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignHead {
    /// Reading the name. Empty so far when [`CaseScan::word`] is empty, which is
    /// where a `=` is just a character.
    Name,
    /// Inside the `[ … ]`, at this bracket depth.
    Sub(usize),
    /// Just past the `]`, where only `=` or `+=` may follow.
    SubEnd,
    /// A `+` that is an assignment only if a `=` follows it.
    Plus,
    /// The `=` has been read: an assignment whatever comes after.
    Yes,
    /// Not one, and nothing later can make it one.
    No,
}

impl AssignHead {
    /// One unquoted character of the word.
    fn push(self, c: char, first: bool) -> Self {
        match self {
            Self::Name if first => {
                if is_name_start(c) {
                    Self::Name
                } else {
                    Self::No
                }
            }
            Self::Name => match c {
                '=' => Self::Yes,
                '[' => Self::Sub(1),
                '+' => Self::Plus,
                _ if is_name_char(c) => Self::Name,
                _ => Self::No,
            },
            Self::Sub(d) => match c {
                '[' => Self::Sub(d + 1),
                ']' if d == 1 => Self::SubEnd,
                ']' => Self::Sub(d - 1),
                _ => Self::Sub(d),
            },
            Self::SubEnd => match c {
                '=' => Self::Yes,
                '+' => Self::Plus,
                _ => Self::No,
            },
            Self::Plus => {
                if c == '=' {
                    Self::Yes
                } else {
                    Self::No
                }
            }
            Self::Yes => Self::Yes,
            Self::No => Self::No,
        }
    }

    /// A quoted or substituted span. Content of the value or of the subscript,
    /// which the scan is past caring about; anywhere else it is a character that
    /// is not the one the head needed.
    const fn push_quoted(self) -> Self {
        match self {
            Self::Sub(d) => Self::Sub(d),
            Self::Yes => Self::Yes,
            _ => Self::No,
        }
    }

    /// Whether the word so far is exactly a name — bash's `token_is_ident`,
    /// which is where a `[` opens a subscript rather than being text.
    const fn is_name(self, empty: bool) -> bool {
        matches!(self, Self::Name) && !empty
    }
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
/// must keep its `)` — so this tracks the word being read and where a command
/// begins. Quoting is what makes `"esac"` a plain word, so a word with any
/// quoted character in it is never reserved.
///
/// The position is the same [`CmdPos`] the token stream is folded through, and
/// not a second model of it: this scan reads characters rather than tokens, so
/// [`Self::feed`] is where a character run becomes the one [`Ev`] it stands for.
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
    /// Where a command begins, and what a word standing here may be.
    pos: CmdPos,
    /// Characters of a multi-character operator still to step over. An operator
    /// is one token however it is spelled, so [`Self::feed`] recognises the whole
    /// of it at its first character and then swallows the rest.
    skip: u8,
    /// How far the word being read has got along an assignment's head, which is
    /// what makes the *next* word a command word as well.
    head: AssignHead,
}

impl CaseScan {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            word: Str::new(),
            word_pure: true,
            word_pat_start: false,
            pos: CmdPos::new(),
            skip: 0,
            head: AssignHead::Name,
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
        self.head = self.head.push(c, self.word.is_empty() && self.word_pure);
        push1(&mut self.word, c);
        self.pattern_seen();
    }

    /// A quoted or substituted span, which is part of the word being read but
    /// cannot be part of a reserved one.
    fn push_quoted(&mut self) {
        self.begin_word();
        self.head = self.head.push_quoted();
        self.word_pure = false;
        self.pattern_seen();
    }

    /// Whether a `[` here opens an array subscript, which the scan must then read
    /// to its `]` however many `)` or newlines stand in the way — bash's
    /// (parse.y:5145-5146):
    ///
    /// ```c
    /// else if MBTEST(character == '[' &&      /* ] */
    ///                ((token_index > 0 && assignment_acceptable (last_read_token) &&
    ///                  token_is_ident (token, token_index)) || …
    /// ```
    ///
    /// `assignment_acceptable` is `command_token_position` minus a `case`
    /// pattern, and a pattern is already out of command position by that macro's
    /// third clause — so [`CmdPos::at_command`] answers both. `token_is_ident` is
    /// [`AssignHead::is_name`].
    fn at_subscript(&self) -> bool {
        self.head.is_name(self.word.is_empty()) && self.pos.at_command()
    }

    /// Note that the current pattern is no longer empty, so a `(` from here on
    /// is a group and not the pattern's optional open.
    fn pattern_seen(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            f.pat_start = false;
        }
    }

    /// The word being read has ended, `delim` being the character that ended it.
    /// `depth` is the scan's current parenthesis depth, which a `case` found here
    /// is recorded at.
    fn finish_word(&mut self, depth: usize, delim: Option<char>) {
        if self.between_words() {
            return;
        }
        let pure = self.word_pure;
        let word = core::mem::take(&mut self.word);
        self.word_pure = true;
        let assign = self.head == AssignHead::Yes;
        self.head = AssignHead::Name;
        let pat_first = self.word_pat_start;
        self.word_pat_start = false;
        // Only an unquoted single literal can be a reserved word — the same
        // shape `bare_word` picks out of a token's segments.
        let lit = if pure { Some(word.as_slice()) } else { None };
        // An IO number (`2>f`) or a `{name}` descriptor variable (`{v}>f`), which
        // `read_token_word` returns as its own token rather than as a WORD. It
        // belongs to the redirection that follows, so it is not the first *word*
        // of the command and must not end a leading run of redirections; the
        // operator behind it speaks for the whole thing.
        if matches!(delim, Some('<' | '>')) && lit.is_some_and(is_redir_prefix) {
            return;
        }
        // Sampled before the word is folded in: `case` and `esac` are reserved
        // words only where one was already acceptable, and an assignment is one
        // only where an assignment could have stood.
        let cmd_pos = self.pos.reserved_ok();
        let was_cmd = self.pos.at_command();
        self.pos.word(lit, assign, was_cmd);
        let word = lit.unwrap_or_default();
        if let Some(f) = self.frames.last_mut() {
            match f.phase {
                CasePhase::AwaitIn if word == b"in" => {
                    f.phase = CasePhase::Pattern;
                    f.pat_start = true;
                    return;
                }
                // `esac` is reserved wherever a pattern could start, which is why
                // `case esac in …` is a syntax error in bash rather than a match
                // against the word `esac`. An empty `case x in esac` ends here.
                CasePhase::Pattern if pat_first && word == b"esac" => {
                    self.frames.pop();
                    return;
                }
                CasePhase::Body if cmd_pos && word == b"esac" => {
                    self.frames.pop();
                    return;
                }
                _ => {}
            }
        }
        if word == b"case" && cmd_pos {
            self.frames.push(CaseFrame {
                depth,
                phase: CasePhase::AwaitIn,
                pat_start: false,
            });
        }
    }

    /// A `;;`, `;&` or `;;&`: the clause body ends and the next pattern list
    /// begins.
    fn arm_end(&mut self) {
        if let Some(f) = self.frames.last_mut()
            && f.phase == CasePhase::Body
        {
            f.phase = CasePhase::Pattern;
            f.pat_start = true;
        }
    }

    /// One character of the body, with the two that follow it, at parenthesis
    /// depth `depth`.
    ///
    /// Every delimiter ends the word being read and stands for one token of the
    /// fold. An operator spelled with more than one character is recognised
    /// whole here — `&&` is not two `&`, and `&>` is a redirection rather than a
    /// background `&` — and [`Self::skip`] then steps over its tail so it counts
    /// once. The tails are only ever `>`, `&`, `|` or `;`, none of which the
    /// caller handles before this, so nothing can slip past a pending skip.
    fn feed(&mut self, c: char, n1: Option<char>, n2: Option<char>, depth: usize) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        if !matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')') {
            self.push(c);
            return;
        }
        self.finish_word(depth, Some(c));
        let ev = match c {
            ' ' | '\t' => return,
            '\n' => Ev::Newline,
            ';' => {
                let arm = match (n1, n2) {
                    (Some(';'), Some('&')) => Some(2),
                    (Some(';' | '&'), _) => Some(1),
                    _ => None,
                };
                if let Some(skip) = arm {
                    self.skip = skip;
                    self.arm_end();
                    Ev::CaseArmEnd
                } else {
                    Ev::Sep
                }
            }
            // `&>` and `&>>` are redirections; `&&` is a separator like a lone
            // `&`, but two characters wide.
            '&' => match (n1, n2) {
                (Some('>'), Some('>')) => {
                    self.skip = 2;
                    Ev::RedirOp
                }
                (Some('>'), _) => {
                    self.skip = 1;
                    Ev::RedirOp
                }
                (Some('&'), _) => {
                    self.skip = 1;
                    Ev::Sep
                }
                _ => Ev::Sep,
            },
            '|' => match n1 {
                Some('|') => {
                    self.skip = 1;
                    Ev::Sep
                }
                Some('&') => {
                    self.skip = 1;
                    Ev::Pipe
                }
                _ => Ev::Pipe,
            },
            // `<<` and `<<<` never arrive here: the here-document arm of
            // `read_balanced_body` consumes them and announces them itself.
            '<' | '>' => {
                if matches!((c, n1), ('<', Some('&' | '>')) | ('>', Some('>' | '&' | '|'))) {
                    self.skip = 1;
                }
                Ev::RedirOp
            }
            // Both a group's `(`/`)` and a `case` pattern's are LPAREN/RPAREN to
            // bash, and both are in `reserved_word_acceptable`.
            _ => Ev::Sep,
        };
        self.pos.ev(ev);
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
            hd_delim: None,
            cond_depth: 0,
            regex_next: false,
            opts,
            extpat_next: false,
            strict_heredoc_eof: false,
            paren_body: false,
            conts: Vec::new(),
            arith_comsubs: Vec::new(),
            warnings: Vec::new(),
            warn_from: Vec::new(),
            hd_ahead: None,
            next_tok_index: 0,
            heredoc_lines: Vec::new(),
            heredocs_forgotten: false,
            refused: false,
            dparens: Vec::new(),
            nul_word: false,
            map: TextMap::whole(0),
            raw: 0,
            raw_from: 0,
            taken: Vec::new(),
            bare_splices: Vec::new(),
            here_text: false,
            ansi_c_quote: true,
            cmd_pos: CmdPos::new(),
            cmd_pos_upto: 0,
        }
    }

    /// Point this scan at text that arrived from outside the parse. See
    /// [`ReadCtx`].
    fn apply_ctx(&mut self, ctx: ReadCtx) {
        self.here_text = ctx.unread;
        self.ansi_c_quote = ctx.ansi_c;
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

    /// As [`Lexer::new`], but over a text bash's reader *assembled*: the input
    /// with alias values bash pushed onto it with `push_string` written in where
    /// their alias words stood. `map` says which run is which. See
    /// [`Lexer::raw`].
    ///
    /// Reading starts at `from`, which must be the first character of a physical
    /// line: bash's reader holds one line at a time, and everything a splice has
    /// to settle — which `<<` on this line is still owed a body, and in what
    /// order they are owed — is settled within the line the alias word stands
    /// on. Starting there rather than at the top of the script also keeps the
    /// re-lex off text the caller has already run, which may no longer *be*
    /// lexable as code (a here-document body it consumed is arbitrary text).
    ///
    /// The raw-input cursor is *not* moved: it stays at 0, meaning "has not
    /// diverged", until the first gather sets it — so the bodies of this line's
    /// own here-documents are gathered afresh, from the line after it, in
    /// `redir_stack` order. See [`Spanned::raws`].
    fn spliced(chars: Vec<Ch>, map: TextMap, from: usize, opts: ParseOpts) -> Self {
        let mut lx = Self { chars, map, ..Self::new(b"", opts) };
        let from = from.min(lx.chars.len());
        lx.pos = from;
        lx.iter_start = from;
        // `cur_line` counts the newlines since `iter_start`, so the base has to
        // be the physical line the cursor already stands on.
        let before = lx.chars.get(..from).unwrap_or(&[]).iter().filter(|&&c| c == '\n').count();
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

/// What a scan should assume about text handed to it from outside the parse:
/// whether any parser *read* it, and whether any reader *translated* it.
///
/// The two are independent — see [`Lexer::here_text`] and
/// [`Lexer::ansi_c_quote`] — so they travel together rather than as one flag.
/// Only three of the four combinations arise, and each has a constructor below;
/// there is no read-but-untranslated text, because reading is what translates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadCtx {
    /// No parser read this text as a word, so a `$( … )` in it has no parse yet
    /// and is read at expansion time. [`Lexer::here_text`].
    pub unread: bool,
    /// `$'…'` and `$"…"` are quote forms here. [`Lexer::ansi_c_quote`].
    pub ansi_c: bool,
}

impl ReadCtx {
    /// Ordinary source, read by the parse and translated by it.
    pub const SOURCE: Self = Self { unread: false, ansi_c: true };
    /// A **pattern** (or replacement, subscript, substring bound) of a `${ … }`
    /// written in a here-document body: never parsed, but re-extracted by
    /// `extract_heredoc_dolbrace_string`, which translates it. See
    /// [`Lexer::ansi_c_quote`].
    pub const HEREDOC_FRAGMENT: Self = Self { unread: true, ansi_c: true };
    /// A string that reached the expansion as a **value** — a here-document
    /// body, a `PS4`, a `${x@P}`, a runtime array subscript. No reader ran over
    /// it at all, at any depth.
    pub const VALUE: Self = Self { unread: true, ansi_c: false };
}

/// Tokenize `src` into a token stream.
///
/// `ctx` says where `src` came from — real source for every caller that is
/// reading a script, and [`ReadCtx::HEREDOC_FRAGMENT`] or [`ReadCtx::VALUE`] for
/// text that arrived as a value, such as the substring bounds of a
/// `${x:off:len}` written in a here-document body.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn tokenize(src: BStr<'_>, opts: ParseOpts, ctx: ReadCtx) -> Result<Vec<Tok>, LexError> {
    let mut lx = Lexer::new(src, opts);
    lx.apply_ctx(ctx);
    lx.run().map(|s| s.toks)
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
    ///
    /// One token is stamped exactly rather than conservatively: the `HereDoc`
    /// standing for a `<<`'s delimiter carries that *word*'s offset, not the
    /// operator's, because the alias pass has to be able to replace the
    /// delimiter alone and to see the pop that happens between the two. See
    /// [`Lexer::lex_heredoc_op`].
    pub starts: Vec<u32>,
    /// Parallel to `toks`: the character offset into `src` just past each
    /// token's last character, as [`Tokenized::ends`]. A syntax error needs it
    /// because bash names the error site by slicing its *input line*, not by
    /// printing the token — see [`crate::parser`]'s `Spans`.
    pub ends: Vec<u32>,
    /// Parallel to `toks`: where the **raw-input cursor** stood when each token
    /// was read — the offset the next here-document body would be taken from.
    /// See [`Lexer::raw`].
    ///
    /// `0` throughout an ordinary lex, which never diverts the cursor. Only the
    /// alias pass reads it, to learn which input line bash's reader had reached
    /// while it was standing inside a value: `line_number` is bumped by the
    /// gather, so a token after one is numbered from the delimiter's line and
    /// not from the alias word's.
    pub raws: Vec<u32>,
    /// The spans of text this lex ate as here-document **bodies**, ascending and
    /// disjoint — the lines it read as data rather than as commands.
    ///
    /// A caller that lexed the same text before under a different reading (the
    /// alias pass, whose splice can put a `<<` where the first lex saw a plain
    /// word) needs it to learn which of that first reading's tokens are not
    /// tokens after all. See [`crate::parser::IncrementalParser::rebuild`].
    pub taken: Vec<(u32, u32)>,
    /// What the reader complained about while fetching those bodies. The lines
    /// are this text's own, so a caller that *assembled* the text has to
    /// renumber them onto the script. See [`ReaderWarning`].
    pub warnings: Vec<ReaderWarning>,
    /// Parallel to `warnings`: the offset into this text the gather that raised
    /// each one had reached when it began reading the body it was complaining
    /// about — a point inside the matching [`Self::taken`] span.
    ///
    /// A warning's token index says nothing about *where* its body was: the
    /// `<<` is stamped when it is read, long before the end of the line sends
    /// the reader off to fetch anything. Only a caller that has to place the
    /// body in the text wants this; see [`AliasExpansion::warnings`].
    pub warn_from: Vec<u32>,
    /// Every `((` this lex handed back to the ordinary grammar, outermost first.
    /// See [`DparenCopy`].
    pub dparens: Vec<DparenCopy>,
}

/// The extent of the text bash re-read after a `((` failed the test for its
/// second closing parenthesis: `parse_arith_cmd` rebuilds what it scanned and
/// `push_string`s the copy (parse.y:4498), so what the reader is working from
/// there is a *string*, not the script.
///
/// Two things follow, and neither can be had from the physical text alone: a
/// diagnostic echoes `shell_input_line`, which is now the whole copy however
/// many physical lines it spans; and no line is fetched while it is being read,
/// so `line_number` stands still — see [`DparenPush`], which is this plus the
/// line it stood at.
///
/// Offsets are into the text that was lexed. `start` is the copy's first
/// character (the *second* `(`, the first having been dropped from the rebuild)
/// and `end` its last (the character the test rejected, which the rebuild
/// appends), so the copy is `src[start ..= end]`.
#[derive(Clone, Copy, Debug)]
pub struct DparenCopy {
    pub start: u32,
    pub end: u32,
}

impl DparenCopy {
    /// Where a token whose text ends at `at` sits inside this copy, if it does.
    /// `at` is one past the token's last character, so it runs from `start + 1`
    /// (the copy's first character consumed) to `end + 1` (its last).
    #[must_use]
    fn offset_of(self, at: u32) -> Option<u32> {
        (at > self.start && at <= self.end.saturating_add(1)).then(|| at.saturating_sub(self.start))
    }
}

/// Which of `copies` owns the offset `at`, innermost first.
///
/// A `((` met while re-reading another's copy pushes its own on top of it
/// (bash's `pushed_string_list` is a stack), and the innermost is the one the
/// reader is in — so the *last* recorded copy containing the offset wins,
/// the list being in push order.
pub(crate) fn dparen_at(copies: &[DparenCopy], at: u32) -> Option<(usize, u32)> {
    copies
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, c)| c.offset_of(at).map(|off| (i, off)))
}

/// The per-token parallel vectors a lex fills beside the token stream, carried
/// together so the one pass that stamps them all takes one argument.
#[derive(Default)]
struct Marks {
    lines: Vec<u32>,
    starts: Vec<u32>,
    ends: Vec<u32>,
    raws: Vec<u32>,
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

/// Where a `$( … )` ends, for a caller that met one while *expanding* a word
/// rather than while parsing it. `src` is the text just past the `$(`; the
/// answer is the body and the character offset one past its `)`, or `None` when
/// the text runs out with the substitution still open.
///
/// The one caller is [`crate::interp::Shell`]'s expansion of an arithmetic
/// string, which is the only text osh reads a `$( … )` out of after the parse.
/// bash reads that one with `extract_command_subst` (subst.c:1290), which —
/// unless what follows the `$(` is another `(` — hands the rest of the string
/// to `xparse_dolparen` (parse.y:4248) and lets the **parser** say where the
/// substitution ends. So the answer is not "the matching `)`": in
/// `$(( '$(case x in x) echo 7;; esac)' + 0 ))` the first unmatched `)` closes
/// a `case` pattern, and bash's substitution runs to the one after `esac`.
///
/// This is the same scan the lexer uses for a `$( … )` written in source, and
/// it tracks `case` for exactly that reason — so pointing the expansion at it
/// is what makes the two agree, rather than a second rule for the same text.
#[must_use]
pub fn scan_cmdsub_body(src: BStr<'_>, opts: ParseOpts) -> Option<(Str, usize)> {
    let mut lx = Lexer::new(src, opts);
    // A fresh read of a string the expansion handed over, so bash's delimiter
    // stack is empty for it: `xparse_dolparen` runs its own parser.
    let body = lx.read_subst_body(false).ok()?;
    Some((body, lx.pos))
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
    /// `Some((error, line))` when lexing stopped early. The tokens the failing
    /// line did yield are kept: bash's parser pulls tokens one at a time, so it
    /// has already *seen* them, and a grammar error among them is raised before
    /// the unclosed construct is ever reached — `) echo "` reports the stray
    /// `)`, not the quote. What stops at the line boundary is *execution*, and
    /// that is [`crate::parser::IncrementalParser::next_unit`]'s business: it
    /// reports the parked error in place of any result once the stream runs
    /// dry, so in `echo two; echo three 'unterm` nothing on that line runs
    /// while every earlier line already has.
    pub err: Option<(LexError, u32)>,
    /// The lowest line an *end of input* diagnostic may be reported on, or 0
    /// when nothing constrains it.
    ///
    /// A `((` handed back to the grammar as `( ( … ) )` buys the reader one
    /// extra empty request when its copy ended on the last character of a line
    /// — see [`DparenPush::eof_charge`]. The request is free wherever a real
    /// line answers it, so the charge shows only as a floor under the end of
    /// input: the scan's last line, plus the line the fetch after it would have
    /// been, plus the extra one.
    pub dparen_eof_floor: u32,
    /// Every `((` this lex handed back to the ordinary grammar, outermost first.
    /// See [`DparenCopy`].
    pub dparens: Vec<DparenCopy>,
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
    let mut marks = Marks::default();
    let res = lx.run_into(&mut toks, &mut marks);
    let Marks { mut lines, starts: mut offsets, mut ends, .. } = marks;
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
    let dparen_eof_floor = lx
        .dparens
        .iter()
        .filter(|p| p.eof_charge)
        .map(|p| p.line.saturating_add(2))
        .max()
        .unwrap_or(0);
    let dparens = lx.dparen_copies();
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
            dparen_eof_floor,
            dparens,
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
                op_offset: h.op_at,
            })
            .collect()
    };
    // The failing token's own line is the fallback when the raise site did not
    // name one. `Lexer::line` only advances at the end of each `run_into`
    // iteration, so mid-token it still holds the line that token started on.
    let line = e.line.unwrap_or(lx.line);
    // Everything the scan managed to produce is kept, the failing line's own
    // tokens included — bash's parser has already been handed them, one at a
    // time, by the time the reader chokes on what follows. The one exception is
    // a here-document still awaiting its body: its `<<` left a placeholder token
    // that was never filled in, so that token must not reach the parser, and the
    // line it stands on is cut back to along with it.
    let keep = match lx.pending_heredocs.iter().map(|h| h.tok_index).min() {
        None => toks.len(),
        Some(limit) => toks
            .get(..limit)
            .unwrap_or(&toks)
            .iter()
            .rposition(|t| matches!(t, Tok::Newline))
            .map_or(0, |i| i.saturating_add(1)),
    };
    toks.truncate(keep);
    lines.truncate(keep);
    offsets.truncate(keep);
    ends.truncate(keep);
    heredoc_lines.truncate(keep);
    // The continuations are keyed by source offset rather than by token index, so
    // any past a cut simply describe text no caller will slice; leaving them
    // costs nothing and keeps the list a faithful record of the whole scan.
    // A here-document that *did* reach EOF can coexist with a deferred lexer
    // error — an unterminated one inside a `$( … )` is warned about by the scan
    // that then fails to find the `)` — and its warning is released by the
    // reader, which lifts its `tok_index` gate entirely on the unit reporting a
    // parked lexer error. That gate is what keeps bash's silence after
    // `echo one )`: input abandoned on a syntax error is input never read.
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
        dparen_eof_floor,
        dparens,
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
    lex_word_verbatim_opts(src, ParseOpts::default(), ReadCtx::SOURCE)
}

/// [`lex_word_verbatim`] with the read's own options — which for this scan is
/// only ever [`ParseOpts::reread`], the flag that turns a word's source into
/// the *second* read bash gives it. See [`crate::wordscan`].
///
/// `ctx` says where the `${ … }` this fragment was written in came from. The
/// *read* half applies to a pattern exactly as it does to an operand — nothing
/// gives unread text a parse it never had, see [`Lexer::here_text`] — but the
/// *translation* half does not. A here-document's pattern is re-extracted by
/// `extract_heredoc_dolbrace_string`, which puts back the ANSI-C translation the
/// body never got (subst.c:1828-1832), so callers pass
/// [`ReadCtx::HEREDOC_FRAGMENT`] there: measured in a here-document body with `v`
/// holding a real tab, `${nope:-$'a\tb'}` prints the `$'…'` back while
/// `${v#$'a\tb'}` trims it away. That second reader is a here-document's alone —
/// a runtime array subscript gets [`ReadCtx::VALUE`], and its pattern stays
/// untranslated.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn lex_word_verbatim_opts(
    src: BStr<'_>,
    opts: ParseOpts,
    ctx: ReadCtx,
) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, opts);
    lx.apply_ctx(ctx);
    lx.read_word_verbatim(Verbatim::Bare, &[])
}

/// Lex `src` as if it were the body of a double-quoted string that runs to the
/// end of the input — for the strings bash expands in `Q_DOUBLE_QUOTES` context
/// without any quotes actually delimiting them (`PS4` before each `set -x`
/// line, and `${x@P}`). `$…`, `` `…` `` and the double-quote backslash escapes
/// are live; a `"`, a `'` and any other backslash are literal.
///
/// Nothing in it was translated by a parser either — the string reached the
/// expansion as a *value*, not as source `read_token_word` had read — so the
/// scan runs with [`Lexer::here_text`] set, and a `$'…'` inside a `${ … }` here
/// stays as written. Measured: `v='${y:-$'\''a\x2Cb'\''}'` makes `"${v@P}"`
/// print `$'a\x2Cb'` back.
///
/// # Errors
/// Returns [`LexError`] on an unterminated substitution.
pub fn lex_dquote_body(src: BStr<'_>) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.apply_ctx(ReadCtx::VALUE);
    lx.read_double_quote_until(false)
}

/// Lex the *replacement* of `${var/pat/repl}` verbatim, like
/// [`lex_word_verbatim`] but preserving a literal backslash before `&` or `\`
/// (`\&` and `\\`) so the replacement's `&`-scan can later distinguish an
/// escaped ampersand (a literal `&`) from an active one (the matched text).
/// Every other backslash escape is still consumed at lex time (`\n` → `n`),
/// matching bash's replacement quote-removal.
///
/// `ctx` is [`lex_word_verbatim_opts`]'s: where the `${ … }` around this
/// replacement came from.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn lex_replacement_verbatim(src: BStr<'_>, ctx: ReadCtx) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.apply_ctx(ctx);
    lx.read_word_verbatim(Verbatim::Replacement, &[])
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
/// `ctx` says where the text this operand was written in came from — real
/// source, or a value such as a here-document body, a `PS4` or a `${x@P}`, in
/// which case that last sentence stops applying because there is no read: see
/// [`Lexer::here_text`]. It is the caller's to know, since the same operand
/// spelling reaches here from both (`crate::parser::Quoting::Dquote` and
/// `Quoting::Unread`). An operand is never a pattern, so it never gets a
/// here-document fragment's re-extraction and its `$'…'` stays as written.
///
/// `unread` gives the stretches of `src` — as **byte** ranges — that the brace
/// scan *spliced* in rather than read: a bare `$'…'` translation, bash's third
/// row (parse.y:3887). No parser passed over those bytes, so a `$( … )` in one
/// is [`crate::ast::CmdSubBody::Unread`] just as a `' … '` run's is. See
/// [`Lexer::bare_splices`], which is where they are collected, and
/// [`Lexer::read_word_verbatim`], which consumes them.
///
/// # Errors
/// Returns [`LexError`] on an unterminated quote or substitution.
pub fn lex_operand_in_dquote(
    src: BStr<'_>,
    ctx: ReadCtx,
    unread: &[core::ops::Range<usize>],
) -> Result<Vec<Seg>, LexError> {
    let mut lx = Lexer::new(src, ParseOpts::default());
    lx.apply_ctx(ctx);
    // The scan counts in characters and the ranges arrive in bytes, so map one
    // to the other over the very decoding the scan will do. A splice writes
    // whole characters, so both ends land on a boundary; an end past the last
    // character is the text's own end.
    let unread: Vec<core::ops::Range<usize>> = if unread.is_empty() {
        Vec::new()
    } else {
        let offs: Vec<usize> = bytes::char_positions(src).map(|(o, _)| o).collect();
        let at = |b: usize| offs.partition_point(|&o| o < b);
        unread.iter().map(|r| at(r.start)..at(r.end)).collect()
    };
    lx.read_word_verbatim(Verbatim::Dquote, &unread)
}

/// What the previously read token was, to the precision the command-position
/// tests need. This is osh's reading of bash's `last_read_token`, which is a
/// *parser* token — so a word's classification depends on where it sat, and the
/// state has to be carried rather than read back off the output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prev {
    /// Nothing yet, a separator, or a reserved word after which a command
    /// begins.
    Start,
    /// `;;`, `;&` or `;;&`. A reserved word would be accepted here, but what
    /// actually follows is the *next `case` arm's pattern* — which is why bash
    /// excludes these three from `command_token_position` alone.
    CaseArmEnd,
    /// bash's TIME: the `time` reserved word.
    ///
    /// It and its two options are all in `reserved_word_acceptable`, so a
    /// command — and so an assignment — still begins after any of them
    /// (`time_command_acceptable`, parse.y:3140-3153):
    ///
    /// ```c
    ///     case BANG:      /* ! time pipeline */
    ///     case TIME:      /* time time pipeline */
    ///     case TIMEOPT:   /* time -p time pipeline */
    ///     case TIMEIGN:   /* time -p -- ... */
    ///       return 1;
    /// ```
    ///
    /// `time` itself is gated on `time_command_acceptable`, which is *narrower*
    /// than `reserved_word_acceptable`: it does not list `|`. See [`AfterPipe`].
    /// The rest of what it leaves out (`;;`, `fi`, `done`, `esac`, `}`) cannot
    /// be followed by a word without a syntax error anyway.
    Time,
    /// bash's TIMEOPT: a `-p` directly after a [`Prev::Time`], and nowhere else
    /// (`special_case_tokens`, parse.y:3292-3302). `--` after a TIME or a
    /// TIMEOPT is TIMEIGN, which needs no state of its own: nothing after *it*
    /// is special, so it is only a [`Prev::Start`]. Keeping the two apart is
    /// what makes `time -p -- x` reach `x` in command position while
    /// `time -- -p x` does not.
    TimeOpt,
    /// bash's FUNCTION. The *name* after one is acceptable even though it is a
    /// plain WORD (parse.y:5406-5412), which is what lets `function f ((1))`
    /// reach an arithmetic command through a word that is not reserved.
    Function,
    /// bash's COPROC, whose name is acceptable for the same reason.
    Coproc,
    /// bash's FOR. Not in `reserved_word_acceptable` — `for f[1` is no command
    /// position and `for do` is a syntax error — but `parse_dparen` tests it
    /// *before* asking that question, in a branch of its own for the arithmetic
    /// `for (( … ))` header (parse.y:4464-4480):
    ///
    /// ```c
    ///   if (last_read_token == FOR)
    ///     { ... return (ARITH_FOR_EXPRS); }
    ///   if (reserved_word_acceptable (last_read_token))
    ///     { ... return (ARITH_CMD); }
    /// ```
    ///
    /// osh reads both shapes with the one `((` token, so the two branches meet
    /// in [`Lexer::arith_cmd_position`].
    For,
    /// An assignment word (`n=v`, `n+=v`, `n=(…)`) in a position where one was
    /// accepted. A command word may still follow it.
    Assignment,
    /// A redirection operator, or one of the two prefixes that introduce one: an
    /// io number (`2>&1`) or a varfd (`{v}>f`). What follows is the
    /// redirection's target, never a command.
    RedirOp,
    /// The word that was a redirection's target, or a here-document body.
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

/// One run of characters in a spliced text, and where they really live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextSeg {
    /// Offset in the spliced text at which this run begins. It runs until the
    /// next segment's `at`, or to the end of the text for the last one.
    at: usize,
    /// The text those characters were written in, as a [`TokSpan::src`].
    src: u32,
    /// Offset in *that* text of this run's first character.
    base: usize,
}

/// Where the characters of the text the alias pass is walking came from.
///
/// The text is in general a *concatenation*. bash reads an alias by pushing its
/// value onto the input: `push_string` (parse.y:2694) makes the value the
/// current `shell_input_line` and stacks the line it displaced together with the
/// reader's index just past the alias word, and `pop_string` restores that line
/// *at that index*. So what the one reader goes on to see, uninterrupted, is the
/// value followed by the tail of the calling line — and since the calling line
/// may itself be such a concatenation, the splices nest.
///
/// A `TextMap` is the run-length record of that concatenation: segments in
/// ascending `at` order, the first always at `0`. It exists for diagnostics —
/// bash echoes `shell_input_line` as it stands once the offending token has been
/// read, i.e. the text that token *ended* in, which is [`Self::at`] of the
/// token's end offset.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TextMap {
    segs: Vec<TextSeg>,
}

impl TextMap {
    /// A text that is nothing but `src` itself, read from its start.
    fn whole(src: u32) -> Self {
        Self { segs: vec![TextSeg { at: 0, src, base: 0 }] }
    }

    /// Which text the character at `off` was written in, and its offset there.
    ///
    /// Offsets past the end of the text answer as the last segment continued,
    /// which is what a caller asking about a token's *end* offset wants.
    fn at(&self, off: usize) -> (u32, usize) {
        let i = self.segs.partition_point(|s| s.at <= off).saturating_sub(1);
        self.segs.get(i).map_or((0, off), |s| {
            (s.src, s.base.saturating_add(off.saturating_sub(s.at)))
        })
    }

    /// The map of `self[..word] ++ value ++ self[cut..]` — the text bash's
    /// reader sees once the alias word running from `word` to `cut` has been
    /// replaced by a `vlen`-character value living in text `src`.
    ///
    /// The text *before* the word is kept even though the reader is long past
    /// it, because the splice is re-read from the beginning: a here-document
    /// operator standing on an earlier line still has a body to gather, and it
    /// must gather it before any the value contributes. Keeping the text is what
    /// lets one lex settle both, in `redir_stack` order.
    fn spliced(&self, word: usize, cut: usize, vlen: usize, src: u32) -> Self {
        let mut segs: Vec<TextSeg> = self.segs.iter().copied().filter(|s| s.at < word).collect();
        segs.push(TextSeg { at: word, src, base: 0 });
        let moved = word.saturating_add(vlen);
        for (i, s) in self.segs.iter().enumerate() {
            // A segment runs until the next one begins; the last runs to the end
            // of the text, which is at or past `cut` whatever `cut` is.
            let end = self.segs.get(i.saturating_add(1)).map_or(usize::MAX, |n| n.at);
            if end <= cut {
                // Wholly inside the word or behind it; the word is gone.
                continue;
            }
            // Only the segment `cut` falls *inside* loses a prefix.
            let skip = cut.saturating_sub(s.at);
            segs.push(TextSeg {
                at: moved.saturating_add(s.at.saturating_add(skip).saturating_sub(cut)),
                src: s.src,
                base: s.base.saturating_add(skip),
            });
        }
        Self { segs }
    }

    /// Where the next run of *real input* begins at or after `off`.
    ///
    /// The body of a here-document is read from the input file, never from a
    /// pushed alias value (`read_a_line` calls `yy_getc` directly), so a reader
    /// standing inside a value that meets a `<<` has to know where the input it
    /// left off reading resumes. That is the first `src == 0` run from here on.
    /// `None` when the text ends inside a value.
    fn real_at_or_after(&self, off: usize) -> Option<usize> {
        if self.at(off).0 == 0 {
            return Some(off);
        }
        self.segs.iter().find(|s| s.at >= off && s.src == 0).map(|s| s.at)
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
    /// Spans of the **real input** this pass's own lex ate as here-document
    /// bodies, ascending and disjoint. A `<<` a replacement text brought with it
    /// does not exist in the input's own reading, so the lines its body takes
    /// were read there as commands; the caller has to unread them. See
    /// [`Spanned::taken`] and [`crate::parser::IncrementalParser::rebuild`].
    pub taken: Vec<(u32, u32)>,
    /// What the reader complained about while fetching those bodies, each paired
    /// with the offset in the real input where its body *began* — the start of
    /// the matching [`Self::taken`] span. The input's own lex never saw the `<<`,
    /// so it could not have raised these. Lines are already the script's; the
    /// caller holds each warning until the parse has passed that offset.
    pub warnings: Vec<(u32, ReaderWarning)>,
}

/// Where in a simple command the reader stands, as the state machine bash's
/// `last_read_token` plus `PST_REDIRLIST` amount to.
///
/// bash asks this question of a *parser* token, so it cannot be answered by
/// looking back at the output stream: whether a word was a reserved word, an
/// assignment or an ordinary word depends on where it sat when it was read, and
/// that classification is gone by the time it is a `Tok`. So the answer is
/// carried forward instead — [`Self::advance`] is fed every token as it is
/// produced, and [`Self::at_command`] and [`Self::reserved_ok`] read it off.
///
/// One machine, several readers: the alias pass drives it over the tokens it
/// re-emits, the scan that has to decide whether a `[` opens a subscript drives
/// it over the ones it is producing, and a flush `((` asks it whether an
/// arithmetic command may stand here. bash answers all three from the one
/// `last_read_token`, so they must not be several approximations of it.
///
/// The fold is recursive, which is why no table keyed on the previous token's
/// *spelling* can express it: a word becomes a reserved word only where one was
/// already acceptable (`CHECK_FOR_RESERVED_WORD`, parse.y:2994, is itself gated
/// on `reserved_word_acceptable`), so `do` opens a loop body after `;` but is an
/// argument after `echo` — `; do ((1))` is an arithmetic command while
/// `echo do ((1))` is a syntax error near `(`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CmdPos {
    prev: Prev,
    /// bash's `reserved_word_acceptable (last_read_token)` (parse.y:5367), for
    /// the token about to be read.
    ok: bool,
    /// bash's `PST_REDIRLIST`: this simple command was *created* by a
    /// redirection, so nothing of it has been read but redirections and the word
    /// after one is still the command word.
    ///
    /// `make_simple_command` sets it only on the branch that makes the bare
    /// command, and clears it the moment a word joins one (make_cmd.c:526-535),
    /// so it is the command's *first* element that decides — which is why `>f c`
    /// alias-expands `c` while `x=1 >f c` does not. [`Self::first`] is that
    /// "command not made yet".
    redir_list: bool,
    /// Whether the next element read would be the first of its simple command,
    /// the one whose kind sets [`Self::redir_list`].
    first: bool,
    /// Whether a `|` is close enough behind to disqualify a `time`. See
    /// [`AfterPipe`].
    after_pipe: AfterPipe,
    /// Inside a `[[ … ]]`, where `last_read_token` is frozen. Not a depth: a
    /// `[[` in there is a word, not a second `COND_START`.
    ///
    /// A whole conditional is *one* token to bash: `parse_cond_command` runs
    /// from inside a single `read_token` call (parse.y:3399) and asks for its own
    /// tokens directly, so `last_read_token` never moves off `COND_START` — which
    /// is not acceptable — until the `]]` comes back as `COND_END`, which is. osh
    /// emits that span as ordinary tokens, so the freeze has to be modelled here.
    /// This is why no paren inside a conditional is ever arithmetic, however deep.
    cond: bool,
}

/// How close behind the nearest `|` is, to the one token's depth
/// `time_command_acceptable` looks back.
///
/// `|` is simply not in its case list, and a newline is thrown out when a pipe
/// stands immediately before it (parse.y:3128-3134):
///
/// ```c
///     case 0:
///     case ';':
///     case '\n':
///       if (token_before_that == '|')
///         return (0);
///       /* FALLTHROUGH */
/// ```
///
/// `token_before_that` is exactly one token back, so this is *not* a run: after
/// two newlines the test no longer sees the pipe and `time` becomes the reserved
/// word again — at which point bash's own grammar rejects it, because `pipeline
/// '|' newline_list pipeline` yields a `pipeline` and `timespec` only appears in
/// `pipeline_command`. `true |⏎time x` is therefore `time: command not found`
/// while `true |⏎⏎time x` is `` syntax error near unexpected token `time' ``.
/// Modelled as measured, one token back.
///
/// Only `time` is fussy about a pipe: `true | h[1 2]=v` *does* slurp its
/// subscript, so a pipe is a command position for an assignment. That is why
/// this is a field of its own rather than another [`Prev`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterPipe {
    /// No pipe within reach.
    No,
    /// The token just read was `|` or `|&`.
    Pipe,
    /// The token just read was a newline, and the one before it a pipe.
    Newline,
}

/// One token's worth of [`CmdPos`]'s fold, for everything that is not a word.
///
/// A word carries too much with it to reduce to a tag — its spelling decides
/// whether it is reserved, and its shape whether it is an assignment — so
/// [`CmdPos::word`] takes those directly. Everything else collapses to one of
/// these, which is what lets a *character* scan drive the same fold a token
/// stream does: [`CaseScan`] never builds a [`Tok`], it just says which of these
/// the operator it has just read stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    /// A newline, which is the one token that can carry a pipe forward. See
    /// [`AfterPipe`].
    Newline,
    /// `|` or `|&`.
    Pipe,
    /// `;;`, `;&` or `;;&` — after which a reserved word *would* be accepted,
    /// but what follows is the next `case` arm's pattern. See
    /// [`CmdPos::at_command`].
    CaseArmEnd,
    /// `;`, `&`, `&&`, `||`, `(` or `)`: a command begins after it.
    Sep,
    /// A redirection operator, or one of the two tokens that may prefix one — an
    /// IO number (`2>`) or a `{name}` descriptor variable.
    RedirOp,
    /// A here-document body, which is the tail of the redirection its delimiter
    /// opened.
    RedirTarget,
    /// A `(( … ))` arithmetic command, bash's ARITH_CMD.
    ArithCmd,
    /// An array literal in a command position (`h=( … )`), bash's
    /// ASSIGNMENT_WORD.
    Assignment,
    /// Anything else: nothing is acceptable after it.
    Other,
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
    pos: CmdPos,
}

impl AliasOut {
    fn new() -> Self {
        Self {
            toks: Vec::new(),
            lines: Vec::new(),
            origin: Vec::new(),
            spans: Vec::new(),
            bodies: Vec::new(),
            pos: CmdPos::new(),
        }
    }
}

impl CmdPos {
    /// The state at the very start of the input, which is bash's
    /// `last_read_token = 0` — a value its own list names (parse.y:5403), so a
    /// reserved word is acceptable and a command begins.
    const fn new() -> Self {
        Self {
            prev: Prev::Start,
            ok: true,
            redir_list: false,
            first: true,
            after_pipe: AfterPipe::No,
            cond: false,
        }
    }

    /// True when a reserved word would be recognised here — bash's
    /// `reserved_word_acceptable`. It is *not* the same question as command
    /// position: `x=1 if` is a command named `if`, and `case x in y) …` accepts
    /// a pattern rather than a reserved word after `;;`.
    const fn reserved_ok(&self) -> bool {
        self.ok
    }

    /// True where a `((` opens an arithmetic command — the two branches
    /// `parse_dparen` tries, in its order. See [`Prev::For`].
    const fn arith_ok(&self) -> bool {
        matches!(self.prev, Prev::For) || self.reserved_ok()
    }

    /// True where `time` is the reserved word — bash's
    /// `time_command_acceptable`, which is `reserved_word_acceptable` less a
    /// pipe. See [`AfterPipe`].
    const fn time_ok(&self) -> bool {
        self.reserved_ok() && matches!(self.after_pipe, AfterPipe::No)
    }

    /// True when a word here is the command word of a simple command — bash's
    /// `command_token_position`, whose three clauses this is, in order:
    ///
    /// ```c
    /// #define command_token_position(token) \
    ///   (((token) == ASSIGNMENT_WORD) || \
    ///    ((parser_state&PST_REDIRLIST) && parsing_redirection(token) == 0) || \
    ///    ((token) != SEMI_SEMI && (token) != SEMI_AND && (token) != SEMI_SEMI_AND && reserved_word_acceptable(token)))
    ///                                                                        /* parse.y:2983-2986 */
    /// ```
    ///
    /// `;;`, `;&` and `;;&` are named in the third because a reserved word
    /// *would* be accepted after one, but what actually follows is the next
    /// `case` arm's pattern — so `case x in y) :;; f[1 2]=v` is two words, not an
    /// assignment with a subscript that swallowed the blank. The other two
    /// clauses do not reach one: no `case` arm ends in an assignment, and
    /// `PST_REDIRLIST` belongs to a simple command the `;;` has already ended.
    ///
    /// Two callers ask it, and bash asks it with this same macro for both: an
    /// alias lookup happens only here (`alias_expand_token`), and an unquoted
    /// blank is only subscript content here (`assignment_acceptable`, which is
    /// this *less* `case` patterns — and a pattern position is out of command
    /// position by the third clause anyway).
    fn at_command(&self) -> bool {
        matches!(self.prev, Prev::Assignment)
            || (self.redir_list && !matches!(self.prev, Prev::RedirOp))
            || (!matches!(self.prev, Prev::CaseArmEnd) && self.reserved_ok())
    }

    /// Record the token just read: what it leaves behind for the next one, and
    /// whether a reserved word may stand there.
    ///
    /// `PST_REDIRLIST` follows from the same answer, which is why every event
    /// goes through here rather than assigning [`Self::prev`].
    fn set(&mut self, prev: Prev, ok: bool, after_pipe: AfterPipe) {
        match prev {
            // The redirection that *made* the simple command is the one that
            // sets the flag; a later one only leaves it as it stands.
            Prev::RedirOp if self.first => {
                self.redir_list = true;
                self.first = false;
            }
            Prev::RedirOp | Prev::RedirTarget => {}
            // A separator, or a reserved word: whatever command follows has not
            // been made yet, so its first element is still to come.
            Prev::Start
            | Prev::CaseArmEnd
            | Prev::Time
            | Prev::TimeOpt
            | Prev::Function
            | Prev::Coproc
            | Prev::For => {
                self.redir_list = false;
                self.first = true;
            }
            // A word joined the command, which is what clears the flag.
            Prev::Assignment | Prev::Other => {
                self.redir_list = false;
                self.first = false;
            }
        }
        self.prev = prev;
        self.ok = ok;
        self.after_pipe = after_pipe;
    }

    /// Record `tok`, which has just been pushed, as the new previous token.
    /// `was_cmd` is what [`Self::at_command`] said about it before the push —
    /// a word is only a reserved word, or an assignment, where one was allowed.
    ///
    /// Every token but a word reduces to one [`Ev`], which is the form a driver
    /// with no [`Tok`] to hand emits. See [`CaseScan`].
    fn advance(&mut self, tok: &Tok, was_cmd: bool) {
        let ev = match tok {
            Tok::Word(segs) => {
                self.word(bare_word(segs), word_is_assignment(segs), was_cmd);
                return;
            }
            Tok::Newline => Ev::Newline,
            Tok::Op(Op::Pipe | Op::PipeAmp) => Ev::Pipe,
            Tok::Op(Op::DSemi | Op::SemiAmp | Op::DSemiAmp) => Ev::CaseArmEnd,
            Tok::Op(
                Op::AndIf | Op::OrIf | Op::Amp | Op::Semi | Op::LParen
                // `)` is in bash's list, commented there `/* only valid in case
                // statement */` — a `case` arm's pattern close, after which the
                // arm's first command begins. It is also a subshell's close,
                // where no word may follow; that is a *grammar* error, and the
                // reader reaches the `[` first either way, so `(:) f[1` is
                // still the reader's unclosed-subscript failure.
                | Op::RParen,
            ) => Ev::Sep,
            // A redirection, or one of the two prefixes that introduce one.
            Tok::Op(_) | Tok::Io(_) | Tok::VarFd(_) => Ev::RedirOp,
            // bash's ARITH_CMD, which is on its list.
            Tok::ArithCmd(..) => Ev::ArithCmd,
            Tok::ArrayAssign { .. } if was_cmd => Ev::Assignment,
            // The body arrives after the delimiter word, and is the tail of the
            // same redirection — so it leaves a run of them running.
            Tok::HereDoc(..) => Ev::RedirTarget,
            // A construct the lexer refused, or an array literal that is not an
            // assignment: nothing follows it.
            _ => Ev::Other,
        };
        self.ev(ev);
    }

    /// One non-word token's worth of the fold.
    fn ev(&mut self, ev: Ev) {
        // Inside a conditional nothing but the `]]` moves `last_read_token`, and
        // a `]]` arrives as a word. See [`Self::cond`].
        if self.cond {
            self.set(Prev::Other, false, AfterPipe::No);
            return;
        }
        match ev {
            Ev::Newline => {
                let carried = if matches!(self.after_pipe, AfterPipe::Pipe) {
                    AfterPipe::Newline
                } else {
                    AfterPipe::No
                };
                self.set(Prev::Start, true, carried);
            }
            Ev::Pipe => self.set(Prev::Start, true, AfterPipe::Pipe),
            Ev::CaseArmEnd => self.set(Prev::CaseArmEnd, true, AfterPipe::No),
            Ev::Sep | Ev::ArithCmd => self.set(Prev::Start, true, AfterPipe::No),
            Ev::RedirOp => self.set(Prev::RedirOp, false, AfterPipe::No),
            Ev::RedirTarget => self.set(Prev::RedirTarget, false, AfterPipe::No),
            Ev::Assignment => self.set(Prev::Assignment, false, AfterPipe::No),
            Ev::Other => self.set(Prev::Other, false, AfterPipe::No),
        }
    }

    /// One word's worth of the fold. `lit` is its spelling when it is a single
    /// unquoted literal segment and `None` otherwise — bash's
    /// `CHECK_FOR_RESERVED_WORD` refuses a word that carries a `$` or any
    /// quoting, so only that shape can be a reserved word. `assign` is whether it
    /// is an assignment as *written*.
    fn word(&mut self, lit: Option<&[u8]>, assign: bool, was_cmd: bool) {
        // A conditional is one token to bash, and the `]]` that ends it is the
        // only word in there that moves `last_read_token`. See [`Self::cond`].
        if self.cond {
            self.cond = lit != Some(COND_END);
            if self.cond {
                self.set(Prev::Other, false, AfterPipe::No);
            } else {
                self.set(Prev::Start, true, AfterPipe::No);
            }
            return;
        }
        // A redirection's target: the tail of the redirection, so it neither
        // ends a run of them nor is ever a reserved word.
        if matches!(self.prev, Prev::RedirOp) {
            self.set(Prev::RedirTarget, false, AfterPipe::No);
            return;
        }
        // `time`'s own options, which bash lexes as TIMEOPT and TIMEIGN and
        // accepts a command after just as it does after `time` itself. Each
        // stands in exactly one place: see [`Prev::TimeOpt`].
        match (lit, self.prev) {
            (Some(b"-p"), Prev::Time) => {
                self.set(Prev::TimeOpt, true, AfterPipe::No);
                return;
            }
            (Some(b"--"), Prev::Time | Prev::TimeOpt) => {
                self.set(Prev::Start, true, AfterPipe::No);
                return;
            }
            _ => {}
        }
        let reserved = lit
            .filter(|_| self.reserved_ok())
            .filter(|w| RESERVED_WORDS.contains(w))
            // `time` alone is refused behind a pipe. See [`AfterPipe`].
            .filter(|w| *w != b"time" || self.time_ok());
        if let Some(w) = reserved {
            if w == COND_START {
                self.cond = true;
                self.set(Prev::Other, false, AfterPipe::No);
                return;
            }
            let leaves = RW_LEAVES_ACCEPTABLE.contains(&w);
            let prev = match w {
                b"time" => Prev::Time,
                b"function" => Prev::Function,
                b"coproc" => Prev::Coproc,
                b"for" => Prev::For,
                _ if leaves => Prev::Start,
                _ => Prev::Other,
            };
            self.set(prev, leaves, AfterPipe::No);
            return;
        }
        // A plain WORD, which is acceptable only as the name that follows
        // `function` or `coproc` (parse.y:5406-5412).
        let name = matches!(self.prev, Prev::Function | Prev::Coproc);
        let prev = if was_cmd && assign { Prev::Assignment } else { Prev::Other };
        self.set(prev, name, AfterPipe::No);
    }
}

/// The text bash's reader currently has in hand, and the tokens it reads from it.
///
/// Expanding an alias does not splice *tokens*: `push_string` (parse.y:2694)
/// makes the value the current `shell_input_line` and stacks the line it
/// displaced together with the reader's index just past the alias word, so the
/// one reader goes straight on into the value and, when the value runs dry,
/// straight back out into the tail of the calling line. Whatever stands across
/// that seam it reads as one thing — an operator whose halves lie either side of
/// it, a here-document delimiter, a comment that runs on past the pop. Modelling
/// that faithfully means splicing the *text* and reading it again, which is what
/// this is: the concatenation, the provenance of its characters, and its lex.
struct AliasView {
    toks: Vec<Tok>,
    /// Parallel to `toks`: the 1-based line of each token *in `text`*, which is
    /// not a line of the script — see [`AliasScan::line`].
    lines: Vec<u32>,
    /// Parallel to `toks`: where each token starts and ends in `text`.
    starts: Vec<u32>,
    ends: Vec<u32>,
    /// Parallel to `toks`: where the body reader stood. See [`Spanned::raws`].
    raws: Vec<u32>,
    /// What this view's lex ate as bodies, in `text`'s offsets. See
    /// [`Spanned::taken`].
    taken: Vec<(u32, u32)>,
    /// What it complained about while eating them, in `text`'s line numbering.
    warnings: Vec<ReaderWarning>,
    /// Parallel to `warnings`: where in `text` each complaint's body was being
    /// read from. See [`Spanned::warn_from`].
    warn_from: Vec<u32>,
    text: Vec<Ch>,
    /// Which text each offset in `text` really belongs to. See [`TextMap`].
    map: TextMap,
}

/// One entry of bash's pushed-string list: the region of the current text an
/// alias value occupies.
struct AliasPush {
    name: Str,
    /// Where the value begins in the current text, and where it ends — the
    /// offset at which `pop_string` happens.
    start: usize,
    end: usize,
    /// The alias's `AL_EXPANDNEXT` (its value ended in a blank), which
    /// `pop_string` turns back into `PST_ALEXPNEXT`.
    expand_next: bool,
}

/// The reader state the scan carries from one spliced text to the next.
struct AliasScan {
    /// bash's pushed-string list, as *regions*. The reader is inside a push
    /// while it is reading between its `start` and its `end`, and an alias whose
    /// push it is inside is `AL_BEINGEXPANDED` and so not a candidate — which is
    /// what makes `alias ls='ls -l'` terminate.
    ///
    /// Regions rather than a stack because every splice re-reads the whole text
    /// from the beginning, so the reader walks into and out of the same push
    /// once per pass. A stack would be emptied by the first pass and the guard
    /// would be gone by the second.
    pushes: Vec<AliasPush>,
    /// Offsets in the current text where an alias word stood, each with the
    /// input token it was.
    ///
    /// The first token read at or after such an offset stands in for the word
    /// and takes its origin: a caller resuming the parse at the start of a
    /// splice would otherwise find the next `Some` origin *past* the value and
    /// silently drop the command. An empty value contributes no token of its
    /// own, and then the mark falls to the token after it — which is right,
    /// since resuming there re-expands the word to nothing again.
    marks: Vec<(usize, usize)>,
    /// The first character of the physical line the scan starts on, which is
    /// where every re-lex begins. See [`Lexer::spliced`]. Fixed for the whole
    /// scan: a splice only ever moves text *after* the alias word, and the
    /// alias word is never before the point the caller handed over.
    head: usize,
    /// bash's `PST_ALEXPNEXT`: the next word is a candidate however the structure
    /// reads. Set by *popping* a value that ended in a blank — that is the pop
    /// that puts `alias sudo='sudo '`'s argument in expansion position.
    force: bool,
    /// The start offset of the token before this one, which bounds the pops that
    /// happened since: the reader left every push whose end lies between them.
    prev: usize,
    /// The source line of the last token emitted. Text pushed by an alias is not
    /// a line of the script and has none of its own — `line_number` is bumped
    /// only by *fetching* a line (parse.y:2346), which reading a pushed string
    /// never does — so every token read out of a value keeps this. Gathering a
    /// here-document body does fetch lines, and moves it on.
    line: u32,
    /// Cursor into the input token stream, for recovering which input token an
    /// offset in the real input belongs to. Monotone: the reader never goes back.
    k: usize,
    /// Cursor into `marks`, likewise monotone.
    m: usize,
}

impl AliasScan {
    /// The input token that begins at `off` in the real input, if the scan has
    /// not already passed it.
    ///
    /// The cursor is monotone because the reader is: the real input's offsets
    /// come out in order however deeply the values spliced between them nest.
    /// Matching by position rather than by search also keeps the answers distinct
    /// where one lexer iteration produced two tokens — a `<<` and the delimiter
    /// after it share a start.
    fn take_input_at(&mut self, off: usize, starts: &[u32]) -> Option<usize> {
        while starts.get(self.k).is_some_and(|&s| (s as usize) < off) {
            self.k = self.k.saturating_add(1);
        }
        if starts.get(self.k).copied()? as usize != off {
            return None;
        }
        let k = self.k;
        self.k = self.k.saturating_add(1);
        Some(k)
    }

    /// The alias word, if any, that the token starting at `off` answers for.
    fn take_mark(&mut self, off: usize) -> Option<usize> {
        let mut mark = None;
        while let Some(&(at, k)) = self.marks.get(self.m) {
            if at > off {
                break;
            }
            mark.get_or_insert(k);
            self.m = self.m.saturating_add(1);
        }
        mark
    }

    /// Whether `name` is `AL_BEINGEXPANDED` for a token starting at `off`.
    fn being_expanded(&self, name: &Str, off: usize) -> bool {
        self.pushes.iter().any(|p| p.name == *name && p.start <= off && off < p.end)
    }

    /// Take `PST_ALEXPNEXT` from the pops that happened between the previous
    /// token and one starting at `off`. The outermost pop — the one whose value
    /// ends last — decides, exactly as the last assignment in bash's pop chain
    /// does.
    fn pop_to(&mut self, off: usize) {
        let (prev, force) = (self.prev, &mut self.force);
        if let Some(p) =
            self.pushes.iter().filter(|p| p.end > prev && p.end <= off).max_by_key(|p| p.end)
        {
            *force = p.expand_next;
        }
        self.prev = off;
    }
}

/// The 1-based script line a reader that has consumed the real input up to `off`
/// sits on — the line of the character *before* `off`, so a cursor left just past
/// a newline is still on the line that newline ended.
///
/// The input token that most recently began before `off` anchors the count,
/// because only a token carries the script's own numbering (a fragment lexed on
/// its own is renumbered by the caller's `LineMap`); the physical lines between
/// the anchor and `off` are then counted in the text.
fn line_at_input(text: &[Ch], starts: &[u32], lines: &[u32], off: usize) -> Option<u32> {
    let k = starts.partition_point(|&s| (s as usize) < off).checked_sub(1)?;
    let anchor = *starts.get(k)? as usize;
    let span = text.get(anchor..off.saturating_sub(1)).unwrap_or(&[]);
    let n = u32::try_from(span.iter().filter(|&&c| c == '\n').count()).unwrap_or(0);
    Some(lines.get(k)?.saturating_add(n))
}

/// The name an alias lookup would be made under for `tok`, if it is a token bash
/// would make one for at all.
///
/// bash asks the question of whatever `read_token_word` just built: `if
/// (expand_aliases && quoted == 0) result = alias_expand_token (token)`
/// (parse.y:5266). So it is the *word*-ness of the token that matters, not its
/// role — and the delimiter of a `<<` is read by `read_token_word` like any
/// other word, since `<<`'s target is an ordinary WORD in the grammar. Quoting
/// inhibits the lookup, which for a delimiter is the flag the token already
/// carries (a quoted delimiter is also a non-expanding one).
///
/// Whether such a token is in a *position* where the lookup happens is a
/// separate question, and the caller's: [`CmdPos::at_command`] for an ordinary
/// word, and for the delimiter only `PST_ALEXPNEXT` — reading the `<<` clears
/// that flag (parse.y:3511), and only a value ending in a blank sets it again.
fn alias_candidate(tok: &Tok) -> Option<&Str> {
    match tok {
        Tok::Word(segs) => match segs.as_slice() {
            [Seg::Lit(name)] => Some(name),
            _ => None,
        },
        Tok::HereDoc(_, delim, false) => Some(delim),
        _ => None,
    }
}

/// Expand shell aliases over a token stream (bash's pre-parse alias pass).
///
/// Only a single unquoted-literal word in command position is a candidate. Its
/// value is pushed onto the reader's input in front of the rest of the text and
/// the whole thing read again from there, so the value's first word is itself a
/// candidate (guarded by [`AliasScan::active`], which is what makes `alias
/// ls='ls -l'` terminate) and anything written across the seam is read as one
/// thing. If a value ends in a blank, the word after the pop is a candidate too
/// (bash's `PST_ALEXPNEXT`, which is what enables `alias sudo='sudo '`).
///
/// `starts` and `ends` are parallel to `toks`: where in `text` each token begins
/// and ends. Both are needed — `ends` so that a diagnostic raised while the
/// reader is inside a value can quote *that* text (see [`TokSpan`]), and
/// `starts` to recognise a token that came through the splice unchanged.
///
/// Beside the expanded stream comes an *origin* vector: for each output token,
/// the index of the input token it came from, or `None` when the token was read
/// out of an alias value (or across the seam into one).
/// [`crate::parser::IncrementalParser`] needs it to resume — after executing one
/// item it must know which *original* token to continue from, and must not
/// re-expand tokens an alias already produced.
#[must_use]
pub fn expand_aliases_tracked(
    toks: &[Tok],
    lines: &[u32],
    starts: &[u32],
    ends: &[u32],
    text: &[Ch],
    aliases: &crate::assoc::AssocArray,
    opts: ParseOpts,
) -> AliasExpansion {
    let mut out = AliasOut::new();
    // Where in the real input the stream this call was handed begins. The
    // caller may be resuming part-way through the script — it has executed
    // everything before this and wants only what follows — but the *text* it
    // hands over is the whole script, because a splice is made in place and a
    // re-lex needs the text on either side of it. Tokens before this point are
    // re-read, then dropped.
    let from = starts.first().map_or(0, |&s| s as usize);
    // Back to the top of the line the resume point stands on, which is where
    // every re-lex begins: as far back as one needs to go, and as far back as
    // one may safely go. See [`Lexer::spliced`].
    let head = text.get(..from).unwrap_or(&[]).iter().rposition(|&c| c == '\n').map_or(0, |i| {
        i.saturating_add(1)
    });
    let mut st = AliasScan {
        pushes: Vec::new(),
        marks: Vec::new(),
        head,
        force: false,
        prev: 0,
        line: lines.first().copied().unwrap_or(1),
        k: 0,
        m: 0,
    };
    let mut view = AliasView {
        toks: toks.to_vec(),
        lines: lines.to_vec(),
        starts: starts.to_vec(),
        ends: ends.to_vec(),
        // The input has no values spliced into it yet, so nothing here is read
        // out of one and no token needs to ask where the body reader stood — and
        // whatever bodies the input's own lex took, the caller took too.
        raws: Vec::new(),
        taken: Vec::new(),
        warnings: Vec::new(),
        warn_from: Vec::new(),
        text: text.to_vec(),
        map: TextMap::whole(0),
    };
    'splice: loop {
        // Every splice re-reads the assembled text from the beginning, so the
        // stream is built afresh each pass. The replacement *texts* are not:
        // they are already written into `view.text`, and their `TokSpan::src`
        // numbers are the ones `view.map` records.
        let bodies = core::mem::take(&mut out.bodies);
        out = AliasOut::new();
        out.bodies = bodies;
        (st.force, st.prev, st.k, st.m) = (false, 0, 0, 0);
        st.line = lines.first().copied().unwrap_or(1);
        for (i, tok) in view.toks.iter().enumerate() {
            let start = view.starts.get(i).map_or(usize::MAX, |&s| s as usize);
            let end = view.ends.get(i).map_or(usize::MAX, |&e| e as usize);
            // Every push the reader has left behind pops here, and the outermost
            // pop decides `PST_ALEXPNEXT` — see [`AliasScan::force`].
            st.pop_to(start);
            // A token read wholly out of the real input is the input's own token,
            // and the parse may resume at it. One that begins inside a value is
            // not, even if it ends outside: it was never written where it is read.
            let (ssrc, soff) = view.map.at(start);
            if ssrc == 0 && soff < from {
                // Already executed by the caller; re-read only to move the
                // body reader's cursor. Nothing before the resume point can be
                // an alias word, because nothing there was spliced.
                continue;
            }
            let origin = if ssrc == 0 { st.take_input_at(soff, starts) } else { None };
            let at_cmd = st.force || out.pos.at_command();
            st.force = false;
            if at_cmd
                && let Some(name) = alias_candidate(tok)
                && !st.being_expanded(name, start)
                && let Some(val) = aliases.get(name)
                && let Some(next) =
                    splice_alias(&view, &mut st, name, val, start..end, &mut out, opts)
            {
                // The word is gone but the parse may still have to resume at it,
                // so the offset it stood at remembers which input token it was.
                if let Some(k) = origin {
                    st.marks.push((start, k));
                }
                view = next;
                continue 'splice;
            }
            // A token standing where an alias word stood answers for it, so that
            // a resume lands on the word and expands it again rather than
            // skipping past the whole command.
            let mark = st.take_mark(start);
            let origin = origin.or(mark);
            // `line_number` is bumped by *fetching* a line and is never wound
            // back (parse.y:2346), so a gather made while the reader stood
            // inside a value leaves even the rest of the *calling* line numbered
            // from where the gather stopped — the calling line was fetched long
            // before, and nothing renumbers it on the way back.
            let line = origin.and_then(|k| lines.get(k).copied()).unwrap_or(st.line).max(st.line);
            st.line = line;
            // A `$( … )` remembers the line its `)` sits on, in the numbering of
            // the text it was lexed from — and a spliced text is not the script.
            // Put those recorded lines back on the script's numbering, which for
            // anything read out of a value is the reader's line.
            let mut tok = tok.clone();
            let by = i64::from(line) - i64::from(view.lines.get(i).copied().unwrap_or(1));
            shift_tok_lines(&mut tok, by);
            // Which text the token was read from is decided by its *last
            // character*, not by the offset past it — because the pop that the
            // offset past it would land in has not necessarily happened.
            // `pop_string` fires on the read that finds the pushed string used
            // up, and an operator finished by its own lookahead never makes
            // that read: `alias A='[[ a>>'` with `A b ']]` is still reported
            // against `[[ a>>`. Whether the read happened is the parser's to
            // judge (`Spans::reader_stop`, which pops when it did), so what is
            // recorded here is the span *before* any pop — the token's own
            // text, with `end` flush against that text's length when the token
            // ends flush against it.
            let (esrc, eoff) = view.map.at(end.saturating_sub(1));
            let end = u32::try_from(eoff).map_or(u32::MAX, |e| e.saturating_add(1));
            out.pos.advance(&tok, at_cmd);
            out.toks.push(tok);
            out.lines.push(line);
            out.origin.push(origin);
            out.spans.push(TokSpan { src: esrc, end });
            // If this token's iteration gathered a here-document body, the reader
            // fetched those lines and `line_number` went with them — so what is
            // read *after* it, still inside the value, is numbered from where the
            // gather stopped rather than from the alias word's line.
            if ssrc != 0
                && let Some(r) = view.raws.get(i).copied().filter(|&r| r > 0)
                && let (0, roff) = view.map.at(r as usize)
                && let Some(l) = line_at_input(text, starts, lines, roff)
            {
                st.line = st.line.max(l);
            }
        }
        break;
    }
    // The gathers were made in the assembled text; what the caller has to unread
    // is the *input* they came out of. A body always is input — that is what
    // makes it a body — so every span maps back whole.
    // A body is one unbroken run of input, so mapping its start is enough: no
    // value can be spliced inside it, and its length is the same on both sides.
    let taken = view
        .taken
        .iter()
        .filter_map(|&(a, b)| match view.map.at(a as usize) {
            (0, off) => {
                let off = u32::try_from(off).unwrap_or(u32::MAX);
                Some((off, off.saturating_add(b.saturating_sub(a))))
            }
            _ => None,
        })
        .collect();
    // A warning is keyed to where the body it complains about was being *read
    // from*, which is inside one of those spans. The caller releases a warning
    // once the parse has passed its offset, and a unit that swallowed a body is
    // stretched to that body's end — so a key at or past the body's end would
    // fall inside every unit before it too, and the warning would go out far too
    // early. Its line numbers are the assembled text's and have to be put back on
    // the script's.
    let warnings = view
        .warnings
        .iter()
        .zip(&view.warn_from)
        .filter_map(|(w, &from)| {
            let (0, off) = view.map.at(from as usize) else { return None };
            let mut w = w.clone();
            w.map_lines(|l| script_line(&view, text, starts, lines, l).unwrap_or(l));
            Some((u32::try_from(off).unwrap_or(u32::MAX), w))
        })
        .collect();
    AliasExpansion {
        toks: out.toks,
        lines: out.lines,
        origin: out.origin,
        spans: out.spans,
        bodies: out.bodies,
        taken,
        warnings,
    }
}

/// The 1-based line `line` of an assembled alias text, in the script's own
/// numbering: the lines of a replacement value are not lines of the input, so a
/// text with one spliced in counts more of them than the script has.
fn script_line(view: &AliasView, text: &[Ch], starts: &[u32], lines: &[u32], line: u32) -> Option<u32> {
    let mut at = 0usize;
    for _ in 1..line {
        let i = view.text.get(at..)?.iter().position(|&c| c == '\n')?;
        at = at.saturating_add(i).saturating_add(1);
    }
    // A line that begins inside a value belongs to the input line the value
    // stands on, which is where the reader's own numbering has stayed.
    let real = view.map.real_at_or_after(at)?;
    let (0, off) = view.map.at(real) else { return None };
    line_at_input(text, starts, lines, off.saturating_add(1))
}

/// Write `val` into the text in place of the alias word standing at `at`, and
/// read the whole thing again: bash's `push_string`, done to the text rather
/// than to the token stream.
///
/// Returns `None` — leaving `st` and `out` untouched — when the spliced text will
/// not lex at all. bash would report that error against the value; osh has
/// already lexed this input once and has nowhere to put a second failure, so the
/// word is left unexpanded, exactly as it was before the splice was tried.
fn splice_alias(
    view: &AliasView,
    st: &mut AliasScan,
    name: &Str,
    val: &[u8],
    at: core::ops::Range<usize>,
    out: &mut AliasOut,
    opts: ParseOpts,
) -> Option<AliasView> {
    let head = st.head;
    let core::ops::Range { start: word, end: cut } = at;
    let mut text: Vec<Ch> = view.text.get(..word)?.to_vec();
    let vlen = bytes::chars(val).count();
    text.extend(bytes::chars(val));
    text.extend_from_slice(view.text.get(cut..).unwrap_or(&[]));
    let src = u32::try_from(out.bodies.len().saturating_add(1)).ok()?;
    let map = view.map.spliced(word, cut, vlen, src);
    // `dparens` is dropped: it indexes the *spliced* text, whose offsets this
    // pass immediately re-labels through `map` into the alias source table, so a
    // copy pushed back inside a replacement cannot be named in that table's terms.
    // The line freeze it also carries is applied by the lex that produced it, so
    // only the echoed text of a `((` written inside an alias value is affected.
    let Spanned { toks, lines, starts, ends, raws, taken, warnings, warn_from, dparens: _ } =
        Lexer::spliced(text.clone(), map.clone(), head, opts).run().ok()?;
    // bash's `pop_string` restores the displaced line at the saved index, which
    // is what a diagnostic raised inside the value points back at.
    let (psrc, poff) = view.map.at(cut);
    out.bodies.push(AliasBody {
        text: val.to_vec(),
        parent: TokSpan { src: psrc, end: u32::try_from(poff).unwrap_or(u32::MAX) },
    });
    // What the splice displaced moved along by the difference in length; what
    // stands in front of it did not move at all.
    let moved = word.saturating_add(vlen);
    let shift = |off: usize| {
        if off <= word { off } else { moved.saturating_add(off.saturating_sub(cut)) }
    };
    for p in &mut st.pushes {
        p.start = shift(p.start);
        p.end = shift(p.end);
    }
    for m in &mut st.marks {
        m.0 = shift(m.0);
    }
    st.pushes.push(AliasPush {
        name: name.clone(),
        start: word,
        end: moved,
        expand_next: val.ends_with(b" ") || val.ends_with(b"\t"),
    });
    Some(AliasView { toks, lines, starts, ends, raws, taken, warnings, warn_from, text, map })
}

/// Shift every source line recorded *inside* a token by `by` lines.
///
/// The alias pass reads a text bash's reader assembled — a value followed by the
/// tail of the calling line — and that text numbers its own lines from 1. For a
/// token read out of the tail the difference from the script's numbering is a
/// constant, so the lines it carries as payload move by the same amount its own
/// constant. Text read out of a *value* is all on the alias word's line, and
/// gets the shift that puts it there.
fn shift_tok_lines(tok: &mut Tok, by: i64) {
    if by == 0 {
        return;
    }
    let shift = |l: &mut u32| {
        *l = u32::try_from(i64::from(*l).saturating_add(by).max(1)).unwrap_or(u32::MAX);
    };
    walk_tok_lines(tok, &shift);
}

/// Apply `f` to every source line a token carries as payload: the line a
/// `$( … )` remembers its `)` on and the one a `<( … )` remembers its `(` on.
fn walk_tok_lines(tok: &mut Tok, f: &dyn Fn(&mut u32)) {
    match tok {
        Tok::Word(segs) | Tok::HereDoc(segs, ..) => walk_seg_lines(segs, f),
        Tok::ArrayAssign { elems, .. } => {
            for e in elems {
                walk_seg_lines(e, f);
            }
        }
        _ => {}
    }
}

fn walk_seg_lines(segs: &mut [Seg], f: &dyn Fn(&mut u32)) {
    for seg in segs {
        match seg {
            Seg::CmdSub(_, close, _) => f(close),
            Seg::ProcSub(_, _, open) => f(open),
            Seg::Dq(inner) => walk_seg_lines(inner, f),
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

/// True for a word `read_token_word` hands back as something other than a WORD
/// when a redirection operator follows it: an IO number (`2>f`), or a `{name}`
/// file-descriptor variable (`{v}>f`).
///
/// Neither is a word *of* the command — both belong to the redirection — so
/// neither ends a leading run of redirections, which is what keeps
/// `2>/dev/null h[1 2]=v` in command position. See
/// [`CaseScan::finish_word`].
fn is_redir_prefix(w: &[u8]) -> bool {
    if !w.is_empty() && w.iter().all(u8::is_ascii_digit) {
        return true;
    }
    w.strip_prefix(b"{")
        .and_then(|r| r.strip_suffix(b"}"))
        .is_some_and(is_valid_name)
}

/// True when a `${ … }` body read so far is still the parameter *name* —
/// bash's `dolbrace_state == DOLBRACE_PARAM`, for the one question that asks
/// it: whether a `[` opens a subscript to jump over on the re-read. See
/// [`ParseOpts::reread`].
///
/// An identifier, optionally behind the `!` of an indirection. `!` is in none
/// of the operator sets the state machine tests, so it leaves the state alone;
/// every other prefix that could stand here (`#`, `%`, `^`, `,`, …) is in the
/// first set it tests and moves the scan out of the name.
fn is_brace_name_so_far(raw: BStr<'_>) -> bool {
    is_valid_name(raw.strip_prefix(b"!").unwrap_or(raw))
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
    /// True when a leading assignment word (`name=…`, `name[sub]=…`, `name+=…`)
    /// would be accepted where the scan now stands — bash's
    /// `assignment_acceptable`, which is the only place its tokenizer slurps an
    /// unquoted-space array subscript (`h[a b]=v`).
    ///
    /// The answer is the carried [`CmdPos`], not a look back at `out`: whether a
    /// word was a reserved word, an assignment or an ordinary word depends on
    /// where it *sat* when it was read, and nothing of that survives into the
    /// `Tok`. Reading it back is what osh used to do, and it got two things
    /// wrong that bash's own state does not — a leading run of redirections
    /// still being command position (`>f g[1 2]=v`), and an assignment-shaped
    /// word *past* the command word not being an ASSIGNMENT_WORD at all
    /// (`printf %s x=1 g[1 2]=v` is three words).
    ///
    /// `out` is append-only within a scan, so the state is folded forward over
    /// whatever has been pushed since the last question and the cursor moved up.
    fn assignment_acceptable(&mut self, out: &[Tok]) -> bool {
        self.fold_cmd_pos(out).at_command()
    }

    /// Bring [`Lexer::cmd_pos`] up to date with everything pushed since it was
    /// last asked. `out` is only ever appended to within a scan, so folding
    /// lazily here costs the same as folding at every push and keeps the
    /// bookkeeping in one place.
    fn fold_cmd_pos(&mut self, out: &[Tok]) -> CmdPos {
        for tok in out.get(self.cmd_pos_upto..).unwrap_or_default() {
            let was_cmd = self.cmd_pos.at_command();
            self.cmd_pos.advance(tok, was_cmd);
        }
        self.cmd_pos_upto = out.len();
        self.cmd_pos
    }

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

    /// The extents of every copy this lex pushed back, in push order.
    fn dparen_copies(&self) -> Vec<DparenCopy> {
        self.dparens
            .iter()
            .map(|p| DparenCopy {
                start: u32::try_from(p.start).unwrap_or(u32::MAX),
                end: u32::try_from(p.end).unwrap_or(u32::MAX),
            })
            .collect()
    }

    fn run(&mut self) -> Result<Spanned, LexError> {
        let mut toks = Vec::new();
        let mut m = Marks::default();
        self.run_into(&mut toks, &mut m)?;
        Ok(Spanned {
            toks,
            lines: m.lines,
            starts: m.starts,
            ends: m.ends,
            raws: m.raws,
            taken: core::mem::take(&mut self.taken),
            warnings: core::mem::take(&mut self.warnings),
            warn_from: core::mem::take(&mut self.warn_from),
            dparens: self.dparen_copies(),
        })
    }

    /// Tokenize the whole input into `out`/`marks`, keeping whatever was lexed
    /// before an error. Split out of [`Lexer::run`] so [`tokenize_deferred`] can
    /// hold on to the good prefix: bash executes every complete line preceding
    /// an unterminated construct before reporting it.
    fn run_into(&mut self, out: &mut Vec<Tok>, marks: &mut Marks) -> Result<(), LexError> {
        loop {
            // Input the body reader already ate is not there to be parsed: the
            // token cursor, arriving at the end of the pushed alias value, finds
            // the raw input already advanced past those lines. See [`Lexer::raw`].
            if self.pos >= self.raw_from && self.pos < self.raw && self.map.at(self.pos).0 == 0 {
                self.pos = self.raw;
            }
            // Before the blank skip: a blank is what *delimits* the owed word,
            // so it has to be seen rather than stepped over.
            if self.nul_word {
                self.take_nul_word(out, marks)?;
                continue;
            }
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
                self.stamp_lines(out, marks, start_line, start_pos);
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
                    // A `((` copy that ends on this newline is exhausted by
                    // reading it, and what the reader finds next is not input
                    // but the popped buffer's own NUL. See
                    // [`Lexer::take_nul_word`].
                    self.nul_word = self.dparens.iter().any(|p| p.end.saturating_add(1) == self.pos);
                    // Before the collection below, so a here-document still
                    // pending from earlier on the line reads from *after* the
                    // bodies a `$( … )` on it already took, not from the top of
                    // them again.
                    self.sync_ahead();
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
                    //
                    // bash gates the arithmetic reading on
                    // `reserved_word_acceptable (last_read_token)`
                    // (`parse_dparen`, parse.y:4484), which [`CmdPos`] models
                    // in full — including the freeze that makes no paren inside
                    // `[[ … ]]` arithmetic however deeply nested, so that
                    // `[[ (( 0 )) ]]` tests the *string* `0` rather than
                    // evaluating it.
                    if self.peek() == Some('(') && self.arith_cmd_position(out) {
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
                            (Some(raw), nested) => out.push(Tok::ArithCmd(raw, nested)),
                            (None, _) if for_header => {
                                // …and that error is reported by *position*, not
                                // by name: `parse_dparen` returns −1 and bison
                                // errors on what it reads as EOF. See
                                // [`Tok::Refused`]. The one character the
                                // adjacency test read is consumed here as
                                // `shell_getc` consumed it, so the token ends
                                // where `shell_input_line_index` is parked and
                                // the slice around it comes out of the same
                                // offset bash's does.
                                self.pos = self.pos.saturating_add(1).min(self.chars.len());
                                out.push(Tok::Refused);
                                self.refused = true;
                            }
                            (None, _) => {
                                // The spans are dropped with the text they index
                                // into. bash *did* parse those bodies before
                                // reaching this point, but the same text is about
                                // to be read again as `( ( … ) )`, and the
                                // ordinary command parser parses each body a
                                // second time — so a body that does not parse is
                                // still fatal, and one that does is re-printed by
                                // `unparse` from the node it produced. Nothing is
                                // lost by letting the re-read be the one that
                                // counts.
                                //
                                // What *is* lost by re-reading the physical text
                                // is that bash re-reads a **copy** — the rebuilt
                                // string it pushed — and a pushed string is not
                                // input: no line is fetched for it, so every line
                                // inside it is blamed on the line the scan gave
                                // up on. Record the copy's extent and that line
                                // so [`Lexer::stamp_lines`] can freeze it. The
                                // cursor sits *at* the rejected character, which
                                // is the copy's last, so the copy is exactly
                                // `chars[arith_from ..= self.pos]`.
                                self.dparens.push(DparenPush {
                                    start: arith_from,
                                    end: self.pos,
                                    line: self.cur_line(),
                                    eof_charge: matches!(self.peek(), None | Some('\n')),
                                });
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
                        let assign_ok = self.assignment_acceptable(out);
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
            self.stamp_lines(out, marks, start_line, start_pos);
            // Nothing after a refusal is ever read — see [`Lexer::refused`].
            // Not even the closing newline below: the reader that would have
            // fetched it is the parser asking for another token, and the parser
            // has already errored.
            if self.refused {
                return Ok(());
            }
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
        // Each of the three tails below is an iteration of its own — it appends
        // tokens and closes with `stamp_lines`, which takes `self.line` as the
        // line the iteration *began* on. So the iteration mark has to move with
        // it, or [`Lexer::cur_line`] adds the previous iteration's newlines to a
        // `self.line` that has already counted them.
        self.iter_start = self.pos;
        if self.paren_body {
            let start_pos = self.pos;
            if !self.pending_heredocs.is_empty() {
                self.collect_heredocs(out)?;
            }
            out.push(Tok::Op(Op::RParen));
            self.stamp_lines(out, marks, self.line, start_pos);
        } else if !matches!(out.last(), None | Some(Tok::Newline)) && !self.reader_at_eof() {
            let start_pos = self.pos;
            out.push(Tok::Newline);
            if !self.pending_heredocs.is_empty() {
                self.collect_heredocs(out)?;
            }
            self.stamp_lines(out, marks, self.line, start_pos);
        } else if !self.pending_heredocs.is_empty() {
            let start_pos = self.pos;
            self.collect_heredocs(out)?;
            self.stamp_lines(out, marks, self.line, start_pos);
        }
        Ok(())
    }

    /// Whether bash's reader has run out of input entirely, rather than merely
    /// reached the end of a last line it still has to close.
    ///
    /// `shell_getc` gives a line that came without a newline one of its own
    /// (parse.y:2567), which is why a script with no final newline parses as
    /// though it had one — see [`crate::parser::close_last_line`], which is where
    /// that writing-back is now done. But a `\<newline>` it *deleted* is a line
    /// it has already consumed: the `goto restart_read` that follows the deletion
    /// fetches again, and if the deleted newline was the input's last that fetch
    /// returns end of file. There is no line left to close and so no newline to
    /// hand over — `read_token` returns `yacc_EOF` instead:
    ///
    /// ```text
    /// echo 1⏎cat >\⏎      line 3: syntax error: unexpected end of file
    /// echo 1⏎cat >⏎       line 2: syntax error near unexpected token `newline'
    /// ```
    ///
    /// The test is the deletion and not just the text: an input ending in
    /// `\\<newline>` ends in a real newline — the first backslash quotes the
    /// second — and that newline is a token, so the question never arises.
    ///
    /// The other way to run out is to have been given no newline in the first
    /// place. A *string* whose last line ends on an odd run of backslashes is
    /// closed with another backslash rather than with a newline, because a
    /// newline would be eaten as a continuation — see
    /// [`crate::parser::closed_with_backslash`]. There too the parser is handed
    /// the end-of-file token, which is why `bash -c 'case x in  \'` reports an
    /// unexpected *end of file* where `bash -c 'case x in  '` reports an
    /// unexpected `newline`.
    fn reader_at_eof(&self) -> bool {
        let n = self.chars.len();
        if self.pos < n {
            return false;
        }
        if crate::parser::closed_with_backslash(&self.chars) {
            return true;
        }
        // `\<newline>`, or the `\<CR><LF>` a CRLF file writes.
        [2usize, 3].into_iter().any(|len| {
            n.checked_sub(len).is_some_and(|at| {
                self.cont_len_at(at) == Some(len)
                    && self.conts.contains(&u32::try_from(at).unwrap_or(u32::MAX))
            })
        })
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
        // A reader that ran out on a deleted `\<newline>` ([`Lexer::reader_at_eof`])
        // has no newline token left to gather at, so the gather that raises this
        // warning can only be the one in the `simple_list` reduction — and that
        // reduction is driven by the end-of-file token, which the parser has to
        // *ask* for. The request finds the buffer used up and enters
        // `shell_getc`'s fetch block, whose first statement is `line_number++`
        // (parse.y:2361). So the number this warning carries is one past the
        // cursor's own line: `cat <<x\` at the end of a two-line script is
        // blamed on line 4.
        //
        // Two past it when the reader ran out on a backslash *close* instead of
        // on a deleted continuation. Both end the input, but the continuation
        // left its newline in the text and `cur_line` counts that character,
        // while a backslash-closed string has no newline anywhere; the second
        // line here is the fetch the last token's own scan made, which the
        // continuation case has already been charged for by that newline.
        if self.reader_at_eof() {
            let no_newline = u32::from(crate::parser::closed_with_backslash(&self.chars));
            return self.cur_line().saturating_add(1).saturating_add(no_newline);
        }
        let at_line_start = self.pos == 0 || self.at(self.pos.wrapping_sub(1)) == Some('\n');
        self.cur_line()
            .saturating_sub(u32::from(at_line_start))
            .max(1)
    }

    /// Emit the word a `((` copy's terminating newline owes, swallowing whatever
    /// follows it without a delimiter in between.
    ///
    /// `pop_string` restores the reader's saved index into the physical line
    /// (parse.y:2667-2669), and for a copy whose last character was that line's
    /// newline the saved index is the buffer's own terminating NUL — which
    /// `shell_getc`'s pop path hands back as if it were input. `read_token_word`
    /// takes it for the start of a word, so the token buffer opens with a NUL and
    /// the word's value is the empty string however much text is appended after
    /// it. Everything up to the next delimiter is therefore read *and lost*:
    ///
    /// ```text
    /// ((:)<nl>echo one two)      one: command not found   ("echo" swallowed)
    /// ((:)<nl> echo one two)     one two                  (a blank delimits it)
    /// ((:)<nl>$(exit 9))         rc=0                     (never performed)
    /// ((:)<nl>#foo bar)          bar: command not found   (no comment starts)
    /// ```
    ///
    /// The word is unquoted and empty, so splitting removes it and it does not
    /// become an `argv[0]` of its own — a copy followed by nothing but its `)`
    /// leaves a command with no words at all, whose status is 0. That is the
    /// same extra read the end-of-input floor is charged for
    /// ([`DparenPush::eof_charge`]), seen in `$?` instead of in a line number.
    ///
    /// Losing the value is not the same as not reading the text. The run is
    /// delimited exactly as any word is — quotes, `$( … )` and `\<newline>` all
    /// hold it together — and a `$( … )` in it is *parsed* where the scan meets
    /// it, so `((:)<nl>$(fi))` is a syntax error even though nothing would ever
    /// have run. So the run is read as the word it is and the NUL is written in
    /// front of it, which is literally what bash's token buffer holds; the cut
    /// that makes the value empty is then the ordinary one every word gets —
    /// `make_word`'s `savestring`, modelled by `word_expanded_from_its_text`.
    fn take_nul_word(&mut self, out: &mut Vec<Tok>, marks: &mut Marks) -> Result<(), LexError> {
        self.nul_word = false;
        let start_line = self.line;
        let start_pos = self.pos;
        self.iter_start = start_pos;
        self.next_tok_index = out.len();
        let mut segs = vec![Seg::Lit(vec![0])];
        // bash's word delimiters: end of input, a shell blank, or a shellmeta.
        // A `#` is not one of them — a comment only opens at the start of a
        // word, and the NUL already started this one.
        if !matches!(
            self.peek(),
            None | Some(' ' | '\t' | '\n' | '\r' | '|' | '&' | ';' | '(' | ')' | '<' | '>')
        ) {
            segs.extend(self.read_word(false)?);
        }
        out.push(Tok::Word(segs));
        self.stamp_lines(out, marks, start_line, start_pos);
        Ok(())
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
    fn stamp_lines(&mut self, out: &[Tok], marks: &mut Marks, start_line: u32, start_pos: usize) {
        let Marks { lines, starts: offsets, ends, raws } = marks;
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
        // A token re-read out of a `((`'s pushed copy is blamed on the line the
        // abandoned scan gave up on, not on the line it physically sits on: the
        // copy is a string, and reading a string fetches no input line. See
        // [`DparenPush`]. Every copy ends at its own scan's last line, so the
        // frozen line is never below the physical one and the resync after the
        // copy is exhausted needs nothing — `self.line` has been advancing
        // underneath all along. Nested copies are read while the enclosing one
        // is still frozen, which is what taking the widest containing push
        // reproduces.
        for p in &self.dparens {
            if (p.start..=p.end).contains(&start_pos) {
                end_line = end_line.max(p.line);
            }
        }
        let start = u32::try_from(start_pos).unwrap_or(u32::MAX);
        while lines.len() < out.len() {
            lines.push(end_line);
        }
        while offsets.len() < out.len() {
            offsets.push(start);
        }
        // …except a here-document delimiter, which is a word of its own and has
        // to be replaceable on its own. See [`Lexer::lex_heredoc_op`]. Still "at
        // or before its first character", so [`Spanned::starts`]'s contract holds.
        if let Some((i, at)) = self.hd_delim.take()
            && let Some(slot) = offsets.get_mut(i)
        {
            *slot = at;
        }
        // Every token this iteration produced shares the iteration's span, so
        // they all end where the cursor now stands. For the `Newline` that
        // triggered here-document collection that is past the collected bodies,
        // which is exactly the property the history slicer relies on.
        let end = u32::try_from(self.pos).unwrap_or(u32::MAX);
        while ends.len() < out.len() {
            ends.push(end);
        }
        // Where the body reader stands now — past anything this iteration's own
        // here-documents took. See [`Spanned::raws`].
        let raw = u32::try_from(self.raw).unwrap_or(u32::MAX);
        while raws.len() < out.len() {
            raws.push(raw);
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

    /// Whether a `((` standing here opens an arithmetic command rather than two
    /// nested subshells.
    ///
    /// bash reads `((` as an `ARITH_CMD` only where a reserved word would be
    /// recognised — the start of the input, after a separator, and after the
    /// words that open or close a compound command. Anywhere else it hands back
    /// a single `(` and reads the second one again as the next token, so
    /// `echo ((1))` is a syntax error near `(` and not an arithmetic command. An
    /// assignment prefix blocks it too (`x=1 ((2))` is the same error), which
    /// falls out of the same rule: bash does not recognise a reserved word after
    /// one either. The one place that is *not* the rule is an arithmetic `for`
    /// header, which `parse_dparen` tries first — see [`Prev::For`].
    ///
    /// The same fold answers this and the subscript question, so both go through
    /// [`Lexer::fold_cmd_pos`].
    fn arith_cmd_position(&mut self, out: &[Tok]) -> bool {
        self.fold_cmd_pos(out).arith_ok()
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
                    segs.push(Seg::CmdSub(raw, close, SubBody::Backtick(src)));
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
    ///
    /// `unread` names stretches of the input — as character index ranges — that
    /// reached this word by a *splice* rather than by being read: bash's bare
    /// `$'…'` translation (parse.y:3887), whose bytes the brace scan wrote into
    /// the operand without any parser ever passing over them. Inside one, this
    /// scan is not a read either, so a `$( … )` there comes out
    /// [`crate::ast::CmdSubBody::Unread`] — the same standing the `' … '` run
    /// below gives what it covers. See [`Lexer::bare_splices`] and
    /// [`crate::parser::operand_from_source`].
    fn read_word_verbatim(
        &mut self,
        mode: Verbatim,
        unread: &[core::ops::Range<usize>],
    ) -> Result<Vec<Seg>, LexError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = Str::new();
        // Where the `' … '` run the cursor is inside ends, in [`Verbatim::Dquote`]
        // — see the second `'` arm below — and whether the cursor is inside one
        // at all (a run with no mate reaches the end of the text, so `sq_close`
        // being `None` does not mean the run is over).
        let mut sq_close: Option<usize> = None;
        let mut in_run = false;
        // What [`Lexer::here_text`] was before any of this: outside both a run
        // and a splice, the answer is still whatever the caller set.
        let outside = self.here_text;
        while let Some(c) = self.peek() {
            // The run's own closing quote: a literal like its mate, and past it
            // a parser was reading again.
            if sq_close == Some(self.pos) {
                sq_close = None;
                in_run = false;
                lit.push(b'\'');
                self.pos += 1;
                continue;
            }
            if !in_run {
                self.here_text = outside || unread.iter().any(|r| r.contains(&self.pos));
            }
            match c {
                // Inside quotes a `'` opens nothing — it is a character like any
                // other, and `"${nope:-'a b'}"` keeps both of them.
                '\'' if mode != Verbatim::Dquote => {
                    flush_lit(&mut segs, &mut lit);
                    self.pos += 1;
                    let s = self.read_single_quote()?;
                    segs.push(Seg::Sq(s, false));
                }
                // …but bash's *parser* never read what lies between one and its
                // mate. A `${ … }` is a grouping construct (`open != close`), so
                // `parse_matched_pair` meets the `'` as one of that construct's
                // shell quotes and hands the run to a `parse_matched_pair ('\'',
                // '\'', '\'', …)` of its own (parse.y:3840-3846), which reads to
                // the mate and reads *nothing* in between. The rule that would
                // instead have stepped over the `'` and left the run's characters
                // to the outer scan is the posix one (parse.y:3836), and it is
                // off here.
                //
                // So the characters stay live — the enclosing double quotes are
                // still in force and the operand is expanded with a `'` for an
                // ordinary character — while nothing in the run was read *as
                // source*: a `$( … )` here is [`SubBody::Unread`], exactly as one
                // in a here-document body is. Two things follow, both measured
                // against bash 5.2.37: `declare -f` prints such an operand back
                // as written rather than re-printed, and a body that does not
                // parse is not a script syntax error — bash meets that one only
                // later, when `brace_gobbler` scans the raw word text
                // (braces.c:646-683).
                '\'' if mode == Verbatim::Dquote => {
                    // Without a mate the run is the rest of the text — what a
                    // `read_single_quote` that ran out would have taken.
                    sq_close = (self.pos + 1..self.chars.len()).find(|&i| self.at(i) == Some('\''));
                    in_run = true;
                    self.here_text = true;
                    lit.push(b'\'');
                    self.pos += 1;
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
                    segs.push(Seg::CmdSub(raw, close, SubBody::Backtick(src)));
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
                '$' => match self.read_dollar(false) {
                    Ok(Some(seg)) => {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    }
                    Ok(None) => lit.push(b'$'),
                    // A construct left open in text that reached the shell as a
                    // *value* is a runtime failure, not a lexing one — this loop
                    // included, which reads the operand of a `${ … }`. bash has
                    // no operand re-lex at all: the operand of a
                    // `${x:-w}` is handed straight back to `expand_word_internal`
                    // (`parameter_brace_expand_word`, subst.c:7078), so a
                    // construct left open in it fails exactly where one left open
                    // in the enclosing string does — at expansion, not at a scan.
                    // Measured: `a='A${x:-$[ 1 + 2 }B'; "${a@P}"` is `A$[ 1 + 2 B`
                    // in bash, the operand keeping itself whole
                    // ([`crate::interp::Shell::expand_unclosed`]); it is only an
                    // error when a *parser* read the same text, and
                    // [`Lexer::unclosed_seg`] keeps that case an error by only
                    // answering to a carrier [`Lexer::unread_eof`] raised.
                    Err(e) => {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(self.unclosed_seg(e)?);
                        self.here_text = outside;
                        return Ok(segs);
                    }
                },
                _ => self.take_into(&mut lit),
            }
        }
        // A run with no mate — or a splice that runs to the end — reaches here
        // still open; the flag is the cursor's, not the text's, so it does not
        // outlive the scan.
        self.here_text = outside;
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
        // The line the outermost `[` stood on, for the diagnostic if it never
        // closes. bash reports a matched pair at its *opening* line —
        // `parser_error (start_lineno, …)` with `start_lineno = line_number`
        // taken on entry (parse.y:3701, 3711) — so a subscript that runs off the
        // end over several lines still names the line it began on.
        let mut sub_line = 0u32;
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
                sub_line = self.cur_line();
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
                // Word level, and `read_token_word` pushes the `(` for a `<(`
                // like any other (parse.y:5071), so the body's delimiter is that
                // `(` and never an enclosing `"`.
                let raw = self.read_subst_body(false).map_err(|e| e.at(self.eof_line()))?;
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
                    segs.push(Seg::CmdSub(raw, close, SubBody::Backtick(src)));
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
        // A subscript in assignment position is one of the reader's matched
        // pairs, not text that happens to hold a `[`: bash reads it with
        // `parse_matched_pair (cd, '[', ']', &ttoklen, P_ARRAYSUB)` and, when
        // that comes back `&matched_pair_error`, does `return -1; /* Bail
        // immediately. */` (parse.y:5145-5149). So running off the end here is
        // the reader's `unexpected EOF while looking for matching `]'` — the
        // input up to the `[` has already been read and run, and everything
        // after it is swallowed by the search.
        if sub_depth > 0 {
            return Err(eof_matching(']').at(sub_line));
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
                // `string_extract_single_quoted` stops at the end of what it
                // was given and says nothing; only the parser, still hunting
                // the end of the word, calls that an error. See
                // [`ParseOpts::tolerant`].
                None if self.opts.tolerant => return Ok(s),
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
        Ok(crate::escape::ansi_c_unescape(&self.read_ansi_c_source()?))
    }

    /// [`Lexer::read_ansi_c_quote`]'s scan alone: the run's *undecoded* bytes,
    /// for the one caller that needs the translation's length as well as its
    /// text (the bare splice of `Lexer::read_dollar_brace_body`).
    fn read_ansi_c_source(&mut self) -> Result<Str, LexError> {
        let open = self.cur_line();
        let mut raw = Str::new();
        loop {
            let Some(c) = self.bump_ch() else {
                return Err(eof_matching('\'').at(open));
            };
            if c == '\'' {
                return Ok(raw);
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
    ///
    /// A third shape sits between the two: `closed` with
    /// [`ParseOpts::tolerant`], where a `"` still ends the run but so does the
    /// end of the input. That is `string_extract_double_quoted` itself, which
    /// is handed a finished word rather than a stream and has nothing to be
    /// short of.
    fn read_double_quote_until(&mut self, closed: bool) -> Result<Vec<Seg>, LexError> {
        let open = self.cur_line();
        let mut segs: Vec<Seg> = Vec::new();
        let mut lit = Str::new();
        loop {
            let Some(c) = self.peek() else {
                if closed && !self.opts.tolerant {
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
                    match self.read_backtick(true) {
                        Ok((raw, src)) => {
                            let close = self.cur_line();
                            segs.push(Seg::CmdSub(raw, close, SubBody::Backtick(src)));
                        }
                        Err(e) => {
                            segs.push(self.unclosed_seg(e)?);
                            return Ok(segs);
                        }
                    }
                }
                '$' => match self.read_dollar(true) {
                    Ok(Some(seg)) => {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(seg);
                    }
                    Ok(None) => lit.push(b'$'),
                    Err(e) => {
                        flush_lit(&mut segs, &mut lit);
                        segs.push(self.unclosed_seg(e)?);
                        return Ok(segs);
                    }
                },
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
    ///
    /// [`Lexer::ansi_c_quote`] blocks those two forms for a different reason and
    /// reaches the same place: an operand written in text no parser read is a
    /// word to *this* scan but was never one to bash's, so nothing in it was
    /// translated at parse time and `expand_word_internal`, which translates no
    /// `$'…'` of its own, meets the `$'` still spelled out. Measured:
    /// `cat <<E` / `${x:-$'a\x2Cb'}` prints `$'a\x2Cb'`, where
    /// `"${x:-$'a\x2Cb'}"` prints `a,b`. A **pattern** in a here-document is the
    /// exception, because its extraction is routed to
    /// `extract_heredoc_dolbrace_string`, which does the translation the read
    /// path skipped — see [`Lexer::ansi_c_quote`].
    fn read_dollar(&mut self, in_dquote: bool) -> Result<Option<Seg>, LexError> {
        let quote_form = !in_dquote && self.ansi_c_quote;
        // Where the construct starts, for a scan that runs off the end of text
        // no parser read — see [`Lexer::unread_eof`].
        let dollar = self.pos;
        // Consume the `$`. What follows it is judged after the reader's
        // deletions, so `$<backslash><newline>(` opens a substitution and
        // `$<backslash><newline>{` a braced parameter. See [`Lexer::eat_conts`].
        self.adv();
        match self.peek() {
            Some('\'') if quote_form => {
                // `$'…'` — ANSI-C quoting: a literal string with backslash
                // escapes processed (no expansion/splitting — like `'…'`).
                self.pos += 1;
                let s = self.read_ansi_c_quote()?;
                Ok(Some(Seg::Sq(s, false)))
            }
            Some('"') if quote_form => {
                // `$"…"` — locale translation. We have no message catalogs, so
                // it behaves as a plain double-quoted string (bash's fallback).
                self.pos += 1;
                let inner = self.read_double_quote()?;
                Ok(Some(Seg::Dq(inner)))
            }
            Some('{') => {
                self.pos += 1;
                let open = self.cur_line();
                // bash's `P_DQUOTE` for the body: `read_token_word` hands the
                // `${` the delimiter it is standing in (`parse_matched_pair (cd,
                // '{', '}', …)`, parse.y:5033), and `rflags` is `P_DQUOTE` when
                // that delimiter is a `"` (parse.y:3696).
                // The splices come back scoped to this `${ … }` and measured
                // against its raw text — a splice at any depth inside, in a
                // `"…"` run in the body or in a body nested in that one, is
                // this segment's, because it is this segment's raw text the
                // leftover has to be carved out of.
                let (raw, nested, spliced) =
                    self.read_dollar_brace(in_dquote).map_err(|e| self.unread_eof(e, '}', dollar, true))?;
                Ok(Some(Seg::ParamBraced(raw, open, nested, spliced)))
            }
            Some('[') => {
                // `$[ … ]` — the deprecated (pre-`$(( ))`) arithmetic expansion.
                // bash still accepts it as an alias for `$(( … ))`.
                self.pos += 1;
                let open = self.cur_line();
                let outer = std::mem::take(&mut self.arith_comsubs);
                // An arithmetic body is a segment of its own, and nothing asks
                // it which of its bytes a splice wrote — so the collection is
                // set aside and dropped rather than joining the enclosing
                // word's. See [`Lexer::bare_splices`].
                let outer_splices = std::mem::take(&mut self.bare_splices);
                let raw = self
                    .read_balanced('[', ']')
                    .map_err(|e| self.unread_eof(e.at(open), ']', dollar, false))?;
                let nested = std::mem::replace(&mut self.arith_comsubs, outer);
                self.bare_splices = outer_splices;
                Ok(Some(Seg::Arith(raw, true, nested)))
            }
            Some('(') => {
                if self.at(self.cont_skip(self.pos + 1)) == Some('(') {
                    // `$((` is ambiguous: arithmetic, or a substitution whose body
                    // opens with a parenthesised group (`$(( cmd ) | cmd )`, which
                    // bash runs). bash settles it in *two* steps, and keeping them
                    // apart is the only way to agree with it.
                    //
                    // The first is this scan, which only finds the `)`. Seeing a
                    // second `(`, `parse_comsub` returns straight into
                    // `parse_matched_pair` with `P_ARITH` (parse.y:4103) — a
                    // character scan, above the `need_here_doc = 0` and nowhere
                    // near a nested parse. So the body is never parsed as commands
                    // here, a `<<` in it declares nothing, and pending
                    // here-documents survive: `cat <<E $((`, `cat <<E $(( 1 +` and
                    // `cat <<E $(( echo a )` all warn about `E`, while `cat <<E $(`
                    // does not. Blamed on the opening line for the same reason —
                    // the scan starts and fails in one place, with no re-parse
                    // afterwards to move the line counter on.
                    //
                    // The second step is [`is_arith_expr`], and it does not happen
                    // until expansion in bash — `param_expand` (subst.c:10580)
                    // strips the parens and asks `chk_arithsub` whether what is
                    // left reads as an expression, falling back to
                    // `command_substitute` when it does not. osh decides here
                    // instead, which is safe because the test is a property of
                    // the text and nothing else — and because the decision does
                    // not stick: both arms re-derive the extent at expansion time
                    // through the same paren count, so a count that stops
                    // somewhere the balance above did not takes the arm the
                    // *extent* calls for. See [`crate::interp::Shell::
                    // arith_extent_route`] and its two callers.
                    let open = self.cur_line();
                    self.adv();
                    let outer = std::mem::take(&mut self.arith_comsubs);
                    // Set aside and dropped, as for `$[ … ]` above.
                    let outer_splices = std::mem::take(&mut self.bare_splices);
                    let raw = self
                        .read_balanced('(', ')')
                        .map_err(|e| self.unread_eof(e.at(open), ')', dollar, true))?;
                    let nested = std::mem::replace(&mut self.arith_comsubs, outer);
                    self.bare_splices = outer_splices;
                    // `chk_arithsub`'s own preamble: the body must open with the
                    // `(` this scan counted and close with its mate.
                    if let Some(expr) = raw
                        .strip_prefix(b"(")
                        .and_then(|e| e.strip_suffix(b")"))
                        .filter(|e| is_arith_expr(e))
                    {
                        // The expression is `raw` minus the `(` this scan
                        // counted, so every range slides one byte left with it.
                        let nested = nested
                            .into_iter()
                            .map(|s| CmdSubSpan {
                                range: s.range.start.saturating_sub(1)
                                    ..s.range.end.saturating_sub(1),
                                ..s
                            })
                            .collect();
                        return Ok(Some(Seg::Arith(expr.into(), false, nested)));
                    }
                    // The nested bodies travel with the fallback too: the eager
                    // parse happened during the scan, and so before the check that
                    // sent the text this way. `echo $(( echo $(fi) ) )` runs as a
                    // command substitution *and* still dies on the `fi`.
                    Ok(Some(Seg::CmdSub(
                        raw,
                        self.cur_line(),
                        SubBody::ArithFallback(nested),
                    )))
                } else {
                    self.pos += 1;
                    // `$( … )` is the one construct bash blames on the *end* of
                    // input rather than its opening line: the body is re-parsed
                    // after the outer scan, by which point the line counter has
                    // moved on. (An unterminated quote *inside* the body still
                    // reports its own line — `at` will not overwrite.)
                    //
                    // The body stands in this scan's delimiter: bash pushes a
                    // `(` for a `$(` it meets in `read_token_word` (parse.y:5041)
                    // but none for one inside a `"…"` (parse.y:3960), so only the
                    // unquoted one gets a delimiter of its own. A here-document
                    // body has no delimiter stack at all — its substitutions are
                    // parsed at expansion time, from a fresh reader — so a `"`
                    // this scan is nominally inside is not one of bash's. See
                    // [`Lexer::here_text`].
                    let body = self.pos;
                    let raw = match self.read_subst_body(in_dquote && !self.here_text) {
                        Ok(raw) => raw,
                        // In unread text there is no scan to fail: `$(` hands
                        // `extract_command_subst` the rest of the string and
                        // `xparse_dolparen` decides where the body stops, which
                        // for a `$(` with no mate is the end of it. See
                        // [`SubBody::Unread`].
                        Err(e) if self.unread_comsub(&e) => {
                            let src = self.slice(body, self.chars.len());
                            self.pos = self.chars.len();
                            let close = self.cur_line();
                            let kind = SubBody::Unread { closed: false };
                            return Ok(Some(Seg::CmdSub(src, close, kind)));
                        }
                        Err(e) => return Err(e.at(self.eof_line())),
                    };
                    let kind = self.subst_kind();
                    Ok(Some(Seg::CmdSub(raw, self.cur_line(), kind)))
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

    /// The whole of the text this lexer is reading — bash's `string`, which is
    /// what the "no closing" diagnostics echo back. See [`Unclosed::BadSubst`].
    fn whole_text(&self) -> Str {
        self.slice(0, self.chars.len())
    }

    /// Reconsider a scan that ran out of input, when the text being scanned is
    /// text no parser read.
    ///
    /// The failure is then not a lexing one at all: it is
    /// `extract_dollar_brace_string`'s or `extract_delimited_string`'s, raised
    /// when the *word is expanded*. `close` is the delimiter this construct was
    /// looking for, and it is stamped over whatever an inner construct wanted —
    /// bash's brace and arithmetic scanners step over a nested quote or
    /// backquote with `string_extract`/`skip_*_quoted`, which run to the end of
    /// the text without a word, so it is the enclosing scan that reports.
    ///
    /// A `$( … )` inside is the exception, but only for a scan that reads one.
    /// `extract_delimited_string` parses a nested command substitution — with
    /// `extract_command_subst`, whose failure is its own and abandons this scan
    /// where it stands — under one condition:
    ///
    /// ```c
    ///   if ((flags & SX_COMMAND) && string[i] == '$' && string[i+1] == LPAREN)
    ///     { si = i + 2; t = extract_command_subst (…); }   /* subst.c:1429 */
    /// ```
    ///
    /// `$((` and `${` are read with `SX_COMMAND`; `$[` is not — subst.c:1303
    /// passes a bare `0` — so a `$(` inside one is just so much text and the
    /// `$[` keeps its own complaint. `command` is that flag. The error carrying
    /// a nested parse is the one carrying a [`SubstBail`], whose body is the
    /// text that parse was handed.
    fn unread_eof(&self, e: LexError, close: char, from: usize, command: bool) -> LexError {
        if !self.here_text || e.looking_for.is_none() {
            return e;
        }
        if let Some(bail) = e.bail.as_ref().filter(|_| command) {
            let src = bail.body.clone();
            return e.unclosed(UnreadEof::CmdSub(src));
        }
        let src = self.slice(from, self.chars.len());
        let what = Unclosed::BadSubst { close, src, text: self.whole_text() };
        e.unclosed(UnreadEof::Subst(what))
    }

    /// Take a scan's failure as the *segment* that carries it, for the three
    /// loops that read text no parser ever read — a double-quoted run
    /// ([`Lexer::read_double_quote_until`]), a here-document body, and the
    /// operand of a `${ … }` ([`Lexer::read_word_verbatim`]). A construct left
    /// open there is a
    /// runtime failure rather than a lexing one, so the scan keeps what it has
    /// and records the reporter; anything else is still an error. See
    /// [`UnreadEof`].
    fn unclosed_seg(&self, mut e: LexError) -> Result<Seg, LexError> {
        match e.unclosed.take().map(|b| *b) {
            Some(UnreadEof::Subst(u)) => Ok(Seg::Unclosed(u)),
            Some(UnreadEof::CmdSub(src)) => {
                Ok(Seg::CmdSub(src, self.cur_line(), SubBody::Unread { closed: false }))
            }
            None => Err(e),
        }
    }

    /// Whether a `$( … )` scan that ran out of input is one bash never made:
    /// in text no parser read, `extract_command_subst` simply takes the rest of
    /// the string for the body. See [`SubBody::Unread`].
    ///
    /// Only an *end of input* qualifies. A scan that stopped on something else
    /// found the trouble the same way bash's would and keeps its own error.
    fn unread_comsub(&self, e: &LexError) -> bool {
        self.here_text && e.looking_for.is_some()
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
        // An arithmetic scan carries no `P_DOLBRACE`, so it reads no `${ … }` of
        // its own and has nothing for the delimiter to decide.
        self.read_balanced_inner(open, close, false, false)
    }

    /// Which of the two `$( … )` variants this scan is producing: the ordinary
    /// eager one, or [`SubBody::Unread`] when the text being scanned is not one
    /// a parser ever read as a word. See [`Lexer::here_text`].
    fn subst_kind(&self) -> SubBody {
        if self.here_text { SubBody::Unread { closed: true } } else { SubBody::Eager }
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
    fn read_subst_body(&mut self, dq: bool) -> Result<Str, LexError> {
        // A body of commands is *text* to this scan: it is copied out and lexed
        // again later, and that re-lex meets every arithmetic expansion in it a
        // second time. So whatever nested substitutions the copy collects belong
        // to that parse and not to an arithmetic scan out here — otherwise
        // `$(( $(echo "$(( $(fi) ))") ))` would parse the innermost body twice.
        // See [`Lexer::arith_comsubs`].
        let outer = std::mem::take(&mut self.arith_comsubs);
        // The flag travels *into* the body, because this scan is only finding the
        // `)`: a substitution written in text no parser read is itself unread,
        // and what has to survive to expansion time is its source. The body is
        // read as a word then — `command_substitute` parses it, `$'…'` and all —
        // and that read starts from the source this collects. See
        // [`Lexer::here_text`] and [`crate::ast::CmdSubBody::Unread`].
        // The body is re-read as a word at expansion time, from this source, so
        // which of its bytes a splice wrote is that read's question and not this
        // one's. See [`Lexer::bare_splices`].
        let outer_splices = std::mem::take(&mut self.bare_splices);
        let r = self.read_balanced_inner('(', ')', true, dq);
        self.arith_comsubs = outer;
        self.bare_splices = outer_splices;
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

    /// The scan proper is [`Lexer::read_balanced_body`]; this wraps it to hang
    /// the unparsed body off an error that ran out of input, for a substitution
    /// body only. See [`SubstBail`].
    ///
    /// It is done on the way *out* rather than at the end-of-input site so that
    /// the **outermost** substitution wins: a nested `$(` bails first with its
    /// own smaller body, and each enclosing scan replaces it as the error
    /// unwinds. That is the order bash blames in, because its nested parse of
    /// the outer body is what reaches the inner `$(` in the first place —
    /// `echo $(fi; $(done` is ``near `fi'``, not ``near `done'``.
    fn read_balanced_inner(
        &mut self,
        open: char,
        close: char,
        command: bool,
        dq: bool,
    ) -> Result<Str, LexError> {
        if !command {
            return self.read_balanced_body(open, close, command, dq);
        }
        let body_start = self.pos;
        let open_line = self.cur_line();
        self.read_balanced_body(open, close, command, dq).map_err(|mut e| {
            // Only an error that *is* the missing `)` — this scan's own
            // end-of-input, or one already carrying a bail from a substitution
            // inside it. An unterminated quote in the body is a different
            // failure and bash reports it as one: `echo $(echo "x` names the
            // `"`, because the nested parse's own lexer dies on it.
            if e.bail.is_none() && !(e.line.is_none() && e.looking_for == Some(close)) {
                return e;
            }
            let mut body = Str::new();
            for cx in self.chars.get(body_start..).unwrap_or_default() {
                cx.push_to(&mut body);
            }
            e.bail = Some(SubstBail { body, open_line });
            // Whatever construct inside the body noticed the end of input first,
            // this is now the `$( … )`'s failure, and bash reads *that* one with
            // a real parse whose diagnostic is its own. Drop any claim an inner
            // scan staked — see [`Unclosed`].
            e.unclosed = None;
            e
        })
    }

    /// Consume one span in which a closing delimiter closes nothing: a quoted
    /// string, a backslash escape, a backtick body, a `${ … }`. `Ok(false)`
    /// means `c` opened none of these, and nothing was consumed.
    ///
    /// bash reaches every one of these from `parse_matched_pair` whatever the
    /// grouping construct it is scanning: `shellquote (ch)` recurses for `'`,
    /// `"` and `` ` `` (parse.y:3844), `LEX_PASSNEXT` appends the character
    /// after a backslash and steps past it (parse.y:3741), and a `${` goes to
    /// `parse_dollar_word` (parse.y:3954). Not one of them is conditional on
    /// `P_COMMAND` or `P_ARITH`, which is why the *arithmetic* scan needs them
    /// exactly as much as the substitution one: in `$(( 1 + "2)3" ))` the
    /// quoted `)` closes nothing and bash evaluates `1 + 2)3`.
    ///
    /// `command` is bash's `P_COMMAND`: true when the text being scanned is a
    /// body of commands rather than an arithmetic expression. It gates the
    /// `${ … }` arm below, and travels to a nested `$( … )` met inside a
    /// double-quoted span, which is read by the balanced scan rather than
    /// skipped — see the comment on that arm.
    ///
    /// `dq` is bash's `current_delimiter (dstack)`, narrowed to the one question
    /// it decides: is the delimiter this scan is standing in a `"`? See
    /// [`Lexer::read_balanced_body`] for why it is not simply "inside quotes".
    fn read_opaque_span(
        &mut self,
        c: char,
        raw: &mut Str,
        command: bool,
        dq: bool,
    ) -> Result<bool, LexError> {
        match c {
            // `$'…'` is not a single-quoted run: a `\'` in it is an ANSI-C escape
            // and does not end it. bash reads one with `P_ALLOWESC` — the `$'…'
            // inside group' case of `parse_matched_pair` (parse.y:3847) — in
            // *every* grouping construct, gated on neither `P_COMMAND` nor
            // `P_ARITH`, so `$(echo $'a\'b')` and `$(( 0 ))` alike see the run
            // whole. Read as a plain quote instead, the run would end at the `\`
            // and the `'` after it would open another that never closes.
            //
            // And it does not travel: bash *translates* it here, in the parser,
            // and splices the result back over the `$'` it had already written
            // (`ansiexpand` then `retind -= 2`, parse.y:3858–3893). With no
            // `P_DQUOTE` — which no grouping construct's own scan carries — the
            // result is re-quoted by `sh_single_quote` (parse.y:3884), so what
            // the text downstream sees is an ordinary single-quoted run.
            //
            // That is not a formality. A single-quoted run is never valid
            // arithmetic, so `$(( $'5' ))` is an error in bash naming `'5'`;
            // the escapes are resolved before anything downstream could resolve
            // them differently (`$'a\0b'` is `'a'`, the NUL having ended the
            // string); and the substitution changes the text's *shape*, so a
            // `$'x\ny'` in a `$( … )` body puts a real newline in it and moves
            // every `$LINENO` after it down a line, exactly as bash's does.
            '$' if self.peek() == Some('\'') => {
                self.pos += 1;
                let s = self.read_ansi_c_quote()?;
                raw.extend_from_slice(&crate::escape::sh_single_quote(&s));
                Ok(true)
            }
            // `$"…"` is translated in the same place and the same way, by
            // `locale_expand` (parse.y:3901) — and re-quoted, with
            // `singlequote_translations` off by default, as a *double*-quoted
            // run (`sh_mkdoublequoted`, parse.y:3919). With no message catalogue
            // the translation is the identity, so the whole of it is the `$`
            // coming off: what is left is a double-quoted run like any other,
            // still expanded and still rescanned. `a=5; echo $(( $"a" ))` prints
            // 5 because arithmetic is handed `"a"`, and `$(( $"1+2" ))` is 3.
            '$' if self.peek() == Some('"') => {
                self.pos += 1;
                self.read_opaque_span('"', raw, command, dq)
            }
            '\'' => {
                let q_open = self.cur_line();
                raw.push(b'\'');
                // Copy verbatim to the closing single quote.
                loop {
                    match self.peek() {
                        Some('\'') => {
                            self.pos += 1;
                            raw.push(b'\'');
                            return Ok(true);
                        }
                        Some(_) => self.take_into(raw),
                        None => return Err(eof_matching('\'').at(q_open)),
                    }
                }
            }
            '"' => {
                let q_open = self.cur_line();
                raw.push(b'"');
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
                            self.take_into(raw);
                        }
                        Some('"') => {
                            self.pos += 1;
                            raw.push(b'"');
                            return Ok(true);
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
                            // The body's own eager parses join this scan's: a
                            // `${ … }` is read by `parse_matched_pair` too, so
                            // nothing in it is deferred by sitting one level in.
                            // This one is inside the double-quoted run, so it
                            // carries `P_DQUOTE` (parse.y:3696).
                            let (inner, nested, spliced) = self.read_dollar_brace(true)?;
                            raw.extend_from_slice(b"${");
                            // Shifted as the body is spliced: the ranges came
                            // out relative to `inner`, and from here on they
                            // have to name the same bytes of `raw`.
                            self.arith_comsubs.extend(shift_spans(nested, raw.len()));
                            self.bare_splices.extend(shift_ranges(spliced, raw.len()));
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
                            let inner = self.read_balanced_inner('(', ')', false, true)?;
                            raw.extend_from_slice(b"$(");
                            raw.extend_from_slice(&inner);
                            raw.push(b')');
                        }
                        // A nested substitution, read by this same scan so that
                        // its here-documents are gathered at *its* `)` like any
                        // other's. Recursing rather than counting depth in the
                        // quoted span is what keeps a bare `)` in the text — as
                        // in `"a)b"` — from closing anything.
                        //
                        // A body of commands whatever encloses the quotes: bash
                        // reaches `parse_comsub` for it from the `open == close`
                        // branch (parse.y:3954), which no `P_ARITH` is in a
                        // position to divert.
                        Some('$') if self.peek_at(1) == Some('(') => {
                            self.pos += 2;
                            // Commands, and so copied for a re-lex — its own
                            // collections stay with it. But the body itself is
                            // one bash parses in place: `echo $(( "$(fi)" ))`
                            // and `echo ${#x:-"$(fi)"}` are both fatal. Recorded
                            // unconditionally, because whether the record is
                            // *wanted* is the enclosing scan's question — a
                            // scan that already parses this text downstream
                            // (a `$( … )` body) drops the whole collection.
                            let outer = std::mem::take(&mut self.arith_comsubs);
                            // The delimiter this scan stands in *is* the `"`, and
                            // bash pushes none for a `$(` it reaches from
                            // `parse_matched_pair` (parse.y:3960 has no
                            // `push_delimiter`, unlike `read_token_word`'s
                            // parse.y:5041). So the body inherits the `"`.
                            // The splices go the other way from the spans: they
                            // are not a record of what to re-read but of which
                            // bytes of `raw` no scan read, and the body's bytes
                            // land in `raw` like any others. See
                            // [`Lexer::bare_splices`].
                            let body_splices = std::mem::take(&mut self.bare_splices);
                            let inner = self.read_balanced_inner('(', ')', true, true)?;
                            self.arith_comsubs = outer;
                            let inner_splices =
                                std::mem::replace(&mut self.bare_splices, body_splices);
                            let start = raw.len();
                            raw.extend_from_slice(b"$(");
                            self.bare_splices.extend(shift_ranges(inner_splices, raw.len()));
                            raw.extend_from_slice(&inner);
                            raw.push(b')');
                            self.arith_comsubs.push(CmdSubSpan {
                                src: inner,
                                close_line: self.cur_line(),
                                range: start..raw.len(),
                                kind: self.subst_kind(),
                            });
                        }
                        Some(_) => self.take_into(raw),
                        None => return Err(eof_matching('"').at(q_open)),
                    }
                }
            }
            // A backslash escapes the next character, the closing delimiter
            // included. Both travel, so the span they make is one character
            // longer than the escape itself.
            '\\' => {
                raw.push(b'\\');
                self.take_into(raw);
                Ok(true)
            }
            '`' => {
                let (_, verbatim) = self.read_backtick(false)?;
                raw.push(b'`');
                raw.extend_from_slice(&verbatim);
                raw.push(b'`');
                Ok(true)
            }
            // …but only for a body of *commands*. bash skips a `${ … }` under
            // `P_ARRAYSUB|P_DOLBRACE` only (parse.y:3929); the arithmetic scan
            // carries neither, so for it a `${ … }` is ordinary text and a
            // closing delimiter inside one closes the construct. That is why
            // `echo $[ 1 + ${x:-]} ]` is a `bad substitution` in bash — the `]`
            // ended the expansion at `${x:-` — and why `echo $(( ${` reports the
            // `)` it wanted rather than the `}`.
            '$' if command && self.peek() == Some('{') => {
                self.pos += 1;
                // Not inside a double-quoted *run* — the `"` arm above reads
                // those and passes `true` — but possibly standing in one all the
                // same: `parse_matched_pair (cd, '{', '}', …)` (parse.y:5033)
                // hands the `${` the delimiter stack's top, and a `$( … )`
                // reached from inside a `"…"` pushes nothing over it
                // (parse.y:3960). So `"$(echo ${x:-$'a\tb'})"` splices bare,
                // where the same body written unquoted re-quotes. See `dq`.
                let (inner, nested, spliced) = self.read_dollar_brace(dq)?;
                raw.extend_from_slice(b"${");
                // See the `"`-run arm above: the body's ranges are relative to
                // `inner` until it is spliced.
                self.arith_comsubs.extend(shift_spans(nested, raw.len()));
                self.bare_splices.extend(shift_ranges(spliced, raw.len()));
                raw.extend_from_slice(&inner);
                raw.push(b'}');
                Ok(true)
            }
            // A nested `$( … )`, in an *arithmetic* scan only. bash reaches
            // `parse_comsub` for one from `P_ARITH` as well (parse.y:3927), and
            // that is a whole nested parse: its here-documents are gathered as
            // it goes, and its own body error is raised in place of the missing
            // `)` out here — `echo $(( $(fi` names `fi`, and
            // `cat <<E $(( $(cat <<F` warns about `F` while `E` goes down with
            // the abandoned parse. A `$((` is not one of these: it opens another
            // arithmetic span, which the depth counting handles. In a *command*
            // scan the counting downstream already does all of this, with the
            // enclosing body's own here-document order to keep — see the
            // `nested` stack in [`Lexer::read_balanced_body`].
            '$' if !command && self.peek() == Some('(') && self.peek_at(1) != Some('(') => {
                self.pos += 1;
                // `parse_dollar_word` again (parse.y:3931), and again with no
                // `push_delimiter`, so the body stands in whatever delimiter the
                // arithmetic scan does.
                let inner = self.read_subst_body(dq).map_err(|e| e.at(self.eof_line()))?;
                // The parse is bash's, not ours to defer: `parse_dollar_word`
                // runs `parse_comsub` here and now, and its failure is a syntax
                // error in the *enclosing* unit — which is why
                // `if false; then echo $(( 1 + $(fi) )); fi` dies even though the
                // branch is never taken. What survives is not the source but the
                // parse *re-printed* — `parse_comsub` returns
                // `print_comsub (parsed_command)` (parse.y:4219) — so the range
                // travels with the record and the parser writes the re-print
                // back over it.
                let start = raw.len();
                raw.extend_from_slice(b"$(");
                raw.extend_from_slice(&inner);
                raw.push(b')');
                // …but only where a parser read this text in the first place: an
                // arithmetic expansion written in a here-document body is found
                // by `expand_word_internal`, not by `read_token_word`, so
                // nothing in it was ever handed to `parse_comsub` and there is
                // no re-print to write back. See [`Lexer::here_text`]. The span
                // is still recorded — marked [`SubBody::Unread`] — because the
                // scan that walks that text at expansion time reads the body
                // there instead; see [`CmdSubSpan::kind`].
                self.arith_comsubs.push(CmdSubSpan {
                    src: inner,
                    close_line: self.cur_line(),
                    range: start..raw.len(),
                    kind: self.subst_kind(),
                });
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// `dq` is bash's `current_delimiter (dstack)` for the words *this* body
    /// reads directly — true when the delimiter the scan stands in is a `"`.
    /// It is not "inside quotes": bash pushes a delimiter in
    /// `read_token_word` only (parse.y:4952 for a quote, 5041 for a `$(`, 5071
    /// for a `<(`), and `parse_matched_pair`'s own nested `$(` arm
    /// (parse.y:3960) pushes none — so a `$( … )` written inside `"…"` hands its
    /// body the enclosing `"`, while one written at word level hands it a `(`.
    /// A nested `$(`/`<(`/`>(` inside this body is therefore the thing that
    /// clears it, which is what the `nested` stack below is consulted for.
    fn read_balanced_body(
        &mut self,
        open: char,
        close: char,
        command: bool,
        dq: bool,
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
        // Whether that span is a `(( … ))` *command* rather than a `$(( … ))`
        // expansion — bash's ARITH_CMD, which a command may follow, against an
        // expansion which is part of the word being read.
        let mut arith_cmd = false;
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
        // Bracket depth inside a `name[ … ]` array subscript, and the line the
        // outermost `[` stood on. bash reads one with `parse_matched_pair (cd,
        // '[', ']', &ttoklen, P_ARRAYSUB)` (parse.y:5148), a scan that knows
        // nothing about the `$( … )` it is standing in — so it runs *past* the
        // substitution's own `)` and takes it as subscript text. See
        // [`CaseScan::at_subscript`].
        let mut sub_depth = 0usize;
        let mut sub_line = 0u32;
        loop {
            let Some(cx) = self.bump_ch() else {
                // A subscript still open is the pair bash reports, and it reports
                // it *instead of* the missing `)` — and before the here-documents
                // this body declared are ever missed, so `$(cat <<E; f[1` names
                // the `]` and warns about nothing. `parse_matched_pair` fails at
                // the end of input and `read_token_word` bails immediately
                // (`return -1`, parse.y:5150) without ever reaching the reduction
                // that would have noticed the pending here-document.
                if sub_depth > 0 {
                    return Err(eof_matching(']').at(sub_line));
                }
                // bash reads a whole line at a time, so the input's last line ends
                // whether or not it carries a newline — and the here-documents that
                // line declared are gathered, and warned about, at that end, before
                // the scan's own failure to find `close` is ever noticed. `echo
                // $(cat <<E` with no trailing newline warns about `E` and *then*
                // reports the missing `)`. With a newline the `'\n'` arm below has
                // already done it, which is why only this path needed saying.
                if !pending.is_empty() {
                    self.consume_subst_heredoc_bodies(&mut pending, &mut raw)?;
                }
                return Err(eof_matching(close));
            };
            let c = syn(cx);
            // `parse_matched_pair` reads with `shell_getc (1)` (parse.y:3705), so
            // the reader has already deleted a `\<newline>` before the scan sees
            // the backslash at all. A command body is *copied* for a re-lex that
            // deletes it again, so there the pair can travel; an arithmetic body's
            // text is used exactly as it stands, so the deletion has to happen
            // here. Without it `$((1+1)\<newline>)` leaves `raw` ending on a
            // newline, the `)` suffix is not found, and the whole thing is
            // misread as a command substitution.
            if !command && c == '\\' && self.cont_len_at(self.pos.saturating_sub(1)).is_some() {
                self.pos = self.pos.saturating_sub(1);
                self.eat_conts();
                continue;
            }
            // `nested` is bash's `push_delimiter (dstack, '(')`: a `${ … }` under
            // a nested `$(`/`<(`/`>(` stands in that `(` and not in this body's
            // delimiter, so it never inherits the enclosing `"`.
            if self.read_opaque_span(c, &mut raw, command, dq && nested.is_empty())? {
                word_start = false;
                // Inside an arithmetic span there are no words to spoil: see the
                // `cases.feed` call below.
                if arith_from.is_none() {
                    cases.push_quoted();
                }
                continue;
            }
            // A `[` at the head of a name in command position. Everything up to
            // the matching `]` is one word's text: no delimiter, no operator and
            // no `)` of ours ends it, which is why this sits ahead of every other
            // arm and why the depth counting below never sees those characters.
            // Quoting still nests — the arm above ran first — so `f[a"]"b` is
            // still looking for its `]`.
            if command && arith_from.is_none() && sub_depth == 0 && c == '[' && cases.at_subscript()
            {
                sub_line = self.cur_line();
                sub_depth = 1;
                cases.push('[');
                raw.push(b'[');
                word_start = false;
                continue;
            }
            if sub_depth > 0 {
                match c {
                    '[' => sub_depth += 1,
                    ']' => sub_depth -= 1,
                    _ => {}
                }
                cases.push(c);
                cx.push_to(&mut raw);
                word_start = false;
                continue;
            }
            if command {
                let in_arith = arith_from.is_some();
                match c {
                    '\n' => {
                        raw.push(b'\n');
                        if !pending.is_empty() {
                            self.consume_subst_heredoc_bodies(&mut pending, &mut raw)?;
                            own = 0;
                        }
                        word_start = true;
                        cases.feed('\n', None, None, depth);
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
                    // `$((` / `((` open an arithmetic span. The parens are left to
                    // the depth counting below, which is what tells us where the
                    // span ends; only the suppression is recorded here.
                    '$' if self.peek() == Some('(') && self.peek_at(1) == Some('(') => {
                        if arith_from.is_none() {
                            arith_from = Some(depth);
                            // An *expansion*, so it is part of the word being
                            // read rather than a command of its own.
                            arith_cmd = false;
                            cases.push_quoted();
                        }
                        raw.push(b'$');
                        word_start = false;
                        continue;
                    }
                    // A `((` already inside an arithmetic span opens no span of
                    // its own — it is a nested grouping the depth counting
                    // handles — so it falls through to the `_` arm.
                    '(' if word_start && self.peek() == Some('(') && arith_from.is_none() => {
                        arith_from = Some(depth);
                        arith_cmd = true;
                    }
                    // A `<<<` here-string is consumed whole, so that its second `<`
                    // is never mistaken for the first of a `<<`.
                    '<' if self.peek() == Some('<') && self.peek_at(1) == Some('<') => {
                        self.pos += 2;
                        raw.extend_from_slice(b"<<<");
                        word_start = false;
                        cases.finish_word(depth, Some('<'));
                        cases.pos.ev(Ev::RedirOp);
                        continue;
                    }
                    // `<<`: a here-document, whose body the next newline brings from
                    // the enclosing input.
                    '<' if !in_arith && self.peek() == Some('<') => {
                        self.pos += 1;
                        raw.extend_from_slice(b"<<");
                        cases.finish_word(depth, Some('<'));
                        cases.pos.ev(Ev::RedirOp);
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
                        // The delimiter is the redirection's target word, and
                        // leaves the run of redirections running.
                        cases.pos.ev(Ev::RedirTarget);
                        word_start = false;
                        continue;
                    }
                    _ => {}
                }
            }
            // Feed the `case` tracker. Every delimiter ends the word being read,
            // and the reserved words are only recognised there.
            //
            // Not inside an arithmetic span: its text is an *expression*, in
            // which `<` is a comparison and `&&` a conjunction rather than
            // anything the shell grammar would recognise, and no `case` can
            // stand there. bash never reaches it with `read_token` at all — the
            // whole span is one `parse_matched_pair` — so feeding it here would
            // be inventing tokens, and `(( case ))` would open a frame that
            // never closes.
            if command && arith_from.is_none() {
                cases.feed(c, self.peek(), self.peek_at(1), depth);
            }
            if c == open {
                // A `case` pattern's optional `(` has no mate of its own.
                if !(command && cases.is_pattern_open(depth)) {
                    depth += 1;
                    // `$(`, `<(` and `>(` — the sigil is the character before the
                    // paren the cursor has just stepped over. An arithmetic `$((`
                    // is excluded: it opens no reader of its own, and a `<<` in it
                    // is a left shift anyway.
                    if command
                        && arith_from.is_none()
                        && matches!(self.at(self.pos.wrapping_sub(2)), Some('$' | '<' | '>'))
                    {
                        nested.push((depth, pending.len()));
                    }
                }
            } else if c == close {
                // A pattern's `)` closes the pattern, not a group — this is the
                // whole reason the scan tracks `case` at all.
                if !(command && cases.take_pattern_close(depth)) {
                    depth -= 1;
                    if depth == 0 {
                        // A `case` still open here is one bash's parser would have
                        // met this `)` in the middle of, so hand the `)` to the
                        // body parse and let it name the token — which also puts
                        // the failure where bash puts it, in the substitution
                        // rather than in the enclosing input (the two exit
                        // differently: 1 for a substitution, 2 for the input).
                        if command && cases.open_at_close() {
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
                        if arith_cmd {
                            cases.pos.ev(Ev::ArithCmd);
                            arith_cmd = false;
                        }
                    }
                }
            }
            cx.push_to(&mut raw);
            if command {
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
        // The line the warning names is the one the reader has reached *now*,
        // before this gather moves it; only where the gather will read from has
        // to wait for `hd_ahead` to be taken.
        let pending_warning = (own > 0).then(|| {
            ReaderWarning::SubstHeredoc(SubstHeredoc {
                count: own,
                line: self.fetched_line(),
                tok_index: self.next_tok_index,
            })
        });
        let resume = self.pos;
        // An earlier substitution on this line may already have taken lines; carry
        // on from where it stopped rather than re-reading them.
        let from = match self.hd_ahead.take() {
            Some(a) => a.pos,
            None => {
                let mut p = self.pos;
                while !matches!(self.at(p), None | Some('\n')) {
                    p += 1;
                }
                p.saturating_add(usize::from(p < self.chars.len()))
            }
        };
        if let Some(w) = pending_warning {
            self.warn(from, w);
        }
        self.pos = from;
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
            // Where this body's lines are being read from, for a caller that has
            // to place it in the text. See [`Spanned::warn_from`].
            let from = self.pos;
            loop {
                if self.pos >= self.chars.len() {
                    if self.strict_heredoc_eof {
                        return Err(unterminated_heredoc(&delim));
                    }
                    let eof_line = self.fetched_line();
                    let tok_index = self.next_tok_index;
                    self.warn(
                        from,
                        ReaderWarning::HeredocEof(HeredocEof {
                            delim: delim.clone(),
                            body_line,
                            eof_line,
                            tok_index,
                        }),
                    );
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
    ///
    /// Returns the raw text and the `$( … )` bodies met while reading it, which
    /// bash parses **here**, as it reads them — `parse_dollar_word` hands the
    /// `$(` to `parse_comsub` (parse.y:3954) exactly as it does under `P_ARITH`.
    /// They are returned rather than left in [`Lexer::arith_comsubs`] because
    /// they belong to *this* scan and not to an arithmetic one enclosing it:
    /// `echo $(( 1 + ${x:-$(fi)} ))` is fatal, but the `$(fi)` is the `${ … }`'s
    /// to report. The caller decides — an enclosing raw-text scan folds them
    /// into its own collection, while the parser runs them only where the body
    /// itself was never parsed (`seg_to_part`'s `ParamBraced` arm).
    ///
    /// `in_dquote` is bash's `P_DQUOTE` for this scan — set when the `${` was
    /// written inside a double-quoted run. The body is read the same either way;
    /// all it decides is what becomes of a `$'…'` in it, and that is the one
    /// place bash 5.2's answer differs from 5.3's. See the arm below.
    fn read_dollar_brace(&mut self, in_dquote: bool) -> Result<BracedBody, LexError> {
        let outer = std::mem::take(&mut self.arith_comsubs);
        // The splices are ranges into the body this call is about to build, so
        // they are scoped to it exactly as the spans are. See
        // [`Lexer::bare_splices`].
        let outer_splices = std::mem::take(&mut self.bare_splices);
        let r = self.read_dollar_brace_body(in_dquote);
        let nested = std::mem::replace(&mut self.arith_comsubs, outer);
        let spliced = std::mem::replace(&mut self.bare_splices, outer_splices);
        r.map(|raw| (raw, nested, spliced))
    }

    /// The scan proper for [`Lexer::read_dollar_brace`].
    fn read_dollar_brace_body(&mut self, in_dquote: bool) -> Result<Str, LexError> {
        let open = self.cur_line();
        let mut raw = Str::new();
        // bash's `dolbrace_state`, a fresh `DOLBRACE_PARAM` per `${ … }`
        // (parse.y:3686) — a nested one recurses (parse.y:3928 →
        // `parse_dollar_word`) and so starts over, which is why
        // `"${a:-${b#$'\''}}"` re-quotes and `"${a#${b:-$'\''}}"` does not.
        let mut state = crate::wordscan::DolBrace::Param;
        loop {
            let Some(cx) = self.bump_ch() else {
                return Err(eof_matching('}').at(open));
            };
            let ch = syn(cx);
            // bash appends the character and *then* runs the machine on it
            // (parse.y:3785 → 3809). The characters that open a nested
            // construct — `'`, `"`, `` ` ``, `$`, `{` — are appended and stepped
            // over like any other before the construct is read, and the
            // construct's own contents never reach the machine; the character
            // after a backslash does not either (`LEX_PASSNEXT` `continue`s,
            // parse.y:3749). Both fall out of stepping here, once, per
            // iteration. The closing `}` breaks out before the machine runs.
            if ch != '}' {
                let started = !raw.is_empty();
                state = state.step(u8::try_from(ch).unwrap_or(b'\xff'), started);
            }
            match ch {
                // First unescaped, unquoted, non-nested `}` closes the span.
                '}' => return Ok(raw),
                // A subscript opening while the scan is still in the parameter
                // *name* is jumped over wholesale, `]` and all — the rule
                // `string_extract` gets from `SX_VARNAME` (subst.c:795) and
                // `extract_dollar_brace_string` from `dolbrace_state ==
                // DOLBRACE_PARAM` (subst.c:1943). It is not the parser's rule,
                // so it is only in force on the re-read
                // ([`ParseOpts::reread`]); with it the `}` that closes this
                // body may be one the parser never saw, since the jump crosses
                // a `}` — and a `"` — as readily as any other character.
                //
                // Which bodies are still "in the name" is bash's state machine,
                // and it splits `#` from `!`: `#` is in the operator set the
                // scan checks first (`"#%^,~:-=?+/"`), so `${#h[` has left
                // `DOLBRACE_PARAM` by the `[` and the jump does not fire, while
                // `!` is in no set at all and `${!h[` is still a name. Both are
                // measured — `"${#h[}"x"]}"` is `${#h[}: bad substitution` and
                // `"${!h[}"x"]}"` indirects through `h[}x]`.
                '[' if self.opts.reread && is_brace_name_so_far(&raw) => {
                    // Only a subscript that *closes* is jumped over: bash's
                    // `if (string[ni] == RBRACK)` leaves the index alone
                    // otherwise, which is why `echo ${a[}tail` stays an
                    // ordinary bad substitution. `read_balanced` has consumed
                    // the hunt by the time it reports one, so the cursor goes
                    // back before the `[` is written as itself.
                    let save = self.pos;
                    match self.read_balanced('[', ']') {
                        Ok(inner) => {
                            raw.push(b'[');
                            raw.extend_from_slice(&inner);
                            raw.push(b']');
                        }
                        Err(_) => {
                            self.pos = save;
                            raw.push(b'[');
                        }
                    }
                }
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
                // Double quotes, read by the same scan every other grouping
                // construct uses: a `"` is a `shellquote` to `parse_matched_pair`
                // wherever it stands (parse.y:3844), so a `$( … )` inside one is
                // still met — and still parsed here and now, which is why
                // `echo ${#x:-"$(fi)"}` is a syntax error and not a
                // `bad substitution`.
                '"' => {
                    // Inside the run, so the delimiter is that `"` whatever
                    // the `${` itself stands in.
                    self.read_opaque_span('"', &mut raw, true, true)?;
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
                    // `$'…'` is translated where it is read, like in every other
                    // grouping construct: `ansiexpand`, then `retind -= 2` to
                    // back up over the `$'` already written (parse.y:3854–3893).
                    // Deciding before the `$` is written is the same thing
                    // without the rewind.
                    //
                    // What is *not* the same everywhere is whether the result is
                    // re-quoted afterwards, and bash makes that a three-way
                    // decision on this scan's `P_DQUOTE` and its
                    // `dolbrace_state`:
                    //
                    // | where the `$'…'` sits | re-quoted? | parse.y |
                    // |---|---|---|
                    // | no `P_DQUOTE` — the `${` was not written in double quotes | yes | 3882 |
                    // | `P_DQUOTE`, past a `#`, `%`, `/`, `^` or `,` — `DOLBRACE_QUOTE`/`QUOTE2` | yes | 3866 |
                    // | `P_DQUOTE`, still in the name or in a `:-`-style word | **no**, spliced bare | 3887 |
                    //
                    // Re-quoting is what keeps the value a value: the text
                    // spliced in is read again by whatever reads this body, so
                    // an unquoted `'` or `}` in it becomes that reader's. Which
                    // is exactly what the third row does — `"${x:-$'a}b'}"` is
                    // the word `${x:-a}b}`, and bash expands `${x:-a}` and
                    // leaves `b}` as text; `"${x:-$'\x27'}"` leaves a lone `'`
                    // that swallows the `}` and is
                    // ``bad substitution: no closing `}' in "${x:-'}"``.
                    //
                    // bash has the fix for that written and disabled behind
                    // `#if 0 /* TAG:bash-5.3 */` (parse.y:3875), so this is a
                    // defect it has already repaired upstream. It is reproduced
                    // rather than waived because §105's bar for waiving is an
                    // *unchecked* error path with nothing suggesting intent, and
                    // a release-tagged `#if 0` is the opposite: a deliberate
                    // decision to keep 5.2 answering this way.
                    //
                    // Either way the *shape* changes: a `$'x\ny'` puts a real
                    // newline where the source had none, so a `$LINENO` after it
                    // in the same `$( … )` body moves down a line, exactly as
                    // bash's does.
                    // …in every grouping construct `read_token_word` reads, which
                    // a here-document body is not: nothing translates it, so the
                    // `$'…'` falls through to the `'` arm below and is copied
                    // verbatim, exactly as `${x:-'q'}` in a body is. See
                    // [`Lexer::here_text`].
                    if !self.here_text && self.peek() == Some('\'') {
                        // The `'` is a character of the body too, and bash steps
                        // the machine over it before reading the run.
                        state = state.step(b'\'', true);
                        self.pos += 1;
                        let s = crate::escape::ansi_c_translate(&self.read_ansi_c_source()?);
                        if in_dquote && state != crate::wordscan::DolBrace::Quote {
                            // Text this scan will not read back — see
                            // [`Lexer::bare_splices`] and
                            // [`crate::wordscan::expansion_body_len`].
                            //
                            // `nestlen = ttranslen` (parse.y:3892), so a NUL the
                            // translation produced is *kept*: this row is the one
                            // place a shell word carries one, and the word it
                            // lands in ends there rather than the translation. See
                            // `parser::segs_hold_a_nul` and
                            // `parser::word_expanded_from_its_text`.
                            let start = raw.len();
                            raw.extend_from_slice(&s);
                            self.bare_splices.push(start..raw.len());
                        } else {
                            // `nestlen = strlen (nestret)` (parse.y:3870, 3886)
                            // after `sh_single_quote` took the translation as a
                            // `char *`, so here the *translation* is what a NUL
                            // ends.
                            let s = crate::escape::cut_at_nul(s);
                            raw.extend_from_slice(&crate::escape::sh_single_quote(&s));
                        }
                        continue;
                    }
                    raw.push(b'$');
                    match self.peek() {
                        Some('{') => {
                            raw.push(b'{');
                            self.pos += 1;
                            // A nested body's eager parses are this body's too:
                            // `echo ${#x:-${y:-$(fi)}}` is a syntax error, not a
                            // `bad substitution`, so the record has to travel out
                            // past the level that collected it.
                            let (inner, nested, spliced) = self.read_dollar_brace(in_dquote)?;
                            // `${` is already written, so the body starts here.
                            self.arith_comsubs.extend(shift_spans(nested, raw.len()));
                            self.bare_splices.extend(shift_ranges(spliced, raw.len()));
                            raw.extend_from_slice(&inner);
                            raw.push(b'}');
                        }
                        Some('(') => {
                            raw.push(b'(');
                            self.pos += 1;
                            if self.peek() == Some('(') {
                                // Only the extent is wanted — this text is being
                                // copied for a re-lex, which will run the same scan
                                // over it and classify it itself. So the balanced
                                // read rather than the arithmetic *command*'s
                                // reader: whether the expression is well formed,
                                // and whether it is an expression at all, are not
                                // questions this scan has to answer.
                                let inner = self.read_balanced('(', ')')?;
                                raw.extend_from_slice(&inner);
                                raw.push(b')');
                            } else {
                                // The same substitution as anywhere else, so it
                                // reads a here-document declared inside it the same
                                // way — the body lines land in this `${ … }`'s raw
                                // text, right where the nested re-lex expects them.
                                // …and, like every `$(`, in this scan's delimiter
                                // — which a here-document body does not have one
                                // of. See [`Lexer::here_text`].
                                let inner = self
                                    .read_subst_body(in_dquote && !self.here_text)
                                    .map_err(|e| e.at(self.eof_line()))?;
                                // …and bash parses it here and now, from
                                // `parse_dollar_word` (parse.y:3954). What
                                // survives is the parse re-printed, not the
                                // source (parse.y:4219), so the range names the
                                // whole `$( … )` — the `$(` two bytes back, which
                                // this arm has already written.
                                let start = raw.len().saturating_sub(2);
                                raw.extend_from_slice(&inner);
                                raw.push(b')');
                                // …unless nothing here was read by a parser at
                                // all, in which case there is no parse to
                                // re-print and the source is what stands. See
                                // [`Lexer::here_text`] and [`CmdSubSpan::kind`].
                                self.arith_comsubs.push(CmdSubSpan {
                                    src: inner,
                                    close_line: self.cur_line(),
                                    range: start..raw.len(),
                                    kind: self.subst_kind(),
                                });
                            }
                        }
                        // `$[ … ]` is a row of `parse_matched_pair`'s and **not**
                        // one of `extract_dollar_brace_string`'s: the
                        // expansion-time scan has rows for `` ` ``, `$(`, `<(`,
                        // `"`, `'` and a `[` subscript, and nothing for `$[`
                        // (subst.c:1881-1950). So which scan is reading this text
                        // decides whether a `$[` with no `]` may swallow the `}`.
                        // Measured, `"${x:-$[ 1 + 2 }B"` written in a script is a
                        // parse error in both shells — the parser's row does
                        // swallow the quote — while the same text under `@P`
                        // closes its brace at the `}` and expands the operand
                        // `$[ 1 + 2 ` on its own, which then keeps itself whole
                        // (see [`crate::interp::Shell::expand_unclosed`]):
                        // `A${x:-$[ 1 + 2 }B` is `A$[ 1 + 2 B`.
                        Some('[') if !self.here_text => {
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

    /// Read the body of an arithmetic *command* — `(( … ))` — up to its `))`.
    ///
    /// `Ok(None)` means the body was balanced but the second `)` was not the
    /// very next character, which is bash's cue to re-read the whole thing as
    /// nested subshells; it is reported rather than raised. `Err` is reserved
    /// for running out of input, which is an error either way, `parse_arith_cmd`
    /// passing the failure straight on without rewinding.
    ///
    /// bash reads that second `)` with `shell_getc (0)`, which does *not* delete
    /// a `\<newline>`, so nothing at all may come between the two — not a space,
    /// not a tab, not a newline, not a continuation. The `$(( … ))` *expansion*
    /// is held to no such rule, and is not read here at all: it goes through
    /// [`Lexer::read_balanced`] like the substitution it may yet turn out to be,
    /// and only afterwards is asked whether it parses as an expression. The
    /// *body* is read the same way for both, which is why `((1 +\<newline>1))`
    /// is arithmetic.
    ///
    /// The `$( … )` bodies met on the way come back with the text, exactly as
    /// they do for the expansion: `parse_arith_cmd` reads a `(( … ))` with
    /// `parse_matched_pair (0, '(', ')', &ttoklen, P_ARITH)` (parse.y:4519–4530),
    /// which is the same scan, so the same `APPEND_NESTRET` splices the same
    /// re-print into the same buffer. They are returned even when the body is
    /// abandoned (`Ok(None)`), because bash parses them *before* it tests for
    /// the second `)` — `((echo $(fi) ) )` is a fatal syntax error at `fi`, not
    /// a subshell containing a bad substitution.
    fn read_arith_body(
        &mut self,
        adjacent: bool,
    ) -> Result<(Option<Str>, Vec<CmdSubSpan>), LexError> {
        // See [`Lexer::arith_comsubs`]: the spans belong to *this* scan, so the
        // enclosing one's collection is set aside for the duration.
        let outer = std::mem::take(&mut self.arith_comsubs);
        // Set aside and dropped: an arithmetic body is a segment of its own.
        // See [`Lexer::bare_splices`].
        let outer_splices = std::mem::take(&mut self.bare_splices);
        let r = self.read_arith_body_inner(adjacent);
        let nested = std::mem::replace(&mut self.arith_comsubs, outer);
        self.bare_splices = outer_splices;
        r.map(|raw| (raw, nested))
    }

    /// The scan proper for [`Lexer::read_arith_body`].
    fn read_arith_body_inner(&mut self, adjacent: bool) -> Result<Option<Str>, LexError> {
        let open = self.cur_line();
        let mut depth = 0usize;
        let mut raw = Str::new();
        loop {
            let Some(cx) = self.bump_ch() else {
                return Err(eof_matching(')').at(open));
            };
            let c = syn(cx);
            // The reader deleted a `\<newline>` before this scan saw it, so it is
            // neither text of the expression nor something that can come between
            // the two closing parentheses. Tested ahead of the escape below
            // because it is not one: nothing follows it to be escaped.
            if c == '\\' && self.cont_len_at(self.pos.saturating_sub(1)).is_some() {
                self.pos = self.pos.saturating_sub(1);
                self.eat_conts();
                continue;
            }
            // A `)` inside a quoted string, a `${ … }` or behind a backslash is
            // text of the expression and closes nothing — see
            // [`Lexer::read_opaque_span`], which is the same set bash's
            // `parse_matched_pair` skips under `P_ARITH`.
            if self.read_opaque_span(c, &mut raw, false, false)? {
                continue;
            }
            match c {
                '(' => {
                    depth += 1;
                    raw.push(b'(');
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
        // Where the delimiter word itself begins, which is where the alias pass
        // has to write a replacement for it. Taken before the read so that the
        // blanks and continuations skipped above are not part of it.
        let delim_at = self.pos;
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
        // It also leaves the operator visible to the alias pass, which re-lexes
        // the assembled text and there finds the delimiter that belongs to it on
        // the calling line. See [`expand_aliases_tracked`].
        if delim.is_empty() && expand {
            return Ok(());
        }
        out.push(Tok::HereDoc(Vec::new(), delim.clone(), !expand));
        // The delimiter is a WORD of its own in the grammar, and bash reads it
        // with `read_token_word` like any other — so it is alias-expandable when
        // `PST_ALEXPNEXT` is set (parse.y:5266), and the pop that sets that flag
        // happens between the `<<` and it. Both need the word's own offset, so
        // the token that stands for it carries that rather than the iteration's.
        self.hd_delim = Some((tok_index, u32::try_from(delim_at).unwrap_or(u32::MAX)));
        self.pending_heredocs.push(PendingHeredoc {
            delim,
            strip,
            expand,
            tok_index,
            op_at: u32::try_from(self.iter_start).unwrap_or(u32::MAX),
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
        if pending.is_empty() {
            return Ok(());
        }
        // A body comes off the *raw input*, not off the text being parsed:
        // `make_here_document` calls `read_secondary_line`, which calls
        // `read_a_line`, which calls `yy_getc` — `bash_input.getter` itself,
        // under `shell_input_line` and under every string pushed onto it. So
        // when the token cursor is inside an alias value, the body is taken from
        // the input after the line the alias word stands on, and the cursor goes
        // back to the value afterwards. Everywhere else the two are the same
        // place and this is the identity. See [`Lexer::raw`].
        //
        // "After the line the alias word stands on" is where the value's run
        // ends: bash has the whole calling line in `shell_input_line` before it
        // ever expands a word on it, so `read_a_line` starts at the line after.
        // Unless a body was already taken, which left the cursor further on.
        let resume = self.pos;
        let in_value = self.map.at(self.pos).0 != 0;
        if in_value {
            let tail = self.map.real_at_or_after(self.pos).unwrap_or(self.chars.len());
            self.pos = self.raw.max(self.line_after(tail));
        }
        self.raw_from = self.pos;
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
                    let eof_line = self.fetched_line();
                    self.warn(
                        from,
                        ReaderWarning::HeredocEof(HeredocEof {
                            delim: ph.delim.clone(),
                            body_line,
                            eof_line,
                            tok_index: ph.tok_index,
                        }),
                    );
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
        self.raw = self.raw.max(self.pos);
        // What the gather ate, for a caller comparing this reading of the text
        // against an earlier one. See [`Spanned::taken`].
        if self.pos > self.raw_from {
            self.taken.push((
                u32::try_from(self.raw_from).unwrap_or(u32::MAX),
                u32::try_from(self.pos).unwrap_or(u32::MAX),
            ));
        }
        if in_value {
            // The token cursor was inside a pushed alias value, which the body
            // reader never touched; it carries on there.
            self.pos = resume;
        }
        Ok(())
    }

    /// Record a reader warning together with `from`, the offset the gather that
    /// raised it was reading the body from. See [`Spanned::warn_from`].
    fn warn(&mut self, from: usize, w: ReaderWarning) {
        self.warn_from.push(u32::try_from(from).unwrap_or(u32::MAX));
        self.warnings.push(w);
    }

    /// The offset just past the newline that ends the line `off` stands on, or
    /// the end of the text when no newline follows.
    fn line_after(&self, off: usize) -> usize {
        self.chars
            .get(off..)
            .unwrap_or(&[])
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.chars.len(), |i| off.saturating_add(i).saturating_add(1))
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
                None => {
                    let e = eof_matching('`').at(open);
                    // In text no parser read this is `param_expand`'s own
                    // failure rather than a lexing one, and it echoes the text
                    // from the backquote on. An enclosing `${ … }` or `$(( … ))`
                    // stamps its own over it — see [`Lexer::unread_eof`] — so
                    // this survives only where bash's reporter is reached, which
                    // is the top level of the text.
                    return Err(if self.here_text {
                        let src = bfmt![b"`", &self.slice(start, self.chars.len())];
                        e.unclosed(UnreadEof::Subst(Unclosed::Backquote { src }))
                    } else {
                        e
                    });
                }
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
///
/// A double-quoted *context* is not a double-quoted *word*, though: bash's
/// reader collected this text without ever calling `read_token_word`, so no
/// `$'…'` in it was translated at parse time and no delimiter was pushed for the
/// quotes around it. [`Lexer::here_text`] is that difference.
fn scan_heredoc_segs(body: BStr<'_>, expand: bool) -> Result<Vec<Seg>, LexError> {
    if !expand {
        return Ok(vec![Seg::Lit(body.to_vec())]);
    }
    let mut lx = Lexer::new(body, ParseOpts::default());
    // The body reached the expansion as a value: no reader ran over it, so a
    // `$'…'` in it survives as written. A *pattern* inside it does get one — see
    // [`Lexer::ansi_c_quote`] — but that is the parser's re-read of the fragment
    // source, not this scan.
    lx.apply_ctx(ReadCtx::VALUE);
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
                match lx.read_backtick(false) {
                    Ok((raw, src)) => {
                        segs.push(Seg::CmdSub(raw, lx.cur_line(), SubBody::Backtick(src)));
                    }
                    Err(e) => {
                        segs.push(lx.unclosed_seg(e)?);
                        break;
                    }
                }
            }
            '$' => match lx.read_dollar(true) {
                Ok(Some(seg)) => {
                    flush_lit(&mut segs, &mut lit);
                    segs.push(seg);
                }
                Ok(None) => lit.push(b'$'),
                Err(e) => {
                    flush_lit(&mut segs, &mut lit);
                    segs.push(lx.unclosed_seg(e)?);
                    break;
                }
            },
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
        super::tokenize(src.as_bytes(), ParseOpts::default(), ReadCtx::SOURCE)
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
            let mut with = super::tokenize(src.as_bytes(), ParseOpts { extglob: true, posix: false, reread: false, tolerant: false }, ReadCtx::SOURCE).unwrap();
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
            assert!(matches!(segs[0], Seg::Arith(_, false, _)));
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
                matches!(seg(src), Seg::Arith(_, false, _)),
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
        assert!(matches!(&toks[2], Tok::Word(s) if matches!(s[0], Seg::Arith(_, false, _))));
    }

    #[test]
    fn no_paren_inside_a_conditional_is_arithmetic() {
        // bash reads `((` as an arithmetic command only where
        // `reserved_word_acceptable (last_read_token)` holds (`parse_dparen`,
        // parse.y:4484). `last_read_token` is assigned only in `yylex`
        // (parse.y:2904), and the whole of `[[ … ]]` is read by
        // `parse_cond_command` from *inside* a single `read_token` call
        // (parse.y:3399) — `cond_term` asks `read_token` for its tokens
        // directly, never going through `yylex`. So for the length of a
        // conditional `last_read_token` is frozen at `COND_START`, which is
        // not on the list, and no paren in there is ever arithmetic however
        // deeply nested.
        let arith_count =
            |src: &str| tokenize(src).unwrap().iter().filter(|t| matches!(t, Tok::ArithCmd(..))).count();

        for src in [
            "[[ ((( a ))) ]]",
            "[[ (((( a )))) ]]",
            "[[ ((((( a ))))) ]]",
            "[[ (( 0 )) ]]",
            "[[ ((( 0 ))) || (( 1 )) ]]",
            "[[ (( a )) && (( b )) ]]",
        ] {
            assert_eq!(arith_count(src), 0, "{src} must not lex an arithmetic command");
        }

        // `[[ ((( a ))) ]]` is three nested groups around one word.
        let toks = tokenize("[[ ((( a ))) ]]").unwrap();
        let shape: Vec<&'static str> = toks
            .iter()
            .map(|t| match t {
                Tok::Op(Op::LParen) => "(",
                Tok::Op(Op::RParen) => ")",
                Tok::Word(_) => "w",
                Tok::Newline => "\\n",
                other => panic!("unexpected token {other:?}"),
            })
            .collect();
        assert_eq!(shape, ["w", "(", "(", "(", "w", ")", ")", ")", "w", "\\n"]);

        // The conditional's own nesting is what is tracked, so a `((` after
        // the `]]` is arithmetic again.
        for src in ["[[ a ]] && ((1))", "((1))", "for ((i=0;i<2;i++)); do :; done"] {
            assert_eq!(arith_count(src), 1, "{src} should lex one arithmetic command");
        }

        // A command substitution written inside the conditional is its own
        // parse — the body travels as raw text and is lexed again from a
        // command position, where its `((` *is* arithmetic. Nothing at this
        // level sees it, so the check here is only that the conditional still
        // ends up as `[[ -n <word> ]]`.
        let toks = tokenize("[[ -n $( ((1)) ; echo hi ) ]]").unwrap();
        assert_eq!(arith_count("[[ -n $( ((1)) ; echo hi ) ]]"), 0);
        assert_eq!(toks.len(), 5, "expected `[[ -n <word> ]]` + newline, got {toks:?}");
        assert!(matches!(&toks[2], Tok::Word(s) if matches!(s[0], Seg::CmdSub(..))));

        // And a `((` where no command can start is still a plain `(` outside a
        // conditional, which the conditional's own freeze must not have
        // disturbed.
        for src in ["x=1 ((2))", "echo ((1))"] {
            assert_eq!(arith_count(src), 0, "{src} must not lex an arithmetic command");
        }
    }

    #[test]
    fn a_reserved_words_spelling_is_not_its_classification() {
        // bash asks `reserved_word_acceptable (last_read_token)` — the previous
        // token's *classification*. A word becomes a reserved word only where
        // one was already acceptable, because `CHECK_FOR_RESERVED_WORD`
        // (parse.y:2994) is gated on the very same test. So the rule is
        // recursive and a table keyed on one token's spelling cannot express it.
        let arith_count =
            |src: &str| tokenize(src).unwrap().iter().filter(|t| matches!(t, Tok::ArithCmd(..))).count();

        // Where a reserved word may stand, it is one, and `((` is arithmetic.
        for src in [
            "; do ((1))",
            "if ((1)); then :; fi",
            "while ((1)); do :; done",
            "until ((1)); do :; done",
            "{ ((1)); }",
            "! ((1))",
            "time ((1))",
            "for ((i=0;i<2;i++)); do :; done",
            "echo a | ((1))",
            "echo a && ((1))",
            "( ((1)) )",
        ] {
            assert_eq!(arith_count(src), 1, "{src} should lex one arithmetic command");
        }

        // In an argument position the same spellings are plain words, so bash
        // hands back a single `(` — `echo do ((1))` is a syntax error near `('.
        for w in ["do", "done", "fi", "then", "esac", "until", "time", "!", "{", "}", "if"] {
            let src = format!("echo {w} ((1))");
            assert_eq!(arith_count(&src), 0, "{src} must not lex an arithmetic command");
        }

        // `]]` is `COND_END`, which *is* on bash's acceptable list — but only
        // when it closed a conditional. A word merely spelled `]]` is a word.
        assert_eq!(arith_count("[[ a ]] ((1))"), 1, "`]]` closes a conditional, so `((` is arithmetic");
        assert_eq!(arith_count("echo ]] ((1))"), 0, "a word spelled `]]` leaves a WORD behind");

        // The two lookbehind cases: `reserved_word_acceptable`'s default branch
        // accepts a WORD whose predecessor was `function` or `coproc`
        // (parse.y:5406–5412), which is how a name reaches an arithmetic body.
        assert_eq!(arith_count("function f ((1))"), 1, "the name after `function`");
        assert_eq!(arith_count("coproc c ((1))"), 1, "the name after `coproc`");
        // And they are one word deep, not a mode.
        assert_eq!(arith_count("function f g ((1))"), 0, "only the first word after `function`");

        // The words bash leaves off the list stay off it: after `case`, `in` or
        // `select` what follows is a pattern or a name.
        for src in ["case x in ((p) echo hi;; esac", "for i in ((1))", "select i in ((1))"] {
            assert_eq!(arith_count(src), 0, "{src} must not lex an arithmetic command");
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
            // A `\<newline>` after the delimiter word is *deleted* by the reader,
            // and every deletion costs a fetch — `shell_getc` bumps `line_number`
            // and re-reads (parse.y:2361). So the two lines here are 3, not 2, and
            // then the parser's own request for the end-of-file token finds the
            // buffer used up and pays one more: both numbers are 4. (The source is
            // written closed, as `st_stream` hands it over — a script whose last
            // line is a lone `\` has a `\n` appended to it.)
            ("echo 1\ncat <<x\\\n", &[("x", 4, 4)]),
            // Each further continuation is one more fetch.
            ("echo 1\ncat <<x\\\n\\\n", &[("x", 5, 5)]),
            ("echo 1\ncat <<x\\\n\\\n\\\n", &[("x", 6, 6)]),
            // Both delimiters of a doubled operator are blamed on the same line:
            // the deletions happen once, while the reader is past both words.
            ("echo 1\ncat <<x <<y\\\n", &[("x", 4, 4), ("y", 4, 4)]),
            // With a real line after it the continuation is still a fetch — line 3
            // is where body collection starts — but the end of file is then found
            // by fetching line 4 rather than by the parser's post-EOF bump.
            ("echo 1\ncat <<x\\\n\necho 3", &[("x", 3, 4)]),
            // The deletion is invisible to the *word* scan, so a continuation in
            // the middle of the delimiter splices it: the delimiter wanted is
            // `xbody`, and it is line 3 that the reader stopped on.
            ("echo 1\ncat <<x\\\nbody\n", &[("xbody", 3, 3)]),
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
                        Seg::CmdSub(raw, _, SubBody::Eager) | Seg::ProcSub(_, raw, _) => Some(raw.clone()),
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
        // The `<<`'s own line being the *last* line, with no newline of its own,
        // changes nothing: bash reads a whole line at a time, so that line still
        // ends, and the gather still happens there. Both of the warning's numbers
        // are that line, while the `)` is blamed one past it as always.
        /// A source, the line its unmatched `)` is blamed on, and the
        /// `(delimiter, body line, EOF line)` of each warning it raises.
        type LastLine<'a> = (&'a str, u32, &'a [(&'a str, u32, u32)]);
        let last_line: &[LastLine<'_>] = &[
            ("echo $(cat <<E", 2, &[("E", 1, 1)]),
            ("echo <(cat <<E", 2, &[("E", 1, 1)]),
            ("echo one\necho $(cat <<E", 3, &[("E", 2, 2)]),
            // One warning each, in declaration order.
            ("echo $(cat <<A; cat <<B", 2, &[("A", 1, 1), ("B", 1, 1)]),
            // The enclosing line's own here-document is not this scan's to gather:
            // only `B` is warned about, `A` going down with the abandoned parse.
            ("cat <<A $(cat <<B", 2, &[("B", 1, 1)]),
        ];
        for &(src, want_line, want_warnings) in last_line {
            let tk = tokenize_deferred(src.as_bytes(), ParseOpts::default());
            let (e, line) = tk.err.as_ref().unwrap_or_else(|| panic!("{src:?} must fail"));
            assert_eq!(e.msg, b"unexpected EOF while looking for matching `)'", "{src:?}");
            assert_eq!(*line, want_line, "{src:?}");
            assert_eq!(heredoc_eofs(&tk), want_warnings, "{src:?}");
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
                    Seg::CmdSub(raw, _, SubBody::Eager) => Some(raw.clone()),
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
        let opts = ParseOpts { extglob: true, posix: false, reread: false, tolerant: false };
        for (src, want) in bodies {
            let tk = tokenize_deferred(src.as_bytes(), opts);
            assert!(tk.err.is_none(), "{src:?} should lex: {:?}", tk.err);
            let raw = tk
                .toks
                .iter()
                .find_map(|t| match t {
                    Tok::Word(segs) => segs.iter().find_map(|s| match s {
                        Seg::CmdSub(raw, _, SubBody::Eager) | Seg::ProcSub(_, raw, _) => Some(raw.clone()),
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
                    Seg::CmdSub(raw, _, SubBody::Eager) => Some(raw.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .expect("a substitution segment");
        assert_eq!(raw, b"case b in b) echo B)");
    }

    /// [`TextMap`] — the provenance of a text bash's reader built by pushing an
    /// alias value onto the input. An unspliced text answers for itself.
    #[test]
    fn a_text_map_of_an_unspliced_text_is_that_text() {
        let m = TextMap::whole(0);
        assert_eq!(m.at(0), (0, 0));
        assert_eq!(m.at(7), (0, 7));
        // Past the end answers as the segment continued: a token's *end* offset
        // is one past its last character and must still name its text.
        assert_eq!(m.at(usize::MAX), (0, usize::MAX));
    }

    /// A splice is `prefix ++ value ++ tail`: the text the reader already passed
    /// is kept ahead of the value, and the tail resumes just past the alias word.
    #[test]
    fn a_text_map_splits_at_the_value_it_spliced_in() {
        // `echo A rest`, with `A` (running from 5 to 6) replaced by a
        // 5-character value.
        let m = TextMap::whole(0).spliced(5, 6, 5, 1);
        // The prefix is still the input, at its own offsets.
        assert_eq!(m.at(0), (0, 0));
        assert_eq!(m.at(4), (0, 4));
        assert_eq!(m.at(5), (1, 0));
        assert_eq!(m.at(9), (1, 4));
        // The tail resumes at the saved index, exactly as `pop_string` does.
        assert_eq!(m.at(10), (0, 6));
        assert_eq!(m.at(14), (0, 10));
    }

    /// Nesting composes, because the calling text is itself a concatenation:
    /// `alias A='B'` over `alias B='cat <<'` leaves three texts in play at once.
    #[test]
    fn a_text_map_composes_under_nesting() {
        // `A E` — `A` runs from 0 to 1, its value `B` is 1 character, in text 1.
        let outer = TextMap::whole(0).spliced(0, 1, 1, 1);
        assert_eq!(outer.at(0), (1, 0));
        assert_eq!(outer.at(1), (0, 1));
        // Now `B` (running from 0 to 1 of *that* text) expands to a 6-character
        // value in text 2. What is left is `<value 2> ++ " E"`, and the `" E"`
        // is still the original input's.
        let inner = outer.spliced(0, 1, 6, 2);
        assert_eq!(inner.at(0), (2, 0));
        assert_eq!(inner.at(5), (2, 5));
        assert_eq!(inner.at(6), (0, 1));
        assert_eq!(inner.at(7), (0, 2));
    }

    /// A splice whose word covers a whole segment drops it rather than leaving
    /// an empty run behind, so the map stays as short as the nesting is deep.
    #[test]
    fn a_text_map_drops_the_segments_the_word_covered() {
        // `V W` where `V` (0 to 1) expanded to 3 characters, then the whole
        // value *and* the space are a second alias word, running from 0 to 4.
        let m = TextMap::whole(0).spliced(0, 1, 3, 1);
        assert_eq!(m.segs.len(), 2);
        let m2 = m.spliced(0, 4, 2, 2);
        assert_eq!(
            m2.segs,
            vec![
                TextSeg { at: 0, src: 2, base: 0 },
                TextSeg { at: 2, src: 0, base: 2 },
            ]
        );
        assert_eq!(m2.at(0), (2, 0));
        assert_eq!(m2.at(2), (0, 2));
    }

    /// An empty alias value splices nothing, so the tail begins at offset 0 and
    /// the value's own segment is unreachable — the tail must win there.
    #[test]
    fn a_text_map_of_an_empty_value_is_all_tail() {
        let m = TextMap::whole(0).spliced(0, 2, 0, 1);
        assert_eq!(m.at(0), (0, 2));
        assert_eq!(m.at(3), (0, 5));
    }

    /// [`scan_cmdsub_body`] answers where a `$( … )` met at *expansion* time
    /// ends, and the answer is the parser's rather than a paren match's — which
    /// is the whole reason the expansion side calls in here instead of counting
    /// parentheses of its own. The offset is a **character** index, like the
    /// cursor it comes from.
    #[test]
    fn a_deferred_command_substitution_ends_where_the_parser_says() {
        let scan = |src: &str| {
            super::scan_cmdsub_body(src.as_bytes(), ParseOpts::default())
                .map(|(body, past)| (String::from_utf8(body).expect("body is text"), past))
        };
        // The plain case: the body stops at the `)` and the offset is one past
        // it, so the caller resumes on the ` + 1`.
        assert_eq!(scan("echo hi) + 1 "), Some(("echo hi".into(), 8)));
        // A `case` pattern's `)` closes a pattern, not the substitution. A
        // paren-matching scan stops at the first one and hands `case x in x` to
        // the shell; the parse runs on to the `)` after `esac`.
        assert_eq!(
            scan("case x in x) echo 7;; esac) + 0 "),
            Some(("case x in x) echo 7;; esac".into(), 27))
        );
        // A nested group is balanced through, and a quoted `)` is not a closer.
        assert_eq!(scan("(echo 8) )x"), Some(("(echo 8) ".into(), 10)));
        assert_eq!(scan(r#"echo ")")y"#), Some((r#"echo ")""#.into(), 9)));
        // Text that runs out with the substitution still open has no answer —
        // the caller then leaves the `$` as a literal, as bash's scan does.
        assert_eq!(scan("echo hi"), None);
        assert_eq!(scan("case x in x) echo 7;;"), None);
        // A body that will not *parse* still has an extent: the parse failure is
        // the caller's to raise (see `Shell::arith_dolparen`), and it can only
        // raise it once it knows how much text to parse.
        assert_eq!(scan("fi) + 1 "), Some(("fi".into(), 3)));
    }

    /// The second read bash gives a word carves a `${ … }` differently from
    /// the parser's: at a `[` still inside the name it jumps to the matching
    /// `]`, so the body may swallow the `}` the parser closed at, and the
    /// quote in between with it. See [`ParseOpts::reread`].
    #[test]
    fn the_re_read_jumps_from_a_subscript_to_its_bracket() {
        fn first_braced(segs: &[Seg]) -> Option<String> {
            for seg in segs {
                match seg {
                    Seg::ParamBraced(raw, _, _, _) => {
                        return Some(String::from_utf8(raw.clone()).expect("body is text"));
                    }
                    Seg::Dq(inner) => {
                        if let Some(found) = first_braced(inner) {
                            return Some(found);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        let body = |src: &str, reread: bool| {
            let segs = super::lex_word_verbatim_opts(
                src.as_bytes(),
                ParseOpts { reread, ..ParseOpts::default() },
                ReadCtx::SOURCE,
            )
            .expect("word lexes");
            first_braced(&segs).expect("a ${ … } in the word")
        };
        // The parser stops at the first `}`; the re-read jumps from the `[` to
        // the `]` three characters past the closing quote and closes on the
        // `}` after it.
        assert_eq!(body(r#""${h[}"x"]}""#, false), "h[");
        assert_eq!(body(r#""${h[}"x"]}""#, true), r#"h[}"x"]"#);
        // Unquoted is no different — the re-read is the expansion's, and every
        // word gets one.
        assert_eq!(body("${h[}x]}", true), "h[}x]");
        // `#` is in the first operator set the state machine tests, so the
        // scan has left the name by the `[` and nothing is jumped. `!` is in
        // none of them, so an indirection still reads a name.
        assert_eq!(body(r#""${#h[}"x"]}""#, true), "#h[");
        assert_eq!(body(r#""${!h[}"x"]}""#, true), r#"!h[}"x"]"#);
        // Only a subscript that closes is jumped over, so a `[` with no `]`
        // after it is left where it stands and the two reads agree.
        assert_eq!(body("${a[}tail", true), "a[");
        assert_eq!(body("${a[}tail", false), "a[");
        // A subscript that closes before the `}` was never in dispute.
        assert_eq!(body("${a[0]}", true), "a[0]");
    }

    /// A `$'…'` in a `${ … }` body is translated wherever it sits, but only
    /// bash's third row splices the translation in *bare* — inside double
    /// quotes with the body still in `DOLBRACE_PARAM`/`OP`/`WORD`
    /// (parse.y:3887). The other two re-quote it with `sh_single_quote`, and
    /// only the bare one leaves text in the body the scan never read back.
    ///
    /// What is recorded is *where* the translation landed, because two readers
    /// need to tell those bytes from the ones the scan read: the one looking for
    /// the `}` the expansion stops at, and the operand lexer, for which a
    /// `$( … )` in a splice was never parsed.
    #[test]
    fn only_a_bare_splice_raises_the_flag() {
        // The splices come back as start/end pairs rather than as ranges, so that a
        // one-element expectation can still be written `vec![…]` — a `vec!` of one
        // range trips `single_range_in_vec_init`, a lint for `vec![0..3]` written
        // where `(0..3).collect()` was meant.
        fn first_braced(segs: &[Seg]) -> Option<(String, Vec<(usize, usize)>)> {
            for seg in segs {
                match seg {
                    Seg::ParamBraced(raw, _, _, spliced) => {
                        let body = String::from_utf8(raw.clone()).expect("body is text");
                        return Some((body, spliced.iter().map(|r| (r.start, r.end)).collect()));
                    }
                    Seg::Dq(inner) => {
                        if let Some(found) = first_braced(inner) {
                            return Some(found);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        let braced = |src: &str| {
            let segs = super::lex_word_verbatim_opts(src.as_bytes(), ParseOpts::default(), ReadCtx::SOURCE)
                .expect("word lexes");
            first_braced(&segs).expect("a ${ … } in the word")
        };
        // Row three: the `:-` word is `DOLBRACE_WORD`, so the translation goes
        // in as it stands and the `}` it carries is text the scan wrote.
        assert_eq!(braced(r#""${x:-$'a}b'}""#), ("x:-a}b".to_string(), vec![(3, 6)]));
        // The name is `DOLBRACE_PARAM`, which is the third row too.
        assert_eq!(braced(r#""${$'a}b'}""#), ("a}b".to_string(), vec![(0, 3)]));
        // Row two: past a `#` the state is `DOLBRACE_QUOTE`, so the
        // translation is re-quoted and contributes no text of its own.
        assert_eq!(braced(r#""${q#$'a}b'}""#), ("q#'a}b'".to_string(), vec![]));
        assert_eq!(braced(r#""${q%$'z'}""#), ("q%'z'".to_string(), vec![]));
        // Row one: no `P_DQUOTE`, so re-quoted whatever the state.
        assert_eq!(braced("${x:-$'a}b'}"), ("x:-'a}b'".to_string(), vec![]));
        // Nothing to translate, nothing to splice.
        assert_eq!(braced(r#""${x:-a}""#), ("x:-a".to_string(), vec![]));
        // A splice anywhere inside is this segment's, because it is this
        // segment's raw text a leftover would have to be carved out of — and
        // the range moves with the body it was carved from.
        assert_eq!(braced(r#""${a:-${b:-$'x}y'}}""#), ("a:-${b:-x}y}".to_string(), vec![(8, 11)]));
        // The recursion resets the state, so an inner `#` is row two on its
        // own account even where the outer body is row three.
        assert_eq!(braced(r#""${a:-${b#$'x'}}""#), ("a:-${b#'x'}".to_string(), vec![]));
        // Two of them in one body, in order.
        assert_eq!(
            braced(r#""${x:-$'a'p$'b'}""#),
            ("x:-apb".to_string(), vec![(3, 4), (5, 6)]),
        );
    }

    /// A `$( … )` the operand scan meets inside one of those splices was never
    /// parsed — the bytes around it were *written* by the brace scan, not read
    /// by it — so it comes back [`SubBody::Unread`], exactly as one inside a
    /// `' … '` run does. Outside a splice the same spelling is an ordinary
    /// eager read, which is what makes the window and not the operand the unit.
    #[test]
    fn a_splice_is_the_only_part_of_an_operand_no_parser_read() {
        fn kinds(src: &str, unread: &[core::ops::Range<usize>]) -> Vec<SubBody> {
            super::lex_operand_in_dquote(src.as_bytes(), ReadCtx::SOURCE, unread)
                .expect("operand lexes")
                .iter()
                .filter_map(|s| match s {
                    Seg::CmdSub(_, _, kind) => Some(kind.clone()),
                    _ => None,
                })
                .collect()
        }
        // `"${z:-$'$(echo Q)'}"`: the body is `z:-$(echo Q)` and the operand
        // `$(echo Q)`, all nine bytes of it spliced.
        assert_eq!(kinds("$(echo Q)", &[0..9]), vec![SubBody::Unread { closed: true }]);
        // The same operand with no splice behind it is read where it stands.
        assert_eq!(kinds("$(echo Q)", &[]), vec![SubBody::Eager]);
        // A window is a window: one written substitution and one spliced beside
        // it in the same operand keep their own answers.
        assert_eq!(
            kinds("$(a)$(b)", &[4..8]),
            vec![SubBody::Eager, SubBody::Unread { closed: true }],
        );
        // A `' … '` run still speaks for what it covers, splices or none.
        assert_eq!(kinds("'$(a)'", &[]), vec![SubBody::Unread { closed: true }]);
    }
}
