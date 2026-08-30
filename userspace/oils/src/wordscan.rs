//! bash's word **extent** scanners, re-run over a word's source text.
//!
//! bash reads a word more than once. The parser reads it to find where it ends;
//! `expand_word_internal` (subst.c:9989) reads it again to expand it; and on
//! that second pass a double-quoted run is read a *third* time, whole, by
//! `string_extract_double_quoted` (subst.c:963), before any part of it is
//! expanded. The passes do not agree about where a `${ … }` ends, and where
//! they disagree the extent pass wins — it raises
//! ``bad substitution: no closing `}' in %s`` and longjmps out with DISCARD,
//! before the first `$( … )` in the word has run.
//!
//! Two rules make them disagree, and both live in `extract_dollar_brace_string`
//! (subst.c:1813):
//!
//! * **A subscript is skipped wholesale.** At `[`, and only while the scan is
//!   still in the parameter *name* (`dolbrace_state == DOLBRACE_PARAM`), the
//!   scan calls `skipsubscript` (subst.c:1943) and jumps to the matching `]`.
//!   The parser has no such rule, so `"${a[}"` closes at the `}` for the
//!   parser and runs off the end of the word for the extent scan.
//! * **The scan sees the whole word, quotes and all.** `string_extract_double_quoted`
//!   hands `extract_dollar_brace_string` the string *it* was given — the entire
//!   word — not the run it is carving out. So the subscript hunt, and the `}`
//!   hunt after it, both continue past the closing `"`.
//!
//! A third disagreement falls out of the same structure: `$[` is not a
//! construct either double-quote scanner knows (only `${`, `$(` and `` ` ``
//! are), so a `${` written inside a `$[ … ]` inside double quotes is met by
//! the extent scan even though the expander would have handed the text to the
//! arithmetic evaluator.
//!
//! What the diagnostic names is the string the *innermost* `expand_word_internal`
//! was handed: the whole word for a fault the extent scan found, and the run's
//! contents — quotes stripped — for one found while expanding them. That is
//! why `echo "a${a[}b"` names `"a${a[}b"` but `echo "${x:-${a[}}"` names
//! `${x:-${a[}}`. So [`word_fault`] returns the string to name rather than a
//! bare flag, and the recursion carries it.
//!
//! Everything here is a pure function of the word's source bytes, mirroring
//! bash function for bash function; nothing needs shell state, because the
//! extent pass runs before any value is looked up.

use crate::bytes::{BStr, Str};
use bstr::ByteSlice;

/// `dolbrace_state` (parse.y / subst.c): how far into a `${ … }` the scan has
/// got. Only [`DolBrace::Param`] — still inside the parameter name — enables
/// the subscript skip, which is why `${#[}` and `${x:-[}` are unaffected.
///
/// bash has a fifth value, `DOLBRACE_QUOTE2` — the state a `/` moves to, kept
/// apart from `DOLBRACE_QUOTE` so that `singlequote_translations` can
/// single-quote a `$"…"` under `${p/…}` and not under `${p#…}` (parse.y:3912).
/// That is the only test in either file that separates them, it needs a
/// *changed* locale translation to fire, and osh carries no message catalogs
/// (`$"…"` is always its own text), so the two are folded into [`DolBrace::Quote`]
/// here. Every other test — parse.y:3854, parse.y:3866, subst.c:1580,
/// subst.c:1596 — asks `QUOTE || QUOTE2`.
///
/// bash keeps this machine in two places and warns that they must agree
/// ("This logic must agree with subst.c:extract_dollar_brace_string since they
/// share the same defines", parse.y:3803). osh keeps it in one:
/// [`DolBrace::step`] is run both by the *extent* pass below and by the lexer's
/// `${ … }` reader, which needs it to decide whether a `$'…'` in the body is
/// re-quoted after it is translated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DolBrace {
    Param,
    Op,
    Word,
    Quote,
}

impl DolBrace {
    /// The transition for the character just consumed (parse.y:3809–3828,
    /// subst.c:1957–1972).
    ///
    /// `started` is bash's `retind > 1` / `i > start`: the character is not the
    /// body's first. It is what keeps `${#x}`'s leading `#` an operator rather
    /// than the `${p#word}` one — and, because bash tests it *after* appending
    /// the character, it is satisfied by a parameter name as short as one
    /// letter, so `${q#…}` is [`DolBrace::Quote`] just as `${qq#…}` is.
    pub(crate) fn step(self, c: u8, started: bool) -> Self {
        match self {
            // `/` is bash's DOLBRACE_QUOTE2; see the type's doc for why it is
            // not kept apart here.
            Self::Param if started && matches!(c, b'%' | b'#' | b'^' | b',' | b'/') => Self::Quote,
            Self::Param if OPS.contains(&c) => Self::Op,
            Self::Op if !OPS.contains(&c) => Self::Word,
            other => other,
        }
    }
}

/// How deep the mutually recursive scanners below may go before they stop
/// looking. bash caps `${ … }` nesting at `PARAMEXPNEST_MAX` (32) for the same
/// reason; here the concern is the Rust stack, since a word like `${"${"${"…`
/// nests one level every three bytes and a word may be megabytes long.
///
/// Past the cap they report *no* fault, which is the safe direction: the check
/// exists to raise a diagnostic bash raises, so failing to raise it leaves the
/// old behaviour rather than inventing a new one.
const MAX_DEPTH: u32 = 64;

/// The characters that end a parameter name and begin an operator
/// (subst.c:1969 and the `strchr` calls around it).
const OPS: BStr<'static> = b"#%^,~:-=?+/";

/// A construct the expansion's walk over a word never closed.
///
/// Both are raised the same way — `report_error` naming a string, then
/// `exp_jump_to_top_level (DISCARD)` — and both are found by the same
/// left-to-right walk, so whichever comes first is the one bash reports.
#[derive(Debug, PartialEq, Eq)]
pub enum WordFault {
    /// ``bad substitution: no closing `}' in %s`` (subst.c:1980), naming the
    /// string the innermost `expand_word_internal` was handed.
    Brace(Str),
    /// ``bad substitution: no closing "`" in %s`` (subst.c:11290), naming that
    /// string only from the offending backquote onward: the call is
    /// `report_error (…, string + t_index)`.
    Backquote(Str),
}

