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
//! `${x:-${a[}}`. So [`unclosed_brace`] returns the string to name rather than
//! a bare flag, and the recursion carries it.
//!
//! Everything here is a pure function of the word's source bytes, mirroring
//! bash function for bash function; nothing needs shell state, because the
//! extent pass runs before any value is looked up.

use crate::bytes::{BStr, Str};
use bstr::ByteSlice;

/// `dolbrace_state` (parse.y / subst.c): how far into a `${ … }` the scan has
/// got. Only [`DolBrace::Param`] — still inside the parameter name — enables
/// the subscript skip, which is why `${#[}` and `${x:-[}` are unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DolBrace {
    Param,
    Op,
    Word,
    Quote,
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

/// The string bash names in ``bad substitution: no closing `}' in %s`` when the
/// extent pass over `word` meets a `${` it cannot close, or `None` when it
/// closes every one.
///
/// `word` is the word's *source* — what the parser read, quotes included —
/// because that is the string `expand_word_internal` is handed.
pub fn unclosed_brace(word: BStr<'_>) -> Option<Str> {
    // No `${` in the word, no brace to leave unclosed. This is the fast path
    // for the overwhelming majority of words.
    word.find(b"${")?;
    scan(word, false, 0).err()
}

/// `expand_word_internal`'s walk over `s`, far enough to find the constructs
/// that consume text. `quoted` is bash's `Q_DOUBLE_QUOTES`: set when `s` is a
/// double-quoted run's contents rather than a word.
///
/// The error carries the string to name, which is `s` itself — every scanner
/// reached from here is handed `s`, so a fault at any depth below names it.
fn scan(s: BStr<'_>, quoted: bool, d: u32) -> Result<(), Str> {
    if d > MAX_DEPTH {
        return Ok(());
    }
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            // A single quote is only a quote outside double quotes.
            b'\'' if !quoted => i = skip_single(s, i + 1),
            b'`' => i = skip_backquote(s, i + 1),
            b'"' => {
                let (run, next) = dquote_run(s, i + 1, d + 1).map_err(|()| s.to_vec())?;
                // `expand_word_internal` re-enters itself on the run's
                // contents, and a fault found there names *them*.
                scan(&run, true, d + 1)?;
                i = next;
            }
            b'$' => match s.get(i + 1) {
                Some(b'{') => i = brace(s, i, quoted, d + 1).map_err(|()| s.to_vec())?,
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
        if !backquote && c == b'\\' {
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
    Ok((temp, i.saturating_add(usize::from(i < s.len())).min(s.len())))
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
        && s.get(name + 1).is_some_and(|&c| c.is_ascii_alphabetic() || c == b'_')
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
    let state = if quoted && posixexp { DolBrace::Quote } else { DolBrace::Word };
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
            // `string_extract (string, &si, "`", …)`: to the next backtick.
            i = past(skip_to(s, i + 1, b'`'), s);
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
        state = match state {
            DolBrace::Param if started && matches!(c, b'%' | b'#' | b'^' | b',') => DolBrace::Quote,
            DolBrace::Param if started && c == b'/' => DolBrace::Quote,
            DolBrace::Param if OPS.contains(&c) => DolBrace::Op,
            DolBrace::Op if !OPS.contains(&c) => DolBrace::Word,
            other => other,
        };
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
        if !backquote && c == b'\\' {
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

/// The index just past the backtick that closes a command substitution, where
/// a `\`` does not close it.
fn skip_backquote(s: BStr<'_>, start: usize) -> usize {
    let mut i = start;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            b'`' => return i + 1,
            _ => i += 1,
        }
    }
    s.len()
}

/// The index of the first `ch` at or after `start`, or `s.len()`.
fn skip_to(s: BStr<'_>, start: usize, ch: u8) -> usize {
    s.get(start..)
        .and_then(|t| t.find_byte(ch))
        .map_or(s.len(), |off| start + off)
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
        if !backquote && c == b'\\' {
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
    use super::unclosed_brace;

    fn named(src: &str) -> Option<String> {
        unclosed_brace(src.as_bytes()).map(|s| String::from_utf8(s).unwrap())
    }

    #[test]
    fn a_subscript_with_no_close_runs_off_the_end_of_the_word() {
        // The extent scan skips the subscript, not the `}` it was written
        // before — so the `${` is never closed and the *word* is named.
        assert_eq!(named(r#""${a[}""#).as_deref(), Some(r#""${a[}""#));
        assert_eq!(named(r#""${a[}"tail"#).as_deref(), Some(r#""${a[}"tail"#));
        assert_eq!(named(r#""pre${a[}post""#).as_deref(), Some(r#""pre${a[}post""#));
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
        assert_eq!(named(r#"${x:-"${a[}"}"#).as_deref(), Some(r#"${x:-"${a[}"}"#));
    }

    #[test]
    fn a_dollar_bracket_is_not_a_construct_the_quote_scanner_knows() {
        // `string_extract_double_quoted` knows `${`, `$(` and a backtick — not
        // `$[` — so the `${` inside the arithmetic is met and never closed.
        assert_eq!(named(r#""$[ ${ ]""#).as_deref(), Some(r#""$[ ${ ]""#));
        assert_eq!(named(r#""x$[ ${x:- ]y""#).as_deref(), Some(r#""x$[ ${x:- ]y""#));
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
        let deep = unclosed_brace(src.as_bytes());
        // Reaching this line *is* the assertion — a stack overflow aborts the
        // process rather than failing a test. Which answer comes back past the
        // cap is deliberately unpinned: this is far past any word bash would
        // still be scanning (it caps `${ … }` nesting at 32), so there is no
        // measurement to be faithful to.
        assert!(deep.is_none_or(|named| named.len() <= src.len()));
    }
}