/// The fault the expansion's walk over `word` meets, or `None` when it closes
/// everything it opens.
///
/// `word` is the word's *source* — what the parser read, quotes included —
/// because that is the string `expand_word_internal` is handed.
///
/// The `${` fast path below is what keeps the backquote fault honest as well as
/// cheap. A backquote nothing closes is an outright *parse* error in any word
/// the parser read as source, so the only way one can survive to be expanded is
/// in a word whose text the parser did not read — and the one thing that writes
/// such text is the bare splice of a translated `$'…'` into a `${ … }` body
/// (see `crate::ast::WordPart::TokenText`), which cannot happen without a `${`.
pub fn word_fault(word: BStr<'_>) -> Option<WordFault> {
    // No `${` in the word, nothing either fault can arise from. This is the
    // fast path for the overwhelming majority of words.
    word.find(b"${")?;
    scan(word, false, 0).err()
}

/// Where the *expansion* ends a `${ … }` body the parser read as `body`.
///
/// The two can only disagree where the parser's scan wrote text into the body
/// that it did not read back: an ANSI-C string spliced in unquoted (see the
/// `$'…'` arm of [`crate::lexer::Lexer::read_dollar_brace_body`]). bash never
/// re-reads what `parse_matched_pair` accumulated either — the `}` that closes
/// the expansion is picked out later, by `parameter_brace_expand` scanning the
/// *word*, and a `}` the splice contributed is as good a terminator there as
/// one the user wrote. Everything past it is ordinary word text; a quote the
/// splice contributed can equally swallow the `}` and leave the brace open.
///
/// `body` is the text between `${` and the parser's `}`. `quoted` is bash's
/// `Q_DOUBLE_QUOTES`, always set here (a bare splice happens only inside
/// double quotes).
pub(crate) fn expansion_body_len(body: BStr<'_>, quoted: bool) -> BraceEnd {
    // The scan below is `parameter_brace_expand`'s, so it is handed the same
    // string that one is: the whole `${ … }` as it stands in the word.
    let mut s: Str = Str::with_capacity(body.len().saturating_add(3));
    s.extend_from_slice(b"${");
    s.extend_from_slice(body);
    s.push(b'}');
    let Ok(end) = brace(&s, 0, quoted, 0) else {
        return BraceEnd::Unclosed;
    };
    // One past the closing `}`, so the body ran from 2 to `end - 1`.
    match end.checked_sub(3) {
        Some(len) if len < body.len() => BraceEnd::Early(len),
        _ => BraceEnd::Same,
    }
}

/// The answer [`expansion_body_len`] reaches.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BraceEnd {
    /// The expansion closes the brace where the parser closed it.
    Same,
    /// It closes earlier, after this many bytes of `body`; `body[len + 1 ..]`,
    /// with the parser's `}` restored on the end, is ordinary word text.
    Early(usize),
    /// It never closes the brace — the word is a runtime
    /// ``bad substitution: no closing `}'``, which [`word_fault`] raises from
    /// the word as a whole.
    Unclosed,
}

/// `expand_word_internal`'s walk over `s`, far enough to find the constructs
/// that consume text. `quoted` is bash's `Q_DOUBLE_QUOTES`: set when `s` is a
/// double-quoted run's contents rather than a word.
///
/// A [`WordFault::Brace`] carries `s` itself — every scanner reached from here
/// is handed `s`, so a fault at any depth below names it — while a
/// [`WordFault::Backquote`] carries `s` from the backquote onward, which is the
/// one place bash narrows the name.
fn scan(s: BStr<'_>, quoted: bool, d: u32) -> Result<(), WordFault> {
    if d > MAX_DEPTH {
        return Ok(());
    }
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            // A single quote is only a quote outside double quotes.
            b'\'' if !quoted => i = skip_single(s, i + 1),
            // `string_extract (…, "`", SX_REQMATCH)` (subst.c:11278): the one
            // call in the walk that insists on its delimiter.
            b'`' => match backquote_extent(s, i + 1) {
                Ok(close) => i = past(close, s),
                // "The test of sindex against t_index is to allow bare
                // instances of ` to pass through, for backwards
                // compatibility" (subst.c:11279). The scan stopping where it
                // started means there was nothing after the backquote to
                // scan, so it is emitted as an ordinary character.
                Err(stop) if stop == i + 1 => i += 1,
                Err(_) => {
                    return Err(WordFault::Backquote(
                        s.get(i..).unwrap_or_default().to_vec(),
                    ));
                }
            },
            b'"' => {
                let (run, next) =
                    dquote_run(s, i + 1, d + 1).map_err(|()| WordFault::Brace(s.to_vec()))?;
                // `expand_word_internal` re-enters itself on the run's
                // contents, and a fault found there names *them*.
                scan(&run, true, d + 1)?;
                i = next;
            }
            b'$' => match s.get(i + 1) {
                Some(b'{') => {
                    i = brace(s, i, quoted, d + 1).map_err(|()| WordFault::Brace(s.to_vec()))?;
                }
                Some(b'(') => i = past(skip_matched(s, i + 2, b'(', b')', d + 1), s),
                // `param_expand` does know `$[ … ]` (the deprecated
                // arithmetic spelling), even though neither double-quote
                // scanner does — which is the whole of the `$[` divergence.
                Some(b'[') => i = past(skip_matched(s, i + 2, b'[', b']', d + 1), s),
                _ => i += 1,
            },
            b'<' | b'>' if s.get(i + 1) == Some(&b'(') => {
                i = past(skip_matched(s, i + 2, b'(', b')', d + 1), s);
            }
            _ => i += 1,
        }
    }
    Ok(())
}

/// One past `close`, clamped: a scanner that ran out returns `s.len()`, and
/// stepping past that would skip the loop's own bound check.
fn past(close: usize, s: BStr<'_>) -> usize {
    close.saturating_add(1).min(s.len())
}

/// `string_extract_double_quoted` (subst.c:963) with `stripdq` clear: carve the
/// run starting at `start` out of `s`, returning its contents and the index
/// just past the closing `"`.
///
/// The contents matter as well as the extent, because they are what
/// `expand_word_internal` re-enters on — and they are not a slice of `s`: a
/// `\"` loses its backslash, and a `${ … }` is copied through by way of
/// `extract_dollar_brace_string`, which is where an unclosed one is caught.
fn dquote_run(s: BStr<'_>, start: usize, d: u32) -> Result<(Str, usize), ()> {
    if d > MAX_DEPTH {
        return Ok((Str::new(), s.len()));
    }
    let mut temp = Str::new();
    let mut i = start;
    let mut backquote = false;
    while i < s.len() {
        let c = s[i];
        // Before the backquote arm, not after it: bash tests `c == '\\'`
        // (subst.c:925) ahead of `if (backquote)` (subst.c:936), so a `` \` ``
        // *inside* a backquote is an escaped backtick and does not close it —
        // ``the characters up to the next backquote that is not preceded by a
        // backslash … defines that command''.
        if c == b'\\' {
            // The backslash is kept in front of everything but a `"`; the
            // string is expanded again afterwards and must still be quoted.
            let Some(&n) = s.get(i + 1) else { break };
            if n != b'"' {
                temp.push(b'\\');
            }
            temp.push(n);
            i += 2;
            continue;
        }
        if backquote {
            backquote = c != b'`';
            temp.push(c);
            i += 1;
            continue;
        }
        match c {
            b'`' => {
                backquote = true;
                temp.push(c);
                i += 1;
            }
            b'$' if matches!(s.get(i + 1), Some(b'(' | b'{')) => {
                let open = s[i + 1];
                let si = if open == b'(' {
                    skip_matched(s, i + 2, b'(', b')', d + 1)
                } else {
                    // The one call that reports: `extract_dollar_brace_string`
                    // is entered here with no `no_longjmp_on_fatal_error`, so
                    // running off the end of the word is fatal rather than a
                    // silent index.
                    edbs(s, i + 2, DolBrace::Param, d + 1)?
                };
                temp.push(b'$');
                temp.push(open);
                temp.extend_from_slice(s.get(i + 2..si.min(s.len())).unwrap_or_default());
                match s.get(si) {
                    Some(&ch) => {
                        temp.push(ch);
                        i = si + 1;
                    }
                    None => i = si,
                }
            }
            b'"' => break,
            _ => {
                temp.push(c);
                i += 1;
            }
        }
    }
    // `if (c) i++;` — step past the closing quote, but not past the end.
    Ok((
        temp,
        i.saturating_add(usize::from(i < s.len())).min(s.len()),
    ))
}

/// `parameter_brace_expand`'s *scan* (subst.c:9539–9915), which is all of it
/// that can raise this fault: read the name, then, if an operator ended it,
/// hand the rest to `extract_dollar_brace_string`.
///
/// `dollar` indexes the `$` of a `${`. Returns the index just past the closing
/// `}`.
///
/// The initial `dolbrace_state` is the point: that call passes `SX_WORD`
/// (subst.c:9910), so the scan begins in [`DolBrace::Word`] — or, for the
/// operators that also take `SX_POSIXEXP` inside double quotes, in
/// [`DolBrace::Quote`]. Neither enables the subscript skip. Only a *nested*
/// `${` resets the state to [`DolBrace::Param`], which is why the fault needs
/// two levels of braces to appear on this path (`${x:-${a[}}`) but only one on
/// the double-quote path (`"${a[}"`).
fn brace(s: BStr<'_>, dollar: usize, quoted: bool, d: u32) -> Result<usize, ()> {
    if d > MAX_DEPTH {
        return Ok(s.len());
    }
    let name = dollar + 2;
    // `${#var}` takes no operator at all: its name runs to the `}`
    // (subst.c:9560), which is why `${#a[}` is a plain bad substitution.
    let stops: BStr<'_> = if s.get(name) == Some(&b'#')
        && s.get(name + 1)
            .is_some_and(|&c| c.is_ascii_alphabetic() || c == b'_')
    {
        b"}"
    } else {
        b"#%^,~:-=?+/@}"
    };
    let end = extract_name(s, name, stops, d);
    let Some(&c) = s.get(end) else {
        // No `}` and no operator: the parser refused this word long before,
        // so nothing here can be reached.
        return Ok(s.len());
    };
    if c == b'}' {
        return Ok(end + 1);
    }
    let posixexp = matches!(c, b'%' | b'#' | b'/' | b'^' | b',' | b':');
    let state = if quoted && posixexp {
        DolBrace::Quote
    } else {
        DolBrace::Word
    };
    edbs(s, end, state, d + 1).map(|close| past(close, s))
}

/// `string_extract` with `SX_VARNAME` (subst.c:795): the index at which the
/// parameter name ends — the first character of `stops` not inside a subscript.
///
/// This scan skips a subscript too, but *only* when it is closed: an
/// unterminated `[` is stepped over rather than run past, which is why
/// `echo ${a[}tail` is an ordinary bad substitution.
fn extract_name(s: BStr<'_>, start: usize, stops: BStr<'_>, d: u32) -> usize {
    if d > MAX_DEPTH {
        return s.len();
    }
    let mut i = start;
    while i < s.len() {
        match s[i] {
            b'\\' => {
                if i + 1 >= s.len() {
                    break;
                }
                i += 1;
            }
            b'[' => {
                let ni = skip_matched(s, i + 1, b'[', b']', d + 1);
                if s.get(ni) == Some(&b']') {
                    i = ni;
                }
            }
            c if stops.contains(&c) => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

/// `extract_dollar_brace_string` (subst.c:1813): the index of the `}` that
/// closes the `${` whose body starts at `start`, or `Err` when the scan runs
/// off the end of `s` with braces still open.
///
/// `Err` is bash's `report_error (…, "}", string); exp_jump_to_top_level (DISCARD)`
/// at subst.c:1975 — reached whenever `no_longjmp_on_fatal_error` is clear,
/// which is every call made from here.
fn edbs(s: BStr<'_>, start: usize, state0: DolBrace, d: u32) -> Result<usize, ()> {
    if d > MAX_DEPTH {
        return Ok(s.len());
    }
    let mut state = state0;
    // The saved state per nesting level (bash's `dbstate`).
    let mut stack: Vec<DolBrace> = Vec::new();
    let mut nesting = 1usize;
    let mut i = start;
    while i < s.len() {
        let c = s[i];
        // A backslash quotes the next character, whatever it is.
        if c == b'\\' {
            i += 2;
            continue;
        }
        if c == b'$' && s.get(i + 1) == Some(&b'{') {
            stack.push(state);
            nesting += 1;
            i += 2;
            // A nested `${` starts a new name, so the subscript skip is live
            // again inside it.
            if matches!(state, DolBrace::Quote | DolBrace::Word) {
                state = DolBrace::Param;
            }
            continue;
        }
        if c == b'}' {
            nesting -= 1;
            if nesting == 0 {
                return Ok(i);
            }
            state = stack.pop().unwrap_or(state0);
            i += 1;
            continue;
        }
        if c == b'`' {
            // `string_extract (string, &si, "`", …)` (subst.c:1884): to the
            // next backtick — and that one "understand[s] about backslashes in
            // the string" (subst.c:788, 812), so a `` \` `` does not end it.
            // Without a match it stops where it ran out and the caller still
            // steps one past, which is what `Err`'s index is for.
            i = past(backquote_extent(s, i + 1).unwrap_or_else(|stop| stop), s);
            continue;
        }
        if c == b'$' && s.get(i + 1) == Some(&b'(')
            || matches!(c, b'<' | b'>') && s.get(i + 1) == Some(&b'(')
        {
            let si = skip_matched(s, i + 2, b'(', b')', d + 1);
            if si >= s.len() {
                break;
            }
            i = si + 1;
            continue;
        }
        if c == b'"' {
            // `skip_double_quoted` — and *it* may meet a `${` of its own, on
            // the same string, still without `no_longjmp_on_fatal_error`.
            i = skip_dq(s, i + 1, true, d + 1)?;
            continue;
        }
        if c == b'\'' {
            i = skip_single(s, i + 1);
            continue;
        }
        if c == b'[' && state == DolBrace::Param {
            let si = skip_matched(s, i + 1, b'[', b']', d + 1);
            if si >= s.len() {
                // `CHECK_STRING_OVERRUN` (subst.c:135): the subscript hunt ran
                // past the end of the word, and there is no `}` after it.
                return Err(());
            }
            if s[si] == b']' {
                i = si;
            }
        }
        // The state machine runs on the character just consumed, and the
        // length guard is against the body's own start (subst.c:1957).
        let started = i > start;
        i += 1;
        state = state.step(c, started);
    }
    Err(())
}

/// `skip_double_quoted` (subst.c:1020): the index just past the `"` that closes
/// the run starting at `start`.
///
/// `report` is the absence of `no_longjmp_on_fatal_error`. It is clear only
/// when this runs inside `skip_matched_pair`, which sets the flag for the whole
/// subscript hunt — so an unclosed `${` met while skipping a subscript is
/// silent, and the same one met anywhere else is fatal.
fn skip_dq(s: BStr<'_>, start: usize, report: bool, d: u32) -> Result<usize, ()> {
    if d > MAX_DEPTH {
        return Ok(s.len());
    }
    let mut i = start;
    let mut backquote = false;
    while i < s.len() {
        let c = s[i];
        // `else if (c == '\\')` at subst.c:1044, ahead of `else if (backquote)`
        // at subst.c:1050: a `` \` `` does not close a backquote.
        if c == b'\\' {
            i += 2;
            continue;
        }
        if backquote {
            backquote = c != b'`';
            i += 1;
            continue;
        }
        match c {
            b'`' => {
                backquote = true;
                i += 1;
            }
            b'$' if matches!(s.get(i + 1), Some(b'(' | b'{')) => {
                let si = if s[i + 1] == b'(' {
                    skip_matched(s, i + 2, b'(', b')', d + 1)
                } else {
                    match edbs(s, i + 2, DolBrace::Param, d + 1) {
                        Ok(close) => close,
                        Err(()) if report => return Err(()),
                        Err(()) => s.len(),
                    }
                };
                if si >= s.len() {
                    return Ok(s.len());
                }
                i = si + 1;
            }
            b'"' => break,
            _ => i += 1,
        }
    }
    Ok(i.saturating_add(usize::from(i < s.len())).min(s.len()))
}

/// `skip_single_quoted` (subst.c:1131): the index just past the closing `'`.
fn skip_single(s: BStr<'_>, start: usize) -> usize {
    past(skip_to(s, start, b'\''), s)
}

/// `string_extract (…, "`", SX_REQMATCH)` (subst.c:794): the index of the
/// backtick that closes the one just before `start`, where a `` \` `` does not
/// close it; `Err` carries the index the scan stopped at when there is none.
///
/// The stop index is not a detail the caller can drop: bash's test for a *bare*
/// backquote is `sindex - 1 == t_index`, which is this index against the
/// backquote's own, so a scan that got nowhere is told apart from one that ran
/// out further along.
fn backquote_extent(s: BStr<'_>, start: usize) -> Result<usize, usize> {
    let mut i = start;
    while let Some(&c) = s.get(i) {
        match c {
            // `if (string[i + 1]) i++; else break;`: a backslash with nothing
            // after it ends the scan *on the backslash*, not past it.
            b'\\' if i + 1 < s.len() => i += 2,
            b'\\' => return Err(i),
            b'`' => return Ok(i),
            _ => i += 1,
        }
    }
    Err(i)
}

/// The index of the first `ch` at or after `start`, or `s.len()`.
fn skip_to(s: BStr<'_>, start: usize, ch: u8) -> usize {
    s.get(start..)
        .and_then(|t| t.find_byte(ch))
        .map_or(s.len(), |off| start + off)
}

/// The stretches of `s` in which `brace_gobbler` would read a command
/// substitution — that is, where its `quoted` is `0` or `"`.
///
/// The gobbler's quoting state is a **flat** character loop, and one byte wide:
///
/// ```c
///   if (c == '\\' && quoted != '\'') { i += 2; continue; }   /* pass_next */
///   if (c == '$' && text[i+1] == '{' && quoted != '\'')      /* like \{ */
///     { pass_next = 1; i++; continue; }
///   if (quoted)
///     {
///       if (c == quoted) quoted = 0;
///       if (quoted == '"' && c == '$' && text[i+1] == '(') goto comsub;
///       ADVANCE_CHAR (text, tlen, i); continue;
///     }
///   if (c == '"' || c == '\'' || c == '`') { quoted = c; i++; continue; }
///   if ((c == '$' || c == '<' || c == '>') && text[i+1] == '(')
///     { comsub: si = i + 2; extract_command_subst (text, &si, 0); i = si + 1; }
///                                                  /* braces.c:637-682 */
/// ```
///
/// Nothing about it nests: a `"` met while `quoted` is `"` clears the state
/// outright, and a `'` or `` ` `` met while it is clear opens a stretch the
/// `$(` row cannot fire in — however deeply the *parse* thought that byte was
/// nested. So a reader that walks a parsed word structurally, as
/// [`crate::interp::Shell::gobbled_subs`] does, has to be told which stretches
/// of a run's text the scan was actually reading in.
///
/// `dquoted` is the state on entry: `true` for text the scan reached from
/// inside `" … "`, which is every caller's case, since that is the only way it
/// walks into a quoted run at all.
///
/// The returned ranges are in order, disjoint, and cover everything the scan
/// reads; a `$( … )` extent belongs whole to the range it starts in, because
/// `extract_command_subst` consumes it whole and a quote inside it flips
/// nothing. A range may be empty — an unreadable stretch that runs to the end
/// of `s` leaves an empty one behind it, and an empty `s` answers with one
/// empty range rather than none, since the scan does read all nothing of it.
pub(crate) fn gobbler_readable(s: BStr<'_>, dquoted: bool) -> Vec<core::ops::Range<usize>> {
    let mut out: Vec<core::ops::Range<usize>> = Vec::new();
    let mut quoted: u8 = if dquoted { b'"' } else { 0 };
    // Where the stretch in progress began, once there is one. The entry state
    // is always readable, so there is one from the first byte.
    let mut open = Some(0usize);
    let mut i = 0usize;
    while i < s.len() {
        let c = s[i];
        let next = s.get(i.saturating_add(1)).copied();
        // A backslash passes the next character over — but not inside `' … '`,
        // where bash's `pass_next` row is gated on `quoted != '\''`.
        if c == b'\\' && quoted != b'\'' {
            i = i.saturating_add(2);
            continue;
        }
        // `${` is treated like `\{`: the `{` is passed over, and no state of
        // its own is opened. So what is between the braces is scanned in
        // whatever quoting was already in force.
        if c == b'$' && next == Some(b'{') && quoted != b'\'' {
            i = i.saturating_add(2);
            continue;
        }
        let comsub = if quoted == 0 {
            if matches!(c, b'"' | b'\'' | b'`') {
                // A stretch the `$(` row cannot fire in starts here, and the
                // opening quote belongs to it.
                if c != b'"'
                    && let Some(from) = open.take()
                {
                    out.push(from..i);
                }
                quoted = c;
                i = i.saturating_add(1);
                continue;
            }
            matches!(c, b'$' | b'<' | b'>') && next == Some(b'(')
        } else {
            if c == quoted {
                quoted = 0;
                i = i.saturating_add(1);
                // …and the closing quote belongs to it too, so a readable
                // stretch resumes only after it.
                if open.is_none() {
                    open = Some(i);
                }
                continue;
            }
            // Only the `"` state reaches the comsub row; `'` and `` ` `` do
            // not, which is the whole of the disagreement this exists for.
            quoted == b'"' && c == b'$' && next == Some(b'(')
        };
        if comsub {
            // `extract_command_subst` takes the extent whole. Everything in it
            // is the substitution's own business, quotes included.
            i = past(skip_matched(s, i.saturating_add(2), b'(', b')', 0), s);
            continue;
        }
        i = i.saturating_add(1);
    }
    if let Some(from) = open {
        out.push(from..s.len());
    }
    out
}

/// Where each `<( … )` / `>( … )` `brace_gobbler` reads in `s` begins — the
/// index of the `<` or `>` itself.
///
/// The gobbler's comsub row names all three spellings in one breath —
/// `(c == '$' || c == '<' || c == '>') && text[i+1] == '('` (braces.c:675) — but
/// only from the *unquoted* state. The `quoted` branch has a row of its own and
/// it is the `$(` alone (`quoted == '"' && c == '$' && text[i+1] == '('`,
/// braces.c:668), so a `<(` written inside `" … "` is not read and a `<(`
/// written where the state has been cleared again is.
///
/// That clearing is why this exists as a question at all. Nothing about the
/// state nests: a `"` met while `quoted` is `"` sets it to 0 outright, and a
/// `${` opens no state of its own, so the *inner* quotes of `"${z:-"<(fi)"}"`
/// leave the scan unquoted over the `<(` — which the parser, whose `" … "` is a
/// recursive `parse_matched_pair` with no such row, read as characters. So this
/// names a substitution no [`crate::ast::WordPart`] stands for, and
/// [`crate::interp::Shell::gobble_scan`] has nowhere but the text to find it.
///
/// A `$( … )` is skipped whole rather than reported: it is the one spelling the
/// parse *did* read, so a part already stands for it wherever it is readable.
///
/// `dquoted` is the state on entry, as in [`gobbler_readable`].
pub(crate) fn gobbler_procsubs(s: BStr<'_>, dquoted: bool) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut quoted: u8 = if dquoted { b'"' } else { 0 };
    let mut i = 0usize;
    while i < s.len() {
        let c = s[i];
        let next = s.get(i.saturating_add(1)).copied();
        if c == b'\\' && quoted != b'\'' {
            i = i.saturating_add(2);
            continue;
        }
        if c == b'$' && next == Some(b'{') && quoted != b'\'' {
            i = i.saturating_add(2);
            continue;
        }
        let (comsub, procsub) = if quoted == 0 {
            if matches!(c, b'"' | b'\'' | b'`') {
                quoted = c;
                i = i.saturating_add(1);
                continue;
            }
            let row = matches!(c, b'$' | b'<' | b'>') && next == Some(b'(');
            (row, row && c != b'$')
        } else {
            if c == quoted {
                quoted = 0;
                i = i.saturating_add(1);
                continue;
            }
            (quoted == b'"' && c == b'$' && next == Some(b'('), false)
        };
        if comsub {
            if procsub {
                out.push(i);
            }
            // `extract_command_subst` takes the extent whole, quotes included —
            // and where it stops is the caller's business, not this scan's: the
            // real answer is a parse, and only the *reported* substitutions are
            // worth one. Skipping on a paren count is what [`gobbler_readable`]
            // does for the same purpose.
            i = past(skip_matched(s, i.saturating_add(2), b'(', b')', 0), s);
            continue;
        }
        i = i.saturating_add(1);
    }
    out
}

/// `skipsubscript (string, start, 0)` (subst.c:2166) — where the `]` that
/// matches the `[` at `start` is, or `s.len()` when there is none.
///
/// `start` addresses the `[` itself, which is bash's calling convention when it
/// does not pass flag 2 (`i = (flags & 2) ? start : start + 1`, subst.c:2093).
pub(crate) fn skip_subscript(s: BStr<'_>, start: usize) -> usize {
    skip_matched(s, start.saturating_add(1), b'[', b']', 0)
}

/// `skip_matched_pair` (subst.c:2085), which `skipsubscript` is a `[`/`]`
/// instance of: the index of the `close` that matches an already-consumed
/// `open`, or `s.len()` when there is none.
///
/// It never reports: bash sets `no_longjmp_on_fatal_error` for its whole
/// extent, so an unterminated `${` met inside a subscript is only an index.
fn skip_matched(s: BStr<'_>, start: usize, open: u8, close: u8, d: u32) -> usize {
    if d > MAX_DEPTH {
        return s.len();
    }
    let mut count = 1usize;
    let mut i = start;
    let mut backquote = false;
    while i < s.len() {
        let c = s[i];
        // `else if ((flags & 1) == 0 && c == '\\')` at subst.c:2093, ahead of
        // `else if (backq)` at subst.c:2099. (Flag 1 is the associative-array
        // key, whose subscript is taken literally; every `skipsubscript` call on
        // this path passes 0 — subst.c:824, 1945, 10861.)
        if c == b'\\' {
            i += 2;
            continue;
        }
        if backquote {
            backquote = c != b'`';
            i += 1;
            continue;
        }
        if c == b'`' {
            backquote = true;
            i += 1;
            continue;
        }
        if c == open {
            count += 1;
            i += 1;
            continue;
        }
        if c == close {
            count -= 1;
            if count == 0 {
                return i;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => i = skip_single(s, i + 1),
            b'"' => i = skip_dq(s, i + 1, false, d + 1).unwrap_or(s.len()),
            b'$' if matches!(s.get(i + 1), Some(b'(' | b'{')) => {
                let si = if s[i + 1] == b'(' {
                    skip_matched(s, i + 2, b'(', b')', d + 1)
                } else {
                    edbs(s, i + 2, DolBrace::Param, d + 1).unwrap_or(s.len())
                };
                i = past(si, s);
            }
            _ => i += 1,
        }
    }
    s.len()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{
        BraceEnd, WordFault, expansion_body_len, gobbler_procsubs, gobbler_readable,
        skip_subscript, word_fault,
    };

    /// The stretches [`gobbler_readable`] answers with, as the text in them.
    fn readable(src: &str, dquoted: bool) -> Vec<&str> {
        gobbler_readable(src.as_bytes(), dquoted)
            .into_iter()
            .map(|r| src.get(r).unwrap())
            .collect()
    }

    #[test]
    fn the_gobblers_flat_quoting_says_which_stretches_of_a_run_it_reads() {
        // Entry state `"`, which is how every caller reaches a quoted run: a
        // plain stretch is read whole, the `$( … )` in it included.
        assert_eq!(readable("a$(fi)b", true), ["a$(fi)b"]);
        // A quote *inside* an extent flips nothing — `extract_command_subst`
        // takes the extent whole and the scan resumes past it.
        assert_eq!(readable("a$(echo '`')b", true), ["a$(echo '`')b"]);
        // A `"` met while the state is `"` clears it outright; it does not
        // nest. What follows is still read, because `0` reaches the `$(` row
        // just as `"` does.
        assert_eq!(readable(r#"a"b$(fi)"#, true), [r#"a"b$(fi)"#]);
        // …but with the state cleared, a backquote opens a stretch the `$(`
        // row cannot fire in. This is the one the fill's gate declines.
        assert_eq!(readable("a\"`$(fi)`", true), ["a\"", ""]);
        // A `'` does the same, and reading resumes after the closing quote.
        assert_eq!(readable("a\"'$(fi)'z", true), ["a\"", "z"]);
        assert_eq!(readable("a\"`x`b$(fi)", true), ["a\"", "b$(fi)"]);
        // At the top level the state starts clear, so the `$(` row fires…
        assert_eq!(readable("a$(fi)", false), ["a$(fi)"]);
        // …and a `"` there opens the readable state rather than closing one.
        assert_eq!(readable(r#"a"$(fi)""#, false), [r#"a"$(fi)""#]);
        // …while a `'` there is a quote, and hides what it holds.
        assert_eq!(readable("a'$(fi)'b", false), ["a", "b"]);
        // A backslash passes the next byte over, so a quote can be hidden…
        assert_eq!(readable(r#"a\`$(fi)"#, true), [r#"a\`$(fi)"#]);
        // …except inside `' … '`, where bash's `pass_next` row is not reached,
        // so the `\` is an ordinary byte and the next `'` closes the run.
        assert_eq!(readable(r#"a"'\'$(fi)"#, true), ["a\"", "$(fi)"]);
        // `${` is passed over like `\{` and opens no state of its own, so the
        // body is scanned in whatever quoting was already in force.
        assert_eq!(readable("${z#$(fi)}", true), ["${z#$(fi)}"]);
        // Nothing at all is read whole, trivially — one empty stretch, not none.
        assert_eq!(readable("", true), [""]);
    }

    /// The process substitutions the gobbler reads in `src`, as the text of each
    /// from its `<`/`>` to the end of the string.
    fn procsubs(src: &str, dquoted: bool) -> Vec<&str> {
        gobbler_procsubs(src.as_bytes(), dquoted)
            .into_iter()
            .filter_map(|i| src.get(i..))
            .collect()
    }

    #[test]
    fn a_process_substitution_is_read_only_where_the_flat_quoting_is_clear() {
        // The unquoted row takes all three spellings; only two are collected,
        // the `$(` being one a part already stands for.
        assert_eq!(procsubs("a<(fi)b", false), ["<(fi)b"]);
        assert_eq!(procsubs("a>(fi)b", false), [">(fi)b"]);
        assert_eq!(procsubs("a$(fi)b", false), [] as [&str; 0]);
        // Inside `" … "` the row is the `$(` alone, so nothing is read…
        assert_eq!(procsubs("a<(fi)b", true), [] as [&str; 0]);
        // …until a `"` clears the state, which is not a nesting but a flip.
        assert_eq!(procsubs(r#"${z:-"<(fi)"}"#, true), [r#"<(fi)"}"#]);
        assert_eq!(procsubs(r#"${z:-"a"<(fi)"b"}"#, true), [] as [&str; 0]);
        assert_eq!(procsubs(r#"${z:-"${y:-"<(fi)"}"}"#, true), [] as [&str; 0]);
        // A `'` or a `` ` `` met with the state clear hides what it holds.
        assert_eq!(procsubs(r#"${z:-"'<(fi)'"}"#, true), [] as [&str; 0]);
        assert_eq!(procsubs("${z:-\"`<(fi)`\"}", true), [] as [&str; 0]);
        // A backslash passes the `<` over.
        assert_eq!(procsubs(r#"${z:-"\<(fi)"}"#, true), [] as [&str; 0]);
        // An extent is stepped over whole, whichever spelling opened it, so one
        // written inside another is the outer one's business and not read here.
        assert_eq!(procsubs("$(x <(fi))<(y)", false), ["<(y)"]);
        assert_eq!(
            procsubs("<(x <(fi))<(y)", false),
            ["<(x <(fi))<(y)", "<(y)"]
        );
        // Two in a row, in the order the scan meets them.
        assert_eq!(
            procsubs(r#"${z:-"<(a)""<(b)"}"#, true),
            [r#"<(a)""<(b)"}"#, r#"<(b)"}"#]
        );
        // One that never closes is still read; where its body ends is the
        // reader's question, not the scan's.
        assert_eq!(procsubs(r#"${z:-"<(fi"}"#, true), [r#"<(fi"}"#]);
    }

    /// The subscript `skipsubscript` reads out of `src`, whose `[` is at `open`,
    /// or `None` when it never closes.
    fn subscript(src: &str, open: usize) -> Option<&str> {
        let close = skip_subscript(src.as_bytes(), open);
        (src.as_bytes().get(close) == Some(&b']')).then(|| src.get(open + 1..close))?
    }

    #[test]
    fn a_subscript_closes_where_skip_matched_pair_says_and_not_where_a_count_would() {
        assert_eq!(subscript("a[1]", 1), Some("1"));
        // Brackets nest, so the *last* `]` closes the outer one.
        assert_eq!(subscript("a[b[0]]", 1), Some("b[0]"));
        // A backslash hides the character after it — both an open and a close.
        // This is the shape an arithmetic word expansion writes, having
        // backslash-quoted the subscript it expanded in place.
        assert_eq!(subscript(r"a[\[]", 1), Some(r"\["));
        assert_eq!(subscript(r"a[\]]", 1), Some(r"\]"));
        assert_eq!(subscript(r"a[1\]]", 1), Some(r"1\]"));
        // …and a quoted run is stepped over whole.
        assert_eq!(subscript(r#"a["]"]"#, 1), Some(r#""]""#));
        assert_eq!(subscript("a[']']", 1), Some("']'"));
        assert_eq!(subscript("a[$(echo ])]", 1), Some("$(echo ])"));
        assert_eq!(subscript("a[`echo ]`]", 1), Some("`echo ]`"));
        // One that never closes is not a subscript at all.
        assert_eq!(subscript("a[[]", 1), None);
        assert_eq!(subscript(r"a[\]", 1), None);
        assert_eq!(subscript("a[\"]", 1), None);
    }

    /// The string a `` no closing `}' `` diagnostic would name, for a word whose
    /// fault is a brace.
    fn named(src: &str) -> Option<String> {
        match word_fault(src.as_bytes()) {
            Some(WordFault::Brace(s)) => Some(String::from_utf8(s).unwrap()),
            Some(WordFault::Backquote(s)) => {
                panic!(
                    "expected a brace fault, got a backquote one naming {:?}",
                    s.as_slice()
                )
            }
            None => None,
        }
    }

    /// The string a ``no closing "`"`` diagnostic would name.
    fn backquote_named(src: &str) -> Option<String> {
        match word_fault(src.as_bytes()) {
            Some(WordFault::Backquote(s)) => Some(String::from_utf8(s).unwrap()),
            _ => None,
        }
    }

    /// Where `parameter_brace_expand` closes the body the parser read as `body`.
    fn ends(body: &str) -> BraceEnd {
        expansion_body_len(body.as_bytes(), true)
    }

    #[test]
    fn a_body_the_expansion_reads_the_same_way_ends_where_the_parser_ended_it() {
        assert_eq!(ends("x:-a"), BraceEnd::Same);
        assert_eq!(ends("x"), BraceEnd::Same);
        assert_eq!(ends(""), BraceEnd::Same);
        // A `}` the *user* wrote inside a nested construct is not a terminator
        // for either read.
        assert_eq!(ends("x:-$(echo })"), BraceEnd::Same);
        assert_eq!(ends("a:-${b:-x}"), BraceEnd::Same);
    }

    #[test]
    fn a_spliced_brace_closes_the_expansion_early() {
        // `"${x:-$'a}b'}"` — the scan wrote `a}b` into the body and never read
        // it back, so the expansion is `${x:-a}` and `b}` is word text.
        assert_eq!(ends("x:-a}b"), BraceEnd::Early(4));
        // The first one wins; the rest is text however many there are.
        assert_eq!(ends("x:-a}b}c"), BraceEnd::Early(4));
        // A splice in the *name* is the same row of bash's table.
        assert_eq!(ends("a}b"), BraceEnd::Early(1));
        // `"${a:-${b:-$'x}y'}}"`: the inner splice closes the *outer* brace, so
        // the leftover is just the parser's own `}`.
        assert_eq!(ends("a:-${b:-x}y}"), BraceEnd::Early(11));
    }

    #[test]
    fn a_spliced_quote_swallows_the_brace_instead() {
        // The run opened by the spliced quote never ends, so the scan runs off
        // the end of the word and the expansion dies on the missing `}`.
        assert_eq!(ends("x:-a'b"), BraceEnd::Unclosed);
        assert_eq!(ends("x:-'"), BraceEnd::Unclosed);
        // Inside double quotes a `"` and a backtick are as live as the quote.
        assert_eq!(ends("x:-a\"b"), BraceEnd::Unclosed);
        assert_eq!(ends("x:-a`b"), BraceEnd::Unclosed);
    }

    #[test]
    fn a_subscript_with_no_close_runs_off_the_end_of_the_word() {
        // The extent scan skips the subscript, not the `}` it was written
        // before — so the `${` is never closed and the *word* is named.
        assert_eq!(named(r#""${a[}""#).as_deref(), Some(r#""${a[}""#));
        assert_eq!(named(r#""${a[}"tail"#).as_deref(), Some(r#""${a[}"tail"#));
        assert_eq!(
            named(r#""pre${a[}post""#).as_deref(),
            Some(r#""pre${a[}post""#)
        );
        assert_eq!(named(r#""${a[0}""#).as_deref(), Some(r#""${a[0}""#));
        assert_eq!(named(r#""${a[ }""#).as_deref(), Some(r#""${a[ }""#));
        // A backslash hides the `]` from `skipsubscript` too.
        assert_eq!(named(r#""${a[\]}""#).as_deref(), Some(r#""${a[\]}""#));
        // Finding the `]` is only half of it: the scan resumes after it and
        // still needs a `}`, which this word does not have either.
        assert_eq!(named(r#""${a[}"x"]""#).as_deref(), Some(r#""${a[}"x"]""#));
    }

    #[test]
    fn only_a_double_quoted_one_is_a_runaway() {
        // Unquoted, the `${` never meets the extent scan at all.
        assert_eq!(named("${a[}tail"), None);
        assert_eq!(named("echo ${a[}"), None);
        // `${#…}` moves out of DOLBRACE_PARAM before the `[` is reached.
        assert_eq!(named(r#""${#a[}""#), None);
    }

    #[test]
    fn a_closed_subscript_is_not_one() {
        assert_eq!(named(r#""${a[0]}""#), None);
        assert_eq!(named(r#""${a[@]}" "${#a[@]}" "${!a[@]}""#), None);
        assert_eq!(named(r#""${a[$(echo 0)]}""#), None);
        assert_eq!(named(r#""${a[']']}""#), None);
        assert_eq!(named(r#""${v/[/y}""#), None);
        assert_eq!(named(r#""${v//[}/z}""#), None);
        // The `]` may live anywhere in the word, even past the closing quote —
        // and once the subscript is closed the `}` after it closes the brace.
        assert_eq!(named(r#""${a[}"x"]}""#), None);
    }

    #[test]
    fn the_name_is_the_string_the_expansion_was_handed() {
        // Found by the extent scan: the whole word, quotes included.
        assert_eq!(named(r#""a${a[}b""#).as_deref(), Some(r#""a${a[}b""#));
        assert_eq!(
            named(r#""${a[}"$(echo hi)"#).as_deref(),
            Some(r#""${a[}"$(echo hi)"#)
        );
        // Found one level in, while expanding the run: its contents, without
        // the quotes, because that is what `expand_word_internal` was handed.
        assert_eq!(named(r#""${x:-${a[}}""#).as_deref(), Some("${x:-${a[}}"));
        // Found by the `${x:- … }` body scan over the unquoted word.
        assert_eq!(
            named(r#"${x:-"${a[}"}"#).as_deref(),
            Some(r#"${x:-"${a[}"}"#)
        );
    }

    #[test]
    fn a_dollar_bracket_is_not_a_construct_the_quote_scanner_knows() {
        // `string_extract_double_quoted` knows `${`, `$(` and a backtick — not
        // `$[` — so the `${` inside the arithmetic is met and never closed.
        assert_eq!(named(r#""$[ ${ ]""#).as_deref(), Some(r#""$[ ${ ]""#));
        assert_eq!(
            named(r#""x$[ ${x:- ]y""#).as_deref(),
            Some(r#""x$[ ${x:- ]y""#)
        );
        // With the brace closed the scan steps over it, and the arithmetic is
        // the expander's business again.
        assert_eq!(named(r#""$[ ${x!} ]""#), None);
        // `$((` starts `$(`, which the scanner does know.
        assert_eq!(named(r#""$(( ${x:- ))""#), None);
    }

    #[test]
    fn an_ordinary_word_is_not_scanned_into_a_fault() {
        for src in [
            "echo",
            r#""$v""#,
            r#""${v}""#,
            r#""${v:-d}""#,
            r#""${v//a/b}""#,
            r#""$(echo "${v}")""#,
            r#""`echo hi`""#,
            r#""a\"b${v}""#,
            "'${a[}'",
            r#""${v-}${w+}""#,
        ] {
            assert_eq!(named(src), None, "{src}");
        }
    }

    #[test]
    fn a_deeply_nested_word_stops_at_the_depth_cap() {
        // `edbs` and `skip_dq` call each other, and a word can drive them one
        // level deeper every three bytes — so without the cap a long enough
        // word exhausts the Rust stack rather than reporting anything. The
        // assertion is that the call returns at all; a stack overflow aborts
        // the process, so reaching the `assert` is the test.
        // `"${"${" … x … "}"}"` — balanced, so nothing but the depth itself
        // can end the scan.
        let n = 100_000;
        let src = format!("\"{}x{}\"", "${\"".repeat(n), "\"}".repeat(n));
        let deep = word_fault(src.as_bytes());
        // Reaching this line *is* the assertion — a stack overflow aborts the
        // process rather than failing a test. Which answer comes back past the
        // cap is deliberately unpinned: this is far past any word bash would
        // still be scanning (it caps `${ … }` nesting at 32), so there is no
        // measurement to be faithful to.
        assert!(deep.is_none_or(|fault| match fault {
            WordFault::Brace(named) | WordFault::Backquote(named) => named.len() <= src.len(),
        }));
    }

    #[test]
    fn a_backquote_the_walk_never_closes_is_named_from_the_backquote_on() {
        // Only reachable in a word whose text the parser did not read, which
        // is why every case here carries a `${ … }`: the walk's fast path is
        // the `${`, and a bare splice is the one thing that writes such text.
        assert_eq!(
            backquote_named(r#""${x:-a}b"c`echo Z}""#),
            Some("`echo Z}\"".to_string())
        );
        // Closed, so nothing to report.
        assert_eq!(backquote_named(r#""${x:-a}b"c`echo Z`}""#), None);
        // "Bare instances of ` … pass through, for backwards compatibility":
        // the backquote is the word's last character.
        assert_eq!(backquote_named("\"${x:-a}\"`"), None);
        // A `\`` does not close it.
        assert_eq!(
            backquote_named(r#""${x:-a}"`echo \`z"#),
            Some("`echo \\`z".to_string())
        );
        // The brace fault comes first in the walk, so it is the one reported.
        assert_eq!(named("\"${a[}\"`echo"), Some("\"${a[}\"`echo".to_string()));
    }

    #[test]
    fn a_backslash_is_honoured_inside_a_backquote_by_every_scanner() {
        // ``the characters up to the next backquote that is not preceded by a
        // backslash'' — all three scanners test `c == '\\'` before they test
        // their backquote flag (subst.c:925/936, 1044/1050, 2093/2099), so the
        // `` \` `` below is an escaped backtick and the backquote runs past it
        // to the real one. Read the other way round the backquote closes early,
        // the `"` after it is taken for the run's own close, and the scan walks
        // out of step with the parser into a fault nothing is wrong with.
        for src in [
            // `string_extract_double_quoted`: the run is the word's own.
            r#""${z:-q}`echo m\`echo n\``B""#,
            // `skip_double_quoted`: the run is inside a `${ … }` operand.
            r#""${z:-A"`echo m\`echo n\``"B}""#,
            // `skip_matched_pair`: the backquote is inside a subscript.
            r#""${a[`echo 1\`echo \``]}""#,
            // The escaped backtick is the last thing before the closing one.
            r#""${z:-a}`echo \``""#,
        ] {
            assert_eq!(named(src), None, "{src}");
            assert_eq!(backquote_named(src), None, "{src}");
        }
        // And the flag is still a flag: an *unescaped* backtick inside the run
        // does close it, so this word's `${` is the one left open.
        assert_eq!(
            named(r#""${z:-A`echo m`${a[}""#).as_deref(),
            Some(r#""${z:-A`echo m`${a[}""#)
        );
    }
}
